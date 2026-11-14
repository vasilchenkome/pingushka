use pingushka_domain::Timestamp;
use pingushka_domain::identity::TargetId;
use pingushka_domain::target::{
    CheckIntervalSecs, HttpUrl, ProbeHost, Target, TargetConfig, TargetName, TargetSpec, TimeoutMs,
};

fn valid_target(
    timeout_ms: u32,
    interval_secs: u32,
) -> Result<Target, pingushka_domain::target::TargetError> {
    let config = TargetConfig::new(
        TargetName::new("api status")?,
        TargetSpec::http(HttpUrl::parse("https://example.test/health")?),
        CheckIntervalSecs::new(interval_secs)?,
        TimeoutMs::new(timeout_ms)?,
    )?;
    Ok(Target::new(
        TargetId::new(),
        config,
        Timestamp::from_unix_timestamp(1_700_000_000).expect("valid test timestamp"),
    ))
}

#[test]
fn check_interval_accepts_inclusive_supported_range() {
    assert!(CheckIntervalSecs::new(5).is_ok());
    assert!(CheckIntervalSecs::new(3600).is_ok());
}

#[test]
fn check_interval_rejects_values_outside_supported_range() {
    assert!(CheckIntervalSecs::new(4).is_err());
    assert!(CheckIntervalSecs::new(3601).is_err());
}

#[test]
fn timeout_must_be_shorter_than_interval_in_milliseconds() {
    assert!(valid_target(4999, 5).is_ok());
    assert!(valid_target(5000, 5).is_err());
}

#[test]
fn http_url_accepts_only_absolute_http_or_https_urls_with_host() {
    assert!(HttpUrl::parse("http://127.0.0.1:8080/health").is_ok());
    assert!(HttpUrl::parse("https://example.test/health").is_ok());
    assert!(HttpUrl::parse("ftp://example.test/file").is_err());
    assert!(HttpUrl::parse("https://").is_err());
}

#[test]
fn tcp_targets_require_a_valid_host_and_nonzero_port() {
    let host = ProbeHost::parse("example.test").unwrap();
    assert!(TargetSpec::tcp(host.clone(), 443).is_ok());
    assert!(TargetSpec::tcp(host, 0).is_err());
    assert!(ProbeHost::parse("   ").is_err());
}

#[test]
fn target_name_is_trimmed_and_cannot_be_blank() {
    let name = TargetName::new("  database  ").expect("valid name");
    assert_eq!(name.as_str(), "database");
    assert!(TargetName::new("   ").is_err());
}

#[test]
fn timeout_accepts_inclusive_supported_range() {
    assert!(TimeoutMs::new(100).is_ok());
    assert!(TimeoutMs::new(30_000).is_ok());
    assert!(TimeoutMs::new(99).is_err());
    assert!(TimeoutMs::new(30_001).is_err());
}

#[test]
fn restored_target_keeps_persisted_state() {
    let created_at = Timestamp::from_unix_timestamp(1_700_000_000).unwrap();
    let updated_at = Timestamp::from_unix_timestamp(1_700_000_010).unwrap();
    let config = TargetConfig::new(
        TargetName::new("api status").unwrap(),
        TargetSpec::http(HttpUrl::parse("https://example.test/health").unwrap()),
        CheckIntervalSecs::new(60).unwrap(),
        TimeoutMs::new(5000).unwrap(),
    )
    .unwrap();
    let target = Target::restore(TargetId::new(), config, false, created_at, updated_at).unwrap();

    assert!(!target.is_enabled());
    assert_eq!(target.created_at(), created_at);
    assert_eq!(target.updated_at(), updated_at);
}

#[test]
fn restored_target_rejects_updated_time_before_creation() {
    let created_at = Timestamp::from_unix_timestamp(1_700_000_000).unwrap();
    let earlier = Timestamp::from_unix_timestamp(1_699_999_999).unwrap();
    let config = TargetConfig::new(
        TargetName::new("api status").unwrap(),
        TargetSpec::http(HttpUrl::parse("https://example.test/health").unwrap()),
        CheckIntervalSecs::new(60).unwrap(),
        TimeoutMs::new(5000).unwrap(),
    )
    .unwrap();
    let target = Target::restore(TargetId::new(), config, true, created_at, earlier);

    assert!(target.is_err());
}
