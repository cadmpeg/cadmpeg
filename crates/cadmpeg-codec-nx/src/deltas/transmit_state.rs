// SPDX-License-Identifier: Apache-2.0
//! Valid transmit-header text and consecutive identities.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TransmitWire", into = "TransmitWire")]
pub(crate) struct TransmitState {
    description: String,
    schema: String,
    first_reference: u32,
}

impl TransmitState {
    pub(crate) fn new(description: String, schema: String, references: [u32; 2]) -> Result<Self, &'static str> {
        if !description.contains("(deltas)") || !description.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' ') {
            return Err("description: require printable ASCII containing (deltas)");
        }
        if schema.len() <= 4 || !schema.starts_with("SCH_") || !schema.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err("schema: require SCH_ followed by ASCII letters, digits, or underscores");
        }
        let [first_reference, second] = references;
        if first_reference <= 1 || first_reference.checked_add(1) != Some(second) {
            return Err("references: require two consecutive non-null identities");
        }
        Ok(Self { description, schema, first_reference })
    }
    #[cfg(test)]
    pub(crate) fn description(&self) -> &str { &self.description }
    #[cfg(test)]
    pub(crate) fn schema(&self) -> &str { &self.schema }
    pub(crate) fn references(&self) -> [u32; 2] { [self.first_reference, self.first_reference + 1] }
}

#[derive(Clone, Serialize, Deserialize)]
struct TransmitWire {
    description: String,
    schema: String,
    references: [u32; 2],
}

impl From<TransmitState> for TransmitWire {
    fn from(state: TransmitState) -> Self {
        Self { references: state.references(), description: state.description, schema: state.schema }
    }
}
impl TryFrom<TransmitWire> for TransmitState {
    type Error = &'static str;
    fn try_from(wire: TransmitWire) -> Result<Self, Self::Error> {
        Self::new(wire.description, wire.schema, wire.references)
    }
}

#[cfg(test)]
mod tests {
    use super::TransmitState;

    #[test]
    fn transmit_wire_preserves_fields_and_rejects_invalid_header_payload() {
        let json = r#"{"description":"Transmit (deltas)","schema":"SCH_1","references":[1063,1064]}"#;
        let state: TransmitState = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        let valid = serde_json::to_value(&state).unwrap();
        for (field, value) in [
            ("description", serde_json::json!("Transmit")),
            ("description", serde_json::json!("(deltas)\n")),
            ("schema", serde_json::json!("SCH_")),
            ("schema", serde_json::json!("SCH_1!")),
            ("references", serde_json::json!([1,2])),
            ("references", serde_json::json!([1063,1065])),
            ("references", serde_json::json!([4294967295u32,0])),
        ] {
            let mut wire = valid.clone();
            wire[field] = value;
            assert!(serde_json::from_value::<TransmitState>(wire).unwrap_err().to_string().contains(field));
        }
    }
}
