//! Coalesced native checkout notifications, including worktree Git metadata.
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    time::{self, Instant},
};

const COALESCE: Duration = Duration::from_millis(350);
const NOTIFICATION_CAPACITY: usize = 256;

impl super::Runtime {
    pub(super) fn start_checkout_watch(&mut self) {
        let (updates, mut projects) = watch::channel(Vec::<String>::new());
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        self.forwarders.push(thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            else {
                return;
            };
            let Ok(mut watch) = CheckoutWatch::new() else {
                return;
            };
            runtime.block_on(async {
                let mut watched = BTreeSet::new();
                let mut dirty = BTreeSet::new();
                let timer = time::sleep(COALESCE);
                tokio::pin!(timer);
                loop {
                    let was_empty = dirty.is_empty();
                    tokio::select! {
                        biased;
                        update = projects.changed() => {
                            if update.is_err() {
                                return;
                            }
                            let next: BTreeSet<_> =
                                projects.borrow_and_update().iter().cloned().collect();
                            watch.sync(next.iter().cloned());
                            dirty.retain(|project| next.contains(project));
                            dirty.extend(next.difference(&watched).cloned());
                            watched = next;
                            timer.as_mut().reset(Instant::now());
                            continue;
                        }
                        _ = &mut timer, if !dirty.is_empty() => {
                            for project in std::mem::take(&mut dirty) {
                                if sender.send(super::Event::Checkout(project)).is_err() {
                                    return;
                                }
                                wake();
                            }
                        }
                        Some(()) = watch.overflow.recv() => watch.invalidate_all(&mut dirty),
                        Some(event) = watch.notifications.recv() => watch.apply(event, &mut dirty),
                    }
                    if was_empty && !dirty.is_empty() {
                        timer.as_mut().reset(Instant::now() + COALESCE);
                    }
                }
            });
        }));
        self.checkout_watch = Some(updates);
    }
}

struct CheckoutWatch {
    watcher: RecommendedWatcher,
    roots: BTreeMap<PathBuf, BTreeSet<String>>,
    notifications: mpsc::Receiver<notify::Result<notify::Event>>,
    overflow: mpsc::Receiver<()>,
}

impl CheckoutWatch {
    fn new() -> notify::Result<Self> {
        let (events, notifications) = mpsc::channel(NOTIFICATION_CAPACITY);
        let (missed, overflow) = mpsc::channel(1);
        let watcher = notify::recommended_watcher(move |event| {
            if let Err(mpsc::error::TrySendError::Full(_)) = events.try_send(event) {
                let _ = missed.try_send(());
            }
        })?;
        Ok(Self {
            watcher,
            roots: BTreeMap::new(),
            notifications,
            overflow,
        })
    }

    fn invalidate_all(&self, dirty: &mut BTreeSet<String>) {
        dirty.extend(self.roots.values().flatten().cloned());
    }

    fn apply(&self, event: notify::Result<notify::Event>, dirty: &mut BTreeSet<String>) {
        let Ok(event) = event else {
            self.invalidate_all(dirty);
            return;
        };
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
        ) {
            return;
        }
        for path in event.paths {
            if path.components().any(|c| c.as_os_str() == ".git")
                && path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(".lock"))
            {
                continue;
            }
            for (root, owners) in &self.roots {
                if let Ok(relative) = path.strip_prefix(root) {
                    if !relative.components().any(|c| {
                        matches!(
                            c.as_os_str().to_str(),
                            Some("target" | "node_modules" | ".DS_Store")
                        )
                    }) {
                        dirty.extend(owners.iter().cloned());
                    }
                }
            }
        }
    }

    fn sync(&mut self, projects: impl IntoIterator<Item = String>) {
        let mut next = BTreeMap::<PathBuf, BTreeSet<String>>::new();
        for project in projects {
            let root = PathBuf::from(&project);
            next.entry(root.clone())
                .or_default()
                .insert(project.clone());
            let Some(marker) = root
                .ancestors()
                .map(|p| p.join(".git"))
                .find(|p| p.exists())
            else {
                continue;
            };
            if marker.is_dir() {
                next.entry(marker).or_default().insert(project);
                continue;
            }
            let parent = marker.parent().unwrap().to_owned();
            if let Some(dir) = std::fs::read_to_string(marker)
                .ok()
                .and_then(|v| v.trim().strip_prefix("gitdir: ").map(str::to_owned))
            {
                let git = parent.join(dir);
                let git = git.canonicalize().unwrap_or(git);
                next.entry(git.clone()).or_default().insert(project.clone());
                if let Ok(common) = std::fs::read_to_string(git.join("commondir")) {
                    let common = git.join(common.trim());
                    next.entry(common.canonicalize().unwrap_or(common))
                        .or_default()
                        .insert(project);
                }
            }
        }
        let previous = std::mem::replace(&mut self.roots, next);
        for path in previous.keys().filter(|p| !self.roots.contains_key(*p)) {
            let _ = self.watcher.unwatch(Path::new(path));
        }
        for path in self.roots.keys().filter(|p| !previous.contains_key(*p)) {
            let _ = self.watcher.watch(path, RecursiveMode::Recursive);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_edits_and_worktree_metadata_notify_the_correct_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("checkout");
        let git = temp.path().join("metadata");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&git).unwrap();
        let root = root.canonicalize().unwrap();
        let git = git.canonicalize().unwrap();
        std::fs::write(root.join(".git"), format!("gitdir: {}", git.display())).unwrap();
        let owner = root.to_string_lossy().into_owned();
        let mut watch = CheckoutWatch::new().unwrap();
        watch.sync([owner.clone()]);
        for path in [
            root.join("file.rs"),
            root.join("Cargo.lock"),
            git.join("HEAD"),
        ] {
            let parent = path.parent().unwrap();
            while watch.notifications.try_recv().is_ok() {}
            std::fs::write(&path, "changed externally").unwrap();
            let dirty = time::timeout(Duration::from_secs(4), async {
                loop {
                    let event = watch.notifications.recv().await.unwrap().unwrap();
                    if event
                        .paths
                        .iter()
                        .any(|reported| reported == &path || reported == parent)
                    {
                        let mut dirty = BTreeSet::new();
                        watch.apply(Ok(event), &mut dirty);
                        if !dirty.is_empty() {
                            break dirty;
                        }
                    }
                }
            })
            .await
            .expect("checkout edit was not delivered");
            assert_eq!(dirty, BTreeSet::from([owner.clone()]));
        }
    }
}
