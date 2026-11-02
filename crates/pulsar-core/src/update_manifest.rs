//! Decides whether a downloaded update may be trusted: a minisign-signed
//! manifest binds a version, architecture, asset name and digest. Nothing here
//! fetches, writes or runs a file.

use std::collections::HashSet;
use std::path::Path;

use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;
use thiserror::Error;
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};

use crate::hardware::registry;
use crate::project::RELEASES_URL;
use crate::update::{Release, Version, is_final_from_this_repository};

/// Minisign public keys, base64. Two slots so a release can be signed by a new
/// key while installed copies still trust the old one. An empty slot trusts
/// nothing, and with both empty every manifest is rejected.
pub const KEY_SLOTS: [&str; 2] = ["", ""];

const FORMAT: u32 = 1;
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{8F3B6C1E-3D8A-4B8E-9C61-5A2E7D4F0B19}_is1";

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum Rejected {
    #[error("no update signing key is configured")]
    NoKey,
    #[error("the manifest signature is not valid")]
    BadSignature,
    #[error("the manifest cannot be read: {0}")]
    BadManifest(String),
    #[error("this copy of Pulsar is not the installed one")]
    NotInstalled,
    #[error("the release cannot be offered as an update")]
    ReleaseNotOffered,
    #[error("the manifest is for {manifest}, but the release is {release}")]
    VersionMismatch { manifest: String, release: String },
    #[error("{0} is not newer than this version")]
    NotNewer(Version),
    #[error("the manifest has no installer for {0}")]
    NoAssetForArchitecture(String),
    #[error("the release does not contain {0}")]
    AssetNotInRelease(String),
    #[error("the file does not match the signed digest")]
    DigestMismatch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    format: u32,
    version: String,
    assets: Vec<RawAsset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAsset {
    name: String,
    architecture: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ManifestAsset {
    name: String,
    architecture: String,
    sha256: [u8; 32],
}

/// A manifest whose signature checked out; only [`verify_manifest`] makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedManifest {
    version: Version,
    assets: Vec<ManifestAsset>,
}

impl VerifiedManifest {
    pub fn version(&self) -> Version {
        self.version
    }
}

/// The installer a release may update this copy with; only [`authorize`] makes
/// one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedAsset {
    version: Version,
    name: String,
    sha256: [u8; 32],
}

impl AuthorizedAsset {
    pub fn version(&self) -> Version {
        self.version
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn url(&self) -> String {
        asset_url(self.version, &self.name)
    }

    pub fn verify_bytes(&self, bytes: &[u8]) -> Result<(), Rejected> {
        match sha256(bytes) {
            Some(digest) if digest == self.sha256 => Ok(()),
            _ => Err(Rejected::DigestMismatch),
        }
    }
}

/// True once a slot holds a usable key. Until then Pulsar only ever points to
/// the release page.
pub fn signed_updates_enabled(slots: &[&str]) -> bool {
    !usable_keys(slots).is_empty()
}

fn usable_keys(slots: &[&str]) -> Vec<PublicKey> {
    slots
        .iter()
        .filter_map(|slot| PublicKey::from_base64(slot).ok())
        .collect()
}

/// Checks the signature first; the text is not parsed unless it is signed.
/// Only pre-hashed signatures count, which is what `minisign -S` writes.
pub fn verify_manifest(
    manifest: &[u8],
    signature: &str,
    slots: &[&str],
) -> Result<VerifiedManifest, Rejected> {
    let keys = usable_keys(slots);
    if keys.is_empty() {
        return Err(Rejected::NoKey);
    }
    let signature = Signature::decode(signature).map_err(|_| Rejected::BadSignature)?;
    if !keys
        .iter()
        .any(|key| key.verify(manifest, &signature, false).is_ok())
    {
        return Err(Rejected::BadSignature);
    }
    parse_manifest(manifest)
}

fn parse_manifest(manifest: &[u8]) -> Result<VerifiedManifest, Rejected> {
    let bad = |reason: &str| Rejected::BadManifest(reason.to_string());
    let raw: RawManifest =
        serde_json::from_slice(manifest).map_err(|e| Rejected::BadManifest(e.to_string()))?;
    if raw.format != FORMAT {
        return Err(bad("unsupported format"));
    }
    let version = Version::parse(&raw.version)
        .filter(|v| v.to_string() == raw.version)
        .ok_or_else(|| bad("invalid version"))?;
    let prefix = format!("pulsar-{version}-");
    let mut names = HashSet::new();
    let mut assets = Vec::with_capacity(raw.assets.len());
    for asset in raw.assets {
        if !is_plain_file_name(&asset.name) || !asset.name.starts_with(&prefix) {
            return Err(bad("an asset name does not fit the version"));
        }
        if !names.insert(asset.name.clone()) {
            return Err(bad("an asset is listed twice"));
        }
        let sha256 = parse_digest(&asset.sha256).ok_or_else(|| bad("invalid digest"))?;
        assets.push(ManifestAsset {
            name: asset.name,
            architecture: asset.architecture,
            sha256,
        });
    }
    Ok(VerifiedManifest { version, assets })
}

fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Lowercase hex only, as `SHA256SUMS.txt` writes it.
fn parse_digest(hex: &str) -> Option<[u8; 32]> {
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    if hex.len() != 64 {
        return None;
    }
    let mut digest = [0u8; 32];
    for (byte, &[high, low]) in digest.iter_mut().zip(hex.as_bytes().as_chunks::<2>().0) {
        *byte = digit(high)? << 4 | digit(low)?;
    }
    Some(digest)
}

fn sha256(bytes: &[u8]) -> Option<[u8; 32]> {
    u32::try_from(bytes.len()).ok()?;
    let mut digest = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) }
        .is_ok()
        .then_some(digest)
}

