// SPDX-License-Identifier: Apache-2.0
//! Exact name records shared by sketch and block construction payloads.

use crate::om::compact::{CompactIndexAtom, CompactIndexTarget};
use crate::om::name_field::NameField;
use serde::{Deserialize, Serialize};

/// Exact framed name retained from a reconstructed construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "FeaturePayloadNameWire", into = "FeaturePayloadNameWire")]
pub struct FeaturePayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Reconstructed construction payload carrying this field.
    pub construction_payload: String,
    /// Zero-based name-field order within the reconstructed payload.
    pub ordinal: u32,
    /// Checked name text and its leading or compact-typed frame.
    pub frame: NameField<String>,
    /// Absolute file offset of the opening marker.
    pub source_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct FeaturePayloadNameWire {
    id: String,
    operation_label: String,
    construction_payload: String,
    ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_type_code: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code_payload_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    type_code_source_offset: Option<u64>,
    payload_leading: bool,
    value: String,
    payload_offset: u64,
    source_offset: u64,
}

impl From<FeaturePayloadName> for FeaturePayloadNameWire {
    fn from(value: FeaturePayloadName) -> Self {
        let code = value.frame.code();
        Self {
            id: value.id,
            operation_label: value.operation_label,
            construction_payload: value.construction_payload,
            ordinal: value.ordinal,
            type_code: code.as_ref().map(|code| code.atom.value()),
            raw_type_code: code.as_ref().map(|code| code.atom.raw().to_vec()),
            type_code_payload_offset: code.as_ref().map(|code| code.offset),
            type_code_source_offset: code.and_then(|code| *code.target),
            payload_leading: value.frame.code().is_none(),
            value: value.frame.value().to_owned(),
            payload_offset: value.frame.offset(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<FeaturePayloadNameWire> for FeaturePayloadName {
    type Error = String;

    fn try_from(wire: FeaturePayloadNameWire) -> Result<Self, Self::Error> {
        let code = match (
            wire.type_code,
            wire.raw_type_code,
            wire.type_code_payload_offset,
            wire.type_code_source_offset,
            wire.payload_leading,
        ) {
            (None, None, None, None, true) => None,
            (Some(value), Some(raw), Some(offset), source_offset, false) => {
                if wire.payload_offset.checked_add(1) != Some(offset) {
                    return Err("type_code_payload_offset: expected payload_offset + 1".to_owned());
                }
                Some(CompactIndexTarget {
                    atom: CompactIndexAtom::from_wire(value, &raw)
                        .map_err(|error| format!("type_code/raw_type_code: {error}"))?,
                    target: source_offset,
                })
            }
            _ => {
                return Err(
                    "payload name type code is present exactly when payload_leading is false"
                        .to_owned(),
                );
            }
        };
        let frame = NameField::new(wire.value, wire.payload_offset, code).map_err(str::to_owned)?;
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            construction_payload: wire.construction_payload,
            ordinal: wire.ordinal,
            frame,
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FeaturePayloadName;

    #[test]
    fn payload_names_reject_inconsistent_compact_type_codes() {

        for (value, raw) in [(0, vec![0]), (131, vec![128, 131]), (1, vec![128, 1])] {
            let wire = serde_json::json!({
                "id": "name", "operation_label": "operation", "construction_payload": "payload",
                "ordinal": 0, "type_code": value, "raw_type_code": raw,
                "type_code_payload_offset": 11, "payload_leading": false,
                "value": "Point1", "payload_offset": 10, "source_offset": 100,
            });
            let name: FeaturePayloadName = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(name).unwrap(), wire);
            for invalid_raw in [vec![], vec![255], vec![128], vec![0, 0], vec![127]] {
                let mut invalid = wire.clone();
                invalid["raw_type_code"] = serde_json::json!(invalid_raw);
                let name = serde_json::from_value::<FeaturePayloadName>(invalid).unwrap_err();
                assert!(name.to_string().contains("type_code/raw_type_code"));
            }
        }
    }

    #[test]
    fn payload_name_wire_enforces_frame_positions_and_leading_form() {

        for json in [
            r#"{"id":"name","operation_label":"operation","construction_payload":"payload","ordinal":0,"payload_leading":true,"value":"Point1","payload_offset":0,"source_offset":100}"#,
            r#"{"id":"name","operation_label":"operation","construction_payload":"payload","ordinal":0,"type_code":131,"raw_type_code":[128,131],"type_code_payload_offset":11,"type_code_source_offset":20,"payload_leading":false,"value":"Point1","payload_offset":10,"source_offset":100}"#,
        ] {
            let name: FeaturePayloadName = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&name).unwrap(), json);
            let wire: serde_json::Value = serde_json::from_str(json).unwrap();
            for (field, replacement) in [
                ("payload_offset", serde_json::json!(1)),
                ("type_code_payload_offset", serde_json::json!(12)),
                ("value", serde_json::json!("A B")),
            ] {
                let mut invalid = wire.clone();
                invalid[field] = replacement;
                assert!(serde_json::from_value::<FeaturePayloadName>(invalid).is_err());
            }
        }
        let overflowing = serde_json::json!({
            "id": "name", "operation_label": "operation", "construction_payload": "payload",
            "ordinal": 0, "type_code": 1, "raw_type_code": [1],
            "type_code_payload_offset": u64::MAX, "payload_leading": false,
            "value": "A", "payload_offset": u64::MAX - 1, "source_offset": 100,
        });
        let name = serde_json::from_value::<FeaturePayloadName>(overflowing).unwrap_err();
        assert!(name.to_string().contains("payload_offset"));
    }
}
