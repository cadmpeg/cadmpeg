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

/// Unambiguous terminal status of one segment-bound body image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentBodyLineageStatus {
    /// Globally unique status identity.
    pub id: String,
    /// Segment binding whose alias pair names the body image.
    pub segment_body_binding: String,
    /// First serialized body identity.
    pub body_object_index: u32,
    /// Alias identity naming the same body image.
    pub body_alias_object_index: u32,
    /// Whether the image remains terminal after retained history.
    pub terminal: bool,
    /// Absolute source offset of the segment binding.
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
    /// Exact 26-byte descriptor prefix following the field-record header.
    pub field_descriptor_prefix: [u8; 26],
    /// One serialized code for every declared field.
    pub field_codes: Vec<u8>,
    /// Offset of the declaration in the inflated stream.
    pub inflated_offset: u64,
}

/// Explicit topology-record ownership of one Parasolid attribute list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeListReference {
    /// Globally unique reference identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Parasolid topology record type.
    pub topology_type: u8,
    /// Stream-local topology-record identity.
    pub topology_xmt: u32,
    /// Stream-local attribute-list identity.
    pub attribute_list_xmt: u32,
    /// Uniquely resolved type-81 attribute-list record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_list_record: Option<String>,
    /// Offset of the attribute-list field in the inflated stream.
    pub inflated_offset: u64,
}

/// Framed Parasolid type-81 entity/attribute-list record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51Record {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact record flags.
    pub flags: u32,
    /// Serialized sequence value.
    pub sequence: u32,
    /// Layout discriminator.
    pub discriminator: u16,
    /// Ordered stream-local references.
    pub references: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Self-framed printable Parasolid type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity54StringRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Exact nonempty printable value.
    pub value: String,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-82 unsigned-integer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity52IntegerRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered big-endian unsigned values.
    pub values: Vec<u32>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Counted Parasolid type-83 finite binary64 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParasolidEntity53DoubleRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Stream-local record identity.
    pub xmt: u32,
    /// Ordered finite big-endian binary64 values.
    pub values: Vec<f64>,
    /// Exact framed record length.
    pub byte_len: u64,
    /// Offset of the record tag in the inflated stream.
    pub inflated_offset: u64,
}

/// Numeric value-record family referenced by a type-81 record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParasolidEntity51NumericKind {
    /// Type-82 unsigned-integer lane.
    UnsignedIntegers,
    /// Type-83 binary64 lane.
    Doubles,
}

/// Exact type-81 reference to one uniquely resolved numeric value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51NumericUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Numeric record family.
    pub kind: ParasolidEntity51NumericKind,
    /// Uniquely resolved numeric record.
    pub value_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Exact type-81 reference to a uniquely resolved type-84 string record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidEntity51StringUse {
    /// Globally unique use identity.
    pub id: String,
    /// Zero-based inflated Parasolid stream ordinal.
    pub stream_ordinal: u32,
    /// Owning type-81 record.
    pub entity_51_record: String,
    /// Zero-based position in the type-81 reference lane.
    pub reference_ordinal: u32,
    /// Stream-local referenced xmt.
    pub referenced_xmt: u32,
    /// Uniquely resolved type-84 string record.
    pub string_record: String,
    /// Offset of the owning type-81 record in the inflated stream.
    pub inflated_offset: u64,
}

/// Resolved attribute class of a topology-owned type-81 attribute instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParasolidTopologyAttributeClassUse {
    /// Globally unique use identity.
    pub id: String,
    /// Owning topology-to-attribute reference.
    pub topology_attribute_reference: String,
    /// Type-81 attribute-instance record.
    pub entity_51_record: String,
    /// One-based definition-catalog index serialized by the instance.
    pub definition_ordinal: u32,
    /// Uniquely resolved attribute definition.
    pub attribute_definition: String,
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

/// Typed operation template carried by a `SIMPLE HOLE` payload string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleTemplate {
    /// Globally unique template identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Source string in the native payload-string arena.
    pub payload_string: String,
    /// Hole construction family token.
    pub family: SimpleHoleFamily,
    /// Hole cross-section token.
    pub form: SimpleHoleForm,
    /// Axial extent token.
    pub extent: SimpleHoleExtent,
    /// Entry treatment token.
    pub start_treatment: SimpleHoleEndTreatment,
    /// Exit treatment token.
    pub end_treatment: SimpleHoleEndTreatment,
}

/// Exact redundantly witnessed scalar pair in a simple-hole payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarPair {
    /// Globally unique repeated-pair identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered finite shifted-binary64 values.
    pub values: [f64; 2],
    /// Absolute offsets of the first scalar pair.
    pub first_witness_offsets: [u64; 2],
    /// Absolute offsets of the byte-identical repeated scalar pair.
    pub second_witness_offsets: [u64; 2],
}

/// Offset-store blocks linked after both repeated scalar-pair witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarPairBlockReferences {
    /// Globally unique reference-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered blocks following the first scalar pair.
    pub first_data_blocks: [String; 2],
    /// Ordered blocks following the repeated scalar pair.
    pub second_data_blocks: [String; 2],
    /// Absolute offsets of the first pair of tagged-index tokens.
    pub first_reference_offsets: [u64; 2],
    /// Absolute offsets of the repeated pair of tagged-index tokens.
    pub second_reference_offsets: [u64; 2],
}

/// Construction family named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleFamily {
    /// General-hole construction family.
    GeneralHole,
}

/// Cross-section named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleForm {
    /// Plain cylindrical cross-section.
    Simple,
}

/// Axial termination named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleExtent {
    /// Continue through all intersected material.
    Through,
}

/// End treatment named by a simple-hole template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimpleHoleEndTreatment {
    /// Chamfer the circular end edge.
    Chamfer,
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

/// Input-block bindings from distinct operations that resolve to one data block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputBlockIdentityGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Input bindings in ascending source-offset order.
    pub input_blocks: Vec<String>,
    /// Operation labels aligned with `input_blocks`.
    pub operation_labels: Vec<String>,
    /// Header slots aligned with `input_blocks`.
    pub input_slots: Vec<u8>,
    /// Object-index token offsets aligned with `input_blocks`.
    pub source_offsets: Vec<u64>,
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
    /// Exact numeric expression bound to the declaration, when unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// Persistent OM object ID of the declaration.
    pub object_id: u32,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// All binding occurrences by which one operation consumes one expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureParameterUse {
    /// Globally unique use identity.
    pub id: String,
    /// Consuming operation-label identity.
    pub operation_label: String,
    /// Exact numeric expression consumed by the operation.
    pub expression: String,
    /// Binding occurrences in ascending source-offset order.
    pub bindings: Vec<String>,
    /// Binding source offsets aligned with `bindings`.
    pub source_offsets: Vec<u64>,
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

/// Completely resolved native construction lane of a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysConstruction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the fixed construction header suffix.
    pub control: u8,
    /// Eight object indices in serialized lane order.
    pub object_indices: [u32; 8],
    /// Eight uniquely resolved same-store blocks in lane order.
    pub data_blocks: [String; 8],
    /// Absolute offsets of the eight canonical reference markers.
    pub source_offsets: [u64; 8],
}

/// Exact logical payload reconstructed from the two leading datum-CSYS blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Construction defining the ordered block lane.
    pub construction: String,
    /// Two leading source blocks in serialized order.
    pub data_blocks: [String; 2],
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: [u64; 2],
    /// Exact source-block lengths.
    pub block_byte_lengths: [u64; 2],
    /// Absolute source-block offsets.
    pub block_source_offsets: [u64; 2],
}

/// One exactly framed scalar pair in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayloadScalarPair {
    /// Globally unique scalar-pair identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative scalar offsets.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: [u64; 2],
    /// Exact discriminator selecting the scalar-pair branch.
    pub discriminator: Vec<u8>,
}

/// Typed descriptor from one of the final three datum-CSYS construction lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysDescriptor {
    /// Globally unique descriptor identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Construction carrying the descriptor lane.
    pub construction: String,
    /// Construction reference ordinal in the range 5–7.
    pub reference_ordinal: u8,
    /// Resolved source block.
    pub data_block: String,
    /// Exact bytes preceding the hexadecimal identity.
    pub prefix: Vec<u8>,
    /// Lowercase 30–32 digit hexadecimal identity.
    pub identity: String,
    /// Exact bytes following the hexadecimal identity.
    pub suffix: Vec<u8>,
    /// Absolute source offset of the block.
    pub source_offset: u64,
    /// Absolute source offset of the identity.
    pub identity_source_offset: u64,
}

/// Exact shared descriptor identity between datum-plane and datum-CSYS history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneCsysIdentityUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Shared lowercase hexadecimal identity.
    pub identity: String,
    /// Typed datum-plane descriptor.
    pub datum_plane_descriptor: String,
    /// Datum-plane operation carrying the descriptor.
    pub datum_plane_operation_label: String,
    /// Typed datum-CSYS descriptor.
    pub datum_csys_descriptor: String,
    /// Datum-CSYS operation carrying the descriptor.
    pub datum_csys_operation_label: String,
    /// Datum-CSYS construction reference ordinal.
    pub datum_csys_reference_ordinal: u8,
}

/// Common typed header of one datum-plane construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneHeader {
    /// Globally unique header identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Payload control byte.
    pub control: u8,
    /// Declared construction count.
    pub declared_count: u8,
    /// Tag selecting the following construction branch.
    pub branch_tag: u8,
    /// Ordered compact descriptor indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_indices: Vec<u32>,
    /// Ordered canonical object indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_indices: Vec<u32>,
    /// Atomically resolved same-store descriptor blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_data_blocks: Vec<String>,
    /// Atomically resolved same-store canonical object blocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_data_blocks: Vec<String>,
    /// Absolute offsets of compact descriptor indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_source_offsets: Vec<u64>,
    /// Absolute offsets of canonical object-index markers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_source_offsets: Vec<u64>,
    /// Absolute offset of the payload control byte.
    pub source_offset: u64,
}

/// Exact logical datum-plane object payload reconstructed in lane order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlanePayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Header defining the ordered object-block lane.
    pub datum_plane_header: String,
    /// Ordered source blocks.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Starting payload offset of each source block.
    pub block_payload_offsets: Vec<u64>,
    /// Exact byte length of each source block.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute file offset of each source block.
    pub block_source_offsets: Vec<u64>,
    /// Payload-relative opening offset of a unique terminal index lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_lane_offset: Option<u64>,
    /// Declared count of the terminal index lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_lane_declared_count: Option<u8>,
    /// Ordered decoded compact indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_lane_values: Vec<u32>,
    /// Payload-relative offsets of decoded compact indices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_lane_value_offsets: Vec<u64>,
    /// Big-endian trailer word of the terminal lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_lane_trailer: Option<u32>,
}

