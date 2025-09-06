use notify::{Error, Event, INotifyWatcher, RecommendedWatcher, Watcher};
use tokio::{
    runtime::Handle,
    sync::mpsc::{self, Receiver},
};

pub(crate) type NotifyEventReceiver = Receiver<Result<Event, Error>>;

pub(crate) async fn create_watcher() -> anyhow::Result<(INotifyWatcher, NotifyEventReceiver)> {
    let (tx, rx) = mpsc::channel(1);

    let handle = Handle::current();
    let watcher = RecommendedWatcher::new(
        move |res| {
            let tx = tx.clone();
            handle.spawn(async move {
                tx.send(res).await.unwrap();
            });
        },
        notify::Config::default(),
    )?;

    Ok((watcher, rx))
}
