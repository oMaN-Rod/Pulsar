fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
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
