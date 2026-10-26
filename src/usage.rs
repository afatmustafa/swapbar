//! Local token / cost usage from Claude Code transcripts
//! (`~/.claude/projects/**/*.jsonl`). Incremental: a per-file cache keyed by
//! (size, mtime) with a byte offset so append-only files are only re-read
//! from where we left off. Dedup key is `message.id:requestId` (one JSONL
//! line per content block repeats the same usage).

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::attribution::{self, Attribution, Owner, Source};
use crate::pricing::{PriceBook, Rate};

const LOOKBACK_DAYS: i64 = 31;
const CACHE_VERSION: u32 = 2;

/// `~/.config/swapbar`, or `%APPDATA%\Swapbar` on Windows.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("Swapbar");
    }
    crate::setup::home().join(".config").join("swapbar")
}

/// Every Claude Code `projects` directory we know about: the default
/// profile, `CLAUDE_CONFIG_DIR`, sibling profiles (`~/.claude-*`) and
/// claude-swap session profiles. Deduped by canonical path, each with the
/// source that decides which account its records count for.
fn claude_projects_dirs() -> Vec<(PathBuf, Source)> {
    let home = crate::setup::home();
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cfg) = std::env::var("CLAUDE_CONFIG_DIR") {
        candidates.push(PathBuf::from(cfg).join("projects"));
    }
    candidates.push(home.join(".claude").join("projects"));
    candidates.push(home.join(".config").join("claude").join("projects"));
    if let Ok(rd) = fs::read_dir(&home) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(".claude-") && !name.starts_with(".claude-swap") {
                candidates.push(e.path().join("projects"));
            }
        }
    }
    if let Ok(rd) = fs::read_dir(home.join(".claude-swap-backup").join("sessions")) {
        for e in rd.flatten() {
            candidates.push(e.path().join("projects"));
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in candidates {
        if !c.is_dir() {
            continue;
        }
        let canon = fs::canonicalize(&c).unwrap_or(c.clone());
        if seen.insert(canon) {
            let source = attribution::source_for(c.parent().unwrap_or(&c), &home);
            out.push((c, source));
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rec {
    /// dedupe key hash
    pub k: u64,
    /// unix seconds
    pub t: i64,
    /// model id; shared so cloning a record does not allocate
    pub m: Arc<str>,
    pub i: u64,
    pub cr: u64,
    pub cc: u64,
    /// 1h cache creation subset of cc
    pub c1: u64,
    pub o: u64,
    /// incomplete streaming row (no cost)
    #[serde(default)]
    pub inc: bool,
    /// account the usage counts for; worked out on every scan, not cached
    #[serde(skip)]
    pub owner: Owner,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct FileEntry {
    size: u64,
    mtime: i64,
    offset: u64,
    recs: Vec<Rec>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Cache {
    version: u32,
    files: BTreeMap<String, FileEntry>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

impl Cache {
    fn path() -> PathBuf {
        data_dir().join("usage-cache.json")
    }
    fn load() -> Cache {
        let Ok(text) = fs::read_to_string(Self::path()) else {
            return Cache {
                version: CACHE_VERSION,
                ..Default::default()
            };
        };
        match serde_json::from_str::<Cache>(&text) {
            Ok(c) if c.version == CACHE_VERSION => c,
            _ => Cache {
                version: CACHE_VERSION,
                ..Default::default()
            },
        }
    }
    fn save(&self) {
        let path = Self::path();
        let _ = fs::create_dir_all(path.parent().unwrap());
        if let Ok(text) = serde_json::to_string(self) {
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, text).is_ok() {
                let _ = fs::rename(tmp, path);
            }
        }
    }
}

// ---- JSONL row shape ----------------------------------------------------

#[derive(Deserialize)]
struct Row<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    timestamp: Option<&'a str>,
    #[serde(rename = "requestId")]
    request_id: Option<&'a str>,
    #[serde(rename = "sessionId")]
    session_id: Option<&'a str>,
    message: Option<Msg<'a>>,
}

#[derive(Deserialize)]
struct Msg<'a> {
    id: Option<&'a str>,
    model: Option<&'a str>,
    stop_reason: Option<&'a str>,
    usage: Option<UsageJson>,
}

#[derive(Deserialize, Default)]
struct UsageJson {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation: Option<CacheCreation>,
}

#[derive(Deserialize, Default)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

fn hash_key(a: &str, b: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    b.hash(&mut h);
    h.finish()
}

fn parse_line(line: &str, min_ts: i64) -> Option<Rec> {
    if !line.contains("\"assistant\"") || !line.contains("\"usage\"") {
        return None;
    }
    let row: Row = serde_json::from_str(line).ok()?;
    if row.kind? != "assistant" {
        return None;
    }
    let msg = row.message?;
    let usage = msg.usage?;
    let model = msg.model?;
    let ts = DateTime::parse_from_rfc3339(row.timestamp?)
        .ok()?
        .timestamp();
    if ts < min_ts {
        return None;
    }
    if usage.input_tokens == 0
        && usage.output_tokens == 0
        && usage.cache_creation_input_tokens == 0
        && usage.cache_read_input_tokens == 0
    {
        return None;
    }
    let msg_id = msg.id.unwrap_or("");
    let key = match (msg_id, row.request_id, row.session_id) {
        (id, Some(req), _) if !id.is_empty() => hash_key(id, req),
        (id, None, Some(sess)) if !id.is_empty() => hash_key(sess, id),
        _ => hash_key(row.timestamp.unwrap_or(""), line),
    };
    let incomplete = msg.stop_reason.is_none()
        && usage.output_tokens == 0
        && usage.input_tokens > 0
        && usage.cache_creation_input_tokens == 0
        && usage.cache_read_input_tokens == 0;
    let c1 = usage
        .cache_creation
        .map(|c| c.ephemeral_1h_input_tokens)
        .unwrap_or(0)
        .min(usage.cache_creation_input_tokens);
    Some(Rec {
        k: key,
        t: ts,
        m: model.into(),
        i: usage.input_tokens,
        cr: usage.cache_read_input_tokens,
        cc: usage.cache_creation_input_tokens,
        c1,
        o: usage.output_tokens,
        inc: incomplete,
        owner: Owner::Unknown,
    })
}

fn scan_file(path: &Path, from_offset: u64, min_ts: i64) -> std::io::Result<(Vec<Rec>, u64)> {
    let mut f = fs::File::open(path)?;
    f.seek(SeekFrom::Start(from_offset))?;
    let mut reader = BufReader::with_capacity(1 << 20, f);
    let mut recs: Vec<Rec> = Vec::new();
    let mut index: HashMap<u64, usize> = HashMap::new();
    let mut buf = Vec::new();
    let mut consumed = from_offset;
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if buf.last() != Some(&b'\n') {
            // partial trailing line (still being written) — leave for next pass
            break;
        }
        consumed += n as u64;
        // read_line would fail the whole file on one bad byte (e.g. a torn write)
        let line = String::from_utf8_lossy(&buf);
        if let Some(rec) = parse_line(&line, min_ts) {
            match index.get(&rec.k) {
                // later row wins within a file (final streaming chunk), unless incomplete
                Some(&idx) => {
                    if !rec.inc || recs[idx].inc {
                        recs[idx] = rec;
                    }
                }
                None => {
                    index.insert(rec.k, recs.len());
                    recs.push(rec);
                }
            }
        }
    }
    Ok((recs, consumed))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, min_mtime: i64) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            walk(&p, out, min_mtime);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if mtime >= min_mtime {
                out.push(p);
            }
        }
    }
}