pub fn installer_name(version: Version) -> String {
    format!("pulsar-{version}-setup.exe")
}

pub fn manifest_name(version: Version) -> String {
    format!("pulsar-{version}-manifest.json")
}

pub fn signature_name(version: Version) -> String {
    format!("{}.minisig", manifest_name(version))
}

/// Built from a verified version and a fixed name, never a URL from elsewhere.
pub fn asset_url(version: Version, name: &str) -> String {
    format!("{RELEASES_URL}download/v{version}/{name}")
}

#[derive(Clone, Copy, Debug)]
pub struct UpdateContext<'a> {
    pub release: &'a Release,
    pub current: Version,
    pub architecture: &'a str,
    pub install: InstallKind,
}

/// Verifies the manifest against the release it came with and picks the
/// installer. The manifest's own version must be the release's and newer than
/// this copy, so an older signed manifest cannot be replayed under a new tag.
pub fn authorize(
    manifest: &[u8],
    signature: &str,
    slots: &[&str],
    context: UpdateContext<'_>,
) -> Result<AuthorizedAsset, Rejected> {
    let manifest = verify_manifest(manifest, signature, slots)?;
    select_asset(&manifest, context)
}

pub fn select_asset(
    manifest: &VerifiedManifest,
    context: UpdateContext<'_>,
) -> Result<AuthorizedAsset, Rejected> {
    if context.install != InstallKind::Installed {
        return Err(Rejected::NotInstalled);
    }
    let release = context.release;
    if !is_final_from_this_repository(release) {
        return Err(Rejected::ReleaseNotOffered);
    }
    if release.tag_name != format!("v{}", manifest.version) {
        return Err(Rejected::VersionMismatch {
            manifest: manifest.version.to_string(),
            release: release.tag_name.clone(),
        });
    }
    if manifest.version <= context.current {
        return Err(Rejected::NotNewer(manifest.version));
    }
    let name = installer_name(manifest.version);
    let asset = manifest
        .assets
        .iter()
        .find(|a| a.name == name && a.architecture == context.architecture)
        .ok_or_else(|| Rejected::NoAssetForArchitecture(context.architecture.to_string()))?;
    if !release.assets.iter().any(|a| a.name == name) {
        return Err(Rejected::AssetNotInRelease(name));
    }
    Ok(AuthorizedAsset {
        version: manifest.version,
        name,
        sha256: asset.sha256,
    })
}

/// Portable copies and `cargo install`s are never replaced; a portable copy can
/// sit beside a real install, so being installed is about this very executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    Installed,
    Standalone,
}

