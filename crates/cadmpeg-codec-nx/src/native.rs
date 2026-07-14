// SPDX-License-Identifier: Apache-2.0
//! Typed Siemens NX object-model records retained in the native namespace.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::container::Container;
use crate::parasolid::{Stream, StreamKind};

/// One row retained from the canonical `UG_PART` segment index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentIndexRow {
    /// Globally unique row identity.
    pub id: String,
    /// Zero-based row ordinal.
    pub ordinal: u32,
    /// First little-endian row word.
    pub type_code: u32,
    /// Second little-endian row word.
    pub subtype_code: u32,
    /// Third little-endian row word.
    pub value: u32,
    /// Directory entry containing the index.
    pub source_entry: String,
    /// Absolute file offset of the row.
    pub source_offset: u64,
}

/// Decode the canonical `UG_PART` segment-index rows.
pub fn segment_index_rows(container: &Container) -> Vec<SegmentIndexRow> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    index
        .rows
        .into_iter()
        .enumerate()
        .map(|(ordinal, row)| SegmentIndexRow {
            id: format!("nx:segment-index:row#{ordinal}"),
            ordinal: ordinal as u32,
            type_code: row.type_code,
            subtype_code: row.subtype_code,
            value: row.value,
            source_entry: entry.name.clone(),
            source_offset: entry_offset + (ordinal * 12) as u64,
        })
        .collect()
}

/// Word position within one segment-index row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentIndexSlot {
    /// First row word.
    TypeCode,
    /// Second row word.
    SubtypeCode,
    /// Third row word.
    Value,
}

/// Validated link from a segment-index word to a compressed stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentStreamLink {
    /// Globally unique link identity.
    pub id: String,
    /// Owning segment-index row.
    pub row: String,
    /// Row word containing the wrapper offset.
    pub slot: SegmentIndexSlot,
    /// Zero-based stream ordinal in source-file order.
    pub stream_ordinal: u32,
    /// Decoded stream classification.
    pub stream_kind: String,
    /// Bytes from the wrapper start to its zlib header.
    pub wrapper_byte_len: u32,
    /// Absolute file offset of the wrapper.
    pub source_offset: u64,
}

/// Body-image identity carried beside one validated Parasolid stream wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentBodyBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Validated stream-wrapper link owning the metadata tuple.
    pub stream_link: String,
    /// Zero-based stream ordinal in source-file order.
    pub stream_ordinal: u32,
    /// Partition or plain cached-body stream classification.
    pub stream_kind: String,
    /// Object index used by feature-history body operands.
    pub body_object_index: u32,
    /// Second object index naming the same body image in feature history.
    pub body_alias_object_index: u32,
    /// Serialized role word completing the five-word segment tuple.
    pub stream_role: u32,
    /// Absolute file offset of the object-index word in the segment index.
    pub source_offset: u64,
}

/// Named Parasolid attribute class declared in one inflated body stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidAttributeDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based embedded stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local definition record identity.
    pub xmt: u16,
    /// Exact printable attribute class name.
    pub name: String,
    /// Declared number of fields.
    pub field_count: u32,
    /// Stream-local identity of the following field record.
    pub field_record_xmt: u16,
    /// Ordered catalog references in the field-record header.
    pub field_record_references: [u16; 2],
    /// Two field-record header words following the catalog references.
    pub field_record_header_words: [u16; 2],
    /// Offset of the declaration in the inflated stream.
    pub inflated_offset: u64,
}

/// Validated link from a segment-index word to a framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentOmLink {
    /// Globally unique link identity.
    pub id: String,
    /// Owning segment-index row.
    pub row: String,
    /// Row word containing the section offset.
    pub slot: SegmentIndexSlot,
    /// Role established by exact class declarations in the pointed registry.
    pub schema_role: OmSchemaRole,
    /// Bytes from the pointed offset to the OM section signature.
    pub separator_byte_len: u32,
    /// Absolute file offset of the pointed location.
    pub source_offset: u64,
    /// Absolute file offset of the `ff ff ff ff` OM signature.
    pub section_offset: u64,
}

/// Semantic family declared by a linked OM section's class registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmSchemaRole {
    /// General part object model declaring `UGS::Solid::Topol`.
    Model,
    /// Construction/history model declaring `UGS::FEATURE_RECORD`.
    FeatureHistory,
    /// Expression model declaring `UGS::EXP_expression`.
    Expressions,
    /// Audit model declaring `UGS::OM::SaveAuditTrail`.
    AuditTrail,
    /// No role marker from the defined families occurs in the registry.
    Other,
}

/// Internally pointed record area in a role-classified size-framed OM section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmRecordArea {
    /// Globally unique record-area identity.
    pub id: String,
    /// Link identifying the owning ordered OM section.
    pub section_link: String,
    /// Registry-derived role of the owning section.
    pub schema_role: OmSchemaRole,
    /// Three exact little-endian control words.
    pub control_words: [u32; 3],
    /// Exact printable product/version string.
    pub product_version: String,
    /// Exact record-area byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete pointed record area.
    pub sha256: String,
    /// Absolute file offset of the first control word.
    pub source_offset: u64,
}

/// Ordered feature operation label from a feature-history record area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationLabel {
    /// Globally unique label identity.
    pub id: String,
    /// Link identifying the owning ordered OM section.
    pub section_link: String,
    /// Zero-based order within the record area.
    pub ordinal: u32,
    /// Exact printable operation name.
    pub value: String,
    /// Four object-index slots in header order.
    pub object_indices: [Option<u32>; 4],
    /// Absolute file offset of the `03` label tag.
    pub source_offset: u64,
}

/// Exactly bounded feature-history operation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based record order within the feature-history section.
    pub ordinal: u32,
    /// Exact record byte length.
    pub byte_len: u64,
    /// SHA-256 of the complete operation record.
    pub sha256: String,
    /// Exact serialized post-label payload length.
    pub payload_byte_len: u64,
    /// SHA-256 of the post-label serialized operation payload.
    pub payload_sha256: String,
    /// Absolute file offset of the first post-label payload byte.
    pub payload_source_offset: u64,
    /// Absolute file offset of the fixed operation-header marker.
    pub source_offset: u64,
}

/// Ordered length-framed string from one bounded feature-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePayloadString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning exact feature-operation record.
    pub operation_record: String,
    /// Zero-based string order within the post-label payload.
    pub ordinal: u32,
    /// Exact UTF-8 string value.
    pub value: String,
    /// Absolute file offset of the `04` marker.
    pub source_offset: u64,
}

/// Primary body object read or written by one feature-history operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Primary body object index.
    pub body_object_index: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Ordered body-reference field retained from one feature-history operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodyReferenceOccurrence {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based field order within the bounded operation record.
    pub ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Operation-header input resolved to one bounded offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputBlock {
    /// Globally unique input-binding identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
    /// Object index serialized in that slot.
    pub object_index: u32,
    /// Target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Ordered parameter declaration reached through one feature input block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureParameterBinding {
    /// Globally unique binding identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
    /// Input block carrying the object-reference field.
    pub input_block: String,
    /// Zero-based object-reference order within the input block.
    pub reference_ordinal: u32,
    /// Target parameter declaration in the native expression arena.
    pub expression_declaration: String,
    /// Persistent OM object ID of the declaration.
    pub object_id: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Ordered sketch-history record and its exact native input lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchRecord {
    /// Globally unique sketch-record identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Zero-based order within the feature-history area.
    pub ordinal: u32,
    /// Exact bounded operation record.
    pub operation_record: String,
    /// Resolved input bindings in header-slot order.
    pub input_blocks: Vec<String>,
    /// Ordered references carried by the sketch payload.
    pub payload_references: Vec<String>,
    /// Absolute file offset of the operation label.
    pub source_offset: u64,
}

/// Completely resolved counted-reference field of one sketch construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchConstructionInputs {
    /// Globally unique construction-input identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Joined typed sketch record.
    pub sketch_record: String,
    /// Ordered references preceding the field separator.
    pub member_references: Vec<String>,
    /// Ordered uniquely resolved member blocks.
    pub member_data_blocks: Vec<String>,
    /// Reference following the field separator.
    pub terminal_reference: String,
    /// Uniquely resolved terminal block.
    pub terminal_data_block: String,
}

/// Ordered object reference carried by a bounded sketch-operation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchReference {
    /// Globally unique sketch-reference identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Zero-based reference order in the counted field.
    pub ordinal: u32,
    /// Effective count encoded by the containing reference field.
    pub declared_count: u8,
    /// Whether this is the reference following the `00 00` separator.
    pub terminal: bool,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Ordered profile reference carried by a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudeProfileReference {
    /// Globally unique profile-reference identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Zero-based profile-reference order.
    pub ordinal: u32,
    /// Whether the payload repeats the complete encoded profile list exactly once.
    pub witnessed: bool,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Fixed shifted-IEEE scalar header from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureExtrudePayloadHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Ordered finite scalar values.
    pub scalars: [f64; 2],
    /// Absolute file offset of the first shifted-IEEE scalar.
    pub source_offset: u64,
}

/// Serialized width form of an extrusion payload scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePayloadScalarEncoding {
    /// Single-byte exact zero.
    Zero,
    /// Four-byte shifted IEEE-754 binary32.
    Binary32,
    /// Eight-byte shifted IEEE-754 binary64.
    Binary64,
}

