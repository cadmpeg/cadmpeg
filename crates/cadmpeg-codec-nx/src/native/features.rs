// SPDX-License-Identifier: Apache-2.0
//! Feature-history record extractors and their record types.

#[allow(clippy::wildcard_imports)]
use super::*;
use crate::native::om::{
    data_blocks, DataBlockColumnIndexTable, DataBlockIndexRow, DataBlockLinkedIndexRow,
    DataBlockReference, DataBlockTargetIndexRow, Expression, ExpressionDeclaration, ExpressionUnit,
    OmSchemaRole,
};
use crate::native::segments::{segment_om_links, SegmentBodyBinding, SegmentOmLink};

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
    /// Exact serialized object-index tokens in header order.
    pub raw_object_indices: [Vec<u8>; 4],
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

/// Exact nonempty redundantly witnessed scalar lane in a simple-hole payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarLane {
    /// Globally unique repeated-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered finite shifted-binary64 values.
    pub values: Vec<f64>,
    /// Exact scalar encodings shared by both witnesses.
    pub raw_values: Vec<[u8; 8]>,
    /// Absolute offsets of the first scalar lane.
    pub first_witness_offsets: Vec<u64>,
    /// Absolute offsets of the byte-identical repeated scalar lane.
    pub second_witness_offsets: Vec<u64>,
}

/// Offset-store blocks linked after both repeated scalar-lane witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
    /// Globally unique reference-lane identity.
    pub id: String,
    /// Owning `SIMPLE HOLE` operation label.
    pub operation_label: String,
    /// Ordered blocks following the first scalar pair.
    pub first_data_blocks: [String; 2],
    /// Ordered blocks following the repeated scalar lane.
    pub second_data_blocks: [String; 2],
    /// Absolute offsets of the first pair of tagged-index tokens.
    pub first_reference_offsets: [u64; 2],
    /// Absolute offsets of the repeated pair of tagged-index tokens.
    pub second_reference_offsets: [u64; 2],
}

/// Distinct simple-hole operations sharing one four-block construction identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSimpleHoleConstructionGroup {
    /// Globally unique group identity.
    pub id: String,
    /// Shared first-witness block pair.
    pub first_data_blocks: [String; 2],
    /// Shared repeated-witness block pair.
    pub second_data_blocks: [String; 2],
    /// Operation labels in feature-history order.
    pub operation_labels: Vec<String>,
    /// Scalar lanes aligned with `operation_labels`.
    pub scalar_lanes: Vec<String>,
    /// Block-reference lanes aligned with `operation_labels`.
    pub block_references: Vec<String>,
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
    /// Exact serialized variable-width object-index token.
    pub raw_body_object_index: Vec<u8>,
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
    /// Exact serialized variable-width object-index token.
    pub raw_body_object_index: Vec<u8>,
    /// Absolute file offset of the object-index token.
    pub source_offset: u64,
}

/// Unambiguous reuse of one segment body image by a primary feature body field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBodySegmentUse {
    /// Globally unique use identity.
    pub id: String,
    /// Primary field in the native `feature_body_references` arena.
    pub feature_body_reference: String,
    /// Segment image in the native `segment_body_bindings` arena.
    pub segment_body_binding: String,
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
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
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

/// Serialized column-row grammar carrying a reused feature input block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnIndexRowKind {
    /// `2d 02 0b ... 93 8a` index row.
    Index,
    /// `02 0b ... 93 8c` linked-index row.
    LinkedIndex,
    /// `02 01 01 01 16` target-index row.
    TargetIndex,
}

impl ColumnIndexRowKind {
    const fn id_component(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::LinkedIndex => "linked-index",
            Self::TargetIndex => "target-index",
        }
    }
}

/// Exact reuse of one feature input block by any column-row slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputColumnRowUse {
    /// Globally unique use identity.
    pub id: String,
    /// Feature input binding that resolves to the shared block.
    pub input_block: String,
    /// Owning feature operation label.
    pub operation_label: String,
    /// Input slot in the operation header.
    pub input_slot: u8,
    /// Serialized grammar of the referenced column row.
    pub row_kind: ColumnIndexRowKind,
    /// Native row identity in its grammar-specific arena.
    pub column_row: String,
    /// Unique complete composite table containing the row, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_table: Option<String>,
    /// Zero-based slot in the row's four-block lane.
    pub row_slot: u8,
    /// Exact shared target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the row's compact block index.
    pub source_offset: u64,
}

/// Unique composite-table target row for one feature input block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInputColumnTarget {
    /// Globally unique target identity.
    pub id: String,
    /// Feature input binding that resolves to the target block.
    pub input_block: String,
    /// Owning feature operation label.
    pub operation_label: String,
    /// Input slot in the operation header.
    pub input_slot: u8,
    /// Linked or target-index row whose slot zero is the input block.
    pub column_row: String,
    /// Serialized grammar of `column_row`.
    pub row_kind: ColumnIndexRowKind,
    /// Leading compact value present only in the linked-row grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_index: Option<u32>,
    /// Absolute offset of `leading_index` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_index_source_offset: Option<u64>,
    /// Linked-row discriminator when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<u8>,
    /// Three compact values following the fixed row marker.
    pub field_indices: [u32; 3],
    /// Three same-section blocks addressed by `field_indices`.
    pub field_data_blocks: [String; 3],
    /// Absolute offsets of the three compact field values.
    pub field_source_offsets: [u64; 3],
    /// Linked-row flag when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<u8>,
    /// Serialized row mode.
    pub mode: u8,
    /// Unique complete composite table containing `column_row`.
    pub column_table: String,
    /// Exact target in the native `data_blocks` arena.
    pub data_block: String,
    /// Absolute file offset of the row's target block index.
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
    /// Exact serialized object-index tokens in lane order.
    pub raw_object_indices: [Vec<u8>; 8],
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
    /// Exact shifted-binary64 encodings in value order.
    pub raw_values: [[u8; 8]; 2],
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

/// One exactly framed signed Q1.55 pair in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub datum_csys_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    pub values: [f64; 2],
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: [[u8; 7]; 2],
    /// Exact discriminator selecting the pair branch.
    pub discriminator: Vec<u8>,
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two `30` atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two `30` atom markers.
    pub value_source_offsets: [u64; 2],
}

/// One exactly framed scalar field in a reconstructed datum-CSYS payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDatumCsysPayloadScalar {
    /// Globally unique scalar-field identity.
    pub id: String,
    /// Owning `DATUM_CSYS` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the field.
    pub datum_csys_payload: String,
    /// Zero-based field order within the payload.
    pub ordinal: u32,
    /// Serialized discriminator following the `50 59 66` marker.
    pub field_code: u8,
    /// Finite shifted-IEEE binary64 value.
    pub value: f64,
    /// Exact shifted-binary64 encoding.
    pub raw_value: [u8; 8],
    /// Payload-relative offset of the field marker.
    pub payload_offset: u64,
    /// Absolute source offset of the field marker.
    pub source_offset: u64,
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
    /// Exact compact descriptor-index tokens in branch order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_descriptor_indices: Vec<Vec<u8>>,
    /// Ordered canonical object indices carried by the selected branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_indices: Vec<u32>,
    /// Exact canonical object-index tokens in branch order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_object_indices: Vec<Vec<u8>>,
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
    /// Exact compact-index tokens in serialized order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_lane_raw_indices: Vec<Vec<u8>>,
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
    /// Exact shifted-binary64 encodings in value order.
    pub raw_values: [[u8; 8]; 2],
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
    /// Exact shifted-binary64 encodings in value order.
    pub raw_values: [[u8; 8]; 2],
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

/// One exactly framed signed fixed-point pair in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPayloadFixedPair {
    /// Globally unique fixed-pair identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Reconstructed sketch payload carrying the frame.
    pub construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless signed Q1.55 values.
    pub values: [f64; 2],
    /// Exact ordered seven-byte two's-complement payloads.
    pub raw_values: [[u8; 7]; 2],
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the two atom markers.
    pub value_payload_offsets: [u64; 2],
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the two atom markers.
    pub value_source_offsets: [u64; 2],
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
    /// Exact shifted-binary64 encoding.
    pub raw_value: [u8; 8],
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
    /// Exact compact type-code token, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type_code: Option<Vec<u8>>,
    /// Payload-relative offset of the compact type-code token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code_payload_offset: Option<u64>,
    /// Absolute source offset of the compact type-code token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code_source_offset: Option<u64>,
    /// Whether the field uses the type-free payload-leading form.
    pub payload_leading: bool,
    /// Exact printable field value.
    pub value: String,
    /// Byte offset of the opening `66` or payload-leading `03` marker.
    pub payload_offset: u64,
    /// Absolute file offset of the opening marker.
    pub source_offset: u64,
}

/// Named sketch payload interval and its ordered framed numeric fields.
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
    /// Ordered fixed-pair fields before the next complete name field.
    pub fixed_pairs: Vec<String>,
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

/// Complete named dimensionless fixed-point record in a reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchFixedPoint {
    /// Globally unique fixed-point identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Name-delimited payload record carrying the pair.
    pub named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Exact fixed-pair field carrying the two values.
    pub fixed_pair: String,
    /// Ordered dimensionless signed Q1.55 values.
    pub values: [f64; 2],
    /// Absolute source offset of the fixed-pair discriminator.
    pub source_offset: u64,
}

/// Exact same-name point identity within one reconstructed sketch payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSketchPointGroup {
    /// Globally unique point-group identity.
    pub id: String,
    /// Owning `SKETCH` operation label.
    pub operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Identical point records in payload order.
    pub points: Vec<String>,
    /// Bit-identical ordered coordinate values.
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
    /// Exact shifted-binary64 encodings in scalar order.
    pub raw_values: [[u8; 8]; 2],
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

/// Exact predecessor relation between a named point and a sketch construction lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPrecedingNamedPointUse {
    /// Globally unique predecessor-use identity.
    pub id: String,
    /// Sketch operation carrying the construction lane.
    pub operation_label: String,
    /// First typed sketch-reference occurrence.
    pub first_sketch_reference: String,
    /// Typed named-point object ending immediately before the construction lane.
    pub named_point: String,
    /// Complete ordered block span of the named point.
    pub point_data_blocks: Vec<String>,
    /// First construction block immediately following the point span.
    pub following_data_block: String,
    /// Absolute source offset of the first sketch reference.
    pub source_offset: u64,
}

/// Exact identity of one solved sketch point across its payload and reference lanes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchPointUse {
    /// Globally unique point-use identity.
    pub id: String,
    /// Sketch operation carrying both point encodings.
    pub operation_label: String,
    /// Ordered sketch-reference occurrences addressing the named-point span.
    pub sketch_references: Vec<String>,
    /// Exact block-use witnesses corresponding to the sketch references.
    pub block_uses: Vec<String>,
    /// Exact same-name sketch-point group.
    pub sketch_point_group: String,
    /// Independently framed named-point object addressed by the reference.
    pub named_point: String,
    /// Absolute source offsets of the sketch references.
    pub source_offsets: Vec<u64>,
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FeatureSketchDatumCsysBlockRelation {
    /// The named-point span and coordinate-system construction address one block.
    Shared {
        /// Block addressed by both the named-point span and construction.
        data_block: String,
    },
    /// The coordinate-system construction begins immediately after the named-point span.
    Consecutive {
        /// Final block in the complete named-point span.
        point_data_block: String,
        /// First block in the coordinate-system construction.
        construction_data_block: String,
    },
}

/// Exact ordered dependency from a sketch point to a datum coordinate system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSketchDatumCsysDependency {
    /// Globally unique dependency identity.
    pub id: String,
    /// Earlier sketch operation owning the point identity.
    pub sketch_operation_label: String,
    /// Later datum-coordinate-system operation consuming the point block.
    pub datum_csys_operation_label: String,
    /// Exact sketch-point identity witnessing ownership.
    pub sketch_point_use: String,
    /// Exact datum-coordinate-system construction witnessing consumption.
    pub datum_csys_construction: String,
    /// Exact block relation between the complete point span and construction.
    pub block_relation: FeatureSketchDatumCsysBlockRelation,
    /// Absolute source offset of the first sketch reference witnessing the point identity.
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
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureProjectedCurveReference {
    /// Globally unique projected-curve reference identity.
    pub id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub operation_label: String,
    /// Zero-based order among the field's non-repeated references.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Exact logical payload reconstructed from a projected-curve reference field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureProjectedCurveConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub operation_label: String,
    /// Exact operation family selecting the reference grammar.
    pub operation_kind: String,
    /// Ordered non-repeated construction-reference records.
    pub construction_references: Vec<String>,
    /// Ordered source blocks.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: Vec<u64>,
    /// Exact source-block lengths.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute source-block offsets.
    pub block_source_offsets: Vec<u64>,
}

/// Canonical printable string in a reconstructed projected-curve payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureProjectedCurveConstructionString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `CPROJ` or `CPROJ_CMB` operation label.
    pub operation_label: String,
    /// Reconstructed projected-curve payload carrying the string.
    pub construction_payload: String,
    /// Zero-based string order within the payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Payload-relative offset of the `66 32 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePatternReference {
    /// Globally unique pattern-reference identity.
    pub id: String,
    /// Owning pattern operation label.
    pub operation_label: String,
    /// Zero-based non-null slot order in the exact reference field.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Exact logical payload reconstructed from an ordered pattern-reference graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePatternConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub operation_label: String,
    /// Exact operation family selecting the graph grammar.
    pub operation_kind: String,
    /// Ordered non-null construction-reference records.
    pub construction_references: Vec<String>,
    /// Ordered source blocks.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: Vec<u64>,
    /// Exact source-block lengths.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute source-block offsets.
    pub block_source_offsets: Vec<u64>,
}

/// Canonical printable string in a reconstructed pattern payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePatternConstructionString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub operation_label: String,
    /// Reconstructed pattern payload carrying the string.
    pub construction_payload: String,
    /// Zero-based string order within the payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Payload-relative offset of the `66 32 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// Complete signed Q1.55 lane in a reconstructed pattern payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturePatternConstructionFixedLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub operation_label: String,
    /// Reconstructed pattern payload carrying the lane.
    pub construction_payload: String,
    /// Zero-based lane order within the payload.
    pub ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    pub values: Vec<f64>,
    /// Exact atom markers in value order.
    pub markers: Vec<u8>,
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: Vec<[u8; 7]>,
    /// Payload-relative offset of the fixed discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the atom markers.
    pub value_payload_offsets: Vec<u64>,
    /// Absolute source offset of the fixed discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the atom markers.
    pub value_source_offsets: Vec<u64>,
}

/// Scalar width selected by one exact pattern-transform lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePatternTransformEncoding {
    /// Four-byte shifted IEEE-754 binary32 rows.
    Binary32,
    /// Eight-byte shifted IEEE-754 binary64 rows.
    Binary64,
}

/// Exact counted transform lane carried by a bounded pattern payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturePatternTransformLane {
    /// Globally unique transform-lane identity.
    pub id: String,
    /// Owning `Pattern Feature` or `Pattern Geometry` operation label.
    pub operation_label: String,
    /// Count including the implicit seed row.
    pub declared_count: u8,
    /// Homogeneous scalar encoding selected by the operation family.
    pub encoding: FeaturePatternTransformEncoding,
    /// Ordered finite row scalars.
    pub values: Vec<f64>,
    /// Exact scalar encodings in row order.
    pub raw_values: Vec<Vec<u8>>,
    /// Ordered non-null compact selectors.
    pub selectors: Vec<u32>,
    /// Exact compact-index selector tokens in row order.
    pub raw_selectors: Vec<Vec<u8>>,
    /// Absolute source offset of the opening `01, count` field.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: Vec<u64>,
    /// Absolute source offsets of the selector tokens.
    pub selector_source_offsets: Vec<u64>,
}

/// Exact leading construction header carried by a bounded point-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturePointConstructionHeader {
    /// Globally unique point-construction-header identity.
    pub id: String,
    /// Owning `POINT` operation label.
    pub operation_label: String,
    /// Serialized construction object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Serialized header mode.
    pub mode: u8,
    /// Absolute file offset of the reference width marker.
    pub source_offset: u64,
}

/// Exact cross-block scalar lane selected by a point-construction header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeaturePointConstructionScalarLane {
    /// Globally unique point-construction scalar-lane identity.
    pub id: String,
    /// Owning `POINT` operation label.
    pub operation_label: String,
    /// Header selecting this lane.
    pub construction_header: String,
    /// Preceding and target data blocks in byte order.
    pub data_blocks: [String; 2],
    /// Six finite scalar values in byte order.
    pub values: [f64; 6],
    /// Exact shifted-binary64 encodings in byte order.
    pub raw_values: [[u8; 8]; 6],
    /// Absolute file offsets of the six scalar markers.
    pub source_offsets: [u64; 6],
}

/// Ordered construction reference carried by a bounded draft-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionReference {
    /// Globally unique draft-construction-reference identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Zero-based slot order in the exact construction graph.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Counted compact-index lane preceding a bounded draft construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionIndexLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Serialized count including the omitted lane owner.
    pub declared_count: u8,
    /// Non-null compact indices in serialized order.
    pub indices: Vec<u32>,
    /// Exact compact-index tokens in serialized order.
    pub raw_indices: Vec<Vec<u8>>,
    /// Same-store native blocks when the complete lane and graph select one store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_blocks: Option<Vec<String>>,
    /// Absolute source offsets of the compact-index tokens.
    pub source_offsets: Vec<u64>,
}

/// Exact logical payload reconstructed from a resolved draft index lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Counted index lane defining the ordered block sequence.
    pub index_lane: String,
    /// Ordered source blocks.
    pub data_blocks: Vec<String>,
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: Vec<u64>,
    /// Exact source-block lengths.
    pub block_byte_lengths: Vec<u64>,
    /// Absolute source-block offsets.
    pub block_source_offsets: Vec<u64>,
}

/// Exact logical payload reconstructed from the ordered draft construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionGraphPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Counted index lane establishing the common offset store.
    pub index_lane: String,
    /// Ordered construction-reference records.
    pub construction_references: [String; 4],
    /// Ordered source blocks.
    pub data_blocks: [String; 4],
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: [u64; 4],
    /// Exact source-block lengths.
    pub block_byte_lengths: [u64; 4],
    /// Absolute source-block offsets.
    pub block_source_offsets: [u64; 4],
}

/// Complete signed Q1.55 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionFixedLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    pub graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    pub ordinal: u32,
    /// Ordered dimensionless Q1.55 values.
    pub values: Vec<f64>,
    /// Exact atom markers in value order.
    pub markers: Vec<u8>,
    /// Exact seven-byte two's-complement payloads.
    pub raw_values: Vec<[u8; 7]>,
    /// Payload-relative offset of the fixed discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the atom markers.
    pub value_payload_offsets: Vec<u64>,
    /// Absolute source offset of the fixed discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the atom markers.
    pub value_source_offsets: Vec<u64>,
}

/// Complete shifted-binary32 lane in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionBinary32Lane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed graph payload carrying the lane.
    pub graph_payload: String,
    /// Zero-based lane order in the reconstructed payload.
    pub ordinal: u32,
    /// Exact discriminator selecting the lane form.
    pub discriminator: [u8; 18],
    /// Exact `03` or `04` branch byte.
    pub branch: u8,
    /// Ordered finite shifted-IEEE binary32 values.
    pub values: Vec<f64>,
    /// Exact four-byte shifted encodings.
    pub raw_values: Vec<[u8; 4]>,
    /// Payload-relative offset of the discriminator.
    pub payload_offset: u64,
    /// Payload-relative offsets of the scalar encodings.
    pub value_payload_offsets: Vec<u64>,
    /// Absolute source offset of the discriminator.
    pub source_offset: u64,
    /// Absolute source offsets of the scalar encodings.
    pub value_source_offsets: Vec<u64>,
}

/// Canonical printable string in a reconstructed draft graph payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionGraphString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed graph payload carrying the string.
    pub graph_payload: String,
    /// Zero-based string order in the reconstructed payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Payload-relative offset of the `66 32 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// Complete identity frame in a reconstructed draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionIdentityFrame {
    /// Globally unique frame identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Reconstructed payload carrying the frame.
    pub draft_construction_payload: String,
    /// Zero-based frame order in the reconstructed payload.
    pub ordinal: u32,
    /// Exact bytes from the opening marker through the identity introducer.
    pub prefix: Vec<u8>,
    /// Typed frame form selected by the exact prefix.
    pub form: FeatureDraftConstructionIdentityFrameForm,
    /// Nonempty lowercase hexadecimal identity.
    pub identity: String,
    /// Payload-relative offset of the opening marker.
    pub payload_offset: u64,
    /// Payload-relative identity offset.
    pub identity_payload_offset: u64,
    /// Absolute source offset of the opening marker.
    pub source_offset: u64,
    /// Absolute source offset of the identity.
    pub identity_source_offset: u64,
}

/// Typed prefix form of a draft construction identity frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FeatureDraftConstructionIdentityFrameForm {
    /// Two compact indices and a `02` or `03` branch.
    IndexedBranch {
        /// Non-null first compact index.
        first_index: u32,
        /// Nullable second compact index.
        second_index: Option<u32>,
        /// Exact `02` or `03` branch byte.
        branch: u8,
    },
    /// One nullable compact index followed by `ff 02 01`.
    Tagged {
        /// Nullable compact index.
        index: Option<u32>,
    },
}

/// End-anchored compact-index lane in a bounded draft construction payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDraftConstructionTerminalLane {
    /// Globally unique lane identity.
    pub id: String,
    /// Owning `DRAFT` operation label.
    pub operation_label: String,
    /// Two non-null compact indices in serialized order.
    pub indices: [u32; 2],
    /// Exact two-byte compact-index tokens in serialized order.
    pub raw_indices: [[u8; 2]; 2],
    /// Exact uninterpreted bytes preceding the terminal zero.
    pub tail: [u8; 3],
    /// Absolute source offsets of the compact-index tokens.
    pub index_source_offsets: [u64; 2],
    /// Absolute source offset of the first compact-index token.
    pub source_offset: u64,
}

