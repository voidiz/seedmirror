use std::pin::Pin;

pub(crate) type Task = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;