/// Three typed scalars following an extrusion body-reference field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureExtrudePayloadScalarTriple {
    /// Globally unique scalar-lane identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Ordered finite scalar values.
    pub values: [f64; 3],
    /// Ordered serialized width forms.
    pub encodings: [FeaturePayloadScalarEncoding; 3],
    /// Absolute file offsets of the three scalar markers.
    pub source_offsets: [u64; 3],
}

/// Three typed scalars anchored to an ordered operation body reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureOperationBodyScalarTriple {
    /// Globally unique scalar-clause identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Ordered finite scalar values.
    pub values: [f64; 3],
    /// Ordered serialized width forms.
    pub encodings: [FeaturePayloadScalarEncoding; 3],
    /// Absolute file offsets of the three scalar markers.
    pub source_offsets: [u64; 3],
}

/// Ordered member index in a branch-`11` operation body clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyMember {
    /// Globally unique member identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Zero-based member order in the counted lane.
    pub ordinal: u32,
    /// Decoded compact index.
    pub member_index: u32,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

/// Wrapped operation member resolved in the feature-body identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyOperand {
    /// Globally unique operand identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Body clause containing the operand.
    pub body_object_index: u32,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Zero-based operand order in the wrapped member lane.
    pub ordinal: u32,
    /// Serialized operand body object index.
    pub operand_object_index: u32,
    /// Segment body bindings naming the same body image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_body_bindings: Vec<String>,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

/// Exact continuation following a `TRIM BODY` branch-`11` member lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBody11Continuation {
    /// Globally unique continuation identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Compact index in the single-entry continuation lane.
    pub continuation_index: u32,
    /// Absolute file offset of the continuation compact-index marker.
    pub continuation_source_offset: u64,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Absolute file offset of the terminal object-index marker.
    pub terminal_source_offset: u64,
}

/// Homogeneous value encoding in an operation body-reference lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureOperationBodyReferenceLaneEncoding {
    /// NX OM compact-index encoding.
    CompactIndex,
    /// `f0`/`f1` payload object-index encoding.
    PayloadObjectIndex,
}

/// Counted reference lane following an operation body scalar clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureOperationBodyReferenceLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning operation label.
    pub operation_label: String,
    /// Zero-based body-reference occurrence order.
    pub body_reference_ordinal: u32,
    /// Serialized body object index.
    pub body_object_index: u32,
    /// Branch discriminator following the body-reference terminator.
    pub branch: u8,
    /// Homogeneous encoding used by every lane value.
    pub encoding: FeatureOperationBodyReferenceLaneEncoding,
    /// Ordered decoded indices.
    pub object_indices: Vec<u32>,
    /// Absolute file offsets of the encoded index markers.
    pub source_offsets: Vec<u64>,
}

/// Atomically witnessed extrusion construction profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudeConstructionProfile {
    /// Globally unique construction-profile identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Body object anchoring the branch-`11` construction clause.
    pub body_object_index: u32,
    /// Ordered serialized profile object indices.
    pub object_indices: Vec<u32>,
    /// Ordered uniquely resolved profile data blocks.
    pub data_blocks: Vec<String>,
    /// Source offsets from the independently encoded profile field.
    pub profile_source_offsets: Vec<u64>,
    /// Source offsets from the body-clause reference lane.
    pub body_lane_source_offsets: Vec<u64>,
}

/// Structured `32` branch following an extrusion body-reference field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureExtrudePayload32Branch {
    /// Globally unique branch identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Finite shifted-IEEE scalar following the branch marker.
    pub scalar: f64,
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    pub atoms_be: Vec<u32>,
    /// Ordered values in the first compact-index lane.
    pub first_indices: Vec<u32>,
    /// Ordered values in the second compact-index lane.
    pub second_indices: Vec<u32>,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Absolute file offset of the `32` branch marker.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstructionReference {
    /// Globally unique construction-reference identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Zero-based reference order across the complete field.
    pub ordinal: u32,
    /// Whether this is the reference following the separator byte.
    pub terminal: bool,
    /// Serialized object index.
    pub object_index: u32,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Feature-history Boolean operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureBooleanKind {
    /// Add tool bodies to the target.
    Unite,
    /// Remove tool bodies from the target.
    Subtract,
    /// Retain target/tool intersections.
    Intersect,
}

/// Ordered target/tool binding from a feature-history Boolean operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBooleanOperation {
    /// Globally unique Boolean identity.
    pub id: String,
    /// Owning operation-label identity.
    pub operation_label: String,
    /// Boolean operation kind.
    pub kind: FeatureBooleanKind,
    /// Object index of the target body.
    pub target_object_index: u32,
    /// Ordered object indices of the tool bodies.
    pub tool_object_indices: Vec<u32>,
    /// Absolute file offset of the operation label tag.
    pub source_offset: u64,
}

/// Decode internally pointed record areas from linked OM sections.
pub fn om_record_areas(container: &Container) -> Vec<OmRecordArea> {
    let links = segment_om_links(container);
    let sections = container.om_sections();
    links
        .into_iter()
        .filter_map(|link| {
            let section = sections
                .iter()
                .find(|(entry, section)| {
                    entry
                        .file_span
                        .map_or(section.offset as u64, |(offset, _)| {
                            offset + section.offset as u64
                        })
                        == link.section_offset
                })?
                .1
                .clone();
            let header = section.record_area_header()?;
            let bytes = section.record_area?;
            let entry_offset = link.section_offset.checked_sub(section.offset as u64)?;
            Some(OmRecordArea {
                id: format!("nx:om-record-areas:area#{}", header.offset),
                section_link: link.id,
                schema_role: link.schema_role,
                control_words: header.control_words,
                product_version: header.product.value.to_string(),
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(bytes),
                source_offset: entry_offset + header.offset as u64,
            })
        })
        .collect()
}

/// Decode ordered operation labels from feature-history record areas.
pub fn feature_operation_labels(container: &Container) -> Vec<FeatureOperationLabel> {
    let sections = container.om_sections();
    let mut labels = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        labels.extend(section.operation_labels().into_iter().enumerate().map(
            |(ordinal, label)| FeatureOperationLabel {
                id: format!("nx:feature-history:operation-label#{section_key}-{ordinal}"),
                section_link: link.id.clone(),
                ordinal: ordinal as u32,
                value: label.value.to_string(),
                object_indices: label.object_indices,
                source_offset: entry_offset + label.offset as u64,
            },
        ));
    }
    labels
}

/// Decode ordered Boolean target/tool bindings from feature-history sections.
pub fn feature_boolean_operations(container: &Container) -> Vec<FeatureBooleanOperation> {
    let sections = container.om_sections();
    let mut operations = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let labels = section.operation_labels();
        for operation in section.boolean_operations() {
            let Some(ordinal) = labels
                .iter()
                .position(|label| label.offset == operation.offset)
            else {
                continue;
            };
            let kind = match operation.kind {
                crate::om::BooleanOperationKind::Unite => FeatureBooleanKind::Unite,
                crate::om::BooleanOperationKind::Subtract => FeatureBooleanKind::Subtract,
                crate::om::BooleanOperationKind::Intersect => FeatureBooleanKind::Intersect,
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{ordinal}");
            operations.push(FeatureBooleanOperation {
                id: format!("nx:feature-history:boolean#{section_key}-{ordinal}"),
                operation_label,
                kind,
                target_object_index: operation.target,
                tool_object_indices: operation.tools,
                source_offset: entry_offset + operation.offset as u64,
            });
        }
    }
    operations
}

/// Decode exact feature-operation record boundaries and byte identities.
pub fn feature_operation_records(container: &Container) -> Vec<FeatureOperationRecord> {
    let sections = container.om_sections();
    let mut records = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        records.extend(section.operation_records().into_iter().enumerate().map(
            |(ordinal, record)| {
                let operation_label =
                    format!("nx:feature-history:operation-label#{section_key}-{ordinal}");
                FeatureOperationRecord {
                    id: format!("nx:feature-history:operation-record#{section_key}-{ordinal}"),
                    operation_label,
                    ordinal: ordinal as u32,
                    byte_len: record.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                    payload_byte_len: record.payload.len() as u64,
                    payload_sha256: cadmpeg_ir::hash::sha256_hex(record.payload),
                    payload_source_offset: entry_offset + record.payload_offset as u64,
                    source_offset: entry_offset + record.offset as u64,
                }
            },
        ));
    }
    records
}

/// Decode ordered self-framed strings from feature-operation payloads.
pub fn feature_payload_strings(container: &Container) -> Vec<FeaturePayloadString> {
    let sections = container.om_sections();
    let mut strings = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let operation_record =
                format!("nx:feature-history:operation-record#{section_key}-{operation_ordinal}");
            strings.extend(
                crate::om::operation_payload_strings(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, value)| FeaturePayloadString {
                        id: format!(
                            "nx:feature-history:payload-string#{section_key}-{operation_ordinal}-{ordinal}"
                        ),
                        operation_record: operation_record.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.to_string(),
                        source_offset: entry_offset + value.offset as u64,
                    }),
            );
        }
    }
    strings
}

/// Decode primary body lineage references from feature-history operations.
pub fn feature_body_references(container: &Container) -> Vec<FeatureBodyReference> {
    let sections = container.om_sections();
    let mut references = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, reference) in section.operation_body_references() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{ordinal}");
            references.push(FeatureBodyReference {
                id: format!("nx:feature-history:body-reference#{section_key}-{ordinal}"),
                operation_label,
                body_object_index: reference.object_index,
                source_offset: entry_offset + reference.offset as u64,
            });
        }
    }
    references
}

