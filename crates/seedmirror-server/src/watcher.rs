use notify::{Error, Event, INotifyWatcher, RecommendedWatcher, Watcher};
use tokio::{
    runtime::Handle,
    sync::mpsc::{self, Receiver},
    task::{self},
};

pub(crate) type NotifyEventReceiver = Receiver<Result<Event, Error>>;

pub(crate) async fn create_watcher() -> anyhow::Result<(INotifyWatcher, NotifyEventReceiver)> {
    let (tx, rx) = mpsc::channel(1);

    let handle = Handle::current();
    let watcher = RecommendedWatcher::new(
        move |res| {
            task::block_in_place(|| {
                handle.block_on(async {
                    tx.send(res).await.unwrap();
                });
            });
        },
        notify::Config::default(),
    )?;

    Ok((watcher, rx))
}
