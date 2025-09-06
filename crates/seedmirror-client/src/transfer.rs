use std::{
    fs::remove_file,
    path::{self, Path, PathBuf},
    pin::Pin,
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use seedmirror_core::message::Message;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::UnixStream,
    process::Command,
    time::sleep,
};

use crate::{
    cli::Args,
    command::{run_with_output, run_with_streaming_output},
    workqueue::Workqueue,
};

type Task = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

pub(crate) fn init_remote_watcher(args: &Args, workqueue: Workqueue) -> anyhow::Result<Task> {
    if args.local_socket_path.try_exists()? {
        remove_file(&args.local_socket_path).with_context(|| {
            format!(
                "failed to remove existing socket: {:?}",
                args.local_socket_path
            )
        })?;
    }

    let ssh_cmd = format!(
        "{} -L {}:{} {}",
        args.ssh_cmd,
        args.local_socket_path.to_string_lossy(),
        args.socket_path.to_string_lossy(),
        args.ssh_hostname
    );

    let _ = Command::new("sh")
        .arg("-c")
        .arg(ssh_cmd)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn ssh, command `{}`", args.ssh_cmd))?;

    let remote_watcher = new_remote_watcher(args.clone(), workqueue);
    Ok(Box::pin(remote_watcher))
}

struct RemoteWatcher {
    /// Program arguments.
    args: Args,

    /// Queue for sync tasks.
    workqueue: Workqueue,

    /// Remote root path to sync from.
    root_path: Option<PathBuf>,
}

impl RemoteWatcher {
    pub(crate) fn new(args: Args, workqueue: Workqueue) -> Self {
        Self {
            args,
            workqueue,
            root_path: None,
        }
    }

    async fn handle_message(&mut self, msg: Message) -> anyhow::Result<()> {
        match msg {
            Message::Connected { root_path } => {
                log::info!("receiving updates from remote directory: {root_path:?} ",);
                self.root_path = Some(root_path.clone());
                self.workqueue
                    .push(
                        "__full_sync".to_string(),
                        full_sync(self.args.clone(), root_path),
                    )
                    .await?;
            }
            Message::FileUpdated { path } => {
                let id = path.to_string_lossy().into_owned();
                let mut remote_path = self.root_path.clone().ok_or(anyhow::anyhow!(
                    "expected root_path to be set. did we receive a `Connected` message?"
                ))?;
                remote_path.push(path);

                self.workqueue
                    .push(id, sync_file(self.args.clone(), remote_path))
                    .await?;
            }
        };

        Ok(())
    }
}

async fn new_remote_watcher(args: Args, workqueue: Workqueue) -> anyhow::Result<()> {
    let local_socket_path = &args.local_socket_path;
    log::info!("waiting for {local_socket_path:?} to be created");
    wait_for_file(local_socket_path).await;

    log::info!("connecting to {local_socket_path:?}");
    let stream = UnixStream::connect(&local_socket_path)
        .await
        .with_context(|| format!("failed to connect to socket at {local_socket_path:?}"))?;
    log::info!("connected to {local_socket_path:?}");

    let mut watcher = RemoteWatcher::new(args, workqueue);
    let mut reader = BufReader::new(stream);

    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let content_length: usize = line.trim().parse()?;
        let mut msg_bytes = vec![0; content_length];
        reader.read_exact(&mut msg_bytes).await?;

        log::debug!("received message: {}", String::from_utf8_lossy(&msg_bytes));
        let msg: Message = serde_json::from_slice(&msg_bytes)?;
        watcher.handle_message(msg).await?;
    }
}

async fn wait_for_file(path: &Path) {
    // TODO: Use file watcher at some point
    while !path.exists() {
        sleep(Duration::from_millis(100)).await;
    }
}

async fn full_sync(args: Args, root_path: PathBuf) -> anyhow::Result<()> {
    log::info!("performing full sync...");
    let rsync_cmd = construct_rsync_cmd(&args, &root_path)?;
    let dry_run_rsync_cmd = format!("{rsync_cmd} -n");
    let fs_entries = run_with_output(&dry_run_rsync_cmd)
        .await?
        .matches("\n")
        .count();

    if fs_entries == 0 {
        log::info!("no new files, full sync done");
    } else {
        log::info!("found difference, syncing {fs_entries} filesystem entries...",);
        run_with_streaming_output(&rsync_cmd, |line| {
            log::debug!(r#"syncing "{line}""#);
        })
        .await?;
        log::info!("full sync done");
    }

    Ok(())
}

async fn sync_file(args: Args, remote_path: PathBuf) -> anyhow::Result<()> {
    log::info!("syncing {remote_path:?}");
    let rsync_cmd = construct_rsync_cmd(&args, &remote_path)?;
    let _ = run_with_output(&rsync_cmd).await?;
    Ok(())
}

fn construct_rsync_cmd(args: &Args, remote_path: &Path) -> anyhow::Result<String> {
    let ssh_cmd = format!(r#""{} {}""#, args.ssh_cmd, args.ssh_hostname);
    let rsync_base_cmd = format!("{} -e {}", args.rsync_cmd, ssh_cmd);
    let destination_path = &args.destination_path;
    let abs_destination_path = path::absolute(destination_path)
        .with_context(|| format!("failed to resolve destination path: {destination_path:?}"))?;
    let rsync_cmd = format!("{rsync_base_cmd} {remote_path:?} {abs_destination_path:?}");

    Ok(rsync_cmd)
}