/// Decode every ordered body-reference field from bounded feature operations.
pub fn feature_body_reference_occurrences(
    container: &Container,
) -> Vec<FeatureBodyReferenceOccurrence> {
    let sections = container.om_sections();
    let mut references = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal}");
            references.extend(
                crate::om::operation_body_references(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| FeatureBodyReferenceOccurrence {
                        id: format!(
                            "nx:feature-history:body-reference-occurrence#{section_key}-{operation_ordinal}-{ordinal}"
                        ),
                        operation_label: operation_label.clone(),
                        ordinal: ordinal as u32,
                        body_object_index: reference.object_index,
                        source_offset: entry_offset + reference.offset as u64,
                    }),
            );
        }
    }
    references
}

/// Return body objects whose latest decoded writer is not consumed by a later
/// Boolean, sewing, or trimming operation. Segment-bound bodies exist before
/// the retained history area unless a decoded operation writes them.
pub fn terminal_feature_body_indices(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
) -> Option<BTreeSet<u32>> {
    let sections = labels
        .iter()
        .map(|label| label.section_link.as_str())
        .collect::<BTreeSet<_>>();
    if sections.len() != 1 || (references.is_empty() && bindings.is_empty()) {
        return None;
    }
    let positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let aliases = body_alias_roots(bindings)?;
    let canonical = |identity: u32| aliases.get(&identity).copied().unwrap_or(identity);
    let mut last_writers = bindings
        .iter()
        .flat_map(|binding| [binding.body_object_index, binding.body_alias_object_index])
        .map(|identity| (canonical(identity), None))
        .collect::<BTreeMap<u32, Option<usize>>>();
    for reference in references {
        let position = *positions.get(reference.operation_label.as_str())?;
        last_writers.insert(canonical(reference.body_object_index), Some(position));
    }
    let mut consumed = BTreeSet::new();
    for operation in booleans {
        let position = *positions.get(operation.operation_label.as_str())?;
        for tool in &operation.tool_object_indices {
            let tool = canonical(*tool);
            if last_writers
                .get(&tool)
                .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
            {
                consumed.insert(tool);
            }
        }
    }
    let operation_kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    for operand in operands {
        if !matches!(
            operation_kinds.get(operand.operation_label.as_str()),
            Some(&("SEW" | "TRIM BODY"))
        ) {
            continue;
        }
        let position = *positions.get(operand.operation_label.as_str())?;
        let body = canonical(operand.operand_object_index);
        if last_writers
            .get(&body)
            .is_some_and(|writer| writer.is_none_or(|writer| writer < position))
        {
            consumed.insert(body);
        }
    }
    let terminal_roots = last_writers
        .into_keys()
        .filter(|body| !consumed.contains(body))
        .collect::<BTreeSet<_>>();
    Some(
        references
            .iter()
            .map(|reference| reference.body_object_index)
            .chain(
                bindings.iter().flat_map(|binding| {
                    [binding.body_object_index, binding.body_alias_object_index]
                }),
            )
            .filter(|identity| terminal_roots.contains(&canonical(*identity)))
            .collect(),
    )
}

/// Map each segment body identity to the smaller identity in its alias pair.
/// Conflicting pairs make body-image identity ambiguous and invalidate the map.
pub(crate) fn body_alias_roots(bindings: &[SegmentBodyBinding]) -> Option<BTreeMap<u32, u32>> {
    let mut roots = BTreeMap::new();
    for binding in bindings {
        let pair = [binding.body_object_index, binding.body_alias_object_index];
        let root = *pair.iter().min().expect("two aliases");
        for identity in pair {
            if roots
                .insert(identity, root)
                .is_some_and(|existing| existing != root)
            {
                return None;
            }
        }
    }
    Some(roots)
}

/// Resolve operation-header object indices to unique offset-only data blocks.
pub fn feature_input_blocks(container: &Container) -> Vec<FeatureInputBlock> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut inputs = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, label) in section.operation_labels().into_iter().enumerate() {
            for (input_slot, object_index) in label.object_indices.into_iter().enumerate() {
                let Some(object_index) = object_index else {
                    continue;
                };
                let Some(data_block) = unique_offset_data_block(&indexed, object_index) else {
                    continue;
                };
                let operation_label =
                    format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal}");
                inputs.push(FeatureInputBlock {
                    id: format!(
                        "nx:feature-history:input-block#{section_key}-{operation_ordinal}-{input_slot}"
                    ),
                    operation_label,
                    input_slot: input_slot as u8,
                    object_index,
                    data_block,
                    source_offset: entry_offset + label.object_index_offsets[input_slot] as u64,
                });
            }
        }
    }
    inputs
}

/// Join each sketch operation to its bounded record and ordered input blocks.
pub fn feature_sketch_records(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    inputs: &[FeatureInputBlock],
    references: &[FeatureSketchReference],
) -> Vec<FeatureSketchRecord> {
    labels
        .iter()
        .filter(|label| label.value == "SKETCH")
        .filter_map(|label| {
            let record = records
                .iter()
                .find(|record| record.operation_label == label.id)?;
            let mut input_blocks = inputs
                .iter()
                .filter(|input| input.operation_label == label.id)
                .collect::<Vec<_>>();
            input_blocks.sort_by_key(|input| input.input_slot);
            let mut payload_references = references
                .iter()
                .filter(|reference| reference.operation_label == label.id)
                .collect::<Vec<_>>();
            payload_references.sort_by_key(|reference| reference.ordinal);
            Some(FeatureSketchRecord {
                id: label.id.replacen("operation-label", "sketch-record", 1),
                operation_label: label.id.clone(),
                ordinal: label.ordinal,
                operation_record: record.id.clone(),
                input_blocks: input_blocks
                    .into_iter()
                    .map(|input| input.id.clone())
                    .collect(),
                payload_references: payload_references
                    .into_iter()
                    .map(|reference| reference.id.clone())
                    .collect(),
                source_offset: label.source_offset,
            })
        })
        .collect()
}

/// Join complete, uniquely resolved sketch construction-reference fields.
pub fn feature_sketch_construction_inputs(
    sketches: &[FeatureSketchRecord],
    references: &[FeatureSketchReference],
) -> Vec<FeatureSketchConstructionInputs> {
    let mut inputs = Vec::new();
    for sketch in sketches {
        let mut field = references
            .iter()
            .filter(|reference| reference.operation_label == sketch.operation_label)
            .collect::<Vec<_>>();
        field.sort_by_key(|reference| reference.ordinal);
        let Some(first) = field.first() else {
            continue;
        };
        let expected_len = usize::from(first.declared_count.max(1));
        if field.len() != expected_len
            || field.iter().enumerate().any(|(ordinal, reference)| {
                reference.declared_count != first.declared_count
                    || reference.ordinal != ordinal as u32
                    || reference.terminal != (ordinal + 1 == expected_len)
            })
        {
            continue;
        }
        let Some((terminal, members)) = field.split_last() else {
            continue;
        };
        let Some(member_data_blocks) = members
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(terminal_data_block) = terminal.data_block.clone() else {
            continue;
        };
        inputs.push(FeatureSketchConstructionInputs {
            id: sketch
                .id
                .replacen("sketch-record", "sketch-construction-inputs", 1),
            operation_label: sketch.operation_label.clone(),
            sketch_record: sketch.id.clone(),
            member_references: members
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            member_data_blocks,
            terminal_reference: terminal.id.clone(),
            terminal_data_block,
        });
    }
    inputs
}

