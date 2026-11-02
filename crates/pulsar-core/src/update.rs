//! The daily release check, minus the network: versions, GitHub's answer and
//! what to remember between checks.

use std::fmt;
use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::paths;
use crate::project::RELEASES_URL;

/// Runs `pulsar.exe` as the one-shot helper that fetches the latest release.
pub const CHECK_ARG: &str = "--check-update";
/// Helper exit code when the repository has no published release.
pub const EXIT_NO_RELEASE: i32 = 2;
pub const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;
const MAX_NOTES_CHARS: usize = 2000;

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
    /// GitHub sends `null` when a release has no notes.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
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

pub fn run_helper(exe: &Path) -> Outcome {
    match Command::new(exe)
        .arg(CHECK_ARG)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
    {
        Ok(out) => interpret(out.status.code(), &out.stdout, &out.stderr),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
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

    /// Replaces the file in one step, so a reader in another process sees the
    /// old state or the new one and never part of either.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).map_err(io::Error::other)?;
        let tmp = path.with_extension(format!(
            "toml.{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
    }

    /// Applies `change` to the saved state and writes the file only if that
    /// changed something, so a check with nothing to record cannot overwrite
    /// what another process saved after the load.
    pub fn change_saved<T>(
        path: &Path,
        change: impl FnOnce(&mut Self) -> T,
    ) -> (T, io::Result<()>) {
        let before = Self::load(path);
        let mut state = before.clone();
        let result = change(&mut state);
        let saved = if state == before {
            Ok(())
        } else {
            state.save(path)
        };
        (result, saved)
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
    if !is_final_from_this_repository(release) {
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

pub(crate) fn is_final_from_this_repository(release: &Release) -> bool {
    !release.draft && !release.prerelease && from_this_repository(&release.html_url)
}

fn from_this_repository(url: &str) -> bool {
    url.to_ascii_lowercase()
        .starts_with(&RELEASES_URL.to_ascii_lowercase())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AvailableRelease {
    pub tag: String,
    pub version: Version,
    pub url: String,
    /// Plain text; shown as is, never interpreted.
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Available(AvailableRelease),
    UpToDate,
    NoRelease,
    Failed(String),
}

pub fn assess(outcome: &Outcome, current: Version) -> Availability {
    let release = match outcome {
        Outcome::Failed(reason) => return Availability::Failed(reason.clone()),
        Outcome::NoRelease => return Availability::NoRelease,
        Outcome::Found(release) => release,
    };
    if release.draft || release.prerelease {
        return Availability::UpToDate;
    }
    let version = Version::parse(&release.tag_name);
    match version {
        Some(version) if from_this_repository(&release.html_url) => {
            if version > current {
                Availability::Available(AvailableRelease {
                    tag: release.tag_name.clone(),
                    version,
                    url: release.html_url.clone(),
                    notes: release.body.as_deref().map(notes_text).unwrap_or_default(),
                })
            } else {
                Availability::UpToDate
            }
        }
        _ => Availability::Failed("unexpected release information".into()),
    }
}

/// Records a check the user asked for. It leaves `announced` to the daily
/// check, and keeps that check due while a release is still unannounced, so
/// the notification is not put off by a day.
pub fn note_manual_check(state: &mut UpdateState, outcome: &Outcome, current: Version, now: u64) {
    match assess(outcome, current) {
        Availability::Failed(_) => {}
        Availability::Available(release) if state.announced.as_ref() != Some(&release.tag) => {}
        _ => state.last_check = now,
    }
}

fn notes_text(body: &str) -> String {
    let clean: String = body
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let clean = clean.trim();
    match clean.char_indices().nth(MAX_NOTES_CHARS) {
        Some((end, _)) => format!("{}…", clean[..end].trim_end()),
        None => clean.to_string(),
    }
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
            body: None,
            assets: Vec::new(),
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
        let read = parse_release(JSON).unwrap();
        assert_eq!(read.tag_name, "v0.2.0");
        assert_eq!(read.body.as_deref(), Some("notes"));
        assert!(read.assets.is_empty());
        assert_eq!(parse_release("<html>rate limited</html>"), None);
        assert_eq!(parse_release("{}"), None);
    }

    #[test]
    fn a_release_without_notes_or_assets_is_still_read() {
        let json = format!(r#"{{"tag_name":"v0.2.0","html_url":"{URL}","body":null}}"#);
        let read = parse_release(&json).unwrap();
        assert_eq!(read.body, None);
        assert!(read.assets.is_empty());
        let json = format!(
            r#"{{"tag_name":"v0.2.0","html_url":"{URL}","assets":[{{"name":"a.exe","size":1}}]}}"#
        );
        assert_eq!(parse_release(&json).unwrap().assets[0].name, "a.exe");
    }

    #[test]
    fn helper_results_are_interpreted() {
        assert!(
            matches!(interpret(Some(0), JSON.as_bytes(), b""), Outcome::Found(r) if r.tag_name == "v0.2.0")
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
    fn the_running_version_is_comparable() {
        assert_eq!(current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn now_is_after_this_code_was_written() {
        assert!(now() > 1_790_000_000);
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
    fn a_failed_check_changes_no_field() {
        let before = UpdateState {
            last_check: 100,
            announced: Some("v0.2.0".into()),
        };
        let mut state = before.clone();
        let failed = Outcome::Failed("offline".into());
        assert_eq!(apply(&mut state, &failed, Version(0, 1, 0), 5000), None);
        assert_eq!(state, before);
    }

    #[test]
    fn a_failed_check_does_not_overwrite_a_newer_saved_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.toml");
        let older = UpdateState {
            last_check: 100,
            announced: None,
        };
        older.save(&path).unwrap();
        let newer = UpdateState {
            last_check: 200,
            announced: None,
        };
        let failed = Outcome::Failed("offline".into());
        let (announcement, saved) = UpdateState::change_saved(&path, |state| {
            newer.save(&path).unwrap();
            apply(state, &failed, Version(0, 1, 0), 5000)
        });
        assert_eq!(announcement, None);
        saved.unwrap();
        assert_eq!(UpdateState::load(&path), newer);
    }

    #[test]
    fn a_failed_check_writes_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.toml");
        let failed = Outcome::Failed("offline".into());
        let (_, saved) =
            UpdateState::change_saved(&path, |state| apply(state, &failed, Version(0, 1, 0), 5000));
        saved.unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_finished_check_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.toml");
        let found = Outcome::Found(release("v0.2.0", URL));
        let (announcement, saved) =
            UpdateState::change_saved(&path, |state| apply(state, &found, Version(0, 1, 0), 5000));
        assert!(announcement.is_some());
        saved.unwrap();
        assert_eq!(
            UpdateState::load(&path),
            UpdateState {
                last_check: 5000,
                announced: Some("v0.2.0".into()),
            }
        );
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

    #[test]
    fn saving_leaves_only_the_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.toml");
        UpdateState::default().save(&path).unwrap();
        UpdateState::default().save(&path).unwrap();
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["update.toml"]);
    }

    #[test]
    fn a_reader_never_sees_a_partly_written_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update.toml");
        let long = |n: u64| UpdateState {
            last_check: n,
            announced: Some("v".repeat(4000 + n as usize)),
        };
        long(1).save(&path).unwrap();
        std::thread::scope(|scope| {
            for n in 1..=2 {
                let (path, long) = (&path, &long);
                scope.spawn(move || {
                    for _ in 0..100 {
                        let _ = long(n).save(path);
                    }
                });
            }
            for _ in 0..500 {
                let read = fs::read_to_string(&path).unwrap();
                let state: UpdateState = toml::from_str(&read).unwrap();
                assert_eq!(state, long(state.last_check));
            }
        });
    }

    fn found(tag: &str, url: &str, body: Option<&str>) -> Outcome {
        let mut r = release(tag, url);
        r.body = body.map(String::from);
        Outcome::Found(r)
    }

    #[test]
    fn a_newer_release_is_shown_with_its_link_and_notes() {
        assert_eq!(
            assess(&found("v0.2.0", URL, Some("Fixes")), Version(0, 1, 0)),
            Availability::Available(AvailableRelease {
                tag: "v0.2.0".into(),
                version: Version(0, 2, 0),
                url: URL.into(),
                notes: "Fixes".into(),
            })
        );
    }

    #[test]
    fn the_current_or_an_older_release_is_up_to_date() {
        for tag in ["v0.2.0", "v0.1.9"] {
            assert_eq!(
                assess(&found(tag, URL, None), Version(0, 2, 0)),
                Availability::UpToDate
            );
        }
    }

    #[test]
    fn answers_that_cannot_be_offered_are_not_shown_as_available() {
        let current = Version(0, 1, 0);
        let foreign = "https://example.com/Pulsar/releases/tag/v9.0.0";
        for outcome in [
            found("v9.0.0", foreign, None),
            found("nightly", URL, None),
            found("v9.0.0-rc.1", URL, None),
        ] {
            assert!(matches!(assess(&outcome, current), Availability::Failed(_)));
        }
        let mut pre = release("v9.0.0", URL);
        pre.prerelease = true;
        assert_eq!(
            assess(&Outcome::Found(pre), current),
            Availability::UpToDate
        );
        assert_eq!(
            assess(&Outcome::NoRelease, current),
            Availability::NoRelease
        );
        assert_eq!(
            assess(&Outcome::Failed("offline".into()), current),
            Availability::Failed("offline".into())
        );
    }

    #[test]
    fn notes_lose_control_characters_and_stay_bounded() {
        assert_eq!(notes_text("  a\r\n\tb\u{1b}[31m\u{7}  "), "a\n\tb[31m");
        let long = "é".repeat(MAX_NOTES_CHARS + 5);
        let short = notes_text(&long);
        assert_eq!(short.chars().count(), MAX_NOTES_CHARS + 1);
        assert!(short.ends_with('…'));
    }

    #[test]
    fn a_manual_check_leaves_the_announcement_to_the_daily_check() {
        let mut state = UpdateState::default();
        let found = Outcome::Found(release("v0.2.0", URL));
        note_manual_check(&mut state, &found, Version(0, 1, 0), 5000);
        assert_eq!(state, UpdateState::default());
        assert!(state.due(5000));
        assert!(apply(&mut state, &found, Version(0, 1, 0), 5001).is_some());
    }

    #[test]
    fn a_manual_check_counts_once_nothing_is_left_to_announce() {
        let mut state = UpdateState {
            last_check: 100,
            announced: Some("v0.2.0".into()),
        };
        let announced = Outcome::Found(release("v0.2.0", URL));
        note_manual_check(&mut state, &announced, Version(0, 1, 0), 5000);
        assert_eq!(state.last_check, 5000);
        assert_eq!(state.announced.as_deref(), Some("v0.2.0"));

        note_manual_check(&mut state, &announced, Version(0, 2, 0), 6000);
        assert_eq!(state.last_check, 6000);
        note_manual_check(&mut state, &Outcome::NoRelease, Version(0, 2, 0), 7000);
        assert_eq!(state.last_check, 7000);
    }

    #[test]
    fn a_failed_manual_check_changes_nothing() {
        let mut state = UpdateState {
            last_check: 100,
            announced: None,
        };
        let failed = Outcome::Failed("offline".into());
        note_manual_check(&mut state, &failed, Version(0, 1, 0), 5000);
        assert_eq!(state.last_check, 100);
    }

    #[test]
    fn a_missing_helper_is_a_failed_check() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            run_helper(&dir.path().join("pulsar.exe")),
            Outcome::Failed(_)
        ));
    }
}
