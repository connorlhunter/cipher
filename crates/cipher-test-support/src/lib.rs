//! Safe resource naming helpers for live integration tests.

const FIXTURE_PREFIX: &str = "cipher-live-it-";

/// Names and recognizes resources owned by one live test run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureScope {
    run_id: String,
    namespace: String,
}

impl FixtureScope {
    /// Creates a scope for a lowercase UUID v4 run identifier.
    ///
    /// Returns an error when the identifier is not a valid lowercase UUID v4.
    pub fn parse(run_id: &str) -> Result<Self, &'static str> {
        if !valid_v4_uuid(run_id) {
            return Err("fixture run id must be a lowercase UUID v4");
        }

        Ok(Self {
            run_id: run_id.into(),
            namespace: format!("{FIXTURE_PREFIX}{run_id}"),
        })
    }

    /// Returns the identifier for this test run.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Builds an owned resource name from a safe label.
    ///
    /// Returns an error when the label contains unsupported characters.
    pub fn resource_id(&self, label: &str) -> Result<String, &'static str> {
        if label.is_empty()
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("fixture label must use lowercase letters, digits, or hyphens");
        }

        Ok(format!("{}-{label}", self.namespace))
    }

    /// Returns the object-storage prefix owned by this test run.
    pub fn object_prefix(&self) -> String {
        format!("fixtures/{}/", self.run_id)
    }

    /// Checks that a resource name and fixture marker belong to this run.
    pub fn owns(&self, resource_id: &str, fixture_run_id: &str) -> bool {
        fixture_run_id == self.run_id
            && resource_id
                .strip_prefix(&self.namespace)
                .is_some_and(|suffix| suffix.starts_with('-') || suffix.starts_with('/'))
    }
}

fn valid_v4_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }

    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        14 => byte == b'4',
        19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
        _ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
    })
}

#[cfg(test)]
mod tests {
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
}