/// Decode and resolve the ordered counted-reference field in sketch payloads.
pub fn feature_sketch_references(container: &Container) -> Vec<FeatureSketchReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let Some(decoded) = crate::om::sketch_payload_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal}");
            let declared_count = decoded.declared_count;
            let terminal_ordinal = decoded.references.len() - 1;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                let data_block = unique_offset_data_block(&indexed, reference.object_index);
                FeatureSketchReference {
                    id: format!(
                        "nx:feature-history:sketch-reference#{section_key}-{operation_ordinal}-{ordinal}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    declared_count,
                    terminal: ordinal == terminal_ordinal,
                    object_index: reference.object_index,
                    data_block,
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

/// Decode and resolve the witnessed ordered profile list in extrusion payloads.
pub fn feature_extrude_profile_references(
    container: &Container,
) -> Vec<FeatureExtrudeProfileReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let Some(decoded) = crate::om::extrude_profile_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal}");
            let witnessed = decoded.witnessed;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                FeatureExtrudeProfileReference {
                    id: format!(
                        "nx:feature-history:extrude-profile-reference#{section_key}-{operation_ordinal}-{ordinal}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    witnessed,
                    object_index: reference.object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

/// Decode fixed scalar headers from bounded extrusion payloads.
pub fn feature_extrude_payload_headers(container: &Container) -> Vec<FeatureExtrudePayloadHeader> {
    let sections = container.om_sections();
    let mut headers = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let Some(header) = crate::om::extrude_payload_header(record) else {
                continue;
            };
            headers.push(FeatureExtrudePayloadHeader {
                id: format!(
                    "nx:feature-history:extrude-payload-header#{section_key}-{operation_ordinal}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                ),
                scalars: header.scalars,
                source_offset: entry_offset + header.offset as u64,
            });
        }
    }
    headers
}

/// Decode typed scalar triples following extrusion body-reference fields.
pub fn feature_extrude_payload_scalar_triples(
    container: &Container,
) -> Vec<FeatureExtrudePayloadScalarTriple> {
    let sections = container.om_sections();
    let mut triples = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let Some(triple) = crate::om::extrude_payload_scalar_triple(record) else {
                continue;
            };
            let encoding = |encoding| match encoding {
                crate::om::PayloadScalarEncoding::Zero => FeaturePayloadScalarEncoding::Zero,
                crate::om::PayloadScalarEncoding::Binary32 => {
                    FeaturePayloadScalarEncoding::Binary32
                }
                crate::om::PayloadScalarEncoding::Binary64 => {
                    FeaturePayloadScalarEncoding::Binary64
                }
            };
            triples.push(FeatureExtrudePayloadScalarTriple {
                id: format!(
                    "nx:feature-history:extrude-payload-scalar-triple#{section_key}-{operation_ordinal}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                ),
                values: triple.scalars.map(|scalar| scalar.value),
                encodings: triple.scalars.map(|scalar| encoding(scalar.encoding)),
                source_offsets: triple
                    .scalars
                    .map(|scalar| entry_offset + scalar.offset as u64),
            });
        }
    }
    triples
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub fn feature_operation_body_scalar_triples(
    container: &Container,
) -> Vec<FeatureOperationBodyScalarTriple> {
    let sections = container.om_sections();
    let mut triples = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            for triple in crate::om::operation_body_scalar_triples(record) {
                let encoding = |encoding| match encoding {
                    crate::om::PayloadScalarEncoding::Zero => FeaturePayloadScalarEncoding::Zero,
                    crate::om::PayloadScalarEncoding::Binary32 => {
                        FeaturePayloadScalarEncoding::Binary32
                    }
                    crate::om::PayloadScalarEncoding::Binary64 => {
                        FeaturePayloadScalarEncoding::Binary64
                    }
                };
                triples.push(FeatureOperationBodyScalarTriple {
                    id: format!(
                        "nx:feature-history:operation-body-scalar-triple#{section_key}-{operation_ordinal}-{}",
                        triple.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                    ),
                    body_reference_ordinal: triple.body_reference_ordinal,
                    body_object_index: triple.body_object_index,
                    branch: triple.branch,
                    values: triple.scalars.map(|scalar| scalar.value),
                    encodings: triple.scalars.map(|scalar| encoding(scalar.encoding)),
                    source_offsets: triple
                        .scalars
                        .map(|scalar| entry_offset + scalar.offset as u64),
                });
            }
        }
    }
    triples
}

/// Decode ordered member lanes following branch-`11` operation body clauses.
pub fn feature_operation_body_members(container: &Container) -> Vec<FeatureOperationBodyMember> {
    let sections = container.om_sections();
    let mut members = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            members.extend(
                crate::om::operation_body_members(record)
                    .into_iter()
                    .map(|member| FeatureOperationBodyMember {
                        id: format!(
                            "nx:feature-history:operation-body-member#{section_key}-{operation_ordinal}-{}-{}",
                            member.body_reference_ordinal, member.ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                        ),
                        body_reference_ordinal: member.body_reference_ordinal,
                        body_object_index: member.body_object_index,
                        ordinal: member.ordinal,
                        member_index: member.member_index,
                        source_offset: entry_offset + member.offset as u64,
                    }),
            );
        }
    }
    members
}

/// Resolve wrapped operation members that name known feature-body identities.
pub fn feature_operation_body_operands(
    members: &[FeatureOperationBodyMember],
    references: &[FeatureBodyReferenceOccurrence],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureOperationBodyOperand> {
    let known = references
        .iter()
        .map(|reference| reference.body_object_index)
        .chain(
            bindings
                .iter()
                .flat_map(|binding| [binding.body_object_index, binding.body_alias_object_index]),
        )
        .collect::<BTreeSet<_>>();
    members
        .iter()
        .filter(|member| {
            member.member_index != member.body_object_index && known.contains(&member.member_index)
        })
        .map(|member| FeatureOperationBodyOperand {
            id: member
                .id
                .replacen("operation-body-member", "operation-body-operand", 1),
            operation_label: member.operation_label.clone(),
            body_object_index: member.body_object_index,
            body_reference_ordinal: member.body_reference_ordinal,
            ordinal: member.ordinal,
            operand_object_index: member.member_index,
            segment_body_bindings: bindings
                .iter()
                .filter(|binding| {
                    binding.body_object_index == member.member_index
                        || binding.body_alias_object_index == member.member_index
                })
                .map(|binding| binding.id.clone())
                .collect(),
            source_offset: member.source_offset,
        })
        .collect()
}

/// Decode exact continuations following `TRIM BODY` branch-`11` member lanes.
pub fn feature_operation_body_11_continuations(
    container: &Container,
) -> Vec<FeatureOperationBody11Continuation> {
    let sections = container.om_sections();
    let mut continuations = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            continuations.extend(
                crate::om::operation_body_11_continuations(record)
                    .into_iter()
                    .map(|continuation| FeatureOperationBody11Continuation {
                        id: format!(
                            "nx:feature-history:trim-body-11-continuation#{section_key}-{operation_ordinal}-{}",
                            continuation.body_reference_ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                        ),
                        body_reference_ordinal: continuation.body_reference_ordinal,
                        body_object_index: continuation.body_object_index,
                        continuation_index: continuation.continuation_index,
                        continuation_source_offset: entry_offset
                            + continuation.continuation_offset as u64,
                        terminal_object_index: continuation.terminal_object_index,
                        terminal_source_offset: entry_offset + continuation.terminal_offset as u64,
                    }),
            );
        }
    }
    continuations
}

/// Decode complete unwrapped counted reference lanes following body scalar clauses.
pub fn feature_operation_body_reference_lanes(
    container: &Container,
) -> Vec<FeatureOperationBodyReferenceLane> {
    let sections = container.om_sections();
    let mut lanes = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            for lane in crate::om::operation_body_reference_lanes(record) {
                let encoding = match lane.encoding {
                    crate::om::OperationBodyReferenceLaneEncoding::CompactIndex => {
                        FeatureOperationBodyReferenceLaneEncoding::CompactIndex
                    }
                    crate::om::OperationBodyReferenceLaneEncoding::PayloadObjectIndex => {
                        FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
                    }
                };
                lanes.push(FeatureOperationBodyReferenceLane {
                    id: format!(
                        "nx:feature-history:operation-body-reference-lane#{section_key}-{operation_ordinal}-{}",
                        lane.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                    ),
                    body_reference_ordinal: lane.body_reference_ordinal,
                    body_object_index: lane.body_object_index,
                    branch: lane.branch,
                    encoding,
                    object_indices: lane.values.iter().map(|value| value.object_index).collect(),
                    source_offsets: lane
                        .values
                        .iter()
                        .map(|value| entry_offset + value.offset as u64)
                        .collect(),
                });
            }
        }
    }
    lanes
}

/// Join the two exact encodings of an extrusion construction profile.
pub fn feature_extrude_construction_profiles(
    references: &[FeatureExtrudeProfileReference],
    lanes: &[FeatureOperationBodyReferenceLane],
) -> Vec<FeatureExtrudeConstructionProfile> {
    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureExtrudeProfileReference>>::new();
    for reference in references {
        references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut profiles = Vec::new();
    for (operation_label, mut operation_references) in references_by_operation {
        operation_references.sort_by_key(|reference| reference.ordinal);
        if operation_references.is_empty()
            || operation_references
                .iter()
                .any(|reference| !reference.witnessed)
        {
            continue;
        }
        let object_indices = operation_references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>();
        let matching_lanes = lanes
            .iter()
            .filter(|lane| {
                lane.operation_label == operation_label
                    && lane.branch == 0x11
                    && lane.encoding
                        == FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex
                    && lane.object_indices == object_indices
            })
            .collect::<Vec<_>>();
        let [lane] = matching_lanes.as_slice() else {
            continue;
        };
        let Some(data_blocks) = operation_references
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        profiles.push(FeatureExtrudeConstructionProfile {
            id: operation_label.replacen("operation-label", "extrude-construction-profile", 1),
            operation_label: operation_label.to_string(),
            body_object_index: lane.body_object_index,
            object_indices,
            data_blocks,
            profile_source_offsets: operation_references
                .iter()
                .map(|reference| reference.source_offset)
                .collect(),
            body_lane_source_offsets: lane.source_offsets.clone(),
        });
    }
    profiles
}

/// Decode structured `32` branches following extrusion body-reference fields.
pub fn feature_extrude_payload_32_branches(
    container: &Container,
) -> Vec<FeatureExtrudePayload32Branch> {
    let sections = container.om_sections();
    let mut branches = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let Some(branch) = crate::om::extrude_payload_32_branch(record) else {
                continue;
            };
            branches.push(FeatureExtrudePayload32Branch {
                id: format!(
                    "nx:feature-history:extrude-payload-32-branch#{section_key}-{operation_ordinal}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                ),
                scalar: branch.scalar,
                atoms_be: branch.atoms_be,
                first_indices: branch.first_indices,
                second_indices: branch.second_indices,
                terminal_object_index: branch.terminal_object_index,
                source_offset: entry_offset + branch.offset as u64,
            });
        }
    }
    branches
}