/// One exactly framed scalar pair in a reconstructed datum-plane payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumPlanePayloadScalarPair {
    /// Globally unique scalar-pair identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_plane_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the scalar encodings.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: [u64; 2],
}

/// Resolved typed descriptor of one datum-plane construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneDescriptor {
    /// Globally unique descriptor identity.
    pub id: String,
    /// Owning `DATUM_PLANE` operation label.
    pub operation_label: String,
    /// Header carrying the descriptor reference.
    pub datum_plane_header: String,
    /// Zero-based descriptor-lane order.
    pub ordinal: u32,
    /// Resolved source block.
    pub data_block: String,
    /// Lowercase hexadecimal identity preceding the delimiter.
    pub identity: String,
    /// Exact descriptor suffix beginning with `?`.
    pub suffix: Vec<u8>,
    /// Non-null compact schema index following `?A`.
    pub schema_index: u32,
    /// Nonempty printable terminal label.
    pub label: String,
    /// Absolute source offset of the descriptor block.
    pub source_offset: u64,
}

/// Datum-plane construction lane containing a reused block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatumPlaneBlockLane {
    /// Compact descriptor lane.
    Descriptor,
    /// Canonical object-reference lane.
    Object,
}

/// Exact reuse of one resolved datum-plane construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumPlaneBlockUse {
    /// Globally unique relation identity.
    pub id: String,
    /// Owning datum-plane header.
    pub datum_plane_header: String,
    /// `DATUM_PLANE` operation owning the construction.
    pub construction_operation_label: String,
    /// Construction lane containing the block.
    pub lane: DatumPlaneBlockLane,
    /// Zero-based position within the lane.
    pub reference_ordinal: u32,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
}

/// Exact reuse of one datum-coordinate-system construction block by an operation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDatumCsysBlockUse {
    /// Globally unique block-use identity.
    pub id: String,
    /// Owning datum-coordinate-system construction.
    pub construction: String,
    /// `DATUM_CSYS` operation owning the construction lane.
    pub construction_operation_label: String,
    /// Zero-based position in the eight-reference construction lane.
    pub reference_ordinal: u8,
    /// Shared offset-store block.
    pub data_block: String,
    /// Matching operation-header input binding.
    pub input_binding: String,
    /// Operation whose header addresses the shared block.
    pub input_operation_label: String,
    /// Zero-based operation-header input slot.
    pub input_slot: u8,
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

/// Exact logical sketch payload reconstructed from its ordered store blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Complete construction-input record defining block order.
    pub construction_inputs: String,
    /// Ordered leading member blocks followed by the separated terminal block.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the exact concatenated payload bytes.
    pub sha256: String,
    /// Starting payload offset of each block in concatenation order.
    pub block_payload_offsets: Vec<u64>,
    /// Exact serialized length of each source block.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute file offset of each source block.
    pub block_source_offsets: Vec<u64>,
}

/// One exactly framed coordinate pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadCoordinatePair {
    /// Globally unique coordinate-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative scalar offsets.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: [u64; 2],
    /// Exact discriminator selecting the coordinate-pair branch.
    pub discriminator: Vec<u8>,
}

/// Exact framed scalar retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadScalar {
    /// Globally unique scalar identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this field.
    pub construction_payload: String,
    /// Zero-based field order within the reconstructed payload.
    pub ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    pub field_code: u8,
    /// Finite shifted-IEEE binary64 value.
    pub value: f64,
    /// Byte offset of the field marker within the reconstructed payload.
    pub payload_offset: u64,
    /// Absolute file offset of the field marker.
    pub source_offset: u64,
}

/// Exact framed name retained from one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this field.
    pub construction_payload: String,
    /// Zero-based name-field order within the reconstructed payload.
    pub ordinal: u32,
    /// Decoded compact type code following the `66` marker, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code: Option<u32>,
    /// Whether the field uses the type-free payload-leading form.
    pub payload_leading: bool,
    /// Exact printable field value.
    pub value: String,
    /// Byte offset of the `66` marker within the reconstructed payload.
    pub payload_offset: u64,
    /// Absolute file offset of the `66` marker.
    pub source_offset: u64,
}

/// Named sketch payload interval and its ordered framed scalar fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadNamedRecord {
    /// Globally unique named-record identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying this record.
    pub construction_payload: String,
    /// Name field opening the retained interval.
    pub name_field: String,
    /// Ordered scalar fields before the next complete name field.
    pub scalar_fields: Vec<String>,
    /// Payload-relative offset of the opening name marker.
    pub payload_start_offset: u64,
    /// Payload-relative exclusive end at the next name or payload boundary.
    pub payload_end_offset: u64,
}

/// Complete named two-dimensional point in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPoint {
    /// Globally unique point identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Name-delimited payload record carrying the point.
    pub named_record: String,
    /// Exact `Point<decimal>` source name.
    pub name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub scalar_fields: [String; 2],
    /// Ordered finite native coordinate values.
    pub coordinates: [f64; 2],
}

/// Named two-scalar point object spanning consecutive offset-store blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetStoreNamedPoint {
    /// Globally unique point-object identity.
    pub id: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Minimal consecutive source-block span carrying the object.
    pub data_blocks: Vec<String>,
    /// Ordered finite native scalar values.
    pub values: [f64; 2],
    /// Absolute source offsets of the two scalar markers.
    pub value_source_offsets: [u64; 2],
    /// Absolute source offset of the name frame.
    pub source_offset: u64,
}

/// Exact reuse of one named-point block by a sketch reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchNamedPointBlockUse {
    /// Globally unique block-use identity.
    pub id: String,
    /// Sketch operation carrying the reference.
    pub operation_label: String,
    /// Typed sketch-reference occurrence.
    pub sketch_reference: String,
    /// Reference order within the sketch field.
    pub reference_ordinal: u32,
    /// Typed named-point object containing the block.
    pub named_point: String,
    /// Shared offset-store block.
    pub data_block: String,
    /// Block position within the named-point span.
    pub point_block_ordinal: u32,
    /// Absolute source offset of the sketch reference.
    pub source_offset: u64,
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
    /// Unique offset-only data blocks addressed by the ordered indices.
    pub data_blocks: Vec<Option<String>>,
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
    /// Body object index anchoring the branch.
    pub body_object_index: u32,
    /// Finite shifted-IEEE scalar following the branch marker.
    pub scalar: f64,
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    pub atoms_be: Vec<u32>,
    /// Compact indices wrapped by the fixed-width atoms.
    pub atom_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the atom indices.
    pub atom_data_blocks: Vec<Option<String>>,
    /// Ordered values in the first compact-index lane.
    pub first_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the first lane.
    pub first_data_blocks: Vec<Option<String>>,
    /// Ordered values in the second compact-index lane.
    pub second_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the second lane.
    pub second_data_blocks: Vec<Option<String>>,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Absolute file offset of the `32` branch marker.
    pub source_offset: u64,
}

/// Complete alternate extrusion construction using the structured `32` branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrude32Construction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Structured branch supplying the body-anchored construction lanes.
    pub branch: String,
    /// Body object index witnessed at both ends of the structured branch.
    pub body_object_index: u32,
    /// Ordered profile-reference identities.
    pub profile_references: Vec<String>,
    /// Ordered uniquely resolved profile blocks.
    pub profile_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the fixed-atom lane.
    pub atom_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the first compact-index lane.
    pub first_data_blocks: Vec<String>,
    /// Ordered uniquely resolved blocks from the second compact-index lane.
    pub second_data_blocks: Vec<String>,
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

/// Completely resolved construction-reference field of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstruction {
    /// Globally unique construction identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Payload control byte preceding the construction field.
    pub control: u8,
    /// Eighteen ordered references preceding the separator.
    pub member_references: Vec<String>,
    /// Eighteen uniquely resolved member blocks.
    pub member_data_blocks: Vec<String>,
    /// Reference following the separator.
    pub terminal_reference: String,
    /// Uniquely resolved terminal block.
    pub terminal_data_block: String,
}

/// Exact logical payload reconstructed from one complete `BLOCK` construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Construction defining serialized block order.
    pub construction: String,
    /// Ordered member blocks followed by the terminal block.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative source-block starts.
    pub block_payload_offsets: Vec<u64>,
    /// Exact source-block lengths.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute source-block offsets.
    pub block_source_offsets: Vec<u64>,
}

/// Ordered three-parameter dimension run of one `BLOCK` feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockDimensions {
    /// Globally unique dimension-set identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Complete resolved block construction.
    pub construction: String,
    /// Bindings selecting the first parameter declaration.
    pub anchor_bindings: Vec<String>,
    /// Ordered consecutive parameter declarations.
    pub declarations: [String; 3],
    /// Ordered exact numeric expression records.
    pub expressions: [String; 3],
    /// Ordered finite dimensions in model millimeters.
    pub values: [f64; 3],
}

/// Persistent object frame carried by one bounded offset-store block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockObjectFrame {
    /// Globally unique relation identity.
    pub id: String,
    /// Source block carrying the object frame.
    pub data_block: String,
    /// Zero-based frame order within the block.
    pub ordinal: u32,
    /// Serialized persistent object ID.
    pub object_id: u32,
    /// Absolute source offset of the compact object index.
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

/// Join exact `SIMPLE HOLE` payload templates to their operation identities.
pub fn feature_simple_hole_templates(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    strings: &[FeaturePayloadString],
) -> Vec<FeatureSimpleHoleTemplate> {
    let labels_by_id = labels
        .iter()
        .map(|label| (label.id.as_str(), label))
        .collect::<BTreeMap<_, _>>();
    let records_by_id = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    strings
        .iter()
        .filter_map(|string| {
            let record = records_by_id.get(string.operation_record.as_str())?;
            let label = labels_by_id.get(record.operation_label.as_str())?;
            (label.value == "SIMPLE HOLE").then_some(())?;
            let (family, form, extent, start_treatment, end_treatment) =
                parse_simple_hole_template(&string.value)?;
            Some(FeatureSimpleHoleTemplate {
                id: string
                    .id
                    .replacen("payload-string", "simple-hole-template", 1),
                operation_label: label.id.clone(),
                payload_string: string.id.clone(),
                family,
                form,
                extent,
                start_treatment,
                end_treatment,
            })
        })
        .collect()
}

