//! Per-model USD pricing. Built-in table (per 1M tokens) with an optional
//! overlay fetched from models.dev (cached 24h), same approach as CodexBar.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rate {
    /// USD per token
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    /// 5-minute cache write
    pub cache_write: f64,
}

impl Rate {
    const fn per_million(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Rate {
            input: input / 1e6,
            output: output / 1e6,
            cache_read: cache_read / 1e6,
            cache_write: cache_write / 1e6,
        }
    }

    /// USD for one request. 1h cache writes are billed at 2× input.
    pub fn cost(
        &self,
        input: u64,
        cache_read: u64,
        cache_create_5m: u64,
        cache_create_1h: u64,
        output: u64,
    ) -> f64 {
        input as f64 * self.input
            + cache_read as f64 * self.cache_read
            + cache_create_5m as f64 * self.cache_write
            + cache_create_1h as f64 * self.input * 2.0
            + output as f64 * self.output
    }
}

const BUILTIN: &[(&str, Rate)] = &[
    (
        "claude-fable-5-1",
        Rate::per_million(10.0, 50.0, 0.25, 12.5),
    ),
    (
        "claude-mythos-5-1",
        Rate::per_million(10.0, 50.0, 0.25, 12.5),
    ),
    ("claude-fable-5", Rate::per_million(10.0, 50.0, 1.0, 12.5)),
    ("claude-mythos-5", Rate::per_million(10.0, 50.0, 1.0, 12.5)),
    ("claude-opus-5-5", Rate::per_million(4.0, 20.0, 0.20, 5.0)),
    ("claude-opus-5", Rate::per_million(5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-8", Rate::per_million(5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-7", Rate::per_million(5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-6", Rate::per_million(5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-5", Rate::per_million(5.0, 25.0, 0.5, 6.25)),
    ("claude-opus-4-1", Rate::per_million(15.0, 75.0, 1.5, 18.75)),
    ("claude-opus-4", Rate::per_million(15.0, 75.0, 1.5, 18.75)),
    ("claude-sonnet-5-5", Rate::per_million(2.0, 10.0, 0.20, 2.5)),
    ("claude-sonnet-5", Rate::per_million(2.0, 10.0, 0.20, 2.5)),
    ("claude-sonnet-4-6", Rate::per_million(3.0, 15.0, 0.3, 3.75)),
    ("claude-sonnet-4-5", Rate::per_million(3.0, 15.0, 0.3, 3.75)),
    ("claude-sonnet-4", Rate::per_million(3.0, 15.0, 0.3, 3.75)),
    (
        "claude-haiku-5-5",
        Rate::per_million(0.10, 0.50, 0.01, 0.125),
    ),
    ("claude-haiku-4-5", Rate::per_million(1.0, 5.0, 0.1, 1.25)),
];

#[derive(Debug, Clone, Default)]
pub struct PriceBook {
    overlay: HashMap<String, Rate>,
}

/// Strip provider prefixes, `[1m]` suffixes and `-YYYYMMDD` date suffixes.
pub fn normalize_model(model: &str) -> String {
    let mut m = model.trim().to_ascii_lowercase();
    if let Some(idx) = m.find('[') {
        m.truncate(idx);
    }
    if let Some(rest) = m.strip_prefix("anthropic.") {
        m = rest.to_string();
    }
    if let Some(pos) = m.rfind('.')
        && m[pos + 1..].starts_with("claude-")
    {
        m = m[pos + 1..].to_string();
    }
    // -YYYYMMDD
    let bytes = m.as_bytes();
    if m.len() > 9
        && bytes[m.len() - 9] == b'-'
        && m[m.len() - 8..].chars().all(|c| c.is_ascii_digit())
    {
        m.truncate(m.len() - 9);
    }
    m
}

impl PriceBook {
    pub fn lookup(&self, model: &str) -> Option<Rate> {
        let norm = normalize_model(model);
        if norm.is_empty() || norm == "<synthetic>" || norm == "synthetic" {
            return None;
        }
        if let Some(r) = self.overlay.get(&norm) {
            return Some(*r);
        }
        if let Some((_, r)) = BUILTIN.iter().find(|(k, _)| *k == norm) {
            return Some(*r);
        }
        // Prefix fallback: e.g. claude-opus-5-5-fast → claude-opus-5-5
        BUILTIN
            .iter()
            .filter(|(k, _)| norm.starts_with(*k))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, r)| *r)
    }

    fn cache_path() -> PathBuf {
        crate::usage::data_dir().join("model-rates.json")
    }

    /// Load the overlay from disk, refreshing from models.dev if older than 24h.
    /// Only the parsed Anthropic rates are cached (a few KB), not the full models.dev payload.
    pub fn load_with_overlay() -> PriceBook {
        let path = Self::cache_path();
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| {
                SystemTime::now().duration_since(t).unwrap_or_default()
                    < Duration::from_secs(24 * 3600)
            })
            .unwrap_or(false);
        if !fresh && let Some(overlay) = fetch_models_dev() {
            write_overlay(&path, &overlay);
            return PriceBook { overlay };
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return PriceBook::default();
        };
        let overlay = match serde_json::from_str::<HashMap<String, Rate>>(&text) {
            Ok(overlay) => overlay,
            // Older builds cached the raw models.dev payload; parse it once and shrink the file.
            Err(_) => {
                let overlay = parse_models_dev(&text);
                // Rewrite only while fresh, so the mtime keeps driving the 24h refetch.
                if fresh && !overlay.is_empty() {
                    write_overlay(&path, &overlay);
                }
                overlay
            }
        };
        PriceBook { overlay }
    }
}

