// SPDX-License-Identifier: Apache-2.0
//! Equal-cardinality UUID group lists without an instance-level association.

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::iter_wire::IterWire;
use crate::om::nonempty::NonEmpty;

/// Two independent admitted lists with equal nonzero cardinality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UuidGroupMembers {
    occurrences: NonEmpty<String>,
    object_uuid_values: NonEmpty<String>,
}

impl UuidGroupMembers {
    pub(super) fn new_charged(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        occurrences: Vec<String>,
        object_uuid_values: Vec<String>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if occurrences.len() != object_uuid_values.len() {
            return Ok(None);
        }
        let Some(occurrences) = NonEmpty::new_charged(ctx, occurrences)? else {
            return Ok(None);
        };
        let Some(object_uuid_values) = NonEmpty::new_charged(ctx, object_uuid_values)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            occurrences,
            object_uuid_values,
        }))
    }

    /// Transfer the two wire-admitted lists without building another collection.
    fn from_admitted_lists(
        occurrences: Vec<String>,
        object_uuid_values: Vec<String>,
    ) -> Result<Self, &'static str> {
        if occurrences.len() != object_uuid_values.len() {
            return Err("occurrences/object_uuid_values: list lengths must match");
        }
        let occurrences = NonEmpty::from_admitted_vec(occurrences)
            .ok_or("occurrences/object_uuid_values: lists must be nonempty")?;
        let object_uuid_values = NonEmpty::from_admitted_vec(object_uuid_values)
            .ok_or("occurrences/object_uuid_values: lists must be nonempty")?;
        Ok(Self {
            occurrences,
            object_uuid_values,
        })
    }

    #[cfg(test)]
    pub(super) fn occurrences(&self) -> impl Iterator<Item = &str> {
        self.occurrences.iter().map(String::as_str)
    }

    #[cfg(test)]
    pub(super) fn object_uuid_values(&self) -> impl Iterator<Item = &str> {
        self.object_uuid_values.iter().map(String::as_str)
    }
}

impl Serialize for UuidGroupMembers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("UuidGroupMembers", 2)?;
        state.serialize_field(
            "occurrences",
            &IterWire(self.occurrences.iter().map(String::as_str)),
        )?;
        state.serialize_field(
            "object_uuid_values",
            &IterWire(self.object_uuid_values.iter().map(String::as_str)),
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
        Self::from_admitted_lists(wire.occurrences, wire.object_uuid_values)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::UuidGroupMembers;

    fn group_refusal(
        configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    ) -> cadmpeg_core::CodecError {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                configure(policy);
            },
            |ctx| {
                UuidGroupMembers::new_charged(
                    ctx,
                    vec!["a".into(), "b".into()],
                    vec!["x".into(), "y".into()],
                )
                .unwrap_err()
            },
        )
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
