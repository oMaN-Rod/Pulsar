use std::path::Path;

/// Lists `locales/<code>/LC_MESSAGES/pulsar-monitor.po` for `language.rs`;
/// Slint bundles the same catalogs for the `.slint` files.
fn write_catalog_list() {
    let package = std::env::var("CARGO_PKG_NAME").expect("set by cargo");
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    println!("cargo:rerun-if-changed=locales");
    let mut catalogs: Vec<(String, String)> = std::fs::read_dir(Path::new(&root).join("locales"))
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

fn main() {
    write_catalog_list();
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_bundled_translations("locales")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/settings.slint", config.clone())
        .expect("compile settings.slint");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("set by cargo"));
    slint_build::compile_with_output_path("ui/details.slint", out.join("details.rs"), config)
        .expect("compile details.slint");
    println!("cargo:rerun-if-changed=ui/details.slint");
    // Only the update helper (`pulsar.exe --check-update`) uses WinHTTP;
    // delay-loading keeps it out of the resident process.
    println!("cargo:rustc-link-arg-bin=pulsar=/DELAYLOAD:winhttp.dll");
    println!("cargo:rustc-link-arg-bin=pulsar=delayimp.lib");
    println!("cargo:rerun-if-changed=assets/pulsar.ico");
    println!("cargo:rerun-if-changed=resources");
    let version = std::env::var("CARGO_PKG_VERSION").expect("set by cargo");
    let parts: Vec<&str> = version.split(['.', '-', '+']).collect();
    let macros = [
        format!("VER_MAJOR={}", parts[0]),
        format!("VER_MINOR={}", parts[1]),
        format!("VER_PATCH={}", parts[2]),
        format!("VER_STRING=\"{version}\""),
    ];
    for bin in [
        "pulsar",
        "pulsar-settings",
        "pulsar-details",
        "pulsar-sensors",
    ] {
        embed_resource::compile_for(format!("resources/{bin}.rc"), [bin], &macros)
            .manifest_optional()
            .expect("compile the Windows resources");
    }
}
