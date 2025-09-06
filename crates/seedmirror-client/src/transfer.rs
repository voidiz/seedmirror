use std::{
    fs::remove_file,
    path::{Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader, Stdin},
    net::UnixStream,
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    time::sleep,
};

use crate::{cli::Args, workqueue::Workqueue};

type Task = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

pub(crate) fn init_remote_watcher<'a>(
    args: &'a Args,
    workqueue: &'a Workqueue,
) -> anyhow::Result<Task> {
    let ssh_cmd = shlex::split(&args.ssh_cmd).ok_or(anyhow::anyhow!(
        "failed to parse ssh command: `{}`",
        args.ssh_cmd
    ))?;

    if ssh_cmd.is_empty() {
        anyhow::bail!("failed to parse ssh command: `{}`", args.ssh_cmd);
    }

    if args.local_socket_path.try_exists()? {
        remove_file(&args.local_socket_path).with_context(|| {
            format!(
                "failed to remove existing socket: {:?}",
                args.local_socket_path
            )
        })?;
    }

    let _ = Command::new(&ssh_cmd[0])
        .args(&ssh_cmd[1..])
        .arg("-L")
        .arg(format!(
            "{}:{}",
            args.local_socket_path.to_string_lossy(),
            args.socket_path.to_string_lossy(),
        ))
        .arg(&args.ssh_hostname)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn ssh, command `{}`", args.ssh_cmd))?;

    let remote_watcher = new_remote_watcher(args.local_socket_path.clone());
    Ok(Box::pin(remote_watcher))
}

async fn new_remote_watcher(local_socket_path: PathBuf) -> anyhow::Result<()> {
    log::info!("waiting for {local_socket_path:?} to be created");
    wait_for_file(&local_socket_path).await;

    log::info!("connecting to {local_socket_path:?}");
    let stream = UnixStream::connect(&local_socket_path)
        .await
        .with_context(|| format!("failed to connect to socket at {local_socket_path:?}"))?;
    log::info!("connected to {local_socket_path:?}");

    let mut reader = BufReader::new(stream);

    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let content_length: usize = line.trim().parse()?;
        let mut msg_bytes = vec![0; content_length];
        reader.read_exact(&mut msg_bytes).await?;

        let msg_str = String::from_utf8_lossy(&msg_bytes);
        log::info!("msg: {}", msg_str);
    }
}

async fn wait_for_file(path: &Path) {
    // TODO: Use file watcher at some point
    while !path.exists() {
        sleep(Duration::from_millis(100)).await;
    }
}
