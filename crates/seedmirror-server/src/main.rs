use clap::Parser;
use notify::{RecursiveMode, Watcher};
use tokio::{signal, task::JoinSet};

use crate::informer::ConnectionManager;

mod cli;
mod informer;
mod watcher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = cli::Args::parse();

    let (mut watcher, notify_rx) = watcher::create_watcher().await?;
    watcher.watch(&args.root_path, RecursiveMode::Recursive)?;

    let (connection_manager, connection_tx) = ConnectionManager::new();

    let mut set = JoinSet::new();
    set.spawn(connection_manager.start(args.root_path.clone(), args.socket_path));
    set.spawn(informer::notify_handler(
        args.root_path,
        notify_rx,
        connection_tx,
    ));

    log::info!("initialized. waiting for connections...");

    tokio::select! {
        res = signal::ctrl_c() => {
            match res {
                Ok(()) => {
                    log::info!("received SIGINT, shutting down...");
                }
                Err(e) => {
                    log::error!("unable to listen for shutdown signal: {e:#}");
                }
            }
        },
        _ = set.join_next() => {
            log::info!("exiting...");
        }
    }

    Ok(())
}
