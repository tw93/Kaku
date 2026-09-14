//! On-demand command transcript extraction. Shell metadata is authenticated;
//! output is read from existing scrollback, never tee'd or logged separately.
use mux::pane::Pane;
use wezterm_term::{SemanticType, StableRowIndex};

struct Command<'a> {
    start: &'a str,
    end: &'a str,
    status: &'a str,
    context: &'a str,
    text: &'a str,
}

impl<'a> Command<'a> {
    fn parse(value: &'a str) -> anyhow::Result<Self> {
        let parts: Vec<_> = value.splitn(5, '\t').collect();
        anyhow::ensure!(
            parts.len() == 5 && !parts[4].is_empty(),
            "No completed command to copy"
        );
        anyhow::ensure!(
            parts[2].parse::<u8>().is_ok(),
            "Invalid command exit status"
        );
        Ok(Self {
            start: parts[0],
            end: parts[1],
            status: parts[2],
            context: parts[3],
            text: parts[4],
        })
    }

    fn format(&self, output: &str) -> String {
        format!(
            "Started: {}\n{} % {}\n\n{}\n\nFinished: {} — exit {}\n",
            self.start,
            self.context,
            self.text,
            output.trim_matches('\n'),
            self.end,
            self.status
        )
    }
}

fn read_region(
    pane: &dyn Pane,
    start: (StableRowIndex, usize),
    end: (StableRowIndex, usize),
) -> String {
    let mut text = String::new();
    for logical in pane.get_logical_lines(start.0..end.0 + 1) {
        for (idx, line) in logical.physical_lines.iter().enumerate() {
            let row = logical.first_row + idx as StableRowIndex;
            if row < start.0 || row > end.0 {
                continue;
            }
            let left = if row == start.0 { start.1 } else { 0 };
            let right = if row == end.0 { end.1 } else { usize::MAX };
            if right > left {
                let content = line.columns_as_str(left..right);
                if line.last_cell_was_wrapped() {
                    text.push_str(&content);
                } else {
                    text.push_str(content.trim_end());
                }
            }
            if row < end.0 && !line.last_cell_was_wrapped() {
                text.push('\n');
            }
        }
    }
    text
}

pub fn transcript(pane: &dyn Pane, value: &str) -> anyhow::Result<String> {
    // Terminal user-var notifications only fire when the value changes.
    // A request id makes consecutive /copy requests distinct.
    let (request_id, value) = value
        .split_once('\n')
        .ok_or_else(|| anyhow::anyhow!("Missing copy request id"))?;
    anyhow::ensure!(request_id.parse::<u64>().is_ok(), "Invalid copy request id");
    anyhow::ensure!(
        !pane.is_alt_screen_active(),
        "Return to the shell prompt before using /copy"
    );
    let command = Command::parse(value)?;
    let zones = pane.get_semantic_zones()?;
    // The current editable /copy line is not a completed command. Inspect the
    // newest earlier input only; never silently fall back to an older match.
    let cursor = pane.get_cursor_position();
    let prompt = zones
        .iter()
        .rev()
        .find(|z| z.semantic_type == SemanticType::Prompt && z.start_y <= cursor.y)
        .ok_or_else(|| anyhow::anyhow!("/copy needs shell command markers"))?;
    let end = (prompt.start_y, prompt.start_x);
    let input = zones
        .iter()
        .rev()
        .find(|z| z.semantic_type == SemanticType::Input && (z.end_y, z.end_x) < end)
        .ok_or_else(|| anyhow::anyhow!("Command start is unavailable in scrollback"))?;
    let actual = read_region(
        pane,
        (input.start_y, input.start_x),
        (input.end_y, input.end_x + 1),
    );
    anyhow::ensure!(
        actual.trim() == command.text.trim(),
        "Command boundary could not be verified; clipboard unchanged"
    );
    let output = read_region(pane, (input.end_y, input.end_x + 1), end);
    Ok(command.format(&output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transcript_keeps_multiline_output_and_failure_status() {
        let command = Command::parse("2026-09-14T10:00:00-0400\t2026-09-14T10:01:00-0400\t1\trich@mac ~/project\tdbt run --select example").unwrap();
        let text = command.format("\nfirst\n\n  indented error\n");
        assert!(text.contains("rich@mac ~/project % dbt run --select example"));
        assert!(text.contains("first\n\n  indented error"));
        assert!(text.ends_with("2026-09-14T10:01:00-0400 — exit 1\n"));
    }
    #[test]
    fn metadata_does_not_split_tabs_inside_command() {
        let c = Command::parse("start\tend\t0\tcontext\tprintf '\ta\nb'").unwrap();
        assert_eq!(c.text, "printf '\ta\nb'");
        assert!(Command::parse("").is_err());
        assert!(Command::parse("start\tend\tinvalid\tcontext\tcmd").is_err());
    }
}
