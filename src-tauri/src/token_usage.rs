//! Tokens a Claude Code conversation used, read from its transcripts, per local day.
//!
//! Every `assistant` record carries its response's `usage` (measured with 2.1.322, see
//! docs/wiki/pages/sources/token-usage-measurement.md). Claude Code writes one record per content
//! block of a response, each with the same `message.id` and the same `usage`, so a response
//! counts once per id. Sub-agent records (`isSidechain`, or the transcripts under
//! `<session>/subagents/`) are paid as well and count; the caller lists their files.
//!
//! Only numbers, model ids and times are read, never the text of a record.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model_prices::PriceTable;
use crate::subagents::claude::{stamp_of, Stamp};
use crate::subagents::claude_log::skipped_cheaply;
use crate::subagents::tail::Tail;

const SECONDS_PER_DAY: i64 = 86_400;

/// Token counts of one or more responses, by price class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
}

impl Tokens {
    pub fn total(&self) -> u64 {
        [
            self.input,
            self.output,
            self.cache_write_5m,
            self.cache_write_1h,
            self.cache_read,
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }

    pub fn add(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_write_5m = self.cache_write_5m.saturating_add(other.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
    }
}

/// A calendar day on the local wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        let valid = (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month);
        valid.then_some(Self { year, month, day })
    }

    /// Days since 1970-01-01.
    pub fn index(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    pub fn from_index(days: i64) -> Self {
        let (year, month, day) = civil_from_days(days);
        Self { year, month, day }
    }

    /// The day `count` days earlier.
    pub fn minus(self, count: i64) -> Self {
        Self::from_index(self.index() - count)
    }

    /// "2026-11-13" back to a day.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        if parts.next().is_some() || text.len() != 10 {
            return None;
        }
        Self::new(year, month, day)
    }
}

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // The ranges follow from the algorithm: month 1..=12, day 1..=31; years fit for any time a
    // transcript can carry.
    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// The UTC day and time of `secs` seconds since the epoch: (day, hour, minute, second).
