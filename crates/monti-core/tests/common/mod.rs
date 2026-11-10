//! Test helpers shared by every crate's integration tests (others include this file by #[path]).

use std::fs;
use std::path::PathBuf;

/// A fresh empty directory under the system temp dir, unique per test process and `name`.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("monti-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}