/// Decode and resolve ordered construction references in `BLOCK` payloads.
pub fn feature_block_construction_references(
    container: &Container,
) -> Vec<FeatureBlockConstructionReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for link in segment_om_links(container)
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
    {
        let Some((entry, section)) = sections.iter().find(|(entry, section)| {
            entry
                .file_span
                .map_or(section.offset as u64, |(offset, _)| {
                    offset + section.offset as u64
                })
                == link.section_offset
        }) else {
            continue;
        };
        let section_key = link.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records().into_iter().enumerate() {
            let Some(field) = crate::om::block_construction_references(record) else {
                continue;
            };
            let terminal_ordinal = field.references.len() - 1;
            references.extend(field.references.into_iter().enumerate().map(
                |(ordinal, reference)| FeatureBlockConstructionReference {
                    id: format!(
                        "nx:feature-history:block-construction-reference#{section_key}-{operation_ordinal}-{ordinal}"
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                    ),
                    control: field.control,
                    ordinal: ordinal as u32,
                    terminal: ordinal == terminal_ordinal,
                    object_index: reference.object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    source_offset: entry_offset + reference.offset as u64,
                },
            ));
        }
    }
    references
}

fn unique_offset_data_block(
    indexed: &[(&crate::container::DirEntry, crate::om::IndexedSection<'_>)],
    object_index: u32,
) -> Option<String> {
    let candidates = indexed
        .iter()
        .enumerate()
        .filter(|(_, (_, candidate))| {
            candidate
                .records
                .first()
                .is_some_and(|record| record.object_id.is_none())
                && candidate.records.get(object_index as usize).is_some()
        })
        .map(|(section_ordinal, (_, candidate))| {
            let block_ordinal = object_index as usize + usize::from(candidate.control.is_some());
            format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}")
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

/// Join operation input lanes to uniquely resolved parameter declarations.
pub fn feature_parameter_bindings(
    inputs: &[FeatureInputBlock],
    references: &[DataBlockReference],
) -> Vec<FeatureParameterBinding> {
    let mut bindings = Vec::new();
    for input in inputs {
        for reference in references
            .iter()
            .filter(|reference| reference.data_block == input.data_block)
        {
            let Some(expression_declaration) = &reference.target_expression_declaration else {
                continue;
            };
            bindings.push(FeatureParameterBinding {
                id: format!("{}:parameter#{}", input.id, reference.ordinal),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                input_block: input.data_block.clone(),
                reference_ordinal: reference.ordinal,
                expression_declaration: expression_declaration.clone(),
                object_id: reference.object_id,
                source_offset: reference.source_offset,
            });
        }
    }
    bindings
}

/// Resolve segment-index words that point to validated framed OM sections.
pub fn segment_om_links(container: &Container) -> Vec<SegmentOmLink> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let entry_start = usize::try_from(entry_offset).expect("in-bounds directory offset");
    let sections = container
        .om_sections()
        .into_iter()
        .filter(|(candidate, _)| candidate.name == entry.name)
        .map(|(_, section)| {
            let has = |name| {
                section
                    .types
                    .iter()
                    .any(|definition| definition.name == name)
            };
            let role = if has("UGS::FEATURE_RECORD") {
                OmSchemaRole::FeatureHistory
            } else if has("UGS::EXP_expression") {
                OmSchemaRole::Expressions
            } else if has("UGS::Solid::Topol") {
                OmSchemaRole::Model
            } else if has("UGS::OM::SaveAuditTrail") {
                OmSchemaRole::AuditTrail
            } else {
                OmSchemaRole::Other
            };
            (section.offset, role)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows.into_iter().enumerate() {
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = relative as usize;
            let (separator_byte_len, schema_role) = if let Some(role) = sections.get(&relative) {
                (0usize, *role)
            } else if container
                .data
                .get(entry_start + relative..entry_start + relative + 4)
                == Some(&[0xc0, 0xd1, 0xf1, 0xed])
            {
                let Some(role) = sections.get(&(relative + 4)) else {
                    continue;
                };
                (4, *role)
            } else {
                continue;
            };
            links.push(SegmentOmLink {
                id: format!("nx:segment-om-links:link#{}", links.len()),
                row: format!("nx:segment-index:row#{row_ordinal}"),
                slot,
                schema_role,
                separator_byte_len: separator_byte_len as u32,
                source_offset: entry_offset + relative as u64,
                section_offset: entry_offset + relative as u64 + separator_byte_len as u64,
            });
        }
    }
    links
}

/// Resolve segment-index words that point to validated compressed wrappers.
pub fn segment_stream_links(container: &Container, streams: &[Stream]) -> Vec<SegmentStreamLink> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let entry_start = usize::try_from(entry_offset).expect("in-bounds directory offset");
    let mut links = Vec::new();
    for (row_ordinal, row) in index.rows.into_iter().enumerate() {
        for (slot, relative) in [
            (SegmentIndexSlot::TypeCode, row.type_code),
            (SegmentIndexSlot::SubtypeCode, row.subtype_code),
            (SegmentIndexSlot::Value, row.value),
        ] {
            let relative = relative as usize;
            let Some(wrapper) = container.data.get(entry_start + relative..) else {
                continue;
            };
            let Some(wrapper_word) = cadmpeg_ir::le::u32_at(wrapper, 0) else {
                continue;
            };
            let extension = (wrapper_word & 0x3fff_ffff) as usize;
            let wrapper_byte_len = match wrapper_word & 0xc000_0000 {
                0x8000_0000 => 8usize.checked_add(extension),
                0xc000_0000 => 33usize.checked_add(extension),
                _ => continue,
            };
            let Some(wrapper_byte_len) = wrapper_byte_len else {
                continue;
            };
            let zlib_offset = entry_start + relative + wrapper_byte_len;
            let Some((stream_ordinal, stream)) = streams
                .iter()
                .enumerate()
                .find(|(_, stream)| stream.file_offset == zlib_offset)
            else {
                continue;
            };
            links.push(SegmentStreamLink {
                id: format!("nx:segment-stream-links:link#{}", links.len()),
                row: format!("nx:segment-index:row#{row_ordinal}"),
                slot,
                stream_ordinal: stream_ordinal as u32,
                stream_kind: match stream.kind {
                    StreamKind::Partition => "partition",
                    StreamKind::Deltas => "deltas",
                    StreamKind::Plain => "plain",
                    StreamKind::Preview => "preview",
                }
                .to_string(),
                wrapper_byte_len: wrapper_byte_len as u32,
                source_offset: entry_offset + relative as u64,
            });
        }
    }
    links
}

/// Bind partition and cached-body streams to feature-history body object indices.
pub fn segment_body_bindings(container: &Container, streams: &[Stream]) -> Vec<SegmentBodyBinding> {
    let Some((entry, index)) = container.segment_index() else {
        return Vec::new();
    };
    let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
    let words = index
        .rows
        .iter()
        .flat_map(|row| [row.type_code, row.subtype_code, row.value])
        .collect::<Vec<_>>();
    segment_stream_links(container, streams)
        .into_iter()
        .filter(|link| matches!(link.stream_kind.as_str(), "partition" | "plain"))
        .filter_map(|link| {
            let row = link.row.rsplit_once('#')?.1.parse::<usize>().ok()?;
            let slot = match link.slot {
                SegmentIndexSlot::TypeCode => 0,
                SegmentIndexSlot::SubtypeCode => 1,
                SegmentIndexSlot::Value => 2,
            };
            let pointer_word = row.checked_mul(3)?.checked_add(slot)?;
            (words.get(pointer_word + 1) == Some(&0)).then_some(())?;
            let body_object_index = *words.get(pointer_word + 2)?;
            let body_alias_object_index = *words.get(pointer_word + 3)?;
            let stream_role = *words.get(pointer_word + 4)?;
            (body_object_index != 0 && body_alias_object_index != 0).then_some(())?;
            Some(SegmentBodyBinding {
                id: format!("nx:segment-body-bindings:binding#{}", link.stream_ordinal),
                stream_link: link.id,
                stream_ordinal: link.stream_ordinal,
                stream_kind: link.stream_kind,
                body_object_index,
                body_alias_object_index,
                stream_role,
                source_offset: entry_offset + ((pointer_word + 2) * 4) as u64,
            })
        })
        .collect()
}

/// Retain named attribute-class declarations from all Parasolid streams.
pub fn parasolid_attribute_definitions(streams: &[Stream]) -> Vec<ParasolidAttributeDefinition> {
    streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::attribute_definitions(&stream.inflated)
                .into_iter()
                .map(move |definition| ParasolidAttributeDefinition {
                    id: format!(
                        "nx:s{stream_ordinal}:attribute-definition#{}",
                        definition.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: definition.xmt,
                    name: definition.name.to_string(),
                    field_count: definition.field_count,
                    field_record_xmt: definition.field_record_xmt,
                    field_record_references: definition.field_record_references,
                    field_record_header_words: definition.field_record_header_words,
                    inflated_offset: definition.offset as u64,
                })
        })
        .collect()
}

/// Unit declared by an NX numeric expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionUnit {
    /// Canonical model length in millimeters.
    Millimeter,
    /// Angular value in degrees as stored by NX.
    Degree,
}

/// Named parameter declaration in a bounded NX expression object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionDeclaration {
    /// Globally unique declaration identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: u32,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Exact NX parameter name.
    pub name: String,
    /// Decimal source parameter identifier following `p`.
    pub parameter_index: u32,
    /// Qualified role following the parameter identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    /// Independently framed constant numeric expression in the declaration record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    /// Directory entry containing the declaration record.
    pub source_entry: String,
    /// Absolute file offset of the declaration-name marker.
    pub source_offset: u64,
}

