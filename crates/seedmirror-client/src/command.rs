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

    log::debug!("cmd: `{cmd}`, stdout: `{stdout}`, stderr: `{stderr}`");

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

    child.wait().await?;

    let mut stderr = child.stderr.take().expect("stderr should not be taken");
    let mut stderr_str = String::new();
    stderr.read_to_string(&mut stderr_str).await?;
    log::debug!("streaming cmd: `{cmd}`, stderr: `{stderr_str}`");

    Ok(())
}
