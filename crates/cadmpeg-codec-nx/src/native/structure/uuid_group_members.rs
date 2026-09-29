// SPDX-License-Identifier: Apache-2.0
//! Equal-cardinality UUID group lists without an instance-level association.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::iter_wire::IterWire;
use crate::om::nonempty::NonEmpty;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListSlot {
    occurrence: String,
    object_uuid_value: String,
}

/// Slots retain positions in the two independent lists, not matched instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UuidGroupMembers(NonEmpty<ListSlot>);

impl UuidGroupMembers {
    pub(super) fn new_charged(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        occurrences: Vec<String>,
        object_uuid_values: Vec<String>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if occurrences.len() != object_uuid_values.len() {
            return Ok(None);
        }
        NonEmpty::new_charged(
            ctx,
            occurrences.into_iter().zip(object_uuid_values).map(
                |(occurrence, object_uuid_value)| ListSlot {
                    occurrence,
                    object_uuid_value,
                },
            ),
        )
        .map(|members| members.map(Self))
    }

    pub(super) fn new(
        occurrences: Vec<String>,
        object_uuid_values: Vec<String>,
    ) -> Result<Self, &'static str> {
        if occurrences.len() != object_uuid_values.len() {
            return Err("occurrences/object_uuid_values: list lengths must match");
        }
        NonEmpty::new(occurrences.into_iter().zip(object_uuid_values).map(
            |(occurrence, object_uuid_value)| ListSlot {
                occurrence,
                object_uuid_value,
            },
        ))
        .map(Self)
        .ok_or("occurrences/object_uuid_values: lists must be nonempty")
    }

    #[cfg(test)]
    pub(super) fn occurrences(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|slot| slot.occurrence.as_str())
    }

    #[cfg(test)]
    pub(super) fn object_uuid_values(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|slot| slot.object_uuid_value.as_str())
    }
}

impl Serialize for UuidGroupMembers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("UuidGroupMembers", 2)?;
        state.serialize_field(
            "occurrences",
            &IterWire(self.0.iter().map(|slot| slot.occurrence.as_str())),
        )?;
        state.serialize_field(
            "object_uuid_values",
            &IterWire(self.0.iter().map(|slot| slot.object_uuid_value.as_str())),
        )?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for UuidGroupMembers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            occurrences: Vec<String>,
            object_uuid_values: Vec<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.occurrences, wire.object_uuid_values).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::UuidGroupMembers;

    fn group_refusal(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        configure(&mut policy);
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        UuidGroupMembers::new_charged(
            &ctx,
            vec!["a".into(), "b".into()],
            vec!["x".into(), "y".into()],
        )
        .unwrap_err()
    }

    #[test]
    fn uuid_group_members_refuse_collection_limit() {
        let error = group_refusal(|policy| policy.limits.max_collection_items = 1);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
        );
    }

    #[test]
    fn uuid_group_members_refuse_retained_limit() {
        let error = group_refusal(|policy| policy.limits.max_retained_bytes = 0);
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn group_lists_preserve_each_order_and_require_equal_nonempty_cardinality() {
        let json =
            r#"{"occurrences":["use-b","use-a"],"object_uuid_values":["value-a","value-b"]}"#;
        let members: UuidGroupMembers = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&members).unwrap(), json);
        for json in [
            r#"{"occurrences":[],"object_uuid_values":[]}"#,
            r#"{"occurrences":["use"],"object_uuid_values":[]}"#,
            r#"{"occurrences":[],"object_uuid_values":["value"]}"#,
            r#"{"occurrences":["use"],"object_uuid_values":["a","b"]}"#,
        ] {
            assert!(serde_json::from_str::<UuidGroupMembers>(json)
                .unwrap_err()
                .to_string()
                .contains("occurrences/object_uuid_values"));
        }
    }
}
