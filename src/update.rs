//! Update check against the GitHub releases API. Uses the system `curl` like the rest
//! of the app shells out to `cswap`, so no HTTP stack is linked in.

use serde::Deserialize;

const LATEST_URL: &str = "https://api.github.com/repos/afatmustafa/swapbar/releases/latest";

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// `Some` when the latest published release is newer than `current`.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let out = crate::setup::curl()
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-time",
            "15",
            "--header",
            "Accept: application/vnd.github+json",
            "--user-agent",
            concat!("Swapbar/", env!("CARGO_PKG_VERSION")),
            LATEST_URL,
        ])
        .output()
        .map_err(|e| format!("curl could not start: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let r: ApiRelease =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("unexpected release data: {e}"))?;
    Ok(newer(&r, current))
}

fn newer(r: &ApiRelease, current: &str) -> Option<Release> {
    if r.draft || r.prerelease {
        return None;
    }
    let version = r.tag_name.trim_start_matches('v');
    (parse(version)? > parse(current)?).then(|| Release {
        version: version.to_string(),
        url: r.html_url.clone(),
    })
}

/// `"1.2.3"` → `[1, 2, 3]`; missing parts count as 0 and a pre-release suffix is ignored.
fn parse(v: &str) -> Option<[u64; 3]> {
    let core = v.split(['-', '+']).next()?;
    let mut out = [0; 3];
    for (i, part) in core.split('.').enumerate() {
        *out.get_mut(i)? = part.parse().ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str) -> ApiRelease {
        ApiRelease {
            tag_name: tag.into(),
            html_url: format!("https://example.com/{tag}"),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn parses_versions() {
        assert_eq!(parse("0.1.1"), Some([0, 1, 1]));
        assert_eq!(parse("1.2"), Some([1, 2, 0]));
        assert_eq!(parse("1.2.3-beta.1"), Some([1, 2, 3]));
        assert_eq!(parse("1.2.3.4"), None);
        assert_eq!(parse("abc"), None);
    }

    #[test]
    fn only_newer_published_releases_count() {
        assert_eq!(
            newer(&rel("v0.2.0"), "0.1.1"),
            Some(Release {
                version: "0.2.0".into(),
                url: "https://example.com/v0.2.0".into()
            })
        );
        assert_eq!(
            newer(&rel("v0.1.10"), "0.1.9").map(|r| r.version),
            Some("0.1.10".into())
        );
        assert_eq!(newer(&rel("v0.1.1"), "0.1.1"), None);
        assert_eq!(newer(&rel("v0.1.0"), "0.1.1"), None);
        let mut pre = rel("v9.0.0");
        pre.prerelease = true;
        assert_eq!(newer(&pre, "0.1.1"), None);
    }
}