/// Decode exact duplicated scalar pairs from simple-hole operations.
pub fn feature_simple_hole_repeated_scalar_pairs(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarPair> {
    let sections = container.om_sections();
    let mut pairs = Vec::new();
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
            let Some(pair) = crate::om::simple_hole_repeated_scalar_pair(record) else {
                continue;
            };
            pairs.push(FeatureSimpleHoleRepeatedScalarPair {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-pair#{section_key}-{operation_ordinal}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                ),
                values: pair.values,
                first_witness_offsets: pair.witness_offsets[0]
                    .map(|offset| entry_offset + offset as u64),
                second_witness_offsets: pair.witness_offsets[1]
                    .map(|offset| entry_offset + offset as u64),
            });
        }
    }
    pairs
}

/// Resolve the tagged block-index pairs following both repeated scalar-pair
/// witnesses through the unique offset store that owns the operation inputs.
pub fn feature_simple_hole_repeated_scalar_pair_block_references(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarPairBlockReferences> {
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
    let blocks = data_blocks(container)
        .into_iter()
        .map(|block| block.id)
        .collect::<BTreeSet<_>>();
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
            let prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            if prefixes.len() != 1 {
                continue;
            }
            let prefix = prefixes.into_iter().next().expect("one checked prefix");
            let Some(decoded) =
                crate::om::simple_hole_repeated_scalar_pair_block_references(record)
            else {
                continue;
            };
            let resolve = |indices: [u32; 2]| {
                let targets = indices.map(|index| format!("{prefix}:block#{index}"));
                targets
                    .iter()
                    .all(|target| blocks.contains(target))
                    .then_some(targets)
            };
            let (Some(first_data_blocks), Some(second_data_blocks)) =
                (resolve(decoded.first), resolve(decoded.second))
            else {
                continue;
            };
            references.push(FeatureSimpleHoleRepeatedScalarPairBlockReferences {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-pair-block-references#{section_key}-{operation_ordinal}"
                ),
                operation_label,
                first_data_blocks,
                second_data_blocks,
                first_reference_offsets: decoded.offsets[0].map(|offset| entry_offset + offset as u64),
                second_reference_offsets: decoded.offsets[1].map(|offset| entry_offset + offset as u64),
            });
        }
    }
    references
}

pub(crate) fn parse_simple_hole_template(
    value: &str,
) -> Option<(
    SimpleHoleFamily,
    SimpleHoleForm,
    SimpleHoleExtent,
    SimpleHoleEndTreatment,
    SimpleHoleEndTreatment,
)> {
    (value.split('_').collect::<Vec<_>>().as_slice()
        == [
            "Hole",
            "GeneralHole",
            "Simple",
            "Through",
            "StartChamfer",
            "EndChamfer",
        ])
    .then_some((
        SimpleHoleFamily::GeneralHole,
        SimpleHoleForm::Simple,
        SimpleHoleExtent::Through,
        SimpleHoleEndTreatment::Chamfer,
        SimpleHoleEndTreatment::Chamfer,
    ))
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

/// Resolve one atomic terminal status for every segment-bound body image.
pub fn segment_body_lineage_statuses(
    labels: &[FeatureOperationLabel],
    references: &[FeatureBodyReference],
    booleans: &[FeatureBooleanOperation],
    operands: &[FeatureOperationBodyOperand],
    bindings: &[SegmentBodyBinding],
) -> Option<Vec<SegmentBodyLineageStatus>> {
    let terminal = terminal_feature_body_indices(labels, references, booleans, operands, bindings)?;
    bindings
        .iter()
        .map(|binding| {
            let statuses = [binding.body_object_index, binding.body_alias_object_index]
                .map(|identity| terminal.contains(&identity));
            if statuses[0] != statuses[1] {
                return None;
            }
            let key = binding
                .id
                .rsplit_once('#')
                .map_or(binding.id.as_str(), |(_, key)| key);
            Some(SegmentBodyLineageStatus {
                id: format!("nx:segment-body-lineage:status#{key}"),
                segment_body_binding: binding.id.clone(),
                body_object_index: binding.body_object_index,
                body_alias_object_index: binding.body_alias_object_index,
                terminal: statuses[0],
                source_offset: binding.source_offset,
            })
        })
        .collect()
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

/// Group bindings from distinct operations by exact resolved data-block identity.
pub fn feature_input_block_identity_groups(
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureInputBlockIdentityGroup> {
    let mut by_block = BTreeMap::<&str, Vec<&FeatureInputBlock>>::new();
    for input in inputs {
        by_block.entry(&input.data_block).or_default().push(input);
    }
    let mut groups = by_block
        .into_iter()
        .filter_map(|(data_block, mut members)| {
            if members
                .iter()
                .map(|member| member.operation_label.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                return None;
            }
            members.sort_by_key(|member| member.source_offset);
            Some((data_block, members))
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|(_, members)| members[0].source_offset);
    groups
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (data_block, members))| FeatureInputBlockIdentityGroup {
                id: format!("nx:feature-history:input-block-identity-group#{ordinal}"),
                data_block: data_block.to_string(),
                input_blocks: members.iter().map(|member| member.id.clone()).collect(),
                operation_labels: members
                    .iter()
                    .map(|member| member.operation_label.clone())
                    .collect(),
                input_slots: members.iter().map(|member| member.input_slot).collect(),
                source_offsets: members.iter().map(|member| member.source_offset).collect(),
            },
        )
        .collect()
}

/// Decode and atomically resolve datum coordinate-system construction lanes
/// through the offset store selected by each operation header.
pub fn feature_datum_csys_constructions(
    container: &Container,
) -> Vec<FeatureDatumCsysConstruction> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
    let mut constructions = Vec::new();
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
            let Some(field) = crate::om::datum_csys_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal}");
            let input_prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            if input_prefixes.len() != 1 {
                continue;
            }
            let input_prefix = input_prefixes
                .into_iter()
                .next()
                .expect("one checked input store");
            let resolved = field.references.map(|reference| {
                unique_offset_data_block(&indexed, reference.object_index).map(|data_block| {
                    (
                        reference.object_index,
                        data_block,
                        entry_offset + reference.offset as u64,
                    )
                })
            });
            let Some(resolved) = resolved.into_iter().collect::<Option<Vec<_>>>() else {
                continue;
            };
            if resolved.iter().any(|(_, data_block, _)| {
                data_block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != input_prefix)
            }) {
                continue;
            }
            constructions.push(FeatureDatumCsysConstruction {
                id: format!(
                    "nx:feature-history:datum-csys-construction#{section_key}-{operation_ordinal}"
                ),
                operation_label,
                control: field.control,
                object_indices: resolved
                    .iter()
                    .map(|(object_index, _, _)| *object_index)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                data_blocks: resolved
                    .iter()
                    .map(|(_, data_block, _)| data_block.clone())
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                source_offsets: resolved
                    .iter()
                    .map(|(_, _, source_offset)| *source_offset)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
            });
        }
    }
    constructions
}

/// Decode common datum-plane payload headers from feature-history records.
pub fn feature_datum_plane_headers(container: &Container) -> Vec<FeatureDatumPlaneHeader> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
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
            let Some(header) = crate::om::datum_plane_payload_header(record) else {
                continue;
            };
            let single = crate::om::datum_plane_descriptor_reference_branch(record);
            let double = crate::om::datum_plane_double_reference_branch(record);
            let descriptor_indices = single
                .iter()
                .map(|branch| branch.descriptor_index)
                .collect::<Vec<_>>();
            let object_indices = single
                .iter()
                .map(|branch| branch.object_index)
                .chain(double.iter().flat_map(|branch| {
                    branch
                        .references
                        .iter()
                        .map(|reference| reference.object_index)
                }))
                .collect::<Vec<_>>();
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal}");
            let input_prefixes = inputs
                .iter()
                .filter(|input| input.operation_label == operation_label)
                .filter_map(|input| {
                    input
                        .data_block
                        .rsplit_once(":block#")
                        .map(|(prefix, _)| prefix)
                })
                .collect::<BTreeSet<_>>();
            let resolved = (object_indices.len() + descriptor_indices.len() > 0)
                .then_some(())
                .and_then(|()| {
                    if input_prefixes.len() != 1 {
                        return None;
                    }
                    let input_prefix = *input_prefixes.iter().next()?;
                    let descriptor_blocks = descriptor_indices
                        .iter()
                        .map(|index| unique_offset_data_block(&indexed, *index))
                        .collect::<Option<Vec<_>>>()?;
                    let object_blocks = object_indices
                        .iter()
                        .map(|index| unique_offset_data_block(&indexed, *index))
                        .collect::<Option<Vec<_>>>()?;
                    let same_store = |block: &str| {
                        block
                            .rsplit_once(":block#")
                            .is_some_and(|(prefix, _)| prefix == input_prefix)
                    };
                    descriptor_blocks
                        .iter()
                        .chain(&object_blocks)
                        .all(|block| same_store(block))
                        .then_some((descriptor_blocks, object_blocks))
                });
            headers.push(FeatureDatumPlaneHeader {
                id: format!(
                    "nx:feature-history:datum-plane-header#{section_key}-{operation_ordinal}"
                ),
                operation_label,
                control: header.control,
                declared_count: header.declared_count,
                branch_tag: header.branch_tag,
                descriptor_indices,
                object_indices,
                descriptor_data_blocks: resolved
                    .as_ref()
                    .map_or_else(Vec::new, |(blocks, _)| blocks.clone()),
                object_data_blocks: resolved
                    .as_ref()
                    .map_or_else(Vec::new, |(_, blocks)| blocks.clone()),
                descriptor_source_offsets: single
                    .iter()
                    .map(|branch| entry_offset + branch.descriptor_offset as u64)
                    .collect(),
                object_source_offsets: single
                    .iter()
                    .map(|branch| entry_offset + branch.object_offset as u64)
                    .chain(double.iter().flat_map(|branch| {
                        branch
                            .references
                            .iter()
                            .map(|reference| entry_offset + reference.offset as u64)
                    }))
                    .collect(),
                source_offset: entry_offset + record.payload_offset as u64,
            });
        }
    }
    headers
}

/// Reconstruct datum-plane object payloads across ordered store blocks.
pub fn feature_datum_plane_payloads(
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Vec<FeatureDatumPlanePayload> {
    let blocks = offset_data_block_bytes(container);
    headers
        .iter()
        .filter(|header| !header.object_data_blocks.is_empty())
        .filter_map(|header| {
            let (payload, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&header.object_data_blocks, &blocks)?;
            let lanes = crate::om::datum_plane_object_index_lanes(&payload);
            let lane = match lanes.as_slice() {
                [lane] => Some(lane),
                _ => None,
            };
            let key = header.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
            Some(FeatureDatumPlanePayload {
                id: format!("nx:feature-history:datum-plane-payload#{key}"),
                operation_label: header.operation_label.clone(),
                datum_plane_header: header.id.clone(),
                data_blocks: header.object_data_blocks.clone(),
                byte_len: payload.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&payload),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
                index_lane_offset: lane.map(|lane| lane.offset as u64),
                index_lane_declared_count: lane.map(|lane| lane.declared_count),
                index_lane_values: lane.map_or_else(Vec::new, |lane| {
                    lane.indices.iter().map(|(value, _)| *value).collect()
                }),
                index_lane_value_offsets: lane.map_or_else(Vec::new, |lane| {
                    lane.indices
                        .iter()
                        .map(|(_, offset)| *offset as u64)
                        .collect()
                }),
                index_lane_trailer: lane.map(|lane| lane.trailer),
            })
        })
        .collect()
}