/// Explicit numeric expression serialized in one NX OM entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    /// Globally unique native-record identity.
    pub id: String,
    /// Persistent OM object identifier.
    pub object_id: Option<u32>,
    /// Owning entry in the native OM record directory, when externally bounded.
    pub record: Option<String>,
    /// Exact-name declaration record for this parameter, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<String>,
    /// NX parameter name.
    pub name: String,
    /// Decimal source parameter identifier following the leading `p`.
    pub parameter_index: Option<u32>,
    /// Qualified role following the parameter identifier.
    pub qualifier: Option<String>,
    /// Declared native unit.
    pub unit: ExpressionUnit,
    /// Exact serialized expression text.
    pub expression: String,
    /// Finite numeric value after context-free and dependency-graph evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the expression text.
    pub source_offset: u64,
}

/// Return exact `p<decimal>[_qualifier]` references in formula occurrence order.
pub(crate) fn expression_parameter_names(expression: &str) -> Vec<&str> {
    let bytes = expression.as_bytes();
    let mut names = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] != b'p'
            || at
                .checked_sub(1)
                .and_then(|before| bytes.get(before))
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            at += 1;
            continue;
        }
        let start = at + 1;
        let mut end = start;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            at += 1;
            continue;
        }
        while bytes
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            end += 1;
        }
        names.push(&expression[at..end]);
        at = end;
    }
    names
}

/// Length-framed class definition from an NX OM type registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// Globally unique native-record identity.
    pub id: String,
    /// Registered `UGS::` class name.
    pub name: String,
    /// Zero-based declaration ordinal used as class identity.
    pub ordinal: u32,
    /// Declaration code serialized after the class name.
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next class declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Variable-width prefix of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_prefix: Vec<u8>,
    /// Stable eight-byte class fingerprint in a framed registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_fingerprint: Option<[u8; 8]>,
    /// Terminal byte of a framed indexed-store registry suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_terminal: Option<u8>,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the definition's length byte.
    pub source_offset: u64,
}

/// Member declaration from an NX OM field registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Globally unique declaration identity.
    pub id: String,
    /// Registered `m_` member name.
    pub name: String,
    /// Zero-based declaration ordinal within its section.
    pub ordinal: u32,
    /// Declaration code serialized immediately after the name.
    pub trailing_code: u8,
    /// Exact bytes between this declaration core and the next member declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registry_suffix: Vec<u8>,
    /// Absolute file offset of the containing OM section signature.
    pub section_offset: u64,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the declaration length byte.
    pub source_offset: u64,
}

/// Directory entry for one externally bounded NX OM entity record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based record ordinal within the indexed section.
    pub record_ordinal: u32,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Exact serialized record length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized record bytes.
    pub sha256: String,
    /// Ordered distinct same-section records referenced by this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Ordered distinct same-section records that reference this record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the record start.
    pub source_offset: u64,
}

/// One externally bounded block in an NX OM offset-only column store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlock {
    /// Globally unique block identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based block ordinal within the offset-only section.
    pub block_ordinal: u32,
    /// Whether this is the store control block or one data column block.
    pub role: DataBlockRole,
    /// Absolute file offset of the containing OM section base.
    pub section_offset: u64,
    /// Exact serialized block length.
    pub byte_len: u64,
    /// SHA-256 of the exact serialized block bytes.
    pub sha256: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the block start.
    pub source_offset: u64,
}

/// Ordered object reference carried by an offset-only OM data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based reference order within the block.
    pub ordinal: u32,
    /// Referenced persistent OM object ID.
    pub object_id: u32,
    /// Uniquely resolved object record in the same directory entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_record: Option<String>,
    /// Uniquely resolved parameter declaration carrying this object ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_expression_declaration: Option<String>,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Product/version header from one indexed NX OM store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreHeader {
    /// Globally unique store-header identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Persistent object identity when the header belongs to an ID-bounded record.
    pub object_id: Option<u32>,
    /// Exact printable product/version text.
    pub version: String,
    /// Directory entry containing the OM store.
    pub source_entry: String,
    /// Absolute file offset of the `04 01` marker.
    pub source_offset: u64,
}

/// Role of one bounded block in an offset-only NX OM store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataBlockRole {
    /// Store-level schema and root metadata from boundary slot zero.
    Control,
    /// One offset-bounded column-storage block.
    Column,
}

/// Self-framed printable string carried by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the `66 32 03` marker.
    pub source_offset: u64,
}

/// Tagged reference family serialized in an NX OM record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectReferenceKind {
    /// `e0` marker followed by a 32-bit persistent handle.
    PersistentHandle,
    /// Four-byte `0xC?` tagged 28-bit reference.
    Tagged28,
    /// Count-framed `90` reference to a record ordinal in the same section.
    RecordOrdinal16,
}

/// Ordered tagged-reference occurrence owned by one NX OM record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning entry in the native OM record directory.
    pub record: String,
    /// Persistent OM object identifier when the section carries an ID table.
    pub object_id: Option<u32>,
    /// Zero-based occurrence ordinal within the owning record.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker/tag bits.
    pub value: u32,
    /// Resolved target in the native OM record directory.
    pub target_record: Option<String>,
    /// Directory entry containing the OM section.
    pub source_entry: String,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Cross-record identity established by equal persistent-handle values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentHandle {
    /// Globally unique handle identity.
    pub id: String,
    /// Unsigned persistent-handle value.
    pub value: u32,
    /// Ordered distinct OM directory records containing the handle.
    pub records: Vec<String>,
    /// Total number of serialized occurrences across those records.
    pub occurrence_count: u32,
    /// Ordered distinct EXTREFSTREAM records containing the same handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_records: Vec<String>,
}

/// Named NX arrangement from `/Root/part/arrangements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration {
    /// Globally unique native-record identity.
    pub id: String,
    /// Arrangement name.
    pub name: String,
    /// Whether NX marks this arrangement as the default.
    pub active: bool,
    /// Directory entry containing the arrangement XML.
    pub source_entry: String,
    /// Absolute file offset of the arrangement element.
    pub source_offset: u64,
}

/// One typed part-level attribute from `/Root/part/attrs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartAttribute {
    /// Globally unique native-record identity.
    pub id: String,
    /// Attribute owner token.
    pub owner: String,
    /// UTF-8 attribute title.
    pub title: String,
    /// UTF-8 attribute value.
    pub value: String,
    /// XML schema type token.
    pub value_type: String,
    /// Whether product-data management owns the value.
    pub pdm_based: bool,
    /// Attribute record schema version.
    pub version: u32,
    /// Directory entry containing the attribute XML.
    pub source_entry: String,
    /// Absolute file offset of the attribute element.
    pub source_offset: u64,
}

/// End-anchored child-part string from an NX external-reference stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReference {
    /// Globally unique native-record identity.
    pub id: String,
    /// Zero-based string-table ordinal within the stream.
    pub ordinal: u32,
    /// Exact serialized child-part name or path.
    pub path: String,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the first path byte.
    pub source_offset: u64,
}

/// Indexed EXTREFSTREAM record prefix with its exact handle membership set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReferenceRecord {
    /// Globally unique native-record identity.
    pub id: String,
    /// Record type from the external-reference directory.
    pub record_id: u32,
    /// Count declared before the four ID slots.
    pub declared_count: u16,
    /// Four uninterpreted little-endian ID slots.
    pub id_slots: [u32; 4],
    /// Strictly ascending persistent handles; the serialized closing duplicate is omitted.
    pub handles: Vec<u32>,
    /// Whether the final serialized handle repeats the preceding handle.
    pub closing_duplicate: bool,
    /// Length of the decoded record prefix.
    pub prefix_byte_len: u64,
    /// Length after the decoded handle-set prefix and before the next record or string table.
    pub tail_byte_len: u64,
    /// Directory entry containing the external-reference stream.
    pub source_entry: String,
    /// Absolute file offset of the record marker.
    pub source_offset: u64,
}

/// Decode end-anchored external child-part string tables.
pub fn external_references(container: &Container) -> Vec<ExternalReference> {
    let mut ordinals = BTreeMap::<String, u32>::new();
    container
        .external_reference_strings()
        .into_iter()
        .map(|(entry, relative, path)| {
            let ordinal = ordinals.entry(entry.name.clone()).or_default();
            let current = *ordinal;
            *ordinal += 1;
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            ExternalReference {
                id: format!("nx:external-reference:{}#{current}", entry.name),
                ordinal: current,
                path,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + relative as u64,
            }
        })
        .collect()
}

/// Decode exact indexed external-reference record prefixes.
pub fn external_reference_records(container: &Container) -> Vec<ExternalReferenceRecord> {
    container
        .external_reference_records()
        .into_iter()
        .map(|(entry, record)| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            ExternalReferenceRecord {
                id: format!(
                    "nx:external-reference-record:{}#{}",
                    entry.name, record.record_id
                ),
                record_id: record.record_id,
                declared_count: record.declared_count,
                id_slots: record.id_slots,
                handles: record.handles,
                closing_duplicate: record.closing_duplicate,
                prefix_byte_len: record.prefix_byte_len as u64,
                tail_byte_len: record.tail_byte_len as u64,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + record.offset as u64,
            }
        })
        .collect()
}