/// All deduplicated records within the lookback window, refreshed from disk.
pub fn collect_records() -> Vec<Rec> {
    let now = chrono::Utc::now().timestamp();
    let min_ts = now - LOOKBACK_DAYS * 86400;
    // Kept in memory between scans; disk is only read on the first scan after launch.
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(Cache::load);

    let mut files = Vec::new();
    let mut sources: HashMap<String, Source> = HashMap::new();
    for (d, source) in claude_projects_dirs() {
        let first = files.len();
        walk(&d, &mut files, min_ts - 36 * 3600);
        for f in &files[first..] {
            sources.insert(f.to_string_lossy().to_string(), source.clone());
        }
    }
    files.sort();
    let owners = Attribution::load();
    if std::env::var("SWAPBAR_DEBUG").is_ok() {
        eprintln!("jsonl files in window: {}", files.len());
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut changed = false;
    for path in &files {
        let key = path.to_string_lossy().to_string();
        seen.insert(key.clone());
        let Ok(md) = fs::metadata(path) else { continue };
        let size = md.len();
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let entry = cache.files.entry(key).or_default();
        if entry.size == size && entry.mtime == mtime {
            continue;
        }
        let (from, mut recs) = if size >= entry.offset && entry.offset > 0 {
            (entry.offset, std::mem::take(&mut entry.recs))
        } else {
            (0, Vec::new())
        };
        match scan_file(path, from, min_ts) {
            Ok((new_recs, consumed)) => {
                if std::env::var("SWAPBAR_DEBUG").is_ok() {
                    eprintln!(
                        "scanned {} from {from}: {} recs, consumed {consumed} of {size}",
                        path.display(),
                        new_recs.len()
                    );
                }
                if from == 0 {
                    recs = new_recs;
                } else {
                    let mut index: HashMap<u64, usize> =
                        recs.iter().enumerate().map(|(i, r)| (r.k, i)).collect();
                    for r in new_recs {
                        match index.get(&r.k) {
                            Some(&i) => {
                                if !r.inc || recs[i].inc {
                                    recs[i] = r;
                                }
                            }
                            None => {
                                index.insert(r.k, recs.len());
                                recs.push(r);
                            }
                        }
                    }
                }
                entry.recs = recs;
                entry.offset = consumed;
                entry.size = size;
                entry.mtime = mtime;
                changed = true;
            }
            Err(_) => {
                entry.recs = recs;
            }
        }
    }
    let before = cache.files.len();
    cache.files.retain(|k, _| seen.contains(k));
    if changed || cache.files.len() != before {
        cache.save();
    }

    // Cross-file dedup: files in sorted path order, first complete row wins.
    let mut out: Vec<Rec> = Vec::new();
    let mut index: HashMap<u64, usize> = HashMap::new();
    for (path, entry) in &cache.files {
        let source = sources.get(path).unwrap_or(&Source::Default);
        for r in &entry.recs {
            if r.t < min_ts {
                continue;
            }
            let with_owner = || Rec {
                owner: owners.owner(source, r.t),
                ..r.clone()
            };
            match index.get(&r.k) {
                Some(&i) => {
                    if out[i].inc && !r.inc {
                        out[i] = with_owner();
                    }
                }
                None => {
                    index.insert(r.k, out.len());
                    out.push(with_owner());
                }
            }
        }
    }
    out
}

// ---- Aggregation -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    Week,
    Month,
}

