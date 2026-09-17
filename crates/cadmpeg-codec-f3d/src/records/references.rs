// SPDX-License-Identifier: Apache-2.0
//! Persistent references, class tags, lost-edge references, visual tokens and material assignments.

use super::identity::{DesignEntityId, RecordedValue};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_physical_token, String, "physical_token");
cadmpeg_core::named_optional_field!(
    deserialize_physical_token_offset,
    u64,
    "physical_token_offset"
);
cadmpeg_core::named_optional_field!(deserialize_visual_preset, String, "visual_preset");
cadmpeg_core::named_optional_field!(
    deserialize_visual_preset_offset,
    u64,
    "visual_preset_offset"
);
/// Persistent-reference channel in the Design construction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentReferenceKind {
    /// Reference identifies a persistent point.
    Point,
    /// Reference identifies the primary id of a persistent curve.
    CurvePrimary,
    /// Reference identifies the secondary id of a persistent curve.
    CurveSecondary,
}

/// One byte-stored persistent point or curve identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistentReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the persistent-reference field name in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the u64 value relative to `byte_offset`.
    pub value_offset: u32,
    /// Whether this reference identifies a persistent point or one end of a curve.
    pub kind: PersistentReferenceKind,
    /// Raw persistent point/curve identifier as stored in the `Design` construction stream.
    pub value: u64,
}

/// A per-file dynamic class tag encoded as three ASCII digits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignClassTag(String);

impl TryFrom<String> for DesignClassTag {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit()) {
            Ok(Self(value))
        } else {
            Err("class_tag must contain three ASCII digits".into())
        }
    }
}

impl From<DesignClassTag> for String {
    fn from(tag: DesignClassTag) -> Self {
        tag.0
    }
}

impl DesignClassTag {
    /// Numeric value of the three-digit tag.
    pub(crate) fn code(&self) -> u32 {
        self.0
            .bytes()
            .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
    /// Dynamic type ordinal: the three digits minus the 256 fixed-class base.
    pub(crate) fn dynamic_ordinal(&self) -> Option<usize> {
        self.0.parse::<usize>().ok()?.checked_sub(256)
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LostEdgeReferenceWire", into = "LostEdgeReferenceWire")]
pub struct LostEdgeReference {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the unresolved record's indexed header.
    record_byte_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag of the unresolved record.
    pub class_tag: DesignClassTag,
    /// Source `BulkStream` record index of the unresolved edge selection.
    pub record_index: u32,
    /// Per-file dynamic class tag of the following indexed record.
    pub next_class_tag: DesignClassTag,
    /// Record index of the following indexed record.
    pub next_record_index: u32,
}

impl LostEdgeReference {
    pub(crate) fn new(
        id: String,
        record_byte_offset: u64,
        class_tag: String,
        record_index: u32,
        next_class_tag: String,
        next_record_index: u32,
    ) -> Result<Self, String> {
        record_byte_offset
            .checked_add(48)
            .ok_or("lost_edge_reference.record_byte_offset overflows the record extent")?;
        Ok(Self {
            id,
            record_byte_offset,
            class_tag: DesignClassTag::try_from(class_tag)?,
            record_index,
            next_class_tag: DesignClassTag::try_from(next_class_tag)
                .map_err(|error| format!("lost_edge_reference.next_class_tag: {error}"))?,
            next_record_index,
        })
    }
    pub(crate) fn record_byte_offset(&self) -> u64 {
        self.record_byte_offset
    }
    pub(crate) fn class_tag_offset(&self) -> u64 {
        self.record_byte_offset + 4
    }
    pub(crate) fn record_index_offset(&self) -> u64 {
        self.record_byte_offset + 7
    }
    pub(crate) fn byte_offset(&self) -> u64 {
        self.record_byte_offset + 29
    }
    pub(crate) fn next_byte_offset(&self) -> u64 {
        self.record_byte_offset + 48
    }
}

/// A construction-history edge selection that Fusion could not re-resolve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LostEdgeReferenceWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// Byte offset of the unresolved record's indexed header.
    pub record_byte_offset: u64,
    /// Byte offset of the unresolved record's three-byte class tag.
    pub class_tag_offset: u64,
    /// Source per-file dynamic three-digit ASCII class tag of the unresolved record.
    pub class_tag: String,
    /// Source `BulkStream` record index of the unresolved edge selection.
    pub record_index: u32,
    /// Byte offset of `record_index`.
    pub record_index_offset: u64,
    /// Byte offset of the `EDGE_REFERENCE_LOST` marker in its Design `BulkStream`.
    pub byte_offset: u64,
    /// Byte offset of the indexed header immediately following this record.
    pub next_byte_offset: u64,
    /// Per-file dynamic class tag of the following indexed record.
    pub next_class_tag: String,
    /// Record index of the following indexed record.
    pub next_record_index: u32,
}

impl TryFrom<LostEdgeReferenceWire> for LostEdgeReference {
    type Error = String;
    fn try_from(wire: LostEdgeReferenceWire) -> Result<Self, Self::Error> {
        let record = Self::new(
            wire.id,
            wire.record_byte_offset,
            wire.class_tag,
            wire.record_index,
            wire.next_class_tag,
            wire.next_record_index,
        )?;
        if wire.class_tag_offset != record.class_tag_offset() {
            return Err(
                "lost_edge_reference.class_tag_offset disagrees with record_byte_offset".into(),
            );
        }
        if wire.record_index_offset != record.record_index_offset() {
            return Err(
                "lost_edge_reference.record_index_offset disagrees with record_byte_offset".into(),
            );
        }
        if wire.byte_offset != record.byte_offset() {
            return Err("lost_edge_reference.byte_offset disagrees with record_byte_offset".into());
        }
        if wire.next_byte_offset != record.next_byte_offset() {
            return Err(
                "lost_edge_reference.next_byte_offset disagrees with record_byte_offset".into(),
            );
        }
        Ok(record)
    }
}

impl From<LostEdgeReference> for LostEdgeReferenceWire {
    fn from(record: LostEdgeReference) -> Self {
        Self {
            record_byte_offset: record.record_byte_offset(),
            class_tag_offset: record.class_tag_offset(),
            record_index_offset: record.record_index_offset(),
            byte_offset: record.byte_offset(),
            next_byte_offset: record.next_byte_offset(),
            id: record.id,
            class_tag: record.class_tag.into(),
            record_index: record.record_index,
            next_class_tag: record.next_class_tag.into(),
            next_record_index: record.next_record_index,
        }
    }
}

/// A complete serialized visual-appearance identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DesignVisualToken(cadmpeg_ir::ids::IdentityKey);

impl TryFrom<String> for DesignVisualToken {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        crate::design::presentation::visual_token(&value)
            .ok_or("visual_guid must be a complete visual token")?;
        cadmpeg_ir::ids::IdentityKey::try_new(value)
            .map(Self)
            .map_err(|_| "visual_guid must be a complete visual token")
    }
}

impl From<DesignVisualToken> for String {
    fn from(value: DesignVisualToken) -> Self {
        value.0.into_string()
    }
}

impl std::ops::Deref for DesignVisualToken {
    type Target = str;
    fn deref(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for DesignVisualToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl DesignVisualToken {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.0.as_str().eq_ignore_ascii_case(other.0.as_str())
    }

    /// The admitted visual token as an identity key.
    pub(crate) fn identity_key(&self) -> cadmpeg_ir::ids::IdentityKey {
        self.0.clone()
    }
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignMaterialAssignmentWire",
    into = "DesignMaterialAssignmentWire"
)]
pub struct DesignMaterialAssignment {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// ASM body key resolved through the Design body map.
    pub asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub asm_body_key_offset: u64,
    /// Byte offset of the body-map entity suffix.
    pub entity_suffix_offset: u64,
    /// UTF-16 design-entity id.
    pub entity_id: DesignEntityId,
    /// Byte offset of the UTF-16 entity-id code units.
    pub entity_id_offset: u64,
    /// Complete serialized visual token.
    pub visual_guid: DesignVisualToken,
    /// Byte offset of the UTF-16 visual-token code units.
    pub visual_guid_offset: u64,
    /// Physical-material token, when present.
    pub physical_token: Option<RecordedValue<String>>,
    /// Visual preset name, when present.
    pub visual_preset: Option<RecordedValue<String>>,
}

/// One Design `BulkStream` material assignment joining a design entity to visual assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignMaterialAssignmentWire {
    /// Globally unique deterministic identifier for this native record.
    pub id: String,
    /// ASM body key resolved through the Design body map.
    pub asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub asm_body_key_offset: u64,
    /// Byte offset of the body-map entity suffix.
    pub entity_suffix_offset: u64,
    /// UTF-16 design-entity id.
    pub entity_id: String,
    /// Byte offset of the UTF-16 entity-id code units.
    pub entity_id_offset: u64,
    /// Complete serialized visual token.
    pub visual_guid: DesignVisualToken,
    /// Byte offset of the UTF-16 visual-token code units.
    pub visual_guid_offset: u64,
    /// Physical-material token, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_physical_token"
    )]
    pub physical_token: Option<String>,
    /// Byte offset of the UTF-16 physical token, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_physical_token_offset"
    )]
    pub physical_token_offset: Option<u64>,
    /// Visual preset name, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visual_preset"
    )]
    pub visual_preset: Option<String>,
    /// Byte offset of the UTF-16 preset name, when present.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visual_preset_offset"
    )]
    pub visual_preset_offset: Option<u64>,
}

