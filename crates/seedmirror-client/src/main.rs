use clap::Parser;
use tokio::{signal, task::JoinSet};

use crate::{transfer::init_remote_watcher, workqueue::Workqueue};

mod cli;
mod transfer;
mod workqueue;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = cli::Args::parse();

    let queue = Workqueue::new();
    let mut set = JoinSet::new();
    set.spawn(init_remote_watcher(&args, &queue)?);

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
        res = set.join_next() => {
            log::info!("task finished, quitting: {res:?}");
        }
    }

    Ok(())
}
