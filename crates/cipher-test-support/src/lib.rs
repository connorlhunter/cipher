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
#[path = "tests/fixture_scope.rs"]
mod tests;
