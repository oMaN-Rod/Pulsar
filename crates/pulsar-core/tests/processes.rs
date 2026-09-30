mod common;

use common::sample_twice;
use pulsar_core::sources::ProcessesSource;

#[test]
fn processes_include_this_test_binary() {
    let snap = sample_twice(&mut ProcessesSource::new().unwrap());
    let exe = std::env::current_exe().unwrap();
    let name = exe.file_stem().unwrap().to_string_lossy();
    let me = snap
        .processes
        .iter()
        .find(|p| p.name == name)
        .expect("own process listed");
    assert!(me.private_bytes > 0.0);
}
