use pulsar_core::config::Config;
use pulsar_core::diag;

#[test]
fn the_report_lists_real_readings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.txt");
    let saved = diag::save(&Config::default(), "Pulsar test", Some(&path)).unwrap();
    let text = std::fs::read_to_string(saved).unwrap();
    assert!(text.starts_with("Pulsar test\n"));
    for expected in [
        "CpuTotal = ",
        "MemTotalBytes = ",
        "UptimeSeconds = ",
        "Processes: ",
    ] {
        assert!(text.contains(expected), "no {expected:?} in\n{text}");
    }
    assert!(
        !text.contains("PingMs"),
        "no pinging unless the ping item is on"
    );
}
