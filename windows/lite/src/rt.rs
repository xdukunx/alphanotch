// One small tokio runtime on its own thread. The UI thread never blocks on it;
// everything that talks to the network or the named pipe is spawned here.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle};

static HANDLE: OnceLock<Handle> = OnceLock::new();

pub fn init() {
    let rt = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _ = HANDLE.set(rt.handle().clone());
    std::thread::Builder::new()
        .name("coucou-rt".into())
        .spawn(move || rt.block_on(std::future::pending::<()>()))
        .expect("runtime thread");
}

pub fn spawn<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Some(h) = HANDLE.get() {
        h.spawn(fut);
    }
}
