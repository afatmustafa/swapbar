//! Update checks: Swapbar against the GitHub releases API, claude-swap against PyPI.
//! Uses the system `curl` like the rest of the app shells out to `cswap`, so no HTTP
//! stack is linked in.

use serde::Deserialize;

const LATEST_URL: &str = "https://api.github.com/repos/afatmustafa/swapbar/releases/latest";
const CSWAP_PYPI_URL: &str = "https://pypi.org/pypi/claude-swap/json";

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
    let body = fetch(LATEST_URL, "application/vnd.github+json")?;
    let r: ApiRelease =
        serde_json::from_slice(&body).map_err(|e| format!("unexpected release data: {e}"))?;
    Ok(newer(&r, current))
}

/// A claude-swap release newer than the installed one.
#[derive(Debug, Clone, PartialEq)]
pub struct CswapUpdate {
    pub installed: String,
    pub latest: String,
}

#[derive(Deserialize)]
struct PypiProject {
    info: PypiInfo,
}

#[derive(Deserialize)]
struct PypiInfo {
    version: String,
}

/// `Some` when PyPI has a final claude-swap release newer than `installed`.
pub fn check_cswap(installed: &str) -> Result<Option<CswapUpdate>, String> {
    let body = fetch(CSWAP_PYPI_URL, "application/json")?;
    let p: PypiProject =
        serde_json::from_slice(&body).map_err(|e| format!("unexpected PyPI data: {e}"))?;
    Ok(
        cswap_newer(&p.info.version, installed).then(|| CswapUpdate {
            installed: installed.to_string(),
            latest: p.info.version,
        }),
    )
}

fn fetch(url: &str, accept: &str) -> Result<Vec<u8>, String> {
    let out = crate::setup::curl()
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-time",
            "15",
            "--header",
            &format!("Accept: {accept}"),
            "--user-agent",
            concat!("Swapbar/", env!("CARGO_PKG_VERSION")),
            url,
        ])
        .output()
        .map_err(|e| format!("curl could not start: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
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

/// claude-swap versions follow PEP 440 ("0.27.2", "0.28.0b1"): the release numbers and
/// whether it is final. Any suffix counts as a pre-release.
fn parse_pep440(v: &str) -> Option<([u64; 3], bool)> {
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(v.len());
    let core = v[..end].trim_end_matches('.');
    Some((parse(core)?, end == v.len()))
}

/// Only a final release is offered, and it beats a pre-release of the same number, as
/// cswap's own update hint does.
fn cswap_newer(latest: &str, installed: &str) -> bool {
    let (Some((l, true)), Some((i, i_final))) = (parse_pep440(latest), parse_pep440(installed))
    else {
        return false;
    };
    l > i || (l == i && !i_final)
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

    #[test]
    fn offers_only_newer_final_cswap_releases() {
        assert!(cswap_newer("0.27.2", "0.26.0"));
        assert!(cswap_newer("0.27.10", "0.27.9"));
        assert!(cswap_newer("0.28.0", "0.28.0b1"));
        assert!(!cswap_newer("0.27.2", "0.27.2"));
        assert!(!cswap_newer("0.27.2", "0.28.0b1"));
        assert!(!cswap_newer("0.28.0b2", "0.27.2"), "never a pre-release");
        assert!(!cswap_newer("0.27", "0.27.0"));
        assert!(!cswap_newer("garbage", "0.26.0"));
        assert_eq!(parse_pep440("0.28.0b1"), Some(([0, 28, 0], false)));
        assert_eq!(parse_pep440("0.27.2"), Some(([0, 27, 2], true)));
    }
}
