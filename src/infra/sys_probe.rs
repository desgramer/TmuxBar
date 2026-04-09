use std::mem;
use std::ffi::CStr;
use std::sync::Mutex;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::models::{ProcStats, SystemProbe};

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Call `sysctlbyname` and return the result as an `i32`.
fn sysctl_by_name(name: &CStr) -> anyhow::Result<i32> {
    let mut value: i32 = 0;
    let mut size = mem::size_of::<i32>();
    let ret = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut value as *mut i32 as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        anyhow::bail!(
            "sysctlbyname({}) failed: {}",
            name.to_str().unwrap_or("?"),
            std::io::Error::last_os_error()
        );
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// MacSysProbe
// ---------------------------------------------------------------------------

/// macOS implementation of [`SystemProbe`].
///
/// Holds a [`sysinfo::System`] instance behind a `Mutex` so that CPU% delta
/// calculations are correct across successive calls to [`process_stats`].
pub struct MacSysProbe {
    system: Mutex<System>,
}

impl MacSysProbe {
    pub fn new() -> Self {
        Self {
            system: Mutex::new(System::new()),
        }
    }
}

impl Default for MacSysProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe for MacSysProbe {
    /// Returns `(current_fds, max_fds)` by reading macOS sysctl keys:
    /// - `kern.num_files`  — current number of open file descriptors system-wide
    /// - `kern.maxfiles`   — system-wide hard limit on open file descriptors
    fn fd_usage(&self) -> anyhow::Result<(u64, u64)> {
        let current = sysctl_by_name(c"kern.num_files")?;
        let max = sysctl_by_name(c"kern.maxfiles")?;
        Ok((current as u64, max as u64))
    }

    /// Returns CPU usage and RSS memory for the given `pid`.
    ///
    /// CPU% is a delta relative to the previous refresh stored in `self.system`,
    /// which is why we hold the `System` instance for reuse.
    fn process_stats(&self, pid: u32) -> anyhow::Result<ProcStats> {
        let sysinfo_pid = Pid::from(pid as usize);
        let mut sys = self.system.lock().expect("sysinfo mutex poisoned");

        // Refresh only the target process for efficiency.
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[sysinfo_pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );

        let process = sys
            .process(sysinfo_pid)
            .ok_or_else(|| anyhow::anyhow!("process with pid {} not found", pid))?;

        Ok(ProcStats {
            cpu_percent: process.cpu_usage(),
            memory_bytes: process.memory(),
        })
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fd_usage_returns_valid_values() {
        let probe = MacSysProbe::new();
        let (current, max) = probe.fd_usage().expect("fd_usage should succeed");
        assert!(current > 0, "current fd count should be > 0, got {current}");
        assert!(max > 0, "max fd count should be > 0, got {max}");
        assert!(
            current <= max,
            "current ({current}) should be <= max ({max})"
        );
    }

    #[test]
    fn test_process_stats_current_process() {
        let probe = MacSysProbe::new();
        let pid = std::process::id();
        let stats = probe
            .process_stats(pid)
            .expect("process_stats for current process should succeed");
        assert!(
            stats.cpu_percent >= 0.0,
            "cpu_percent should be >= 0.0, got {}",
            stats.cpu_percent
        );
        assert!(
            stats.memory_bytes > 0,
            "memory_bytes should be > 0, got {}",
            stats.memory_bytes
        );
    }

    #[test]
    fn test_process_stats_invalid_pid() {
        let probe = MacSysProbe::new();
        // PID 0 is not a user process; PID 999999999 almost certainly doesn't exist.
        let result = probe.process_stats(999_999_999);
        assert!(result.is_err(), "expected error for non-existent PID");
    }
}
