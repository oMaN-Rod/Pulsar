use std::thread::sleep;
use std::time::Duration;

use perfbar_core::pdh::Query;

#[test]
fn reads_a_real_counter() {
    let query = Query::new().unwrap();
    let counter = query
        .add(r"\Processor Information(_Total)\% Processor Utility")
        .unwrap();
    query.collect().unwrap();
    sleep(Duration::from_millis(500));
    query.collect().unwrap();
    let value = counter.value().unwrap();
    assert!(value >= 0.0, "{value}");
}

#[test]
fn reads_a_wildcard_array() {
    let query = Query::new().unwrap();
    let counter = query
        .add(r"\Processor Information(*)\% Processor Utility")
        .unwrap();
    query.collect().unwrap();
    sleep(Duration::from_millis(500));
    query.collect().unwrap();
    let items = counter.array().unwrap();
    assert!(items.iter().any(|(name, _)| name == "_Total"), "{items:?}");
}

#[test]
fn unknown_counter_is_an_error() {
    let query = Query::new().unwrap();
    assert!(query.add(r"\No Such Object(*)\Nothing").is_err());
}
