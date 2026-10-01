//! Windows handle-based access to the hub private key.
//! Existing files are never repaired: callers must explicitly provision secure files.
use std::{ffi::c_void, fs::File, io, os::windows::io::FromRawHandle, path::Path};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, INVALID_HANDLE_VALUE, GENERIC_READ, GENERIC_WRITE},
    Security::{ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, EqualSid,
        GetAce, GetAclInformation, GetLengthSid, GetSecurityDescriptorDacl, GetTokenInformation,
        IsValidSid, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
        PROTECTED_DACL_SECURITY_INFORMATION},
    Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
    Storage::FileSystem::{CreateFileW, GetFileInformationByHandle, GetFileType, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_TYPE_DISK, CREATE_NEW, OPEN_EXISTING, FILE_GENERIC_READ, FILE_GENERIC_WRITE},
    System::{SystemServices::ACCESS_ALLOWED_ACE_TYPE, Threading::{GetCurrentProcess, OpenProcessToken}},
};

struct Handle(HANDLE);
impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
impl Handle { fn into_raw(self) -> HANDLE { let raw = self.0; std::mem::forget(self); raw } }
struct LocalPtr(*mut c_void);
impl Drop for LocalPtr { fn drop(&mut self) { if !self.0.is_null() { unsafe { LocalFree(self.0 as _); } } } }
fn winerr() -> io::Error { io::Error::from_raw_os_error(unsafe { GetLastError() } as i32) }
fn wide(s: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let mut out: Vec<u16> = s.encode_wide().collect();
    if out.contains(&0) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "Windows path contains NUL")); }
    out.push(0); Ok(out)
}
fn fail_invalid(msg: &'static str) -> io::Error { io::Error::new(io::ErrorKind::PermissionDenied, msg) }

// Keep token backing storage alive and aligned for all SID API calls.
fn current_user_sid() -> io::Result<(Handle, Vec<usize>, *mut c_void)> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 { return Err(winerr()); }
        let token = Handle(token);
        let mut len = 0;
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len);
        if len == 0 { return Err(winerr()); }
        let mut data = vec![0usize; (len as usize + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>()];
        if GetTokenInformation(token.0, TokenUser, data.as_mut_ptr().cast(), len, &mut len) == 0 { return Err(winerr()); }
        let user = &*(data.as_ptr() as *const TOKEN_USER);
        let sid = user.User.Sid;
        if sid.is_null() || IsValidSid(sid) == 0 { return Err(fail_invalid("current process token has invalid user SID")); }
        Ok((token, data, sid))
    }
}
fn system_sid() -> io::Result<LocalPtr> {
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    let s: Vec<u16> = "S-1-5-18".encode_utf16().chain(Some(0)).collect();
    let mut sid = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(s.as_ptr(), &mut sid) } == 0 { return Err(winerr()); }
    Ok(LocalPtr(sid))
}
fn sid_to_string(sid: *mut c_void) -> io::Result<Vec<u16>> {
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    let mut p = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut p) } == 0 { return Err(winerr()); }
    let p = LocalPtr(p.cast());
    let mut n = 0; unsafe { while *p.0.cast::<u16>().add(n) != 0 { n += 1; } }
    Ok(unsafe { std::slice::from_raw_parts(p.0.cast::<u16>(), n).to_vec() })
}

/// Atomically create a new key with an explicit current-user owner and protected user+SYSTEM DACL.
pub fn create_new(path: &Path) -> io::Result<File> {
    let (_token, _backing, user) = current_user_sid()?;
    let user_sid = String::from_utf16_lossy(&sid_to_string(user)?);
    // Explicit owner is essential when running elevated: otherwise the token's default owner may be Administrators.
    let sddl: Vec<u16> = format!("O:{user_sid}D:P(A;;FA;;;{user_sid})(A;;FA;;;SY)").encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut descriptor, std::ptr::null_mut()) } == 0 { return Err(winerr()); }
    let descriptor = LocalPtr(descriptor);
    let attrs = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: descriptor.0, bInheritHandle: 0 };
    let name = wide(path.as_os_str())?;
    let h = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ, &attrs, CREATE_NEW, 0, std::ptr::null_mut()) };
    if h == INVALID_HANDLE_VALUE { return Err(winerr()); }
    Ok(unsafe { File::from_raw_handle(h as _) })
}