impl TryFrom<DesignMaterialAssignmentWire> for DesignMaterialAssignment {
    type Error = String;
    fn try_from(wire: DesignMaterialAssignmentWire) -> Result<Self, Self::Error> {
        let entity_id = DesignEntityId::try_from(wire.entity_id)?;
        Ok(Self {
            id: wire.id,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: wire.asm_body_key_offset,
            entity_suffix_offset: wire.entity_suffix_offset,
            entity_id,
            entity_id_offset: wire.entity_id_offset,
            visual_guid: wire.visual_guid,
            visual_guid_offset: wire.visual_guid_offset,
            physical_token: RecordedValue::from_wire(
                wire.physical_token,
                wire.physical_token_offset,
                "physical_token",
            )?,
            visual_preset: RecordedValue::from_wire(
                wire.visual_preset,
                wire.visual_preset_offset,
                "visual_preset",
            )?,
        })
    }
}

impl From<DesignMaterialAssignment> for DesignMaterialAssignmentWire {
    fn from(value: DesignMaterialAssignment) -> Self {
        Self {
            id: value.id,
            asm_body_key: value.asm_body_key,
            asm_body_key_offset: value.asm_body_key_offset,
            entity_suffix_offset: value.entity_suffix_offset,
            entity_id: value.entity_id.text,
            entity_id_offset: value.entity_id_offset,
            visual_guid: value.visual_guid,
            visual_guid_offset: value.visual_guid_offset,
            physical_token_offset: value.physical_token.as_ref().map(|field| field.offset),
            physical_token: value.physical_token.map(|field| field.value),
            visual_preset_offset: value.visual_preset.as_ref().map(|field| field.offset),
            visual_preset: value.visual_preset.map(|field| field.value),
        }
    }
}