fn write_overlay(path: &std::path::Path, overlay: &HashMap<String, Rate>) {
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(json) = serde_json::to_string(overlay) {
        let _ = std::fs::write(path, json);
    }
}

fn fetch_models_dev() -> Option<HashMap<String, Rate>> {
    let out = crate::setup::curl()
        .args(["-sS", "--max-time", "10", "https://models.dev/api.json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let overlay = parse_models_dev(&text);
    (!overlay.is_empty()).then_some(overlay)
}

fn parse_models_dev(text: &str) -> HashMap<String, Rate> {
    let mut map = HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return map;
    };
    let Some(models) = v
        .get("anthropic")
        .and_then(|a| a.get("models"))
        .and_then(|m| m.as_object())
    else {
        return map;
    };
    for (id, m) in models {
        let Some(cost) = m.get("cost") else { continue };
        let f = |k: &str| cost.get(k).and_then(|x| x.as_f64());
        let (Some(input), Some(output)) = (f("input"), f("output")) else {
            continue;
        };
        let cache_read = f("cache_read").unwrap_or(input * 0.1);
        let cache_write = f("cache_write").unwrap_or(input * 1.25);
        map.insert(
            normalize_model(id),
            Rate::per_million(input, output, cache_read, cache_write),
        );
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn normalizes_model_ids() {
        assert_eq!(normalize_model("claude-opus-5-5"), "claude-opus-5-5");
        assert_eq!(normalize_model(" Claude-Opus-5-5[1m] "), "claude-opus-5-5");
        assert_eq!(
            normalize_model("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            normalize_model("anthropic.claude-haiku-4-5-20251001"),
            "claude-haiku-4-5"
        );
        assert_eq!(
            normalize_model("us.anthropic.claude-opus-4-1-20250805"),
            "claude-opus-4-1"
        );
        // a trailing 8-digit run is only a date with the dash in front of it
        assert_eq!(normalize_model("model12345678"), "model12345678");
    }

    #[test]
    fn looks_up_builtin_rates_with_prefix_fallback() {
        let book = PriceBook::default();
        let opus = book.lookup("claude-opus-5-5").unwrap();
        assert!(close(opus.input, 4.0 / 1e6));
        assert!(close(opus.output, 20.0 / 1e6));
        // the longest matching prefix wins: opus-5-5, not opus-5
        let fast = book.lookup("claude-opus-5-5-fast").unwrap();
        assert!(close(fast.input, opus.input));
        assert!(close(
            book.lookup("claude-opus-5").unwrap().input,
            5.0 / 1e6
        ));
        assert!(book.lookup("<synthetic>").is_none());
        assert!(book.lookup("").is_none());
        assert!(book.lookup("gpt-5").is_none());
    }

    #[test]
    fn overlay_overrides_builtin() {
        let mut book = PriceBook::default();
        book.overlay.insert(
            "claude-opus-5-5".into(),
            Rate::per_million(1.0, 2.0, 0.1, 1.25),
        );
        assert!(close(
            book.lookup("claude-opus-5-5").unwrap().input,
            1.0 / 1e6
        ));
    }

    #[test]
    fn cost_bills_each_token_kind() {
        let r = Rate::per_million(10.0, 50.0, 1.0, 12.5);
        let m = 1_000_000;
        assert!(close(r.cost(m, 0, 0, 0, 0), 10.0));
        assert!(close(r.cost(0, m, 0, 0, 0), 1.0));
        assert!(close(r.cost(0, 0, m, 0, 0), 12.5));
        // 1h cache writes cost 2× input
        assert!(close(r.cost(0, 0, 0, m, 0), 20.0));
        assert!(close(r.cost(0, 0, 0, 0, m), 50.0));
        assert!(close(r.cost(m, m, m, m, m), 93.5));
    }

    #[test]
    fn parses_models_dev_payload() {
        let json = r#"{
            "anthropic": {"models": {
                "claude-opus-9-20300101": {"cost": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75}},
                "claude-new": {"cost": {"input": 2, "output": 8}},
                "claude-free": {"cost": {"input": 1}},
                "claude-nocost": {}
            }},
            "openai": {"models": {"gpt": {"cost": {"input": 1, "output": 1}}}}
        }"#;
        let map = parse_models_dev(json);
        assert_eq!(map.len(), 2);
        let opus = map["claude-opus-9"];
        assert!(close(opus.cache_write, 3.75 / 1e6));
        // missing cache prices default to 0.1× and 1.25× input
        let new = map["claude-new"];
        assert!(close(new.cache_read, 0.2 / 1e6));
        assert!(close(new.cache_write, 2.5 / 1e6));
        assert!(parse_models_dev("not json").is_empty());
        assert!(parse_models_dev("{}").is_empty());
    }
}
