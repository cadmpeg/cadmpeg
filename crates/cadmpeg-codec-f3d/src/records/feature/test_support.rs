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
/// source offsets and frame length of the decoder's record layout beside its
/// fields. The offsets become the scope's restored pointers.
pub(crate) fn located_scope_from_fixture(
    mut value: serde_json::Value,
) -> super::scope::DesignParameterScope {
    fn strip(
        value: &mut serde_json::Value,
        path: &str,
        pointers: &mut std::collections::BTreeMap<String, u64>,
    ) {
        let join = |name: &str| {
            if path.is_empty() {
                name.to_owned()
            } else {
                format!("{path}.{name}")
            }
        };
        match value {
            serde_json::Value::Object(fields) => {
                let keys = fields.keys().cloned().collect::<Vec<_>>();
                for key in keys {
                    if key.ends_with("_offset") || key.ends_with("_offsets") {
                        let field = fields.remove(&key).expect("present key");
                        match field {
                            serde_json::Value::Number(number) => {
                                pointers.insert(join(&key), number.as_u64().expect("u64 offset"));
                            }
                            serde_json::Value::Array(items) => {
                                for (index, item) in items.iter().enumerate() {
                                    if let Some(offset) = item.as_u64() {
                                        pointers.insert(join(&format!("{key}.{index}")), offset);
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if let Some(field) = fields.get_mut(&key) {
                        strip(field, &join(&key), pointers);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for (index, item) in items.iter_mut().enumerate() {
                    strip(item, &join(&index.to_string()), pointers);
                }
            }
            _ => {}
        }
    }
    let frame_length = value
        .as_object_mut()
        .and_then(|fields| fields.remove("frame_length"))
        .and_then(|length| length.as_u64())
        .expect("fixture frame_length");
    let mut pointers = std::collections::BTreeMap::new();
    strip(&mut value, "", &mut pointers);
    value["generation"] = serde_json::json!({"form": "envelope"});
    let mut scope: super::scope::DesignParameterScope =
        serde_json::from_value(value).expect("fixture scope document");
    crate::records::source_offsets::restore(&mut scope, &pointers);
    scope.restore_frame_length(frame_length);
    let draft = scope.located_draft().expect("fixture scope frame");
    super::scope::DesignParameterScope::try_new(draft).expect("fixture scope layout")
}

/// A located record states each of `names` as a pointer, restores from its
/// pointers to itself, and reports each pointer that a capture lacks.
pub(crate) fn assert_located_pointers<T>(located: &T, names: &[&str])
where
    T: Clone + std::fmt::Debug + PartialEq + crate::records::source_offsets::SourceOffsets,
{
    let pointers = crate::records::source_offsets::collect(&mut located.clone());
    assert_eq!(
        pointers.keys().map(String::as_str).collect::<Vec<_>>(),
        {
            let mut names = names.to_vec();
            names.sort_unstable();
            names
        },
        "pointer names"
    );
    let mut restored = located.clone();
    assert!(crate::records::source_offsets::restore(&mut restored, &pointers).is_empty());
    assert_eq!(&restored, located);
    for name in pointers.keys() {
        let mut partial = pointers.clone();
        partial.remove(name);
        let mut restored = located.clone();
        assert_eq!(
            crate::records::source_offsets::restore(&mut restored, &partial),
            vec![name.clone()],
            "capture without {name}"
        );
    }
}

/// `record` located at `pointers`, which hold every pointer it must have.
pub(crate) fn located_at<T>(mut record: T, pointers: &[(&str, u64)]) -> T
where
    T: crate::records::source_offsets::SourceOffsets,
{
    let pointers = pointers
        .iter()
        .map(|(name, offset)| ((*name).to_owned(), *offset))
        .collect();
    assert_eq!(
        crate::records::source_offsets::restore(&mut record, &pointers),
        Vec::<String>::new()
    );
    record
}

/// `record` located at `pointers`, checked by [`assert_located_pointers`].
pub(crate) fn assert_located_at<T>(record: T, pointers: &[(&str, u64)]) -> T
where
    T: Clone + std::fmt::Debug + PartialEq + crate::records::source_offsets::SourceOffsets,
{
    let located = located_at(record, pointers);
    assert_located_pointers(
        &located,
        &pointers.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
    );
    located
}

/// Each of `names` is a pointer of the located `record`, and a capture
/// without it lacks that pointer beside those its full capture lacks.
pub(crate) fn assert_required_pointers<T>(located: &T, names: &[&str])
where
    T: Clone + crate::records::source_offsets::SourceOffsets,
{
    let pointers = crate::records::source_offsets::collect(&mut located.clone());
    let baseline = crate::records::source_offsets::restore(&mut located.clone(), &pointers);
    for name in names {
        assert!(
            pointers.contains_key(*name),
            "pointer {name} in {pointers:?}"
        );
        let mut partial = pointers.clone();
        partial.remove(*name);
        let mut missing = crate::records::source_offsets::restore(&mut located.clone(), &partial);
        missing.retain(|key| !baseline.contains(key));
        assert_eq!(missing, vec![(*name).to_owned()], "capture without {name}");
    }
}

/// `read`, a record read from its wire, with the source offsets that
/// `located` holds.
pub(crate) fn relocated<T>(mut read: T, located: &T) -> T
where
    T: Clone + crate::records::source_offsets::SourceOffsets,
{
    let pointers = crate::records::source_offsets::collect(&mut located.clone());
    crate::records::source_offsets::restore(&mut read, &pointers);
    read
}
