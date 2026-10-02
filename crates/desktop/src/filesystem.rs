use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::sync::mpsc;

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind, RenameMode},
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
fn event_changes_workspace_content(event: &Event, workspace_path: &Path) -> bool {
    // atomic-write-file stages `.request.yml.XXXXXX` beside the destination.
    // Creation/metadata can arrive more than one debounce before commit. These
    // files are not YAML inputs; only the destination replacement invalidates
    // the collection. Keep ambiguous events (especially directory renames) and
    // rescan hints, rather than silencing writes for a time window.
    if !event.need_rescan()
        && matches!(
            event.kind,
            EventKind::Create(CreateKind::File)
                | EventKind::Remove(RemoveKind::File)
                | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
                | EventKind::Access(AccessKind::Close(AccessMode::Write))
        )
        && !event.paths.is_empty()
        && event.paths.iter().all(|path| {
            // A bundled workspace can have any filename, including this shape.
            path.file_name() != workspace_path.file_name() && is_atomic_yaml_temporary(path)
        })
    {
        return false;
    }
    match event.kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

fn is_atomic_yaml_temporary(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some((destination, suffix)) = name
        .strip_prefix('.')
        .and_then(|name| name.rsplit_once('.'))
    else {
        return false;
    };
    matches!(
        Path::new(destination)
            .extension()
            .and_then(|ext| ext.to_str()),
        Some("yml" | "yaml")
    ) && suffix.len() == 6
        && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub(crate) fn events_reload_workspace(events: &[Event], workspace_path: &Path) -> bool {
    events.iter().any(|event| {
        event_affects_workspace(event, workspace_path)
            && event_changes_workspace_content(event, workspace_path)
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
        event::{
            AccessKind, AccessMode, CreateKind, DataChange, Flag, MetadataKind, ModifyKind,
            RemoveKind, RenameMode,
        },
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
    fn atomic_request_save_only_reloads_at_commit_across_separate_batches() {
        let root = std::env::temp_dir();
        let temporary = root.join(".request.yml.A2K5AL");
        let destination = root.join("request.yml");
        // FSEvents delivers staging creation and metadata before the rename.
        let staging = [
            Event::new(EventKind::Create(CreateKind::File)).add_path(temporary.clone()),
            Event::new(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::Ownership,
            )))
            .add_path(temporary.clone()),
            Event::new(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::Extended,
            )))
            .add_path(temporary.clone()),
            // Other backends also report staging writes and write-close.
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(temporary.clone()),
            Event::new(EventKind::Access(AccessKind::Close(AccessMode::Write)))
                .add_path(temporary.clone()),
        ];
        assert!(!events_reload_workspace(&staging, &root));
        let commit = [
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                .add_path(temporary.clone()),
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                .add_path(destination.clone()),
        ];
        assert!(events_reload_workspace(&commit, &root));
        assert!(!events_reload_workspace(
            &[Event::new(EventKind::Remove(RemoveKind::File)).add_path(temporary)],
            &root,
        ));
        // An external edit in the staging batch must still invalidate it.
        let mut mixed = staging.to_vec();
        mixed.push(
            Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                .add_path(destination),
        );
        assert!(events_reload_workspace(&mixed, &root));
    }

    #[test]
    fn temporary_filter_preserves_external_yaml_directory_renames_and_rescans() {
        let root = std::env::temp_dir();
        for name in [
            "request.yml",
            ".request.yml",
            ".request.yml.A2K5AL.yaml",
            ".request.yaml.A2K5AL.yml",
        ] {
            let event = Event::new(EventKind::Create(CreateKind::File)).add_path(root.join(name));
            assert!(events_reload_workspace(&[event], &root));
        }
        let temporary = root.join(".request.yaml.A2K5AL");
        assert!(events_reload_workspace(
            &[Event::new(EventKind::Create(CreateKind::File)).add_path(temporary.clone())],
            &temporary,
        ));
        for kind in [
            EventKind::Create(CreateKind::Folder),
            EventKind::Remove(RemoveKind::Folder),
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
            EventKind::Any,
        ] {
            assert!(events_reload_workspace(
                &[Event::new(kind).add_path(temporary.clone())],
                &root
            ));
        }
        let paired = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(root.join("request.yml"))
            .add_path(temporary.clone());
        assert!(events_reload_workspace(&[paired], &root));
        let rescan = Event::new(EventKind::Create(CreateKind::File))
            .add_path(temporary)
            .set_flag(Flag::Rescan);
        assert!(events_reload_workspace(&[rescan], &root));
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
