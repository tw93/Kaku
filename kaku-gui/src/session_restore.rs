use crate::frontend;
use anyhow::{anyhow, Context};
use config::GuiPosition;
use mux::domain::Domain;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, PaneNode, SerdeUrl, Tab};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use promise::spawn::{spawn, spawn_into_main_thread};
use serde::{Deserialize, Serialize};
use smol::Timer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wezterm_toast_notification::persistent_toast_notification;

static SAVE_SCHEDULED: AtomicBool = AtomicBool::new(false);
const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct SavedWindowSnapshot {
    version: u32,
    active_tab_idx: usize,
    window_title: String,
    tabs: Vec<SavedTabSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedTabSnapshot {
    title: String,
    pane_tree: PaneNode,
}

fn config_dir_file(name: &str) -> PathBuf {
    config::CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| config::HOME_DIR.join(".config").join("kaku"))
        .join(name)
}

fn snapshot_file() -> PathBuf {
    config_dir_file("last_window_session.json")
}

fn collect_leaf_entries(node: &PaneNode, out: &mut Vec<PaneEntry>) {
    match node {
        PaneNode::Empty => {}
        PaneNode::Split { left, right, .. } => {
            collect_leaf_entries(left, out);
            collect_leaf_entries(right, out);
        }
        PaneNode::Leaf(entry) => out.push(entry.clone()),
    }
}

fn cwd_from_working_dir(working_dir: Option<&SerdeUrl>) -> Option<String> {
    let url = working_dir?;
    if url.url.scheme() != "file" {
        return None;
    }
    url.url
        .to_file_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn focused_window_id() -> Option<MuxWindowId> {
    frontend::try_front_end()
        .and_then(|fe| fe.focused_mux_window_id())
        .or_else(|| {
            let mux = Mux::get();
            let mut windows = mux.iter_windows();
            windows.sort();
            windows.pop()
        })
}

fn build_snapshot_for_window(
    window_id: MuxWindowId,
) -> anyhow::Result<Option<SavedWindowSnapshot>> {
    let mux = Mux::get();
    let window = mux
        .get_window(window_id)
        .ok_or_else(|| anyhow!("window {window_id} not found"))?;

    if window.len() <= 1 {
        return Ok(None);
    }

    let tabs = window
        .iter()
        .map(|tab| SavedTabSnapshot {
            title: tab.get_title(),
            pane_tree: tab.codec_pane_tree(),
        })
        .collect::<Vec<_>>();

    Ok(Some(SavedWindowSnapshot {
        version: SNAPSHOT_VERSION,
        active_tab_idx: window.get_active_idx(),
        window_title: window.get_title().to_string(),
        tabs,
    }))
}

fn write_snapshot(snapshot: &SavedWindowSnapshot) -> anyhow::Result<()> {
    let file_name = snapshot_file();
    if let Some(parent) = file_name.parent() {
        config::create_user_owned_dirs(parent)
            .with_context(|| format!("create snapshot dir {}", parent.display()))?;
    }

    let encoded = serde_json::to_string_pretty(snapshot).context("encode window snapshot")?;
    std::fs::write(&file_name, format!("{encoded}\n"))
        .with_context(|| format!("write {}", file_name.display()))?;
    Ok(())
}

pub fn save_focused_window_snapshot() -> anyhow::Result<()> {
    let Some(window_id) = focused_window_id() else {
        return Ok(());
    };

    let Some(snapshot) = build_snapshot_for_window(window_id)? else {
        return Ok(());
    };

    write_snapshot(&snapshot)
}

pub fn schedule_snapshot_save() {
    if SAVE_SCHEDULED.swap(true, Ordering::SeqCst) {
        return;
    }

    spawn_into_main_thread(async move {
        Timer::after(Duration::from_millis(150)).await;
        SAVE_SCHEDULED.store(false, Ordering::SeqCst);
        if let Err(err) = save_focused_window_snapshot() {
            log::debug!("failed to save last window snapshot: {err:#}");
        }
    })
    .detach();
}

fn load_snapshot() -> anyhow::Result<SavedWindowSnapshot> {
    let file_name = snapshot_file();
    let contents = std::fs::read_to_string(&file_name)
        .with_context(|| format!("read {}", file_name.display()))?;
    let snapshot: SavedWindowSnapshot = serde_json::from_str(&contents)
        .with_context(|| format!("parse {}", file_name.display()))?;

    if snapshot.version != SNAPSHOT_VERSION {
        anyhow::bail!(
            "unsupported window snapshot version {} in {}",
            snapshot.version,
            file_name.display()
        );
    }

    if snapshot.tabs.is_empty() {
        anyhow::bail!("snapshot {} does not contain any tabs", file_name.display());
    }

    Ok(snapshot)
}

async fn spawn_panes_for_tab(
    domain: &Arc<dyn Domain>,
    root: &PaneNode,
) -> anyhow::Result<HashMap<PaneId, Arc<dyn Pane>>> {
    let mux = Mux::get();
    let encoding = config::configuration().default_encoding;
    let mut entries = Vec::new();
    collect_leaf_entries(root, &mut entries);

    let mut panes = HashMap::new();
    for entry in entries {
        let pane = domain
            .spawn_pane(
                &mux,
                entry.size,
                None,
                cwd_from_working_dir(entry.working_dir.as_ref()),
                encoding,
            )
            .await
            .with_context(|| format!("spawn pane for snapshot pane {}", entry.pane_id))?;
        panes.insert(entry.pane_id, pane);
    }

    Ok(panes)
}

async fn restore_snapshot(snapshot: SavedWindowSnapshot) -> anyhow::Result<()> {
    let mux = Mux::get();
    let workspace = mux.active_workspace();
    let domain = mux.default_domain();
    let active_tab_idx = snapshot.active_tab_idx;
    let window_title = snapshot.window_title;

    let builder = mux.new_empty_window(Some(workspace), None::<GuiPosition>);
    let window_id = *builder;

    for saved_tab in snapshot.tabs {
        let size = saved_tab.pane_tree.root_size().unwrap_or_default();
        let tab = Arc::new(Tab::new(&size));
        let panes = spawn_panes_for_tab(&domain, &saved_tab.pane_tree).await?;
        let pane_tree = saved_tab.pane_tree;

        tab.sync_with_pane_tree(size, pane_tree, |entry| {
            panes
                .get(&entry.pane_id)
                .cloned()
                .unwrap_or_else(|| panic!("missing restored pane {}", entry.pane_id))
        });

        if !saved_tab.title.is_empty() {
            tab.set_title(&saved_tab.title);
        }

        mux.add_tab_no_panes(&tab);
        mux.add_tab_to_window(&tab, window_id)?;
    }

    if let Some(mut window) = mux.get_window_mut(window_id) {
        if !window_title.is_empty() {
            window.set_title(&window_title);
        }
        if window.len() > 0 {
            let max_idx = window.len() - 1;
            window.set_active_without_saving(active_tab_idx.min(max_idx));
        }
    }

    drop(builder);
    Ok(())
}

pub fn restore_previous_window_from_menu() {
    spawn(async move {
        let result = async {
            let snapshot = load_snapshot()?;
            restore_snapshot(snapshot).await
        }
        .await;

        if let Err(err) = result {
            log::warn!("failed to restore previous window: {err:#}");
            persistent_toast_notification("Restore Previous Window", &format!("{err:#}"));
        }
    })
    .detach();
}
