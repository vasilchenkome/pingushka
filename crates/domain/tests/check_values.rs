use pingushka_domain::check::StatusCode;

#[test]
fn status_code_accepts_http_range_boundaries() {
    assert_eq!(StatusCode::new(100).expect("valid status").get(), 100);
    assert_eq!(StatusCode::new(599).expect("valid status").get(), 599);
}

#[test]
fn status_code_rejects_values_outside_http_range() {
    assert!(StatusCode::new(99).is_err());
    assert!(StatusCode::new(600).is_err());
}
