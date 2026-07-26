use std::{ffi::OsStr, io::ErrorKind, iter::once, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};

/// Run the given command until completion and return the stdout.
pub(crate) async fn run_with_output<I, S>(cmd: &str, args: I) -> anyhow::Result<String>
where
    I: AsRef<[S]>,
    S: AsRef<OsStr>,
{
    let cmdline = format_cmdline(cmd, args.as_ref());
    log::debug!("running cmd: `{cmdline}`");

    let child = Command::new(cmd)
        .args(args.as_ref())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                anyhow::anyhow!(
                    "executable `{cmd}` was not found on PATH while running `{cmdline}`"
                )
            }
            _ => anyhow::anyhow!("failed to spawn command `{cmdline}`: {e}"),
        })?;

    let stdout = String::from_utf8_lossy(&child.stdout);
    let stderr = String::from_utf8_lossy(&child.stderr);

    if !child.status.success() {
        let code_str = match child.status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by signal".to_string(),
        };

        anyhow::bail!(
            "cmd `{cmdline}` failed ({code_str}), stderr: `{stderr}`, stdout: `{stdout}`"
        );
    }

    log::debug!("cmd: `{cmdline}`, stderr: `{stderr}`, stdout: `{stdout}`");

    Ok(stdout.into_owned())
}

/// Run the given command until completion and apply `f` to each line of streaming stdout.
pub(crate) async fn run_with_streaming_output<I, S, F>(
    cmd: &str,
    args: I,
    mut f: F,
) -> anyhow::Result<()>
where
    I: AsRef<[S]>,
    S: AsRef<OsStr>,
    F: FnMut(&str),
{
    let cmdline = format_cmdline(cmd, args.as_ref());
    log::debug!("streaming cmd: `{cmdline}`");

    let mut child = Command::new(cmd)
        .args(args.as_ref())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                anyhow::anyhow!(
                    "executable `{cmd}` was not found on PATH while running `{cmdline}`"
                )
            }
            _ => anyhow::anyhow!("failed to spawn command `{cmdline}`: {e}"),
        })?;

    let stdout = child.stdout.take().expect("stdout should not be taken");
    let mut stderr = child.stderr.take().expect("stderr should not be taken");

    let stderr_handle = tokio::spawn(async move {
        let mut stderr_str = String::new();
        stderr.read_to_string(&mut stderr_str).await?;
        Ok::<String, std::io::Error>(stderr_str)
    });

    let mut stdout_reader = BufReader::new(stdout);

    let mut buffer = Vec::new();
    let mut byte_buf = [0u8; 1024];

    while let Ok(bytes_read) = stdout_reader.read(&mut byte_buf).await {
        if bytes_read == 0 {
            break; // EOF
        }

        for &byte in &byte_buf[..bytes_read] {
            if byte == b'\r' || byte == b'\n' {
                if !buffer.is_empty() {
                    let chunk = String::from_utf8_lossy(&buffer);
                    f(&chunk);
                    buffer.clear();
                }
            } else {
                buffer.push(byte);
            }
        }
    }

    // Flush any leftover trailing bytes
    if !buffer.is_empty() {
        let chunk = String::from_utf8_lossy(&buffer);
        f(&chunk);
    }

    let status = child.wait().await?;
    let stderr_str = stderr_handle
        .await
        .map_err(|e| anyhow::anyhow!("failed to join stderr reader task: {e}"))??;

    if !status.success() {
        let code_str = match status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by signal".to_string(),
        };

        anyhow::bail!("streaming cmd `{cmdline}` failed ({code_str}), stderr: `{stderr_str}`");
    }

    log::debug!("streaming cmd: `{cmdline}`, stderr: `{stderr_str}`");

    Ok(())
}

fn format_cmdline<I, S>(cmd: &str, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let lossy_args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().to_string());

    once(cmd.to_string())
        .chain(lossy_args)
        .collect::<Vec<_>>()
        .join(" ")
}
