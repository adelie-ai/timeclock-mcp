use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

/// Global mutex to serialize tests that touch the filesystem/env.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that:
/// 1. Acquires the global test mutex (serializing all storage tests).
/// 2. Creates a fresh temporary directory for data.
/// 3. Points `TIMECLOCK_DATA_DIR` at it.
/// 4. Restores the environment on drop.
pub struct TestEnv {
    _dir: TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl TestEnv {
    pub fn new() -> Self {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("failed to create temp dir");
        // Safety: single-threaded at this point due to mutex.
        unsafe {
            std::env::set_var("TIMECLOCK_DATA_DIR", dir.path());
        }
        Self { _dir: dir, _guard: guard }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("TIMECLOCK_DATA_DIR");
        }
    }
}