pub fn utc_parts(secs: i64) -> (Day, u32, u32, u32) {
    let day = Day::from_index(secs.div_euclid(SECONDS_PER_DAY));
    let rest = u32::try_from(secs.rem_euclid(SECONDS_PER_DAY)).unwrap_or(0);
    (day, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Seconds since the epoch of an RFC 3339 time as Claude Code writes it
/// (`2026-11-12T10:00:00.000Z`); an offset instead of `Z` is honored.
pub fn parse_utc(text: &str) -> Option<i64> {
    let text = text.trim();
    let date = Day::parse(text.get(..10)?)?;
    let rest = text.get(10..)?;
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix('t'))?;
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = rest.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (hour, minute, second) = (number(0..2)?, number(3..5)?, number(6..8)?);
    if rest.get(2..3)? != ":" || rest.get(5..6)? != ":" || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut zone = rest.get(8..)?;
    if let Some(fraction) = zone.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        zone = fraction.get(digits..)?;
    }
    let offset = match zone {
        "Z" | "z" => 0,
        _ => {
            let sign = match zone.get(..1)? {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            let hours: i64 = zone.get(1..3)?.parse().ok()?;
            let minutes: i64 = zone.get(4..6)?.parse().ok()?;
            if zone.get(3..4)? != ":" || zone.len() != 6 {
                return None;
            }
            sign * (hours * 3600 + minutes * 60)
        }
    };
    Some(date.index() * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second - offset)
}

/// Turns seconds since the epoch into the local day; the operating system's zone in the app
/// ([`system_local_day`]), a fixed one in tests.
pub type LocalDay = fn(i64) -> Option<Day>;

/// The day of `secs` in the user's time zone, daylight saving time included.
#[cfg(windows)]
pub fn system_local_day(secs: i64) -> Option<Day> {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

    let (day, hour, minute, second) = utc_parts(secs);
    let part = |value: u32| u16::try_from(value).ok();
    let utc = SYSTEMTIME {
        wYear: u16::try_from(day.year).ok()?,
        wMonth: part(day.month)?,
        wDayOfWeek: 0,
        wDay: part(day.day)?,
        wHour: part(hour)?,
        wMinute: part(minute)?,
        wSecond: part(second)?,
        wMilliseconds: 0,
    };
    let mut local = utc;
    // SAFETY: both pointers refer to initialized SYSTEMTIME values on this stack frame that
    // outlive the call; a null zone pointer asks for the currently active time zone.
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
    if ok == 0 {
        return None;
    }
    Day::new(
        i32::from(local.wYear),
        u32::from(local.wMonth),
        u32::from(local.wDay),
    )
}

/// The day of `secs` in the user's time zone (`TZ` or the system zone), daylight saving included.
#[cfg(unix)]
pub fn system_local_day(secs: i64) -> Option<Day> {
    // `time_t` is 64 bits on the supported targets but 32 bits on some Unix ones.
    #[allow(clippy::useless_conversion)]
    let time: libc::time_t = secs.try_into().ok()?;
    // SAFETY: `tm` is a plain C struct for which all zero bytes are a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live values on this stack frame; localtime_r is the
    // thread-safe variant and writes only into `tm`.
    let result = unsafe { libc::localtime_r(&time, &mut tm) };
    if result.is_null() {
        return None;
    }
    Day::new(
        tm.tm_year.checked_add(1900)?,
        u32::try_from(tm.tm_mon.checked_add(1)?).ok()?,
        u32::try_from(tm.tm_mday).ok()?,
    )
}

/// Without a known local zone the UTC day is used.
#[cfg(not(any(windows, unix)))]
pub fn system_local_day(secs: i64) -> Option<Day> {
    Some(utc_parts(secs).0)
}

#[derive(Deserialize)]
struct Head {
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct Record {
    timestamp: Option<String>,
    message: Option<Message>,
}

/// `content` is never read.
#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreation>,
    speed: Option<String>,
}

#[derive(Deserialize)]
struct CacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

/// One response as far as its cost goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// `message.id`; the same response appears on several lines.
    pub id: Option<String>,
    pub model: String,
    pub tokens: Tokens,
    /// Seconds since the epoch, `None` without a readable `timestamp`.
    pub at: Option<i64>,
    /// Fast mode, priced higher.
    pub fast: bool,
}

/// The response a transcript line carries, `None` for any other line. Synthetic responses
/// (Claude Code's own notes) and empty usage are skipped.
pub fn response(line: &[u8]) -> Option<Response> {
    if skipped_cheaply(line) {
        return None;
    }
    let head = serde_json::from_slice::<Head>(line).ok()?;
    if head.kind.as_deref() != Some("assistant") {
        return None;
    }
    let record = serde_json::from_slice::<Record>(line).ok()?;
    let message = record.message?;
    let model = message.model.unwrap_or_default();
    if model == "<synthetic>" {
        return None;
    }
    let usage = message.usage?;
    let written = usage.cache_creation_input_tokens.unwrap_or(0);
    // The split by lifetime is newer than the total; what it does not cover counts as 5 minutes.
    let (write_5m, write_1h) = match usage.cache_creation {
        Some(split) => {
            let hour = split.ephemeral_1h_input_tokens.unwrap_or(0).min(written);
            let five = split.ephemeral_5m_input_tokens.unwrap_or(0);
            (five.max(written - hour), hour)
        }
        None => (written, 0),
    };
    let tokens = Tokens {
        input: usage.input_tokens.unwrap_or(0),
        output: usage.output_tokens.unwrap_or(0),
        cache_write_5m: write_5m,
        cache_write_1h: write_1h,
        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
    };
    if tokens.total() == 0 {
        return None;
    }
    Some(Response {
        id: message.id.filter(|id| !id.is_empty() && id.len() <= 200),
        model,
        tokens,
        at: record.timestamp.as_deref().and_then(parse_utc),
        fast: usage.speed.as_deref() == Some("fast"),
    })
}

