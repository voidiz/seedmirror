use std::process::Stdio;

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};

/// Run the given command until completion and return the stdout.
pub(crate) async fn run_with_output(cmd: &str) -> anyhow::Result<String> {
    let child = Command::new("sh").arg("-c").arg(cmd).output().await?;
    let stdout = String::from_utf8_lossy(&child.stdout);
    let stderr = String::from_utf8_lossy(&child.stderr);

    match child.status.code() {
        Some(code) => {
            if code > 0 {
                anyhow::bail!(
                    "cmd `{cmd}` exited with non-zero code: {code}, stderr: `{stderr}`, stdout: `{stdout}`"
                )
            } else {
                log::debug!("cmd: `{cmd}`, stderr: `{stderr}, stdout: `{stdout}`");
            }
        }
        None => {
            anyhow::bail!("cmd `{cmd}` was terminated unexpectedly");
        }
    }

    Ok(stdout.into_owned())
}

/// Run the given command until completion and apply `f` to each line of streaming stdout.
pub(crate) async fn run_with_streaming_output<F>(cmd: &str, f: F) -> anyhow::Result<()>
where
    F: Fn(String),
{
    log::debug!("streaming cmd: `{cmd}`");
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout should not be taken");
    let mut lines = BufReader::new(stdout).lines();

    while let Some(line) = lines.next_line().await? {
        log::debug!("streaming cmd: `{cmd}`, stdout line: `{line}`");
        f(line);
    }

    let exit_code = child.wait().await?.code();

    let mut stderr = child.stderr.take().expect("stderr should not be taken");
    let mut stderr_str = String::new();
    stderr.read_to_string(&mut stderr_str).await?;

    match exit_code {
        Some(code) => {
            if code > 0 {
                anyhow::bail!(
                    "streaming cmd `{cmd}` exited with non-zero code: {code}, stderr: `{stderr_str}`"
                )
            } else {
                log::debug!("streaming cmd: `{cmd}`, stderr: `{stderr_str}`");
            }
        }
        None => {
            anyhow::bail!("streaming cmd `{cmd}` was terminated unexpectedly");
        }
    }

    Ok(())
}
