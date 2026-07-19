// SPDX-License-Identifier: Apache-2.0
//! Deltas between source metadata sidecars.

use crate::source_fidelity::SourceFidelity;

/// Differences between two source metadata sidecars.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct FidelityDiff {
    /// Schema-version transition, when it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<(String, String)>,
    /// Whether provenance or exactness annotations changed.
    pub annotations_changed: bool,
    /// Whether retained-record metadata or bytes changed.
    pub retained_records_changed: bool,
}

impl FidelityDiff {
    /// Returns whether the two sidecars are identical.
    pub fn is_empty(&self) -> bool {
        self.version.is_none() && !self.annotations_changed && !self.retained_records_changed
    }
}

/// Compares source annotations and retained native records.
pub fn diff_source_fidelity(left: &SourceFidelity, right: &SourceFidelity) -> FidelityDiff {
    FidelityDiff {
        version: (left.version != right.version)
            .then(|| (left.version.clone(), right.version.clone())),
        annotations_changed: left.annotations != right.annotations,
        retained_records_changed: left.retained_records != right.retained_records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_record_changes_are_material() {
        let left = SourceFidelity::default();
        let mut right = SourceFidelity::default();
        right.retained_records.push(crate::RetainedSourceRecord {
            id: "record".into(),
            stream: "source".into(),
            offset: 0,
            byte_len: 1,
            sha256: crate::hash::sha256_hex(&[0]),
            data: Some(vec![0]),
        });
        assert!(diff_source_fidelity(&left, &right).retained_records_changed);
    }
}
