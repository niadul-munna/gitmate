//! Filesystem helpers for GitMate-owned files.
//!
//! All files GitMate owns are written through [`atomic_write`]: a new file
//! is created exclusively (`O_CREAT|O_EXCL`, so a planted symlink is never
//! followed) next to the destination, synced, and renamed over it. Directories
//! are created with mode 0700 and files with 0600 on Unix.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

/// Create `dir` (and parents) if needed. Newly created directories get mode
/// 0700. An existing directory that is writable by group or others is
/// rejected, because another user could then replace GitMate's files.
pub fn ensure_private_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(dir)
            .map_err(|e| Error::io("could not create directory", dir, e))?;
    }
    let meta = fs::metadata(dir).map_err(|e| Error::io("could not inspect directory", dir, e))?;
    if !meta.is_dir() {
        return Err(Error::InvalidPath {
            path: dir.to_path_buf(),
            reason: "exists but is not a directory".into(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode();
        if mode & 0o022 != 0 {
            return Err(Error::InvalidPath {
                path: dir.to_path_buf(),
                reason: format!(
                    "is writable by other users (mode {:o}); run `chmod go-w` on it",
                    mode & 0o777
                ),
            });
        }
    }
    Ok(())
}

/// A unique temporary path next to `dest`.
fn temp_path_for(dest: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    dest.with_file_name(format!(".{name}.tmp-{}-{nanos:09}", std::process::id()))
}

/// Create a new file exclusively with mode 0600.
pub fn create_private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| Error::io("could not create file", path, e))
}

/// Atomically replace `dest` with `contents` (mode 0600). The parent
/// directory must already exist.
pub fn atomic_write(dest: &Path, contents: &[u8]) -> Result<()> {
    let tmp = temp_path_for(dest);
    let result = (|| {
        let mut file = create_private_file(&tmp)?;
        file.write_all(contents)
            .and_then(|_| file.sync_all())
            .map_err(|e| Error::io("could not write", &tmp, e))?;
        drop(file);
        fs::rename(&tmp, dest).map_err(|e| Error::io("could not replace", dest, e))?;
        sync_parent(dest);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Prepare a fresh temp path next to `dest`, let `fill` populate it, then
/// rename it over `dest`. Used when Git itself writes the content.
pub fn atomic_replace_with(dest: &Path, fill: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let tmp = temp_path_for(dest);
    let result = (|| {
        create_private_file(&tmp)?;
        fill(&tmp)?;
        // Windows only flushes a file opened for writing.
        OpenOptions::new()
            .write(true)
            .open(&tmp)
            .and_then(|f| f.sync_all())
            .map_err(|e| Error::io("could not sync", &tmp, e))?;
        fs::rename(&tmp, dest).map_err(|e| Error::io("could not replace", dest, e))?;
        sync_parent(dest);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Read a file that may not exist.
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io("could not read", path, e)),
    }
}

/// 64-bit FNV-1a. Stable across Rust versions, used only to notice that a
/// file changed, never for security.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Exclusive advisory lock serialising GitMate writers. Released on drop.
#[derive(Debug)]
pub struct WriterLock {
    _file: File,
}

impl WriterLock {
    /// Acquire the lock at `path`, waiting up to `timeout`.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<WriterLock> {
        if let Some(parent) = path.parent() {
            ensure_private_dir(parent)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|e| Error::io("could not open lock file", path, e))?;
        let deadline = Instant::now() + timeout;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(WriterLock { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(std::fs::TryLockError::WouldBlock) => return Err(Error::Locked),
                Err(std::fs::TryLockError::Error(e)) => {
                    return Err(Error::io("could not lock", path, e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_private_file_and_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("profiles.toml");
        atomic_write(&dest, b"one").unwrap();
        atomic_write(&dest, b"two").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"two");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // No temp files left behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_replaces_a_symlink_instead_of_following_it() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        fs::write(&victim, b"precious").unwrap();
        let dest = dir.path().join("profiles.toml");
        std::os::unix::fs::symlink(&victim, &dest).unwrap();
        atomic_write(&dest, b"new").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"precious");
        assert!(!fs::symlink_metadata(&dest).unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_is_0700_and_world_writable_dir_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("a/b");
        ensure_private_dir(&mine).unwrap();
        assert_eq!(fs::metadata(&mine).unwrap().permissions().mode() & 0o777, 0o700);

        let open = dir.path().join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            ensure_private_dir(&open),
            Err(Error::InvalidPath { .. })
        ));
    }

    #[test]
    fn atomic_replace_with_syncs_and_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("work.gitconfig");
        fs::write(&dest, b"old").unwrap();
        atomic_replace_with(&dest, |tmp| {
            fs::write(tmp, b"new").map_err(|e| Error::io("could not write", tmp, e))
        })
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn writer_lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/lock");
        let held = WriterLock::acquire(&path, Duration::from_millis(10)).unwrap();
        let second = WriterLock::acquire(&path, Duration::from_millis(50));
        assert!(matches!(second, Err(Error::Locked)));
        drop(held);
        WriterLock::acquire(&path, Duration::from_millis(10)).unwrap();
    }

    #[test]
    fn fnv_is_stable() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
