use std::path::Path;

#[test]
fn catalogs_match_the_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Err(problems) = pulsar_core::l10n::catalog::sync(root, env!("CARGO_PKG_NAME"), &["src"])
    {
        panic!("{problems}");
    }
}

/// The installer closes the settings window by its title, in whatever
/// language Pulsar uses; skipped where the installer is not alongside.
#[test]
fn the_installer_knows_every_settings_title() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../installer/pulsar.iss");
    let Ok(script) = std::fs::read_to_string(script) else {
        return;
    };
    for language in &pulsar_core::l10n::LANGUAGES {
        pulsar_core::l10n::set_language(Some(language.code));
        let title = pulsar_core::ipc::settings_title();
        assert!(
            script.contains(&format!("'{title}'")),
            "installer/pulsar.iss does not look for '{title}'"
        );
    }
}
