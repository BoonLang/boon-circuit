// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.
#[test]
fn elapsed_delta_ms_only_reports_forward_time() {
    assert_eq!(elapsed_delta_ms(Some(10.0), Some(14.5)), Some(4.5));
    assert_eq!(elapsed_delta_ms(Some(14.5), Some(10.0)), None);
    assert_eq!(elapsed_delta_ms(None, Some(10.0)), None);
    assert_eq!(elapsed_delta_ms(Some(10.0), None), None);
}
