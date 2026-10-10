use super::{Channel, Error, Result, REPOSITORY};
use reqwest::Url;
use semver::Version;

pub(super) fn feed(channel: Channel) -> String {
    let channel = match channel {
        Channel::Stable => "stable",
        Channel::Beta => "beta",
    };
    format!("https://github.com/{REPOSITORY}/releases/download/update-{channel}/latest.json")
}

pub(super) fn artifact(version: &Version) -> String {
    format!(
        "https://github.com/{REPOSITORY}/releases/download/v{version}/Maxed_{version}_x64-setup.exe"
    )
}

/// Inspect the raw authority before URL parsing normalizes away an explicit :443.
fn parse(raw: &str, host: &str) -> Result<Url> {
    if raw.len() > 8192
        || raw
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err(Error::Url);
    }
    let rest = raw.strip_prefix("https://").ok_or(Error::Url)?;
    if rest.split(['/', '?', '#']).next() != Some(host) {
        return Err(Error::Url);
    }
    let url = Url::parse(raw).map_err(|_| Error::Url)?;
    if url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.host_str() != Some(host)
    {
        return Err(Error::Url);
    }
    // Reject path normalization, escaped separators and dot-segment aliases.
    let raw_path = &rest[host.len()..].split(['?', '#']).next().unwrap_or("");
    if *raw_path != url.path() || url.path().contains('%') {
        return Err(Error::Url);
    }
    Ok(url)
}

pub(super) fn initial(raw: &str, expected: &str) -> Result<Url> {
    if raw != expected {
        return Err(Error::Url);
    }
    let url = parse(raw, "github.com")?;
    if url.query().is_some() {
        return Err(Error::Url);
    }
    Ok(url)
}

/// Frozen delivery grammar, not a wildcard prefix: one decimal repository ID
/// and one canonical lower-case UUID. A chain cannot switch delivery object paths.
pub(super) fn redirect(raw: &str, first_delivery_path: Option<&str>) -> Result<Url> {
    let url = parse(raw, "release-assets.githubusercontent.com")?;
    let parts: Vec<_> = url.path().split('/').collect();
    if parts.len() != 4
        || !parts[0].is_empty()
        || parts[1] != "github-production-release-asset"
        || parts[2].is_empty()
        || parts[2].len() > 20
        || !parts[2].bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Error::Url);
    }
    let uuid = parts[3].as_bytes();
    if uuid.len() != 36
        || !uuid.iter().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                *b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(b)
            }
        })
        || first_delivery_path.is_some_and(|path| path != url.path())
    {
        return Err(Error::Url);
    }
    Ok(url)
}