/// Ordered construction reference carried by a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionReference {
    /// Globally unique surface-construction-reference identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Zero-based slot order in the exact common envelope.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// Exact logical payload reconstructed from an ordered surface-construction graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionPayload {
    /// Globally unique reconstructed-payload identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Ordered construction-reference records.
    pub construction_references: [String; 14],
    /// Ordered source blocks.
    pub data_blocks: [String; 14],
    /// Exact concatenated payload length.
    pub byte_len: u64,
    /// SHA-256 of the concatenated bytes.
    pub sha256: String,
    /// Payload-relative block starts.
    pub block_payload_offsets: [u64; 14],
    /// Exact source-block lengths.
    pub block_byte_lengths: [u64; 14],
    /// Absolute source-block offsets.
    pub block_source_offsets: [u64; 14],
}

/// One exactly framed scalar pair in a reconstructed surface payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionScalarPair {
    /// Globally unique scalar-pair identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Reconstructed surface payload carrying the frame.
    pub surface_construction_payload: String,
    /// Zero-based frame order within the payload.
    pub ordinal: u32,
    /// Ordered finite shifted-IEEE values.
    pub values: [f64; 2],
    /// Exact shifted-binary64 encodings in value order.
    pub raw_values: [[u8; 8]; 2],
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

/// One printable string frame in a reconstructed surface payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionString {
    /// Globally unique string identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Reconstructed surface payload carrying the frame.
    pub surface_construction_payload: String,
    /// Zero-based string order within the payload.
    pub ordinal: u32,
    /// Exact printable value.
    pub value: String,
    /// Payload-relative offset of the `66 1b 03` marker.
    pub payload_offset: u64,
    /// Absolute source offset of the marker.
    pub source_offset: u64,
}

/// One resolved reference within a surface-construction branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceBranchReference {
    /// Zero-based member order, or the declared count minus one for the terminal.
    pub ordinal: u32,
    /// Serialized object index.
    pub object_index: u32,
    /// Exact serialized variable-width object-index token.
    pub raw_object_index: Vec<u8>,
    /// Unique target in the native `data_blocks` arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_block: Option<String>,
    /// Absolute file offset of the width marker.
    pub source_offset: u64,
}

/// One exact counted branch in a bounded surface-feature payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSurfaceConstructionBranch {
    /// Globally unique branch identity.
    pub id: String,
    /// Owning `SKIN` or `Studio Surface` operation label.
    pub operation_label: String,
    /// Zero-based branch order.
    pub ordinal: u32,
    /// Serialized construction family byte following `a0 5a`.
    pub family: u8,
    /// Serialized branch-group header code.
    pub header_code: u8,
    /// Serialized `16` or `40` branch mode.
    pub mode: u8,
    /// Count including the terminal reference.
    pub declared_count: u8,
    /// Whether the payload repeats the declared count before its zero lane.
    pub witnessed: bool,
    /// Ordered nonterminal references.
    pub members: Vec<FeatureSurfaceBranchReference>,
    /// Terminal reference.
    pub terminal: FeatureSurfaceBranchReference,
    /// Opaque bytes separating the terminal from the next branch or terminator.
    pub suffix: Vec<u8>,
    /// Absolute file offset of the branch mode byte.
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
    /// Exact serialized payload object-index token.
    pub raw_object_index: Vec<u8>,
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
    /// Exact shifted-binary64 encodings in scalar order.
    pub raw_scalars: [[u8; 8]; 2],
    /// Absolute file offset of the first shifted-IEEE scalar.
    pub source_offset: u64,
}

/// Exact terminal discriminator lane from a bounded extrusion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureExtrudePayloadFooter {
    /// Globally unique footer identity.
    pub id: String,
    /// Owning `EXTRUDE` operation label.
    pub operation_label: String,
    /// Two compact type indices following the footer prelude.
    pub type_indices: [u32; 2],
    /// Exact compact-index tokens for the two type indices.
    pub raw_type_indices: [Vec<u8>; 2],
    /// Absolute file offsets of the two type-index tokens.
    pub type_index_source_offsets: [u64; 2],
    /// Two values in the counted footer lane.
    pub mode_indices: [u32; 2],
    /// Four serialized one-byte flags.
    pub flags: [u8; 4],
    /// Compact values preceding the payload terminator.
    pub trailing_indices: Vec<u32>,
    /// Exact compact-index tokens in the trailing lane.
    pub raw_trailing_indices: Vec<Vec<u8>>,
    /// Absolute file offsets of the trailing compact-index tokens.
    pub trailing_index_source_offsets: Vec<u64>,
    /// Absolute file offset of the footer prelude.
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
    /// Exact serialized scalar atoms in value order.
    pub raw_values: [Vec<u8>; 3],
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
    /// Exact compact-index token.
    pub raw_member_index: Vec<u8>,
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
    /// Exact serialized compact-index token.
    pub raw_operand_object_index: Vec<u8>,
    /// Segment body bindings naming the same body image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segment_body_bindings: Vec<String>,
    /// Absolute file offset of the compact-index marker.
    pub source_offset: u64,
}

impl FeatureOperationBodyOperand {
    pub(crate) fn source_property_key(&self) -> String {
        format!(
            "operation_body_operand.{}.{}",
            self.body_reference_ordinal, self.ordinal
        )
    }
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
    /// Exact compact-index token in the continuation lane.
    pub raw_continuation_index: Vec<u8>,
    /// Absolute file offset of the continuation compact-index marker.
    pub continuation_source_offset: u64,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    pub raw_terminal_object_index: Vec<u8>,
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
    /// Exact encoded index tokens in lane order.
    pub raw_object_indices: Vec<Vec<u8>>,
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
    /// Exact shifted-binary64 scalar encoding.
    pub raw_scalar: [u8; 8],
    /// Ordered fixed-width big-endian atoms in the first counted lane.
    pub atoms_be: Vec<u32>,
    /// Absolute source offsets of the fixed-width atoms in lane order.
    pub atom_source_offsets: Vec<u64>,
    /// Compact indices wrapped by the fixed-width atoms.
    pub atom_indices: Vec<u32>,
    /// Unique offset-only data blocks addressed by the atom indices.
    pub atom_data_blocks: Vec<Option<String>>,
    /// Ordered values in the first compact-index lane.
    pub first_indices: Vec<u32>,
    /// Exact compact-index tokens in the first lane.
    pub raw_first_indices: Vec<Vec<u8>>,
    /// Absolute source offsets of the first-lane tokens.
    pub first_index_source_offsets: Vec<u64>,
    /// Unique offset-only data blocks addressed by the first lane.
    pub first_data_blocks: Vec<Option<String>>,
    /// Ordered values in the second compact-index lane.
    pub second_indices: Vec<u32>,
    /// Exact compact-index tokens in the second lane.
    pub raw_second_indices: Vec<Vec<u8>>,
    /// Absolute source offsets of the second-lane tokens.
    pub second_index_source_offsets: Vec<u64>,
    /// Unique offset-only data blocks addressed by the second lane.
    pub second_data_blocks: Vec<Option<String>>,
    /// Object index in the terminal field.
    pub terminal_object_index: u32,
    /// Exact serialized terminal object-index token.
    pub raw_terminal_object_index: Vec<u8>,
    /// Absolute file offset of the terminal object-index token.
    pub terminal_source_offset: u64,
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
    /// Exact serialized payload object-index token.
    pub raw_object_index: Vec<u8>,
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

/// One complete shifted-binary64 field in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadScalar {
    /// Globally unique scalar-field identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the field.
    pub construction_payload: String,
    /// Zero-based field order in the reconstructed payload.
    pub ordinal: u32,
    /// Serialized field discriminator following `PYf`.
    pub field_code: u8,
    /// Exact finite shifted-binary64 value.
    pub value: f64,
    /// Exact shifted-binary64 encoding.
    pub raw_value: [u8; 8],
    /// Payload-relative `PYf` marker offset.
    pub payload_offset: u64,
    /// Absolute source offset of the `PYf` marker.
    pub source_offset: u64,
}

/// One complete compact-code name field in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadName {
    /// Globally unique name-field identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the field.
    pub construction_payload: String,
    /// Zero-based name order in the reconstructed payload.
    pub ordinal: u32,
    /// Non-null compact type code, absent for the payload-leading form.
    pub type_code: Option<u32>,
    /// Exact compact type-code token, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type_code: Option<Vec<u8>>,
    /// Payload-relative offset of the compact type-code token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code_payload_offset: Option<u64>,
    /// Absolute source offset of the compact type-code token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_code_source_offset: Option<u64>,
    /// Whether the field uses the type-free payload-leading form.
    pub payload_leading: bool,
    /// Exact printable field value.
    pub value: String,
    /// Payload-relative opening `66` or payload-leading `03` marker offset.
    pub payload_offset: u64,
    /// Absolute source offset of the opening marker.
    pub source_offset: u64,
}

/// Name-delimited interval in a reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadNamedRecord {
    /// Globally unique interval identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Reconstructed payload containing the interval.
    pub construction_payload: String,
    /// Name field opening the interval.
    pub name_field: String,
    /// Complete scalar fields in payload order within the interval.
    pub scalar_fields: Vec<String>,
    /// Inclusive payload-relative start.
    pub payload_start_offset: u64,
    /// Exclusive payload-relative end.
    pub payload_end_offset: u64,
}

/// Exactly two-scalar `Point<positive decimal>` record in a `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadPoint {
    /// Globally unique typed-point identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Name-delimited payload interval carrying the point.
    pub named_record: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Ordered scalar fields carrying the two coordinates.
    pub scalar_fields: [String; 2],
    /// Ordered finite native coordinate values.
    pub coordinates: [f64; 2],
}

/// Exact same-name point identity within one reconstructed `BLOCK` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureBlockPayloadPointGroup {
    /// Globally unique point-group identity.
    pub id: String,
    /// Owning `BLOCK` operation label.
    pub operation_label: String,
    /// Exact `Point<positive decimal>` source name.
    pub name: String,
    /// Identical point records in payload order.
    pub points: Vec<String>,
    /// Bit-identical ordered coordinate values.
    pub coordinates: [f64; 2],
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
    /// Exact serialized compact object-index token.
    pub raw_object_id: Vec<u8>,
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
    /// Exact serialized target object-index token.
    pub raw_target_object_index: Vec<u8>,
    /// Absolute file offset of the target object-index token.
    pub target_source_offset: u64,
    /// Ordered object indices of the tool bodies.
    pub tool_object_indices: Vec<u32>,
    /// Exact serialized tool object-index tokens in tool order.
    pub raw_tool_object_indices: Vec<Vec<u8>>,
    /// Absolute file offsets of the tool object-index tokens in tool order.
    pub tool_source_offsets: Vec<u64>,
    /// Absolute file offset of the operation label tag.
    pub source_offset: u64,
}

fn feature_history_sections(container: &Container) -> Vec<(usize, SegmentOmLink)> {
    canonical_feature_history_links(segment_om_links(container))
        .into_iter()
        .enumerate()
        .collect()
}

fn visit_feature_history_operation_records(
    container: &Container,
    mut visit: impl FnMut(&str, u64, usize, crate::om::OperationRecord<'_>),
) {
    let sections = container.om_sections();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            visit(&section_key, entry_offset, operation_ordinal, record);
        }
    }
}

pub(crate) fn canonical_feature_history_links(
    links: impl IntoIterator<Item = SegmentOmLink>,
) -> Vec<SegmentOmLink> {
    let mut links = links
        .into_iter()
        .filter(|link| link.schema_role == OmSchemaRole::FeatureHistory)
        .collect::<Vec<_>>();
    links.sort_by(|first, second| {
        first
            .section_offset
            .cmp(&second.section_offset)
            .then_with(|| first.source_offset.cmp(&second.source_offset))
            .then_with(|| first.id.cmp(&second.id))
    });
    links.dedup_by_key(|link| link.section_offset);
    links
}

/// Decode ordered operation labels from feature-history record areas.
pub fn feature_operation_labels(container: &Container) -> Vec<FeatureOperationLabel> {
    let sections = container.om_sections();
    let mut labels = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let Some(record_area) = section.record_area else {
            continue;
        };
        let Some(record_area_offset) = section.record_area_offset else {
            continue;
        };
        labels.extend(
            section
                .operation_labels()
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, label)| {
                    let raw_object_indices: [Option<Vec<u8>>; 4] = std::array::from_fn(|slot| {
                        let start = label.object_index_offsets[slot] - record_area_offset;
                        let end = if slot + 1 < label.object_index_offsets.len() {
                            label.object_index_offsets[slot + 1] - record_area_offset
                        } else {
                            label.offset - record_area_offset
                        };
                        record_area.get(start..end).map(<[u8]>::to_vec)
                    });
                    let raw_object_indices = raw_object_indices
                        .into_iter()
                        .collect::<Option<Vec<_>>>()?
                        .try_into()
                        .ok()?;
                    Some(FeatureOperationLabel {
                        id: format!(
                            "nx:feature-history:operation-label#{section_key}-{ordinal:010}"
                        ),
                        section_link: link.id.clone(),
                        ordinal: ordinal as u32,
                        value: label.value.to_string(),
                        object_indices: label.object_indices,
                        raw_object_indices,
                        source_offset: entry_offset + label.offset as u64,
                    })
                }),
        );
    }
    labels
}

/// Decode ordered Boolean target/tool bindings from feature-history sections.
pub fn feature_boolean_operations(container: &Container) -> Vec<FeatureBooleanOperation> {
    let sections = container.om_sections();
    let mut operations = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
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
                format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}");
            operations.push(FeatureBooleanOperation {
                id: format!("nx:feature-history:boolean#{section_key}-{ordinal:010}"),
                operation_label,
                kind,
                target_object_index: operation.target,
                raw_target_object_index: operation.raw_target,
                target_source_offset: entry_offset + operation.target_offset as u64,
                tool_object_indices: operation.tools,
                raw_tool_object_indices: operation.raw_tools,
                tool_source_offsets: operation
                    .tool_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let labels = section.operation_labels();
        records.extend(
            section
                .operation_records()
                .into_iter()
                .filter_map(|record| {
                    let ordinal = labels
                        .iter()
                        .position(|label| label.offset == record.label.offset)?;
                    let operation_label =
                        format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}");
                    Some(FeatureOperationRecord {
                        id: format!(
                            "nx:feature-history:operation-record#{section_key}-{ordinal:010}"
                        ),
                        operation_label,
                        ordinal: ordinal as u32,
                        byte_len: record.bytes.len() as u64,
                        sha256: cadmpeg_ir::hash::sha256_hex(record.bytes),
                        payload_byte_len: record.payload.len() as u64,
                        payload_sha256: cadmpeg_ir::hash::sha256_hex(record.payload),
                        payload_source_offset: entry_offset + record.payload_offset as u64,
                        source_offset: entry_offset + record.offset as u64,
                    })
                }),
        );
    }
    records
}

