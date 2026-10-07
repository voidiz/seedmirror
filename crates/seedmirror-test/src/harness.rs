use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;

use crate::{path::dir_contains, process::ProcessGuard};

pub enum Entry {
    /// An (empty) directory
    Dir,

    /// A zero-byte file
    Empty,

    /// A file with the given contents
    Bytes(Vec<u8>),

    /// A sparse, zero-filled file of the given length
    Zeros(u64),
}

impl Entry {
    pub fn dir() -> Self {
        Self::Dir
    }

    pub fn empty() -> Self {
        Self::Empty
    }

    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Bytes(bytes.into())
    }

    pub fn zeros(len: u64) -> Self {
        Self::Zeros(len)
    }

    fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("failed to create {parent:?}"))?;
        }

        match self {
            Self::Dir => {
                fs::create_dir_all(path).with_context(|| format!("failed to create {path:?}"))?;
            }
            Self::Empty => {
                fs::write(path, []).with_context(|| format!("failed to write {path:?}"))?;
            }
            Self::Bytes(bytes) => {
                fs::write(path, bytes).with_context(|| format!("failed to write {path:?}"))?;
            }
            Self::Zeros(len) => {
                fs::File::create(path)
                    .and_then(|file| file.set_len(*len))
                    .with_context(|| format!("failed to create {path:?}"))?;
            }
        }

        Ok(())
    }
}

pub struct Fixture {
    name: String,
    source: Vec<(PathBuf, Entry)>,
    target: Vec<(PathBuf, Entry)>,
}

impl Fixture {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: Vec::new(),
            target: Vec::new(),
        }
    }

    /// Add `entry` into the source directory at `path` (relative)
    pub fn source(mut self, path: impl Into<PathBuf>, entry: Entry) -> Self {
        self.source.push((path.into(), entry));
        self
    }

    /// Add `entry` into the target directory at `path` (relative)
    pub fn target(mut self, path: impl Into<PathBuf>, entry: Entry) -> Self {
        self.target.push((path.into(), entry));
        self
    }

    pub fn build(self) -> anyhow::Result<SyncHarness> {
        let workspace_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .with_context(|| "failed to resolve workspace dir")?;

        let mut root = env::temp_dir();
        root.push(format!("seedmirror-test-{}", self.name));
        if root.exists() {
            fs::remove_dir_all(&root).with_context(|| format!("failed to clear {root:?}"))?;
        }

        // Make sure source/ is synced with target/ instead of putting source/ inside target/
        let src = root.join("source").join("");
        let dst = root.join("target").join("");
        fs::create_dir_all(&src).with_context(|| format!("failed to create {src:?}"))?;
        fs::create_dir_all(&dst).with_context(|| format!("failed to create {dst:?}"))?;

        for (rel, entry) in &self.source {
            entry.write(&src.join(rel))?;
        }
        for (rel, entry) in &self.target {
            entry.write(&dst.join(rel))?;
        }

        let target_paths = self.target.iter().map(|(rel, _)| rel.clone()).collect();
        let socket_path = root.join("seedmirror-server.sock");

        Ok(SyncHarness {
            workspace_dir,
            tmp: TempDir(root),
            src,
            dst,
            socket_path,
            target_paths,
        })
    }
}

pub struct SyncHarness {
    pub workspace_dir: PathBuf,
    pub tmp: TempDir,
    pub src: PathBuf,
    pub dst: PathBuf,
    pub socket_path: PathBuf,
    target_paths: Vec<PathBuf>,
}

impl SyncHarness {
    pub fn build(&self, profile: &str) -> anyhow::Result<()> {
        let status = Command::new("cargo")
            .current_dir(&self.workspace_dir)
            .args(["build", "--profile", profile])
            .status()
            .with_context(|| "failed to run cargo build")?;

        anyhow::ensure!(status.success(), "cargo build failed: {status}");
        Ok(())
    }

    pub fn bin(&self, profile: &str, name: &str) -> PathBuf {
        self.workspace_dir.join("target").join(profile).join(name)
    }

    pub fn server_cmd(&self, profile: &str) -> Command {
        let mut cmd = Command::new(self.bin(profile, "seedmirror-server"));
        cmd.current_dir(&self.workspace_dir)
            .arg("--socket-path")
            .arg(&self.socket_path)
            .arg("--sync-delay")
            .arg("100");

        cmd
    }

    pub fn client_cmd(&self, profile: &str) -> Command {
        let mut cmd = Command::new(self.bin(profile, "seedmirror-client"));
        cmd.current_dir(&self.workspace_dir)
            .arg("--socket-path")
            .arg(&self.socket_path)
            .arg("--ssh-hostname")
            .arg("localhost")
            .arg("-p")
            .arg(format!(
                "{}:{}",
                self.src.to_string_lossy(),
                self.dst.to_string_lossy()
            ));

        cmd
    }

    pub fn spawn_and_sync(&self, profile: &str) -> anyhow::Result<(ProcessGuard, ProcessGuard)> {
        let server = ProcessGuard::spawn(&mut self.server_cmd(profile))?;
        let client = ProcessGuard::spawn(&mut self.client_cmd(profile))?;
        self.wait_for_initial_sync()?;
        Ok((server, client))
    }

    pub fn wait_for(&self, timeout: Duration, cond: impl FnMut() -> bool) -> anyhow::Result<()> {
        let start = Instant::now();
        let mut cond = cond;
        while !cond() {
            if start.elapsed() > timeout {
                anyhow::bail!("timed out after {timeout:?} waiting for condition");
            }
            thread::sleep(Duration::from_millis(250));
        }

        Ok(())
    }

    /// Wait until the destination mirrors the source (all entries present)
    pub fn wait_for_initial_sync(&self) -> anyhow::Result<()> {
        self.wait_for(Duration::from_secs(60), || {
            dir_contains(&self.src, &self.dst)
        })
    }

    /// Assert that files which only existed locally were not removed
    pub fn assert_target_preserved(&self) -> anyhow::Result<()> {
        for rel in &self.target_paths {
            let path = self.dst.join(rel);
            anyhow::ensure!(path.exists(), "target-only path {path:?} was removed");
        }

        Ok(())
    }
}

/// Owns a temp dir, removing it on drop
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
