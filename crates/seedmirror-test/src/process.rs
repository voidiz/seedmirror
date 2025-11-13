use std::process::{Child, Command};

pub struct ProcessGuard {
    child: Child,
}

impl ProcessGuard {
    pub fn spawn(cmd: &mut Command) -> anyhow::Result<Self> {
        let child = cmd.spawn()?;
        Ok(Self { child })
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        // Not very cross-platform, but Child::kill leaves orphan processes
        let _ = Command::new("kill")
            // Negative PID for entire process group
            .args(["-s", "INT", &format!("-{}", self.child.id())])
            .status();
    }
}
