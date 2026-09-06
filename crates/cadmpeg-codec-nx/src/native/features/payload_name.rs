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