impl Period {
    pub fn label(self) -> &'static str {
        match self {
            Period::Today => "Bugün",
            Period::Week => "7 gün",
            Period::Month => "30 gün",
        }
    }
    fn start(self) -> i64 {
        let today = Local::now().date_naive();
        let days_back = match self {
            Period::Today => 0,
            Period::Week => 6,
            Period::Month => 29,
        };
        day_start(&Local, today - chrono::Duration::days(days_back))
    }
}

/// When `d` begins in `tz`. A DST change at midnight can skip it (the day
/// starts at 01:00) or repeat it (the first one counts).
fn day_start<Tz: TimeZone>(tz: &Tz, d: NaiveDate) -> i64 {
    (0..3)
        .filter_map(|h| d.and_hms_opt(h, 0, 0))
        .find_map(|t| tz.from_local_datetime(&t).earliest())
        .map(|t| t.timestamp())
        .unwrap_or_else(|| d.and_time(NaiveTime::MIN).and_utc().timestamp())
}

#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub input: u64,
    pub cache_read: u64,
    pub cache_create: u64,
    pub output: u64,
    pub cost: f64,
    pub requests: u64,
    pub unpriced: u64,
}

impl Totals {
    pub fn tokens(&self) -> u64 {
        self.input + self.cache_read + self.cache_create + self.output
    }
    fn add(&mut self, r: &Rec, cost: Option<f64>) {
        self.input += r.i;
        self.cache_read += r.cr;
        self.cache_create += r.cc;
        self.output += r.o;
        self.requests += 1;
        match cost {
            Some(c) => self.cost += c,
            None => self.unpriced += 1,
        }
    }
    fn merge(&mut self, o: &Totals) {
        self.input += o.input;
        self.cache_read += o.cache_read;
        self.cache_create += o.cache_create;
        self.output += o.output;
        self.requests += o.requests;
        self.cost += o.cost;
        self.unpriced += o.unpriced;
    }
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub total: Totals,
    /// sorted by cost desc
    pub by_model: Vec<(String, Totals)>,
    /// newest first
    pub by_day: Vec<(NaiveDate, Totals)>,
    /// sorted by cost desc
    pub by_account: Vec<(Owner, Totals)>,
}

