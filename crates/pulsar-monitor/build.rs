fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/settings.slint", config).expect("compile settings.slint");
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
    for bin in ["pulsar", "pulsar-settings"] {
        embed_resource::compile_for(format!("resources/{bin}.rc"), [bin], &macros)
            .manifest_optional()
            .expect("compile the Windows resources");
    }
}