/// Tokens and cost of some responses. `cost` covers only priced responses; `unpriced` counts the
/// tokens of the others, so a total is never shown as complete when it is not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub tokens: Tokens,
    pub cost: f64,
    pub unpriced: u64,
}

impl Bucket {
    pub fn add(&mut self, other: &Self) {
        self.tokens.add(&other.tokens);
        self.cost += other.cost;
        self.unpriced = self.unpriced.saturating_add(other.unpriced);
    }

    fn of(tokens: Tokens, cost: Option<f64>) -> Self {
        Self {
            tokens,
            cost: cost.unwrap_or(0.0),
            unpriced: if cost.is_some() { 0 } else { tokens.total() },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    day: Option<Day>,
    bucket: Bucket,
}

/// One transcript file: its read position and the responses seen in it.
#[derive(Debug)]
struct FileTally {
    tail: Tail,
    generation: u32,
    stamp: Option<Stamp>,
    /// By `message.id`; responses without one get a running key.
    entries: HashMap<String, Entry>,
    unnamed: u64,
}

impl FileTally {
    fn new() -> Self {
        Self {
            // From the start: every response of the conversation counts.
            tail: Tail::with_initial(u64::MAX),
            generation: 0,
            stamp: None,
            entries: HashMap::new(),
            unnamed: 0,
        }
    }

    /// Reads what was appended; `true` when an entry changed.
    fn poll(&mut self, path: &Path, prices: &PriceTable, local_day: LocalDay) -> bool {
        let stamp = stamp_of(path);
        if stamp.is_some() && stamp == self.stamp {
            return false;
        }
        // A read error (file replaced, locked for a moment) is retried on the next poll.
        let Ok(lines) = self.tail.read_new(path) else {
            return false;
        };
        let mut changed = false;
        if self.tail.generation() != self.generation {
            // The file shrank and is read again from its start.
            self.generation = self.tail.generation();
            changed = !self.entries.is_empty();
            self.entries.clear();
            self.unnamed = 0;
        }
        // A read that stopped before the end is continued on the next poll.
        self.stamp = stamp.filter(|_| !self.tail.behind());
        for line in lines {
            let Some(found) = response(&line) else {
                continue;
            };
            let cost = prices.cost(&found.model, &found.tokens, found.fast);
            let entry = Entry {
                day: found.at.and_then(local_day),
                bucket: Bucket::of(found.tokens, cost),
            };
            let key = found.id.unwrap_or_else(|| {
                self.unnamed += 1;
                format!("#{}", self.unnamed)
            });
            self.entries.insert(key, entry);
            changed = true;
        }
        changed
    }
}

/// Tokens and cost of one conversation across its transcript files, read incrementally.
#[derive(Debug, Default)]
pub struct SessionTally {
    files: BTreeMap<PathBuf, FileTally>,
}

impl SessionTally {
    /// Reads what was appended to `files` (the main transcript and its sub-agents' transcripts);
    /// files no longer listed are forgotten. `true` when a number changed.
    pub fn poll(&mut self, files: &[PathBuf], prices: &PriceTable, local_day: LocalDay) -> bool {
        let before = self.files.len();
        self.files.retain(|path, _| files.contains(path));
        let mut changed = self.files.len() != before;
        for path in files {
            let file = self
                .files
                .entry(path.clone())
                .or_insert_with(FileTally::new);
            changed |= file.poll(path, prices, local_day);
        }
        changed
    }

    /// Every response once: a response id seen in two files counts once.
    fn entries(&self) -> impl Iterator<Item = &Entry> {
        let mut seen = HashSet::new();
        self.files.values().flat_map(move |file| {
            file.entries
                .iter()
                .filter(|(key, _)| key.starts_with('#') || seen.insert((*key).clone()))
                .map(|(_, entry)| entry)
                .collect::<Vec<_>>()
        })
    }

    /// The whole conversation.
    pub fn total(&self) -> Bucket {
        let mut total = Bucket::default();
        for entry in self.entries() {
            total.add(&entry.bucket);
        }
        total
    }

    /// Per local day; responses without a time are only in [`Self::total`].
    pub fn days(&self) -> BTreeMap<Day, Bucket> {
        let mut days: BTreeMap<Day, Bucket> = BTreeMap::new();
        for entry in self.entries() {
            if let Some(day) = entry.day {
                days.entry(day).or_default().add(&entry.bucket);
            }
        }
        days
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::io::Write as _;

    use super::*;
    use crate::test_support::TempDir;

    /// Synthetic transcript lines (never a real user's file), with a response split over two
    /// records, a sub-agent response, a synthetic one, an unknown model and times around the
    /// daylight saving changes of Europe/Berlin in 2026.
    pub(crate) const FIXTURE: &str = include_str!("../tests/fixtures/claude-transcript.jsonl");

    /// Europe/Berlin in 2026: summer time from 2026-03-29 01:00 UTC to 2026-10-25 01:00 UTC.
    pub(crate) fn berlin(secs: i64) -> Option<Day> {
        let start = parse_utc("2026-03-29T01:00:00Z")?;
        let end = parse_utc("2026-10-25T01:00:00Z")?;
        let offset = if (start..end).contains(&secs) {
            7200
        } else {
            3600
        };
        Some(utc_parts(secs + offset).0)
    }

    fn day(text: &str) -> Day {
        Day::parse(text).unwrap()
    }

    #[test]
    fn civil_days_round_trip() {
        assert_eq!(day("1970-01-01").index(), 0);
        assert_eq!(day("2000-03-01").index(), 11_017);
        assert_eq!(day("2026-11-13").to_string(), "2026-11-13");
        for index in [-800_000, -1, 0, 1, 59, 60, 11_016, 20_000, 2_932_896] {
            assert_eq!(Day::from_index(index).index(), index);
        }
        assert_eq!(day("2024-03-01").minus(1), day("2024-02-29"));
        assert_eq!(day("2026-01-01").minus(1), day("2025-12-31"));
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-1-01",
            "2026-01-01x",
            "",
            "abcd-ef-gh",
        ] {
            assert_eq!(Day::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn times_parse_as_claude_writes_them() {
        assert_eq!(parse_utc("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_utc("1970-01-02T01:02:03Z"), Some(86_400 + 3723));
        assert_eq!(parse_utc("1970-01-01T02:00:00.5+02:00"), Some(0));
        assert_eq!(parse_utc("1970-01-01T00:00:00-01:30"), Some(5400));
        for bad in [
            "1970-01-01 00:00:00Z",
            "1970-01-01T24:00:00Z",
            "1970-01-01T00:00:00",
            "1970-01-01T00:00:00+0200",
            "1970-01-01T0a:00:00Z",
            "",
        ] {
            assert_eq!(parse_utc(bad), None, "{bad}");
        }
    }

    #[test]
    fn days_follow_the_local_clock_across_daylight_saving_changes() {
        let at = |text: &str| berlin(parse_utc(text).unwrap()).unwrap().to_string();
        // Winter time, +1: just after local midnight.
        assert_eq!(at("2026-03-28T23:30:00Z"), "2026-03-29");
        assert_eq!(at("2026-03-28T22:30:00Z"), "2026-03-28");
        // Summer time, +2.
        assert_eq!(at("2026-03-29T21:30:00Z"), "2026-03-29");
        assert_eq!(at("2026-03-29T22:30:00Z"), "2026-03-30");
        assert_eq!(at("2026-10-24T22:30:00Z"), "2026-10-25");
        // Back to winter time, +1.
        assert_eq!(at("2026-10-25T22:30:00Z"), "2026-10-25");
        assert_eq!(at("2026-10-25T23:30:00Z"), "2026-10-26");
    }

    #[test]
    fn the_system_zone_stays_within_a_day_of_utc() {
        let now = parse_utc("2026-11-13T12:00:00Z").unwrap();
        let local = system_local_day(now).unwrap().index();
        let utc = utc_parts(now).0.index();
        assert!((local - utc).abs() <= 1, "{local} {utc}");
    }

    #[test]
    fn responses_carry_their_tokens_without_text() {
        let lines: Vec<Response> = FIXTURE
            .lines()
            .filter_map(|l| response(l.as_bytes()))
            .collect();
        // Two records of msg_a, msg_b, the sub-agent's msg_s, msg_c, msg_d, msg_e, msg_f, msg_g
        // and one without an id; the synthetic one, user lines, the request blob and the broken
        // line yield nothing.
        assert_eq!(lines.len(), 10);
        let first = &lines[0];
        assert_eq!(first.id.as_deref(), Some("msg_a"));
        assert_eq!(first.model, "claude-opus-5-5");
        assert_eq!(
            first.tokens,
            Tokens {
                input: 10,
                output: 200,
                cache_write_5m: 1000,
                cache_write_1h: 3000,
                cache_read: 50_000,
            }
        );
        assert_eq!(first.at, parse_utc("2026-03-28T23:30:00.000Z"));
        assert!(!first.fast);
        assert!(lines.iter().any(|line| line.fast));
        assert!(lines.iter().any(|line| line.id.is_none()));
        assert!(lines.iter().all(|line| line.model != "<synthetic>"));
    }

    fn write(path: &Path, data: &str) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(data.as_bytes()).unwrap();
    }

    #[test]
    fn a_tally_counts_each_response_once_per_local_day() {
        let dir = TempDir::new("token-usage");
        let main = dir.path().join("main.jsonl");
        let agent = dir.path().join("agent.jsonl");
        write(&main, FIXTURE);
        // The sub-agent file repeats msg_s, which the main transcript also carries.
        write(&agent, "{\"type\":\"assistant\",\"timestamp\":\"2026-03-29T22:30:00.000Z\",\"message\":{\"id\":\"msg_s\",\"model\":\"claude-haiku-4-5\",\"usage\":{\"input_tokens\":1000000,\"output_tokens\":0}}}\n");
        let prices = PriceTable::built_in();
        let mut tally = SessionTally::default();
        assert!(tally.poll(&[main.clone(), agent.clone()], &prices, berlin));
        let days = tally.days();
        let names: Vec<String> = days.keys().map(Day::to_string).collect();
        assert_eq!(
            names,
            ["2026-03-28", "2026-03-29", "2026-03-30", "2026-10-25"]
        );
        // msg_a (once, not twice) is on the 29th local time.
        let a = days[&day("2026-03-29")];
        assert_eq!(a.tokens.input, 10 + 5);
        assert_eq!(a.unpriced, 0);
        // The unknown model's tokens are counted but carry no price.
        let unknown = days[&day("2026-10-25")];
        assert_eq!(
            unknown.unpriced,
            unknown.tokens.total() - 1_000_000 - 1_000_000
        );
        assert!(unknown.cost > 0.0);
        // msg_s counts once: Haiku 4.5 at $1 per million input tokens.
        let s = days[&day("2026-03-30")];
        assert_eq!(s.tokens.input, 1_000_000);
        assert!((s.cost - 1.0).abs() < 1e-9, "{}", s.cost);
        // The response without a time is only in the total.
        let total = tally.total();
        let summed: u64 = days.values().map(|b| b.tokens.total()).sum();
        assert_eq!(total.tokens.total(), summed + 7);
        // Unchanged files change nothing; appended lines do.
        assert!(!tally.poll(&[main.clone(), agent.clone()], &prices, berlin));
        write(&main, "{\"type\":\"assistant\",\"timestamp\":\"2026-03-28T10:00:00Z\",\"message\":{\"id\":\"msg_z\",\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n");
        assert!(tally.poll(&[main.clone(), agent.clone()], &prices, berlin));
        assert_eq!(tally.days()[&day("2026-03-28")].tokens.input, 100 + 1 + 1);
        // A file that is gone is forgotten; msg_s stays through the main transcript.
        assert!(tally.poll(&[main], &prices, berlin));
        assert_eq!(tally.days()[&day("2026-03-30")].tokens.input, 1_000_000);
    }
}
