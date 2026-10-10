//! One app-data namespace lease survives staging, launch and child completion.
use super::{Error, Result, ARTIFACT_LIMIT};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) struct Namespace {
    root: PathBuf,
    _lease: File,
    _app_data: File,
    _directory: File,
}

pub(crate) struct Staged {
    // Close immutable handles before removing the owned directory and lease.
    file: Option<File>,
    directory_handle: Option<File>,
    directory: Option<tempfile::TempDir>,
    _namespace: Namespace,
}

fn plain(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::Install)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::Install);
        }
    }
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(Error::Install);
    }
    Ok(())
}

fn hold(path: &Path, directory: bool) -> Result<File> {
    plain(path, directory)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // No sharing of writes/deletion: both the executable and its parents
        // remain bound to these names until CreateProcess has opened the image.
        options.share_mode(if directory { 3 } else { 1 });
        options.custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 });
    }
    let file = options.open(path).map_err(|_| Error::Install)?;
    plain(path, directory)?;
    Ok(file)
}

fn private_directory(path: &Path, created: bool) -> Result<()> {
    #[cfg(windows)]
    {
        // Native per-user app data supplies the inherited user ACL. Integrity
        // relies on the lease, held image/directory handles and final verification.
        let _ = (path, created);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if created {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::Install)?;
        }
        if fs::metadata(path)
            .map_err(|_| Error::Install)?
            .permissions()
            .mode()
            & 0o777
            != 0o700
        {
            return Err(Error::Install);
        }
    }
    Ok(())
}

impl Namespace {
    pub(crate) fn acquire(app_data: &Path) -> Result<Self> {
        let app_data_handle = hold(app_data, true)?;
        let root = app_data.join("updates-v1");
        let created = match fs::create_dir(&root) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(Error::Install),
        };
        let directory = hold(&root, true)?;

        let path = root.join("namespace.lock");
        if path.try_exists().map_err(|_| Error::Install)? {
            plain(&path, false)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(3).custom_flags(0x00200000);
        }
        let lease = options.open(path).map_err(|_| Error::Install)?;
        lease.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            _ => Error::Install,
        })?;
        private_directory(&root, created)?;
        let namespace = Self {
            root,
            _lease: lease,
            _directory: directory,
            _app_data: app_data_handle,
        };
        namespace.clean_stale()?;
        Ok(namespace)
    }

    fn clean_stale(&self) -> Result<()> {
        let mut stale = Vec::new();
        for (index, entry) in fs::read_dir(&self.root)
            .map_err(|_| Error::Install)?
            .enumerate()
        {
            if index >= 256 {
                return Err(Error::Install);
            }
            let entry = entry.map_err(|_| Error::Install)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::Install)?;
            if name == "namespace.lock" {
                plain(&entry.path(), false)?;
                continue;
            }
            let suffix = name.strip_prefix("stage-v1-").ok_or(Error::Install)?;
            if suffix.len() != 12 || !suffix.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(Error::Install);
            }
            let path = entry.path();
            plain(&path, true)?;
            let entries = fs::read_dir(&path)
                .map_err(|_| Error::Install)?
                .take(2)
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|_| Error::Install)?;
            if entries.len() > 1
                || entries
                    .first()
                    .is_some_and(|e| e.file_name() != "installer.exe")
            {
                return Err(Error::Install);
            }
            if let Some(file) = entries.first() {
                plain(&file.path(), false)?;
            }
            stale.push((path, entries.first().map(|file| file.path())));
        }
        for (directory, file) in stale {
            if let Some(file) = file {
                fs::remove_file(file).map_err(|_| Error::Install)?;
            }
            fs::remove_dir(directory).map_err(|_| Error::Install)?;
        }
        Ok(())
    }

    pub(crate) fn stage(self, bytes: &[u8]) -> Result<(Staged, PathBuf)> {
        if !bytes.starts_with(b"MZ") {
            return Err(Error::Install);
        }
        let directory = tempfile::Builder::new()
            .prefix("stage-v1-")
            .rand_bytes(12)
            .tempdir_in(&self.root)
            .map_err(|_| Error::Install)?;
        let path = directory.path().join("installer.exe");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| Error::Install)?;
        file.write_all(bytes).map_err(|_| Error::Install)?;
        file.sync_all().map_err(|_| Error::Install)?;
        drop(file);
        let file = hold(&path, false)?;
        let directory_handle = hold(directory.path(), true)?;
        Ok((
            Staged {
                file: Some(file),
                directory_handle: Some(directory_handle),
                directory: Some(directory),
                _namespace: self,
            },
            path,
        ))
    }
}

impl Staged {
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        self.directory.as_ref().expect("owned staging").path()
    }
    pub(crate) fn read(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.file
            .as_ref()
            .ok_or(Error::Install)?
            .take(ARTIFACT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Install)?;
        if bytes.len() > ARTIFACT_LIMIT {
            return Err(Error::Size);
        }
        Ok(bytes)
    }
    pub(crate) fn close(mut self) -> Result<()> {
        self.file.take();
        self.directory_handle.take();
        self.directory
            .take()
            .ok_or(Error::Install)?
            .close()
            .map_err(|_| Error::Install)
    }
}
impl Drop for Staged {
    fn drop(&mut self) {
        self.file.take();
        self.directory_handle.take();
        self.directory.take();
    }
}