pub fn summarize(recs: &[Rec], period: Period, prices: &PriceBook) -> Summary {
    let start = period.start();
    let mut total = Totals::default();
    let mut by_model: HashMap<String, Totals> = HashMap::new();
    let mut by_day: BTreeMap<NaiveDate, Totals> = BTreeMap::new();
    let mut by_account: HashMap<&Owner, Totals> = HashMap::new();
    // Price and group by raw model id (one lookup per distinct id), normalize names afterwards.
    let mut by_raw: HashMap<&str, (Option<Rate>, Totals)> = HashMap::new();
    for r in recs {
        if r.t < start {
            continue;
        }
        let (rate, totals) = by_raw
            .entry(&r.m)
            .or_insert_with(|| (prices.lookup(&r.m), Totals::default()));
        let cost = if r.inc {
            Some(0.0)
        } else {
            rate.map(|rate| rate.cost(r.i, r.cr, r.cc - r.c1, r.c1, r.o))
        };
        total.add(r, cost);
        totals.add(r, cost);
        let day = Local
            .timestamp_opt(r.t, 0)
            .single()
            .map(|d| d.date_naive())
            .unwrap_or_default();
        by_day.entry(day).or_default().add(r, cost);
        by_account.entry(&r.owner).or_default().add(r, cost);
    }
    for (raw, (_, t)) in by_raw {
        by_model
            .entry(crate::pricing::normalize_model(raw))
            .or_default()
            .merge(&t);
    }
    let by_cost = |a: &Totals, b: &Totals| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.tokens().cmp(&a.tokens()))
    };
    let mut by_model: Vec<_> = by_model.into_iter().collect();
    by_model.sort_by(|a, b| by_cost(&a.1, &b.1).then(a.0.cmp(&b.0)));
    let mut by_account: Vec<_> = by_account
        .into_iter()
        .map(|(o, t)| (o.clone(), t))
        .collect();
    by_account.sort_by(|a, b| by_cost(&a.1, &b.1).then(a.0.label().cmp(b.0.label())));
    let by_day: Vec<_> = by_day.into_iter().rev().collect();
    Summary {
        total,
        by_model,
        by_day,
        by_account,
    }
}

