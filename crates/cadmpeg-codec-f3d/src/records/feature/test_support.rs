// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::scope::{DesignFeatureKind, DesignParameterScopeDraft};
use crate::records::identity::{Located, ReferenceRun};

impl DesignParameterScopeDraft {
    pub(crate) fn with_fixture_layout(mut self) -> Self {
        self.paired_byte_offset = self.byte_offset + self.frame_length;
        self.layout_fixture_references();
        self.layout_fixture_tail();
        self
    }

    pub(crate) fn layout_fixture_references(&mut self) {
        self.reference_members = ReferenceRun::located(
            self.reference_members
                .values()
                .copied()
                .enumerate()
                .map(|(ordinal, value)| Located {
                    value,
                    offset: self.reference_count_offset + 5 + 11 * u64_from_index(ordinal),
                })
                .collect(),
        );
        self.kind_offset =
            self.reference_count_offset + 12 + 11 * u64_from_index(self.reference_members.len());
    }

    pub(crate) fn layout_fixture_tail(&mut self) {
        let (tail_length, previous_delta) =
            match (self.payload.kind(), self.previous_history_state_id) {
                (DesignFeatureKind::CopyPasteBodies, Some(_)) => (110, 53),
                (DesignFeatureKind::CopyPasteBodies, None) => (80, 0),
                _ => (72, 30),
            };
        self.feature_ordinal_offset = self.paired_byte_offset - tail_length;
        self.previous_history_state_id_offset = self.previous_history_state_id.map(|previous| {
            self.history_state_id.get_or_insert(previous + 1);
            self.feature_ordinal_offset + previous_delta
        });
    }
}

/// The located scope a flat scope fixture states: a scope document with the
/// frame offsets and frame length of the decoded scope beside its fields.
/// Offsets nested in the payload are dropped.
pub(crate) fn located_scope_from_fixture(
    mut value: serde_json::Value,
) -> super::scope::DesignParameterScope {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                fields.retain(|key, _| !(key.ends_with("_offset") || key.ends_with("_offsets")));
                fields.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let field = |name: &str| value.get(name).and_then(serde_json::Value::as_u64);
    let required = |name: &str| field(name).unwrap_or_else(|| panic!("fixture {name}"));
    let byte_offset = required("byte_offset");
    let frame_length = required("frame_length");
    let kind_offset = required("kind_offset");
    let feature_ordinal_offset = required("feature_ordinal_offset");
    let previous_history_state_id_offset = field("previous_history_state_id_offset");
    let reference_count_offset = required("reference_count_offset");
    let paired_byte_offset = required("paired_byte_offset");
    let reference_offsets: Vec<u64> = value
        .get("reference_member_offsets")
        .and_then(serde_json::Value::as_array)
        .map(|offsets| {
            offsets
                .iter()
                .filter_map(serde_json::Value::as_u64)
                .collect()
        })
        .unwrap_or_default();
    if let Some(fields) = value.as_object_mut() {
        fields.remove("frame_length");
    }
    strip(&mut value);
    value["generation"] = serde_json::json!({"form": "envelope"});
    let scope: super::scope::DesignParameterScope =
        serde_json::from_value(value).expect("fixture scope document");
    let values: Vec<u32> = scope.reference_members().values().copied().collect();
    let reference_members = if reference_offsets.len() == values.len() {
        ReferenceRun::located(
            values
                .into_iter()
                .zip(reference_offsets)
                .map(|(value, offset)| Located { value, offset })
                .collect(),
        )
    } else {
        ReferenceRun::unlocated(values)
    };
    super::scope::DesignParameterScope::try_new(DesignParameterScopeDraft {
        id: scope.id.clone(),
        byte_offset,
        class_tag: scope.class_tag.clone(),
        record_index: scope.record_index,
        frame_length,
        kind_offset,
        feature_ordinal: scope.feature_ordinal,
        feature_ordinal_offset,
        history_state_id: scope.history_state_id(),
        previous_history_state_id: scope.previous_history_state_id(),
        previous_history_state_id_offset,
        reference_count_offset,
        reference_members,
        payload: scope.payload().clone(),
        unclosed_construction_operand_groups: scope.unclosed_construction_operand_groups.clone(),
        paired_class_tag: scope.paired_class_tag.clone(),
        paired_byte_offset,
    })
    .expect("fixture scope layout")
}
