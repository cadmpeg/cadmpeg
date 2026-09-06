// SPDX-License-Identifier: Apache-2.0
//! Equal-cardinality UUID group lists without an instance-level association.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde::ser::SerializeStruct;

use crate::om::nonempty::NonEmpty;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListSlot {
    occurrence: String,
    object_uuid_value: String,
}

/// Slots retain positions in the two independent lists, not matched instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UuidGroupMembers(NonEmpty<ListSlot>);

impl UuidGroupMembers {
    pub(crate) fn new(occurrences: Vec<String>, object_uuid_values: Vec<String>) -> Result<Self, &'static str> {
        if occurrences.len() != object_uuid_values.len() {
            return Err("occurrences/object_uuid_values: list lengths must match");
        }
        NonEmpty::new(occurrences.into_iter().zip(object_uuid_values)
            .map(|(occurrence, object_uuid_value)| ListSlot { occurrence, object_uuid_value }))
            .map(Self).ok_or("occurrences/object_uuid_values: lists must be nonempty")
    }

    pub(crate) fn occurrences(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|slot| slot.occurrence.as_str())
    }

    pub(crate) fn object_uuid_values(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|slot| slot.object_uuid_value.as_str())
    }
}

impl Serialize for UuidGroupMembers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("UuidGroupMembers", 2)?;
        state.serialize_field("occurrences", &self.occurrences().collect::<Vec<_>>())?;
        state.serialize_field("object_uuid_values", &self.object_uuid_values().collect::<Vec<_>>())?;
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

    #[test]
    fn group_lists_preserve_each_order_and_require_equal_nonempty_cardinality() {
        let json = r#"{"occurrences":["use-b","use-a"],"object_uuid_values":["value-a","value-b"]}"#;
        let members: UuidGroupMembers = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&members).unwrap(), json);
        for json in [
            r#"{"occurrences":[],"object_uuid_values":[]}"#,
            r#"{"occurrences":["use"],"object_uuid_values":[]}"#,
            r#"{"occurrences":[],"object_uuid_values":["value"]}"#,
            r#"{"occurrences":["use"],"object_uuid_values":["a","b"]}"#,
        ] {
            assert!(serde_json::from_str::<UuidGroupMembers>(json)
                .unwrap_err().to_string().contains("occurrences/object_uuid_values"));
        }
    }
}