pub fn format_tokens(n: u64) -> String {
    let f = n as f64;
    let (v, suffix) = if f >= 1e12 {
        (f / 1e12, "T")
    } else if f >= 1e9 {
        (f / 1e9, "B")
    } else if f >= 1e6 {
        (f / 1e6, "M")
    } else if f >= 1e3 {
        (f / 1e3, "K")
    } else {
        return n.to_string();
    };
    let s = if v < 10.0 {
        format!("{v:.2}")
    } else if v < 100.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.0}")
    };
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    format!("{s}{suffix}")
}

pub fn format_usd(v: f64) -> String {
    // Round to whole cents first so 0.999 carries into the dollars ($1.00, not $0.100).
    let total_cents = (v * 100.0).round() as i64;
    let (whole, cents) = (total_cents / 100, total_cents % 100);
    let mut s = whole.to_string();
    let mut grouped = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    s = grouped.chars().rev().collect();
    format!("${s}.{cents:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, LocalResult, NaiveDateTime, Timelike};
    use std::io::Write;

    const T0: &str = "2026-10-18T10:00:00.000Z";

    fn line(id: &str, req: &str, ts: &str, model: &str, usage: &str, stop: Option<&str>) -> String {
        let stop = stop.map(|s| format!("\"{s}\"")).unwrap_or("null".into());
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","requestId":"{req}","sessionId":"s1","message":{{"id":"{id}","model":"{model}","stop_reason":{stop},"usage":{usage}}}}}"#
        )
    }

    fn usage(i: u64, cc: u64, cr: u64, o: u64) -> String {
        format!(
            r#"{{"input_tokens":{i},"cache_creation_input_tokens":{cc},"cache_read_input_tokens":{cr},"output_tokens":{o}}}"#
        )
    }

    #[test]
    fn parses_an_assistant_row() {
        let u = r#"{"input_tokens":5,"cache_creation_input_tokens":100,"cache_read_input_tokens":2000,"output_tokens":40,"cache_creation":{"ephemeral_1h_input_tokens":300}}"#;
        let r = parse_line(
            &line("m1", "r1", T0, "claude-opus-5-5", u, Some("end_turn")),
            0,
        )
        .unwrap();
        assert_eq!((r.i, r.cc, r.cr, r.o), (5, 100, 2000, 40));
        // the 1h subset can't exceed the cache writes it is part of
        assert_eq!(r.c1, 100);
        assert_eq!(&*r.m, "claude-opus-5-5");
        assert_eq!(r.t, DateTime::parse_from_rfc3339(T0).unwrap().timestamp());
        assert!(!r.inc);
        assert_eq!(r.k, hash_key("m1", "r1"));
    }

    #[test]
    fn skips_rows_without_billable_usage() {
        let ok = usage(1, 0, 0, 1);
        assert!(parse_line(&line("m", "r", T0, "x", &usage(0, 0, 0, 0), None), 0).is_none());
        assert!(parse_line(&ok.replace("input", "x"), 0).is_none());
        assert!(
            parse_line(
                &line("m", "r", T0, "x", &ok, None).replace("\"assistant\"", "\"user\""),
                0
            )
            .is_none()
        );
        assert!(parse_line("{not json \"assistant\" \"usage\"", 0).is_none());
        // older than the lookback window
        let ts = DateTime::parse_from_rfc3339(T0).unwrap().timestamp();
        assert!(parse_line(&line("m", "r", T0, "x", &ok, None), ts + 1).is_none());
    }

    #[test]
    fn flags_incomplete_streaming_rows() {
        let r = parse_line(&line("m", "r", T0, "x", &usage(10, 0, 0, 0), None), 0).unwrap();
        assert!(r.inc);
        let r = parse_line(
            &line("m", "r", T0, "x", &usage(10, 0, 0, 0), Some("end_turn")),
            0,
        )
        .unwrap();
        assert!(!r.inc);
    }

    fn temp_file(name: &str, body: impl AsRef<[u8]>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("swapbar-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p)
            .unwrap()
            .write_all(body.as_ref())
            .unwrap();
        p
    }

    #[test]
    fn scan_reads_past_a_line_with_invalid_utf8() {
        let a = line("m1", "r1", T0, "x", &usage(1, 0, 0, 1), Some("end_turn"));
        let b = line("m2", "r2", T0, "x", &usage(2, 0, 0, 2), Some("end_turn"));
        let mut body = format!("{a}\n").into_bytes();
        body.extend_from_slice(b"{\"type\":\"user\",\"text\":\"\xff\xfe\"}\n");
        body.extend_from_slice(format!("{b}\n").as_bytes());
        let p = temp_file("utf8.jsonl", &body);
        let (recs, consumed) = scan_file(&p, 0, 0).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(consumed as usize, body.len());
    }

    #[test]
    fn scan_counts_a_row_with_a_large_tool_input() {
        // a reply that is a single tool_use block writing a big file
        let input = "a".repeat(600 * 1024);
        let row = line("m1", "r1", T0, "x", &usage(3, 0, 0, 9), Some("tool_use")).replacen(
            "\"usage\"",
            &format!(
                r#""content":[{{"type":"tool_use","input":{{"content":"{input}"}}}}],"usage""#
            ),
            1,
        );
        let p = temp_file("large.jsonl", format!("{row}\n"));
        let (recs, _) = scan_file(&p, 0, 0).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].o, 9);
    }

    /// A zone at UTC-3 that moves to UTC-2 at midnight on `DST_DAY`, so that
    /// day has no 00:xx. With `REPEAT` it instead moves from UTC-2 back to
    /// UTC-3 at 01:00, so 00:xx happens twice.
    #[derive(Clone)]
    struct DstAtMidnight<const REPEAT: bool>;

    const DST_DAY: NaiveDate = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();

    fn west(hours: i32) -> FixedOffset {
        FixedOffset::west_opt(hours * 3600).unwrap()
    }

    impl<const REPEAT: bool> TimeZone for DstAtMidnight<REPEAT> {
        type Offset = FixedOffset;
        fn from_offset(_: &FixedOffset) -> Self {
            Self
        }
        fn offset_from_local_date(&self, _: &NaiveDate) -> LocalResult<FixedOffset> {
            LocalResult::None
        }
        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<FixedOffset> {
            let before = local.date() < DST_DAY;
            match (REPEAT, local.date() == DST_DAY && local.hour() == 0) {
                (false, true) => LocalResult::None,
                (true, true) => LocalResult::Ambiguous(west(2), west(3)),
                (false, false) => LocalResult::Single(west(if before { 3 } else { 2 })),
                (true, false) => LocalResult::Single(west(if before { 2 } else { 3 })),
            }
        }
        fn offset_from_utc_date(&self, _: &NaiveDate) -> FixedOffset {
            west(3)
        }
        fn offset_from_utc_datetime(&self, _: &NaiveDateTime) -> FixedOffset {
            west(3)
        }
    }

    fn utc(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp()
    }

    #[test]
    fn a_day_starts_at_local_midnight() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        assert_eq!(
            day_start(&DstAtMidnight::<false>, d),
            utc("2026-10-21T03:00:00Z")
        );
    }

    #[test]
    fn a_day_whose_midnight_is_skipped_starts_at_one() {
        assert_eq!(
            day_start(&DstAtMidnight::<false>, DST_DAY),
            utc("2026-11-01T03:00:00Z")
        );
    }

    #[test]
    fn a_day_whose_midnight_repeats_starts_at_the_first_one() {
        assert_eq!(
            day_start(&DstAtMidnight::<true>, DST_DAY),
            utc("2026-11-01T02:00:00Z")
        );
    }

    #[test]
    fn scan_dedupes_and_leaves_a_partial_line_for_later() {
        let first = line("m1", "r1", T0, "x", &usage(10, 0, 0, 0), None);
        let last = line("m1", "r1", T0, "x", &usage(10, 0, 0, 7), Some("end_turn"));
        let other = line("m2", "r2", T0, "x", &usage(1, 0, 0, 1), Some("end_turn"));
        let partial = "{\"type\":\"assistant\"";
        let body = format!("{first}\n{last}\n{other}\n{partial}");
        let p = temp_file("scan.jsonl", &body);
        let (recs, consumed) = scan_file(&p, 0, 0).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].o, 7, "the final streaming chunk wins");
        assert_eq!(consumed as usize, body.len() - partial.len());
        // resuming from the offset reads nothing new
        let (again, end) = scan_file(&p, consumed, 0).unwrap();
        assert!(again.is_empty());
        assert_eq!(end, consumed);
    }

    #[test]
    fn a_complete_row_is_not_replaced_by_an_incomplete_one() {
        let done = line("m1", "r1", T0, "x", &usage(10, 0, 0, 7), Some("end_turn"));
        let stale = line("m1", "r1", T0, "x", &usage(10, 0, 0, 0), None);
        let p = temp_file("order.jsonl", format!("{done}\n{stale}\n"));
        let (recs, _) = scan_file(&p, 0, 0).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].o, 7);
    }

    fn rec(t: i64, model: &str, owner: Owner, i: u64, o: u64) -> Rec {
        Rec {
            k: t as u64,
            t,
            m: model.into(),
            i,
            cr: 0,
            cc: 0,
            c1: 0,
            o,
            inc: false,
            owner,
        }
    }

    #[test]
    fn summarizes_by_model_and_account() {
        let now = chrono::Utc::now().timestamp();
        let a: Owner = Owner::Account(1, "a@x.com".into());
        let b: Owner = Owner::Account(2, "b@x.com".into());
        let m = 1_000_000;
        let recs = vec![
            rec(now, "claude-opus-5-5", a.clone(), m, 0),
            rec(now, "claude-opus-5-5-20300101", b.clone(), m, m),
            rec(now, "mystery-model", a.clone(), 5, 5),
            // outside the 30-day window
            rec(now - 40 * 86400, "claude-opus-5-5", a.clone(), m, m),
        ];
        let s = summarize(&recs, Period::Month, &PriceBook::default());
        assert_eq!(s.total.requests, 3);
        assert_eq!(s.total.unpriced, 1);
        assert!((s.total.cost - 28.0).abs() < 1e-9);
        // dated ids merge into one model row
        assert_eq!(s.by_model[0].0, "claude-opus-5-5");
        assert_eq!(s.by_model[0].1.requests, 2);
        assert_eq!(
            s.by_account
                .iter()
                .map(|(o, t)| (o.label(), t.cost))
                .collect::<Vec<_>>(),
            vec![("b@x.com", 24.0), ("a@x.com", 4.0)]
        );
    }

    #[test]
    fn incomplete_rows_count_tokens_but_no_cost() {
        let now = chrono::Utc::now().timestamp();
        let mut r = rec(now, "claude-opus-5-5", Owner::Unknown, 1_000_000, 0);
        r.inc = true;
        let s = summarize(&[r], Period::Today, &PriceBook::default());
        assert_eq!(
            (s.total.cost, s.total.input, s.total.unpriced),
            (0.0, 1_000_000, 0)
        );
    }

    #[test]
    fn formats_token_counts() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1K");
        assert_eq!(format_tokens(1_234), "1.23K");
        assert_eq!(format_tokens(12_340), "12.3K");
        assert_eq!(format_tokens(123_400), "123K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
        assert_eq!(format_tokens(26_200_000_000), "26.2B");
        assert_eq!(format_tokens(3_000_000_000_000), "3T");
    }

    #[test]
    fn formats_dollars() {
        assert_eq!(format_usd(0.0), "$0.00");
        assert_eq!(format_usd(0.05), "$0.05");
        assert_eq!(format_usd(9887.02), "$9,887.02");
        assert_eq!(format_usd(1_234_567.5), "$1,234,567.50");
        assert_eq!(format_usd(0.999), "$1.00");
        assert_eq!(format_usd(41.996), "$42.00");
    }
}
