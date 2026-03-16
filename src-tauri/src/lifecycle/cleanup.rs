use std::fs;
use std::path::PathBuf;

pub struct TaskGuard {
    temp_dir: PathBuf,
}

impl TaskGuard {
    pub fn new(temp_dir: PathBuf) -> Result<Self, String> {
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)
                .map_err(|e| format!("remove stale temp dir failed: {e}"))?;
        }

        fs::create_dir_all(&temp_dir).map_err(|e| format!("create temp dir failed: {e}"))?;

        Ok(Self { temp_dir })
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

pub fn run_with_task_guard<T, F>(temp_dir: PathBuf, task: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let _guard = TaskGuard::new(temp_dir)?;
    task()
}
