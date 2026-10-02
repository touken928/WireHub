use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt}, path::Path};
#[cfg(test)]
use boringtun::x25519::{PublicKey, StaticSecret};
use rand::{rngs::OsRng, RngCore};
use crate::storage::Store;

#[cfg(test)]
fn load_or_create_hub_key(path:&Path)->std::io::Result<[u8;32]> {
    match fs::symlink_metadata(path) {
        Ok(_)=>load_hub_key(path),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{
            let mut key=[0u8;32];OsRng.fill_bytes(&mut key);
            publish_hub_key(path,&key)
        }
        Err(e)=>Err(e),
    }
}
#[cfg(test)]
fn load_hub_key(path:&Path)->std::io::Result<[u8;32]> {
    load_hub_key_inner(path,false)
}
fn load_hub_key_synced(path:&Path)->std::io::Result<[u8;32]> {
    load_hub_key_inner(path,true)
}
fn load_hub_key_inner(path:&Path,sync:bool)->std::io::Result<[u8;32]> {
    let meta=fs::symlink_metadata(path)?;
    if !meta.file_type().is_file(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key must be a regular, non-symlink file"))}
    if meta.permissions().mode()&0o777!=0o600{return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"existing hub key must have mode 0600"))}
    let file=File::open(path)?;
    let opened=file.metadata()?;
    if meta.dev()!=opened.dev()||meta.ino()!=opened.ino(){return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"hub key changed while opening"))}
    if opened.permissions().mode()&0o777!=0o600{return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied,"existing hub key must have mode 0600"))}
    let mut bytes=Vec::new();(&file).take(33).read_to_end(&mut bytes)?;if bytes.len()!=32{return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"existing hub key must be exactly 32 bytes; refusing to rotate"))}if sync { file.sync_all()?; } Ok(bytes.try_into().unwrap())
}
pub(crate) fn load_or_bind_hub_key(store:&Store,path:&Path)->std::io::Result<[u8;32]> {
    store.bootstrap_hub_identity(||match fs::symlink_metadata(path){Ok(_)=>load_hub_key_synced(path),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{let mut key=[0;32];OsRng.fill_bytes(&mut key);Ok(key)},Err(e)=>Err(e)},|_|load_hub_key_synced(path),|key|publish_hub_key(path,key))
        .map_err(|e|std::io::Error::new(std::io::ErrorKind::InvalidData,e.to_string()))
}

