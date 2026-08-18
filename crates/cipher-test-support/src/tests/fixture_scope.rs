use super::FixtureScope;

const RUN_A: &str = "f83f56c0-b34b-4ca7-8c7a-c4b6985aa45f";
const RUN_B: &str = "61601476-46c6-4a56-ac7a-3374d35a986d";

#[test]
fn creates_scoped_resource_names() {
    let scope = FixtureScope::parse(RUN_A).unwrap();

    assert_eq!(
        scope.resource_id("alice").unwrap(),
        format!("cipher-live-it-{RUN_A}-alice")
    );
    assert_eq!(scope.object_prefix(), format!("fixtures/{RUN_A}/"));
}

#[test]
fn rejects_missing_or_broad_run_ids() {
    for value in [
        "",
        "cipher-live-it-",
        "production",
        "F83F56C0-B34B-4CA7-8C7A-C4B6985AA45F",
    ] {
        assert!(FixtureScope::parse(value).is_err());
    }
}

#[test]
fn rejects_unsafe_labels() {
    let scope = FixtureScope::parse(RUN_A).unwrap();

    for label in ["", "../alice", "Alice", "alice/bob"] {
        assert!(scope.resource_id(label).is_err());
    }
}

#[test]
fn cleanup_requires_both_owned_name_and_exact_marker() {
    let scope = FixtureScope::parse(RUN_A).unwrap();
    let alice = scope.resource_id("alice").unwrap();

    assert!(scope.owns(&alice, RUN_A));
    assert!(!scope.owns(&alice, RUN_B));
    assert!(!scope.owns("cipher-live-it-alice", RUN_A));
    assert!(!scope.owns("production-user", RUN_A));
}

#[test]
fn separate_runs_cannot_select_each_others_resources() {
    let first = FixtureScope::parse(RUN_A).unwrap();
    let second = FixtureScope::parse(RUN_B).unwrap();
    let resource = first.resource_id("alice").unwrap();

    assert!(!second.owns(&resource, RUN_A));
    assert!(!second.owns(&resource, RUN_B));
}
