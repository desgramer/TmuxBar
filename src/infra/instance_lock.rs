use anyhow::{bail, Context, Result};
use std::fs::File;
use std::os::unix::io::AsRawFd;

// ---------------------------------------------------------------------------
// InstanceLock
// ---------------------------------------------------------------------------

/// Holds an exclusive file lock for the duration of the process.
///
/// The lock is acquired via `libc::flock(LOCK_EX | LOCK_NB)` on
/// `~/.local/share/tmuxbar/tmuxbar.lock`.  Dropping this struct releases the
/// lock (the kernel releases `flock` locks when the last file descriptor to the
/// lock file is closed).
#[derive(Debug)]
pub struct InstanceLock {
    // Keep the file open so the OS-level flock is held.
    _file: File,
}

impl InstanceLock {
    /// Try to acquire an exclusive instance lock.
    ///
    /// Returns `Ok(InstanceLock)` if the lock was acquired.
    /// Returns an error if another TmuxBar process already holds the lock or if
    /// the lock file cannot be created.
    pub fn acquire() -> Result<Self> {
        let lock_path = Self::lock_path().context("cannot determine lock file path")?;

        // Ensure parent directory exists.
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create lock directory {}", parent.display()))?;
        }

        // Open (or create) the lock file.
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;

        // Try to acquire a non-blocking exclusive lock.
        let fd = file.as_raw_fd();
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            let raw = err.raw_os_error().unwrap_or(0);
            if raw == libc::EWOULDBLOCK {
                bail!("Another TmuxBar instance is already running");
            }
            return Err(err).context("flock failed unexpectedly");
        }

        Ok(Self { _file: file })
    }

    /// Returns the canonical path to the lock file:
    /// `~/.local/share/tmuxbar/tmuxbar.lock`
    fn lock_path() -> Option<std::path::PathBuf> {
        let home = dirs::home_dir()?;
        Some(
            home.join(".local")
                .join("share")
                .join("tmuxbar")
                .join("tmuxbar.lock"),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::io::AsRawFd;

    // Helper: acquire a lock on a specific path (used to test with temp dirs).
    fn acquire_on(path: &std::path::Path) -> Result<InstanceLock> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        let fd = file.as_raw_fd();
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            let raw = err.raw_os_error().unwrap_or(0);
            if raw == libc::EWOULDBLOCK {
                bail!("Another TmuxBar instance is already running");
            }
            return Err(err).context("flock failed unexpectedly");
        }
        Ok(InstanceLock { _file: file })
    }

    #[test]
    fn test_acquire_succeeds_first_call() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let lock_path = tmp.path().join("tmuxbar.lock");
        let result = acquire_on(&lock_path);
        assert!(result.is_ok(), "first acquire should succeed: {result:?}");
    }

    #[test]
    fn test_second_acquire_fails_while_first_held() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let lock_path = tmp.path().join("tmuxbar.lock");

        let _first = acquire_on(&lock_path).expect("first acquire should succeed");
        let second = acquire_on(&lock_path);
        assert!(
            second.is_err(),
            "second acquire should fail while first is held"
        );
        let msg = format!("{:#}", second.unwrap_err());
        assert!(
            msg.contains("Another TmuxBar instance is already running"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn test_lock_released_on_drop() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let lock_path = tmp.path().join("tmuxbar.lock");

        {
            let _first = acquire_on(&lock_path).expect("first acquire should succeed");
            // _first goes out of scope here, dropping the File and releasing the lock.
        }

        // After drop, acquiring again should succeed.
        let second = acquire_on(&lock_path);
        assert!(
            second.is_ok(),
            "acquire after drop should succeed: {second:?}"
        );
    }

    #[test]
    fn test_check_tmux_available_invalid_path_returns_false() {
        // Simulate check_tmux_available with a non-existent binary path.
        let result = std::process::Command::new("/nonexistent/path/to/tmux")
            .arg("-V")
            .output();
        assert!(
            result.is_err(),
            "running invalid path should fail at the OS level"
        );
    }
}