/// Reconstruct the two leading object blocks of each datum coordinate system.
pub fn feature_datum_csys_payloads(
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureDatumCsysPayload> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let data_blocks = [
                construction.data_blocks[0].clone(),
                construction.data_blocks[1].clone(),
            ];
            let (bytes, starts, lengths, sources) = join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureDatumCsysPayload {
                id: construction
                    .id
                    .replacen("datum-csys-construction", "datum-csys-payload", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets: starts.try_into().ok()?,
                block_byte_lengths: lengths.try_into().ok()?,
                block_source_offsets: sources.try_into().ok()?,
            })
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadScalarPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::object_payload_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureDatumCsysPayloadScalarPair {
                        id: format!("{}-scalar-pair-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        datum_csys_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                        discriminator: pair.discriminator,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode the final three descriptor lanes of datum coordinate systems.
pub fn feature_datum_csys_descriptors(
    container: &Container,
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureDatumCsysDescriptor> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            (5..8)
                .filter_map(|reference_ordinal| {
                    let data_block = &construction.data_blocks[reference_ordinal];
                    let &(bytes, source_offset) = blocks.get(data_block)?;
                    let descriptor = crate::om::datum_csys_descriptor_block(bytes)?;
                    Some(FeatureDatumCsysDescriptor {
                        id: format!("{}-descriptor-{reference_ordinal}", construction.id),
                        operation_label: construction.operation_label.clone(),
                        construction: construction.id.clone(),
                        reference_ordinal: reference_ordinal as u8,
                        data_block: data_block.clone(),
                        prefix: descriptor.prefix,
                        identity: descriptor.identity,
                        suffix: descriptor.suffix,
                        source_offset,
                        identity_source_offset: source_offset + descriptor.identity_offset as u64,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join equal typed descriptor identities across datum-plane and datum-CSYS history.
pub fn feature_datum_plane_csys_identity_uses(
    plane_descriptors: &[FeatureDatumPlaneDescriptor],
    csys_descriptors: &[FeatureDatumCsysDescriptor],
) -> Vec<FeatureDatumPlaneCsysIdentityUse> {
    plane_descriptors
        .iter()
        .flat_map(|plane| {
            csys_descriptors
                .iter()
                .filter(|csys| csys.identity == plane.identity)
                .map(|csys| {
                    let plane_key = plane.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
                    let csys_key = csys.id.rsplit_once('#').map_or("unknown", |(_, key)| key);
                    FeatureDatumPlaneCsysIdentityUse {
                        id: format!(
                            "nx:feature-history:datum-plane-csys-identity-use#{plane_key}-{csys_key}"
                        ),
                        identity: plane.identity.clone(),
                        datum_plane_descriptor: plane.id.clone(),
                        datum_plane_operation_label: plane.operation_label.clone(),
                        datum_csys_descriptor: csys.id.clone(),
                        datum_csys_operation_label: csys.operation_label.clone(),
                        datum_csys_reference_ordinal: csys.reference_ordinal,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-plane payloads.
pub fn feature_datum_plane_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumPlanePayload],
) -> Vec<FeatureDatumPlanePayloadScalarPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::datum_plane_object_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureDatumPlanePayloadScalarPair {
                        id: format!("{}-scalar-pair-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        datum_plane_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode atomically resolved datum-plane descriptor blocks.
pub fn feature_datum_plane_descriptors(
    container: &Container,
    headers: &[FeatureDatumPlaneHeader],
) -> Vec<FeatureDatumPlaneDescriptor> {
    let blocks = offset_data_block_bytes(container);
    headers
        .iter()
        .flat_map(|header| {
            header
                .descriptor_data_blocks
                .iter()
                .enumerate()
                .filter_map(|(ordinal, data_block)| {
                    let (bytes, source_offset) = blocks.get(data_block)?.to_owned();
                    let descriptor = crate::om::datum_plane_descriptor_block(bytes)?;
                    Some(FeatureDatumPlaneDescriptor {
                        id: format!("{}-descriptor-{ordinal}", header.id),
                        operation_label: header.operation_label.clone(),
                        datum_plane_header: header.id.clone(),
                        ordinal: ordinal as u32,
                        data_block: data_block.clone(),
                        identity: descriptor.identity,
                        suffix: descriptor.suffix,
                        schema_index: descriptor.schema_index,
                        label: descriptor.label,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join resolved datum-plane blocks to operation inputs addressing the same block.
pub fn feature_datum_plane_block_uses(
    headers: &[FeatureDatumPlaneHeader],
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureDatumPlaneBlockUse> {
    let mut uses = Vec::new();
    for header in headers {
        let construction_key = header
            .operation_label
            .rsplit_once('#')
            .map_or(header.operation_label.as_str(), |(_, key)| key);
        for (lane, blocks) in [
            (
                DatumPlaneBlockLane::Descriptor,
                &header.descriptor_data_blocks,
            ),
            (DatumPlaneBlockLane::Object, &header.object_data_blocks),
        ] {
            for (reference_ordinal, data_block) in blocks.iter().enumerate() {
                for input in inputs
                    .iter()
                    .filter(|input| input.data_block == *data_block)
                {
                    let input_key = input
                        .operation_label
                        .rsplit_once('#')
                        .map_or(input.operation_label.as_str(), |(_, key)| key);
                    let lane_key = match lane {
                        DatumPlaneBlockLane::Descriptor => "descriptor",
                        DatumPlaneBlockLane::Object => "object",
                    };
                    uses.push(FeatureDatumPlaneBlockUse {
                        id: format!(
                            "nx:feature-history:datum-plane-block-use#{construction_key}-{lane_key}-{reference_ordinal}-{input_key}-{}",
                            input.input_slot
                        ),
                        datum_plane_header: header.id.clone(),
                        construction_operation_label: header.operation_label.clone(),
                        lane,
                        reference_ordinal: reference_ordinal as u32,
                        data_block: data_block.clone(),
                        input_binding: input.id.clone(),
                        input_operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                    });
                }
            }
        }
    }
    uses
}

/// Join resolved datum-coordinate-system blocks to every exact operation input
/// addressing the same native block.
pub fn feature_datum_csys_block_uses(
    constructions: &[FeatureDatumCsysConstruction],
    inputs: &[FeatureInputBlock],
) -> Vec<FeatureDatumCsysBlockUse> {
    let mut uses = Vec::new();
    for construction in constructions {
        for (reference_ordinal, data_block) in construction.data_blocks.iter().enumerate() {
            for input in inputs
                .iter()
                .filter(|input| input.data_block == *data_block)
            {
                let construction_key = construction
                    .operation_label
                    .rsplit_once('#')
                    .map_or(construction.operation_label.as_str(), |(_, key)| key);
                let input_key = input
                    .operation_label
                    .rsplit_once('#')
                    .map_or(input.operation_label.as_str(), |(_, key)| key);
                uses.push(FeatureDatumCsysBlockUse {
                    id: format!(
                        "nx:feature-history:datum-csys-block-use#{construction_key}-{reference_ordinal}-{input_key}-{}",
                        input.input_slot
                    ),
                    construction: construction.id.clone(),
                    construction_operation_label: construction.operation_label.clone(),
                    reference_ordinal: reference_ordinal as u8,
                    data_block: data_block.clone(),
                    input_binding: input.id.clone(),
                    input_operation_label: input.operation_label.clone(),
                    input_slot: input.input_slot,
                });
            }
        }
    }
    uses
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

/// Reconstruct exact sketch payloads across offset-store block boundaries.
pub fn feature_sketch_construction_payloads(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureSketchConstructionPayload> {
    let blocks = offset_data_block_bytes(container);

    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let (payload, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureSketchConstructionPayload {
                id: construction.id.replacen(
                    "sketch-construction-inputs",
                    "sketch-construction-payload",
                    1,
                ),
                operation_label: construction.operation_label.clone(),
                construction_inputs: construction.id.clone(),
                data_blocks,
                byte_len: payload.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&payload),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
            })
        })
        .collect()
}

/// Decode exact coordinate-pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_coordinate_pairs(
    container: &Container,
    payloads: &[FeatureSketchConstructionPayload],
) -> Vec<FeatureSketchPayloadCoordinatePair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let source_offset =
                |relative: usize| {
                    let relative = relative as u64;
                    starts.iter().zip(&lengths).zip(&sources).find_map(
                        |((start, length), source)| {
                            (relative >= *start && relative < start.saturating_add(*length))
                                .then_some(source + relative - start)
                        },
                    )
                };
            crate::om::object_payload_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureSketchPayloadCoordinatePair {
                        id: format!("{}-coordinate-pair-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: source_offset(pair.offset)?,
                        value_source_offsets: [
                            source_offset(pair.value_offsets[0])?,
                            source_offset(pair.value_offsets[1])?,
                        ],
                        discriminator: pair.discriminator,
                    })
                })
                .collect()
        })
        .collect()
}

fn offset_data_block_bytes(container: &Container) -> BTreeMap<String, (&[u8], u64)> {
    let mut blocks = BTreeMap::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        if section
            .records
            .first()
            .is_none_or(|record| record.object_id.is_some())
        {
            continue;
        }
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (record_ordinal, block) in section.records.into_iter().enumerate() {
            blocks.insert(
                format!(
                    "nx:om-data-blocks-{section_ordinal}:block#{}",
                    record_ordinal + 1
                ),
                (block.bytes, entry_offset + block.offset as u64),
            );
        }
    }
    blocks
}

/// Decode exact framed scalar fields across reconstructed sketch payloads.
pub fn feature_sketch_payload_scalars(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureSketchPayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let (payload, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            Some(
                crate::om::sketch_payload_scalar_fields(&payload)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, field)| {
                        let source_offset = block_payload_offsets
                            .iter()
                            .zip(&block_byte_lengths)
                            .zip(&block_source_offsets)
                            .find_map(|((payload_start, byte_len), source_start)| {
                                let relative = u64::try_from(field.offset).ok()?;
                                (relative >= *payload_start
                                    && relative < payload_start.saturating_add(*byte_len))
                                .then_some(source_start + relative - payload_start)
                            })
                            .expect("field lies in joined payload");
                        FeatureSketchPayloadScalar {
                            id: format!(
                                "nx:feature-history:sketch-payload-scalar#{}-{ordinal}",
                                construction_payload
                                    .rsplit_once('#')
                                    .map_or("unknown", |(_, key)| key)
                            ),
                            operation_label: construction.operation_label.clone(),
                            construction_payload: construction_payload.clone(),
                            ordinal: ordinal as u32,
                            field_code: field.field_code,
                            value: field.value,
                            payload_offset: field.offset as u64,
                            source_offset,
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

/// Decode exact compact-code name fields across reconstructed sketch payloads.
pub fn feature_sketch_payload_names(
    container: &Container,
    constructions: &[FeatureSketchConstructionInputs],
) -> Vec<FeatureSketchPayloadName> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .flat_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let Some((payload, block_payload_offsets, block_byte_lengths, block_source_offsets)) =
                join_data_block_bytes(&data_blocks, &blocks)
            else {
                return Vec::new();
            };
            let construction_payload = construction.id.replacen(
                "sketch-construction-inputs",
                "sketch-construction-payload",
                1,
            );
            crate::om::sketch_payload_named_fields(&payload)
                .into_iter()
                .enumerate()
                .map(|(ordinal, field)| {
                    let relative = field.offset as u64;
                    let source_offset = block_payload_offsets
                        .iter()
                        .zip(&block_byte_lengths)
                        .zip(&block_source_offsets)
                        .find_map(|((payload_start, byte_len), source_start)| {
                            (relative >= *payload_start
                                && relative < payload_start.saturating_add(*byte_len))
                            .then_some(source_start + relative - payload_start)
                        })
                        .expect("field lies in joined payload");
                    FeatureSketchPayloadName {
                        id: format!(
                            "nx:feature-history:sketch-payload-name#{}-{ordinal}",
                            construction_payload
                                .rsplit_once('#')
                                .map_or("unknown", |(_, key)| key)
                        ),
                        operation_label: construction.operation_label.clone(),
                        construction_payload: construction_payload.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code,
                        payload_leading: field.payload_leading,
                        value: field.value.to_string(),
                        payload_offset: relative,
                        source_offset,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join complete name-delimited intervals to their framed scalar fields.
pub fn feature_sketch_payload_named_records(
    payloads: &[FeatureSketchConstructionPayload],
    names: &[FeatureSketchPayloadName],
    scalars: &[FeatureSketchPayloadScalar],
) -> Vec<FeatureSketchPayloadNamedRecord> {
    let mut records = Vec::new();
    for payload in payloads {
        let mut payload_names = names
            .iter()
            .filter(|name| name.construction_payload == payload.id)
            .collect::<Vec<_>>();
        payload_names.sort_by_key(|name| name.payload_offset);
        for (ordinal, name) in payload_names.iter().enumerate() {
            let end = payload_names
                .get(ordinal + 1)
                .map_or(payload.byte_len, |next| next.payload_offset);
            let mut scalar_fields = scalars
                .iter()
                .filter(|scalar| {
                    scalar.construction_payload == payload.id
                        && scalar.payload_offset > name.payload_offset
                        && scalar.payload_offset < end
                })
                .collect::<Vec<_>>();
            scalar_fields.sort_by_key(|scalar| scalar.payload_offset);
            records.push(FeatureSketchPayloadNamedRecord {
                id: format!(
                    "nx:feature-history:sketch-payload-record#{}-{ordinal}",
                    payload
                        .id
                        .rsplit_once('#')
                        .map_or("unknown", |(_, key)| key)
                ),
                operation_label: payload.operation_label.clone(),
                construction_payload: payload.id.clone(),
                name_field: name.id.clone(),
                scalar_fields: scalar_fields
                    .into_iter()
                    .map(|scalar| scalar.id.clone())
                    .collect(),
                payload_start_offset: name.payload_offset,
                payload_end_offset: end,
            });
        }
    }
    records
}

/// Decode complete `Point<decimal>` records with exactly two scalar fields.
pub fn feature_sketch_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeatureSketchPayloadName],
    scalars: &[FeatureSketchPayloadScalar],
) -> Vec<FeatureSketchPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let scalars = scalars
        .iter()
        .map(|scalar| (scalar.id.as_str(), scalar))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            let name = names.get(record.name_field.as_str())?;
            parse_sketch_point_name(&name.value)?;
            let [first_id, second_id] = record.scalar_fields.as_slice() else {
                return None;
            };
            let first = scalars.get(first_id.as_str())?;
            let second = scalars.get(second_id.as_str())?;
            Some(FeatureSketchPoint {
                id: format!(
                    "nx:feature-history:sketch-point#{}",
                    record.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
                ),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.value, second.value],
            })
        })
        .collect()
}

/// Decode exact named point objects across consecutive offset-store blocks.
pub fn offset_store_named_points(container: &Container) -> Vec<OffsetStoreNamedPoint> {
    let mut points = Vec::new();
    for (section_ordinal, (entry, section)) in
        container.indexed_om_sections().into_iter().enumerate()
    {
        if section
            .records
            .first()
            .is_none_or(|record| record.object_id.is_some())
        {
            continue;
        }
        let section_key = format!("nx:om-data-blocks-{section_ordinal}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for ordinal in 0..section.records.len() {
            let remaining = section.records[ordinal..]
                .iter()
                .map(|record| record.bytes)
                .collect::<Vec<_>>();
            let Some(point) = crate::om::offset_store_named_point(&remaining) else {
                continue;
            };
            let records = &section.records[ordinal..ordinal + point.block_count];
            let first_source = entry_offset + records[0].offset as u64;
            let value_source_offset = |payload_offset: usize| {
                let mut relative = payload_offset;
                for record in records {
                    if relative < record.bytes.len() {
                        return Some(entry_offset + record.offset as u64 + relative as u64);
                    }
                    relative -= record.bytes.len();
                }
                None
            };
            points.push(OffsetStoreNamedPoint {
                id: format!(
                    "nx:offset-store:named-point#{section_ordinal}-{}",
                    ordinal + 1
                ),
                name: point.name,
                data_blocks: (0..point.block_count)
                    .map(|relative| format!("{section_key}:block#{}", ordinal + relative + 1))
                    .collect(),
                values: point.values,
                value_source_offsets: [
                    value_source_offset(point.value_offsets[0]).expect("first scalar in span"),
                    value_source_offset(point.value_offsets[1]).expect("second scalar in span"),
                ],
                source_offset: first_source,
            });
        }
    }
    points
}

/// Join sketch references to named points through exact shared block identity.
pub fn feature_sketch_named_point_block_uses(
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Vec<FeatureSketchNamedPointBlockUse> {
    let mut uses = Vec::new();
    for reference in references {
        let Some(data_block) = reference.data_block.as_deref() else {
            continue;
        };
        for point in points {
            let Some(point_block_ordinal) = point
                .data_blocks
                .iter()
                .position(|block| block == data_block)
            else {
                continue;
            };
            let operation_key = reference
                .operation_label
                .rsplit_once('#')
                .map_or(reference.operation_label.as_str(), |(_, key)| key);
            let point_key = point
                .id
                .rsplit_once('#')
                .map_or(point.id.as_str(), |(_, key)| key);
            uses.push(FeatureSketchNamedPointBlockUse {
                id: format!(
                    "nx:feature-history:sketch-named-point-block-use#{operation_key}-{}-{point_key}-{point_block_ordinal}",
                    reference.ordinal
                ),
                operation_label: reference.operation_label.clone(),
                sketch_reference: reference.id.clone(),
                reference_ordinal: reference.ordinal,
                named_point: point.id.clone(),
                data_block: data_block.to_string(),
                point_block_ordinal: point_block_ordinal as u32,
                source_offset: reference.source_offset,
            });
        }
    }
    uses
}

pub(crate) fn parse_sketch_point_name(value: &str) -> Option<u32> {
    let suffix = value.strip_prefix("Point")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let ordinal = suffix.parse::<u32>().ok()?;
    (ordinal != 0).then_some(ordinal)
}

pub(crate) type JoinedDataBlockBytes = (Vec<u8>, Vec<u64>, Vec<u64>, Vec<u64>);

pub(crate) fn join_data_block_bytes(
    ids: &[String],
    blocks: &BTreeMap<String, (&[u8], u64)>,
) -> Option<JoinedDataBlockBytes> {
    let source_blocks = ids
        .iter()
        .map(|id| blocks.get(id).copied())
        .collect::<Option<Vec<_>>>()?;
    let byte_len = source_blocks
        .iter()
        .map(|(bytes, _)| bytes.len())
        .sum::<usize>();
    let mut payload = Vec::with_capacity(byte_len);
    let mut block_payload_offsets = Vec::with_capacity(source_blocks.len());
    let mut block_byte_lengths = Vec::with_capacity(source_blocks.len());
    let mut block_source_offsets = Vec::with_capacity(source_blocks.len());
    for (bytes, source_offset) in source_blocks {
        block_payload_offsets.push(payload.len() as u64);
        block_byte_lengths.push(bytes.len() as u64);
        block_source_offsets.push(source_offset);
        payload.extend_from_slice(bytes);
    }
    Some((
        payload,
        block_payload_offsets,
        block_byte_lengths,
        block_source_offsets,
    ))
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
    let indexed = container.indexed_om_sections();
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
                let object_indices = lane
                    .values
                    .iter()
                    .map(|value| value.object_index)
                    .collect::<Vec<_>>();
                let data_blocks = object_indices
                    .iter()
                    .map(|index| unique_offset_data_block(&indexed, *index))
                    .collect();
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
                    object_indices,
                    data_blocks,
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
        let Some(lane_data_blocks) = lane.data_blocks.iter().cloned().collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if lane_data_blocks != data_blocks {
            continue;
        }
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
    let indexed = container.indexed_om_sections();
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
            let resolve = |indices: &[u32]| {
                indices
                    .iter()
                    .map(|index| unique_offset_data_block(&indexed, *index))
                    .collect::<Vec<_>>()
            };
            let atom_data_blocks = resolve(&branch.atom_indices);
            let first_data_blocks = resolve(&branch.first_indices);
            let second_data_blocks = resolve(&branch.second_indices);
            branches.push(FeatureExtrudePayload32Branch {
                id: format!(
                    "nx:feature-history:extrude-payload-32-branch#{section_key}-{operation_ordinal}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal}"
                ),
                body_object_index: branch.body_object_index,
                scalar: branch.scalar,
                atoms_be: branch.atoms_be,
                atom_indices: branch.atom_indices,
                atom_data_blocks,
                first_indices: branch.first_indices,
                first_data_blocks,
                second_indices: branch.second_indices,
                second_data_blocks,
                terminal_object_index: branch.terminal_object_index,
                source_offset: entry_offset + branch.offset as u64,
            });
        }
    }
    branches
}

/// Join exact profile fields to self-witnessed structured extrusion branches.
pub fn feature_extrude_32_constructions(
    references: &[FeatureExtrudeProfileReference],
    branches: &[FeatureExtrudePayload32Branch],
) -> Vec<FeatureExtrude32Construction> {
    let mut constructions = Vec::new();
    for branch in branches {
        let mut profile = references
            .iter()
            .filter(|reference| reference.operation_label == branch.operation_label)
            .collect::<Vec<_>>();
        profile.sort_by_key(|reference| reference.ordinal);
        if profile.is_empty()
            || profile
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
        {
            continue;
        }
        let Some(profile_data_blocks) = profile
            .iter()
            .map(|reference| reference.data_block.clone())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(atom_data_blocks) = branch.atom_data_blocks.iter().cloned().collect() else {
            continue;
        };
        let Some(first_data_blocks) = branch.first_data_blocks.iter().cloned().collect() else {
            continue;
        };
        let Some(second_data_blocks) = branch.second_data_blocks.iter().cloned().collect() else {
            continue;
        };
        constructions.push(FeatureExtrude32Construction {
            id: branch
                .id
                .replacen("extrude-payload-32-branch", "extrude-32-construction", 1),
            operation_label: branch.operation_label.clone(),
            branch: branch.id.clone(),
            body_object_index: branch.body_object_index,
            profile_references: profile
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            profile_data_blocks,
            atom_data_blocks,
            first_data_blocks,
            second_data_blocks,
        });
    }
    constructions
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

/// Join complete, uniquely resolved `BLOCK` construction-reference fields.
pub fn feature_block_constructions(
    references: &[FeatureBlockConstructionReference],
) -> Vec<FeatureBlockConstruction> {
    let mut by_operation = BTreeMap::<&str, Vec<&FeatureBlockConstructionReference>>::new();
    for reference in references {
        by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut constructions = Vec::new();
    for (operation_label, mut field) in by_operation {
        field.sort_by_key(|reference| reference.ordinal);
        if field.len() != 19
            || field.iter().enumerate().any(|(ordinal, reference)| {
                reference.ordinal != ordinal as u32
                    || reference.control != field[0].control
                    || reference.terminal != (ordinal == 18)
            })
        {
            continue;
        }
        let (terminal, members) = field.split_last().expect("nineteen references");
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
        constructions.push(FeatureBlockConstruction {
            id: operation_label.replacen("operation-label", "block-construction", 1),
            operation_label: operation_label.to_string(),
            control: field[0].control,
            member_references: members
                .iter()
                .map(|reference| reference.id.clone())
                .collect(),
            member_data_blocks,
            terminal_reference: terminal.id.clone(),
            terminal_data_block,
        });
    }
    constructions
}

/// Reconstruct complete `BLOCK` construction payloads in reference order.
pub fn feature_block_construction_payloads(
    container: &Container,
    constructions: &[FeatureBlockConstruction],
) -> Vec<FeatureBlockConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    constructions
        .iter()
        .filter_map(|construction| {
            let mut data_blocks = construction.member_data_blocks.clone();
            data_blocks.push(construction.terminal_data_block.clone());
            let (bytes, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureBlockConstructionPayload {
                id: construction
                    .id
                    .replacen("block-construction", "block-construction-payload", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets,
                block_byte_lengths,
                block_source_offsets,
            })
        })
        .collect()
}

/// Resolve the consecutive three-parameter dimension run of `BLOCK` features.
pub fn feature_block_dimensions(
    constructions: &[FeatureBlockConstruction],
    bindings: &[FeatureParameterBinding],
    declarations: &[ExpressionDeclaration],
    expressions: &[Expression],
) -> Vec<FeatureBlockDimensions> {
    constructions
        .iter()
        .filter_map(|construction| {
            let mut operation_bindings = bindings
                .iter()
                .filter(|binding| binding.operation_label == construction.operation_label)
                .collect::<Vec<_>>();
            operation_bindings
                .sort_by_key(|binding| (binding.input_slot, binding.reference_ordinal));
            let mut anchors = operation_bindings
                .iter()
                .map(|binding| binding.expression_declaration.as_str())
                .collect::<Vec<_>>();
            anchors.sort_unstable();
            anchors.dedup();
            let [anchor] = anchors.as_slice() else {
                return None;
            };
            let start = declarations
                .iter()
                .position(|declaration| declaration.id == *anchor)?;
            let run: [&ExpressionDeclaration; 3] = declarations
                .get(start..start + 3)?
                .iter()
                .collect::<Vec<_>>()
                .try_into()
                .ok()?;
            let first = run[0].parameter_index;
            if run.iter().enumerate().any(|(ordinal, declaration)| {
                declaration.parameter_index != first + ordinal as u32
                    || declaration.name != format!("p{}", declaration.parameter_index)
                    || declaration.qualifier.is_some()
            }) {
                return None;
            }
            let resolved: [(&Expression, f64); 3] = run
                .iter()
                .map(|declaration| {
                    let mut matches = expressions.iter().filter(|expression| {
                        expression.declaration.as_deref() == Some(&declaration.id)
                    });
                    let expression = matches.next()?;
                    if matches.next().is_some() || expression.unit != ExpressionUnit::Millimeter {
                        return None;
                    }
                    Some((
                        expression,
                        expression.value.filter(|value| value.is_finite())?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            Some(FeatureBlockDimensions {
                id: construction
                    .id
                    .replacen("block-construction", "block-dimensions", 1),
                operation_label: construction.operation_label.clone(),
                construction: construction.id.clone(),
                anchor_bindings: operation_bindings
                    .into_iter()
                    .map(|binding| binding.id.clone())
                    .collect(),
                declarations: run.map(|declaration| declaration.id.clone()),
                expressions: resolved
                    .each_ref()
                    .map(|(expression, _)| expression.id.clone()),
                values: resolved.map(|(_, value)| value),
            })
        })
        .collect()
}

/// Decode persistent object frames from bounded offset-store blocks.
pub fn data_block_object_frames(container: &Container) -> Vec<DataBlockObjectFrame> {
    let blocks = offset_data_block_bytes(container);
    blocks
        .into_iter()
        .flat_map(|(data_block, (bytes, source_offset))| {
            crate::om::data_block_object_frames(bytes)
                .into_iter()
                .enumerate()
                .map(|(ordinal, frame)| DataBlockObjectFrame {
                    id: format!(
                        "nx:om-data-block-object-frames-{}:frame#{}",
                        data_block
                            .rsplit_once('#')
                            .map_or("unknown", |(_, key)| key),
                        ordinal
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    object_id: frame.object_id,
                    source_offset: source_offset + frame.offset as u64,
                })
                .collect::<Vec<_>>()
        })
        .collect()
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
                && object_index != 0
                && usize::try_from(object_index)
                    .ok()
                    .is_some_and(|ordinal| ordinal <= candidate.records.len())
        })
        .map(|(section_ordinal, _)| {
            format!("nx:om-data-blocks-{section_ordinal}:block#{object_index}")
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
    expressions: &[Expression],
) -> Vec<FeatureParameterBinding> {
    let mut expressions_by_declaration = BTreeMap::<&str, Vec<&str>>::new();
    for expression in expressions {
        if let Some(declaration) = expression.declaration.as_deref() {
            expressions_by_declaration
                .entry(declaration)
                .or_default()
                .push(expression.id.as_str());
        }
    }
    let mut bindings = Vec::new();
    for input in inputs {
        for reference in references
            .iter()
            .filter(|reference| reference.data_block == input.data_block)
        {
            let Some(expression_declaration) = &reference.target_expression_declaration else {
                continue;
            };
            let operation_key = input
                .operation_label
                .rsplit_once('#')
                .map_or(input.operation_label.as_str(), |(_, key)| key);
            bindings.push(FeatureParameterBinding {
                id: format!(
                    "nx:feature-history:parameter-binding#{operation_key}-{}-{}",
                    input.input_slot, reference.ordinal
                ),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                input_block: input.data_block.clone(),
                reference_ordinal: reference.ordinal,
                expression_declaration: expression_declaration.clone(),
                expression: expressions_by_declaration
                    .get(expression_declaration.as_str())
                    .and_then(|matches| matches.as_slice().first().filter(|_| matches.len() == 1))
                    .map(|expression| (*expression).to_string()),
                object_id: reference.object_id,
                source_offset: reference.source_offset,
            });
        }
    }
    bindings
}

/// Group exact expression bindings by consuming operation and expression.
pub fn feature_parameter_uses(bindings: &[FeatureParameterBinding]) -> Vec<FeatureParameterUse> {
    let mut grouped = BTreeMap::<(&str, &str), Vec<&FeatureParameterBinding>>::new();
    for binding in bindings {
        if let Some(expression) = binding.expression.as_deref() {
            grouped
                .entry((binding.operation_label.as_str(), expression))
                .or_default()
                .push(binding);
        }
    }
    let mut uses = grouped
        .into_iter()
        .map(|((operation_label, expression), mut bindings)| {
            bindings.sort_by_key(|binding| binding.source_offset);
            (operation_label, expression, bindings)
        })
        .collect::<Vec<_>>();
    uses.sort_by_key(|(_, _, bindings)| bindings[0].source_offset);
    uses.into_iter()
        .map(|(operation_label, expression, bindings)| {
            let operation_key = operation_label
                .rsplit_once('#')
                .map_or(operation_label, |(_, key)| key);
            let expression_key = expression
                .rsplit_once('#')
                .map_or(expression, |(_, key)| key);
            FeatureParameterUse {
                id: format!("nx:feature-history:parameter-use#{operation_key}-{expression_key}"),
                operation_label: operation_label.to_string(),
                expression: expression.to_string(),
                bindings: bindings.iter().map(|binding| binding.id.clone()).collect(),
                source_offsets: bindings
                    .iter()
                    .map(|binding| binding.source_offset)
                    .collect(),
            }
        })
        .collect()
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
                    field_descriptor_prefix: definition.field_descriptor_prefix,
                    field_codes: definition.field_codes.to_vec(),
                    inflated_offset: definition.offset as u64,
                })
        })
        .collect()
}

/// Retain every non-null topology-to-attribute-list reference.
pub fn parasolid_topology_attribute_list_references(
    streams: &[Stream],
    entity_records: &[ParasolidEntity51Record],
) -> Vec<ParasolidTopologyAttributeListReference> {
    let mut records_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for record in entity_records {
        records_by_identity
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push(record.id.as_str());
    }
    let mut references = Vec::new();
    for (stream_ordinal, stream) in streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
        let graph = crate::topology::Graph::parse(&stream.inflated);
        for topology_type in [13, 14, 15, 16, 17, 18] {
            for node in graph.of_kind(topology_type) {
                let attribute_list_xmt = match topology_type {
                    13 => node.shell_fields().map(|fields| fields.attributes),
                    14 => node.face_fields().map(|fields| fields.attributes),
                    15 => node.loop_fields().map(|fields| fields.attributes),
                    16 => node.edge_fields().map(|fields| fields.attributes),
                    17 => node.fin_fields().map(|fields| fields.attributes),
                    18 => node.vertex_fields().map(|fields| fields.attributes),
                    _ => unreachable!("bounded topology family"),
                };
                let Some(attribute_list_xmt) = attribute_list_xmt.filter(|value| *value > 1) else {
                    continue;
                };
                let Some(inflated_offset) = node.attribute_field_offset() else {
                    continue;
                };
                references.push(ParasolidTopologyAttributeListReference {
                    id: format!(
                        "nx:s{stream_ordinal}:topology-attribute-list-reference#{topology_type}-{}",
                        node.xmt
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    topology_type,
                    topology_xmt: node.xmt,
                    attribute_list_xmt,
                    attribute_list_record: records_by_identity
                        .get(&(stream_ordinal as u32, attribute_list_xmt))
                        .and_then(|records| {
                            let [record] = records.as_slice() else {
                                return None;
                            };
                            Some((*record).to_string())
                        }),
                    inflated_offset: inflated_offset as u64,
                });
            }
        }
    }
    references
}

/// Decode every framed type-81 entity/attribute-list record.
pub fn parasolid_entity_51_records(streams: &[Stream]) -> Vec<ParasolidEntity51Record> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_51_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity51Record {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-51#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    flags: record.flags,
                    sequence: record.sequence,
                    discriminator: record.discriminator,
                    references: record.references,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every self-framed printable type-84 string record.
pub fn parasolid_entity_54_string_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity54StringRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_54_string_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity54StringRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-54-string#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    value: record.value.to_string(),
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every counted type-82 unsigned-integer record.
pub fn parasolid_entity_52_integer_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity52IntegerRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_52_integer_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity52IntegerRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-52-integers#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    values: record.values,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Decode every counted type-83 finite binary64 record.
pub fn parasolid_entity_53_double_records(
    streams: &[Stream],
) -> Vec<ParasolidEntity53DoubleRecord> {
    let mut records = streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind.is_parasolid())
        .flat_map(|(stream_ordinal, stream)| {
            crate::parasolid::entity_53_double_records(&stream.inflated)
                .into_iter()
                .map(move |record| ParasolidEntity53DoubleRecord {
                    id: format!(
                        "nx:s{stream_ordinal}:entity-53-doubles#{}-{}",
                        record.xmt, record.offset
                    ),
                    stream_ordinal: stream_ordinal as u32,
                    xmt: record.xmt,
                    values: record.values,
                    byte_len: record.byte_len as u64,
                    inflated_offset: record.offset as u64,
                })
        })
        .collect::<Vec<_>>();
    records.sort_by(|first, second| first.id.cmp(&second.id));
    records
}

/// Join type-81 reference slots to unique same-stream numeric value records.
pub fn parasolid_entity_51_numeric_uses(
    entities: &[ParasolidEntity51Record],
    integers: &[ParasolidEntity52IntegerRecord],
    doubles: &[ParasolidEntity53DoubleRecord],
) -> Vec<ParasolidEntity51NumericUse> {
    let mut values = BTreeMap::<(u32, u32), Vec<(ParasolidEntity51NumericKind, &str)>>::new();
    for record in integers {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::UnsignedIntegers, &record.id));
    }
    for record in doubles {
        values
            .entry((record.stream_ordinal, record.xmt))
            .or_default()
            .push((ParasolidEntity51NumericKind::Doubles, &record.id));
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (reference_ordinal, referenced_xmt) in entity.references.iter().copied().enumerate() {
            let Some([(kind, value_record)]) = values
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51NumericUse {
                id: format!(
                    "nx:s{}:entity-51-numeric-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                kind: *kind,
                value_record: (*value_record).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join type-81 reference slots to unique same-stream type-84 strings.
pub fn parasolid_entity_51_string_uses(
    entities: &[ParasolidEntity51Record],
    strings: &[ParasolidEntity54StringRecord],
) -> Vec<ParasolidEntity51StringUse> {
    let mut strings_by_identity = BTreeMap::<(u32, u32), Vec<&str>>::new();
    for string in strings {
        strings_by_identity
            .entry((string.stream_ordinal, string.xmt))
            .or_default()
            .push(string.id.as_str());
    }
    let mut uses = Vec::new();
    for entity in entities {
        for (reference_ordinal, referenced_xmt) in entity.references.iter().copied().enumerate() {
            let Some([string]) = strings_by_identity
                .get(&(entity.stream_ordinal, referenced_xmt))
                .map(Vec::as_slice)
            else {
                continue;
            };
            uses.push(ParasolidEntity51StringUse {
                id: format!(
                    "nx:s{}:entity-51-string-use#{}-{}-{reference_ordinal}",
                    entity.stream_ordinal, entity.xmt, entity.inflated_offset
                ),
                stream_ordinal: entity.stream_ordinal,
                entity_51_record: entity.id.clone(),
                reference_ordinal: reference_ordinal as u32,
                referenced_xmt,
                string_record: (*string).to_string(),
                inflated_offset: entity.inflated_offset,
            });
        }
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
}

/// Join topology-owned type-81 attribute instances to their stream-local class catalog.
pub fn parasolid_topology_attribute_class_uses(
    topology_references: &[ParasolidTopologyAttributeListReference],
    entities: &[ParasolidEntity51Record],
    definitions: &[ParasolidAttributeDefinition],
) -> Vec<ParasolidTopologyAttributeClassUse> {
    let entities = entities
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let mut definitions_by_stream = BTreeMap::<u32, Vec<&ParasolidAttributeDefinition>>::new();
    for definition in definitions {
        definitions_by_stream
            .entry(definition.stream_ordinal)
            .or_default()
            .push(definition);
    }
    for stream_definitions in definitions_by_stream.values_mut() {
        stream_definitions.sort_by_key(|definition| definition.inflated_offset);
    }

    let mut uses = Vec::new();
    for reference in topology_references {
        let Some(entity_id) = reference.attribute_list_record.as_deref() else {
            continue;
        };
        let Some(entity) = entities.get(entity_id) else {
            continue;
        };
        if entity.discriminator != 0x21 || entity.references.len() < 3 {
            continue;
        }
        let definition_ordinal = entity.references[2];
        let Some(definition_index) = definition_ordinal.checked_sub(1) else {
            continue;
        };
        let Some(definition) = definitions_by_stream
            .get(&entity.stream_ordinal)
            .and_then(|definitions| definitions.get(definition_index as usize))
        else {
            continue;
        };
        uses.push(ParasolidTopologyAttributeClassUse {
            id: format!(
                "nx:s{}:topology-attribute-class-use#{}-{}",
                reference.stream_ordinal, reference.topology_type, reference.topology_xmt
            ),
            topology_attribute_reference: reference.id.clone(),
            entity_51_record: entity.id.clone(),
            definition_ordinal,
            attribute_definition: definition.id.clone(),
        });
    }
    uses.sort_by(|first, second| first.id.cmp(&second.id));
    uses
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

/// Ordered value from a zero-prefixed offset-only OM store control array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlValue {
    /// Globally unique control-value identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based word order in the complete control block.
    pub ordinal: u32,
    /// Unsigned 24-bit value serialized after the zero byte.
    pub value: u32,
    /// Absolute file offset of the four-byte word.
    pub source_offset: u64,
}

/// Ordered little-endian value preceding a control-block product record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlIndexValue {
    /// Globally unique value identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based value order in the aligned prefix array.
    pub ordinal: u32,
    /// Number of leading zero bytes before the aligned array.
    pub prefix_byte_len: u8,
    /// Unsigned little-endian value.
    pub value: u32,
    /// Same-section offset-store block addressed by an in-range value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_data_block: Option<String>,
    /// Absolute file offset of the four-byte value.
    pub source_offset: u64,
}

/// Registered class selected by the leading lane of an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlClassReference {
    /// Globally unique class-reference identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based order in the class-selection lane.
    pub ordinal: u32,
    /// Zero-based ordinal in the store's class registry.
    pub class_ordinal: u32,
    /// Target in the native `class_definitions` arena.
    pub class_definition: String,
    /// Exact registered class name.
    pub class_name: String,
    /// Absolute file offset of the four-byte control word.
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

/// Complete counted block-index lane carried by one offset-store block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockCountedIndexLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based lane order within the block.
    pub ordinal: u32,
    /// Serialized count including the anchor and terminal slot.
    pub declared_count: u8,
    /// Decoded anchoring block index.
    pub anchor_index: u32,
    /// Same-section block addressed by the anchor.
    pub anchor_data_block: String,
    /// Ordered decoded member block indices.
    pub member_indices: Vec<u32>,
    /// Ordered same-section blocks addressed by the members.
    pub member_data_blocks: Vec<String>,
    /// Absolute file offset of the opening `01` marker.
    pub source_offset: u64,
    /// Absolute file offset of the anchoring compact index.
    pub anchor_source_offset: u64,
    /// Ordered absolute file offsets of member compact indices.
    pub member_source_offsets: Vec<u64>,
}

/// Fixed-width nullable `ABR` block-reference lane in contiguous column storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockAbrReferenceLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Zero-based indexed-section ordinal within the container.
    pub section_ordinal: u32,
    /// Zero-based lane order within the section's column storage.
    pub ordinal: u32,
    /// Sixteen ordered nullable serialized block indices.
    pub slot_indices: Vec<Option<u32>>,
    /// Sixteen ordered nullable same-section block identities.
    pub slot_data_blocks: Vec<Option<String>>,
    /// Absolute file offsets of the sixteen compact-index tokens.
    pub slot_source_offsets: Vec<u64>,
    /// Directory entry containing the offset-only store.
    pub source_entry: String,
    /// Absolute file offset of the opening `11` marker.
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

/// Ordered persistent or tagged reference in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlReference {
    /// Globally unique occurrence identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// Zero-based retained-reference order within the control block.
    pub ordinal: u32,
    /// Tagged reference family.
    pub kind: ObjectReferenceKind,
    /// Reference value without marker or tag bits.
    pub value: u32,
    /// Absolute file offset of the reference marker.
    pub source_offset: u64,
}

/// Exact two-token persistent-handle run in an offset-store control block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBlockControlHandlePair {
    /// Globally unique pair identity.
    pub id: String,
    /// Owning control block in the native `data_blocks` arena.
    pub data_block: String,
    /// First handle-reference occurrence.
    pub first_reference: String,
    /// Second handle-reference occurrence.
    pub second_reference: String,
    /// First persistent-handle value.
    pub first_handle: u32,
    /// Second persistent-handle value.
    pub second_handle: u32,
    /// Absolute file offset of the first `e0` marker.
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
    /// Ordered distinct offset-store control blocks containing the handle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_blocks: Vec<String>,
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

/// Decode complete zero-prefixed control arrays from offset-only OM stores.
pub fn data_block_control_values(container: &Container) -> Vec<DataBlockControlValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let Some(values) = crate::om::offset_store_control_values(control.bytes) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlValue {
                    id: format!(
                        "nx:om-data-block-control-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    value,
                    source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                })
                .collect()
        })
        .collect()
}

/// Resolve each atomic leading control lane through its store-local class registry.
pub fn data_block_control_class_references(
    container: &Container,
) -> Vec<DataBlockControlClassReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let registry = container
                .om_sections()
                .into_iter()
                .filter(|(candidate, _)| std::ptr::eq(*candidate, entry))
                .flat_map(|(_, section)| section.types)
                .map(|definition| (definition.offset, definition))
                .collect::<BTreeMap<_, _>>()
                .into_values()
                .collect::<Vec<_>>();
            let Some(ordinals) = crate::om::offset_store_control_class_ordinals(
                control.bytes,
                registry.len(),
            ) else {
                return Vec::new();
            };
            let entry_index = container
                .entries
                .iter()
                .position(|candidate| std::ptr::eq(candidate, entry))
                .expect("indexed entry belongs to container");
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            ordinals
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, class_ordinal)| {
                    let definition = registry.get(usize::try_from(class_ordinal).ok()?)?;
                    Some(DataBlockControlClassReference {
                        id: format!(
                            "nx:om-data-block-control-class-references-{section_ordinal}:class#{ordinal}"
                        ),
                        data_block: data_block.clone(),
                        ordinal: ordinal as u32,
                        class_ordinal,
                        class_definition: format!(
                            "nx:om-entry-{entry_index}:class#{}",
                            definition.offset
                        ),
                        class_name: definition.name.to_string(),
                        source_offset: entry_offset + control.offset as u64 + ordinal as u64 * 4,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode aligned index arrays ending at a unique control-block product record.
pub fn data_block_control_index_values(container: &Container) -> Vec<DataBlockControlIndexValue> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let Some((prefix_byte_len, values)) =
                crate::om::offset_store_index_values(control.bytes)
            else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            let block_count = section.records.len() + 1;
            values
                .into_iter()
                .enumerate()
                .map(|(ordinal, value)| DataBlockControlIndexValue {
                    id: format!(
                        "nx:om-data-block-control-index-values-{section_ordinal}:value#{ordinal}"
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    prefix_byte_len: prefix_byte_len as u8,
                    value,
                    target_data_block: control_index_data_block(
                        section_ordinal,
                        block_count,
                        value,
                    ),
                    source_offset: entry_offset
                        + control.offset as u64
                        + prefix_byte_len as u64
                        + ordinal as u64 * 4,
                })
                .collect()
        })
        .collect()
}

pub(crate) fn control_index_data_block(
    section_ordinal: usize,
    block_count: usize,
    value: u32,
) -> Option<String> {
    let ordinal = usize::try_from(value)
        .ok()
        .filter(|ordinal| *ordinal < block_count)?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{ordinal}"
    ))
}

/// Decode persistent-handle and tagged-28 occurrences in bounded control blocks.
pub fn data_block_control_references(container: &Container) -> Vec<DataBlockControlReference> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(control) = section.control else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let data_block = format!("nx:om-data-blocks-{section_ordinal}:block#0");
            crate::om::references(control.bytes, control.offset)
                .into_iter()
                .filter(|reference| reference.kind != crate::om::ReferenceKind::RecordOrdinal16)
                .enumerate()
                .map(|(ordinal, reference)| DataBlockControlReference {
                    id: format!(
                        "nx:om-data-block-control-references-{section_ordinal}:reference#{}",
                        reference.offset
                    ),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    kind: match reference.kind {
                        crate::om::ReferenceKind::PersistentHandle => {
                            ObjectReferenceKind::PersistentHandle
                        }
                        crate::om::ReferenceKind::Tagged28 => ObjectReferenceKind::Tagged28,
                        crate::om::ReferenceKind::RecordOrdinal16 => unreachable!("filtered"),
                    },
                    value: reference.value,
                    source_offset: entry_offset + reference.offset as u64,
                })
                .collect()
        })
        .collect()
}

/// Join maximal two-token adjacent persistent-handle runs atomically.
pub fn data_block_control_handle_pairs(
    references: &[DataBlockControlReference],
) -> Vec<DataBlockControlHandlePair> {
    let mut by_block = BTreeMap::<&str, Vec<&DataBlockControlReference>>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        by_block
            .entry(reference.data_block.as_str())
            .or_default()
            .push(reference);
    }
    let mut pairs = Vec::new();
    for (data_block, mut block_references) in by_block {
        block_references.sort_by_key(|reference| reference.source_offset);
        let mut at = 0;
        while at < block_references.len() {
            let start = at;
            while block_references
                .get(at + 1)
                .is_some_and(|next| next.source_offset == block_references[at].source_offset + 5)
            {
                at += 1;
            }
            let run = &block_references[start..=at];
            if let [first, second] = run {
                pairs.push(DataBlockControlHandlePair {
                    id: format!(
                        "nx:om-data-block-control:handle-pair#{}",
                        first.source_offset
                    ),
                    data_block: data_block.to_string(),
                    first_reference: first.id.clone(),
                    second_reference: second.id.clone(),
                    first_handle: first.value,
                    second_handle: second.value,
                    source_offset: first.source_offset,
                });
            }
            at += 1;
        }
    }
    pairs
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

/// Decode complete in-range counted block-index lanes from offset-only stores.
pub fn data_block_counted_index_lanes(container: &Container) -> Vec<DataBlockCountedIndexLane> {
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
            let block_count = section.records.len() + 1;
            section
                .records
                .into_iter()
                .enumerate()
                .flat_map(|(record_ordinal, block)| {
                    let block_ordinal = record_ordinal + 1;
                    crate::om::offset_store_counted_index_lanes(block.bytes)
                        .into_iter()
                        .filter_map(|lane| {
                            let anchor_data_block = control_index_data_block(
                                section_ordinal,
                                block_count,
                                lane.anchor,
                            )?;
                            let member_data_blocks = lane
                                .members
                                .iter()
                                .map(|(value, _)| {
                                    control_index_data_block(
                                        section_ordinal,
                                        block_count,
                                        *value,
                                    )
                                })
                                .collect::<Option<Vec<_>>>()?;
                            let source_base = entry_offset + block.offset as u64;
                            Some((lane, anchor_data_block, member_data_blocks, source_base))
                        })
                        .enumerate()
                        .map(
                            |(
                                ordinal,
                                (lane, anchor_data_block, member_data_blocks, source_base),
                            )| DataBlockCountedIndexLane {
                                id: format!(
                                    "nx:om-data-block-counted-index-lanes-{section_ordinal}-{block_ordinal}:lane#{ordinal}"
                                ),
                                data_block: format!(
                                    "nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"
                                ),
                                ordinal: ordinal as u32,
                                declared_count: lane.declared_count,
                                anchor_index: lane.anchor,
                                anchor_data_block,
                                member_indices: lane
                                    .members
                                    .iter()
                                    .map(|(value, _)| *value)
                                    .collect(),
                                member_data_blocks,
                                source_offset: source_base + lane.offset as u64,
                                anchor_source_offset: source_base + lane.anchor_offset as u64,
                                member_source_offsets: lane
                                    .members
                                    .iter()
                                    .map(|(_, offset)| source_base + *offset as u64)
                                    .collect(),
                            },
                        )
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

/// Decode complete in-range `ABR` reference lanes from offset-store column storage.
pub fn data_block_abr_reference_lanes(container: &Container) -> Vec<DataBlockAbrReferenceLane> {
    container
        .indexed_om_sections()
        .into_iter()
        .enumerate()
        .flat_map(|(section_ordinal, (entry, section))| {
            let Some(storage) = section.column_storage else {
                return Vec::new();
            };
            let Some(storage_offset) = section.records.first().map(|record| record.offset) else {
                return Vec::new();
            };
            let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
            let source_base = entry_offset + storage_offset as u64;
            let block_count = section.records.len() + 1;
            crate::om::offset_store_abr_reference_lanes(storage)
                .into_iter()
                .filter_map(|lane| {
                    let slot_data_blocks = lane
                        .slots
                        .iter()
                        .map(|(value, _)| {
                            value.map_or(Some(None), |value| {
                                control_index_data_block(section_ordinal, block_count, value)
                                    .map(Some)
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some((lane, slot_data_blocks))
                })
                .enumerate()
                .map(
                    |(ordinal, (lane, slot_data_blocks))| DataBlockAbrReferenceLane {
                        id: format!(
                            "nx:om-data-block-abr-reference-lanes-{section_ordinal}:lane#{ordinal}"
                        ),
                        section_ordinal: section_ordinal as u32,
                        ordinal: ordinal as u32,
                        slot_indices: lane.slots.iter().map(|(value, _)| *value).collect(),
                        slot_data_blocks,
                        slot_source_offsets: lane
                            .slots
                            .iter()
                            .map(|(_, offset)| source_base + *offset as u64)
                            .collect(),
                        source_entry: entry.name.clone(),
                        source_offset: source_base + lane.offset as u64,
                    },
                )
                .collect::<Vec<_>>()
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
    control_references: &[DataBlockControlReference],
    external: &[ExternalReferenceRecord],
) -> Vec<PersistentHandle> {
    let mut groups = BTreeMap::<u32, (Vec<String>, u32, Vec<String>, Vec<String>)>::new();
    for reference in references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let (records, occurrence_count, _, _) = groups.entry(reference.value).or_default();
        *occurrence_count += 1;
        if records.last() != Some(&reference.record) && !records.contains(&reference.record) {
            records.push(reference.record.clone());
        }
    }
    for reference in control_references
        .iter()
        .filter(|reference| reference.kind == ObjectReferenceKind::PersistentHandle)
    {
        let (_, occurrence_count, _, data_blocks) = groups.entry(reference.value).or_default();
        *occurrence_count += 1;
        if !data_blocks.contains(&reference.data_block) {
            data_blocks.push(reference.data_block.clone());
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
            |(value, (records, occurrence_count, external_records, data_blocks))| {
                PersistentHandle {
                    id: format!("nx:om-persistent-handles:handle#{value:08x}"),
                    value,
                    records,
                    occurrence_count,
                    data_blocks,
                    external_records,
                }
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
