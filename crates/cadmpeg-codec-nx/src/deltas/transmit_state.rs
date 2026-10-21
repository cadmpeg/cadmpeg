// SPDX-License-Identifier: Apache-2.0
//! Counted transmit metadata, ending before the first node.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TransmitWire")]
pub(crate) struct TransmitState {
    description: String,
    schema: String,
    maximum_node_type: NonZeroU16,
    user_field_size: u32,
}

impl TransmitState {
    pub(super) fn new(
        description: String,
        schema: String,
        maximum_node_type: u16,
        user_field_size: u32,
    ) -> Result<Self, &'static str> {
        if !description.contains("(deltas)")
            || !description
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            return Err("description: require printable ASCII containing (deltas)");
        }
        if schema.split('_').count() != 4
            || schema.len() <= 4
            || !schema.starts_with("SCH_")
            || !schema
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err("schema: require an embedded SCH_ token with four components");
        }
        let maximum_node_type =
            NonZeroU16::new(maximum_node_type).ok_or("maximum_node_type: must be nonzero")?;
        if user_field_size > 16 {
            return Err("user_field_size: require zero through sixteen integer words");
        }
        Ok(Self {
            description,
            schema,
            maximum_node_type,
            user_field_size,
        })
    }

    #[cfg(test)]
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    pub(crate) fn schema(&self) -> &str {
        &self.schema
    }

    pub(crate) fn maximum_node_type(&self) -> u16 {
        self.maximum_node_type.get()
    }

    pub(crate) fn user_field_size(&self) -> u32 {
        self.user_field_size
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct TransmitWire {
    description: String,
    schema: String,
    maximum_node_type: u16,
    user_field_size: u32,
}

#[cfg(test)]
impl From<TransmitState> for TransmitWire {
    fn from(state: TransmitState) -> Self {
        Self {
            description: state.description,
            schema: state.schema,
            maximum_node_type: state.maximum_node_type.get(),
            user_field_size: state.user_field_size,
        }
    }
}

impl TryFrom<TransmitWire> for TransmitState {
    type Error = &'static str;
    fn try_from(wire: TransmitWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.description,
            wire.schema,
            wire.maximum_node_type,
            wire.user_field_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::TransmitState;

    #[test]
    fn transmit_wire_preserves_metadata_and_rejects_invalid_fields() {
        let json = r#"{"description":"Transmit (deltas)","schema":"SCH_3501171_35102_13006","maximum_node_type":239,"user_field_size":0}"#;
        let state: TransmitState = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        assert_eq!(
            serde_json::to_vec(&state).unwrap(),
            serde_json::to_vec(&super::TransmitWire::from(state.clone())).unwrap()
        );
        let valid = serde_json::to_value(&state).unwrap();
        for (field, value) in [
            ("description", serde_json::json!("Transmit")),
            ("description", serde_json::json!("(deltas)\n")),
            ("schema", serde_json::json!("SCH_")),
            ("schema", serde_json::json!("SCH_1!")),
            ("schema", serde_json::json!("SCH_3501171_13006")),
            ("maximum_node_type", serde_json::json!(0)),
            ("user_field_size", serde_json::json!(17)),
        ] {
            let mut wire = valid.clone();
            wire[field] = value;
            assert!(serde_json::from_value::<TransmitState>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }

    #[test]
    fn transmit_state_native_limit_refuses_before_text_copy() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            #[serde(flatten)]
            state: &'a TransmitState,
        }
        let state: TransmitState = serde_json::from_str(
            r#"{"description":"Transmit (deltas)","schema":"SCH_3501171_35102_13006","maximum_node_type":231,"user_field_size":0}"#,
        )
        .unwrap();
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &Record {
                id: "nx:parasolid:transmit-state#0",
                state: &state,
            },
            serde_json::json!({"id": "nx:parasolid:transmit-state#0",
                "description": "Transmit (deltas)", "schema": "SCH_3501171_35102_13006",
                "maximum_node_type": 231, "user_field_size": 0}),
        );
    }
}