/// Decode the explicit NX arrangement table.
pub fn configurations(container: &Container) -> Vec<Configuration> {
    container
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.name == "/Root/part/arrangements")
        .filter_map(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let (offset_usize, size) = (usize::try_from(offset).ok()?, usize::try_from(size).ok()?);
            let payload = container
                .data
                .get(offset_usize..offset_usize.checked_add(size)?)?;
            let xml = std::str::from_utf8(payload).ok()?;
            let document = roxmltree::Document::parse(xml).ok()?;
            let root = document.root_element();
            if root.tag_name().name() != "Arrangements" {
                return None;
            }

            let mut active_count = 0usize;
            let mut configurations = Vec::new();
            for (ordinal, node) in root
                .children()
                .filter(roxmltree::Node::is_element)
                .enumerate()
            {
                if node.tag_name().name() != "Arrangement" {
                    return None;
                }
                let name = node.attribute("Name")?;
                if name.is_empty() {
                    return None;
                }
                let active = match node.attribute("Default")? {
                    "YES" => true,
                    "NO" => false,
                    _ => return None,
                };
                active_count += usize::from(active);
                configurations.push(Configuration {
                    id: format!("nx:arrangements-{entry_index}:configuration#{ordinal}"),
                    name: name.to_string(),
                    active,
                    source_entry: entry.name.clone(),
                    source_offset: offset + node.range().start as u64,
                });
            }
            (!configurations.is_empty() && active_count <= 1).then_some(configurations)
        })
        .flatten()
        .collect()
}

/// Decode the typed part-attribute XML stream atomically.
pub fn part_attributes(container: &Container) -> Vec<PartAttribute> {
    container
        .entries
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == "/Root/part/attrs")
        .and_then(|(entry_index, entry)| {
            let (offset, size) = entry.file_span?;
            let start = usize::try_from(offset).ok()?;
            let payload = container
                .data
                .get(start..start.checked_add(usize::try_from(size).ok()?)?)?;
            parse_part_attributes(payload, entry_index, &entry.name, offset)
        })
        .unwrap_or_default()
}

pub(crate) fn parse_part_attributes(
    payload: &[u8],
    entry_index: usize,
    source_entry: &str,
    entry_offset: u64,
) -> Option<Vec<PartAttribute>> {
    let document = roxmltree::Document::parse(std::str::from_utf8(payload).ok()?).ok()?;
    let root = document.root_element();
    if root.tag_name().name() != "UgAttributes"
        || root.attribute("version")?.parse::<u32>().ok()? < 4
    {
        return None;
    }
    root.children()
        .filter(roxmltree::Node::is_element)
        .enumerate()
        .map(|(ordinal, node)| {
            if node.tag_name().name() != "Attribute" {
                return None;
            }
            Some(PartAttribute {
                id: format!("nx:part-attributes-{entry_index}:attribute#{ordinal}"),
                owner: node.attribute("owner")?.to_string(),
                title: node
                    .attribute("utf8title")
                    .or_else(|| node.attribute("title"))?
                    .to_string(),
                value: node
                    .attribute("utf8value")
                    .or_else(|| node.attribute("value"))?
                    .to_string(),
                value_type: node.attribute("type")?.to_string(),
                pdm_based: match node.attribute("pdmBased")? {
                    "true" => true,
                    "false" => false,
                    _ => return None,
                },
                version: node.attribute("version")?.parse().ok()?,
                source_entry: source_entry.to_string(),
                source_offset: entry_offset + node.range().start as u64,
            })
        })
        .collect()
}

/// Decode class definitions from every framed OM section.
pub fn class_definitions(container: &Container) -> Vec<ClassDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                class_layout_fields(definition.registry_suffix);
            definitions.insert(
                (entry_index, definition.offset),
                ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix,
                    schema_fingerprint,
                    layout_terminal,
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.types.into_iter().enumerate() {
            let (layout_prefix, schema_fingerprint, layout_terminal) =
                class_layout_fields(definition.registry_suffix);
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| ClassDefinition {
                    id: format!("nx:om-entry-{entry_index}:class#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    layout_prefix,
                    schema_fingerprint,
                    layout_terminal,
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

fn class_layout_fields(suffix: &[u8]) -> (Vec<u8>, Option<[u8; 8]>, Option<u8>) {
    if !(11..=14).contains(&suffix.len()) {
        return (Vec::new(), None, None);
    }
    let fingerprint_start = suffix.len() - 9;
    (
        suffix[..fingerprint_start].to_vec(),
        suffix[fingerprint_start..fingerprint_start + 8]
            .try_into()
            .ok(),
        suffix.last().copied(),
    )
}

/// Decode member definitions from every framed OM section.
pub fn field_definitions(container: &Container) -> Vec<FieldDefinition> {
    let mut definitions = BTreeMap::new();
    for (entry, section) in container.om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("OM entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, definition) in section.fields.into_iter().enumerate() {
            definitions.insert(
                (entry_index, definition.offset),
                FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    section_offset: entry_offset + section.offset as u64,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                },
            );
        }
    }
    for (entry, section) in container.indexed_om_sections() {
        let entry_index = container
            .entries
            .iter()
            .position(|candidate| std::ptr::eq(candidate, entry))
            .expect("indexed entry belongs to container");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let section_offset = entry_offset + section.base_offset() as u64;
        for (ordinal, definition) in section.fields.into_iter().enumerate() {
            definitions
                .entry((entry_index, definition.offset))
                .or_insert_with(|| FieldDefinition {
                    id: format!("nx:om-entry-{entry_index}:field#{}", definition.offset),
                    name: definition.name.to_string(),
                    ordinal: ordinal as u32,
                    trailing_code: definition.trailing_code,
                    registry_suffix: definition.registry_suffix.to_vec(),
                    section_offset,
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + definition.offset as u64,
                });
        }
    }
    definitions.into_values().collect()
}

/// Catalog every externally bounded NX OM entity record.
pub fn object_records(container: &Container) -> Vec<ObjectRecord> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_offset = entry_offset + section.base_offset() as u64;
            let mut dependencies = BTreeMap::<usize, Vec<usize>>::new();
            let mut dependents = BTreeMap::<usize, Vec<usize>>::new();
            for (source, _, _, reference) in section.references() {
                if reference.kind != crate::om::ReferenceKind::RecordOrdinal16 {
                    continue;
                }
                let target = reference.value as usize;
                let outgoing = dependencies.entry(source).or_default();
                if !outgoing.contains(&target) {
                    outgoing.push(target);
                }
                let incoming = dependents.entry(target).or_default();
                if !incoming.contains(&source) {
                    incoming.push(source);
                }
            }
            section
                .records
                .into_iter()
                .enumerate()
                .map(move |(record_ordinal, record)| {
                    let record_id = |ordinal| {
                        format!("nx:om-record-directory-{section_ordinal}:entry#{ordinal}")
                    };
                    ObjectRecord {
                        id: record_id(record_ordinal),
                        object_id: record.object_id,
                        section_ordinal: section_ordinal as u32,
                        record_ordinal: record_ordinal as u32,
                        section_offset,
                        byte_len: record.bytes.len() as u64,
                        sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                        dependencies: dependencies
                            .get(&record_ordinal)
                            .into_iter()
                            .flatten()
                            .map(|ordinal| record_id(*ordinal))
                            .collect(),
                        dependents: dependents
                            .get(&record_ordinal)
                            .into_iter()
                            .flatten()
                            .map(|ordinal| record_id(*ordinal))
                            .collect(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + record.offset as u64,
                    }
                })
                .collect()
        })
        .collect()
}

/// Catalog every externally bounded block in offset-only NX OM storage.
pub fn data_blocks(container: &Container) -> Vec<DataBlock> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let section_offset = entry_offset + section.base_offset() as u64;
            let mut source_blocks = Vec::with_capacity(section.records.len() + 1);
            if let Some(control) = section.control {
                source_blocks.push((DataBlockRole::Control, control));
            }
            source_blocks.extend(
                section
                    .records
                    .into_iter()
                    .map(|block| (DataBlockRole::Column, block)),
            );
            source_blocks
                .into_iter()
                .enumerate()
                .map(move |(block_ordinal, (role, block))| DataBlock {
                    id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
                    section_ordinal: section_ordinal as u32,
                    block_ordinal: block_ordinal as u32,
                    role,
                    section_offset,
                    byte_len: block.bytes.len() as u64,
                    sha256: cadmpeg_ir::hash::sha256_hex(block.bytes),
                    source_entry: entry.name.clone(),
                    source_offset: entry_offset + block.offset as u64,
                })
                .collect()
        })
        .collect()
}

