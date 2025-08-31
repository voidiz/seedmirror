use tokio::signal;

use crate::workqueue::Workqueue;

mod cli;
mod transfer;
mod workqueue;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let queue = Workqueue::new();

    queue
        .push("one".to_string(), || async {
            println!("one sleep 5");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            Ok(())
        })
        .await?;
    println!("pushed first");

    queue
        .push("two".to_string(), || async {
            println!("two done");
            Ok(())
        })
        .await?;
    println!("pushed second");

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
    }

    Ok(())
}
