use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub(super) struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub(super) fn acquire(path: &Path) -> io::Result<(PathBuf, Self)> {
        if path.as_os_str().is_empty() {
            return Err(input("database path must not be empty"));
        }
        let raw = path.to_string_lossy();
        if raw == ":memory:" || raw.starts_with("file:") {
            return Err(input("memory and SQLite URI database paths are unsupported"));
        }

        let db_path = normalize(path)?;
        validate_db_path(&db_path)?;
        let lock_path = append_suffix(&db_path, ".wirehub.lock");
        let file = open_lock(&lock_path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "another WireHub service instance already owns this database",
                ));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }

        // The data directory is trusted and must remain stable while WireHub runs.
        let after = normalize(path)?;
        if after != db_path {
            return Err(input("database path changed while acquiring its instance lock"));
        }
        validate_db_path(&after)?;
        validate_lock_path(&lock_path, &file, None)?;
        Ok((db_path, Self { _file: file }))
    }
}

fn input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn normalize(path: &Path) -> io::Result<PathBuf> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::canonicalize(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let name = path.file_name().ok_or_else(|| input("database path must name a file"))?;
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            Ok(std::fs::canonicalize(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

fn validate_db_path(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() || metadata.nlink() != 1 {
                return Err(input("database must be a regular file with exactly one hard link"));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn open_lock(path: &Path) -> io::Result<File> {
    let existing = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_lock_metadata(&metadata)?;
            Some(metadata)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600);
    let file = options.open(path)?;
    validate_lock_path(path, &file, existing.as_ref())?;
    Ok(file)
}

fn validate_lock_metadata(metadata: &std::fs::Metadata) -> io::Result<()> {
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(input("instance lock must be a regular file with exactly one hard link"));
    }
    Ok(())
}

fn validate_lock_path(
    path: &Path,
    file: &File,
    existing: Option<&std::fs::Metadata>,
) -> io::Result<()> {
    let path_meta = std::fs::symlink_metadata(path)?;
    let opened = file.metadata()?;
    validate_lock_metadata(&path_meta)?;
    if path_meta.dev() != opened.dev()
        || path_meta.ino() != opened.ino()
        || existing.is_some_and(|meta| meta.dev() != opened.dev() || meta.ino() != opened.ino())
    {
        return Err(input("instance lock path does not identify the opened regular file"));
    }
    Ok(())
}
