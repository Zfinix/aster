use super::*;

use tracing_subscriber::Registry;

#[test]
fn a_layer_builds_without_a_collector_listening() {
    let built = layer::<Registry>("aster-test", "http://127.0.0.1:4318");
    assert!(built.is_ok(), "{:?}", built.err());
    let (_layer, telemetry) = built.unwrap();
    telemetry.shutdown();
}

#[test]
fn shutdown_is_safe_to_repeat() {
    let (_layer, telemetry) = layer::<Registry>("aster-test", "http://127.0.0.1:4318").unwrap();
    telemetry.shutdown();
    telemetry.shutdown();
}

#[test]
fn the_signal_path_is_appended_exactly_once() {
    assert_eq!(
        traces_url("http://localhost:4318"),
        "http://localhost:4318/v1/traces"
    );
    assert_eq!(
        traces_url("http://localhost:4318/"),
        "http://localhost:4318/v1/traces"
    );
    assert_eq!(
        traces_url("http://localhost:4318/v1/traces"),
        "http://localhost:4318/v1/traces"
    );
    assert_eq!(
        traces_url("https://api.honeycomb.io"),
        "https://api.honeycomb.io/v1/traces"
    );
}

#[test]
fn an_unset_or_blank_endpoint_reads_as_none() {
    // Reading, never writing: the environment is shared by every test thread.
    match std::env::var(ENDPOINT_VAR) {
        Ok(set) if !set.trim().is_empty() => assert_eq!(endpoint_from_env(), Some(set)),
        _ => assert_eq!(endpoint_from_env(), None),
    }
}