/// Decode ordered self-framed strings from feature-operation payloads.
pub fn feature_payload_strings(container: &Container) -> Vec<FeaturePayloadString> {
    let sections = container.om_sections();
    let mut strings = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let operation_record = format!(
                "nx:feature-history:operation-record#{section_key}-{operation_ordinal:010}"
            );
            strings.extend(
                crate::om::operation_payload_strings(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, value)| FeaturePayloadString {
                        id: format!(
                            "nx:feature-history:payload-string#{section_key}-{operation_ordinal:010}-{ordinal:010}"
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
    let mut templates_by_operation = BTreeMap::<String, Vec<_>>::new();
    for string in strings {
        let Some(record) = records_by_id.get(string.operation_record.as_str()) else {
            continue;
        };
        let Some(label) = labels_by_id.get(record.operation_label.as_str()) else {
            continue;
        };
        if label.value != "SIMPLE HOLE" || !string.value.starts_with("Hole_") {
            continue;
        }
        templates_by_operation
            .entry(label.id.clone())
            .or_default()
            .push((string, *label));
    }
    templates_by_operation
        .into_values()
        .filter_map(|candidates| {
            let [(string, label)] = candidates.as_slice() else {
                return None;
            };
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

/// Decode exact nonempty duplicated scalar lanes from simple-hole operations.
pub fn feature_simple_hole_repeated_scalar_lanes(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarLane> {
    let sections = container.om_sections();
    let mut pairs = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(pair) = crate::om::simple_hole_repeated_scalar_lane(record) else {
                continue;
            };
            pairs.push(FeatureSimpleHoleRepeatedScalarLane {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                values: pair.values,
                raw_values: pair.raw_values,
                first_witness_offsets: pair.witness_offsets[0]
                    .iter()
                    .map(|offset| entry_offset + *offset as u64)
                    .collect(),
                second_witness_offsets: pair.witness_offsets[1]
                    .iter()
                    .map(|offset| entry_offset + *offset as u64)
                    .collect(),
            });
        }
    }
    pairs
}

/// Resolve the tagged block-index pairs following both repeated scalar-lane
/// witnesses through the unique offset store that owns the operation inputs.
pub fn feature_simple_hole_repeated_scalar_lane_block_references(
    container: &Container,
) -> Vec<FeatureSimpleHoleRepeatedScalarLaneBlockReferences> {
    let sections = container.om_sections();
    let inputs = feature_input_blocks(container);
    let blocks = data_blocks(container)
        .into_iter()
        .map(|block| block.id)
        .collect::<BTreeSet<_>>();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
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
                crate::om::simple_hole_repeated_scalar_lane_block_references(record)
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
            references.push(FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id: format!(
                    "nx:feature-history:simple-hole-repeated-scalar-lane-block-references#{section_key}-{operation_ordinal:010}"
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

/// Group distinct simple-hole operations that address the same four construction blocks.
pub fn feature_simple_hole_construction_groups(
    lanes: &[FeatureSimpleHoleRepeatedScalarLane],
    references: &[FeatureSimpleHoleRepeatedScalarLaneBlockReferences],
) -> Vec<FeatureSimpleHoleConstructionGroup> {
    let mut lanes_by_operation = BTreeMap::<&str, Vec<_>>::new();
    for lane in lanes {
        lanes_by_operation
            .entry(lane.operation_label.as_str())
            .or_default()
            .push(lane);
    }
    let mut grouped = BTreeMap::<([String; 2], [String; 2]), Vec<_>>::new();
    let mut ambiguous_groups = BTreeSet::new();
    for reference in references {
        let key = (
            reference.first_data_blocks.clone(),
            reference.second_data_blocks.clone(),
        );
        let lane = match lanes_by_operation
            .get(reference.operation_label.as_str())
            .map(Vec::as_slice)
        {
            Some([lane]) => *lane,
            Some(_) => {
                ambiguous_groups.insert(key);
                continue;
            }
            None => continue,
        };
        grouped.entry(key).or_default().push((reference, lane));
    }
    grouped
        .into_iter()
        .filter_map(|(key, mut members)| {
            if ambiguous_groups.contains(&key) {
                return None;
            }
            members.sort_by(|(first, _), (second, _)| {
                first.operation_label.cmp(&second.operation_label)
            });
            if members.len() < 2
                || members
                    .windows(2)
                    .any(|pair| pair[0].0.operation_label == pair[1].0.operation_label)
            {
                return None;
            }
            let first = members[0].0;
            let key = first
                .operation_label
                .rsplit_once('#')
                .map_or("unknown", |(_, key)| key);
            Some(FeatureSimpleHoleConstructionGroup {
                id: format!("nx:feature-history:simple-hole-construction-group#{key}"),
                first_data_blocks: first.first_data_blocks.clone(),
                second_data_blocks: first.second_data_blocks.clone(),
                operation_labels: members
                    .iter()
                    .map(|(reference, _)| reference.operation_label.clone())
                    .collect(),
                scalar_lanes: members.iter().map(|(_, lane)| lane.id.clone()).collect(),
                block_references: members
                    .iter()
                    .map(|(reference, _)| reference.id.clone())
                    .collect(),
            })
        })
        .collect()
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (ordinal, reference) in section.operation_body_references() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{ordinal:010}");
            references.push(FeatureBodyReference {
                id: format!("nx:feature-history:body-reference#{section_key}-{ordinal:010}"),
                operation_label,
                body_object_index: reference.object_index,
                raw_body_object_index: reference.raw_object_index,
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            references.extend(
                crate::om::operation_body_references(record)
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| FeatureBodyReferenceOccurrence {
                        id: format!(
                            "nx:feature-history:body-reference-occurrence#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                        ),
                        operation_label: operation_label.clone(),
                        ordinal: ordinal as u32,
                        body_object_index: reference.object_index,
                        raw_body_object_index: reference.raw_object_index,
                        source_offset: entry_offset + reference.offset as u64,
                    }),
            );
        }
    }
    references
}

/// Join primary feature body fields to exactly one segment body alias pair.
pub fn feature_body_segment_uses(
    references: &[FeatureBodyReference],
    bindings: &[SegmentBodyBinding],
) -> Vec<FeatureBodySegmentUse> {
    references
        .iter()
        .filter_map(|reference| {
            let matches = bindings
                .iter()
                .filter(|binding| {
                    binding.body_object_index == reference.body_object_index
                        || binding.body_alias_object_index == reference.body_object_index
                })
                .collect::<Vec<_>>();
            let [binding] = matches.as_slice() else {
                return None;
            };
            Some(FeatureBodySegmentUse {
                id: reference
                    .id
                    .replacen("body-reference", "body-segment-use", 1),
                feature_body_reference: reference.id.clone(),
                segment_body_binding: binding.id.clone(),
            })
        })
        .collect()
}

/// Resolve operation-header object indices to unique offset-only data blocks.
pub fn feature_input_blocks(container: &Container) -> Vec<FeatureInputBlock> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut inputs = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, label) in section.operation_labels().into_iter().enumerate() {
            for (input_slot, object_index) in label.object_indices.into_iter().enumerate() {
                let Some(object_index) = object_index else {
                    continue;
                };
                let Some(data_block) = unique_offset_data_block(&indexed, object_index) else {
                    continue;
                };
                let Some(record_area_offset) = section.record_area_offset else {
                    continue;
                };
                let token_offset = label.object_index_offsets[input_slot];
                let token_end = label
                    .object_index_offsets
                    .get(input_slot + 1)
                    .copied()
                    .unwrap_or(label.offset);
                let Some(raw_object_index) = section.record_area.and_then(|record_area| {
                    record_area
                        .get(
                            token_offset.checked_sub(record_area_offset)?
                                ..token_end.checked_sub(record_area_offset)?,
                        )
                        .map(<[u8]>::to_vec)
                }) else {
                    continue;
                };
                let operation_label = format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                );
                inputs.push(FeatureInputBlock {
                    id: format!(
                        "nx:feature-history:input-block#{section_key}-{operation_ordinal:010}-{input_slot:010}"
                    ),
                    operation_label,
                    input_slot: input_slot as u8,
                    object_index,
                    raw_object_index,
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
                id: format!("nx:feature-history:input-block-identity-group#{ordinal:010}"),
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

/// Join feature inputs to every column-row slot addressing the same block.
pub fn feature_input_column_row_uses(
    inputs: &[FeatureInputBlock],
    index_rows: &[DataBlockIndexRow],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
    tables: &[DataBlockColumnIndexTable],
) -> Vec<FeatureInputColumnRowUse> {
    let mut table_by_row = BTreeMap::<&str, Option<&str>>::new();
    for table in tables {
        for row in std::iter::once(table.opening_linked_row.as_str())
            .chain(table.target_rows.iter().map(String::as_str))
            .chain(table.linked_rows.iter().map(String::as_str))
        {
            table_by_row
                .entry(row)
                .and_modify(|value| *value = None)
                .or_insert(Some(table.id.as_str()));
        }
    }
    let mut slots_by_block = BTreeMap::<&str, Vec<(&str, ColumnIndexRowKind, usize, u64)>>::new();
    for row in index_rows {
        for (slot, data_block) in row.data_blocks.iter().enumerate() {
            slots_by_block.entry(data_block).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::Index,
                slot,
                row.index_source_offsets[slot],
            ));
        }
    }
    for row in linked_rows {
        for (slot, data_block) in row.data_blocks.iter().enumerate() {
            slots_by_block.entry(data_block).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::LinkedIndex,
                slot,
                if slot == 0 {
                    row.target_index_source_offset
                } else {
                    row.index_source_offsets[slot - 1]
                },
            ));
        }
    }
    for row in target_rows {
        for (slot, data_block) in row.data_blocks.iter().enumerate() {
            slots_by_block.entry(data_block).or_default().push((
                row.id.as_str(),
                ColumnIndexRowKind::TargetIndex,
                slot,
                if slot == 0 {
                    row.target_index_source_offset
                } else {
                    row.index_source_offsets[slot - 1]
                },
            ));
        }
    }
    inputs
        .iter()
        .flat_map(|input| {
            slots_by_block
                .get(input.data_block.as_str())
                .into_iter()
                .flatten()
                .enumerate()
                .map(
                    |(ordinal, (row, row_kind, slot, source_offset))| FeatureInputColumnRowUse {
                        id: format!(
                            "nx:feature-history:input-column-row-use#{}-{}-{ordinal:010}",
                            input.id.rsplit_once('#').map_or("unknown", |(_, key)| key),
                            row_kind.id_component(),
                        ),
                        input_block: input.id.clone(),
                        operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                        row_kind: *row_kind,
                        column_row: (*row).to_string(),
                        column_table: table_by_row
                            .get(row)
                            .and_then(|table| *table)
                            .map(str::to_string),
                        row_slot: *slot as u8,
                        data_block: input.data_block.clone(),
                        source_offset: *source_offset,
                    },
                )
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Retain inputs having exactly one slot-zero use in one complete column table.
pub fn feature_input_column_targets(
    inputs: &[FeatureInputBlock],
    uses: &[FeatureInputColumnRowUse],
    linked_rows: &[DataBlockLinkedIndexRow],
    target_rows: &[DataBlockTargetIndexRow],
) -> Vec<FeatureInputColumnTarget> {
    inputs
        .iter()
        .filter_map(|input| {
            let targets = uses
                .iter()
                .filter(|use_| {
                    use_.input_block == input.id
                        && use_.row_slot == 0
                        && use_.column_table.is_some()
                        && use_.row_kind != ColumnIndexRowKind::Index
                })
                .collect::<Vec<_>>();
            let [target] = targets.as_slice() else {
                return None;
            };
            let (
                leading_index,
                leading_index_source_offset,
                discriminator,
                field_indices,
                field_data_blocks,
                field_source_offsets,
                flag,
                mode,
            ) = match target.row_kind {
                ColumnIndexRowKind::LinkedIndex => {
                    let rows = linked_rows
                        .iter()
                        .filter(|row| row.id == target.column_row)
                        .collect::<Vec<_>>();
                    let [row] = rows.as_slice() else {
                        return None;
                    };
                    (
                        Some(row.first_index),
                        Some(row.first_index_source_offset),
                        Some(row.discriminator),
                        row.indices,
                        std::array::from_fn(|index| row.data_blocks[index + 1].clone()),
                        row.index_source_offsets,
                        Some(row.flag),
                        row.mode,
                    )
                }
                ColumnIndexRowKind::TargetIndex => {
                    let rows = target_rows
                        .iter()
                        .filter(|row| row.id == target.column_row)
                        .collect::<Vec<_>>();
                    let [row] = rows.as_slice() else {
                        return None;
                    };
                    (
                        None,
                        None,
                        None,
                        row.indices,
                        std::array::from_fn(|index| row.data_blocks[index + 1].clone()),
                        row.index_source_offsets,
                        None,
                        row.mode,
                    )
                }
                ColumnIndexRowKind::Index => return None,
            };
            Some(FeatureInputColumnTarget {
                id: format!(
                    "nx:feature-history:input-column-target#{}",
                    input.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
                ),
                input_block: input.id.clone(),
                operation_label: input.operation_label.clone(),
                input_slot: input.input_slot,
                column_row: target.column_row.clone(),
                row_kind: target.row_kind,
                leading_index,
                leading_index_source_offset,
                discriminator,
                field_indices,
                field_data_blocks,
                field_source_offsets,
                flag,
                mode,
                column_table: target
                    .column_table
                    .clone()
                    .expect("complete target use has a table"),
                data_block: input.data_block.clone(),
                source_offset: target.source_offset,
            })
        })
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(field) = crate::om::datum_csys_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
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
                        reference.raw_object_index,
                        data_block,
                        entry_offset + reference.offset as u64,
                    )
                })
            });
            let Some(resolved) = resolved.into_iter().collect::<Option<Vec<_>>>() else {
                continue;
            };
            if resolved.iter().any(|(_, _, data_block, _)| {
                data_block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != input_prefix)
            }) {
                continue;
            }
            constructions.push(FeatureDatumCsysConstruction {
                id: format!(
                    "nx:feature-history:datum-csys-construction#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                control: field.control,
                object_indices: resolved
                    .iter()
                    .map(|(object_index, _, _, _)| *object_index)
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                raw_object_indices: resolved
                    .iter()
                    .map(|(_, raw_object_index, _, _)| raw_object_index.clone())
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                data_blocks: resolved
                    .iter()
                    .map(|(_, _, data_block, _)| data_block.clone())
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("eight decoded references"),
                source_offsets: resolved
                    .iter()
                    .map(|(_, _, _, source_offset)| *source_offset)
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
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
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
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
                    "nx:feature-history:datum-plane-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label,
                control: header.control,
                declared_count: header.declared_count,
                branch_tag: header.branch_tag,
                descriptor_indices,
                raw_descriptor_indices: single
                    .iter()
                    .map(|branch| branch.raw_descriptor_index.clone())
                    .collect(),
                object_indices,
                raw_object_indices: single
                    .iter()
                    .map(|branch| branch.raw_object_index.clone())
                    .chain(double.iter().flat_map(|branch| {
                        branch
                            .references
                            .iter()
                            .map(|reference| reference.raw_object_index.clone())
                    }))
                    .collect(),
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
                index_lane_raw_indices: lane.map_or_else(Vec::new, |lane| lane.raw_indices.clone()),
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

/// Shared body for the datum payload-frame extractors. Reconstruct each
/// payload's concatenated bytes, build the payload-relative-to-source-offset
/// mapper once, scan the bytes, and let each family build its record, dropping
/// frames whose offsets fall outside a source block. The four extractors below
/// differ only in their payload block lane, their scanner, and their record.
fn datum_payload_frames<P, S, R>(
    container: &Container,
    payloads: &[P],
    data_blocks: impl Fn(&P) -> &[String],
    scan: impl Fn(&[u8]) -> Vec<S>,
    build: impl Fn(&P, usize, S, &dyn Fn(usize) -> Option<u64>) -> Option<R>,
) -> Vec<R> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(data_blocks(payload), &blocks)
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
            scan(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, row)| build(payload, ordinal, row, &source_offset))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalar_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadScalarPair> {
    datum_payload_frames(
        container,
        payloads,
        |payload| &payload.data_blocks[..],
        crate::om::object_payload_scalar_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeatureDatumCsysPayloadScalarPair {
                id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                datum_csys_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                values: pair.values,
                raw_values: pair.raw_values,
                payload_offset: pair.offset as u64,
                value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                source_offset: source_offset(pair.offset)?,
                value_source_offsets: [
                    source_offset(pair.value_offsets[0])?,
                    source_offset(pair.value_offsets[1])?,
                ],
                discriminator: pair.discriminator,
            })
        },
    )
}

/// Decode complete signed Q1.55 pair frames from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_fixed_pairs(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadFixedPair> {
    datum_payload_frames(
        container,
        payloads,
        |payload| &payload.data_blocks[..],
        crate::om::datum_csys_payload_fixed_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeatureDatumCsysPayloadFixedPair {
                id: format!("{}-fixed-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                datum_csys_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                values: pair.values,
                raw_values: pair.raw_values,
                discriminator: pair.discriminator,
                payload_offset: pair.offset as u64,
                value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                source_offset: source_offset(pair.offset)?,
                value_source_offsets: [
                    source_offset(pair.value_offsets[0])?,
                    source_offset(pair.value_offsets[1])?,
                ],
            })
        },
    )
}

/// Decode complete shifted-binary64 fields from reconstructed datum-CSYS payloads.
pub fn feature_datum_csys_payload_scalars(
    container: &Container,
    payloads: &[FeatureDatumCsysPayload],
) -> Vec<FeatureDatumCsysPayloadScalar> {
    datum_payload_frames(
        container,
        payloads,
        |payload| &payload.data_blocks[..],
        crate::om::construction_payload_scalar_fields,
        |payload, ordinal, scalar, source_offset| {
            Some(FeatureDatumCsysPayloadScalar {
                id: format!("{}-scalar-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                datum_csys_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                field_code: scalar.field_code,
                value: scalar.value,
                raw_value: scalar.raw_value,
                payload_offset: scalar.offset as u64,
                source_offset: source_offset(scalar.offset)?,
            })
        },
    )
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
    datum_payload_frames(
        container,
        payloads,
        |payload| payload.data_blocks.as_slice(),
        crate::om::datum_plane_object_scalar_pairs,
        |payload, ordinal, pair, source_offset| {
            Some(FeatureDatumPlanePayloadScalarPair {
                id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                operation_label: payload.operation_label.clone(),
                datum_plane_payload: payload.id.clone(),
                ordinal: ordinal as u32,
                values: pair.values,
                raw_values: pair.raw_values,
                payload_offset: pair.offset as u64,
                value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                source_offset: source_offset(pair.offset)?,
                value_source_offsets: [
                    source_offset(pair.value_offsets[0])?,
                    source_offset(pair.value_offsets[1])?,
                ],
            })
        },
    )
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
                        id: format!("{}-descriptor-{ordinal:010}", header.id),
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
            let mut operation_records = records
                .iter()
                .filter(|record| record.operation_label == label.id);
            let record = operation_records.next()?;
            if operation_records.next().is_some() {
                return None;
            }
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
                        id: format!("{}-coordinate-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        raw_values: pair.raw_values,
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

/// Decode exact signed fixed-point pair frames from reconstructed sketch payloads.
pub fn feature_sketch_payload_fixed_pairs(
    container: &Container,
    payloads: &[FeatureSketchConstructionPayload],
) -> Vec<FeatureSketchPayloadFixedPair> {
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
            crate::om::sketch_payload_fixed_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureSketchPayloadFixedPair {
                        id: format!("{}-fixed-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        raw_values: pair.raw_values,
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

pub(crate) fn offset_data_block_bytes_for_section<'a>(
    section_ordinal: usize,
    entry_offset: u64,
    control: Option<&crate::om::EntityRecord<'a>>,
    records: &[crate::om::EntityRecord<'a>],
) -> BTreeMap<String, (&'a [u8], u64)> {
    let mut blocks = BTreeMap::new();
    let first_record_ordinal = usize::from(control.is_some());
    if let Some(control) = control {
        blocks.insert(
            format!("nx:om-data-blocks-{section_ordinal}:block#0"),
            (control.bytes, entry_offset + control.offset as u64),
        );
    }
    for (record_ordinal, block) in records.iter().enumerate() {
        blocks.insert(
            format!(
                "nx:om-data-blocks-{section_ordinal}:block#{}",
                record_ordinal + first_record_ordinal
            ),
            (block.bytes, entry_offset + block.offset as u64),
        );
    }
    blocks
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
        blocks.extend(offset_data_block_bytes_for_section(
            section_ordinal,
            entry_offset,
            section.control.as_ref(),
            &section.records,
        ));
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
                crate::om::construction_payload_scalar_fields(&payload)
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
                                "nx:feature-history:sketch-payload-scalar#{}-{ordinal:010}",
                                construction_payload
                                    .rsplit_once('#')
                                    .map_or("unknown", |(_, key)| key)
                            ),
                            operation_label: construction.operation_label.clone(),
                            construction_payload: construction_payload.clone(),
                            ordinal: ordinal as u32,
                            field_code: field.field_code,
                            value: field.value,
                            raw_value: field.raw_value,
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
            crate::om::construction_payload_named_fields(&payload)
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
                            "nx:feature-history:sketch-payload-name#{}-{ordinal:010}",
                            construction_payload
                                .rsplit_once('#')
                                .map_or("unknown", |(_, key)| key)
                        ),
                        operation_label: construction.operation_label.clone(),
                        construction_payload: construction_payload.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code,
                        raw_type_code: field.raw_type_code,
                        type_code_payload_offset: field
                            .type_code_offset
                            .map(|offset| offset as u64),
                        type_code_source_offset: field.type_code_offset.and_then(|offset| {
                            joined_payload_source_offset(
                                offset as u64,
                                &block_payload_offsets,
                                &block_byte_lengths,
                                &block_source_offsets,
                            )
                        }),
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
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
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
            let mut record_fixed_pairs = fixed_pairs
                .iter()
                .filter(|pair| {
                    pair.construction_payload == payload.id
                        && pair.payload_offset > name.payload_offset
                        && pair.payload_offset < end
                })
                .collect::<Vec<_>>();
            record_fixed_pairs.sort_by_key(|pair| pair.payload_offset);
            records.push(FeatureSketchPayloadNamedRecord {
                id: format!(
                    "nx:feature-history:sketch-payload-record#{}-{ordinal:010}",
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
                fixed_pairs: record_fixed_pairs
                    .into_iter()
                    .map(|pair| pair.id.clone())
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

/// Decode `Point<positive decimal>` records containing exactly one fixed pair.
pub fn feature_sketch_fixed_points(
    records: &[FeatureSketchPayloadNamedRecord],
    names: &[FeatureSketchPayloadName],
    fixed_pairs: &[FeatureSketchPayloadFixedPair],
) -> Vec<FeatureSketchFixedPoint> {
    let names = names
        .iter()
        .map(|name| (name.id.as_str(), name))
        .collect::<BTreeMap<_, _>>();
    let fixed_pairs = fixed_pairs
        .iter()
        .map(|pair| (pair.id.as_str(), pair))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter_map(|record| {
            if !record.scalar_fields.is_empty() {
                return None;
            }
            let [fixed_pair_id] = record.fixed_pairs.as_slice() else {
                return None;
            };
            let name = names.get(record.name_field.as_str())?;
            parse_sketch_point_name(&name.value)?;
            let pair = fixed_pairs.get(fixed_pair_id.as_str())?;
            Some(FeatureSketchFixedPoint {
                id: record
                    .id
                    .replacen("sketch-payload-record", "sketch-fixed-point", 1),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
                fixed_pair: pair.id.clone(),
                values: pair.values,
                source_offset: pair.source_offset,
            })
        })
        .collect()
}

/// Group every bit-identical same-name sketch-point witness.
pub fn feature_sketch_point_groups(points: &[FeatureSketchPoint]) -> Vec<FeatureSketchPointGroup> {
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if !grouped.insert(key) {
            continue;
        }
        let witnesses = points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .collect::<Vec<_>>();
        if witnesses.iter().any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }
        groups.push(FeatureSketchPointGroup {
            id: format!(
                "nx:feature-history:sketch-point-group#{}",
                point.id.rsplit_once('#').map_or("unknown", |(_, key)| key)
            ),
            operation_label: point.operation_label.clone(),
            name: point.name.clone(),
            points: witnesses
                .into_iter()
                .map(|point| point.id.clone())
                .collect(),
            coordinates: point.coordinates,
        });
    }
    groups
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
                raw_values: point.raw_values,
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

/// Join one named point to a complete sketch lane through unique consecutive block adjacency.
pub fn feature_sketch_preceding_named_point_uses(
    references: &[FeatureSketchReference],
    points: &[OffsetStoreNamedPoint],
) -> Vec<FeatureSketchPrecedingNamedPointUse> {
    fn block_key(block: &str) -> Option<(&str, u32)> {
        let (store, ordinal) = block.rsplit_once(":block#")?;
        Some((store, ordinal.parse().ok()?))
    }

    let mut references_by_operation = BTreeMap::<&str, Vec<&FeatureSketchReference>>::new();
    for reference in references {
        references_by_operation
            .entry(reference.operation_label.as_str())
            .or_default()
            .push(reference);
    }
    let mut uses = Vec::new();
    for (operation_label, mut operation_references) in references_by_operation {
        operation_references.sort_by_key(|reference| reference.ordinal);
        let complete_lane = !operation_references.is_empty()
            && operation_references
                .iter()
                .enumerate()
                .all(|(ordinal, reference)| {
                    reference.ordinal == ordinal as u32
                        && usize::from(reference.declared_count) == operation_references.len()
                        && reference.data_block.is_some()
                        && reference.terminal == (ordinal + 1 == operation_references.len())
                });
        if !complete_lane {
            continue;
        }
        let first_reference = operation_references[0];
        let first_block = first_reference
            .data_block
            .as_deref()
            .expect("complete lane has resolved references");
        let Some((first_store, first_ordinal)) = block_key(first_block) else {
            continue;
        };
        let candidates = points
            .iter()
            .filter(|point| {
                let Some(last_block) = point.data_blocks.last() else {
                    return false;
                };
                let Some((point_store, point_ordinal)) = block_key(last_block) else {
                    return false;
                };
                point_store == first_store && point_ordinal.checked_add(1) == Some(first_ordinal)
            })
            .collect::<Vec<_>>();
        let [point] = candidates.as_slice() else {
            continue;
        };
        let operation_key = operation_label
            .rsplit_once('#')
            .map_or(operation_label, |(_, key)| key);
        let point_key = point
            .id
            .rsplit_once('#')
            .map_or(point.id.as_str(), |(_, key)| key);
        uses.push(FeatureSketchPrecedingNamedPointUse {
            id: format!(
                "nx:feature-history:sketch-preceding-named-point-use#{operation_key}-{point_key}"
            ),
            operation_label: operation_label.to_string(),
            first_sketch_reference: first_reference.id.clone(),
            named_point: point.id.clone(),
            point_data_blocks: point.data_blocks.clone(),
            following_data_block: first_block.to_string(),
            source_offset: first_reference.source_offset,
        });
    }
    uses
}

/// Join the two exact encodings of a solved sketch point.
pub fn feature_sketch_point_uses(
    point_groups: &[FeatureSketchPointGroup],
    named_points: &[OffsetStoreNamedPoint],
    block_uses: &[FeatureSketchNamedPointBlockUse],
) -> Vec<FeatureSketchPointUse> {
    let named_points = named_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    let mut uses = Vec::new();
    let mut joined = BTreeSet::new();
    for block_use in block_uses {
        let key = (
            block_use.operation_label.as_str(),
            block_use.named_point.as_str(),
        );
        if !joined.insert(key) {
            continue;
        }
        let Some(named_point) = named_points.get(block_use.named_point.as_str()) else {
            continue;
        };
        let mut point_block_uses = block_uses
            .iter()
            .filter(|candidate| {
                candidate.operation_label == block_use.operation_label
                    && candidate.named_point == block_use.named_point
            })
            .collect::<Vec<_>>();
        point_block_uses.sort_by_key(|block_use| {
            (
                block_use.reference_ordinal,
                block_use.source_offset,
                block_use.id.as_str(),
            )
        });
        let candidates = point_groups
            .iter()
            .filter(|group| {
                group.operation_label == block_use.operation_label && group.name == named_point.name
            })
            .collect::<Vec<_>>();
        let [point_group] = candidates.as_slice() else {
            continue;
        };
        if point_group
            .coordinates
            .iter()
            .zip(named_point.values)
            .any(|(first, second)| first.to_bits() != second.to_bits())
        {
            continue;
        }
        uses.push(FeatureSketchPointUse {
            id: point_block_uses[0].id.replacen(
                "sketch-named-point-block-use",
                "sketch-point-use",
                1,
            ),
            operation_label: block_use.operation_label.clone(),
            sketch_references: point_block_uses
                .iter()
                .map(|block_use| block_use.sketch_reference.clone())
                .collect(),
            block_uses: point_block_uses
                .iter()
                .map(|block_use| block_use.id.clone())
                .collect(),
            sketch_point_group: point_group.id.clone(),
            named_point: named_point.id.clone(),
            source_offsets: point_block_uses
                .iter()
                .map(|block_use| block_use.source_offset)
                .collect(),
        });
    }
    uses
}

/// Join one uniquely sketch-owned named-point block to a later datum-CSYS construction.
pub fn feature_sketch_datum_csys_dependencies(
    labels: &[FeatureOperationLabel],
    named_points: &[OffsetStoreNamedPoint],
    point_uses: &[FeatureSketchPointUse],
    constructions: &[FeatureDatumCsysConstruction],
) -> Vec<FeatureSketchDatumCsysDependency> {
    fn block_key(block: &str) -> Option<(&str, u32)> {
        let (store, ordinal) = block.rsplit_once(":block#")?;
        Some((store, ordinal.parse().ok()?))
    }

    let positions = labels
        .iter()
        .enumerate()
        .map(|(position, label)| (label.id.as_str(), position))
        .collect::<BTreeMap<_, _>>();
    let points = named_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = Vec::new();
    for construction in constructions {
        let Some(consumer_position) = positions.get(construction.operation_label.as_str()) else {
            continue;
        };
        let mut candidates = Vec::new();
        for point_use in point_uses {
            let Some(producer_position) = positions.get(point_use.operation_label.as_str()) else {
                continue;
            };
            if producer_position >= consumer_position {
                continue;
            }
            let Some(point) = points.get(point_use.named_point.as_str()) else {
                continue;
            };
            for shared_block in construction
                .data_blocks
                .iter()
                .filter(|block| point.data_blocks.contains(block))
            {
                candidates.push((
                    point_use,
                    FeatureSketchDatumCsysBlockRelation::Shared {
                        data_block: shared_block.clone(),
                    },
                ));
            }
            let Some(point_last_block) = point.data_blocks.last() else {
                continue;
            };
            let Some(construction_first_block) = construction.data_blocks.first() else {
                continue;
            };
            if let (
                Some((point_store, point_ordinal)),
                Some((construction_store, construction_ordinal)),
            ) = (
                block_key(point_last_block),
                block_key(construction_first_block),
            ) {
                if point_store == construction_store
                    && point_ordinal.checked_add(1) == Some(construction_ordinal)
                {
                    candidates.push((
                        point_use,
                        FeatureSketchDatumCsysBlockRelation::Consecutive {
                            point_data_block: point_last_block.clone(),
                            construction_data_block: construction_first_block.clone(),
                        },
                    ));
                }
            }
        }
        candidates.sort_by(|(left_use, left_relation), (right_use, right_relation)| {
            (left_use.id.as_str(), left_relation).cmp(&(right_use.id.as_str(), right_relation))
        });
        candidates.dedup_by(|(left_use, left_relation), (right_use, right_relation)| {
            left_use.id == right_use.id && left_relation == right_relation
        });
        let [(point_use, block_relation)] = candidates.as_slice() else {
            continue;
        };
        dependencies.push(FeatureSketchDatumCsysDependency {
            id: construction.id.replacen(
                "datum-csys-construction",
                "sketch-datum-csys-dependency",
                1,
            ),
            sketch_operation_label: point_use.operation_label.clone(),
            datum_csys_operation_label: construction.operation_label.clone(),
            sketch_point_use: point_use.id.clone(),
            datum_csys_construction: construction.id.clone(),
            block_relation: block_relation.clone(),
            source_offset: point_use.source_offsets[0],
        });
    }
    dependencies.sort_by(|left, right| left.id.cmp(&right.id));
    dependencies
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(decoded) = crate::om::sketch_payload_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let declared_count = decoded.declared_count;
            let terminal_ordinal = decoded.references.len() - 1;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                let data_block = unique_offset_data_block(&indexed, reference.object_index);
                FeatureSketchReference {
                    id: format!(
                        "nx:feature-history:sketch-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    declared_count,
                    terminal: ordinal == terminal_ordinal,
                    object_index: reference.object_index,
                    raw_object_index: reference.raw_object_index,
                    data_block,
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

struct ResolvedFeaturePayloadReference {
    section_key: String,
    operation_ordinal: usize,
    ordinal: usize,
    object_index: u32,
    raw_object_index: Vec<u8>,
    data_block: Option<String>,
    source_offset: u64,
}

fn resolved_feature_payload_references(
    container: &Container,
    decode: impl Fn(crate::om::OperationRecord<'_>) -> Option<Vec<crate::om::PayloadObjectReference>>,
) -> Vec<ResolvedFeaturePayloadReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(decoded) = decode(record) else {
                continue;
            };
            references.extend(decoded.into_iter().enumerate().map(|(ordinal, reference)| {
                ResolvedFeaturePayloadReference {
                    section_key: section_key.clone(),
                    operation_ordinal,
                    ordinal,
                    object_index: reference.object_index,
                    raw_object_index: reference.raw_object_index,
                    data_block: unique_offset_data_block(&indexed, reference.object_index),
                    source_offset: entry_offset + reference.offset as u64,
                }
            }));
        }
    }
    references
}

/// Decode and resolve the exact ordered construction-reference field in
/// projected-curve payloads without assigning semantic roles to its slots.
pub fn feature_projected_curve_references(
    container: &Container,
) -> Vec<FeatureProjectedCurveReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::projected_curve_payload_references(record).map(|field| field.references)
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureProjectedCurveReference {
            id: format!(
                "nx:feature-history:projected-curve-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            raw_object_index: reference.raw_object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Reconstruct ordered logical payloads from projected-curve reference fields.
pub fn feature_projected_curve_construction_payloads(
    container: &Container,
    labels: &[FeatureOperationLabel],
    references: &[FeatureProjectedCurveReference],
) -> Vec<FeatureProjectedCurveConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    let kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|operation_label| {
            let operation_kind = *kinds.get(operation_label)?;
            let expected_len = match operation_kind {
                "CPROJ" => 3,
                "CPROJ_CMB" => 8,
                _ => return None,
            };
            let mut field = references
                .iter()
                .filter(|reference| reference.operation_label == operation_label)
                .collect::<Vec<_>>();
            field.sort_by_key(|reference| reference.ordinal);
            if field.len() != expected_len
                || field
                    .iter()
                    .enumerate()
                    .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let data_blocks = field
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect::<Option<Vec<_>>>()?;
            let store = data_blocks.first()?.rsplit_once(":block#")?.0;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (bytes, starts, lengths, sources) = join_data_block_bytes(&data_blocks, &blocks)?;
            let (_, operation_key) = operation_label.rsplit_once('#')?;
            Some(FeatureProjectedCurveConstructionPayload {
                id: format!(
                    "nx:feature-history:projected-curve-construction-payload#{operation_key}"
                ),
                operation_label: operation_label.to_string(),
                operation_kind: operation_kind.to_string(),
                construction_references: field
                    .iter()
                    .map(|reference| reference.id.clone())
                    .collect(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets: starts,
                block_byte_lengths: lengths,
                block_source_offsets: sources,
            })
        })
        .collect()
}

/// Decode canonical printable strings from reconstructed projected-curve payloads.
pub fn feature_projected_curve_construction_strings(
    container: &Container,
    payloads: &[FeatureProjectedCurveConstructionPayload],
) -> Vec<FeatureProjectedCurveConstructionString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::string_values(&bytes, 0)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeatureProjectedCurveConstructionString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.to_string(),
                        payload_offset,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode and resolve exact ordered construction references in pattern
/// payloads without assigning seed or transform semantics to their slots.
pub fn feature_pattern_references(container: &Container) -> Vec<FeaturePatternReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::pattern_payload_references(record).map(|field| field.references)
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeaturePatternReference {
            id: format!(
                "nx:feature-history:pattern-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            raw_object_index: reference.raw_object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Reconstruct ordered logical payloads from complete pattern-reference graphs.
pub fn feature_pattern_construction_payloads(
    container: &Container,
    labels: &[FeatureOperationLabel],
    references: &[FeaturePatternReference],
) -> Vec<FeaturePatternConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    let kinds = labels
        .iter()
        .map(|label| (label.id.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|operation_label| {
            let operation_kind = *kinds.get(operation_label)?;
            if !matches!(operation_kind, "Pattern Feature" | "Pattern Geometry") {
                return None;
            }
            let mut graph = references
                .iter()
                .filter(|reference| reference.operation_label == operation_label)
                .collect::<Vec<_>>();
            graph.sort_by_key(|reference| reference.ordinal);
            if !matches!(graph.len(), 9 | 10)
                || graph
                    .iter()
                    .enumerate()
                    .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let data_blocks = graph
                .iter()
                .map(|reference| reference.data_block.clone())
                .collect::<Option<Vec<_>>>()?;
            let store = data_blocks.first()?.rsplit_once(":block#")?.0;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (bytes, starts, lengths, sources) = join_data_block_bytes(&data_blocks, &blocks)?;
            let (_, operation_key) = operation_label.rsplit_once('#')?;
            Some(FeaturePatternConstructionPayload {
                id: format!("nx:feature-history:pattern-construction-payload#{operation_key}"),
                operation_label: operation_label.to_string(),
                operation_kind: operation_kind.to_string(),
                construction_references: graph
                    .iter()
                    .map(|reference| reference.id.clone())
                    .collect(),
                data_blocks,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets: starts,
                block_byte_lengths: lengths,
                block_source_offsets: sources,
            })
        })
        .collect()
}

/// Decode canonical printable strings from reconstructed pattern payloads.
pub fn feature_pattern_construction_strings(
    container: &Container,
    payloads: &[FeaturePatternConstructionPayload],
) -> Vec<FeaturePatternConstructionString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::string_values(&bytes, 0)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeaturePatternConstructionString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.to_string(),
                        payload_offset,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode complete signed Q1.55 lanes from reconstructed pattern payloads.
pub fn feature_pattern_construction_fixed_lanes(
    container: &Container,
    payloads: &[FeaturePatternConstructionPayload],
) -> Vec<FeaturePatternConstructionFixedLane> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_fixed_lanes(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, lane)| {
                    let payload_offset = lane.offset as u64;
                    let value_payload_offsets = lane
                        .value_offsets
                        .into_iter()
                        .map(|offset| offset as u64)
                        .collect::<Vec<_>>();
                    let value_source_offsets = value_payload_offsets
                        .iter()
                        .map(|offset| {
                            joined_payload_source_offset(*offset, &starts, &lengths, &sources)
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(FeaturePatternConstructionFixedLane {
                        id: format!("{}-fixed-lane-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: lane.values,
                        markers: lane.markers,
                        raw_values: lane.raw_values,
                        payload_offset,
                        value_payload_offsets,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                        value_source_offsets,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact counted transform lanes from bounded pattern payloads.
pub fn feature_pattern_transform_lanes(container: &Container) -> Vec<FeaturePatternTransformLane> {
    let sections = container.om_sections();
    let mut lanes = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(lane) = crate::om::pattern_payload_transform_lane(record) else {
                continue;
            };
            lanes.push(FeaturePatternTransformLane {
                id: format!(
                    "nx:feature-history:pattern-transform-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                declared_count: lane.declared_count,
                encoding: match lane.encoding {
                    crate::om::PatternTransformEncoding::Binary32 => {
                        FeaturePatternTransformEncoding::Binary32
                    }
                    crate::om::PatternTransformEncoding::Binary64 => {
                        FeaturePatternTransformEncoding::Binary64
                    }
                },
                values: lane.values,
                raw_values: lane.raw_values,
                selectors: lane.selectors,
                raw_selectors: lane.raw_selectors,
                source_offset: entry_offset + lane.offset as u64,
                value_source_offsets: lane
                    .value_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
                selector_source_offsets: lane
                    .selector_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
            });
        }
    }
    lanes
}

/// Decode exact point-feature construction headers without assigning coordinate semantics.
pub fn feature_point_construction_headers(
    container: &Container,
) -> Vec<FeaturePointConstructionHeader> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut headers = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(header) = crate::om::point_feature_payload_header(record) else {
                continue;
            };
            headers.push(FeaturePointConstructionHeader {
                id: format!(
                    "nx:feature-history:point-construction-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                object_index: header.reference.object_index,
                raw_object_index: header.reference.raw_object_index,
                data_block: unique_offset_data_block(&indexed, header.reference.object_index),
                mode: header.mode,
                source_offset: entry_offset + header.reference.offset as u64,
            });
        }
    }
    headers
}

/// Decode exact scalar lanes selected by uniquely resolved point-feature headers.
pub fn feature_point_construction_scalar_lanes(
    container: &Container,
    headers: &[FeaturePointConstructionHeader],
) -> Vec<FeaturePointConstructionScalarLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    for header in headers {
        let Some(expected_target) = header.data_block.as_deref() else {
            continue;
        };
        let Ok(target_ordinal) = usize::try_from(header.object_index) else {
            continue;
        };
        let candidates = indexed
            .iter()
            .enumerate()
            .filter_map(|(section_ordinal, (entry, section))| {
                if section.records.first()?.object_id.is_some() || target_ordinal < 2 {
                    return None;
                }
                let target_id =
                    format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}");
                if target_id != expected_target {
                    return None;
                }
                let preceding = section.records.get(target_ordinal - 2)?;
                let target = section.records.get(target_ordinal - 1)?;
                let lane = crate::om::point_feature_scalar_lane(preceding.bytes, target.bytes)?;
                Some((section_ordinal, *entry, preceding, target, lane))
            })
            .collect::<Vec<_>>();
        let [(section_ordinal, entry, preceding, target, lane)] = candidates.as_slice() else {
            continue;
        };
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        let source_offsets = lane.value_offsets.map(|offset| {
            if offset < preceding.bytes.len() {
                entry_offset + preceding.offset as u64 + offset as u64
            } else {
                entry_offset + target.offset as u64 + (offset - preceding.bytes.len()) as u64
            }
        });
        lanes.push(FeaturePointConstructionScalarLane {
            id: header.id.replacen(
                "point-construction-header#",
                "point-construction-scalar-lane#",
                1,
            ),
            operation_label: header.operation_label.clone(),
            construction_header: header.id.clone(),
            data_blocks: [
                format!(
                    "nx:om-data-blocks-{section_ordinal}:block#{}",
                    target_ordinal - 1
                ),
                format!("nx:om-data-blocks-{section_ordinal}:block#{target_ordinal}"),
            ],
            values: lane.values,
            raw_values: lane.raw_values,
            source_offsets,
        });
    }
    lanes
}

/// Decode exact ordered draft construction references without assigning semantic roles.
pub fn feature_draft_construction_references(
    container: &Container,
) -> Vec<FeatureDraftConstructionReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::draft_feature_payload_references(record)
            .map(|field| field.references.into_iter().collect())
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureDraftConstructionReference {
            id: format!(
                "nx:feature-history:draft-construction-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            raw_object_index: reference.raw_object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Decode exact counted compact-index lanes preceding draft construction graphs.
pub fn feature_draft_construction_index_lanes(
    container: &Container,
) -> Vec<FeatureDraftConstructionIndexLane> {
    let indexed = container.indexed_om_sections();
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::draft_feature_leading_index_lane(record) else {
                return;
            };
            let indices = lane
                .indices
                .iter()
                .map(|(value, _)| *value)
                .collect::<Vec<_>>();
            let data_blocks =
                crate::om::draft_feature_payload_references(record).and_then(|graph| {
                    let mut complete_indices = graph
                        .references
                        .iter()
                        .map(|reference| reference.object_index)
                        .collect::<Vec<_>>();
                    complete_indices.extend(&indices);
                    let section_ordinal = unique_offset_data_store(&indexed, &complete_indices)?;
                    Some(
                        indices
                            .iter()
                            .map(|index| {
                                format!("nx:om-data-blocks-{section_ordinal}:block#{index}")
                            })
                            .collect(),
                    )
                });
            lanes.push(FeatureDraftConstructionIndexLane {
                id: format!(
                    "nx:feature-history:draft-construction-index-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                declared_count: lane.declared_count,
                indices,
                raw_indices: lane.raw_indices,
                data_blocks,
                source_offsets: lane
                    .indices
                    .iter()
                    .map(|(_, offset)| entry_offset + *offset as u64)
                    .collect(),
            });
        },
    );
    lanes
}

/// Reconstruct ordered logical payloads from resolved draft index lanes.
pub fn feature_draft_construction_payloads(
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
) -> Vec<FeatureDraftConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    lanes
        .iter()
        .filter_map(|lane| {
            let data_blocks = lane.data_blocks.clone()?;
            let (bytes, block_payload_offsets, block_byte_lengths, block_source_offsets) =
                join_data_block_bytes(&data_blocks, &blocks)?;
            Some(FeatureDraftConstructionPayload {
                id: lane.id.replacen(
                    "draft-construction-index-lane#",
                    "draft-construction-payload#",
                    1,
                ),
                operation_label: lane.operation_label.clone(),
                index_lane: lane.id.clone(),
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

/// Reconstruct ordered logical payloads from complete draft construction graphs.
pub fn feature_draft_construction_graph_payloads(
    container: &Container,
    lanes: &[FeatureDraftConstructionIndexLane],
    references: &[FeatureDraftConstructionReference],
) -> Vec<FeatureDraftConstructionGraphPayload> {
    let blocks = offset_data_block_bytes(container);
    lanes
        .iter()
        .filter_map(|lane| {
            let lane_blocks = lane.data_blocks.as_ref()?;
            let store = lane_blocks.first()?.rsplit_once(":block#")?.0;
            let mut graph = references
                .iter()
                .filter(|reference| reference.operation_label == lane.operation_label)
                .collect::<Vec<_>>();
            graph.sort_by_key(|reference| reference.ordinal);
            if graph
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let graph: [&FeatureDraftConstructionReference; 4] = graph.try_into().ok()?;
            let data_blocks = graph
                .each_ref()
                .map(|reference| reference.data_block.clone())
                .into_iter()
                .collect::<Option<Vec<_>>>()?;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (bytes, starts, lengths, sources) = join_data_block_bytes(&data_blocks, &blocks)?;
            let (_, key) = lane.id.rsplit_once('#')?;
            Some(FeatureDraftConstructionGraphPayload {
                id: format!("nx:feature-history:draft-construction-graph-payload#{key}"),
                operation_label: lane.operation_label.clone(),
                index_lane: lane.id.clone(),
                construction_references: graph.each_ref().map(|reference| reference.id.clone()),
                data_blocks: data_blocks.try_into().ok()?,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets: starts.try_into().ok()?,
                block_byte_lengths: lengths.try_into().ok()?,
                block_source_offsets: sources.try_into().ok()?,
            })
        })
        .collect()
}

/// Decode complete signed Q1.55 lanes from reconstructed draft graph payloads.
pub fn feature_draft_construction_fixed_lanes(
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Vec<FeatureDraftConstructionFixedLane> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_fixed_lanes(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, lane)| {
                    let payload_offset = lane.offset as u64;
                    let value_payload_offsets = lane
                        .value_offsets
                        .into_iter()
                        .map(|offset| offset as u64)
                        .collect::<Vec<_>>();
                    let value_source_offsets = value_payload_offsets
                        .iter()
                        .map(|offset| {
                            joined_payload_source_offset(*offset, &starts, &lengths, &sources)
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(FeatureDraftConstructionFixedLane {
                        id: format!("{}-fixed-lane-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        graph_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: lane.values,
                        markers: lane.markers,
                        raw_values: lane.raw_values,
                        payload_offset,
                        value_payload_offsets,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                        value_source_offsets,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete shifted-binary32 lanes from reconstructed draft graph payloads.
pub fn feature_draft_construction_binary32_lanes(
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Vec<FeatureDraftConstructionBinary32Lane> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_binary32_lanes(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, lane)| {
                    let payload_offset = lane.offset as u64;
                    let value_payload_offsets = lane
                        .value_offsets
                        .into_iter()
                        .map(|offset| offset as u64)
                        .collect::<Vec<_>>();
                    let value_source_offsets = value_payload_offsets
                        .iter()
                        .map(|offset| {
                            joined_payload_source_offset(*offset, &starts, &lengths, &sources)
                        })
                        .collect::<Option<Vec<_>>>()?;
                    Some(FeatureDraftConstructionBinary32Lane {
                        id: format!("{}-binary32-lane-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        graph_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        discriminator: lane.discriminator,
                        branch: lane.branch,
                        values: lane.values,
                        raw_values: lane.raw_values,
                        payload_offset,
                        value_payload_offsets,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                        value_source_offsets,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode canonical printable strings from reconstructed draft graph payloads.
pub fn feature_draft_construction_graph_strings(
    container: &Container,
    payloads: &[FeatureDraftConstructionGraphPayload],
) -> Vec<FeatureDraftConstructionGraphString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::string_values(&bytes, 0)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeatureDraftConstructionGraphString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        graph_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.to_string(),
                        payload_offset,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete identity frames from reconstructed draft construction payloads.
pub fn feature_draft_construction_identity_frames(
    container: &Container,
    payloads: &[FeatureDraftConstructionPayload],
) -> Vec<FeatureDraftConstructionIdentityFrame> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::draft_construction_identity_frames(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, frame)| {
                    let payload_offset = frame.offset as u64;
                    let identity_payload_offset = frame.identity_offset as u64;
                    let form = match frame.form {
                        crate::om::DraftConstructionIdentityFrameForm::IndexedBranch {
                            first_index,
                            second_index,
                            branch,
                        } => FeatureDraftConstructionIdentityFrameForm::IndexedBranch {
                            first_index,
                            second_index,
                            branch,
                        },
                        crate::om::DraftConstructionIdentityFrameForm::Tagged { index } => {
                            FeatureDraftConstructionIdentityFrameForm::Tagged { index }
                        }
                    };
                    Some(FeatureDraftConstructionIdentityFrame {
                        id: format!("{}-identity-frame-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        draft_construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        prefix: frame.prefix,
                        form,
                        identity: frame.identity,
                        payload_offset,
                        identity_payload_offset,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                        identity_source_offset: joined_payload_source_offset(
                            identity_payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode complete end-anchored terminal lanes from draft construction payloads.
pub fn feature_draft_construction_terminal_lanes(
    container: &Container,
) -> Vec<FeatureDraftConstructionTerminalLane> {
    let mut lanes = Vec::new();
    visit_feature_history_operation_records(
        container,
        |section_key, entry_offset, operation_ordinal, record| {
            let Some(lane) = crate::om::draft_feature_terminal_lane(record) else {
                return;
            };
            lanes.push(FeatureDraftConstructionTerminalLane {
                id: format!(
                    "nx:feature-history:draft-construction-terminal-lane#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                indices: lane.indices,
                raw_indices: lane.raw_indices,
                tail: lane.tail,
                index_source_offsets: lane
                    .index_offsets
                    .map(|offset| entry_offset + offset as u64),
                source_offset: entry_offset + lane.offset as u64,
            });
        },
    );
    lanes
}

/// Decode and resolve the exact common reference envelope in surface-feature
/// payloads without assigning section or guide semantics to its slots.
pub fn feature_surface_construction_references(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionReference> {
    resolved_feature_payload_references(container, |record| {
        crate::om::surface_feature_payload_references(record)
            .map(|field| field.references.into_iter().collect())
    })
    .into_iter()
    .map(|reference| {
        let operation_label = format!(
            "nx:feature-history:operation-label#{}-{:010}",
            reference.section_key, reference.operation_ordinal
        );
        FeatureSurfaceConstructionReference {
            id: format!(
                "nx:feature-history:surface-construction-reference#{}-{:010}-{:010}",
                reference.section_key, reference.operation_ordinal, reference.ordinal
            ),
            operation_label,
            ordinal: reference.ordinal as u32,
            object_index: reference.object_index,
            raw_object_index: reference.raw_object_index,
            data_block: reference.data_block,
            source_offset: reference.source_offset,
        }
    })
    .collect()
}

/// Reconstruct ordered logical payloads from complete surface-construction graphs.
pub fn feature_surface_construction_payloads(
    container: &Container,
    references: &[FeatureSurfaceConstructionReference],
) -> Vec<FeatureSurfaceConstructionPayload> {
    let blocks = offset_data_block_bytes(container);
    references
        .iter()
        .map(|reference| reference.operation_label.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|operation_label| {
            let mut graph = references
                .iter()
                .filter(|reference| reference.operation_label == operation_label)
                .collect::<Vec<_>>();
            graph.sort_by_key(|reference| reference.ordinal);
            if graph
                .iter()
                .enumerate()
                .any(|(ordinal, reference)| reference.ordinal != ordinal as u32)
            {
                return None;
            }
            let graph: [&FeatureSurfaceConstructionReference; 14] = graph.try_into().ok()?;
            let data_blocks = graph
                .each_ref()
                .map(|reference| reference.data_block.clone())
                .into_iter()
                .collect::<Option<Vec<_>>>()?;
            let store = data_blocks.first()?.rsplit_once(":block#")?.0;
            if data_blocks.iter().any(|block| {
                block
                    .rsplit_once(":block#")
                    .is_none_or(|(prefix, _)| prefix != store)
            }) {
                return None;
            }
            let (bytes, starts, lengths, sources) = join_data_block_bytes(&data_blocks, &blocks)?;
            let (_, operation_key) = operation_label.rsplit_once('#')?;
            Some(FeatureSurfaceConstructionPayload {
                id: format!("nx:feature-history:surface-construction-payload#{operation_key}"),
                operation_label: operation_label.to_string(),
                construction_references: graph.each_ref().map(|reference| reference.id.clone()),
                data_blocks: data_blocks.try_into().ok()?,
                byte_len: bytes.len() as u64,
                sha256: cadmpeg_ir::hash::sha256_hex(&bytes),
                block_payload_offsets: starts.try_into().ok()?,
                block_byte_lengths: lengths.try_into().ok()?,
                block_source_offsets: sources.try_into().ok()?,
            })
        })
        .collect()
}

/// Decode exact scalar-pair frames from reconstructed surface payloads.
pub fn feature_surface_construction_scalar_pairs(
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Vec<FeatureSurfaceConstructionScalarPair> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::object_payload_scalar_pairs(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, pair)| {
                    Some(FeatureSurfaceConstructionScalarPair {
                        id: format!("{}-scalar-pair-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        surface_construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        values: pair.values,
                        raw_values: pair.raw_values,
                        payload_offset: pair.offset as u64,
                        value_payload_offsets: pair.value_offsets.map(|offset| offset as u64),
                        source_offset: joined_payload_source_offset(
                            pair.offset as u64,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                        value_source_offsets: [
                            joined_payload_source_offset(
                                pair.value_offsets[0] as u64,
                                &starts,
                                &lengths,
                                &sources,
                            )?,
                            joined_payload_source_offset(
                                pair.value_offsets[1] as u64,
                                &starts,
                                &lengths,
                                &sources,
                            )?,
                        ],
                        discriminator: pair.discriminator,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode exact printable string frames from reconstructed surface payloads.
pub fn feature_surface_construction_strings(
    container: &Container,
    payloads: &[FeatureSurfaceConstructionPayload],
) -> Vec<FeatureSurfaceConstructionString> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::surface_payload_strings(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, value)| {
                    let payload_offset = value.offset as u64;
                    Some(FeatureSurfaceConstructionString {
                        id: format!("{}-string-{ordinal:010}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        surface_construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        value: value.value.to_string(),
                        payload_offset,
                        source_offset: joined_payload_source_offset(
                            payload_offset,
                            &starts,
                            &lengths,
                            &sources,
                        )?,
                    })
                })
                .collect()
        })
        .collect()
}

/// Decode and resolve exact counted surface-construction branches without
/// assigning section or guide semantics to their members.
pub fn feature_surface_construction_branches(
    container: &Container,
) -> Vec<FeatureSurfaceConstructionBranch> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut branches = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(group) = crate::om::surface_feature_payload_branches(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            branches.extend(group.branches.into_iter().enumerate().map(|(ordinal, branch)| {
                let resolve = |ordinal: usize, reference: crate::om::PayloadObjectReference| {
                    FeatureSurfaceBranchReference {
                        ordinal: ordinal as u32,
                        object_index: reference.object_index,
                        raw_object_index: reference.raw_object_index,
                        data_block: unique_offset_data_block(&indexed, reference.object_index),
                        source_offset: entry_offset + reference.offset as u64,
                    }
                };
                let members = branch
                    .members
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, reference)| resolve(ordinal, reference))
                    .collect::<Vec<_>>();
                let terminal = resolve(members.len(), branch.terminal);
                FeatureSurfaceConstructionBranch {
                    id: format!(
                        "nx:feature-history:surface-construction-branch#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    family: group.family,
                    header_code: group.header_code,
                    mode: branch.mode,
                    declared_count: branch.declared_count,
                    witnessed: branch.witnessed,
                    members,
                    terminal,
                    suffix: branch.suffix,
                    source_offset: entry_offset + branch.offset as u64,
                }
            }));
        }
    }
    branches
}

/// Decode and resolve the witnessed ordered profile list in extrusion payloads.
pub fn feature_extrude_profile_references(
    container: &Container,
) -> Vec<FeatureExtrudeProfileReference> {
    let indexed = container.indexed_om_sections();
    let sections = container.om_sections();
    let mut references = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(decoded) = crate::om::extrude_profile_references(record) else {
                continue;
            };
            let operation_label =
                format!("nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}");
            let witnessed = decoded.witnessed;
            references.extend(decoded.references.into_iter().enumerate().map(|(ordinal, reference)| {
                FeatureExtrudeProfileReference {
                    id: format!(
                        "nx:feature-history:extrude-profile-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: operation_label.clone(),
                    ordinal: ordinal as u32,
                    witnessed,
                    object_index: reference.object_index,
                    raw_object_index: reference.raw_object_index,
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(header) = crate::om::extrude_payload_header(record) else {
                continue;
            };
            headers.push(FeatureExtrudePayloadHeader {
                id: format!(
                    "nx:feature-history:extrude-payload-header#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                scalars: header.scalars,
                raw_scalars: header.raw_scalars,
                source_offset: entry_offset + header.offset as u64,
            });
        }
    }
    headers
}

/// Decode exact terminal discriminator lanes from bounded extrusion payloads.
pub fn feature_extrude_payload_footers(container: &Container) -> Vec<FeatureExtrudePayloadFooter> {
    let sections = container.om_sections();
    let mut footers = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(footer) = crate::om::extrude_payload_footer(record) else {
                continue;
            };
            footers.push(FeatureExtrudePayloadFooter {
                id: format!(
                    "nx:feature-history:extrude-payload-footer#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                type_indices: footer.type_indices,
                raw_type_indices: footer.raw_type_indices,
                type_index_source_offsets: footer
                    .type_index_offsets
                    .map(|offset| entry_offset + offset as u64),
                mode_indices: footer.mode_indices,
                flags: footer.flags,
                trailing_indices: footer.trailing_indices,
                raw_trailing_indices: footer.raw_trailing_indices,
                trailing_index_source_offsets: footer
                    .trailing_index_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
                source_offset: entry_offset + footer.offset as u64,
            });
        }
    }
    footers
}

/// Decode typed scalar clauses anchored to operation body-reference fields.
pub fn feature_operation_body_scalar_triples(
    container: &Container,
) -> Vec<FeatureOperationBodyScalarTriple> {
    let sections = container.om_sections();
    let mut triples = Vec::new();
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
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
                        "nx:feature-history:operation-body-scalar-triple#{section_key}-{operation_ordinal:010}-{}",
                        triple.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    body_reference_ordinal: triple.body_reference_ordinal,
                    body_object_index: triple.body_object_index,
                    branch: triple.branch,
                    values: triple.scalars.each_ref().map(|scalar| scalar.value),
                    encodings: triple
                        .scalars
                        .each_ref()
                        .map(|scalar| encoding(scalar.encoding)),
                    raw_values: triple
                        .scalars
                        .each_ref()
                        .map(|scalar| scalar.raw_value.clone()),
                    source_offsets: triple
                        .scalars
                        .each_ref()
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            members.extend(
                crate::om::operation_body_members(record)
                    .into_iter()
                    .map(|member| FeatureOperationBodyMember {
                        id: format!(
                            "nx:feature-history:operation-body-member#{section_key}-{operation_ordinal:010}-{}-{}",
                            member.body_reference_ordinal, member.ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: member.body_reference_ordinal,
                        body_object_index: member.body_object_index,
                        ordinal: member.ordinal,
                        member_index: member.member_index,
                        raw_member_index: member.raw_member_index,
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
            raw_operand_object_index: member.raw_member_index.clone(),
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            continuations.extend(
                crate::om::operation_body_11_continuations(record)
                    .into_iter()
                    .map(|continuation| FeatureOperationBody11Continuation {
                        id: format!(
                            "nx:feature-history:trim-body-11-continuation#{section_key}-{operation_ordinal:010}-{}",
                            continuation.body_reference_ordinal
                        ),
                        operation_label: format!(
                            "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                        ),
                        body_reference_ordinal: continuation.body_reference_ordinal,
                        body_object_index: continuation.body_object_index,
                        continuation_index: continuation.continuation_index,
                        raw_continuation_index: continuation.raw_continuation_index,
                        continuation_source_offset: entry_offset
                            + continuation.continuation_offset as u64,
                        terminal_object_index: continuation.terminal_object_index,
                        raw_terminal_object_index: continuation.raw_terminal_object_index,
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
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
                        "nx:feature-history:operation-body-reference-lane#{section_key}-{operation_ordinal:010}-{}",
                        lane.body_reference_ordinal
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    body_reference_ordinal: lane.body_reference_ordinal,
                    body_object_index: lane.body_object_index,
                    branch: lane.branch,
                    encoding,
                    object_indices,
                    raw_object_indices: lane
                        .values
                        .iter()
                        .map(|value| value.raw_value.clone())
                        .collect(),
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
                .enumerate()
                .any(|(ordinal, reference)| {
                    reference.ordinal != ordinal as u32 || !reference.witnessed
                })
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
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
                    "nx:feature-history:extrude-payload-32-branch#{section_key}-{operation_ordinal:010}"
                ),
                operation_label: format!(
                    "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                ),
                body_object_index: branch.body_object_index,
                scalar: branch.scalar,
                raw_scalar: branch.raw_scalar,
                atoms_be: branch.atoms_be,
                atom_source_offsets: branch
                    .atom_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
                atom_indices: branch.atom_indices,
                atom_data_blocks,
                first_indices: branch.first_indices,
                raw_first_indices: branch.raw_first_indices,
                first_index_source_offsets: branch
                    .first_index_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
                first_data_blocks,
                second_indices: branch.second_indices,
                raw_second_indices: branch.raw_second_indices,
                second_index_source_offsets: branch
                    .second_index_offsets
                    .into_iter()
                    .map(|offset| entry_offset + offset as u64)
                    .collect(),
                second_data_blocks,
                terminal_object_index: branch.terminal_object_index,
                raw_terminal_object_index: branch.raw_terminal_object_index,
                terminal_source_offset: entry_offset + branch.terminal_offset as u64,
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
    let mut branches_by_operation = BTreeMap::<&str, Vec<&FeatureExtrudePayload32Branch>>::new();
    for branch in branches {
        branches_by_operation
            .entry(branch.operation_label.as_str())
            .or_default()
            .push(branch);
    }
    let mut constructions = Vec::new();
    for (operation_label, operation_branches) in branches_by_operation {
        let [branch] = operation_branches.as_slice() else {
            continue;
        };
        let mut profile = references
            .iter()
            .filter(|reference| reference.operation_label == operation_label)
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
    for (section_ordinal, link) in feature_history_sections(container) {
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
        let section_key = format!("{section_ordinal:010}");
        let entry_offset = entry.file_span.map_or(0, |(offset, _)| offset);
        for (operation_ordinal, record) in section.operation_records_with_label_ordinals() {
            let Some(field) = crate::om::block_construction_references(record) else {
                continue;
            };
            let terminal_ordinal = field.references.len() - 1;
            references.extend(field.references.into_iter().enumerate().map(
                |(ordinal, reference)| FeatureBlockConstructionReference {
                    id: format!(
                        "nx:feature-history:block-construction-reference#{section_key}-{operation_ordinal:010}-{ordinal:010}"
                    ),
                    operation_label: format!(
                        "nx:feature-history:operation-label#{section_key}-{operation_ordinal:010}"
                    ),
                    control: field.control,
                    ordinal: ordinal as u32,
                    terminal: ordinal == terminal_ordinal,
                    object_index: reference.object_index,
                    raw_object_index: reference.raw_object_index,
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

/// Decode exact framed scalar fields across reconstructed `BLOCK` payloads.
pub fn feature_block_payload_scalars(
    container: &Container,
    payloads: &[FeatureBlockConstructionPayload],
) -> Vec<FeatureBlockPayloadScalar> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::construction_payload_scalar_fields(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined_payload_source_offset(
                        field.offset as u64,
                        &starts,
                        &lengths,
                        &sources,
                    )?;
                    Some(FeatureBlockPayloadScalar {
                        id: format!("{}-scalar-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        field_code: field.field_code,
                        value: field.value,
                        raw_value: field.raw_value,
                        payload_offset: field.offset as u64,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Decode exact compact-code name fields across reconstructed `BLOCK` payloads.
pub fn feature_block_payload_names(
    container: &Container,
    payloads: &[FeatureBlockConstructionPayload],
) -> Vec<FeatureBlockPayloadName> {
    let blocks = offset_data_block_bytes(container);
    payloads
        .iter()
        .flat_map(|payload| {
            let Some((bytes, starts, lengths, sources)) =
                join_data_block_bytes(&payload.data_blocks, &blocks)
            else {
                return Vec::new();
            };
            crate::om::construction_payload_named_fields(&bytes)
                .into_iter()
                .enumerate()
                .filter_map(|(ordinal, field)| {
                    let source_offset = joined_payload_source_offset(
                        field.offset as u64,
                        &starts,
                        &lengths,
                        &sources,
                    )?;
                    Some(FeatureBlockPayloadName {
                        id: format!("{}-name-{ordinal}", payload.id),
                        operation_label: payload.operation_label.clone(),
                        construction_payload: payload.id.clone(),
                        ordinal: ordinal as u32,
                        type_code: field.type_code,
                        raw_type_code: field.raw_type_code,
                        type_code_payload_offset: field
                            .type_code_offset
                            .map(|offset| offset as u64),
                        type_code_source_offset: field.type_code_offset.and_then(|offset| {
                            joined_payload_source_offset(offset as u64, &starts, &lengths, &sources)
                        }),
                        payload_leading: field.payload_leading,
                        value: field.value.to_string(),
                        payload_offset: field.offset as u64,
                        source_offset,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Join complete `BLOCK` payload names to scalar fields in their intervals.
pub fn feature_block_payload_named_records(
    payloads: &[FeatureBlockConstructionPayload],
    names: &[FeatureBlockPayloadName],
    scalars: &[FeatureBlockPayloadScalar],
) -> Vec<FeatureBlockPayloadNamedRecord> {
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
            records.push(FeatureBlockPayloadNamedRecord {
                id: format!("{}-record-{ordinal}", payload.id),
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

/// Type exact two-scalar `Point<positive decimal>` `BLOCK` payload intervals.
pub fn feature_block_payload_points(
    records: &[FeatureBlockPayloadNamedRecord],
    names: &[FeatureBlockPayloadName],
    scalars: &[FeatureBlockPayloadScalar],
) -> Vec<FeatureBlockPayloadPoint> {
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
            Some(FeatureBlockPayloadPoint {
                id: format!("{}-point", record.id),
                operation_label: record.operation_label.clone(),
                named_record: record.id.clone(),
                name: name.value.clone(),
                scalar_fields: [first.id.clone(), second.id.clone()],
                coordinates: [first.value, second.value],
            })
        })
        .collect()
}

/// Group every bit-identical same-name `BLOCK` construction-point witness.
pub fn feature_block_payload_point_groups(
    points: &[FeatureBlockPayloadPoint],
) -> Vec<FeatureBlockPayloadPointGroup> {
    let mut grouped = BTreeSet::new();
    let mut groups = Vec::new();
    for point in points {
        let key = (point.operation_label.as_str(), point.name.as_str());
        if !grouped.insert(key) {
            continue;
        }
        let witnesses = points
            .iter()
            .filter(|candidate| {
                candidate.operation_label == point.operation_label && candidate.name == point.name
            })
            .collect::<Vec<_>>();
        if witnesses.iter().any(|candidate| {
            candidate
                .coordinates
                .iter()
                .zip(point.coordinates)
                .any(|(first, second)| first.to_bits() != second.to_bits())
        }) {
            continue;
        }
        groups.push(FeatureBlockPayloadPointGroup {
            id: format!("{}-group", point.id),
            operation_label: point.operation_label.clone(),
            name: point.name.clone(),
            points: witnesses
                .into_iter()
                .map(|point| point.id.clone())
                .collect(),
            coordinates: point.coordinates,
        });
    }
    groups
}

fn joined_payload_source_offset(
    relative: u64,
    starts: &[u64],
    lengths: &[u64],
    sources: &[u64],
) -> Option<u64> {
    starts
        .iter()
        .zip(lengths)
        .zip(sources)
        .find_map(|((start, length), source)| {
            (relative >= *start && relative < start.saturating_add(*length))
                .then_some(source + relative - start)
        })
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
                declaration.record.split_once(":entry#").map(|pair| pair.0)
                    != run[0].record.split_once(":entry#").map(|pair| pair.0)
                    || declaration.source_entry != run[0].source_entry
                    || Some(declaration.parameter_index) != first.checked_add(ordinal as u32)
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
            if resolved[0].0.source_table.is_empty()
                || resolved
                    .iter()
                    .zip(run)
                    .any(|((expression, _), declaration)| {
                        expression.source_entry != declaration.source_entry
                            || expression.source_table != resolved[0].0.source_table
                    })
            {
                return None;
            }
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
                    id: data_block_object_frame_id(&data_block, ordinal),
                    data_block: data_block.clone(),
                    ordinal: ordinal as u32,
                    object_id: frame.object_id,
                    raw_object_id: frame.raw_object_id,
                    source_offset: source_offset + frame.offset as u64,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn data_block_object_frame_id(data_block: &str, ordinal: usize) -> String {
    format!(
        "{}-{ordinal}",
        data_block
            .replacen("nx:om-data-blocks-", "nx:om-data-block-object-frames-", 1)
            .replacen(":block#", ":block-frame#", 1)
    )
}

fn unique_offset_data_block(
    indexed: &[(&crate::container::DirEntry, crate::om::IndexedSection<'_>)],
    object_index: u32,
) -> Option<String> {
    let section_ordinal = unique_offset_data_store(indexed, &[object_index])?;
    Some(format!(
        "nx:om-data-blocks-{section_ordinal}:block#{object_index}"
    ))
}

fn unique_offset_data_store(
    indexed: &[(&crate::container::DirEntry, crate::om::IndexedSection<'_>)],
    object_indices: &[u32],
) -> Option<usize> {
    if object_indices.is_empty() || object_indices.contains(&0) {
        return None;
    }
    let candidates = indexed
        .iter()
        .enumerate()
        .filter(|(_, (_, candidate))| {
            candidate
                .records
                .first()
                .is_some_and(|record| record.object_id.is_none())
                && object_indices.iter().all(|object_index| {
                    usize::try_from(*object_index)
                        .ok()
                        .is_some_and(|ordinal| ordinal <= candidate.records.len())
                })
        })
        .map(|(section_ordinal, _)| section_ordinal)
        .collect::<Vec<_>>();
    let [section_ordinal] = candidates.as_slice() else {
        return None;
    };
    Some(*section_ordinal)
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

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use std::io::{Cursor, Write};

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use cadmpeg_ir::codec::{Codec, CodecEntry, Confidence, DecodeOptions};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry,
        ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point2, Vector3};
    use cadmpeg_ir::report::LossCategory;
    use cadmpeg_ir::Exactness;

    use crate::container;
    use crate::parasolid::{self, StreamKind};
    use crate::test_support::*;
    use crate::NxCodec;

    use super::*;

    #[test]
    fn nx_feature_source_content_orders_parameter_occurrences_with_text() {
        let text = super::FeaturePayloadString {
            id: "text".into(),
            operation_record: "record".into(),
            ordinal: 0,
            value: "Through".into(),
            source_offset: 30,
        };
        let parameter_use = super::FeatureParameterUse {
            id: "use".into(),
            operation_label: "operation".into(),
            expression: "nx:test:expression#20".into(),
            bindings: vec!["first".into(), "second".into()],
            source_offsets: vec![20, 40],
        };
        let content = crate::native::attach::feature_source_content(&[&text], &[&parameter_use]);
        assert_eq!(content.len(), 3);
        assert!(matches!(
            &content[0],
            cadmpeg_ir::features::FeatureSourceContent::Parameter(id)
                if id.0 == "nx:test:parameter#20"
        ));
        assert!(matches!(
            &content[1],
            cadmpeg_ir::features::FeatureSourceContent::Text(value) if value == "Through"
        ));
        assert!(matches!(
            &content[2],
            cadmpeg_ir::features::FeatureSourceContent::Parameter(id)
                if id.0 == "nx:test:parameter#20"
        ));
    }

    #[test]
    fn nx_block_dimensions_do_not_cross_expression_sections() {
        use super::{FeatureBlockConstruction, FeatureParameterBinding};
        use crate::native::om::{Expression, ExpressionDeclaration, ExpressionUnit};

        let operation = "nx:feature-history:operation-label#0-1";
        let construction = FeatureBlockConstruction {
            id: "nx:feature-history:block-construction#0-1".into(),
            operation_label: operation.into(),
            control: 0,
            member_references: Vec::new(),
            member_data_blocks: Vec::new(),
            terminal_reference: "terminal-reference".into(),
            terminal_data_block: "terminal-block".into(),
        };
        let binding = FeatureParameterBinding {
            id: "binding".into(),
            operation_label: operation.into(),
            input_slot: 0,
            input_block: "input".into(),
            reference_ordinal: 0,
            expression_declaration: "declaration-20".into(),
            expression: Some("expression-20".into()),
            object_id: 20,
            source_offset: 1,
        };
        let declaration = |index: u32, source_entry: &str| ExpressionDeclaration {
            id: format!("declaration-{index}"),
            object_id: index,
            record: format!("{source_entry}:entry#{index}"),
            name: format!("p{index}"),
            parameter_index: index,
            qualifier: None,
            literal: None,
            source_entry: source_entry.into(),
            source_offset: u64::from(index),
        };
        let expression = |index: u32, source_entry: &str, source_table: &str| Expression {
            id: format!("expression-{index}"),
            object_id: Some(index),
            record: Some(format!("{source_entry}:entry#{index}")),
            declaration: Some(format!("declaration-{index}")),
            name: format!("p{index}"),
            parameter_index: Some(index),
            qualifier: None,
            unit: ExpressionUnit::Millimeter,
            expression: index.to_string(),
            value: Some(f64::from(index)),
            source_entry: source_entry.into(),
            source_table: source_table.into(),
            source_offset: u64::from(index),
        };
        let mut expressions = [
            expression(20, "section-a", "table-a"),
            expression(21, "section-a", "table-a"),
            expression(22, "section-b", "table-b"),
        ];
        let mut declarations = [
            declaration(20, "section-a"),
            declaration(21, "section-a"),
            declaration(22, "section-b"),
        ];

        assert!(super::feature_block_dimensions(
            std::slice::from_ref(&construction),
            std::slice::from_ref(&binding),
            &declarations,
            &expressions,
        )
        .is_empty());

        declarations[2].source_entry = "section-a".into();
        declarations[2].record = "section-a:entry#22".into();
        assert!(super::feature_block_dimensions(
            std::slice::from_ref(&construction),
            std::slice::from_ref(&binding),
            &declarations,
            &expressions,
        )
        .is_empty());

        expressions[2].source_entry = "section-a".into();
        expressions[2].source_table = "table-a".into();
        assert_eq!(
            super::feature_block_dimensions(
                &[construction],
                &[binding],
                &declarations,
                &expressions,
            )
            .len(),
            1
        );
    }

    #[test]
    fn nx_boolean_projection_rejects_target_tool_alias_overlap() {
        use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};
        use cadmpeg_ir::ids::BodyId;
        use std::collections::BTreeMap;

        let operation = super::FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#0".to_string(),
            kind: super::FeatureBooleanKind::Subtract,
            target_object_index: 10,
            raw_target_object_index: vec![10],
            target_source_offset: 0,
            tool_object_indices: vec![20],
            raw_tool_object_indices: vec![vec![20]],
            tool_source_offsets: vec![1],
            source_offset: 0,
        };
        let body = BodyId("body#10".to_string());
        let bodies = BTreeMap::from([(10, vec![body.clone()]), (20, vec![body])]);

        assert_eq!(
            crate::native::attach::boolean_feature_definition(&operation, &bodies),
            FeatureDefinition::Combine {
                target: BodySelection::Native("nx:om-object-index#10".to_string()),
                tools: BodySelection::Native("nx:om-object-indices#20".to_string()),
                op: BooleanOp::Cut,
            }
        );

        let missing_tool = BTreeMap::from([(10, vec![BodyId("body#10".to_string())])]);
        assert!(matches!(
            crate::native::attach::boolean_feature_definition(&operation, &missing_tool),
            FeatureDefinition::Combine {
                target: BodySelection::Native(target),
                tools: BodySelection::Native(tools),
                ..
            } if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20"
        ));
    }

    #[test]
    fn nx_simple_hole_template_requires_exact_ordered_tokens() {
        use super::{
            FeatureOperationLabel, FeatureOperationRecord, FeaturePayloadString,
            SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
        };

        let label = FeatureOperationLabel {
            id: "operation#3".to_string(),
            section_link: "section#0".to_string(),
            ordinal: 3,
            value: "SIMPLE HOLE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 100,
        };
        let record = FeatureOperationRecord {
            id: "record#3".to_string(),
            operation_label: label.id.clone(),
            ordinal: 3,
            byte_len: 80,
            sha256: "a".repeat(64),
            payload_byte_len: 40,
            payload_sha256: "b".repeat(64),
            payload_source_offset: 120,
            source_offset: 90,
        };
        let string = FeaturePayloadString {
            id: "payload-string#3-0".to_string(),
            operation_record: record.id.clone(),
            ordinal: 0,
            value: "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer".to_string(),
            source_offset: 130,
        };
        let templates = super::feature_simple_hole_templates(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            std::slice::from_ref(&string),
        );
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].payload_string, string.id);
        assert_eq!(templates[0].family, SimpleHoleFamily::GeneralHole);
        assert_eq!(templates[0].form, SimpleHoleForm::Simple);
        assert_eq!(templates[0].extent, SimpleHoleExtent::Through);
        assert_eq!(
            templates[0].start_treatment,
            SimpleHoleEndTreatment::Chamfer
        );
        assert_eq!(templates[0].end_treatment, SimpleHoleEndTreatment::Chamfer);

        let mut duplicate = string.clone();
        duplicate.id = "payload-string#3-1".to_string();
        duplicate.ordinal = 1;
        duplicate.source_offset += 64;
        assert!(super::feature_simple_hole_templates(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            &[string.clone(), duplicate],
        )
        .is_empty());

        let unknown = FeaturePayloadString {
            id: "payload-string#3-1".to_string(),
            operation_record: record.id.clone(),
            ordinal: 1,
            value: "Hole_Unknown".to_string(),
            source_offset: 194,
        };
        assert!(super::feature_simple_hole_templates(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            &[string.clone(), unknown],
        )
        .is_empty());

        let mut malformed = string;
        malformed.value = "Hole_GeneralHole_Simple_Through_EndChamfer_StartChamfer".to_string();
        assert!(super::feature_simple_hole_templates(&[label], &[record], &[malformed]).is_empty());
    }

    #[test]
    fn nx_sketch_record_joins_exact_operation_and_ordered_input_lanes() {
        use super::{
            FeatureInputBlock, FeatureOperationLabel, FeatureOperationRecord,
            FeatureSketchReference,
        };

        let label = FeatureOperationLabel {
            id: "nx:feature-history:operation-label#0-7".to_string(),
            section_link: "nx:feature-history#0".to_string(),
            ordinal: 7,
            value: "SKETCH".to_string(),
            object_indices: [Some(45), None, Some(81), None],
            raw_object_indices: [vec![45], vec![0xff], vec![81], vec![0xff]],
            source_offset: 700,
        };
        let record = FeatureOperationRecord {
            id: "nx:feature-history:operation-record#0-7".to_string(),
            operation_label: label.id.clone(),
            ordinal: 7,
            byte_len: 173,
            sha256: "00".repeat(32),
            payload_byte_len: 140,
            payload_sha256: "11".repeat(32),
            payload_source_offset: 733,
            source_offset: 700,
        };
        let input = |slot, index| FeatureInputBlock {
            id: format!("nx:feature-history:input-block#0-7-{slot}"),
            operation_label: label.id.clone(),
            input_slot: slot,
            object_index: index,
            raw_object_index: vec![index as u8],
            data_block: format!("nx:om-data-blocks-2:block#{index}"),
            source_offset: 710 + u64::from(slot),
        };
        let inputs = [input(2, 81), input(0, 45)];
        let reference = |ordinal, index| FeatureSketchReference {
            id: format!("nx:feature-history:sketch-reference#0-7-{ordinal}"),
            operation_label: label.id.clone(),
            ordinal,
            declared_count: 2,
            terminal: ordinal == 1,
            object_index: index,
            raw_object_index: vec![0xf0, index as u8],
            data_block: Some(format!("nx:om-data-blocks-2:block#{index}")),
            source_offset: 740 + u64::from(ordinal),
        };
        let references = [reference(1, 97), reference(0, 96)];

        let sketches = super::feature_sketch_records(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            &inputs,
            &references,
        );
        assert_eq!(sketches.len(), 1);
        assert_eq!(sketches[0].ordinal, 7);
        assert_eq!(
            sketches[0].operation_record,
            "nx:feature-history:operation-record#0-7"
        );
        assert_eq!(
            sketches[0].input_blocks,
            [
                "nx:feature-history:input-block#0-7-0",
                "nx:feature-history:input-block#0-7-2"
            ]
        );
        assert_eq!(
            sketches[0].payload_references,
            [
                "nx:feature-history:sketch-reference#0-7-0",
                "nx:feature-history:sketch-reference#0-7-1"
            ]
        );
        let mut duplicate_record = record.clone();
        duplicate_record.id.push_str("-duplicate");
        assert!(super::feature_sketch_records(
            std::slice::from_ref(&label),
            &[record.clone(), duplicate_record],
            &inputs,
            &references,
        )
        .is_empty());
        let construction = super::feature_sketch_construction_inputs(&sketches, &references);
        assert_eq!(construction.len(), 1);
        assert_eq!(
            construction[0].member_references,
            ["nx:feature-history:sketch-reference#0-7-0"]
        );
        assert_eq!(
            construction[0].member_data_blocks,
            ["nx:om-data-blocks-2:block#96"]
        );
        assert_eq!(
            construction[0].terminal_reference,
            "nx:feature-history:sketch-reference#0-7-1"
        );
        assert_eq!(
            construction[0].terminal_data_block,
            "nx:om-data-blocks-2:block#97"
        );

        let mut malformed = references;
        malformed[0].ordinal = 2;
        assert!(super::feature_sketch_construction_inputs(&sketches, &malformed).is_empty());
    }

    #[test]
    fn nx_sketch_payload_join_preserves_order_and_cross_block_values() {
        let ids = vec!["block#2".to_string(), "block#3".to_string()];
        let blocks = std::collections::BTreeMap::from([
            ("block#2".to_string(), (&[0x30, 0x43][..], 120_u64)),
            (
                "block#3".to_string(),
                (&[0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72][..], 900_u64),
            ),
        ]);
        let joined = super::join_data_block_bytes(&ids, &blocks).expect("required invariant");
        assert_eq!(joined.0, [0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72]);
        assert_eq!(joined.1, [0, 2]);
        assert_eq!(joined.2, [2, 6]);
        assert_eq!(joined.3, [120, 900]);

        let missing = vec!["block#2".to_string(), "missing".to_string()];
        assert!(super::join_data_block_bytes(&missing, &blocks).is_none());
    }

    #[test]
    fn nx_offset_store_block_bytes_follow_catalog_identity() {
        let control = crate::om::EntityRecord {
            object_id: None,
            object_id_offset: None,
            offset: 5,
            bytes: &[0xaa],
        };
        let first = crate::om::EntityRecord {
            object_id: None,
            object_id_offset: None,
            offset: 6,
            bytes: &[0xbb],
        };
        let second = crate::om::EntityRecord {
            object_id: None,
            object_id_offset: None,
            offset: 7,
            bytes: &[0xcc],
        };
        let controlled = super::offset_data_block_bytes_for_section(
            3,
            100,
            Some(&control),
            &[first.clone(), second.clone()],
        );
        assert_eq!(
            controlled["nx:om-data-blocks-3:block#0"],
            (&[0xaa][..], 105)
        );
        assert_eq!(
            controlled["nx:om-data-blocks-3:block#1"],
            (&[0xbb][..], 106)
        );
        assert_eq!(
            controlled["nx:om-data-blocks-3:block#2"],
            (&[0xcc][..], 107)
        );

        let control_free =
            super::offset_data_block_bytes_for_section(4, 200, None, &[first, second]);
        assert_eq!(
            control_free["nx:om-data-blocks-4:block#0"],
            (&[0xbb][..], 206)
        );
        assert_eq!(
            control_free["nx:om-data-blocks-4:block#1"],
            (&[0xcc][..], 207)
        );
    }

    #[test]
    fn feature_history_links_follow_unique_physical_section_order() {
        use crate::native::om::OmSchemaRole;
        use crate::native::segments::{SegmentIndexSlot, SegmentOmLink};

        let link = |id: &str, schema_role, source_offset, section_offset| SegmentOmLink {
            id: id.to_string(),
            row: format!("row-{id}"),
            slot: SegmentIndexSlot::Value,
            schema_role,
            separator_byte_len: (section_offset - source_offset) as u32,
            source_offset,
            section_offset,
        };
        let links = super::canonical_feature_history_links([
            link("late", OmSchemaRole::FeatureHistory, 300, 300),
            link("model", OmSchemaRole::Model, 50, 50),
            link("duplicate", OmSchemaRole::FeatureHistory, 100, 100),
            link("early", OmSchemaRole::FeatureHistory, 100, 100),
        ]);

        assert_eq!(
            links
                .iter()
                .map(|link| (link.id.as_str(), link.section_offset))
                .collect::<Vec<_>>(),
            [("duplicate", 100), ("late", 300)]
        );
    }

    #[test]
    fn decode_orders_and_deduplicates_linked_feature_history_sections() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            multi_section_feature_history_payload(),
        )]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let namespace = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant");
        let links = namespace
            .arena_as::<crate::native::segments::SegmentOmLink>("segment_om_links")
            .expect("required invariant");
        assert_eq!(links.len(), 4);
        let labels = namespace
            .arena_as::<super::FeatureOperationLabel>("feature_operation_labels")
            .expect("required invariant");
        assert_eq!(
            labels
                .iter()
                .map(|label| (label.value.as_str(), label.ordinal))
                .collect::<Vec<_>>(),
            [("BLOCK", 0), ("UNITE", 0)]
        );
        assert_ne!(labels[0].section_link, labels[1].section_link);
        assert_eq!(
            labels[0].raw_object_indices,
            [
                vec![0x01],
                vec![0x82, 0x40],
                vec![0x90, 0x17, 0xd3],
                vec![0xff]
            ]
        );
        assert_eq!(labels[1].raw_object_indices, labels[0].raw_object_indices);
        let records = namespace
            .arena_as::<super::FeatureOperationRecord>("feature_operation_records")
            .expect("required invariant");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].operation_label, labels[0].id);
        assert_eq!(records[1].operation_label, labels[1].id);
        assert_eq!(
            result
                .ir
                .model
                .features
                .iter()
                .map(|feature| feature.name.as_deref())
                .collect::<Vec<_>>(),
            [Some("BLOCK"), Some("UNITE")]
        );
    }

    #[test]
    fn decoded_feature_ids_preserve_double_digit_operation_order() {
        let section = size_framed_om_section_with_repeated_operations(12);
        let mut payload = Vec::new();
        for word in [24_u32, 9, 11, 1, 1, 24] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.extend_from_slice(&section);
        let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let labels = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureOperationLabel>("feature_operation_labels")
            .expect("required invariant");

        assert_eq!(
            labels.iter().map(|label| label.ordinal).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
        assert!(labels
            .windows(2)
            .all(|pair| pair[0].id.as_str() < pair[1].id.as_str()));
    }

    #[test]
    fn decode_retains_role_scoped_om_record_area_header() {
        let file =
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let areas = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<crate::native::om::OmRecordArea>("om_record_areas")
            .expect("required invariant");
        assert_eq!(areas.len(), 1);
        assert_eq!(
            areas[0].schema_role,
            crate::native::om::OmSchemaRole::FeatureHistory
        );
        assert_eq!(areas[0].control_words, [13, 14, 44]);
        assert_eq!(areas[0].product_version, "NX 2027.3102");
        assert!(areas[0].byte_len > 12);
        assert_eq!(areas[0].sha256.len(), 64);
        let labels = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureOperationLabel>("feature_operation_labels")
            .expect("required invariant");
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].ordinal, 0);
        assert_eq!(labels[0].value, "UNITE");
        assert_eq!(
            labels[0].object_indices,
            [Some(1), Some(576), Some(6099), None]
        );
        assert_eq!(labels[0].section_link, areas[0].section_link);
        let records = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureOperationRecord>("feature_operation_records")
            .expect("required invariant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].operation_label, labels[0].id);
        assert!(records[0].byte_len > 40);
        assert_eq!(records[0].sha256.len(), 64);
        let booleans = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureBooleanOperation>("feature_boolean_operations")
            .expect("required invariant");
        assert_eq!(booleans.len(), 1);
        assert_eq!(booleans[0].kind, super::FeatureBooleanKind::Unite);
        assert_eq!(booleans[0].target_object_index, 6466);
        assert_eq!(booleans[0].tool_object_indices, [6476, 127]);
        let body_references = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureBodyReference>("feature_body_references")
            .expect("required invariant");
        assert_eq!(body_references.len(), 1);
        assert_eq!(body_references[0].operation_label, labels[0].id);
        assert_eq!(body_references[0].body_object_index, 6466);
        let body_reference_occurrences = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureBodyReferenceOccurrence>("feature_body_reference_occurrences")
            .expect("required invariant");
        assert_eq!(body_reference_occurrences.len(), 1);
        assert_eq!(body_reference_occurrences[0].operation_label, labels[0].id);
        assert_eq!(body_reference_occurrences[0].ordinal, 0);
        assert_eq!(body_reference_occurrences[0].body_object_index, 6466);
        let feature = result.ir.model.features.first().expect("neutral feature");
        assert_eq!(feature.name.as_deref(), Some("UNITE"));
        assert_eq!(feature.suppressed, None);
        assert_eq!(feature.native_ref.as_deref(), Some(labels[0].id.as_str()));
        assert_eq!(
            feature.source_properties.get("body_reference.0"),
            Some(&"6466".to_string())
        );
        assert!(matches!(
            &feature.definition,
            cadmpeg_ir::features::FeatureDefinition::Combine {
                target: cadmpeg_ir::features::BodySelection::Native(target),
                tools: cadmpeg_ir::features::BodySelection::Native(tools),
                op: cadmpeg_ir::features::BooleanOp::Join,
            } if target == "nx:om-object-index#6466" && tools == "nx:om-object-indices#6476,127"
        ));
        assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
    }

    #[test]
    fn decode_resolves_feature_header_input_to_unique_data_block() {
        let file = prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            segment_om_record_area_with_input_store_payload(),
        )]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .expect("required invariant");
        let inputs = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<super::FeatureInputBlock>("feature_input_blocks")
            .expect("required invariant");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].input_slot, 0);
        assert_eq!(inputs[0].object_index, 1);
        assert!(inputs[0].data_block.ends_with(":block#1"));
        assert_eq!(
            result.ir.model.features[0].source_properties["input_block.0"],
            inputs[0].data_block
        );
        let references = result
            .ir
            .native
            .namespace("nx")
            .expect("required invariant")
            .arena_as::<crate::native::om::DataBlockReference>("data_block_references")
            .expect("required invariant");
        assert_eq!(references.len(), 1);
        assert!(references[0].data_block.ends_with(":block#2"));
        assert_ne!(references[0].data_block, inputs[0].data_block);
        assert_eq!(references[0].object_id, 42);
        assert_eq!(references[0].target_record, None);
    }

    #[test]
    fn sketch_point_blocks_establish_ordered_datum_csys_dependencies() {
        use super::{
            FeatureDatumCsysConstruction, FeatureOperationLabel,
            FeatureSketchDatumCsysBlockRelation, FeatureSketchPointUse, OffsetStoreNamedPoint,
        };

        let label = |id: &str, ordinal| FeatureOperationLabel {
            id: id.to_string(),
            section_link: "section".to_string(),
            ordinal,
            value: if ordinal == 0 { "SKETCH" } else { "DATUM_CSYS" }.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 100 + u64::from(ordinal),
        };
        let labels = [label("sketch", 0), label("csys", 1)];
        let point = OffsetStoreNamedPoint {
            id: "point".to_string(),
            name: "Point1".to_string(),
            data_blocks: vec!["point-first".to_string(), "shared".to_string()],
            values: [1.0, 2.0],
            raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
            value_source_offsets: [200, 220],
            source_offset: 190,
        };
        let point_use = FeatureSketchPointUse {
            id: "point-use".to_string(),
            operation_label: "sketch".to_string(),
            sketch_references: vec!["reference".to_string()],
            block_uses: vec!["block-use".to_string()],
            sketch_point_group: "point-group".to_string(),
            named_point: point.id.clone(),
            source_offsets: vec![300],
        };
        let mut blocks = std::array::from_fn(|index| format!("block-{index}"));
        blocks[3] = "shared".to_string();
        let construction = FeatureDatumCsysConstruction {
            id: "construction".to_string(),
            operation_label: "csys".to_string(),
            control: 19,
            object_indices: [0; 8],
            raw_object_indices: std::array::from_fn(|_| vec![0]),
            data_blocks: blocks,
            source_offsets: [400; 8],
        };

        let dependencies = super::feature_sketch_datum_csys_dependencies(
            &labels,
            std::slice::from_ref(&point),
            std::slice::from_ref(&point_use),
            std::slice::from_ref(&construction),
        );
        assert_eq!(dependencies[0].datum_csys_operation_label, "csys");
        assert_eq!(dependencies[0].sketch_operation_label, "sketch");
        assert_eq!(dependencies[0].sketch_point_use, "point-use");
        assert_eq!(
            dependencies[0].block_relation,
            FeatureSketchDatumCsysBlockRelation::Shared {
                data_block: "shared".to_string()
            }
        );

        let consecutive_point = OffsetStoreNamedPoint {
            id: "consecutive-point".to_string(),
            name: "Point2".to_string(),
            data_blocks: vec![
                "nx:om:offset-store#7:block#10".to_string(),
                "nx:om:offset-store#7:block#11".to_string(),
            ],
            values: [3.0, 4.0],
            raw_values: [shifted_f64_bytes(3.0), shifted_f64_bytes(4.0)],
            value_source_offsets: [500, 520],
            source_offset: 490,
        };
        let consecutive_use = FeatureSketchPointUse {
            id: "consecutive-use".to_string(),
            named_point: consecutive_point.id.clone(),
            ..point_use.clone()
        };
        let mut consecutive_construction = construction.clone();
        consecutive_construction.id = "consecutive-construction".to_string();
        consecutive_construction.data_blocks[0] = "nx:om:offset-store#7:block#12".to_string();
        let consecutive_dependencies = super::feature_sketch_datum_csys_dependencies(
            &labels,
            &[consecutive_point],
            &[consecutive_use],
            &[consecutive_construction],
        );
        assert_eq!(
            consecutive_dependencies[0].block_relation,
            FeatureSketchDatumCsysBlockRelation::Consecutive {
                point_data_block: "nx:om:offset-store#7:block#11".to_string(),
                construction_data_block: "nx:om:offset-store#7:block#12".to_string(),
            }
        );

        let mut ambiguous_point = point.clone();
        ambiguous_point.id = "ambiguous-point".to_string();
        let ambiguous_use = FeatureSketchPointUse {
            id: "ambiguous-use".to_string(),
            named_point: ambiguous_point.id.clone(),
            ..point_use.clone()
        };
        assert!(super::feature_sketch_datum_csys_dependencies(
            &labels,
            &[point.clone(), ambiguous_point],
            &[point_use.clone(), ambiguous_use],
            std::slice::from_ref(&construction),
        )
        .is_empty());

        let reversed_labels = [label("csys", 0), label("sketch", 1)];
        assert!(super::feature_sketch_datum_csys_dependencies(
            &reversed_labels,
            &[point],
            &[point_use],
            &[construction],
        )
        .is_empty());
    }

    #[test]
    fn nx_sketch_point_names_require_positive_decimal_suffixes() {
        assert_eq!(super::parse_sketch_point_name("Point1"), Some(1));
        assert_eq!(super::parse_sketch_point_name("Point2048"), Some(2048));
        for malformed in ["Point", "Point0", "point1", "Point-1", "Point1A"] {
            assert_eq!(super::parse_sketch_point_name(malformed), None);
        }
    }

    #[test]
    fn nx_datum_plane_csys_identity_uses_join_only_equal_typed_identities() {
        let plane = super::FeatureDatumPlaneDescriptor {
            id: "plane-descriptor".into(),
            operation_label: "operation#4".into(),
            datum_plane_header: "plane-header".into(),
            ordinal: 0,
            data_block: "plane-block".into(),
            identity: "012345678901234567890123456789".into(),
            suffix: vec![b'?', b'A'],
            schema_index: 1,
            label: "p".into(),
            source_offset: 10,
        };
        let csys = super::FeatureDatumCsysDescriptor {
            id: "csys-descriptor".into(),
            operation_label: "operation#2".into(),
            construction: "csys-construction".into(),
            reference_ordinal: 7,
            data_block: "csys-block".into(),
            prefix: vec![2, 1],
            identity: plane.identity.clone(),
            suffix: vec![b'?', b'A'],
            source_offset: 20,
            identity_source_offset: 22,
        };
        let uses = super::feature_datum_plane_csys_identity_uses(&[plane], &[csys]);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].identity, "012345678901234567890123456789");
        assert_eq!(uses[0].datum_plane_operation_label, "operation#4");
        assert_eq!(uses[0].datum_csys_operation_label, "operation#2");
        assert_eq!(uses[0].datum_csys_reference_ordinal, 7);
    }

    #[test]
    fn nx_datum_csys_block_uses_preserve_reference_and_input_order() {
        let construction = super::FeatureDatumCsysConstruction {
            id: "construction".to_string(),
            operation_label: "operation#0".to_string(),
            control: 0x13,
            object_indices: std::array::from_fn(|index| index as u32 + 40),
            raw_object_indices: std::array::from_fn(|index| vec![index as u8 + 40]),
            data_blocks: std::array::from_fn(|index| format!("block#{}", index + 40)),
            source_offsets: std::array::from_fn(|index| index as u64 + 100),
        };
        let input = |id: &str, operation: &str, slot: u8, block: &str| super::FeatureInputBlock {
            id: id.to_string(),
            operation_label: operation.to_string(),
            input_slot: slot,
            object_index: 44,
            raw_object_index: vec![44],
            data_block: block.to_string(),
            source_offset: 200,
        };
        let uses = super::feature_datum_csys_block_uses(
            &[construction],
            &[
                input("input#0", "operation#0", 1, "block#43"),
                input("input#1", "operation#6", 0, "block#44"),
                input("input#2", "operation#7", 0, "block#44"),
            ],
        );
        assert_eq!(uses.len(), 3);
        assert_eq!(
            uses[0].id,
            "nx:feature-history:datum-csys-block-use#0-3-0-1"
        );
        assert_eq!(uses[0].reference_ordinal, 3);
        assert_eq!(uses[0].input_operation_label, "operation#0");
        assert_eq!(uses[1].reference_ordinal, 4);
        assert_eq!(uses[1].input_operation_label, "operation#6");
        assert_eq!(uses[2].reference_ordinal, 4);
        assert_eq!(uses[2].input_operation_label, "operation#7");
    }

    #[test]
    fn nx_extrude_construction_profile_requires_matching_resolved_encodings() {
        use super::{
            FeatureExtrudeProfileReference, FeatureOperationBodyReferenceLane,
            FeatureOperationBodyReferenceLaneEncoding,
        };

        let references = [10, 11].map(|ordinal| FeatureExtrudeProfileReference {
            id: format!("profile-{ordinal}"),
            operation_label: "operation".to_string(),
            ordinal: ordinal - 10,
            witnessed: true,
            object_index: ordinal + 90,
            raw_object_index: vec![(ordinal + 90) as u8],
            data_block: Some(format!("block-{ordinal}")),
            source_offset: u64::from(ordinal),
        });
        let lane = FeatureOperationBodyReferenceLane {
            id: "lane".to_string(),
            operation_label: "operation".to_string(),
            body_reference_ordinal: 0,
            body_object_index: 42,
            branch: 0x11,
            encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
            object_indices: vec![100, 101],
            raw_object_indices: vec![vec![0xf0, 100], vec![0xf0, 101]],
            data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
            source_offsets: vec![20, 21],
        };
        let profiles =
            super::feature_extrude_construction_profiles(&references, std::slice::from_ref(&lane));
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].body_object_index, 42);
        assert_eq!(profiles[0].object_indices, [100, 101]);
        assert_eq!(profiles[0].data_blocks, ["block-10", "block-11"]);

        for ordinal in [0, 2] {
            let mut malformed = references.clone();
            malformed[1].ordinal = ordinal;
            assert!(super::feature_extrude_construction_profiles(
                &malformed,
                std::slice::from_ref(&lane),
            )
            .is_empty());
        }

        let mut mismatched = lane.clone();
        mismatched.object_indices[1] = 102;
        assert!(
            super::feature_extrude_construction_profiles(&references, &[mismatched]).is_empty()
        );

        let mut unresolved = FeatureOperationBodyReferenceLane {
            id: "lane".to_string(),
            operation_label: "operation".to_string(),
            body_reference_ordinal: 0,
            body_object_index: 42,
            branch: 0x11,
            encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
            object_indices: vec![100, 101],
            raw_object_indices: vec![vec![0xf0, 100], vec![0xf0, 101]],
            data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
            source_offsets: vec![20, 21],
        };
        unresolved.data_blocks[1] = None;
        assert!(
            super::feature_extrude_construction_profiles(&references, &[unresolved]).is_empty()
        );
    }

    #[test]
    fn nx_operation_body_operands_require_known_distinct_body_identities() {
        use super::{FeatureBodyReferenceOccurrence, FeatureOperationBodyMember};
        use crate::native::segments::SegmentBodyBinding;
        let member = |ordinal, member_index| FeatureOperationBodyMember {
            id: format!("nx:feature-history:operation-body-member#0-{ordinal}"),
            operation_label: "operation".to_string(),
            body_reference_ordinal: 0,
            body_object_index: 10,
            ordinal,
            member_index,
            raw_member_index: vec![member_index as u8],
            source_offset: u64::from(ordinal),
        };
        let members = [member(0, 20), member(1, 30), member(2, 10)];
        let references = [FeatureBodyReferenceOccurrence {
            id: "reference".to_string(),
            operation_label: "earlier".to_string(),
            ordinal: 0,
            body_object_index: 20,
            raw_body_object_index: vec![20],
            source_offset: 0,
        }];
        let bindings = [SegmentBodyBinding {
            id: "binding".to_string(),
            stream_link: "stream".to_string(),
            stream_ordinal: 0,
            stream_kind: "partition".to_string(),
            body_object_index: 40,
            body_alias_object_index: 30,
            stream_role: 0,
            source_offset: 0,
        }];
        let operands = super::feature_operation_body_operands(&members, &references, &bindings);
        assert_eq!(
            operands
                .iter()
                .map(|operand| operand.operand_object_index)
                .collect::<Vec<_>>(),
            [20, 30]
        );
        assert!(operands[0].segment_body_bindings.is_empty());
        assert_eq!(operands[1].segment_body_bindings, ["binding"]);

        let mut second_clause = operands[0].clone();
        second_clause.body_reference_ordinal = 1;
        assert_eq!(
            operands[0].source_property_key(),
            "operation_body_operand.0.0"
        );
        assert_eq!(
            second_clause.source_property_key(),
            "operation_body_operand.1.0"
        );
    }

    #[test]
    fn nx_extrude_32_construction_requires_resolved_contiguous_profile() {
        let reference = super::FeatureExtrudeProfileReference {
            id: "profile#0".to_string(),
            operation_label: "operation".to_string(),
            ordinal: 0,
            witnessed: false,
            object_index: 100,
            raw_object_index: vec![100],
            data_block: Some("block#100".to_string()),
            source_offset: 10,
        };
        let branch = super::FeatureExtrudePayload32Branch {
            id: "branch".to_string(),
            operation_label: "operation".to_string(),
            body_object_index: 42,
            scalar: 1.0,
            raw_scalar: [0x2f, 0xf0, 0, 0, 0, 0, 0, 0],
            atoms_be: vec![0x3d80_0100],
            atom_source_offsets: vec![20],
            atom_indices: vec![1],
            atom_data_blocks: vec![Some("block#1".to_string())],
            first_indices: vec![2],
            raw_first_indices: vec![vec![2]],
            first_index_source_offsets: vec![21],
            first_data_blocks: vec![Some("block#2".to_string())],
            second_indices: vec![3],
            raw_second_indices: vec![vec![3]],
            second_index_source_offsets: vec![22],
            second_data_blocks: vec![Some("block#3".to_string())],
            terminal_object_index: 42,
            raw_terminal_object_index: vec![42],
            terminal_source_offset: 23,
            source_offset: 20,
        };
        let constructions = super::feature_extrude_32_constructions(
            std::slice::from_ref(&reference),
            std::slice::from_ref(&branch),
        );
        assert_eq!(constructions.len(), 1);
        assert_eq!(constructions[0].body_object_index, 42);
        assert_eq!(constructions[0].profile_references, ["profile#0"]);
        assert_eq!(constructions[0].profile_data_blocks, ["block#100"]);
        assert_eq!(constructions[0].atom_data_blocks, ["block#1"]);
        assert_eq!(constructions[0].first_data_blocks, ["block#2"]);
        assert_eq!(constructions[0].second_data_blocks, ["block#3"]);

        assert!(super::feature_extrude_32_constructions(
            std::slice::from_ref(&reference),
            &[branch.clone(), branch.clone()],
        )
        .is_empty());

        let mut unresolved = reference;
        unresolved.data_block = None;
        assert!(super::feature_extrude_32_constructions(
            &[unresolved],
            std::slice::from_ref(&branch),
        )
        .is_empty());
        let mut unresolved_lane = branch;
        unresolved_lane.first_data_blocks[0] = None;
        assert!(super::feature_extrude_32_constructions(
            &[super::FeatureExtrudeProfileReference {
                id: "profile#0".to_string(),
                operation_label: "operation".to_string(),
                ordinal: 0,
                witnessed: false,
                object_index: 100,
                raw_object_index: vec![100],
                data_block: Some("block#100".to_string()),
                source_offset: 10,
            }],
            &[unresolved_lane],
        )
        .is_empty());
    }

    #[test]
    fn nx_block_construction_requires_complete_resolved_reference_field() {
        let references = (0..19)
            .map(|ordinal| super::FeatureBlockConstructionReference {
                id: format!("reference#{ordinal}"),
                operation_label: "operation".to_string(),
                control: 0x26,
                ordinal,
                terminal: ordinal == 18,
                object_index: ordinal + 100,
                raw_object_index: vec![(ordinal + 100) as u8],
                data_block: Some(format!("block#{ordinal}")),
                source_offset: u64::from(ordinal),
            })
            .collect::<Vec<_>>();
        let constructions = super::feature_block_constructions(&references);
        assert_eq!(constructions.len(), 1);
        assert_eq!(constructions[0].control, 0x26);
        assert_eq!(constructions[0].member_references.len(), 18);
        assert_eq!(constructions[0].terminal_reference, "reference#18");
        assert_eq!(constructions[0].terminal_data_block, "block#18");

        let mut unresolved = references;
        unresolved[7].data_block = None;
        assert!(super::feature_block_constructions(&unresolved).is_empty());
    }

    #[test]
    fn data_block_object_frame_ids_include_the_store_qualifier() {
        let first = super::data_block_object_frame_id("nx:om-data-blocks-2:block#17", 0);
        let second = super::data_block_object_frame_id("nx:om-data-blocks-3:block#17", 0);
        assert_eq!(first, "nx:om-data-block-object-frames-2:block-frame#17-0");
        assert_eq!(second, "nx:om-data-block-object-frames-3:block-frame#17-0");
        assert_ne!(first, second);
    }

    #[test]
    fn feature_input_identity_groups_require_distinct_operations_and_preserve_order() {
        use super::{feature_input_block_identity_groups, FeatureInputBlock};

        let input =
            |id: &str, operation: &str, slot: u8, block: &str, offset: u64| FeatureInputBlock {
                id: id.to_string(),
                operation_label: operation.to_string(),
                input_slot: slot,
                object_index: 7,
                raw_object_index: vec![7],
                data_block: block.to_string(),
                source_offset: offset,
            };
        let groups = feature_input_block_identity_groups(&[
            input("late", "operation-b", 1, "block-7", 30),
            input("single-a", "operation-a", 0, "block-8", 10),
            input("early", "operation-a", 2, "block-7", 20),
            input("single-b", "operation-a", 3, "block-8", 40),
        ]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].data_block, "block-7");
        assert_eq!(groups[0].input_blocks, ["early", "late"]);
        assert_eq!(groups[0].operation_labels, ["operation-a", "operation-b"]);
        assert_eq!(groups[0].input_slots, [2, 1]);
        assert_eq!(groups[0].source_offsets, [20, 30]);
    }

    #[test]
    fn feature_input_column_row_uses_preserve_index_row_slots() {
        use super::{feature_input_column_row_uses, ColumnIndexRowKind, FeatureInputBlock};
        use crate::native::om::DataBlockIndexRow;

        let input = FeatureInputBlock {
            id: "input#0000000001".into(),
            operation_label: "operation#1".into(),
            input_slot: 2,
            object_index: 7,
            raw_object_index: vec![7],
            data_block: "block#4".into(),
            source_offset: 10,
        };
        let row = DataBlockIndexRow {
            id: "row#3".into(),
            section_ordinal: 0,
            ordinal: 3,
            first_index: 20,
            raw_first_index: vec![20],
            flag: 3,
            indices: [4, 4, 5, 6],
            raw_indices: [vec![4], vec![4], vec![5], vec![6]],
            data_blocks: [
                "block#4".into(),
                "block#4".into(),
                "block#5".into(),
                "block#6".into(),
            ],
            source_entry: "entry".into(),
            opening_data_block: "opening-block".into(),
            opening_block_offset: 8,
            source_offset: 100,
            first_index_source_offset: 103,
            index_source_offsets: [108, 109, 110, 111],
        };

        let uses = feature_input_column_row_uses(&[input], &[row], &[], &[], &[]);
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].input_block, "input#0000000001");
        assert_eq!(uses[0].operation_label, "operation#1");
        assert_eq!(uses[0].input_slot, 2);
        assert_eq!(uses[0].row_kind, ColumnIndexRowKind::Index);
        assert_eq!(uses[0].column_row, "row#3");
        assert_eq!(uses[0].row_slot, 0);
        assert_eq!(uses[0].source_offset, 108);
        assert_eq!(uses[1].row_slot, 1);
        assert_eq!(uses[1].source_offset, 109);
    }

    #[test]
    fn feature_input_column_row_uses_preserve_linked_row_slots() {
        use super::{
            feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
            FeatureInputBlock,
        };
        use crate::native::om::{DataBlockColumnIndexTable, DataBlockLinkedIndexRow};

        let input = FeatureInputBlock {
            id: "input#0000000001".into(),
            operation_label: "operation#1".into(),
            input_slot: 2,
            object_index: 4,
            raw_object_index: vec![4],
            data_block: "block#4".into(),
            source_offset: 10,
        };
        let row = DataBlockLinkedIndexRow {
            id: "linked-row#3".into(),
            section_ordinal: 0,
            ordinal: 3,
            first_index: 20,
            raw_first_index: vec![20],
            discriminator: 0x16,
            target_index: 4,
            raw_target_index: vec![4],
            indices: [5, 6, 4],
            raw_indices: [vec![5], vec![6], vec![4]],
            data_blocks: [
                "block#4".into(),
                "block#5".into(),
                "block#6".into(),
                "block#4".into(),
            ],
            flag: 3,
            mode: 4,
            source_entry: "entry".into(),
            opening_data_block: "opening-block".into(),
            opening_block_offset: 8,
            source_offset: 100,
            first_index_source_offset: 102,
            target_index_source_offset: 107,
            index_source_offsets: [112, 113, 114],
        };

        let table = DataBlockColumnIndexTable {
            id: "column-table".into(),
            section_ordinal: 0,
            opening_linked_row: row.id.clone(),
            target_rows: vec!["target-row".into()],
            linked_rows: vec!["suffix-row".into()],
            first_target_index: 4,
            last_target_index: 2,
            source_entry: "entry".into(),
            source_offset: 100,
        };
        let uses = feature_input_column_row_uses(
            std::slice::from_ref(&input),
            &[],
            std::slice::from_ref(&row),
            &[],
            &[table],
        );
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].input_block, "input#0000000001");
        assert_eq!(uses[0].operation_label, "operation#1");
        assert_eq!(uses[0].input_slot, 2);
        assert_eq!(uses[0].row_kind, ColumnIndexRowKind::LinkedIndex);
        assert_eq!(uses[0].column_row, "linked-row#3");
        assert_eq!(uses[0].row_slot, 0);
        assert_eq!(uses[0].source_offset, 107);
        assert_eq!(uses[1].row_slot, 3);
        assert_eq!(uses[1].source_offset, 114);
        let targets = feature_input_column_targets(&[input], &uses, &[row], &[]);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].leading_index, Some(20));
        assert_eq!(targets[0].leading_index_source_offset, Some(102));
        assert_eq!(targets[0].discriminator, Some(0x16));
        assert_eq!(targets[0].field_indices, [5, 6, 4]);
        assert_eq!(targets[0].flag, Some(3));
        assert_eq!(targets[0].mode, 4);
    }

    #[test]
    fn feature_input_column_row_uses_preserve_target_row_slots() {
        use super::{
            feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
            FeatureInputBlock,
        };
        use crate::native::om::{DataBlockColumnIndexTable, DataBlockTargetIndexRow};

        let input = FeatureInputBlock {
            id: "input#0000000001".into(),
            operation_label: "operation#1".into(),
            input_slot: 2,
            object_index: 4,
            raw_object_index: vec![4],
            data_block: "block#4".into(),
            source_offset: 10,
        };
        let row = DataBlockTargetIndexRow {
            id: "target-row#3".into(),
            section_ordinal: 0,
            ordinal: 3,
            target_index: 4,
            raw_target_index: vec![4],
            indices: [5, 6, 4],
            raw_indices: [vec![5], vec![6], vec![4]],
            data_blocks: [
                "block#4".into(),
                "block#5".into(),
                "block#6".into(),
                "block#4".into(),
            ],
            mode: 7,
            source_entry: "entry".into(),
            opening_data_block: "opening-block".into(),
            opening_block_offset: 8,
            source_offset: 100,
            target_index_source_offset: 105,
            index_source_offsets: [110, 111, 112],
        };

        let table = DataBlockColumnIndexTable {
            id: "column-table".into(),
            section_ordinal: 0,
            opening_linked_row: "opening-row".into(),
            target_rows: vec!["target-row#3".into()],
            linked_rows: vec!["suffix-row".into()],
            first_target_index: 5,
            last_target_index: 3,
            source_entry: "entry".into(),
            source_offset: 50,
        };
        let ambiguous = feature_input_column_row_uses(
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[table.clone(), table.clone()],
        );
        assert!(ambiguous.iter().all(|use_| use_.column_table.is_none()));
        let uses = feature_input_column_row_uses(
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[table],
        );
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].input_block, "input#0000000001");
        assert_eq!(uses[0].operation_label, "operation#1");
        assert_eq!(uses[0].input_slot, 2);
        assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
        assert_eq!(uses[0].column_row, "target-row#3");
        assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
        assert_eq!(uses[0].row_slot, 0);
        assert_eq!(uses[0].source_offset, 105);
        assert_eq!(uses[1].row_slot, 3);
        assert_eq!(uses[1].source_offset, 112);
        let targets = feature_input_column_targets(
            std::slice::from_ref(&input),
            &uses,
            &[],
            std::slice::from_ref(&row),
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].input_block, input.id);
        assert_eq!(targets[0].column_row, "target-row#3");
        assert_eq!(targets[0].column_table, "column-table");
        assert_eq!(targets[0].field_indices, [5, 6, 4]);
        assert_eq!(
            targets[0].field_data_blocks,
            ["block#5", "block#6", "block#4"]
        );
        assert_eq!(targets[0].field_source_offsets, [110, 111, 112]);
        assert_eq!(targets[0].mode, 7);
        assert_eq!(targets[0].leading_index, None);
        let mut duplicate = uses.clone();
        duplicate.push(uses[0].clone());
        assert!(feature_input_column_targets(&[input], &duplicate, &[], &[row]).is_empty());
    }

    #[test]
    fn sketch_named_records_own_fixed_pairs_within_their_intervals() {
        use super::{
            feature_sketch_fixed_points, feature_sketch_payload_named_records,
            FeatureSketchConstructionPayload, FeatureSketchPayloadFixedPair,
            FeatureSketchPayloadName,
        };
        let payload = FeatureSketchConstructionPayload {
            id: "payload".to_string(),
            operation_label: "sketch".to_string(),
            construction_inputs: "inputs".to_string(),
            data_blocks: vec!["block".to_string()],
            byte_len: 100,
            sha256: "00".repeat(32),
            block_payload_offsets: vec![0],
            block_byte_lengths: vec![100],
            block_source_offsets: vec![1000],
        };
        let name = |id: &str, ordinal, offset| FeatureSketchPayloadName {
            id: id.to_string(),
            operation_label: "sketch".to_string(),
            construction_payload: "payload".to_string(),
            ordinal,
            type_code: Some(1),
            raw_type_code: Some(vec![1]),
            type_code_payload_offset: Some(offset + 1),
            type_code_source_offset: Some(1001 + offset),
            payload_leading: false,
            value: format!("Point{}", ordinal + 1),
            payload_offset: offset,
            source_offset: 1000 + offset,
        };
        let pair = FeatureSketchPayloadFixedPair {
            id: "pair".to_string(),
            operation_label: "sketch".to_string(),
            construction_payload: "payload".to_string(),
            ordinal: 0,
            values: [0.5, -0.5],
            raw_values: [[0; 7]; 2],
            payload_offset: 20,
            value_payload_offsets: [28, 37],
            source_offset: 1020,
            value_source_offsets: [1028, 1037],
        };

        let names = [name("first", 0, 10), name("second", 1, 50)];
        let pairs = [pair];
        let records = feature_sketch_payload_named_records(&[payload], &names, &[], &pairs);
        assert_eq!(records[0].fixed_pairs, ["pair"]);
        assert!(records[1].fixed_pairs.is_empty());
        let points = feature_sketch_fixed_points(&records, &names, &pairs);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].name, "Point1");
        assert_eq!(points[0].values, [0.5, -0.5]);
    }

    #[test]
    fn sketch_named_point_block_uses_require_exact_shared_block_identity() {
        use super::{
            feature_sketch_named_point_block_uses, FeatureSketchReference, OffsetStoreNamedPoint,
        };

        let point = OffsetStoreNamedPoint {
            id: "nx:offset-store:named-point#2-10".to_string(),
            name: "Point1".to_string(),
            data_blocks: vec!["block-10".to_string(), "block-11".to_string()],
            values: [1.0, 2.0],
            raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
            value_source_offsets: [100, 120],
            source_offset: 90,
        };
        let reference = |id: &str, ordinal: u32, block: Option<&str>| FeatureSketchReference {
            id: id.to_string(),
            operation_label: "nx:feature-history:operation-label#1-4".to_string(),
            ordinal,
            declared_count: 2,
            terminal: ordinal == 1,
            object_index: 10 + ordinal,
            raw_object_index: vec![0xf0, (10 + ordinal) as u8],
            data_block: block.map(str::to_string),
            source_offset: 200 + u64::from(ordinal),
        };
        let uses = feature_sketch_named_point_block_uses(
            &[
                reference("miss", 0, Some("block-9")),
                reference("hit", 1, Some("block-11")),
                reference("unresolved", 2, None),
            ],
            &[point],
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].sketch_reference, "hit");
        assert_eq!(uses[0].reference_ordinal, 1);
        assert_eq!(uses[0].point_block_ordinal, 1);
        assert_eq!(uses[0].data_block, "block-11");
    }

    #[test]
    fn sketch_preceding_named_point_uses_require_a_complete_unique_consecutive_lane() {
        use super::{
            feature_sketch_preceding_named_point_uses, FeatureSketchReference,
            OffsetStoreNamedPoint,
        };

        let reference = |ordinal, terminal, block: Option<&str>| FeatureSketchReference {
            id: format!("reference-{ordinal}"),
            operation_label: "nx:feature-history:operation-label#1-4".to_string(),
            ordinal,
            declared_count: 2,
            terminal,
            object_index: 12 + ordinal,
            raw_object_index: vec![0xf0, (12 + ordinal) as u8],
            data_block: block.map(str::to_string),
            source_offset: 300 + u64::from(ordinal),
        };
        let references = [
            reference(0, false, Some("nx:om-data-blocks-2:block#12")),
            reference(1, true, Some("nx:om-data-blocks-2:block#13")),
        ];
        let point = |id: &str, blocks: &[&str]| OffsetStoreNamedPoint {
            id: id.to_string(),
            name: "Point1".to_string(),
            data_blocks: blocks.iter().map(|block| (*block).to_string()).collect(),
            values: [1.0, 2.0],
            raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
            value_source_offsets: [200, 220],
            source_offset: 190,
        };
        let preceding = point(
            "nx:offset-store:named-point#2-10",
            &[
                "nx:om-data-blocks-2:block#10",
                "nx:om-data-blocks-2:block#11",
            ],
        );
        let uses = feature_sketch_preceding_named_point_uses(
            &references,
            std::slice::from_ref(&preceding),
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].first_sketch_reference, references[0].id);
        assert_eq!(uses[0].named_point, preceding.id);
        assert_eq!(uses[0].following_data_block, "nx:om-data-blocks-2:block#12");

        let ambiguous = point(
            "nx:offset-store:named-point#2-11",
            &["nx:om-data-blocks-2:block#11"],
        );
        assert!(feature_sketch_preceding_named_point_uses(
            &references,
            &[preceding.clone(), ambiguous]
        )
        .is_empty());
        let gap = point(
            "nx:offset-store:named-point#2-9",
            &["nx:om-data-blocks-2:block#9"],
        );
        let other_store = point(
            "nx:offset-store:named-point#3-11",
            &["nx:om-data-blocks-3:block#11"],
        );
        assert!(
            feature_sketch_preceding_named_point_uses(&references, &[gap, other_store]).is_empty()
        );

        let unresolved = [references[0].clone(), reference(1, true, None)];
        assert!(feature_sketch_preceding_named_point_uses(
            &unresolved,
            std::slice::from_ref(&preceding)
        )
        .is_empty());
        let noncontiguous = [
            references[0].clone(),
            reference(2, true, Some("nx:om-data-blocks-2:block#13")),
        ];
        assert!(feature_sketch_preceding_named_point_uses(
            &noncontiguous,
            std::slice::from_ref(&preceding),
        )
        .is_empty());
        let bad_terminal = [
            references[0].clone(),
            reference(1, false, Some("nx:om-data-blocks-2:block#13")),
        ];
        assert!(feature_sketch_preceding_named_point_uses(&bad_terminal, &[preceding]).is_empty());
    }

    #[test]
    fn sketch_point_uses_retain_identical_witnesses_and_reject_conflicts() {
        use super::{
            feature_sketch_point_groups, feature_sketch_point_uses,
            FeatureSketchNamedPointBlockUse, FeatureSketchPoint, OffsetStoreNamedPoint,
        };

        let operation_label = "nx:feature-history:operation-label#1-4".to_string();
        let point = FeatureSketchPoint {
            id: "payload-point".to_string(),
            operation_label: operation_label.clone(),
            named_record: "named-record".to_string(),
            name: "Point1".to_string(),
            coordinates: [1.0, 2.0],
            scalar_fields: ["scalar-1".to_string(), "scalar-2".to_string()],
        };
        let named_point = OffsetStoreNamedPoint {
            id: "named-point".to_string(),
            name: "Point1".to_string(),
            data_blocks: vec!["block-10".to_string()],
            values: [1.0, 2.0],
            raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
            value_source_offsets: [200, 220],
            source_offset: 190,
        };
        let block_use = FeatureSketchNamedPointBlockUse {
            id: "nx:feature-history:sketch-named-point-block-use#1-4-0".to_string(),
            operation_label,
            sketch_reference: "reference".to_string(),
            reference_ordinal: 0,
            named_point: named_point.id.clone(),
            data_block: "block-10".to_string(),
            point_block_ordinal: 0,
            source_offset: 300,
        };
        let mut second_block_use = block_use.clone();
        second_block_use.id = "nx:feature-history:sketch-named-point-block-use#1-4-1".to_string();
        second_block_use.sketch_reference = "reference-2".to_string();
        second_block_use.reference_ordinal = 1;
        second_block_use.source_offset = 301;

        let groups = feature_sketch_point_groups(std::slice::from_ref(&point));
        let uses = feature_sketch_point_uses(
            &groups,
            std::slice::from_ref(&named_point),
            &[second_block_use.clone(), block_use.clone()],
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].sketch_point_group, groups[0].id);
        assert_eq!(uses[0].named_point, named_point.id);
        assert_eq!(uses[0].sketch_references, ["reference", "reference-2"]);
        assert_eq!(uses[0].block_uses.len(), 2);
        assert_eq!(uses[0].source_offsets, [300, 301]);

        let mut different = point.clone();
        different.id = "different".to_string();
        different.coordinates[1] = f64::from_bits(2.0_f64.to_bits() + 1);
        let different_groups = feature_sketch_point_groups(std::slice::from_ref(&different));
        assert!(feature_sketch_point_uses(
            &different_groups,
            std::slice::from_ref(&named_point),
            std::slice::from_ref(&block_use),
        )
        .is_empty());
        let mut duplicate = point.clone();
        duplicate.id = "payload-point-2".to_string();
        let duplicate_groups = feature_sketch_point_groups(&[point.clone(), duplicate.clone()]);
        assert_eq!(duplicate_groups[0].points, [point.id.clone(), duplicate.id]);
        let uses = feature_sketch_point_uses(
            &duplicate_groups,
            std::slice::from_ref(&named_point),
            std::slice::from_ref(&block_use),
        );
        assert_eq!(uses[0].sketch_point_group, duplicate_groups[0].id);
        let conflicting_groups = feature_sketch_point_groups(&[point, different]);
        assert!(conflicting_groups.is_empty());
        assert!(
            feature_sketch_point_uses(&conflicting_groups, &[named_point], &[block_use]).is_empty()
        );
    }

    #[test]
    fn segment_body_lineage_statuses_cover_every_bound_image() {
        use super::{
            FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
            FeatureOperationLabel,
        };
        use crate::native::segments::{segment_body_lineage_statuses, SegmentBodyBinding};
        let labels = [
            FeatureOperationLabel {
                id: "operation#0".to_string(),
                section_link: "history#0".to_string(),
                ordinal: 0,
                value: "EXTRUDE".to_string(),
                object_indices: [None; 4],
                raw_object_indices: std::array::from_fn(|_| vec![0xff]),
                source_offset: 0,
            },
            FeatureOperationLabel {
                id: "operation#1".to_string(),
                section_link: "history#0".to_string(),
                ordinal: 1,
                value: "UNITE".to_string(),
                object_indices: [None; 4],
                raw_object_indices: std::array::from_fn(|_| vec![0xff]),
                source_offset: 1,
            },
        ];
        let references = [FeatureBodyReference {
            id: "reference#0".to_string(),
            operation_label: "operation#0".to_string(),
            body_object_index: 10,
            raw_body_object_index: vec![10],
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#1".to_string(),
            kind: FeatureBooleanKind::Unite,
            target_object_index: 10,
            raw_target_object_index: vec![10],
            target_source_offset: 1,
            tool_object_indices: vec![21],
            raw_tool_object_indices: vec![vec![21]],
            tool_source_offsets: vec![1],
            source_offset: 1,
        }];
        let binding =
            |id: &str, stream_ordinal: u32, stream_kind: &str, body, alias| SegmentBodyBinding {
                id: id.to_string(),
                stream_link: format!("stream#{stream_ordinal}"),
                stream_ordinal,
                stream_kind: stream_kind.to_string(),
                body_object_index: body,
                body_alias_object_index: alias,
                stream_role: 19,
                source_offset: u64::from(stream_ordinal),
            };
        let statuses = segment_body_lineage_statuses(
            &labels,
            &references,
            &booleans,
            &[],
            &[
                binding("binding#0", 0, "partition", 10, 11),
                binding("binding#1", 1, "plain", 20, 21),
            ],
        )
        .expect("required invariant");
        assert_eq!(statuses.len(), 2);
        assert!(statuses[0].terminal);
        assert!(!statuses[1].terminal);
    }

    #[test]
    fn feature_body_segment_uses_require_one_alias_pair() {
        use super::{feature_body_segment_uses, FeatureBodyReference};
        use crate::native::segments::SegmentBodyBinding;
        let reference = FeatureBodyReference {
            id: "nx:feature-history:body-reference#0".into(),
            operation_label: "operation#0".into(),
            body_object_index: 11,
            raw_body_object_index: vec![11],
            source_offset: 90,
        };
        let binding = SegmentBodyBinding {
            id: "binding#0".into(),
            stream_link: "stream#3".into(),
            stream_ordinal: 3,
            stream_kind: "plain".into(),
            body_object_index: 10,
            body_alias_object_index: 11,
            stream_role: 19,
            source_offset: 40,
        };
        let uses = feature_body_segment_uses(
            std::slice::from_ref(&reference),
            std::slice::from_ref(&binding),
        );
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].feature_body_reference, reference.id);
        assert_eq!(uses[0].segment_body_binding, binding.id);
        assert!(feature_body_segment_uses(&[reference], &[binding.clone(), binding]).is_empty());
    }

    #[test]
    fn feature_body_lineage_closes_overlapping_alias_pairs_transitively() {
        use super::{
            FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
            FeatureOperationLabel,
        };
        use crate::native::segments::{segment_body_lineage_statuses, SegmentBodyBinding};

        let label = |ordinal: u32, value: &str| FeatureOperationLabel {
            id: format!("operation#{ordinal}"),
            section_link: "history#0".to_string(),
            ordinal,
            value: value.to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: u64::from(ordinal),
        };
        let labels = [label(0, "EXTRUDE"), label(1, "UNITE")];
        let references = [FeatureBodyReference {
            id: "reference#30".to_string(),
            operation_label: "operation#0".to_string(),
            body_object_index: 30,
            raw_body_object_index: vec![30],
            source_offset: 0,
        }];
        let booleans = [FeatureBooleanOperation {
            id: "boolean#0".to_string(),
            operation_label: "operation#1".to_string(),
            kind: FeatureBooleanKind::Unite,
            target_object_index: 99,
            raw_target_object_index: vec![99],
            target_source_offset: 1,
            tool_object_indices: vec![10],
            raw_tool_object_indices: vec![vec![10]],
            tool_source_offsets: vec![1],
            source_offset: 1,
        }];
        let binding = |id: &str, stream_ordinal, body, alias| SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("stream#{stream_ordinal}"),
            stream_ordinal,
            stream_kind: "partition".to_string(),
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(stream_ordinal),
        };
        let bindings = [
            binding("binding#0", 0, 10, 20),
            binding("binding#1", 1, 30, 20),
            binding("binding#2", 2, 40, 20),
        ];

        let statuses =
            segment_body_lineage_statuses(&labels, &references, &booleans, &[], &bindings)
                .expect("required invariant");
        assert_eq!(statuses.len(), 3);
        assert!(statuses.iter().all(|status| !status.terminal));
    }

    #[test]
    fn nx_simple_hole_construction_groups_require_shared_four_block_identity() {
        use super::{
            feature_simple_hole_construction_groups, FeatureSimpleHoleRepeatedScalarLane,
            FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
        };
        let lane = |operation: &str| FeatureSimpleHoleRepeatedScalarLane {
            id: format!("lane-{operation}"),
            operation_label: operation.into(),
            values: vec![25.4],
            raw_values: vec![[0x30; 8]],
            first_witness_offsets: vec![1],
            second_witness_offsets: vec![2],
        };
        let reference =
            |operation: &str, last: &str| FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
                id: format!("reference-{operation}"),
                operation_label: operation.into(),
                first_data_blocks: ["block-1".into(), "block-2".into()],
                second_data_blocks: ["block-3".into(), last.into()],
                first_reference_offsets: [3, 4],
                second_reference_offsets: [5, 6],
            };
        let lanes = [
            lane("operation#1-2"),
            lane("operation#1-3"),
            lane("operation#1-4"),
        ];
        let references = [
            reference("operation#1-4", "block-5"),
            reference("operation#1-3", "block-4"),
            reference("operation#1-2", "block-4"),
        ];
        let groups = feature_simple_hole_construction_groups(&lanes, &references);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].operation_labels,
            ["operation#1-2", "operation#1-3"]
        );
        assert_eq!(
            groups[0].scalar_lanes,
            ["lane-operation#1-2", "lane-operation#1-3"]
        );
        assert_eq!(
            groups[0].block_references,
            ["reference-operation#1-2", "reference-operation#1-3"]
        );

        let duplicate_references = [
            reference("operation#1-2", "block-4"),
            reference("operation#1-2", "block-4"),
        ];
        assert!(feature_simple_hole_construction_groups(&lanes, &duplicate_references).is_empty());

        let duplicate_lanes = [
            lane("operation#1-2"),
            lane("operation#1-2"),
            lane("operation#1-3"),
            lane("operation#1-4"),
        ];
        let shared_references = [
            reference("operation#1-2", "block-4"),
            reference("operation#1-3", "block-4"),
            reference("operation#1-4", "block-4"),
        ];
        assert!(
            feature_simple_hole_construction_groups(&duplicate_lanes, &shared_references)
                .is_empty()
        );
    }

    #[test]
    fn nx_block_payload_points_require_exactly_two_named_scalars() {
        use super::{
            feature_block_payload_point_groups, feature_block_payload_points,
            FeatureBlockPayloadName, FeatureBlockPayloadNamedRecord, FeatureBlockPayloadScalar,
        };

        let operation_label = "operation".to_string();
        let construction_payload = "payload".to_string();
        let name = FeatureBlockPayloadName {
            id: "name".to_string(),
            operation_label: operation_label.clone(),
            construction_payload: construction_payload.clone(),
            ordinal: 0,
            type_code: Some(131),
            raw_type_code: Some(vec![0x80, 0x83]),
            type_code_payload_offset: Some(11),
            type_code_source_offset: Some(101),
            payload_leading: false,
            value: "Point7".to_string(),
            payload_offset: 10,
            source_offset: 100,
        };
        let scalar = |id: &str, ordinal: u32, value: f64| {
            let mut raw_value = value.to_be_bytes();
            raw_value[0] -= 0x10;
            FeatureBlockPayloadScalar {
                id: id.to_string(),
                operation_label: operation_label.clone(),
                construction_payload: construction_payload.clone(),
                ordinal,
                field_code: 100,
                value,
                raw_value,
                payload_offset: 20 + u64::from(ordinal) * 13,
                source_offset: 110 + u64::from(ordinal) * 13,
            }
        };
        let scalars = [scalar("first", 0, 1.25), scalar("second", 1, -2.5)];
        let record = FeatureBlockPayloadNamedRecord {
            id: "record".to_string(),
            operation_label,
            construction_payload,
            name_field: name.id.clone(),
            scalar_fields: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
            payload_start_offset: 10,
            payload_end_offset: 50,
        };

        let points = feature_block_payload_points(
            std::slice::from_ref(&record),
            std::slice::from_ref(&name),
            &scalars,
        );
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].name, "Point7");
        assert_eq!(points[0].coordinates, [1.25, -2.5]);

        let mut duplicate = points[0].clone();
        duplicate.id = "point-2".to_string();
        let groups = feature_block_payload_point_groups(&[points[0].clone(), duplicate]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].points.len(), 2);
        assert_eq!(groups[0].coordinates, [1.25, -2.5]);

        let mut conflicting = points[0].clone();
        conflicting.id = "conflicting".to_string();
        conflicting.coordinates[1] = f64::from_bits((-2.5_f64).to_bits() + 1);
        assert!(feature_block_payload_point_groups(&[points[0].clone(), conflicting]).is_empty());

        let mut incomplete = record.clone();
        incomplete.scalar_fields.pop();
        assert!(
            feature_block_payload_points(&[incomplete], std::slice::from_ref(&name), &scalars,)
                .is_empty()
        );
        let mut malformed = name;
        malformed.value = "Point0".to_string();
        assert!(feature_block_payload_points(&[record], &[malformed], &scalars).is_empty());
    }
}
