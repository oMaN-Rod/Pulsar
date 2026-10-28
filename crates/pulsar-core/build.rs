//! Lists `locales/<code>/LC_MESSAGES/pulsar-monitor-core.po` for `l10n`.

use std::path::Path;

fn main() {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    let package = std::env::var("CARGO_PKG_NAME").expect("set by cargo");
    let locales = Path::new(&root).join("locales");
    println!("cargo:rerun-if-changed=locales");
    let mut catalogs: Vec<(String, String)> = std::fs::read_dir(&locales)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let po = entry
                .path()
                .join("LC_MESSAGES")
                .join(format!("{package}.po"));
            po.is_file().then(|| {
                let code = entry.file_name().to_string_lossy().into_owned();
                (code, po.display().to_string())
            })
        })
        .collect();
    catalogs.sort();
    let mut out = String::from("&[\n");
    for (code, path) in catalogs {
        out.push_str(&format!("    ({code:?}, include_str!({path:?})),\n"));
    }
    out.push_str("]\n");
    let dest = Path::new(&std::env::var("OUT_DIR").expect("set by cargo")).join("catalogs.rs");
    std::fs::write(dest, out).expect("write the catalog list");
}
