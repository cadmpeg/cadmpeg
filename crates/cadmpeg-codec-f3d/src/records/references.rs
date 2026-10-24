// SPDX-License-Identifier: Apache-2.0
//! Persistent references, class tags, lost-edge references, visual tokens and material assignments.

use super::identity::{DesignEntityId, RecordedValue};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(deserialize_physical_token, String, "physical_token");
cadmpeg_core::named_optional_field!(deserialize_visual_preset, String, "visual_preset");
/// Persistent-reference channel in the Design construction stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PersistentReferenceKind {
    /// Reference identifies a persistent point.
    Point,
    /// Reference identifies the primary id of a persistent curve.
    CurvePrimary,
    /// Reference identifies the secondary id of a persistent curve.
    CurveSecondary,
}

/// One byte-stored persistent point or curve identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistentReference {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the persistent-reference field name in its Design `BulkStream`.
    #[serde(skip)]
    pub(crate) byte_offset: Option<u64>,
    /// Byte offset of the u64 value relative to `byte_offset`.
    #[serde(skip)]
    pub(crate) value_offset: Option<u32>,
    /// Whether this reference identifies a persistent point or one end of a curve.
    pub(crate) kind: PersistentReferenceKind,
    /// Raw persistent point/curve identifier as stored in the `Design` construction stream.
    pub(crate) value: u64,
}

/// A per-file dynamic class tag encoded as three ASCII digits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignClassTag(String);

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
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "LostEdgeReferenceWire")]
pub(crate) struct LostEdgeReference {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Byte offset of the unresolved record's indexed header.
    record_byte_offset: Option<u64>,
    /// Source per-file dynamic three-digit ASCII class tag of the unresolved record.
    pub(crate) class_tag: DesignClassTag,
    /// Source `BulkStream` record index of the unresolved edge selection.
    pub(crate) record_index: u32,
    /// Per-file dynamic class tag of the following indexed record.
    pub(crate) next_class_tag: DesignClassTag,
    /// Record index of the following indexed record.
    pub(crate) next_record_index: u32,
}

#[cfg(test)]
thread_local! {
    static LOST_EDGE_REFERENCE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for LostEdgeReference {
    fn clone(&self) -> Self {
        LOST_EDGE_REFERENCE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            record_byte_offset: self.record_byte_offset,
            class_tag: self.class_tag.clone(),
            record_index: self.record_index,
            next_class_tag: self.next_class_tag.clone(),
            next_record_index: self.next_record_index,
        }
    }
}

#[derive(Serialize)]
struct LostEdgeReferenceWireRef<'a> {
    id: &'a str,
    class_tag: &'a str,
    record_index: u32,
    next_class_tag: &'a str,
    next_record_index: u32,
}
impl Serialize for LostEdgeReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        LostEdgeReferenceWireRef {
            id: &self.id,
            class_tag: self.class_tag.as_str(),
            record_index: self.record_index,
            next_class_tag: self.next_class_tag.as_str(),
            next_record_index: self.next_record_index,
        }
        .serialize(serializer)
    }
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
            record_byte_offset: Some(record_byte_offset),
            class_tag: class_tag.try_into()?,
            record_index,
            next_class_tag: next_class_tag
                .try_into()
                .map_err(|error| format!("lost_edge_reference.next_class_tag: {error}"))?,
            next_record_index,
        })
    }
    pub(crate) fn record_byte_offset(&self) -> Option<u64> {
        self.record_byte_offset
    }
    pub(crate) fn class_tag_offset(&self) -> Option<u64> {
        self.record_byte_offset?.checked_add(4)
    }
    pub(crate) fn record_index_offset(&self) -> Option<u64> {
        self.record_byte_offset?.checked_add(7)
    }
    pub(crate) fn byte_offset(&self) -> Option<u64> {
        self.record_byte_offset?.checked_add(29)
    }
    pub(crate) fn next_byte_offset(&self) -> Option<u64> {
        self.record_byte_offset?.checked_add(48)
    }
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct LostEdgeReferenceWire {
    id: String,
    class_tag: String,
    record_index: u32,
    next_class_tag: String,
    next_record_index: u32,
}
impl TryFrom<LostEdgeReferenceWire> for LostEdgeReference {
    type Error = String;
    fn try_from(wire: LostEdgeReferenceWire) -> Result<Self, String> {
        Ok(Self {
            id: wire.id,
            record_byte_offset: None,
            class_tag: wire.class_tag.try_into()?,
            record_index: wire.record_index,
            next_class_tag: wire
                .next_class_tag
                .try_into()
                .map_err(|error| format!("lost_edge_reference.next_class_tag: {error}"))?,
            next_record_index: wire.next_record_index,
        })
    }
}
#[cfg(test)]
impl From<LostEdgeReference> for LostEdgeReferenceWire {
    fn from(record: LostEdgeReference) -> Self {
        Self {
            id: record.id,
            class_tag: record.class_tag.into(),
            record_index: record.record_index,
            next_class_tag: record.next_class_tag.into(),
            next_record_index: record.next_record_index,
        }
    }
}

/// A complete serialized visual-appearance identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct DesignVisualToken(cadmpeg_ir::ids::IdentityKey);