/// Open and validate on the same handle, rejecting reparse points and broad/unknown ACLs.
/// The handle is deliberately opened read/write: startup sync_all() calls FlushFileBuffers on it.
pub fn open_secure(path: &Path) -> io::Result<File> {
    let name = wide(path.as_os_str())?;
    let h = unsafe { CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, std::ptr::null_mut()) };
    if h == INVALID_HANDLE_VALUE { return Err(winerr()); }
    let handle = Handle(h);
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileType(h) } != FILE_TYPE_DISK || unsafe { GetFileInformationByHandle(h, &mut info) } == 0 { return Err(fail_invalid("hub key is not a regular disk file")); }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 { return Err(fail_invalid("hub key reparse points are not allowed")); }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 { return Err(fail_invalid("hub key must be a regular file, not a directory")); }
    let (_token, _backing, user) = current_user_sid()?;
    let system = system_sid()?;
    let mut owner = std::ptr::null_mut(); let mut dacl = std::ptr::null_mut(); let mut sd = std::ptr::null_mut();
    let status = unsafe { GetSecurityInfo(h, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, &mut owner, std::ptr::null_mut(), &mut dacl, std::ptr::null_mut(), &mut sd) };
    if status != 0 { return Err(io::Error::from_raw_os_error(status as i32)); }
    let sd = LocalPtr(sd);
    if owner.is_null() || (unsafe { EqualSid(owner, user) } == 0 && unsafe { EqualSid(owner, system.0.cast()) } == 0) { return Err(fail_invalid("hub key owner must be current user or SYSTEM")); }
    let mut present = 0; let mut defaulted = 0;
    if unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) } == 0 || present == 0 || dacl.is_null() { return Err(fail_invalid("hub key must have a non-null DACL")); }
    let mut acl_info: ACL_SIZE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetAclInformation(dacl, (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(), std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32, 2) } == 0 { return Err(winerr()); }
    let mut user_read = false; let mut user_write = false;
    for i in 0..acl_info.AceCount {
        let mut raw = std::ptr::null_mut();
        if unsafe { GetAce(dacl, i, &mut raw) } == 0 { return Err(winerr()); }
        if raw.is_null() { return Err(fail_invalid("hub key DACL contains a malformed ACE")); }
        let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 || header.AceFlags != 0 || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
            return Err(fail_invalid("hub key DACL contains unsupported, inherited, or denying ACE"));
        }
        let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
        let sid = (&ace.SidStart as *const u32).cast_mut().cast();
        let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
        // A SID needs its 8-byte fixed header before GetLengthSid can safely inspect it.
        if (header.AceSize as usize) < sid_offset + 8 { return Err(fail_invalid("hub key DACL contains a truncated SID")); }
        let sid_size = unsafe { GetLengthSid(sid) } as usize;
        if sid_size == 0 || sid_offset + sid_size > header.AceSize as usize || unsafe { IsValidSid(sid) } == 0 {
            return Err(fail_invalid("hub key DACL contains a malformed SID"));
        }
        if unsafe { EqualSid(sid, user) } == 0 && unsafe { EqualSid(sid, system.0.cast()) } == 0 { return Err(fail_invalid("hub key DACL grants access to a non-user principal")); }
        if unsafe { EqualSid(sid, user) } != 0 {
            user_read |= ace.Mask & FILE_GENERIC_READ == FILE_GENERIC_READ;
            user_write |= ace.Mask & FILE_GENERIC_WRITE == FILE_GENERIC_WRITE;
        }
    }
    if !user_read || !user_write { return Err(fail_invalid("hub key DACL must grant current user read and write access")); }
    Ok(unsafe { File::from_raw_handle(handle.into_raw() as _) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::{Read, Write}, os::windows::io::AsRawHandle};
    use tempfile::tempdir;

    fn read_secure(path: &Path) -> io::Result<Vec<u8>> { let mut f = open_secure(path)?; let mut bytes = Vec::new(); f.read_to_end(&mut bytes)?; Ok(bytes) }
    fn broad_parent(path: &Path) {
        use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;
        let sddl: Vec<u16> = "D:(A;OICI;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
        let mut sd = std::ptr::null_mut(); assert_ne!(unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut sd, std::ptr::null_mut()) }, 0);
        let sd = LocalPtr(sd);
        let attrs = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: 0 };
        assert_ne!(unsafe { CreateDirectoryW(wide(path.as_os_str()).unwrap().as_ptr(), &attrs) }, 0);
    }
    fn set_dacl(path: &Path, sddl: Option<&str>) {
        use windows_sys::Win32::Storage::FileSystem::CreateFileW;
        use windows_sys::Win32::Storage::FileSystem::WRITE_DAC;
        let h = unsafe { CreateFileW(wide(path.as_os_str()).unwrap().as_ptr(), GENERIC_READ | GENERIC_WRITE | WRITE_DAC, FILE_SHARE_READ, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
        assert_ne!(h, INVALID_HANDLE_VALUE, "test setup could not open key file");
        let handle = Handle(h);
        let (sd, dacl) = if let Some(sddl) = sddl {
            let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
            let mut sd = std::ptr::null_mut();
            assert_ne!(unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), 1, &mut sd, std::ptr::null_mut()) }, 0, "test SDDL must parse");
            let sd = LocalPtr(sd);
            let mut present = 0; let mut defaulted = 0; let mut dacl = std::ptr::null_mut();
            assert_ne!(unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) }, 0);
            assert_ne!(present, 0);
            (Some(sd), dacl)
        } else { (None, std::ptr::null_mut()) };
        let flags = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
        assert_eq!(unsafe { SetSecurityInfo(handle.0, SE_FILE_OBJECT, flags, std::ptr::null_mut(), std::ptr::null_mut(), dacl, std::ptr::null_mut()) }, 0, "test ACL setup failed");
        drop(sd);
    }
    #[test]
    fn secure_create_is_exclusive_readwrite_and_flushable() {
        let dir = tempdir().unwrap(); let path = dir.path().join("new.key");
        let mut f = create_new(&path).unwrap(); f.write_all(&[7;32]).unwrap(); f.sync_all().unwrap(); drop(f);
        assert!(create_new(&path).is_err()); assert_eq!(read_secure(&path).unwrap(), [7;32]);
        open_secure(&path).unwrap().sync_all().unwrap();
        let f = open_secure(&path).unwrap();
        let (_token, _backing, user) = current_user_sid().unwrap();
        let mut owner = std::ptr::null_mut(); let mut sd = std::ptr::null_mut();
        assert_eq!(unsafe { GetSecurityInfo(f.as_raw_handle() as HANDLE, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd) }, 0);
        let _sd = LocalPtr(sd);
        assert!(!owner.is_null()); assert_ne!(unsafe { EqualSid(owner, user) }, 0, "new-file owner must be the current user, including elevated accounts");
    }
    #[test]
    fn rejects_broad_acl_without_repairing_existing_file_and_protects_creation_in_broad_parent() {
        let dir = tempdir().unwrap(); let parent = dir.path().join("permissive"); broad_parent(&parent);
        let insecure = parent.join("default.key"); fs::write(&insecure, [8;32]).unwrap();
        assert!(open_secure(&insecure).is_err()); assert_eq!(fs::read(&insecure).unwrap(), [8;32]);
        let secure = parent.join("protected.key"); let mut f = create_new(&secure).unwrap(); f.write_all(&[9;32]).unwrap(); drop(f);
        assert_eq!(read_secure(&secure).unwrap(), [9;32]);
    }
    #[test]
    fn rejects_null_dacl_and_deny_ace() {
        let dir = tempdir().unwrap();
        let null_path = dir.path().join("null.key");
        let mut f = create_new(&null_path).unwrap(); f.write_all(&[1;32]).unwrap(); drop(f);
        set_dacl(&null_path, None);
        assert!(open_secure(&null_path).is_err());

        let deny_path = dir.path().join("deny.key");
        let mut f = create_new(&deny_path).unwrap(); f.write_all(&[2;32]).unwrap(); drop(f);
        // Deny ACEs are unsupported even if the account has a later allow ACE.
        let (_token, _backing, user) = current_user_sid().unwrap();
        let user_sid = String::from_utf16_lossy(&sid_to_string(user).unwrap());
        set_dacl(&deny_path, Some(&format!("D:P(D;;FR;;;WD)(A;;FA;;;{user_sid})(A;;FA;;;SY)")));
        assert!(open_secure(&deny_path).is_err());
    }
    #[test]
    fn rejects_file_symlink_reparse_point_without_skipping() {
        use std::os::windows::fs::symlink_file;
        let dir = tempdir().unwrap(); let target = dir.path().join("target.key"); let link = dir.path().join("link.key");
        let mut file = create_new(&target).unwrap(); file.write_all(&[9;32]).unwrap(); drop(file);
        symlink_file(&target, &link).expect("Windows CI must permit file symlink creation; enable Developer Mode or run elevated");
        assert!(open_secure(&link).is_err()); assert_eq!(fs::read(&target).unwrap(), [9;32]);
    }
    #[test]
    fn rejects_nul_path_and_keeps_temp_handle_exclusive_for_publication_window() {
        use std::os::windows::ffi::OsStringExt;
        let os_path = std::ffi::OsString::from_wide(&[b'a' as u16, 0, b'b' as u16]);
        let path = Path::new(&os_path);
        assert_eq!(open_secure(path).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
