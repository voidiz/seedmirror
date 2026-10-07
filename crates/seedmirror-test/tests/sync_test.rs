use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use seedmirror_test::{
    path::{assert_dst_contains_src, copy_recursive},
    process::ProcessGuard,
};

const SHARED_TEST_DIR: &str = "seedmirror-test";

#[test]
fn test_full_sync() -> anyhow::Result<()> {
    let test_dir = TestDir::from("sync_test")?;

    // Empty component to add a trailing slash to ensure that the directories are synced, instead
    // of placing the directory named "source" inside target
    let src = test_dir.path.join("source").join("");
    let dst = test_dir.path.join("target").join("");

    let expected_dst = test_dir.path.join("expected_target");
    let socket_path = test_dir.path.join("seedmirror-server.sock");

    let _ = Command::new("cargo")
        .current_dir(&test_dir.workspace_dir)
        .arg("build")
        .arg("--release")
        .status()?;

    let _server = ProcessGuard::spawn(
        Command::new("target/release/seedmirror-server")
            .current_dir(&test_dir.workspace_dir)
            .arg("--socket-path")
            .arg(&socket_path)
            .arg("--sync-delay")
            .arg("100"),
    )?;

    // This will trigger a full sync
    let _client = ProcessGuard::spawn(
        Command::new("target/release/seedmirror-client")
            .current_dir(&test_dir.workspace_dir)
            .arg("--socket-path")
            .arg(&socket_path)
            .arg("--ssh-hostname")
            .arg("localhost")
            .arg("-p")
            .arg(format!(
                "{}:{}",
                src.to_string_lossy(),
                dst.to_string_lossy()
            )),
    )?;

    // Wait for the initial sync
    wait_until(Duration::from_secs(10), || {
        initial_files_present(&expected_dst, &dst)
    })?;

    // This will trigger sync for a single file
    fs::write(src.join("new_file.txt"), "")?;

    // The sync delay is set to 100ms, so a short sleep is fine
    thread::sleep(Duration::new(1, 0));
    assert_dst_contains_src(&expected_dst, &dst)?;

    Ok(())
}

/// Poll `cond` until it holds or `timeout` expires.
fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> anyhow::Result<()> {
    let start = Instant::now();
    while !cond() {
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for expected synced state");
        }

        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

/// True if everything in `expected` except `new_file.txt` (which doesn't
/// exist on the source side yet) is present in `dst`, recursively.
fn initial_files_present(expected: &Path, dst: &Path) -> bool {
    let Ok(entries) = fs::read_dir(expected) else {
        return false;
    };

    entries.flatten().all(|entry| {
        if entry.file_name() == "new_file.txt" {
            return true;
        }

        let src_path = entry.path();
        let Ok(rel) = src_path.strip_prefix(expected) else {
            return false;
        };

        let dst_path = dst.join(rel);
        if !dst_path.exists() {
            return false;
        }

        if src_path.is_dir() {
            return initial_files_present(&src_path, &dst_path);
        }

        true
    })
}

struct TestDir {
    pub workspace_dir: PathBuf,
    pub path: PathBuf,
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl TestDir {
    pub fn from(test_files_dir: &str) -> anyhow::Result<Self> {
        let mut test_dir = env::temp_dir();
        test_dir.push(format!("{}-{}", SHARED_TEST_DIR, test_files_dir));

        // file!() is relative to workspace, but CARGO_MANIFEST_DIR is the directory of the crate-level
        // cargo manifest
        let workspace_dir =
            Path::new(&format!("{}/../..", env!("CARGO_MANIFEST_DIR"))).canonicalize()?;

        let mut test_file_path = workspace_dir
            .join(file!())
            .parent()
            .with_context(|| "invalid file path")?
            .to_owned();

        test_file_path.push("test_files");
        test_file_path.push(test_files_dir);

        copy_recursive(&test_file_path, &test_dir)?;

        Ok(Self {
            workspace_dir,
            path: test_dir,
        })
    }
}
