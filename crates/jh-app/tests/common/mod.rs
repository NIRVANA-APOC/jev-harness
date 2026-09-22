//! 测试共用的小工具：一次性临时目录。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 测试用的临时目录：创建时随机后缀，Drop 时清理。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 建一个新的临时目录。
    pub fn new(name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("jh-app-{name}-{}-{}", std::process::id(), unique));
        std::fs::create_dir_all(&path).expect("临时目录应能创建");
        Self { path }
    }

    /// 目录路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 目录下的一个路径。
    pub fn join(&self, file: &str) -> PathBuf {
        self.path.join(file)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).ok();
    }
}