/// Anything uncertain counts as standalone.
pub fn install_kind(exe: &Path, install_location: Option<&str>) -> InstallKind {
    let installed = install_location
        .map(Path::new)
        .filter(|location| location.is_absolute())
        .and_then(|location| {
            let location = location.canonicalize().ok()?;
            let exe = exe.canonicalize().ok()?;
            Some(exe.parent() == Some(location.as_path()))
        })
        .unwrap_or(false);
    if installed {
        InstallKind::Installed
    } else {
        InstallKind::Standalone
    }
}

pub fn detect_install() -> InstallKind {
    let Ok(exe) = std::env::current_exe() else {
        return InstallKind::Standalone;
    };
    install_kind(
        &exe,
        registry::user_string(UNINSTALL_KEY, "InstallLocation").as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use blake2::{Blake2b512, Digest};
    use ed25519_compact::{KeyPair, Seed};

    use super::*;
    use crate::update::ReleaseAsset;

    const SEED_A: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const SEED_B: &str = "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb";
    const SEED_C: &str = "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7";
    const CURRENT: Version = Version(0, 6, 0);
    const ARCH: &str = "x86_64";

    /// Signs like `minisign -S`. The seeds are RFC 8032's published test
    /// vectors, so these keys can never protect a real release.
    struct TestKey {
        pair: KeyPair,
        id: [u8; 8],
    }

    impl TestKey {
        fn new(seed: &str, id: u8) -> Self {
            Self {
                pair: KeyPair::from_seed(Seed::new(parse_digest(seed).unwrap())),
                id: [id; 8],
            }
        }

        fn public(&self) -> String {
            base64(&[b"Ed".as_slice(), &self.id, self.pair.pk.as_ref()].concat())
        }

        fn sign_with_comment(&self, message: &[u8], comment: &str) -> (String, String) {
            let prehash = Blake2b512::digest(message);
            let signature = self.pair.sk.sign(prehash.as_slice(), None);
            let global = self
                .pair
                .sk
                .sign([signature.as_ref(), comment.as_bytes()].concat(), None);
            (
                base64(&[b"ED".as_slice(), &self.id, signature.as_ref()].concat()),
                base64(global.as_ref()),
            )
        }

        fn sign(&self, message: &[u8]) -> String {
            let (signature, global) = self.sign_with_comment(message, "test");
            format!("untrusted comment: test\n{signature}\ntrusted comment: test\n{global}\n")
        }
    }

    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let group = chunk.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b))
                << (8 * (3 - chunk.len()));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(group >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn key_a() -> TestKey {
        TestKey::new(SEED_A, 0xA)
    }

    fn key_b() -> TestKey {
        TestKey::new(SEED_B, 0xB)
    }

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn digest_of(bytes: &[u8]) -> String {
        hex_of(&sha256(bytes).unwrap())
    }

    fn manifest_json(version: &str, name: &str, architecture: &str, sha256: &str) -> String {
        format!(
            r#"{{"format":1,"version":"{version}","assets":[{{"name":"{name}","architecture":"{architecture}","sha256":"{sha256}"}}]}}"#
        )
    }

    fn manifest_for(version: &str, artifact: &[u8]) -> String {
        manifest_json(
            version,
            &format!("pulsar-{version}-setup.exe"),
            ARCH,
            &digest_of(artifact),
        )
    }

    fn release(tag: &str) -> Release {
        let version = tag.trim_start_matches('v');
        Release {
            tag_name: tag.into(),
            html_url: format!("{RELEASES_URL}tag/{tag}"),
            draft: false,
            prerelease: false,
            body: None,
            assets: vec![ReleaseAsset {
                name: format!("pulsar-{version}-setup.exe"),
            }],
        }
    }

    fn context(release: &Release) -> UpdateContext<'_> {
        UpdateContext {
            release,
            current: CURRENT,
            architecture: ARCH,
            install: InstallKind::Installed,
        }
    }

    fn signed(manifest: &str) -> Result<VerifiedManifest, Rejected> {
        let key = key_a();
        verify_manifest(
            manifest.as_bytes(),
            &key.sign(manifest.as_bytes()),
            &[&key.public(), ""],
        )
    }

    fn authorized(manifest: &str, release: &Release) -> Result<AuthorizedAsset, Rejected> {
        let key = key_a();
        authorize(
            manifest.as_bytes(),
            &key.sign(manifest.as_bytes()),
            &[&key.public(), ""],
            context(release),
        )
    }

    fn assert_bad_manifest(result: Result<VerifiedManifest, Rejected>) {
        assert!(
            matches!(result, Err(Rejected::BadManifest(_))),
            "{result:?}"
        );
    }

    #[test]
    fn the_test_keys_are_the_published_rfc_8032_vectors() {
        for (seed, public) in [
            (
                SEED_A,
                "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            ),
            (
                SEED_B,
                "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            ),
            (
                SEED_C,
                "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
            ),
        ] {
            assert_eq!(hex_of(TestKey::new(seed, 0).pair.pk.as_ref()), public);
        }
    }

    #[test]
    fn embedded_slots_are_empty_or_hold_a_real_key() {
        for slot in KEY_SLOTS {
            assert!(slot.is_empty() || PublicKey::from_base64(slot).is_ok());
        }
    }

    #[test]
    fn with_no_key_a_well_signed_manifest_is_rejected() {
        let key = key_a();
        let manifest = manifest_for("0.7.0", b"setup");
        let signature = key.sign(manifest.as_bytes());
        for slots in [&["", ""], &["not a key", "RWQ="]] {
            assert!(!signed_updates_enabled(slots));
            assert_eq!(
                verify_manifest(manifest.as_bytes(), &signature, slots),
                Err(Rejected::NoKey)
            );
        }
        assert_eq!(
            verify_manifest(manifest.as_bytes(), &signature, &[]),
            Err(Rejected::NoKey)
        );
    }

    #[test]
    fn test_signatures_never_pass_the_embedded_slots() {
        let manifest = manifest_for("0.7.0", b"setup");
        let signature = key_a().sign(manifest.as_bytes());
        assert!(verify_manifest(manifest.as_bytes(), &signature, &KEY_SLOTS).is_err());
    }

    #[test]
    fn a_configured_key_turns_signed_updates_on() {
        assert!(signed_updates_enabled(&[&key_a().public(), ""]));
        assert!(signed_updates_enabled(&["", &key_b().public()]));
    }

    #[test]
    fn real_minisign_output_is_accepted_and_its_legacy_form_is_not() {
        const PUBLIC: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        const PREHASHED: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
        const LEGACY: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";
        assert!(matches!(
            verify_manifest(b"test", PREHASHED, &[PUBLIC]),
            Err(Rejected::BadManifest(_))
        ));
        assert_eq!(
            verify_manifest(b"tess", PREHASHED, &[PUBLIC]),
            Err(Rejected::BadSignature)
        );
        assert_eq!(
            verify_manifest(b"test", LEGACY, &[PUBLIC]),
            Err(Rejected::BadSignature)
        );
    }

    #[test]
    fn either_key_slot_can_sign_during_a_rotation() {
        let (old, new) = (key_a(), key_b());
        let manifest = manifest_for("0.7.0", b"setup");
        let slots = [old.public(), new.public()];
        let slots = [slots[0].as_str(), slots[1].as_str()];
        for key in [&old, &new] {
            let signature = key.sign(manifest.as_bytes());
            assert!(verify_manifest(manifest.as_bytes(), &signature, &slots).is_ok());
        }
    }

    #[test]
    fn a_retired_or_unknown_key_is_rejected() {
        let (old, new) = (key_a(), key_b());
        let manifest = manifest_for("0.7.0", b"setup");
        assert_eq!(
            verify_manifest(
                manifest.as_bytes(),
                &old.sign(manifest.as_bytes()),
                &["", &new.public()]
            ),
            Err(Rejected::BadSignature)
        );
        let impostor = TestKey {
            pair: TestKey::new(SEED_C, 0).pair,
            id: new.id,
        };
        assert_eq!(
            verify_manifest(
                manifest.as_bytes(),
                &impostor.sign(manifest.as_bytes()),
                &["", &new.public()]
            ),
            Err(Rejected::BadSignature)
        );
    }

    #[test]
    fn a_changed_manifest_fails_its_signature() {
        let key = key_a();
        let manifest = manifest_for("0.7.0", b"setup");
        let signature = key.sign(manifest.as_bytes());
        let edited = manifest.replace("0.7.0", "0.8.0");
        assert_eq!(
            verify_manifest(edited.as_bytes(), &signature, &[&key.public()]),
            Err(Rejected::BadSignature)
        );
    }

    #[test]
    fn a_changed_trusted_comment_fails_the_signature() {
        let key = key_a();
        let manifest = manifest_for("0.7.0", b"setup");
        let (signature, global) = key.sign_with_comment(manifest.as_bytes(), "test");
        let edited =
            format!("untrusted comment: x\n{signature}\ntrusted comment: edited\n{global}\n");
        assert_eq!(
            verify_manifest(manifest.as_bytes(), &edited, &[&key.public()]),
            Err(Rejected::BadSignature)
        );
    }

    #[test]
    fn a_malformed_signature_is_rejected() {
        let key = key_a();
        for signature in ["", "untrusted comment: x\n", "a\nb\nc\nd\n"] {
            assert_eq!(
                verify_manifest(b"{}", signature, &[&key.public()]),
                Err(Rejected::BadSignature)
            );
        }
    }

    #[test]
    fn a_signed_manifest_is_still_held_to_its_format() {
        let digest = digest_of(b"setup");
        let name = "pulsar-0.7.0-setup.exe";
        assert!(signed(&manifest_json("0.7.0", name, ARCH, &digest)).is_ok());
        for manifest in [
            "not json".to_string(),
            "{}".to_string(),
            manifest_json("0.7.0", name, ARCH, &digest).replace("\"format\":1", "\"format\":2"),
            manifest_json("0.7.0", name, ARCH, &digest)
                .replace("\"format\"", "\"extra\":1,\"format\""),
            manifest_json("v0.7.0", name, ARCH, &digest),
            manifest_json("0.7.0-rc.1", name, ARCH, &digest),
            manifest_json("00.7.0", name, ARCH, &digest),
            manifest_json("0.7.0", name, ARCH, &digest[..62]),
            manifest_json("0.7.0", name, ARCH, &digest.to_uppercase()),
            manifest_json("0.7.0", name, ARCH, &format!("zz{}", &digest[2..])),
            manifest_json("0.7.0", "../pulsar-0.7.0-setup.exe", ARCH, &digest),
            manifest_json("0.7.0", "pulsar-0.7.0-../setup.exe", ARCH, &digest),
            manifest_json("0.7.0", r"pulsar-0.7.0-a\setup.exe", ARCH, &digest),
            manifest_json("0.7.0", "pulsar-0.7.0-a b.exe", ARCH, &digest),
            manifest_json("0.7.0", "other.exe", ARCH, &digest),
            manifest_json("0.7.0", "pulsar-0.6.0-setup.exe", ARCH, &digest),
        ] {
            assert_bad_manifest(signed(&manifest));
        }
    }

    #[test]
    fn a_doubled_field_or_asset_is_rejected() {
        let digest = digest_of(b"setup");
        let entry = format!(
            r#"{{"name":"pulsar-0.7.0-setup.exe","architecture":"{ARCH}","sha256":"{digest}"}}"#
        );
        assert_bad_manifest(signed(&format!(
            r#"{{"format":1,"version":"0.7.0","assets":[{entry},{entry}]}}"#
        )));
        assert_bad_manifest(signed(&format!(
            r#"{{"format":1,"version":"0.8.0","version":"0.7.0","assets":[{entry}]}}"#
        )));
    }

    #[test]
    fn the_installer_is_chosen_from_a_good_manifest() {
        let manifest = manifest_for("0.7.0", b"setup");
        let release = release("v0.7.0");
        let asset = authorized(&manifest, &release).unwrap();
        assert_eq!(asset.name(), "pulsar-0.7.0-setup.exe");
        assert_eq!(asset.version(), Version(0, 7, 0));
        assert_eq!(
            asset.url(),
            "https://github.com/oMaN-Rod/Pulsar/releases/download/v0.7.0/pulsar-0.7.0-setup.exe"
        );
        assert_eq!(asset.verify_bytes(b"setup"), Ok(()));
    }

    #[test]
    fn the_portable_zip_is_never_chosen() {
        let digest = digest_of(b"zip");
        let manifest = manifest_json("0.7.0", "pulsar-0.7.0-portable.zip", ARCH, &digest);
        let mut with_zip = release("v0.7.0");
        with_zip.assets.push(ReleaseAsset {
            name: "pulsar-0.7.0-portable.zip".into(),
        });
        assert_eq!(
            authorized(&manifest, &with_zip),
            Err(Rejected::NoAssetForArchitecture(ARCH.into()))
        );
    }

    #[test]
    fn an_older_signed_manifest_fails_under_a_newer_tag() {
        let manifest = manifest_for("0.5.0", b"old setup");
        let mut relabelled = release("v0.7.0");
        relabelled.assets.push(ReleaseAsset {
            name: "pulsar-0.5.0-setup.exe".into(),
        });
        assert_eq!(
            authorized(&manifest, &relabelled),
            Err(Rejected::VersionMismatch {
                manifest: "0.5.0".into(),
                release: "v0.7.0".into(),
            })
        );
    }

    #[test]
    fn an_older_artifact_renamed_to_the_new_name_fails_its_digest() {
        let manifest = manifest_for("0.7.0", b"new setup");
        let asset = authorized(&manifest, &release("v0.7.0")).unwrap();
        assert_eq!(asset.verify_bytes(b"new setup"), Ok(()));
        assert_eq!(
            asset.verify_bytes(b"old setup"),
            Err(Rejected::DigestMismatch)
        );
        assert_eq!(asset.verify_bytes(b""), Err(Rejected::DigestMismatch));
    }

    #[test]
    fn an_older_digest_relabelled_with_a_new_name_needs_a_valid_signature() {
        let key = key_a();
        let genuine = manifest_for("0.5.0", b"old setup");
        let signature = key.sign(genuine.as_bytes());
        let relabelled = genuine.replace("0.5.0", "0.7.0");
        assert_eq!(
            authorize(
                relabelled.as_bytes(),
                &signature,
                &[&key.public()],
                context(&release("v0.7.0")),
            ),
            Err(Rejected::BadSignature)
        );
    }

    #[test]
    fn a_replayed_release_is_not_newer() {
        for version in ["0.6.0", "0.5.0"] {
            let manifest = manifest_for(version, b"setup");
            assert_eq!(
                authorized(&manifest, &release(&format!("v{version}"))),
                Err(Rejected::NotNewer(Version::parse(version).unwrap()))
            );
        }
    }

    #[test]
    fn the_tag_must_be_exactly_the_manifest_version() {
        let manifest = manifest_for("0.7.0", b"setup");
        for tag in ["0.7.0", "v00.7.0", "v0.7.1"] {
            let mut r = release("v0.7.0");
            r.tag_name = tag.into();
            assert!(matches!(
                authorized(&manifest, &r),
                Err(Rejected::VersionMismatch { .. })
            ));
        }
    }

    #[test]
    fn another_architecture_is_rejected() {
        let manifest = manifest_json(
            "0.7.0",
            "pulsar-0.7.0-setup.exe",
            "aarch64",
            &digest_of(b"setup"),
        );
        assert_eq!(
            authorized(&manifest, &release("v0.7.0")),
            Err(Rejected::NoAssetForArchitecture(ARCH.into()))
        );
    }

    #[test]
    fn the_asset_must_be_on_the_release() {
        let manifest = manifest_for("0.7.0", b"setup");
        let mut r = release("v0.7.0");
        r.assets.clear();
        assert_eq!(
            authorized(&manifest, &r),
            Err(Rejected::AssetNotInRelease("pulsar-0.7.0-setup.exe".into()))
        );
    }

    #[test]
    fn drafts_prereleases_and_foreign_releases_are_not_offered() {
        let manifest = manifest_for("0.7.0", b"setup");
        let mut draft = release("v0.7.0");
        draft.draft = true;
        let mut pre = release("v0.7.0");
        pre.prerelease = true;
        let mut foreign = release("v0.7.0");
        foreign.html_url = "https://example.com/Pulsar/releases/tag/v0.7.0".into();
        for r in [draft, pre, foreign] {
            assert_eq!(authorized(&manifest, &r), Err(Rejected::ReleaseNotOffered));
        }
    }

    #[test]
    fn a_standalone_copy_is_never_offered_an_installer() {
        let manifest = signed(&manifest_for("0.7.0", b"setup")).unwrap();
        let release = release("v0.7.0");
        let context = UpdateContext {
            install: InstallKind::Standalone,
            ..context(&release)
        };
        assert_eq!(
            select_asset(&manifest, context),
            Err(Rejected::NotInstalled)
        );
    }

    #[test]
    fn sha256_matches_the_published_vector() {
        assert_eq!(
            digest_of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn asset_names_follow_the_release_files() {
        let version = Version(0, 7, 0);
        assert_eq!(installer_name(version), "pulsar-0.7.0-setup.exe");
        assert_eq!(manifest_name(version), "pulsar-0.7.0-manifest.json");
        assert_eq!(
            signature_name(version),
            "pulsar-0.7.0-manifest.json.minisig"
        );
    }

    fn fake_install(root: &Path) -> (std::path::PathBuf, String) {
        let dir = root.join("Pulsar");
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("pulsar.exe");
        fs::write(&exe, b"").unwrap();
        (exe, format!("{}\\", dir.display()))
    }

    #[test]
    fn the_installed_executable_is_recognised() {
        let root = tempfile::tempdir().unwrap();
        let (exe, location) = fake_install(root.path());
        assert_eq!(install_kind(&exe, Some(&location)), InstallKind::Installed);
        assert_eq!(
            install_kind(&exe, Some(&location.to_uppercase())),
            InstallKind::Installed
        );
    }

    #[test]
    fn a_copy_outside_the_install_location_is_standalone() {
        let root = tempfile::tempdir().unwrap();
        let (exe, location) = fake_install(root.path());
        for other in ["Pulsar2", "Pulsar\\tools", "Portable"] {
            let dir = root.path().join(other);
            fs::create_dir_all(&dir).unwrap();
            let elsewhere = dir.join("pulsar.exe");
            fs::write(&elsewhere, b"").unwrap();
            assert_eq!(
                install_kind(&elsewhere, Some(&location)),
                InstallKind::Standalone,
                "{other}"
            );
        }
        assert_eq!(install_kind(&exe, None), InstallKind::Standalone);
        assert_eq!(install_kind(&exe, Some("")), InstallKind::Standalone);
        assert_eq!(install_kind(&exe, Some("Pulsar")), InstallKind::Standalone);
        let gone = format!("{}\\", root.path().join("gone").display());
        assert_eq!(install_kind(&exe, Some(&gone)), InstallKind::Standalone);
        assert_eq!(
            install_kind(&root.path().join("gone.exe"), Some(&location)),
            InstallKind::Standalone
        );
    }

    #[test]
    fn a_path_through_a_junction_is_the_same_install() {
        let root = tempfile::tempdir().unwrap();
        let (exe, location) = fake_install(root.path());
        let link = root.path().join("Link");
        let made = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(root.path().join("Pulsar"))
            .output()
            .unwrap();
        assert!(made.status.success());
        assert_eq!(
            install_kind(&link.join("pulsar.exe"), Some(&location)),
            InstallKind::Installed
        );
        assert_eq!(
            install_kind(&exe, Some(&format!("{}\\", link.display()))),
            InstallKind::Installed
        );
    }

    #[test]
    fn these_tests_do_not_run_from_an_install() {
        assert_eq!(detect_install(), InstallKind::Standalone);
    }

    #[test]
    fn the_uninstall_key_names_the_installers_app_id() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../installer/pulsar.iss");
        let Ok(script) = fs::read_to_string(script) else {
            return;
        };
        let guid = UNINSTALL_KEY
            .split_once('{')
            .and_then(|(_, rest)| rest.split_once('}'))
            .unwrap()
            .0;
        assert!(script.contains(&format!("#define AppGuid \"{guid}\"")));
        assert!(script.contains("PrivilegesRequired=lowest"));
    }
}
