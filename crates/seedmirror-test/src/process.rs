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
        // Child::kill sends a SIGKILL which leaves orphan processes
        let _ = Command::new("kill")
            .args(["-s", "INT", &self.child.id().to_string()])
            .status();
    }
}
