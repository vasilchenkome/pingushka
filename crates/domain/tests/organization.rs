use pingushka_domain::organization::{
    MAX_GROUP_LEN, MAX_TAG_LEN, MAX_TAGS, TargetGroupName, TargetTag, TargetTagSet,
};
use pingushka_domain::target::TargetError;

#[test]
fn group_names_are_trimmed_and_validated() {
    assert_eq!(TargetGroupName::new("  Neva  ").unwrap().as_str(), "Neva");
    assert_eq!(TargetGroupName::new("   "), Err(TargetError::EmptyGroup));
    let too_long = "a".repeat(MAX_GROUP_LEN + 1);
    assert_eq!(
        TargetGroupName::new(too_long),
        Err(TargetError::GroupTooLong)
    );
    assert_eq!(
        TargetGroupName::new("bad\nname"),
        Err(TargetError::InvalidGroup)
    );
}

#[test]
fn tags_are_lowercased_unique_and_sorted() {
    let tags = TargetTagSet::parse(["Prod", " api ", "prod"]).unwrap();
    let values = tags
        .as_slice()
        .iter()
        .map(TargetTag::as_str)
        .collect::<Vec<_>>();
    assert_eq!(values, vec!["api", "prod"]);
    assert_eq!(tags.len(), 2);
}

#[test]
fn tags_reject_empty_whitespace_and_overlong_values() {
    assert_eq!(TargetTag::new("  "), Err(TargetError::EmptyTag));
    assert_eq!(TargetTag::new("two words"), Err(TargetError::InvalidTag));
    assert_eq!(TargetTag::new("bad/tag"), Err(TargetError::InvalidTag));
    assert_eq!(
        TargetTag::new("a".repeat(MAX_TAG_LEN + 1)),
        Err(TargetError::TagTooLong)
    );
}

#[test]
fn tag_set_is_limited_to_ten_values() {
    let values = (0..=MAX_TAGS)
        .map(|index| format!("tag-{index}"))
        .collect::<Vec<_>>();
    assert_eq!(TargetTagSet::parse(values), Err(TargetError::TooManyTags));
}
