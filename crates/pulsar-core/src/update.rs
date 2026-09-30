//! The daily release check, minus the network: versions, GitHub's answer and
//! what to remember between checks.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;
use crate::project::RELEASES_URL;

/// Runs `pulsar.exe` as the one-shot helper that fetches the latest release.
pub const CHECK_ARG: &str = "--check-update";
/// Helper exit code when the repository has no published release.
pub const EXIT_NO_RELEASE: i32 = 2;
pub const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// `1.2.3` or `v1.2.3`. Pre-release and build suffixes are rejected, so
    /// only final releases are ever offered.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.split('.').map(|p| {
            (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        });
        let version = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
}

pub fn parse_release(json: &str) -> Option<Release> {
    serde_json::from_str(json).ok()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Found(Release),
    NoRelease,
    Failed(String),
}

pub fn interpret(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Outcome {
    match code {
        Some(0) => std::str::from_utf8(stdout)
            .ok()
            .and_then(parse_release)
            .map_or_else(
                || Outcome::Failed("unreadable release information".into()),
                Outcome::Found,
            ),
        Some(EXIT_NO_RELEASE) => Outcome::NoRelease,
        _ => {
            let reason = String::from_utf8_lossy(stderr).trim().to_string();
            Outcome::Failed(if reason.is_empty() {
                "the update check did not finish".into()
            } else {
                reason
            })
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateState {
    /// Unix seconds of the last completed check; 0 means never.
    pub last_check: u64,
    /// Tag of the last release announced, so each is announced once.
    pub announced: Option<String>,
}

impl UpdateState {
    pub fn due(&self, now: u64) -> bool {
        self.last_check == 0 || now < self.last_check || now - self.last_check >= CHECK_EVERY_SECS
    }

    /// A missing or unreadable file means "never checked".
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).map_err(io::Error::other)?;
        fs::write(path, text)
    }
}

pub fn state_path() -> Option<PathBuf> {
    paths::data_dir().map(|dir| dir.join("update.toml"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub version: Version,
    pub url: String,
}

/// Records a finished check and returns the release to announce, if any: a
/// final release newer than `current`, on this repository, not yet announced.
/// A failed check changes nothing, so it is retried on the next tick.
pub fn apply(
    state: &mut UpdateState,
    outcome: &Outcome,
    current: Version,
    now: u64,
) -> Option<Announcement> {
    let release = match outcome {
        Outcome::Failed(_) => return None,
        Outcome::NoRelease => {
            state.last_check = now;
            return None;
        }
        Outcome::Found(release) => release,
    };
    state.last_check = now;
    if release.draft || release.prerelease || !from_this_repository(&release.html_url) {
        return None;
    }
    let version = Version::parse(&release.tag_name).filter(|v| *v > current)?;
    if state.announced.as_deref() == Some(release.tag_name.as_str()) {
        return None;
    }
    state.announced = Some(release.tag_name.clone());
    Some(Announcement {
        version,
        url: release.html_url.clone(),
    })
}

fn from_this_repository(url: &str) -> bool {
    url.to_ascii_lowercase()
        .starts_with(&RELEASES_URL.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://github.com/oMaN-Rod/Pulsar/releases/tag/v0.2.0";
    const JSON: &str = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/oMaN-Rod/Pulsar/releases/tag/v0.2.0","draft":false,"prerelease":false,"body":"notes","assets":[]}"#;

    fn release(tag: &str, url: &str) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: url.into(),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn versions_parse_with_or_without_a_v() {
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("1.20.0"), Some(Version(1, 20, 0)));
        for bad in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "1.2.3-beta.1",
            "1.x.3",
            "+1.2.3",
            "1..3",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn versions_compare_numerically_and_display_plainly() {
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert_eq!(Version(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn github_release_json_is_read() {
        assert_eq!(parse_release(JSON), Some(release("v0.2.0", URL)));
        assert_eq!(parse_release("<html>rate limited</html>"), None);
        assert_eq!(parse_release("{}"), None);
    }

    #[test]
    fn helper_results_are_interpreted() {
        assert_eq!(
            interpret(Some(0), JSON.as_bytes(), b""),
            Outcome::Found(release("v0.2.0", URL))
        );
        assert_eq!(
            interpret(Some(EXIT_NO_RELEASE), b"", b""),
            Outcome::NoRelease
        );
        assert_eq!(
            interpret(Some(1), b"", b"HTTP 403\n"),
            Outcome::Failed("HTTP 403".into())
        );
        assert_eq!(
            interpret(None, b"", b""),
            Outcome::Failed("the update check did not finish".into())
        );
        assert_eq!(
            interpret(Some(0), b"not json", b""),
            Outcome::Failed("unreadable release information".into())
        );
    }

    #[test]
    fn a_check_is_due_once_a_day() {
        assert!(UpdateState::default().due(1_800_000_000), "never checked");
        let state = UpdateState {
            last_check: 1_800_000_000,
            announced: None,
        };
        assert!(!state.due(1_800_000_000 + CHECK_EVERY_SECS - 1));
        assert!(state.due(1_800_000_000 + CHECK_EVERY_SECS));
        assert!(state.due(1_700_000_000), "the clock went back");
    }

    #[test]
    fn a_newer_release_is_announced_once() {
        let mut state = UpdateState::default();
        let found = Outcome::Found(release("v0.2.0", URL));
        assert_eq!(
            apply(&mut state, &found, Version(0, 1, 0), 5000),
            Some(Announcement {
                version: Version(0, 2, 0),
                url: URL.into()
            })
        );
        assert_eq!(state.last_check, 5000);
        assert_eq!(state.announced.as_deref(), Some("v0.2.0"));
        assert_eq!(apply(&mut state, &found, Version(0, 1, 0), 99_999), None);
        assert_eq!(state.last_check, 99_999);
    }

    #[test]
    fn current_or_older_releases_are_not_announced() {
        let mut state = UpdateState::default();
        for tag in ["v0.2.0", "v0.1.9"] {
            let found = Outcome::Found(release(tag, URL));
            assert_eq!(apply(&mut state, &found, Version(0, 2, 0), 5000), None);
        }
        assert_eq!(state.last_check, 5000);
        assert_eq!(state.announced, None);
    }

    #[test]
    fn drafts_prereleases_and_foreign_links_are_ignored() {
        let mut state = UpdateState::default();
        let mut draft = release("v9.0.0", URL);
        draft.draft = true;
        let mut pre = release("v9.0.0", URL);
        pre.prerelease = true;
        let foreign = release("v9.0.0", "https://example.com/Pulsar/releases/tag/v9.0.0");
        for r in [draft, pre, foreign] {
            assert_eq!(
                apply(&mut state, &Outcome::Found(r), Version(0, 1, 0), 1),
                None
            );
        }
        let differently_cased = release(
            "v9.0.0",
            "https://github.com/oman-rod/pulsar/releases/tag/v9.0.0",
        );
        assert!(
            apply(
                &mut state,
                &Outcome::Found(differently_cased),
                Version(0, 1, 0),
                1
            )
            .is_some()
        );
    }

    #[test]
    fn a_failed_check_is_retried_later() {
        let mut state = UpdateState {
            last_check: 100,
            announced: None,
        };
        let failed = Outcome::Failed("offline".into());
        assert_eq!(apply(&mut state, &failed, Version(0, 1, 0), 5000), None);
        assert_eq!(state.last_check, 100);
    }

    #[test]
    fn no_release_yet_counts_as_a_check() {
        let mut state = UpdateState::default();
        assert_eq!(
            apply(&mut state, &Outcome::NoRelease, Version(0, 1, 0), 5000),
            None
        );
        assert_eq!(state.last_check, 5000);
    }

    #[test]
    fn state_round_trips_and_a_broken_file_resets_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("update.toml");
        assert_eq!(UpdateState::load(&path), UpdateState::default());
        let state = UpdateState {
            last_check: 42,
            announced: Some("v0.2.0".into()),
        };
        state.save(&path).unwrap();
        assert_eq!(UpdateState::load(&path), state);
        fs::write(&path, "last_check = \"soon\"").unwrap();
        assert_eq!(UpdateState::load(&path), UpdateState::default());
    }
}
