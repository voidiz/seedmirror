use std::process::Stdio;

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

/// Run the given command until completion and return the stdout.
pub(crate) async fn run_with_output(cmd: &str) -> anyhow::Result<String> {
    let child = Command::new("sh").arg("-c").arg(cmd).output().await?;
    let stdout = String::from_utf8_lossy(&child.stdout);

    Ok(stdout.into_owned())
}

/// Run the given command until completion and apply `f` to each line of streaming stdout.
pub(crate) async fn run_with_streaming_output<F>(cmd: &str, f: F) -> anyhow::Result<()>
where
    F: Fn(String),
{
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout should not be taken");
    let mut lines = BufReader::new(stdout).lines();

    while let Some(line) = lines.next_line().await? {
        f(line);
    }

    child.wait().await?;
    Ok(())
}
