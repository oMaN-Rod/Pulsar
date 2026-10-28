use std::path::Path;

#[test]
fn catalogs_match_the_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dirs = ["src", "ui"];
    if let Err(problems) = pulsar_core::l10n::catalog::sync(root, env!("CARGO_PKG_NAME"), &dirs) {
        panic!("{problems}");
    }
}
