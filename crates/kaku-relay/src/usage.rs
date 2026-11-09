//! Anonymous usage counts, so how many people use the relay can be answered
//! per day and overall. `KAKU_RELAY_USAGE_FILE` names the file; unset (local
//! runs, tests) nothing is kept.
//!
//! The file holds a random salt, a salted hash of each Mac's id, and per day
//! only two numbers. It never holds an id, an address or anything a Mac sends.
//! The salt stays in the file, so a hash cannot be matched to a link without
//! reading the file, and yesterday's list of Macs is dropped at the day's end.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// Macs remembered overall. Registering needs only a made-up secret, so this
/// bounds what someone inventing ids can make the relay hold; past it the
/// counts stop growing, and the log says so once.
const MAX_MACS: usize = 100_000;
/// Days start at midnight in UTC+8, the maintainer's day.
const DAY_OFFSET_SECS: i64 = 8 * 3600;

/// Hash of a Mac's id to whether a browser it admitted has been seen.
type Marks = BTreeMap<String, bool>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    pub macs: usize,
    pub used: usize,
}

impl Counts {
    fn of(marks: &Marks) -> Self {
        Self {
            macs: marks.len(),
            used: marks.values().filter(|used| **used).count(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub today: Counts,
    pub ever: Counts,
}

pub struct Usage {
    path: PathBuf,
    salt: String,
    ever: Marks,
    day: String,
    today: Marks,
    days: Vec<(String, Counts)>,
    full_logged: bool,
}

impl Usage {
    /// Read the file, or start a new one with a new salt when there is none.
    /// A file that cannot be read is an error rather than a fresh start, so a
    /// fault here cannot overwrite the history.
    pub fn open(path: PathBuf, day: &str) -> std::io::Result<Self> {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path,
                    salt: new_salt()?,
                    ever: Marks::new(),
                    day: day.to_string(),
                    today: Marks::new(),
                    days: Vec::new(),
                    full_logged: false,
                });
            }
            Err(e) => return Err(e),
        };
        Self::parse(&text, path.clone()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{} is not a usage file", path.display()),
            )
        })
    }

    fn parse(text: &str, path: PathBuf) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let days = value
            .get("days")?
            .as_array()?
            .iter()
            .map(|entry| {
                Some((
                    entry.get("day")?.as_str()?.to_string(),
                    Counts {
                        macs: entry.get("macs")?.as_u64()? as usize,
                        used: entry.get("used")?.as_u64()? as usize,
                    },
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            path,
            salt: value
                .get("salt")?
                .as_str()
                .filter(|s| !s.is_empty())?
                .to_string(),
            ever: parse_marks(value.get("ever")?)?,
            day: value.get("day")?.as_str()?.to_string(),
            today: parse_marks(value.get("today")?)?,
            days,
            full_logged: false,
        })
    }

    /// Count a Mac for `day`; `used` means a browser it admitted is connected.
    pub fn note(&mut self, id: &str, used: bool, day: &str) {
        self.roll(day);
        let hash = self.hash(id);
        if !self.ever.contains_key(&hash) && self.ever.len() >= MAX_MACS {
            if !self.full_logged {
                self.full_logged = true;
                warn!("usage counts stopped growing at {MAX_MACS} Macs");
            }
            return;
        }
        *self.ever.entry(hash.clone()).or_default() |= used;
        *self.today.entry(hash).or_default() |= used;
    }

    pub fn summary(&mut self, day: &str) -> Summary {
        self.roll(day);
        Summary {
            today: Counts::of(&self.today),
            ever: Counts::of(&self.ever),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's text. `days` sorts ahead of the long `ever` list, so the
    /// history is at the top of the file.
    pub fn snapshot(&self) -> String {
        let days: Vec<_> = self
            .days
            .iter()
            .map(|(day, counts)| {
                serde_json::json!({"day": day, "macs": counts.macs, "used": counts.used})
            })
            .collect();
        serde_json::json!({
            "day": self.day,
            "days": days,
            "ever": self.ever,
            "salt": self.salt,
            "today": self.today,
        })
        .to_string()
    }

    /// Close finished days into plain numbers, dropping their list of Macs.
    fn roll(&mut self, day: &str) {
        if self.day == day {
            return;
        }
        let counts = Counts::of(&self.today);
        info!(
            "usage on {}: {} Macs, {} used",
            self.day, counts.macs, counts.used
        );
        self.days
            .push((std::mem::replace(&mut self.day, day.to_string()), counts));
        self.today.clear();
    }

    fn hash(&self, id: &str) -> String {
        let digest = Sha256::new()
            .chain_update(self.salt.as_bytes())
            .chain_update(id.as_bytes())
            .finalize();
        hex(&digest[..16])
    }
}

fn parse_marks(value: &serde_json::Value) -> Option<Marks> {
    value
        .as_object()?
        .iter()
        .map(|(hash, used)| Some((hash.clone(), used.as_bool()?)))
        .collect()
}

fn new_salt() -> std::io::Result<String> {
    let mut salt = [0u8; 32];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom")?, &mut salt)?;
    Ok(hex(&salt))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Replace the file in one step, so a stop mid-write leaves the last good one.
pub fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    day_of(secs)
}

/// The UTC+8 date of a Unix time, by the civil-from-days calculation.
fn day_of(unix_secs: u64) -> String {
    let z = (unix_secs as i64 + DAY_OFFSET_SECS).div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";
    const OTHER: &str = "fedcba9876543210fedcba9876543210";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kaku-relay-usage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("usage.json")
    }

    #[test]
    fn a_mac_counts_once_a_day_and_once_overall() {
        let mut usage = Usage::open(scratch("once"), "2026-11-09").unwrap();
        usage.note(ID, false, "2026-11-09");
        usage.note(ID, true, "2026-11-09");
        usage.note(ID, false, "2026-11-09");
        usage.note(OTHER, false, "2026-11-09");
        let seen = usage.summary("2026-11-09");
        assert_eq!(seen.today, Counts { macs: 2, used: 1 });
        assert_eq!(seen.ever, Counts { macs: 2, used: 1 });

        usage.note(ID, false, "2026-11-10");
        let next = usage.summary("2026-11-10");
        assert_eq!(
            next.today,
            Counts { macs: 1, used: 0 },
            "a new day starts over"
        );
        assert_eq!(
            next.ever,
            Counts { macs: 2, used: 1 },
            "overall keeps its use"
        );
        assert_eq!(
            usage.days,
            vec![("2026-11-09".to_string(), Counts { macs: 2, used: 1 })]
        );
    }

    #[test]
    fn a_finished_day_keeps_numbers_only() {
        let mut usage = Usage::open(scratch("numbers"), "2026-11-09").unwrap();
        usage.note(ID, true, "2026-11-09");
        let during = usage.snapshot();
        usage.summary("2026-11-10");
        let value: serde_json::Value = serde_json::from_str(&usage.snapshot()).unwrap();
        assert_eq!(value["today"].as_object().unwrap().len(), 0);
        assert_eq!(value["days"][0]["macs"], 1);
        assert_eq!(value["days"][0]["used"], 1);
        assert!(!during.contains(ID), "the id itself is never written");
    }

    #[test]
    fn the_same_mac_hashes_differently_under_another_salt() {
        let mut first = Usage::open(scratch("salt-a"), "2026-11-09").unwrap();
        let mut second = Usage::open(scratch("salt-b"), "2026-11-09").unwrap();
        first.note(ID, false, "2026-11-09");
        second.note(ID, false, "2026-11-09");
        assert_ne!(first.ever, second.ever);
        assert_eq!(first.hash(ID).len(), 32);
    }

    #[test]
    fn a_restart_keeps_the_counts_and_the_salt() {
        let path = scratch("restart");
        let mut usage = Usage::open(path.clone(), "2026-11-09").unwrap();
        usage.note(ID, true, "2026-11-09");
        usage.summary("2026-11-10");
        usage.note(OTHER, false, "2026-11-10");
        write_file(usage.path(), &usage.snapshot()).unwrap();

        let mut again = Usage::open(path.clone(), "2026-11-10").unwrap();
        again.note(ID, false, "2026-11-10");
        let seen = again.summary("2026-11-10");
        assert_eq!(
            seen.ever,
            Counts { macs: 2, used: 1 },
            "same hash, same Mac"
        );
        assert_eq!(seen.today, Counts { macs: 2, used: 0 });
        assert_eq!(again.days.len(), 1);
        assert!(
            !path.with_extension("tmp").exists(),
            "the temporary file was renamed into place"
        );
    }

    #[test]
    fn a_file_it_cannot_read_is_left_alone() {
        let path = scratch("damaged");
        for damaged in ["{\"salt\":\"", "{\"salt\":\"\",\"days\":[]}", "[]"] {
            std::fs::write(&path, damaged).unwrap();
            assert!(
                Usage::open(path.clone(), "2026-11-09").is_err(),
                "{damaged}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), damaged);
        }
    }

    #[test]
    fn made_up_ids_cannot_grow_the_counts_without_bound() {
        let mut usage = Usage::open(scratch("cap"), "2026-11-09").unwrap();
        usage.note(ID, false, "2026-11-09");
        let known = usage.hash(ID);
        for n in 0..MAX_MACS {
            usage.ever.insert(format!("filler-{n}"), false);
        }
        usage.note(OTHER, false, "2026-11-09");
        assert!(!usage.ever.contains_key(&usage.hash(OTHER)));
        assert!(!usage.today.contains_key(&usage.hash(OTHER)));
        usage.note(ID, true, "2026-11-09");
        assert_eq!(
            usage.ever.get(&known),
            Some(&true),
            "known Macs still count"
        );
    }

    #[test]
    fn days_turn_over_at_midnight_in_utc_plus_8() {
        assert_eq!(day_of(0), "1970-01-01");
        assert_eq!(day_of(16 * 3600 - 1), "1970-01-01");
        assert_eq!(day_of(16 * 3600), "1970-01-02");
        // 2024-02-29 00:00:00 UTC+8 is 2024-02-28 16:00:00 UTC.
        assert_eq!(day_of(1_709_136_000 - 1), "2024-02-28");
        assert_eq!(day_of(1_709_136_000), "2024-02-29");
        assert_eq!(day_of(1_709_222_400), "2024-03-01");
        assert_eq!(day_of(1_735_660_799), "2024-12-31");
        assert_eq!(day_of(1_735_660_800), "2025-01-01");
    }
}