/// Decode framed object references from offset-only OM data blocks.
pub fn data_block_references(container: &Container) -> Vec<DataBlockReference> {
    let mut records = BTreeMap::<(String, u32), Vec<String>>::new();
    for record in object_records(container) {
        let Some(object_id) = record.object_id else {
            continue;
        };
        records
            .entry((record.source_entry, object_id))
            .or_default()
            .push(record.id);
    }
    let mut declarations = BTreeMap::<(String, u32), Vec<String>>::new();
    for declaration in expression_declarations(container) {
        declarations
            .entry((declaration.source_entry, declaration.object_id))
            .or_default()
            .push(declaration.id);
    }
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_some())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let mut source_blocks = Vec::with_capacity(section.records.len() + 1);
            if let Some(control) = section.control {
                source_blocks.push(control);
            }
            source_blocks.extend(section.records);
            source_blocks
                .into_iter()
                .enumerate()
                .flat_map(|(block_ordinal, block)| {
                    crate::om::data_block_object_references(block.bytes)
                        .into_iter()
                        .enumerate()
                        .map(|(ordinal, reference)| {
                            let key = (entry.name.clone(), reference.object_index);
                            let unique = |candidates: Option<&Vec<String>>| {
                                let [target] = candidates?.as_slice() else {
                                    return None;
                                };
                                Some(target.clone())
                            };
                            DataBlockReference {
                                id: format!(
                                    "nx:om-data-block-references-{section_ordinal}-{block_ordinal}:reference#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                object_id: reference.object_index,
                                target_record: unique(records.get(&key)),
                                target_expression_declaration: unique(declarations.get(&key)),
                                source_offset: entry_offset
                                    + block.offset as u64
                                    + reference.offset as u64,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode one product/version header from each indexed NX OM store.
pub fn store_headers(container: &Container) -> Vec<StoreHeader> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .filter_map(|(section_ordinal, (entry, section))| {
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .control
                .iter()
                .chain(section.records.iter())
                .find_map(|record| {
                    crate::om::store_version(record.bytes, record.offset).map(|version| {
                        StoreHeader {
                            id: format!("nx:om-store-headers:store#{section_ordinal}"),
                            section_ordinal: section_ordinal as u32,
                            object_id: record.object_id,
                            version: version.value.to_string(),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + version.offset as u64,
                        }
                    })
                })
        })
        .collect()
}

/// Decode self-framed printable values from bounded NX OM records.
pub fn string_values(container: &Container) -> Vec<StringValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .string_values()
                .into_iter()
                .map(move |(record_ordinal, value_ordinal, object_id, value)| {
                    let record =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    StringValue {
                        id: format!(
                            "nx:om-string-values-{section_ordinal}-{record_ordinal}:value#{}",
                            value.offset
                        ),
                        record,
                        object_id,
                        ordinal: value_ordinal as u32,
                        value: value.value.to_string(),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset + value.offset as u64,
                    }
                })
                .collect()
        })
        .collect()
}

/// Decode ordered tagged references from bounded NX OM records.
pub fn object_references(container: &Container) -> Vec<ObjectReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if section
                .records
                .first()
                .is_none_or(|record| record.object_id.is_none())
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .references()
                .into_iter()
                .map(
                    move |(record_ordinal, reference_ordinal, object_id, reference)| {
                        let record = format!(
                            "nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"
                        );
                        ObjectReference {
                            id: format!(
                                "nx:om-references-{section_ordinal}-{record_ordinal}:reference#{}",
                                reference.offset
                            ),
                            record,
                            object_id,
                            ordinal: reference_ordinal as u32,
                            kind: match reference.kind {
                                crate::om::ReferenceKind::PersistentHandle => {
                                    ObjectReferenceKind::PersistentHandle
                                }
                                crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                                crate::om::ReferenceKind::RecordOrdinal16 => {
                                    ObjectReferenceKind::RecordOrdinal16
                                }
                            },
                            value: reference.value,
                            target_record: (reference.kind
                                == crate::om::ReferenceKind::RecordOrdinal16)
                                .then(|| {
                                    format!(
                                        "nx:om-record-directory-{section_ordinal}:entry#{}",
                                        reference.value
                                    )
                                }),
                            source_entry: entry.name.clone(),
                            source_offset: entry_offset + reference.offset as u64,
                        }
                    },
                )
                .collect()
        })
        .collect()
}

/// Group persistent-handle occurrences into cross-record identities.
pub fn persistent_handles(
    references: &[ObjectReference],
    external: &[ExternalReferenceRecord],
) -> Vec<PersistentHandle> {
    let mut groups = BTreeMap::<u32, (Vec<String>, u32, Vec<String>)>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let (records, occurrence_count, _) = groups.entry(reference.value).or_default();
        *occurrence_count += 1;
        if records.last() != Some(&reference.record) && !records.contains(&reference.record) {
            records.push(reference.record.clone());
        }
    }
    for record in external {
        for handle in &record.handles {
            let external_records = &mut groups.entry(*handle).or_default().2;
            if !external_records.contains(&record.id) {
                external_records.push(record.id.clone());
            }
        }
    }
    groups
        .into_iter()
        .map(
            |(value, (records, occurrence_count, external_records))| PersistentHandle {
                id: format!("nx:om-persistent-handles:handle#{value:08x}"),
                value,
                records,
                occurrence_count,
                external_records,
            },
        )
        .collect()
}

/// Decode named parameter declarations from expression-class OM records.
pub fn expression_declarations(container: &Container) -> Vec<ExpressionDeclaration> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            if !section
                .types
                .iter()
                .any(|definition| definition.name == "UGS::EXP_expression")
            {
                return Vec::new();
            }
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            section
                .records
                .into_iter()
                .enumerate()
                .filter_map(|(record_ordinal, record)| {
                    let object_id = record.object_id?;
                    let declaration = crate::om::expression_declaration_name(record.bytes)?;
                    let record_id =
                        format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}");
                    Some(ExpressionDeclaration {
                        id: format!(
                            "nx:om-expression-declarations-{section_ordinal}:declaration#{record_ordinal}"
                        ),
                        object_id,
                        record: record_id,
                        name: declaration.value.to_string(),
                        parameter_index: declaration.parameter_index,
                        qualifier: declaration.qualifier.map(str::to_string),
                        literal: declaration.literal.map(str::to_string),
                        source_entry: entry.name.clone(),
                        source_offset: entry_offset
                            + record.offset as u64
                            + declaration.offset as u64,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode explicit numeric expressions from all indexed OM sections.
pub fn expressions(container: &Container) -> Vec<Expression> {
    let declarations = expression_declarations(container);
    let mut declarations_by_name = BTreeMap::<(&str, &str), Vec<&ExpressionDeclaration>>::new();
    for declaration in &declarations {
        declarations_by_name
            .entry((declaration.source_entry.as_str(), declaration.name.as_str()))
            .or_default()
            .push(declaration);
    }
    let mut indexed = BTreeMap::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        for (record_ordinal, expression) in section.numeric_expression_records() {
            let Some(object_id) = expression.object_id else {
                continue;
            };
            indexed.insert(
                (entry.name.clone(), expression.offset),
                (
                    Some(object_id),
                    format!("nx:om-record-directory-{section_ordinal}:entry#{record_ordinal}"),
                ),
            );
        }
    }
    let mut expressions = Vec::new();
    for (entry_index, entry) in container.entries.iter().enumerate() {
        let Some((entry_offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(offset), Ok(size)) = (usize::try_from(entry_offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = container.data.get(offset..offset.saturating_add(size)) else {
            continue;
        };
        for expression in crate::om::numeric_expressions(payload) {
            let indexed_record = indexed
                .get(&(entry.name.clone(), expression.offset))
                .cloned();
            let declaration = declarations_by_name
                .get(&(entry.name.as_str(), expression.name))
                .and_then(|candidates| {
                    let [declaration] = candidates.as_slice() else {
                        return None;
                    };
                    Some(declaration.id.clone())
                });
            expressions.push(Expression {
                id: format!("nx:om-entry-{entry_index}:expression#{}", expression.offset),
                object_id: indexed_record
                    .as_ref()
                    .and_then(|(object_id, _)| *object_id),
                record: indexed_record.map(|(_, record)| record),
                declaration,
                name: expression.name.to_string(),
                parameter_index: expression.parameter_index,
                qualifier: expression.qualifier.map(str::to_string),
                unit: match expression.unit {
                    crate::om::ExpressionUnit::Millimeter => ExpressionUnit::Millimeter,
                    crate::om::ExpressionUnit::Degree => ExpressionUnit::Degree,
                },
                expression: expression.expression.to_string(),
                value: expression.value,
                source_entry: entry.name.clone(),
                source_offset: entry_offset + expression.offset as u64,
            });
        }
    }
    evaluate_expression_graphs(&mut expressions);
    expressions
}

pub(crate) fn evaluate_expression_graphs(expressions: &mut [Expression]) {
    let mut values = BTreeMap::<(String, String), (ExpressionUnit, f64)>::new();
    let mut name_counts = BTreeMap::<(String, String), usize>::new();
    for expression in expressions.iter() {
        *name_counts
            .entry((expression.source_entry.clone(), expression.name.clone()))
            .or_default() += 1;
        if let Some(value) = expression.value {
            values.insert(
                (expression.source_entry.clone(), expression.name.clone()),
                (expression.unit, value),
            );
        }
    }

    loop {
        let mut changed = false;
        for expression in expressions
            .iter_mut()
            .filter(|expression| expression.value.is_none())
        {
            let mut substituted = String::with_capacity(expression.expression.len());
            let bytes = expression.expression.as_bytes();
            let mut at = 0usize;
            let mut complete = true;
            while at < bytes.len() {
                if bytes[at] == b'p'
                    && at
                        .checked_sub(1)
                        .and_then(|before| bytes.get(before))
                        .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
                {
                    let start = at;
                    at += 1;
                    let digits = at;
                    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                        at += 1;
                    }
                    if at > digits {
                        while bytes
                            .get(at)
                            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                        {
                            at += 1;
                        }
                        let name = &expression.expression[start..at];
                        let key = (expression.source_entry.clone(), name.to_string());
                        let Some((unit, value)) = values.get(&key).copied() else {
                            complete = false;
                            break;
                        };
                        if name_counts.get(&key) != Some(&1) || expression.unit != unit {
                            complete = false;
                            break;
                        }
                        substituted.push_str(&value.to_string());
                        continue;
                    }
                    at = start;
                }
                substituted.push(char::from(bytes[at]));
                at += 1;
            }
            if complete {
                if let Some(value) = crate::om::evaluate_constant_expression(&substituted) {
                    expression.value = Some(value);
                    values.insert(
                        (expression.source_entry.clone(), expression.name.clone()),
                        (expression.unit, value),
                    );
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}
