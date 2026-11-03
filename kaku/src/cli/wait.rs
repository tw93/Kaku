use crate::cli::list::list_pane_entries;
use clap::Parser;
use mux::pane::PaneId;
use std::time::{Duration, Instant};
use wezterm_client::client::Client;

/// How often the pane list is read while waiting.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// The states `--for` accepts: the OSC 7501 states a program reports for
/// itself, and `exit` for a pane that is gone.
pub(crate) const STATES: &[&str] = &["working", "blocked", "done", "error", "idle", "exit"];

#[derive(Debug, Parser, Clone)]
pub struct Wait {
    /// The pane to wait on.
    #[arg(long)]
    pane_id: PaneId,

    /// Comma-separated states that end the wait: working, blocked, done,
    /// error, idle (as the program reports them over OSC 7501, which Claude
    /// Code and Codex do) and exit (the pane closed, as when the command it
    /// was split with ends).
    #[arg(long = "for", default_value = "blocked,done,error,idle,exit")]
    states: String,

    /// Only a state reached after this command starts counts, so a `done`
    /// left from the agent's previous turn does not end the wait at once.
    #[arg(long)]
    next: bool,

    /// Give up after this long, such as 90s or 10m, and exit with an error.
    #[arg(long)]
    timeout: Option<humantime::Duration>,
}

/// The parsed `--for` list, rejecting a state Kaku never reports.
pub(crate) fn parse_states(list: &str) -> anyhow::Result<Vec<String>> {
    let states: Vec<String> = list
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if states.is_empty() {
        anyhow::bail!("--for needs at least one of {}", STATES.join(", "));
    }
    for state in &states {
        if !STATES.contains(&state.as_str()) {
            anyhow::bail!("unknown state {state:?}; use {}", STATES.join(", "));
        }
    }
    Ok(states)
}

/// The pane's state as `wait` and `events` see it: its reported OSC 7501
/// state, `exit` once it is gone, or `None` when it never reported one.
pub(crate) fn pane_state(
    entries: &[(mux::tab::PaneEntry, String, String)],
    pane_id: PaneId,
) -> Option<String> {
    match entries
        .iter()
        .find(|(entry, _, _)| entry.pane_id == pane_id)
    {
        Some((entry, _, _)) => entry.program_status.clone(),
        None => Some("exit".to_string()),
    }
}

/// How many reads of the pane list in a row may fail (Kaku busy, a socket
/// hiccup) before `wait` and `events` give up.
pub(crate) const MAX_LIST_FAILURES: u32 = 4;

/// The exit status of a wait that ran out of time, so a script can tell it
/// from a failure to reach Kaku (1).
const TIMEOUT_EXIT_CODE: i32 = 2;

/// Decides when a wait ends. With `next`, a state only counts once the
/// pane has left the state it was in when the wait began, so a `done` from
/// the previous turn is skipped but the `done` after the next `working` is
/// not.
struct Waiter {
    states: Vec<String>,
    next: bool,
    initial: Option<String>,
    changed: bool,
}

impl Waiter {
    fn new(states: Vec<String>, next: bool, initial: Option<String>) -> Self {
        Self {
            states,
            next,
            initial,
            changed: false,
        }
    }

    /// Whether `state`, the latest read, ends the wait.
    fn observe(&mut self, state: Option<&str>) -> bool {
        if state != self.initial.as_deref() {
            self.changed = true;
        }
        if self.next && !self.changed {
            return false;
        }
        state.is_some_and(|state| self.states.iter().any(|s| s == state))
    }
}

/// Reads the pane list, retrying a few failed reads before giving up.
pub(crate) async fn list_with_retry(
    client: &Client,
) -> anyhow::Result<Vec<(mux::tab::PaneEntry, String, String)>> {
    let mut failures = 0;
    loop {
        match list_pane_entries(client).await {
            Ok(entries) => return Ok(entries),
            Err(err) => {
                failures += 1;
                if failures >= MAX_LIST_FAILURES {
                    return Err(err);
                }
                smol::Timer::after(POLL_INTERVAL).await;
            }
        }
    }
}

impl Wait {
    pub async fn run(self, client: Client) -> anyhow::Result<()> {
        let states = parse_states(&self.states)?;
        let deadline = self
            .timeout
            .map(|timeout| Instant::now() + Duration::from(timeout));

        let entries = list_with_retry(&client).await?;
        let exists = entries
            .iter()
            .any(|(entry, _, _)| entry.pane_id == self.pane_id);
        if !exists {
            // Pane ids are never reused, so a pane that is gone has exited,
            // as when a split command finished before the wait began.
            if states.iter().any(|s| s == "exit") {
                println!("exit");
                return Ok(());
            }
            anyhow::bail!("no pane with id {}", self.pane_id);
        }
        let initial = pane_state(&entries, self.pane_id);
        let mut current = initial.clone();
        let mut waiter = Waiter::new(states, self.next, initial);
        loop {
            if waiter.observe(current.as_deref()) {
                println!("{}", current.unwrap_or_default());
                return Ok(());
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                eprintln!(
                    "timed out waiting for pane {} ({})",
                    self.pane_id,
                    current.as_deref().unwrap_or("no reported state")
                );
                std::process::exit(TIMEOUT_EXIT_CODE);
            }
            smol::Timer::after(POLL_INTERVAL).await;
            let entries = list_with_retry(&client).await?;
            current = pane_state(&entries, self.pane_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_states, Waiter};

    #[test]
    fn states_are_checked_and_normalized() {
        assert_eq!(
            parse_states(" Done,exit ,").unwrap(),
            vec!["done".to_string(), "exit".to_string()]
        );
        assert!(parse_states("finished").is_err());
        assert!(parse_states(",").is_err());
    }

    fn run(
        states: &str,
        next: bool,
        initial: Option<&str>,
        reads: &[Option<&str>],
    ) -> Option<usize> {
        let mut waiter = Waiter::new(
            parse_states(states).unwrap(),
            next,
            initial.map(str::to_string),
        );
        std::iter::once(initial)
            .chain(reads.iter().copied())
            .position(|state| waiter.observe(state))
    }

    #[test]
    fn a_plain_wait_ends_on_the_current_state() {
        assert_eq!(run("done", false, Some("done"), &[]), Some(0));
        // A pane that never reported anything matches nothing.
        assert_eq!(
            run("done,blocked", false, None, &[None, Some("working")]),
            None
        );
    }

    #[test]
    fn next_waits_for_the_turn_after_a_previous_done() {
        // done from the last turn, then the agent works and is done again:
        // the second done ends the wait.
        assert_eq!(
            run(
                "done,blocked,error",
                true,
                Some("done"),
                &[Some("done"), Some("working"), Some("done")]
            ),
            Some(3)
        );
        assert_eq!(
            run("blocked", true, Some("done"), &[Some("blocked")]),
            Some(1)
        );
        // The pane closing ends it too when exit is asked for.
        assert_eq!(
            run("done,exit", true, Some("working"), &[Some("exit")]),
            Some(1)
        );
    }
}
