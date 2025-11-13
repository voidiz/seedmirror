use clap::Parser;
use tokio::{signal::{self, unix::SignalKind}, task::JoinSet};

use crate::{transfer::init_remote_watcher, workqueue::Workqueue};

mod cli;
mod command;
mod transfer;
mod workqueue;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = cli::Args::parse();

    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())?;

    let queue = Workqueue::new();
    let mut set = JoinSet::new();
    set.spawn(init_remote_watcher(&args, queue)?);

    tokio::select! {
        _ = sigterm.recv() => {
            log::info!("received SIGTERM, shutting down...");
        },
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
        res = set.join_next() => {
            log::info!("task finished, quitting: {res:?}");
        }
    }

    Ok(())
}
