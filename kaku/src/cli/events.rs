use crate::cli::wait::{list_with_retry, parse_states, POLL_INTERVAL};
use clap::Parser;
use mux::pane::PaneId;
use std::collections::BTreeMap;
use std::io::Write;
use wezterm_client::client::Client;

#[derive(Debug, Parser, Clone)]
pub struct Events {
    /// Only report this pane.
    #[arg(long)]
    pane_id: Option<PaneId>,

    /// Only report changes to these comma-separated states (working,
    /// blocked, done, error, idle, exit).
    #[arg(long = "for")]
    states: Option<String>,

    /// Exit after the first reported change.
    #[arg(long)]
    once: bool,

    /// Print one JSON object per line instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(serde::Serialize)]
struct Event<'a> {
    pane_id: PaneId,
    title: &'a str,
    from: Option<&'a str>,
    to: &'a str,
}

/// The changes between two snapshots of pane states, in pane order: a pane
/// whose reported state changed, and `exit` for one that went away. Panes
/// that appear without a state yet are not changes.
fn changes(
    before: &BTreeMap<PaneId, (Option<String>, String)>,
    after: &BTreeMap<PaneId, (Option<String>, String)>,
) -> Vec<(PaneId, String, Option<String>, String)> {
    let mut out = vec![];
    for (pane_id, (state, title)) in after {
        let old = before.get(pane_id).and_then(|(state, _)| state.clone());
        if let Some(state) = state {
            if old.as_ref() != Some(state) {
                out.push((*pane_id, title.clone(), old, state.clone()));
            }
        }
    }
    for (pane_id, (state, title)) in before {
        if !after.contains_key(pane_id) {
            out.push((*pane_id, title.clone(), state.clone(), "exit".to_string()));
        }
    }
    out.sort_by_key(|(pane_id, ..)| *pane_id);
    out
}

impl Events {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let states = self.states.as_deref().map(parse_states).transpose()?;
        let snapshot = |entries: Vec<(mux::tab::PaneEntry, String, String)>| {
            entries
                .into_iter()
                .filter(|(entry, _, _)| self.pane_id.is_none_or(|id| id == entry.pane_id))
                .map(|(entry, _, _)| (entry.pane_id, (entry.program_status, entry.title)))
                .collect::<BTreeMap<_, _>>()
        };
        let mut before = snapshot(list_with_retry(&client).await?);
        if let Some(pane_id) = self.pane_id {
            if !before.contains_key(&pane_id) {
                anyhow::bail!("no pane with id {pane_id}");
            }
        }
        loop {
            smol::Timer::after(POLL_INTERVAL).await;
            let after = snapshot(list_with_retry(&client).await?);
            for (pane_id, title, from, to) in changes(&before, &after) {
                if states.as_ref().is_some_and(|states| !states.contains(&to)) {
                    continue;
                }
                let mut out = std::io::stdout().lock();
                if self.json {
                    let event = Event {
                        pane_id,
                        title: &title,
                        from: from.as_deref(),
                        to: &to,
                    };
                    writeln!(out, "{}", serde_json::to_string(&event)?)?;
                } else {
                    writeln!(
                        out,
                        "pane {pane_id} {}: {} -> {to}",
                        title,
                        from.as_deref().unwrap_or("-")
                    )?;
                }
                out.flush()?;
                if self.once {
                    return Ok(());
                }
            }
            // A single watched pane that closed has nothing more to report.
            if self.pane_id.is_some() && after.is_empty() {
                return Ok(());
            }
            before = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::changes;
    use mux::pane::PaneId;
    use std::collections::BTreeMap;

    fn snap(items: &[(usize, Option<&str>)]) -> BTreeMap<PaneId, (Option<String>, String)> {
        items
            .iter()
            .map(|(id, state)| {
                (
                    PaneId::new(*id),
                    (state.map(str::to_string), format!("p{id}")),
                )
            })
            .collect()
    }

    #[test]
    fn reports_state_changes_and_closed_panes() {
        let before = snap(&[(1, Some("working")), (2, None), (3, Some("idle"))]);
        let after = snap(&[(1, Some("blocked")), (2, None), (4, None)]);
        let got = changes(&before, &after);
        assert_eq!(
            got,
            vec![
                (
                    PaneId::new(1),
                    "p1".into(),
                    Some("working".into()),
                    "blocked".into()
                ),
                (
                    PaneId::new(3),
                    "p3".into(),
                    Some("idle".into()),
                    "exit".into()
                ),
            ]
        );
        // Nothing changed, nothing reported.
        assert!(changes(&after, &after).is_empty());
    }
}
