use std::{collections::HashSet, pin::Pin, sync::Arc};

use tokio::sync::{Mutex, mpsc};

type Task = (String, TaskFn);
type TaskFn = Box<dyn FnOnce() -> BoxFutureResult + Send + Sync>;
type BoxFutureResult = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

pub(crate) struct Workqueue {
    sender: mpsc::UnboundedSender<Task>,
    active: Arc<Mutex<HashSet<String>>>,
}

impl Workqueue {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<Task>();
        let active = Arc::new(Mutex::new(HashSet::new()));
        tokio::spawn(Self::start(rx, active.clone()));

        Self { sender: tx, active }
    }

    pub(crate) async fn push<F, Fut>(&self, id: String, f: F) -> anyhow::Result<()>
    where
        F: FnOnce() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let mut active = self.active.lock().await;
        if active.contains(&id) {
            // Already queued/running
            log::debug!("skipping task `{id}` since it already exists");
            return Ok(());
        }

        active.insert(id.clone());
        self.sender.send((id, Box::new(move || Box::pin(f()))))?;

        Ok(())
    }

    async fn start(
        mut rx: mpsc::UnboundedReceiver<Task>,
        active_worker: Arc<Mutex<HashSet<String>>>,
    ) {
        while let Some((id, task)) = rx.recv().await {
            if let Err(e) = task().await {
                log::error!("task `{id}` failed: {e:#}");
            }

            let mut active = active_worker.lock().await;
            active.remove(&id);
        }
    }
}