impl Serialize for DesignVisualToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

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
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignMaterialAssignmentWire")]
pub(crate) struct DesignMaterialAssignment {
    /// Globally unique deterministic identifier for this native record.
    pub(crate) id: String,
    /// Exact typed Design body-map entry this assignment names.
    pub(crate) body_binding_id: super::bodies::DesignBodyBindingId,
    /// ASM body key resolved through the Design body map.
    pub(crate) asm_body_key: u64,
    /// Byte offset of the body-map ASM key.
    pub(crate) asm_body_key_offset: Option<u64>,
    /// Byte offset of the body-map entity suffix.
    pub(crate) entity_suffix_offset: Option<u64>,
    /// UTF-16 design-entity id.
    pub(crate) entity_id: DesignEntityId,
    /// Byte offset of the UTF-16 entity-id code units.
    pub(crate) entity_id_offset: Option<u64>,
    /// Complete serialized visual token.
    pub(crate) visual_guid: DesignVisualToken,
    /// Byte offset of the UTF-16 visual-token code units.
    pub(crate) visual_guid_offset: Option<u64>,
    /// Physical-material token, when present.
    pub(crate) physical_token: Option<RecordedValue<String, Option<u64>>>,
    /// Visual preset name, when present.
    pub(crate) visual_preset: Option<RecordedValue<String, Option<u64>>>,
}

#[cfg(test)]
thread_local! {
    static DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignMaterialAssignment {
    fn clone(&self) -> Self {
        DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            body_binding_id: self.body_binding_id.clone(),
            asm_body_key: self.asm_body_key,
            asm_body_key_offset: self.asm_body_key_offset,
            entity_suffix_offset: self.entity_suffix_offset,
            entity_id: self.entity_id.clone(),
            entity_id_offset: self.entity_id_offset,
            visual_guid: self.visual_guid.clone(),
            visual_guid_offset: self.visual_guid_offset,
            physical_token: self.physical_token.clone(),
            visual_preset: self.visual_preset.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignMaterialAssignmentWireRef<'a> {
    id: &'a str,
    body_binding_id: &'a super::bodies::DesignBodyBindingId,
    asm_body_key: u64,
    entity_id: &'a str,
    visual_guid: &'a DesignVisualToken,
    #[serde(skip_serializing_if = "Option::is_none")]
    physical_token: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visual_preset: Option<&'a str>,
}
impl Serialize for DesignMaterialAssignment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignMaterialAssignmentWireRef {
            id: &self.id,
            body_binding_id: &self.body_binding_id,
            asm_body_key: self.asm_body_key,
            entity_id: self.entity_id.as_str(),
            visual_guid: &self.visual_guid,
            physical_token: self
                .physical_token
                .as_ref()
                .map(|field| field.value.as_str()),
            visual_preset: self
                .visual_preset
                .as_ref()
                .map(|field| field.value.as_str()),
        }
        .serialize(serializer)
    }
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
#[serde(deny_unknown_fields)]
struct DesignMaterialAssignmentWire {
    id: String,
    body_binding_id: super::bodies::DesignBodyBindingId,
    asm_body_key: u64,
    entity_id: String,
    visual_guid: DesignVisualToken,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_physical_token"
    )]
    physical_token: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_visual_preset"
    )]
    visual_preset: Option<String>,
}
impl TryFrom<DesignMaterialAssignmentWire> for DesignMaterialAssignment {
    type Error = String;
    fn try_from(wire: DesignMaterialAssignmentWire) -> Result<Self, String> {
        Ok(Self {
            id: wire.id,
            body_binding_id: wire.body_binding_id,
            asm_body_key: wire.asm_body_key,
            asm_body_key_offset: None,
            entity_suffix_offset: None,
            entity_id: wire.entity_id.try_into()?,
            entity_id_offset: None,
            visual_guid: wire.visual_guid,
            visual_guid_offset: None,
            physical_token: wire.physical_token.map(|value| RecordedValue {
                value,
                offset: None,
            }),
            visual_preset: wire.visual_preset.map(|value| RecordedValue {
                value,
                offset: None,
            }),
        })
    }
}
#[cfg(test)]
impl From<DesignMaterialAssignment> for DesignMaterialAssignmentWire {
    fn from(value: DesignMaterialAssignment) -> Self {
        Self {
            id: value.id,
            body_binding_id: value.body_binding_id,
            asm_body_key: value.asm_body_key,
            entity_id: value.entity_id.into_string(),
            visual_guid: value.visual_guid,
            physical_token: value.physical_token.map(|field| field.value),
            visual_preset: value.visual_preset.map(|field| field.value),
        }
    }
}
impl PersistentReference {
    /// The source offsets of a decoded reference, as the `record_layout`
    /// pointers of its capture: the property start and the value offset
    /// relative to it.
    pub(crate) fn source_offsets(&self) -> [(&'static str, Option<u64>); 2] {
        [
            ("byte_offset", self.byte_offset),
            ("value_offset", self.value_offset.map(u64::from)),
        ]
    }
}
impl LostEdgeReference {
    /// The source offsets of a decoded reference, as the `record_layout`
    /// pointers of its capture.
    pub(crate) fn source_offsets(&self) -> [(&'static str, Option<u64>); 5] {
        [
            ("record_byte_offset", self.record_byte_offset()),
            ("class_tag_offset", self.class_tag_offset()),
            ("record_index_offset", self.record_index_offset()),
            ("byte_offset", self.byte_offset()),
            ("next_byte_offset", self.next_byte_offset()),
        ]
    }
}
impl DesignMaterialAssignment {
    /// The source offsets of a decoded assignment, as the `record_layout`
    /// pointers of its capture.
    pub(crate) fn source_offsets(&self) -> [(&'static str, Option<u64>); 6] {
        [
            ("asm_body_key_offset", self.asm_body_key_offset),
            ("entity_suffix_offset", self.entity_suffix_offset),
            ("entity_id_offset", self.entity_id_offset),
            ("visual_guid_offset", self.visual_guid_offset),
            (
                "physical_token_offset",
                self.physical_token.as_ref().and_then(|value| value.offset),
            ),
            (
                "visual_preset_offset",
                self.visual_preset.as_ref().and_then(|value| value.offset),
            ),
        ]
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
