//! Filesystem watcher → debounced batches of changed paths.

use crossbeam_channel::{Receiver, Sender};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Watcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    pub rx: Receiver<Vec<PathBuf>>,
}

impl Watcher {
    pub fn new(root: &Path, debounce: Duration) -> notify::Result<Self> {
        let (tx, rx): (Sender<Vec<PathBuf>>, Receiver<Vec<PathBuf>>) =
            crossbeam_channel::unbounded();
        let mut debouncer = new_debouncer(debounce, None, move |res: DebounceEventResult| {
            if let Ok(events) = res {
                let mut paths: Vec<PathBuf> =
                    events.into_iter().flat_map(|e| e.event.paths).collect();
                paths.sort();
                paths.dedup();
                if !paths.is_empty() {
                    let _ = tx.send(paths);
                }
            }
        })?;
        debouncer.watch(root, RecursiveMode::Recursive)?;
        Ok(Watcher {
            _debouncer: debouncer,
            rx,
        })
    }
}
