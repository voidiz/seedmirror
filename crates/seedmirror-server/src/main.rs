use clap::Parser;
use tokio::{signal, task::JoinSet};

mod cli;
mod connection;
mod informer;
mod watcher;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = cli::Args::parse();

    let mut set = JoinSet::new();
    set.spawn(connection::connection_manager(args.socket_path.clone()));

    log::info!(
        "initialized. waiting for connections on socket {:?}...",
        args.socket_path
    );

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