fn publish_hub_key(path:&Path,key:&[u8;32])->std::io::Result<[u8;32]> {
    publish_hub_key_with(path,key,|_|Ok(()))
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum PublishCheckpoint { TempSynced, Linked, CollisionWinnerSynced }
fn publish_hub_key_with<F>(path:&Path,key:&[u8;32],mut checkpoint:F)->std::io::Result<[u8;32]>
where F:FnMut(PublishCheckpoint)->std::io::Result<()> {
    let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or_else(||Path::new("."));
    let name=path.file_name().ok_or_else(||std::io::Error::new(std::io::ErrorKind::InvalidInput,"hub key path has no filename"))?;
    let mut options=OpenOptions::new();options.write(true).create_new(true).mode(0o600);
    let (mut file,temp_path)=loop {let candidate=parent.join(format!(".{}.{}.tmp",name.to_string_lossy(),hex::encode(rand::random::<[u8;8]>())));match options.open(&candidate){Ok(f)=>break(f,candidate),Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(e)}};
    let result=(||{file.write_all(key)?;file.sync_all()?;checkpoint(PublishCheckpoint::TempSynced)?;
        drop(file);
        let published=match fs::hard_link(&temp_path,path){Ok(())=>{checkpoint(PublishCheckpoint::Linked)?;*key},Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>{let winner=load_hub_key_synced(path)?;checkpoint(PublishCheckpoint::CollisionWinnerSynced)?;winner},Err(e)=>return Err(e)};
        fs::remove_file(&temp_path)?;
        File::open(parent)?.sync_all()?;
        Ok(published)})();
    if result.is_err(){let _=fs::remove_file(&temp_path);} result
}

#[cfg(test)]
mod security_tests {
    use super::*;
    fn secure_test_file(path: &Path, bytes: &[u8]) {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        options.open(path).expect("test fixture creation failed").write_all(bytes).expect("test fixture write failed");
    }
    fn replace_secure_test_file(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new().write(true).truncate(true).open(path).expect("secure test fixture open failed");
        file.write_all(bytes).expect("test fixture write failed");
    }
    #[test]
    fn hub_key_is_exclusive_persistent_and_rejects_malformed_existing_file() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("hub.key");
        let first=load_or_create_hub_key(&path).unwrap();assert_eq!(first.len(),32);
        assert_eq!(load_or_create_hub_key(&path).unwrap(),first);
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode()&0o777,0o600);
        replace_secure_test_file(&path,b"bad");
        assert_eq!(fs::metadata(&path).unwrap().len(),3,"malformed fixture must replace rather than overwrite the key prefix");
        assert_eq!(load_or_create_hub_key(&path).unwrap_err().kind(),std::io::ErrorKind::InvalidData);
        assert_eq!(fs::read(&path).unwrap(),b"bad");
    }
    #[test]
    fn existing_symlink_and_insecure_key_are_rejected() {
        use std::os::unix::fs::symlink;
        let dir=tempfile::tempdir().unwrap();let key=dir.path().join("real");fs::write(&key,[3u8;32]).unwrap();fs::set_permissions(&key,fs::Permissions::from_mode(0o600)).unwrap();
        let link=dir.path().join("link");symlink(&key,&link).unwrap();assert!(load_or_create_hub_key(&link).is_err());
        fs::set_permissions(&key,fs::Permissions::from_mode(0o644)).unwrap();assert!(load_or_create_hub_key(&key).is_err());
    }
    #[test]
    fn startup_identity_restart_and_uncommitted_key_recovery_are_paired() {
        let dir=tempfile::tempdir().unwrap();let db=dir.path().join("db");let key=dir.path().join("hub.key");
        let store=Store::open(db.to_str().unwrap()).unwrap();let first=load_or_bind_hub_key(&store,&key).unwrap();assert_eq!(store.hub_identity().unwrap().unwrap(),*PublicKey::from(&StaticSecret::from(first)).as_bytes());
        assert_eq!(load_or_bind_hub_key(&store,&key).unwrap(),first);fs::remove_file(&key).unwrap();assert!(load_or_bind_hub_key(&store,&key).is_err());assert!(!key.exists());
        let interrupted=dir.path().join("interrupted.sqlite");let interrupted_key=dir.path().join("interrupted.key");let orphan=load_or_create_hub_key(&interrupted_key).unwrap();let unbound=Store::open(interrupted.to_str().unwrap()).unwrap();assert_eq!(load_or_bind_hub_key(&unbound,&interrupted_key).unwrap(),orphan);assert!(unbound.hub_identity().unwrap().is_some());
    }
    #[test]
    fn key_reader_rejects_oversized_file_after_short_reads() {
        let dir=tempfile::tempdir().unwrap(); let path=dir.path().join("oversized");
        secure_test_file(&path,&[9u8;33]);
        let error=load_hub_key(&path).unwrap_err();
        assert_eq!(error.kind(),std::io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(),"existing hub key must be exactly 32 bytes; refusing to rotate");
    }
    #[test]
    fn relative_key_path_can_be_published() {
        let path=Path::new("wirehub-phase1-relative-test.key");
        let _=fs::remove_file(path);
        assert!(load_or_create_hub_key(path).is_ok());
        let _=fs::remove_file(path);
    }
    #[test]
    fn missing_key_for_bound_unconfigured_database_does_not_rotate_identity() {
        let dir=tempfile::tempdir().unwrap(); let db=dir.path().join("db"); let key=dir.path().join("key");
        let store=Store::open(db.to_str().unwrap()).unwrap();
        store.bind_hub_identity(&[4;32]).unwrap();
        assert!(load_or_bind_hub_key(&store,&key).is_err());
        assert!(!key.exists()); assert_eq!(store.hub_identity().unwrap(),Some([4;32]));
    }
    #[test]
    fn bootstrap_publication_failure_boundaries_preserve_recoverable_pairing() {
        let dir=tempfile::tempdir().unwrap();let db=dir.path().join("boundary.sqlite");let path=dir.path().join("hub.key");
        let store=Store::open(db.to_str().unwrap()).unwrap();let proposed=[31u8;32];
        let mut before_stages=Vec::new();let before=store.bootstrap_hub_identity(||Ok(proposed),|_|unreachable!(),|key|publish_hub_key_with(&path,key,|stage|{before_stages.push(stage);if stage==PublishCheckpoint::TempSynced{Err(std::io::Error::other("before link"))}else{Ok(())}}));
        assert!(before.is_err());assert_eq!(before_stages,vec![PublishCheckpoint::TempSynced]);assert!(!path.exists());assert_eq!(store.hub_identity().unwrap(),None);assert_eq!(fs::read_dir(dir.path()).unwrap().count(),1);
        let mut after_stages=Vec::new();let after=store.bootstrap_hub_identity(||Ok(proposed),|_|unreachable!(),|key|publish_hub_key_with(&path,key,|stage|{after_stages.push(stage);if stage==PublishCheckpoint::Linked{Err(std::io::Error::other("after link"))}else{Ok(())}}));
        assert!(after.is_err());assert_eq!(after_stages,vec![PublishCheckpoint::TempSynced,PublishCheckpoint::Linked]);assert_eq!(fs::read(&path).unwrap(),proposed);assert_eq!(store.hub_identity().unwrap(),None);assert_eq!(fs::read_dir(dir.path()).unwrap().count(),2);
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode()&0o777,0o600);
        drop(store);let restarted=Store::open(db.to_str().unwrap()).unwrap();
        assert_eq!(load_or_bind_hub_key(&restarted,&path).unwrap(),proposed);assert!(restarted.hub_identity().unwrap().is_some());
    }
    #[test]
    fn key_publication_collision_keeps_complete_winner_without_overwrite() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("winner.key");let winner=[51u8;32];let contender=[52u8;32];
        secure_test_file(&path,&winner);
        let mut stages=Vec::new();assert_eq!(publish_hub_key_with(&path,&contender,|stage|{stages.push(stage);Ok(())}).unwrap(),winner);assert_eq!(fs::read(&path).unwrap(),winner);assert_eq!(stages,vec![PublishCheckpoint::TempSynced,PublishCheckpoint::CollisionWinnerSynced]);
    }
}
