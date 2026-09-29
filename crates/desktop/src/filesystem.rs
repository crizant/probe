use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::sync::mpsc;

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode, ModifyKind, RenameMode},
};

pub(crate) const WATCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);
#[cfg(test)]
pub(crate) const WATCH_POLL: std::time::Duration = std::time::Duration::from_millis(50);

pub(crate) struct WorkspaceWatcher {
    pub(crate) watcher: RecommendedWatcher,
    #[cfg(not(test))]
    pub(crate) receiver: tokio::sync::mpsc::UnboundedReceiver<notify::Result<Event>>,
    #[cfg(test)]
    pub(crate) receiver: mpsc::Receiver<notify::Result<Event>>,
    pub(crate) workspace_path: PathBuf,
}

pub(crate) fn workspace_base_directory(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        Some(path.to_owned())
    } else {
        path.parent().map(Path::to_owned)
    }
}

impl WorkspaceWatcher {
    pub(crate) fn start(workspace_path: &Path) -> notify::Result<Self> {
        #[cfg(not(test))]
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        #[cfg(test)]
        let (sender, receiver) = mpsc::channel();
        // Tests use std mpsc so the notify backend thread (inotify on Linux) never
        // wakes GPUI's deterministic executor. Production remains event-driven.
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        let watched_path =
            workspace_base_directory(workspace_path).unwrap_or_else(|| PathBuf::from("."));
        watcher.watch(
            &watched_path,
            if workspace_path.is_dir() {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            },
        )?;
        Ok(Self {
            watcher,
            receiver,
            workspace_path: workspace_path.to_owned(),
        })
    }
}

#[cfg(test)]
pub(crate) fn drain_watch_events(
    receiver: &mpsc::Receiver<notify::Result<Event>>,
    events: &mut Vec<Event>,
    watch_error: &mut Option<String>,
) -> bool {
    loop {
        match receiver.try_recv() {
            Ok(Ok(event)) => events.push(event),
            Ok(Err(error)) => *watch_error = Some(error.to_string()),
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => return true,
        }
    }
}

pub(crate) fn event_affects_workspace(event: &Event, workspace_path: &Path) -> bool {
    workspace_path.is_dir()
        || event
            .paths
            .iter()
            .any(|path| path == workspace_path || path.file_name() == workspace_path.file_name())
}

/// Whether `event` can change collection contents.
///
/// Linux inotify emits open and close-after-read for every read. Reloading on
/// those events reads the file again and queues the same events, so the
/// desktop refreshes continuously. A close-after-write still reloads, because
/// some editors report a save only that way.
fn event_changes_workspace_content(event: &Event) -> bool {
    match event.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

pub(crate) fn events_reload_workspace(events: &[Event], workspace_path: &Path) -> bool {
    events.iter().any(|event| {
        event_affects_workspace(event, workspace_path) && event_changes_workspace_content(event)
    })
}

pub(crate) fn rename_hints(events: &[Event], workspace_path: &Path) -> BTreeMap<String, String> {
    if !workspace_path.is_dir() {
        return BTreeMap::new();
    }
    events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::Modify(ModifyKind::Name(RenameMode::Both))
            )
        })
        .filter_map(|event| match event.paths.as_slice() {
            [from, to, ..] => Some((
                selector(workspace_path, from)?,
                selector(workspace_path, to)?,
            )),
            _ => None,
        })
        .collect()
}

fn selector(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use notify::{
        Event, EventKind,
        event::{AccessKind, AccessMode, ModifyKind, RenameMode},
    };

    use super::{drain_watch_events, events_reload_workspace, rename_hints};

    #[test]
    fn extracts_repository_selectors_from_paired_rename_events() {
        let root = std::env::temp_dir();
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(root.join("users/old.yml"))
            .add_path(root.join("users/new.yml"));

        assert_eq!(
            rename_hints(&[event], &root).get("users/old.yml"),
            Some(&"users/new.yml".to_owned())
        );
    }

    #[test]
    fn drain_collects_pending_events_and_detects_disconnect() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(std::env::temp_dir().join("users/old.yml"));
        sender.send(Ok(event.clone())).unwrap();
        drop(sender);

        let mut events = Vec::new();
        let mut watch_error = None;
        assert!(drain_watch_events(&receiver, &mut events, &mut watch_error));
        assert_eq!(events, [event]);
        assert!(watch_error.is_none());
    }

    #[test]
    fn reading_the_collection_does_not_reload_it() {
        let path = Path::new("/tmp/mycollection.yml");
        let open = Event::new(EventKind::Access(AccessKind::Open(AccessMode::Any)))
            .add_path(path.to_path_buf());
        let close_read = Event::new(EventKind::Access(AccessKind::Close(AccessMode::Read)))
            .add_path(path.to_path_buf());

        assert!(!events_reload_workspace(&[open, close_read], path));
    }

    #[test]
    fn writing_the_collection_reloads_it() {
        let path = Path::new("/tmp/mycollection.yml");
        let close_write = Event::new(EventKind::Access(AccessKind::Close(AccessMode::Write)))
            .add_path(path.to_path_buf());
        let modify = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path.to_path_buf());
        let unrelated_write = Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(Path::new("/tmp/notes.txt").to_path_buf());

        assert!(events_reload_workspace(&[close_write], path));
        assert!(events_reload_workspace(&[modify], path));
        assert!(!events_reload_workspace(&[unrelated_write], path));
    }

    #[test]
    fn drain_leaves_an_idle_channel_connected() {
        let (_sender, receiver) = std::sync::mpsc::channel::<notify::Result<Event>>();
        let mut events = Vec::new();
        let mut watch_error = None;
        assert!(!drain_watch_events(
            &receiver,
            &mut events,
            &mut watch_error
        ));
        assert!(events.is_empty());
        assert!(watch_error.is_none());
    }
}
