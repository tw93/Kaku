//! Performs tab changes requested by the web terminal (kaku-remote) through
//! the window's own paths: Cmd+T's SpawnTab for a new tab and the tab menu's
//! in-place close, so the browser behaves like the Mac does.

use crate::frontend::front_end;
use crate::termwindow::TermWindowNotif;
use config::keyassignment::{KeyAssignment, SpawnTabDomain};
use kaku_remote::{set_tab_action_handler, TabAction};
use mux::pane::PaneId;
use mux::Mux;
use std::sync::Arc;
use window::WindowOps;

pub fn register() {
    set_tab_action_handler(Arc::new(|action: TabAction| {
        promise::spawn::spawn_into_main_thread(async move { perform(action) }).detach();
    }));
}

fn perform(action: TabAction) {
    let pane_id = match action {
        TabAction::New { pane_id } | TabAction::Close { pane_id } => PaneId::from(pane_id),
    };
    let mux = Mux::get();
    let Some((_, window_id, tab_id)) = mux.resolve_pane_id(pane_id) else {
        return;
    };
    let Some(gui) = front_end().gui_window_for_mux_window(window_id) else {
        return;
    };
    match action {
        TabAction::New { .. } => {
            let (tx, rx) = smol::channel::bounded(1);
            gui.window.notify(TermWindowNotif::PerformAssignment {
                pane_id,
                assignment: KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain),
                tx: Some(tx),
            });
            promise::spawn::spawn(async move {
                if let Ok(Err(err)) = rx.recv().await {
                    log::warn!("kaku-remote web: new tab failed: {err:#}");
                }
            })
            .detach();
        }
        TabAction::Close { .. } => {
            gui.window
                .notify(TermWindowNotif::CloseConfirmedTab { tab_id });
        }
    }
}
