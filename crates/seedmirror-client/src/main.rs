use std::process::ExitCode;

use clap::Parser;
use tokio::{
    signal::{self, unix::SignalKind},
    task::JoinSet,
};

use crate::{state::init_state_broker, transfer::init_remote_watcher, workqueue::Workqueue};

mod cli;
mod command;

#[cfg(feature = "gui")]
mod http;

mod state;
mod task;
mod transfer;
mod workqueue;

async fn run() -> anyhow::Result<()> {
    let args = cli::Args::parse();

    let mut sigterm = tokio::signal::unix::signal(SignalKind::terminate())?;

    let (state_updater, state_tx, state_bcast) = init_state_broker();

    let queue = Workqueue::new();
    let mut set = JoinSet::new();
    set.spawn(state_updater);

    #[cfg(feature = "gui")]
    if args.gui {
        set.spawn(http::init_http_server(&args, state_tx.clone(), state_bcast).await?);
    }

    set.spawn(init_remote_watcher(&args, queue, state_tx)?);

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
                    anyhow::bail!("unable to listen for shutdown signal: {e:#}");
                }
            }
        },
        res = set.join_next() => {
            if let Some(join_result) = res {
                match join_result {
                    Ok(task_result) => {
                        if let Err(e) = task_result {
                            anyhow::bail!("task failed: {e:#}");
                        }
                    },
                    Err(e) => {
                        anyhow::bail!("failed to wait for task: {e:#}");
                    }
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Err(e) = run().await {
        log::error!("run failure: {e:#}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
