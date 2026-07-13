// SPDX-License-Identifier: Apache-2.0
//! Structural `AllFeatur` feature-to-generated-entity bindings.
//!
//! A mixed generated-entity table is `f8 <count> f7 1d fb e3`, followed by
//! exactly `<count>` compact entity identifiers, each terminated by `e3`.
//! `f7 1e` may prefix an entry. The table belongs to an `AllFeatur` row only
//! when its byte offset is bounded by that row's known feature-id header.

use std::collections::BTreeSet;

use crate::psb;
use crate::scalar;

/// Feature-operation family named by an `MdlStatus` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOperation {
    /// Numeric feature identifier following `id` in the stored name.
    pub feature_id: u32,
    /// Stored operation-family name.
    pub kind: String,
    /// Byte offset of the operation name in the original stream.
    pub offset: usize,
}

/// Decode NUL-terminated `<Kind> id <N>` operation names from one
/// `MdlStatus` payload.
pub fn operations(payload: &[u8]) -> Vec<FeatureOperation> {
    const SEPARATOR: &[u8] = b" id ";
    let family_byte = |byte: u8| {
        byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-' | b'/' | b'(' | b')')
    };
    let mut result = Vec::new();
    for separator in 0..payload.len().saturating_sub(SEPARATOR.len()) {
        if payload.get(separator..separator + SEPARATOR.len()) != Some(SEPARATOR) {
            continue;
        }
        let mut offset = separator;
        while offset > 0 && family_byte(payload[offset - 1]) {
            offset -= 1;
        }
        let family = &payload[offset..separator];
        if family.is_empty() || family.first() == Some(&b' ') || family.last() == Some(&b' ') {
            continue;
        }
        let digits = &payload[separator + SEPARATOR.len()..];
        let Some(end) = digits.iter().position(|byte| *byte == 0) else {
            continue;
        };
        if end == 0 || !digits[..end].iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(feature_id) = String::from_utf8_lossy(&digits[..end]).parse::<u32>() else {
            continue;
        };
        result.push(FeatureOperation {
            feature_id,
            kind: String::from_utf8_lossy(family).into_owned(),
            offset,
        });
    }
    result.sort_by_key(|operation| operation.offset);
    result.dedup_by_key(|operation| operation.feature_id);
    result
}

/// One `AllFeatur` mixed generated-entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityTable {
    /// Owning feature when the table lies in a bounded `AllFeatur` feature row.
    pub feature_id: Option<u32>,
    /// Entity identifiers in their declared generated-entity order.
    pub entry_ids: Vec<u32>,
    /// Entries that are materialized `srf_array` identifiers.
    pub surface_ids: Vec<u32>,
    /// Entries outside the materialized surface namespace.
    pub non_surface_entity_ids: Vec<u32>,
    /// Byte offset of the `f8` table opener in the original stream.
    pub offset: usize,
}

/// One byte-bounded positional `AllFeatur` row for a known geometry owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRow {
    /// Feature identifier decoded from the row prefix.
    pub feature_id: u32,
    /// Two-byte row-header family discriminator.
    pub header: [u8; 2],
    /// Row bytes after the compact feature identifier, ending before the next
    /// known feature row or at the end of the section.
    pub body: Vec<u8>,
    /// Byte offset of `body[0]` in the original stream.
    pub body_offset: usize,
    /// Byte offset of the feature identifier in the original stream.
    pub offset: usize,
}

/// One labeled procedural-choice span inside a known feature row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureChoice {
    /// Owning feature row identifier.
    pub feature_id: u32,
    /// Procedural choice label without its NUL terminator.
    pub label: String,
    /// Named-record type byte when the label has an `e0` header.
    pub type_byte: Option<u8>,
    /// Exact bytes from the label terminator to the next choice span.
    pub payload: Vec<u8>,
    /// Byte offset of `payload[0]` in the original stream.
    pub payload_offset: usize,
    /// Byte offset of the choice header or bare label in the original stream.
    pub offset: usize,
}

/// Byte-declared wrapper around one procedural choice field value.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureFieldValue {
    /// No payload bytes follow the field header.
    Empty,
    /// One compact integer occupying the complete field payload.
    CompactInt(u32),
    /// An `f8` count followed by exactly that many compact integers.
    CompactIntArray(Vec<u32>),
    /// One canonical `f7` entity reference, optionally followed by `fb`.
    EntityReference {
        /// Walker-order entity identifier.
        entity_id: u32,
        /// Whether an `fb` terminator follows the identifier.
        terminated: bool,
    },
    /// An `f9 <dimensions> <count>` scalar-array wrapper and its undecoded body.
    ScalarArray {
        /// Scalar tuple dimensionality from the wrapper.
        dimensions: u8,
        /// Number of scalar tuples from the wrapper.
        count: u8,
        /// Exact scalar-body bytes after the wrapper header.
        body: Vec<u8>,
        /// Values when exactly `dimensions × count` defined scalar tokens
        /// consume the complete body.
        decoded_values: Option<Vec<f64>>,
    },
    /// Bytes whose enclosing field is known but whose wrapper is not.
    Raw(Vec<u8>),
}

/// One named field bounded inside a procedural choice span.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureChoiceField {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Owning procedural choice label.
    pub choice_label: String,
    /// Field name from its named-record header.
    pub name: String,
    /// Named-record type byte.
    pub type_byte: u8,
    /// Structurally decoded field-value wrapper.
    pub value: FeatureFieldValue,
    /// Byte offset of the named-record header in the original stream.
    pub offset: usize,
}

/// Generated-geometry namespace declared inside a feature row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureGeometryTableKind {
    /// `edg_id_tab_ptr` edge identifiers.
    EdgeIds,
    /// `lo_id_tab_ptr` loop identifiers.
    LoopIds,
    /// `bnd_type` boundary records.
    Boundaries,
    /// `used_bodies` body references.
    UsedBodies,
    /// `geom_lists` geometry-list references.
    GeometryLists,
    /// `dtm_id_tab` datum identifiers.
    DatumIds,
}

/// One typed generated-geometry table header owned by a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureGeometryTable {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Declared namespace kind.
    pub kind: FeatureGeometryTableKind,
    /// Declared entry count.
    pub count: u32,
    /// Entity-class identifier following the `f7` marker.
    pub entity_class: u32,
    /// Byte offset of the field label in the original stream.
    pub offset: usize,
}

/// Namespace of IDs affected by a procedural feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedIdKind {
    /// `geoms_affected` geometry identifiers.
    Geometry,
    /// `edgs_affected` edge identifiers.
    Edges,
    /// `strong_parents` parent-feature identifiers.
    StrongParents,
    /// `parent_table` regeneration-parent feature identifiers.
    Parents,
    /// `contours` contour identifiers.
    Contours,
}

/// One complete affected-ID array owned by a feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureAffectedIds {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Affected namespace.
    pub kind: AffectedIdKind,
    /// Declared compact identifiers in stored order.
    pub ids: Vec<u32>,
    /// Byte offset of the named field header in the original stream.
    pub offset: usize,
}

/// Affected IDs recovered from an unlabeled positional replay recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureReplayAffectedIds {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Affected identifiers in replay order; geometry/edge partition is not
    /// implied by this combined sequence.
    pub ids: Vec<u32>,
    /// Whether the run contained an `f8 <count>` framing opener.
    pub has_count_opener: bool,
    /// Byte offset of the replay anchor in the original stream.
    pub offset: usize,
}

/// Which named direction lane supplied a recipe byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectionLane {
    /// `direction`.
    Primary,
    /// `direction2`.
    Secondary,
}

/// Interpretation permitted by the direction byte itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectionValue {
    /// Defined boolean side flag (`00` or `01`).
    SideFlag(bool),
    /// Any other raw byte; no side semantics are assigned.
    Raw(u8),
}

/// One named recipe direction byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDirectionByte {
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Primary or secondary direction lane.
    pub lane: DirectionLane,
    /// Byte interpretation.
    pub value: DirectionValue,
    /// Byte offset of the named field header in the original stream.
    pub offset: usize,
}

/// Definition-space parameter-frame field in a `FeatDefs` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureParameterFrameKind {
    /// `local_sys` frame field.
    LocalSystem,
    /// `transf` transform field.
    Transform,
}

/// One `f9 04 03` definition-space parameter frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureParameterFrame {
    /// Frame field kind.
    pub kind: FeatureParameterFrameKind,
    /// Exact scalar-body bytes after `f9 04 03`.
    pub body: Vec<u8>,
    /// Twelve values when the body consists entirely of defined scalar tokens.
    pub decoded_values: Option<Vec<f64>>,
    /// Byte offset of the field label in the original stream.
    pub offset: usize,
}

/// Feature-history phase associated with a local outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlinePhase {
    /// Labeled `outline` before rollback.
    PreRollback,
    /// Positional replay after rollback.
    PostRollback,
    /// Positional replay after regeneration.
    PostRegen,
}

/// Six-slot feature-local outline bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureOutline {
    /// Feature-history phase.
    pub phase: OutlinePhase,
    /// Six feature-local scalar slots; undefined prefixes remain `None`.
    pub local_values: Vec<Option<f64>>,
    /// Byte offset of the outline label in the original stream.
    pub offset: usize,
}

/// One positional solver-variable row from `var_arr`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVariableRow {
    /// Coordinate class: `1` is section `u`, `2` is section `v`.
    pub variable_type: u32,
    /// Point or solver-variable key.
    pub key: u32,
    /// Solved value when the scalar token is defined inline.
    pub value: Option<f64>,
    /// Pre-solve estimate when defined inline.
    pub guess: Option<f64>,
    /// Solver unknown identifier from the third trailing compact field.
    pub uvar_id: Option<u32>,
    /// Whether the value used the nine-byte dimension-driven sentinel.
    pub dimension_driven: bool,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// One section-frame point joined from `var_arr` type-1/type-2 rows.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSectionPoint {
    /// Shared variable-row key.
    pub point_id: u32,
    /// Section `u` coordinate.
    pub u: Option<f64>,
    /// Section `v` coordinate.
    pub v: Option<f64>,
}

/// Solved section-variable table from one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVariableTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table reference following the opener.
    pub entity_ref: Option<u32>,
    /// Positional variable rows in stored order.
    pub rows: Vec<FeatureVariableRow>,
    /// Section points joined by row key.
    pub points: Vec<FeatureSectionPoint>,
    /// Byte offset of the `var_arr` label in the original stream.
    pub offset: usize,
}

/// Defined positional segment family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSegmentKind {
    /// Type `2` line segment.
    Line,
    /// Type `3` circular-arc segment.
    Arc,
}

/// One positional `segtab_ptr` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegment {
    /// Line or arc discriminator.
    pub kind: FeatureSegmentKind,
    /// Three direction fields; control-range sentinels remain `None`.
    pub directions: [Option<u32>; 3],
    /// Endpoint IDs into the section variable table.
    pub point_ids: [u32; 2],
    /// Arc center point ID, or `None` for the null sentinel.
    pub center_id: Option<u32>,
    /// Arc orientation field.
    pub arc_orientation: Option<u32>,
    /// Vertical/horizontal constraint field.
    pub vertical_horizontal: Option<u32>,
    /// Radius reference field.
    pub radius_ref: Option<u32>,
    /// Secondary radius reference field.
    pub radius2_ref: Option<u32>,
    /// External segment identifier used by the order table.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// Defining-sketch segment table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegmentTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table reference following the opener.
    pub entity_ref: Option<u32>,
    /// Fully aligned line and arc rows.
    pub rows: Vec<FeatureSegment>,
    /// Byte offset of the `segtab_ptr` label in the original stream.
    pub offset: usize,
}

/// Solved/trimmed section entity family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEntityKind {
    /// No center vertex: trimmed line.
    Line,
    /// Center vertex present: trimmed circular arc.
    Arc,
}

/// One positional `ent_tab` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimEntity {
    /// External ID matching a `segtab` row.
    pub external_id: u32,
    /// Entity mode field.
    pub mode: Option<u32>,
    /// Solved start and end vertex IDs.
    pub vertices: [u32; 2],
    /// Solved center vertex ID for an arc.
    pub center_vertex: Option<u32>,
    /// Line or arc classification derived from center presence.
    pub kind: TrimEntityKind,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// Solved/trimmed entity graph for one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimEntityTable {
    /// Complete positional rows in stored order.
    pub rows: Vec<FeatureTrimEntity>,
    /// Sorted external IDs present in the trimmed profile.
    pub solved_external_ids: Vec<u32>,
    /// Byte offset of the `ent_tab` label in the original stream.
    pub offset: usize,
}

/// One solved trim vertex and the two trimmed entities incident to it.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertex {
    /// Vertex identifier shared with `ent_tab` endpoint and center fields.
    pub vertex_id: u32,
    /// Distinct `ent_tab` external entity identifiers meeting at the vertex.
    pub entities: [u32; 2],
    /// Solved section-frame coordinates for a nonparallel line-line junction.
    pub section_coordinates: Option<[f64; 2]>,
    /// Byte offset of the positional triple in the original stream.
    pub offset: usize,
}

/// Solved trim-vertex adjacency table for one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertexTable {
    /// Complete validated vertex rows in stored order.
    pub rows: Vec<FeatureTrimVertex>,
    /// Byte offset of the `vert_tab` label in the original stream.
    pub offset: usize,
}

/// One generated-entity ordering row from a gsec3d section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOrderRow {
    /// Section entity identifier matching a defining-sketch segment.
    pub external_id: u32,
    /// One-based position in the feature's generated-entity table.
    pub internal_id: u32,
    /// Orientation and side flags stored for the generated entity.
    pub bitmask: u32,
    /// Byte offset of the positional triple in the original stream.
    pub offset: usize,
}

/// Generated-entity ordering table for one gsec3d section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOrderTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional triples in stored order.
    pub rows: Vec<FeatureOrderRow>,
    /// Byte offset of the `order_table` label in the original stream.
    pub offset: usize,
}

impl FeatureOrderTable {
    /// Resolve a generated-entity position to its section entity identifier.
    pub fn external_id(&self, internal_id: u32) -> Option<u32> {
        self.rows
            .iter()
            .find(|row| row.internal_id == internal_id)
            .map(|row| row.external_id)
    }

    /// Resolve a section entity identifier to its generated-entity position.
    pub fn internal_id(&self, external_id: u32) -> Option<u32> {
        self.rows
            .iter()
            .find(|row| row.external_id == external_id)
            .map(|row| row.internal_id)
    }
}

/// Defined value of a one-byte binary section flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryFlag {
    /// Stored byte `00`.
    Clear,
    /// Stored byte `01`.
    Set,
}

impl BinaryFlag {
    fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clear),
            1 => Some(Self::Set),
            _ => None,
        }
    }
}

/// Reference fields that orient a gsec3d sketch frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeatureSectionOrientation {
    /// Section-side flip.
    pub section_flip: Option<BinaryFlag>,
    /// Orientation-reference type discriminator.
    pub reference_type: Option<u32>,
    /// Referenced sketch segment identifier.
    pub segment_id: Option<u32>,
    /// Referenced-plane flip.
    pub reference_flip: Option<BinaryFlag>,
}

/// Byte-backed gsec3d placement and ordering inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSection3d {
    /// Sketch-plane entity identifier.
    pub sketch_plane_entity_id: Option<u32>,
    /// Sketch-plane side flag.
    pub sketch_plane_flip: Option<BinaryFlag>,
    /// Entity references that orient the sketch plane.
    pub reference_plane_entity_ids: Vec<u32>,
    /// Geometry identifier joining the reference plane to its datum surface.
    pub reference_plane_datum_geometry_id: Option<u32>,
    /// Section-frame orientation reference fields.
    pub orientation: FeatureSectionOrientation,
    /// Stored dimension identifiers in section order.
    pub dimension_ids: Vec<u32>,
    /// Byte offset of the gsec3d record header in the original stream.
    pub offset: usize,
}

/// Interpretation of a stored feature-dimension value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionUnit {
    /// Type `0x0a` angle value stored in radians.
    Radians,
    /// Dimension type whose unit is defined by its enclosing section schema.
    SchemaDefined,
}

/// One dimension record from a gsec2d `dimtab_ptr` table.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDimension {
    /// Dimension type discriminator.
    pub dimension_type: u32,
    /// Decoded primary scalar, when its prefix is defined.
    pub value: Option<f64>,
    /// Unit interpretation selected by the dimension type.
    pub value_unit: DimensionUnit,
    /// Stored direction byte.
    pub direction_byte: u8,
    /// Decoded auxiliary scalar, when its prefix is defined.
    pub auxiliary_value: Option<f64>,
    /// External dimension identifier.
    pub external_id: u32,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// Dimension table for one gsec2d section.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDimensionTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Entity-table class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Labeled prototype followed by positional replay rows.
    pub rows: Vec<FeatureDimension>,
    /// Byte offset of the `dimtab_ptr` label in the original stream.
    pub offset: usize,
}

/// One positional constraint-relation row from `relat_ptr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelation {
    /// Relation identifier from the first positional field.
    pub relation_id: u32,
    /// Stored `used` field from the second positional field.
    pub used: u32,
    /// Complete positional fields before the `e2` row terminator.
    pub body: Vec<u8>,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// Counted `relat_ptr` constraint-relation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelationTable {
    /// Allocation count declared by the table's `f8` opener. Two entries are
    /// structural; positional row count is `declared_count - 2`.
    pub declared_count: u32,
    /// Relation entity-class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional relation rows in stored order.
    pub rows: Vec<FeatureRelation>,
    /// Byte offset of the `relat_ptr` label in the original stream.
    pub offset: usize,
}

/// One solved line retained in feature-definition section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedLine {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Entity references preceding or embedded in the record.
    pub references: Vec<u32>,
    /// Five-byte `eb` attribute payloads in stored order.
    pub attributes: Vec<[u8; 5]>,
    /// Two three-dimensional endpoints in the section sketch frame.
    pub endpoints: [[Option<f64>; 3]; 2],
    /// Byte offset of the record preamble in the original stream.
    pub offset: usize,
}

/// One solved circular arc retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedArc {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Arc center in the section sketch frame.
    pub center: [Option<f64>; 3],
    /// Arc radius.
    pub radius: Option<f64>,
    /// Trimmed arc endpoints in the section sketch frame.
    pub endpoints: [[Option<f64>; 3]; 2],
    /// Start and end curve parameters.
    pub parameters: [Option<f64>; 2],
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One solved circle retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedCircle {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Circle center in the section sketch frame.
    pub center: [Option<f64>; 3],
    /// Circle radius.
    pub radius: Option<f64>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One saved placeholder entity without analytic geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSavedDummy {
    /// Saved-section entity identifier, when stored.
    pub entity_id: Option<u32>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// Solved saved-section entity with kind-specific valid fields.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureSavedEntity {
    /// Saved straight-line entity.
    Line(FeatureSavedLine),
    /// Saved circular-arc entity.
    Arc(FeatureSavedArc),
    /// Saved full-circle entity.
    Circle(FeatureSavedCircle),
    /// Saved non-geometric placeholder.
    Dummy(FeatureSavedDummy),
}

/// Solved entity table stored below `p_saved_result`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedSection {
    /// Solved entities in stored table order.
    pub entities: Vec<FeatureSavedEntity>,
    /// Byte offset of the `p_saved_result` record header in the original stream.
    pub offset: usize,
}

/// One byte-bounded `feat_defs_<id>` feature-definition template.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDefinition {
    /// Numeric identifier embedded in the record name.
    pub id: u32,
    /// Unique named `feat_id` in the bounded definition body, joining the
    /// definition to its modeling feature. Definitions with zero or multiple
    /// distinct values have no owner binding.
    pub owner_feature_id: Option<u32>,
    /// Exact record bytes through the next feature definition or section end.
    pub body: Vec<u8>,
    /// Definition-space local-system and transform fields.
    pub parameter_frames: Vec<FeatureParameterFrame>,
    /// Feature-local outline records in history order.
    pub outlines: Vec<FeatureOutline>,
    /// Section solver-variable table, when present and structurally valid.
    pub variables: Option<FeatureVariableTable>,
    /// Defining-sketch segment table, when present and structurally valid.
    pub segments: Option<FeatureSegmentTable>,
    /// Solved/trimmed entity graph, when present and structurally valid.
    pub trim_entities: Option<FeatureTrimEntityTable>,
    /// Solved trim-vertex adjacency, when present and structurally valid.
    pub trim_vertices: Option<FeatureTrimVertexTable>,
    /// gsec3d generated-entity ordering, when present and structurally valid.
    pub order_table: Option<FeatureOrderTable>,
    /// gsec3d placement and ordering inputs, when present.
    pub section_3d: Option<FeatureSection3d>,
    /// gsec2d dimension table, when present and structurally valid.
    pub dimensions: Option<FeatureDimensionTable>,
    /// gsec2d constraint-relation table, when present and structurally valid.
    pub relations: Option<FeatureRelationTable>,
    /// Solved saved-section entities, when present and structurally valid.
    pub saved_section: Option<FeatureSavedSection>,
    /// Byte offset of the record name in the original stream.
    pub offset: usize,
}

/// One named record in the implicit `AllFeatur` walker-order entity table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntity {
    /// Zero-based walker-order identifier used by `f7` references.
    pub entity_id: u32,
    /// Named-record type byte.
    pub type_byte: u8,
    /// NUL-terminated named-record name.
    pub name: String,
    /// Byte offset of the `e0` header in the original stream.
    pub offset: usize,
}

/// One `f7 <id>` reference in `AllFeatur`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEntityReference {
    /// Walker-order entity containing this token, when one precedes it.
    pub source_entity_id: Option<u32>,
    /// Referenced walker-order entity identifier.
    pub target_entity_id: u32,
    /// Whether the target identifier exists in the decoded entity table.
    pub target_resolved: bool,
    /// Byte offset of the `f7` token in the original stream.
    pub offset: usize,
}

const TABLE_TAG: &[u8] = &[0xf7, 0x1d, 0xfb, 0xe3];
const ROW_HEADERS: &[&[u8]] = &[&[0xeb, 0x04], &[0x90, 0x01], &[0xc8, 0x10]];
const CHOICE_LABELS: &[&[u8]] = &[
    b"blend_choice",
    b"depth_choice",
    b"angle_choice",
    b"pat_choice",
    b"round_choice",
    b"subsec_choice",
    b"sweep_choice",
    b"dome_choice",
    b"draft_choice",
    b"misc_choice",
];

fn row_spans(payload: &[u8], feature_ids: &BTreeSet<u32>) -> Vec<(usize, usize, u32)> {
    let mut starts = Vec::new();
    for offset in 0..payload.len() {
        let Ok((id, after)) = psb::reference_id(payload, offset) else {
            continue;
        };
        if feature_ids.contains(&id)
            && ROW_HEADERS
                .iter()
                .any(|header| payload.get(after..after + header.len()) == Some(*header))
        {
            starts.push((offset, id));
        }
    }
    starts.sort_unstable();
    starts.dedup_by_key(|(_, id)| *id);
    starts
        .iter()
        .enumerate()
        .map(|(index, &(start, id))| {
            let end = starts
                .get(index + 1)
                .map_or(payload.len(), |&(next, _)| next);
            (start, end, id)
        })
        .collect()
}

/// Decode positional `AllFeatur` rows whose feature identifiers are proven by
/// geometry ownership. Unknown feature-like byte sequences remain unclaimed.
pub fn rows(payload: &[u8], feature_ids: &BTreeSet<u32>) -> Vec<FeatureRow> {
    row_spans(payload, feature_ids)
        .into_iter()
        .filter_map(|(start, end, feature_id)| {
            let (_, body_start) = psb::reference_id(payload, start).ok()?;
            let header = payload.get(body_start..body_start + 2)?.try_into().ok()?;
            Some(FeatureRow {
                feature_id,
                header,
                body: payload.get(body_start..end)?.to_vec(),
                body_offset: body_start,
                offset: start,
            })
        })
        .collect()
}

/// Bound recognized procedural-choice labels within decoded feature rows.
pub fn choices(rows: &[FeatureRow]) -> Vec<FeatureChoice> {
    let mut result = Vec::new();
    for row in rows {
        let mut hits = Vec::new();
        for &label in CHOICE_LABELS {
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(label.len() + 1)
                    .position(|window| window == [label, b"\0"].concat())
            }) {
                let label_offset = from + relative;
                let (header_offset, type_byte) = if label_offset >= 2
                    && row.body[label_offset - 2] == psb::token::NAMED_RECORD
                {
                    (label_offset - 2, Some(row.body[label_offset - 1]))
                } else {
                    (label_offset, None)
                };
                hits.push((header_offset, label_offset, label, type_byte));
                from = label_offset + label.len() + 1;
            }
        }
        hits.sort_by_key(|hit| hit.0);
        for (index, &(header, label_at, label, type_byte)) in hits.iter().enumerate() {
            let value = label_at + label.len() + 1;
            let end = hits.get(index + 1).map_or(row.body.len(), |hit| hit.0);
            result.push(FeatureChoice {
                feature_id: row.feature_id,
                label: String::from_utf8_lossy(label).into_owned(),
                type_byte,
                payload: row.body[value..end].to_vec(),
                payload_offset: row.body_offset + value,
                offset: row.body_offset + header,
            });
        }
    }
    result.sort_by_key(|choice| choice.offset);
    result
}

fn decode_exact_scalars(
    payload: &[u8],
    slot_count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<f64>> {
    let mut cursor = 0;
    let mut values = Vec::with_capacity(slot_count);
    for _ in 0..slot_count {
        let (value, next) = scalar::decode_in_lane(payload, cursor, cache)?;
        values.push(value);
        cursor = next;
    }
    (cursor == payload.len()).then_some(values)
}

fn decode_optional_scalars(
    payload: &[u8],
    mut cursor: usize,
    count: usize,
    cache: &scalar::ScalarCache,
) -> Vec<Option<f64>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, cache) {
            values.push(Some(value));
            cursor = next;
        } else if cursor < payload.len() {
            values.push(None);
            cursor += 1;
        } else {
            values.push(None);
        }
    }
    values
}

fn find_bytes(payload: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    payload
        .get(start..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| start + relative)
}

fn decode_parameter_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    if let Some((value, next)) =
        scalar::decode_in_lane(payload, offset, cache).filter(|(_, next)| *next <= end)
    {
        return Some((value, next));
    }
    let (first, second) = match payload.get(offset)? {
        0x74 => (0x3f, 0xe9),
        0x81 => (0x3f, 0xf6),
        0xb7 => (0x3f, 0xe4),
        _ => return None,
    };
    let tail = payload
        .get(offset + 1..offset + 7)
        .filter(|_| offset + 7 <= end)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1] = second;
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

fn decode_variable_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize, bool) {
    let Some(&prefix) = payload.get(offset).filter(|_| offset < end) else {
        return (None, offset, false);
    };
    if matches!(prefix, 0x90 | 0xd7) && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(if prefix == 0x90 {
            &[0x40, 0x05]
        } else {
            &[0xc0, 0x05]
        });
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x18 | 0xe0 | 0xe2 | 0xe3 | 0x10 | 0xe4 | 0xe6))
    {
        return (Some(0.0), offset + 1, false);
    }
    if prefix == 0xed && offset + 9 <= end {
        return (None, offset + 9, true);
    }
    decode_parameter_scalar(payload, offset, end, cache)
        .map_or((None, offset + 1, false), |(value, next)| {
            (Some(value), next, false)
        })
}

fn variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureVariableTable> {
    let table = find_bytes(payload, b"var_arr\0", start, end)?;
    let mut cursor = table + b"var_arr\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end)?;
    let (_, after_close_ref) = psb::compact_int(payload, close + 2);
    cursor = after_close_ref;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    let max_rows = usize::try_from(declared_count)
        .unwrap_or(usize::MAX)
        .saturating_add(64)
        .min(end.saturating_sub(cursor));
    while cursor < end && rows.len() < max_rows {
        if payload[cursor] == 0xe2 {
            cursor += 1;
            continue;
        }
        if payload[cursor] >= 0xc0 {
            break;
        }
        let row_offset = cursor;
        let (variable_type, next) = psb::compact_int(payload, cursor);
        cursor = next;
        if cursor >= end || payload[cursor] >= 0xc0 {
            break;
        }
        let (key, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (value, next, dimension_driven) = decode_variable_scalar(payload, cursor, end, cache);
        cursor = next;
        let (guess, next, _) = decode_variable_scalar(payload, cursor, end, cache);
        cursor = next;
        let mut trailing = Vec::new();
        while cursor < end && payload[cursor] != 0xe2 && trailing.len() < 3 {
            if payload[cursor] >= 0xc0 {
                break;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            if next == cursor {
                break;
            }
            trailing.push(field);
            cursor = next;
        }
        rows.push(FeatureVariableRow {
            variable_type,
            key,
            value,
            guess,
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        });
        let delimiter = payload[cursor..end].iter().position(|&byte| byte == 0xe2)?;
        cursor += delimiter + 1;
    }
    (!rows.is_empty()).then(|| {
        let mut coordinates = std::collections::BTreeMap::<u32, (Option<f64>, Option<f64>)>::new();
        for row in &rows {
            let point = coordinates.entry(row.key).or_insert((None, None));
            match row.variable_type {
                1 if row.value.is_some() => point.0 = row.value,
                2 if row.value.is_some() => point.1 = row.value,
                _ => {}
            }
        }
        FeatureVariableTable {
            declared_count,
            entity_ref,
            rows,
            points: coordinates
                .into_iter()
                .map(|(point_id, (u, v))| FeatureSectionPoint { point_id, u, v })
                .collect(),
            offset: table,
        }
    })
}

fn segment_int(payload: &[u8], offset: usize) -> (Option<u32>, usize) {
    let Some(&head) = payload.get(offset) else {
        return (None, offset);
    };
    match head {
        0..=0x7f => (Some(u32::from(head)), offset + 1),
        0x80..=0xbf => payload.get(offset + 1).map_or((None, offset + 1), |&tail| {
            (
                Some((u32::from(head - 0x80) << 8) | u32::from(tail)),
                offset + 2,
            )
        }),
        _ => (None, offset + 1),
    }
}

fn next_segment_int(payload: &[u8], offset: &mut usize) -> Option<u32> {
    let (value, next) = segment_int(payload, *offset);
    *offset = next;
    value
}

fn segment_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureSegmentTable> {
    let table = find_bytes(payload, b"segtab_ptr\0", start, end)?;
    let mut cursor = table + b"segtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let close = find_bytes(payload, &[0xf2, psb::token::ENTITY_REF], cursor, end)?;
    let (_, after_close_ref) = psb::compact_int(payload, close + 2);
    cursor = after_close_ref;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let region_end = find_bytes(payload, b"order_table", cursor, end).unwrap_or(end);
    let mut rows = Vec::new();
    while cursor < region_end {
        let chunk_end = payload[cursor..region_end]
            .iter()
            .position(|&byte| byte == 0xe3)
            .map_or(region_end, |relative| cursor + relative);
        let chunk = &payload[cursor..chunk_end];
        let Some(row_start) = chunk.iter().position(|byte| matches!(byte, 2 | 3)) else {
            cursor = chunk_end.saturating_add(1);
            continue;
        };
        let mut p = row_start;
        let (kind_raw, next) = segment_int(chunk, p);
        p = next;
        let kind = match kind_raw {
            Some(2) => FeatureSegmentKind::Line,
            Some(3) => FeatureSegmentKind::Arc,
            _ => {
                cursor = chunk_end.saturating_add(1);
                continue;
            }
        };
        let directions = [
            next_segment_int(chunk, &mut p),
            next_segment_int(chunk, &mut p),
            next_segment_int(chunk, &mut p),
        ];
        let (Some(point0), Some(point1)) = (
            next_segment_int(chunk, &mut p),
            next_segment_int(chunk, &mut p),
        ) else {
            cursor = chunk_end.saturating_add(1);
            continue;
        };
        let center_id = next_segment_int(chunk, &mut p);
        let arc_orientation = next_segment_int(chunk, &mut p);
        let verhor_flag = chunk.get(p) == Some(&0xf5);
        let vertical_horizontal = next_segment_int(chunk, &mut p);
        if verhor_flag {
            let _ = next_segment_int(chunk, &mut p);
        }
        let radius_ref = next_segment_int(chunk, &mut p);
        let radius2_ref = next_segment_int(chunk, &mut p);
        let Some(external_id) = next_segment_int(chunk, &mut p).filter(|id| *id != 0) else {
            cursor = chunk_end.saturating_add(1);
            continue;
        };
        if chunk.get(p) == Some(&0xe2) && point0 < 256 && point1 < 256 {
            rows.push(FeatureSegment {
                kind,
                directions,
                point_ids: [point0, point1],
                center_id,
                arc_orientation,
                vertical_horizontal,
                radius_ref,
                radius2_ref,
                external_id,
                offset: cursor + row_start,
            });
        }
        cursor = chunk_end.saturating_add(1);
    }
    (!rows.is_empty()).then_some(FeatureSegmentTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

fn trim_entity_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureTrimEntityTable> {
    let table = find_bytes(payload, b"ent_tab\0", start, end)?;
    let prototype = find_bytes(payload, b"entry_ptr(entity_entry)", table, end)?;
    let close = find_bytes(payload, &[0xf2, psb::token::ENTITY_REF], prototype, end)?;
    let (_, mut cursor) = psb::compact_int(payload, close + 2);
    if payload.get(cursor) == Some(&0xe3) {
        cursor += 1;
    }
    let region_end = find_bytes(payload, b"vert_tab", cursor, end).unwrap_or(end);
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    while cursor < region_end {
        let chunk_end = payload[cursor..region_end]
            .iter()
            .position(|&byte| byte == 0xe3)
            .map_or(region_end, |relative| cursor + relative);
        let chunk = &payload[cursor..chunk_end];
        let mut p = 0;
        let external_id = next_segment_int(chunk, &mut p);
        let mode = next_segment_int(chunk, &mut p);
        let start_vertex = next_segment_int(chunk, &mut p);
        let end_vertex = next_segment_int(chunk, &mut p);
        let center_vertex = next_segment_int(chunk, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && seen.insert(external_id) {
                rows.push(FeatureTrimEntity {
                    external_id,
                    mode,
                    vertices: [start_vertex, end_vertex],
                    center_vertex,
                    kind: if center_vertex.is_some() {
                        TrimEntityKind::Arc
                    } else {
                        TrimEntityKind::Line
                    },
                    offset: cursor,
                });
            }
        }
        cursor = chunk_end.saturating_add(1);
    }
    (!rows.is_empty()).then(|| FeatureTrimEntityTable {
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

fn trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    entities: Option<&FeatureTrimEntityTable>,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let table = find_bytes(payload, b"vert_tab\0", start, end)?;
    let chains_end = table
        .saturating_add(b"vert_tab\0".len())
        .saturating_add(120)
        .min(end);
    let chains = find_bytes(payload, b"chains\0", table, chains_end)?;
    let mut cursor = chains + b"chains\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (_, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let reference_start = cursor + 1;
    let (_, reference_end) = psb::reference_id(payload, reference_start).ok()?;
    let mut block_marker = vec![0xf3, psb::token::ENTITY_REF];
    block_marker.extend_from_slice(payload.get(reference_start..reference_end)?);
    block_marker.push(0xe2);
    cursor = find_bytes(payload, &block_marker, reference_end, end)?;

    let valid_entities = entities.map(|table| {
        table
            .rows
            .iter()
            .map(|row| row.external_id)
            .collect::<BTreeSet<_>>()
    });
    let valid_vertices = entities.map(|table| {
        table
            .rows
            .iter()
            .flat_map(|row| row.vertices.into_iter().chain(row.center_vertex))
            .collect::<BTreeSet<_>>()
    });
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    while cursor < end {
        if payload.get(cursor..cursor + block_marker.len()) == Some(block_marker.as_slice()) {
            cursor += block_marker.len();
            let (_, next) = segment_int(payload, cursor);
            cursor = next;
            continue;
        }
        match payload[cursor] {
            psb::token::ARRAY_OPEN => {
                let (_, next) = psb::compact_int(payload, cursor + 1);
                cursor = next;
                continue;
            }
            psb::token::ENTITY_REF => {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    cursor += 1;
                    continue;
                };
                cursor = next;
                continue;
            }
            0x00 | 0xf1 | 0xe2 | 0xe3 | 0xfb => {
                cursor += 1;
                continue;
            }
            _ => {}
        }
        let row_offset = cursor;
        let (entity_1, next) = segment_int(payload, cursor);
        let (entity_2, next) = segment_int(payload, next);
        let (vertex_id, next) = segment_int(payload, next);
        let (Some(entity_1), Some(entity_2), Some(vertex_id)) = (entity_1, entity_2, vertex_id)
        else {
            cursor += 1;
            continue;
        };
        if payload.get(next) != Some(&0) {
            cursor += 1;
            continue;
        }
        let valid = match (&valid_entities, &valid_vertices) {
            (Some(entity_ids), Some(vertex_ids)) => {
                entity_ids.contains(&entity_1)
                    && entity_ids.contains(&entity_2)
                    && vertex_ids.contains(&vertex_id)
            }
            _ => entity_1 > 2 && entity_2 > 2,
        };
        if valid && entity_1 != entity_2 && seen.insert(vertex_id) {
            rows.push(FeatureTrimVertex {
                vertex_id,
                entities: [entity_1, entity_2],
                section_coordinates: line_intersection([entity_1, entity_2], segments, variables),
                offset: row_offset,
            });
        }
        cursor = next + 1;
    }
    (!rows.is_empty()).then_some(FeatureTrimVertexTable {
        rows,
        offset: table,
    })
}

fn line_intersection(
    entity_ids: [u32; 2],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    let segments = segments?;
    let variables = variables?;
    let segment = |external_id| {
        segments.rows.iter().find(|segment| {
            segment.external_id == external_id && segment.kind == FeatureSegmentKind::Line
        })
    };
    let point = |point_id| {
        variables
            .points
            .iter()
            .find(|point| point.point_id == point_id)
            .and_then(|point| Some([point.u?, point.v?]))
    };
    let first = segment(entity_ids[0])?;
    let second = segment(entity_ids[1])?;
    let [x1, y1] = point(first.point_ids[0])?;
    let [x2, y2] = point(first.point_ids[1])?;
    let [x3, y3] = point(second.point_ids[0])?;
    let [x4, y4] = point(second.point_ids[1])?;
    let denominator = (x1 - x2).mul_add(y3 - y4, -(y1 - y2) * (x3 - x4));
    if denominator == 0.0 {
        return None;
    }
    let first_cross = x1.mul_add(y2, -(y1 * x2));
    let second_cross = x3.mul_add(y4, -(y3 * x4));
    Some([
        first_cross.mul_add(x3 - x4, -(x1 - x2) * second_cross) / denominator,
        first_cross.mul_add(y3 - y4, -(y1 - y2) * second_cross) / denominator,
    ])
}

fn order_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureOrderTable> {
    let table = find_bytes(payload, b"order_table\0", start, end)?;
    let mut cursor = table + b"order_table\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::reference_id(payload, cursor + 1).ok()?;
        cursor = next;
        Some(value)
    } else {
        None
    };
    let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end)?;
    let (_, next) = psb::reference_id(payload, close + 2).ok()?;
    cursor = next;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    let mut external_ids = BTreeSet::new();
    let mut internal_ids = BTreeSet::new();
    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    while cursor < end && rows.len() < row_limit {
        if payload[cursor] == 0xe2 {
            cursor += 1;
            continue;
        }
        if matches!(payload[cursor], 0xe0 | 0xf1) {
            break;
        }
        let row_offset = cursor;
        let (external_id, next) = segment_int(payload, cursor);
        let (internal_id, next) = segment_int(payload, next);
        let (bitmask, next) = segment_int(payload, next);
        let (Some(external_id), Some(internal_id), Some(bitmask)) =
            (external_id, internal_id, bitmask)
        else {
            break;
        };
        if payload.get(next) != Some(&0xe2)
            || !external_ids.insert(external_id)
            || !internal_ids.insert(internal_id)
        {
            break;
        }
        rows.push(FeatureOrderRow {
            external_id,
            internal_id,
            bitmask,
            offset: row_offset,
        });
        cursor = next + 1;
    }
    (!rows.is_empty()).then_some(FeatureOrderTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

fn named_compact_int(payload: &[u8], label: &[u8], start: usize, end: usize) -> Option<u32> {
    let at = find_bytes(payload, label, start, end)? + label.len();
    let (value, next) = segment_int(payload, at);
    value.filter(|_| next <= end)
}

fn section_3d(payload: &[u8], start: usize, end: usize) -> Option<FeatureSection3d> {
    let section = find_bytes(payload, b"\xe0\x00gsec3d_ptr\0", start, end)?;
    let nearby_end = section.saturating_add(260).min(end);
    let sketch_plane_entity_id = named_compact_int(payload, b"plane_id\0", section, nearby_end);
    let sketch_plane_flip = find_bytes(payload, b"plane_flip\0", section, nearby_end)
        .and_then(|at| payload.get(at + b"plane_flip\0".len()).copied())
        .and_then(BinaryFlag::decode);

    let mut reference_plane_entity_ids = Vec::new();
    let mut reference_plane_datum_geometry_id = None;
    if let Some(references) = find_bytes(payload, b"\xe0\x00ref_planes\0", section, nearby_end) {
        let mut cursor = references + b"\xe0\x00ref_planes\0".len();
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            for _ in 0..count {
                if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
                    break;
                }
                let Ok((entity_id, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                reference_plane_entity_ids.push(entity_id);
                cursor = next;
            }
            let nested_end = cursor.saturating_add(48).min(end);
            reference_plane_datum_geometry_id =
                named_compact_int(payload, b"\xe0\x01plane_id\0", cursor, nested_end);
        }
    }

    let placement_end = find_bytes(payload, b"\xe0\x00p_saved_result\0", section, end)
        .unwrap_or_else(|| section.saturating_add(400).min(end));
    let named_flag = |label: &[u8]| {
        find_bytes(payload, label, section, placement_end)
            .and_then(|at| payload.get(at + label.len()).copied())
            .and_then(BinaryFlag::decode)
    };
    let orientation = FeatureSectionOrientation {
        section_flip: named_flag(b"\xe0\x01flip\0"),
        reference_type: named_compact_int(payload, b"\xe0\x01ref_type\0", section, placement_end),
        segment_id: named_compact_int(payload, b"\xe0\x01seg_id\0", section, placement_end),
        reference_flip: named_flag(b"\xe0\x01flip_flag\0"),
    };

    let mut dimension_ids = Vec::new();
    if let Some(table) = find_bytes(payload, b"dim_id_tab\0", section, end) {
        let mut cursor = table + b"dim_id_tab\0".len();
        while payload
            .get(cursor)
            .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
        {
            cursor += 1;
        }
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            for _ in 0..count {
                let (Some(value), next) = segment_int(payload, cursor) else {
                    break;
                };
                dimension_ids.push(value);
                cursor = next;
            }
        }
    }
    Some(FeatureSection3d {
        sketch_plane_entity_id,
        sketch_plane_flip,
        reference_plane_entity_ids,
        reference_plane_datum_geometry_id,
        orientation,
        dimension_ids,
        offset: section,
    })
}

fn dimension_unit(dimension_type: u32) -> DimensionUnit {
    if dimension_type == 0x0a {
        DimensionUnit::Radians
    } else {
        DimensionUnit::SchemaDefined
    }
}

fn labeled_dimension(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimension> {
    let type_label = find_bytes(payload, b"type\0", start, end)?;
    let (dimension_type, after_type) = segment_int(payload, type_label + b"type\0".len());
    let dimension_type = dimension_type?;
    let value_label = find_bytes(payload, b"value\0", after_type, end)?;
    let (value, after_value, _) =
        decode_variable_scalar(payload, value_label + b"value\0".len(), end, cache);
    let direction_label = find_bytes(payload, b"direct\0", after_value, end)?;
    let direction_byte = *payload.get(direction_label + b"direct\0".len())?;
    let auxiliary_label = find_bytes(payload, b"aux_value\0", direction_label, end)?;
    let (auxiliary_value, after_auxiliary, _) =
        decode_variable_scalar(payload, auxiliary_label + b"aux_value\0".len(), end, cache);
    let external_label = find_bytes(payload, b"ext_id\0", after_auxiliary, end)?;
    let (external_id, _) = segment_int(payload, external_label + b"ext_id\0".len());
    Some(FeatureDimension {
        dimension_type,
        value,
        value_unit: dimension_unit(dimension_type),
        direction_byte,
        auxiliary_value,
        external_id: external_id?,
        offset: type_label,
    })
}

fn positional_dimension(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimension> {
    let (dimension_type, cursor) = segment_int(payload, start);
    let dimension_type = dimension_type?;
    let (value, cursor, _) = decode_variable_scalar(payload, cursor, end, cache);
    let direction_byte = *payload.get(cursor).filter(|_| cursor < end)?;
    let auxiliary_start = cursor + 1;
    let (auxiliary_value, cursor) = if payload.get(auxiliary_start) == Some(&0x18) {
        (Some(0.0), auxiliary_start + 1)
    } else {
        let (value, next, _) = decode_variable_scalar(payload, auxiliary_start, end, cache);
        (value, next)
    };
    let (external_id, _) = segment_int(payload, cursor);
    Some(FeatureDimension {
        dimension_type,
        value,
        value_unit: dimension_unit(dimension_type),
        direction_byte,
        auxiliary_value,
        external_id: external_id?,
        offset: start,
    })
}

fn dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimensionTable> {
    let table = find_bytes(payload, b"dimtab_ptr\0", start, end)?;
    let mut cursor = table + b"dimtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let mut reference_bytes = None;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let reference_start = cursor + 1;
        let (value, next) = psb::reference_id(payload, reference_start).ok()?;
        reference_bytes = payload.get(reference_start..next).map(<[u8]>::to_vec);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let region_end = find_bytes(payload, b"\xe0\x00relat_ptr\0", cursor, end).unwrap_or(end);
    let mut separator = vec![0xf3, psb::token::ENTITY_REF];
    if let Some(bytes) = &reference_bytes {
        separator.extend_from_slice(bytes);
    }
    separator.push(0xe2);
    let first_end = if reference_bytes.is_some() {
        find_bytes(payload, &separator, cursor, region_end).unwrap_or(region_end)
    } else {
        region_end
    };
    let mut rows = Vec::new();
    if let Some(row) = labeled_dimension(payload, cursor, first_end, cache) {
        rows.push(row);
    }
    if reference_bytes.is_some() {
        let mut replay = first_end;
        while replay < region_end
            && rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX)
        {
            if payload.get(replay..replay + separator.len()) != Some(separator.as_slice()) {
                break;
            }
            replay += separator.len();
            let next_separator =
                find_bytes(payload, &separator, replay, region_end).unwrap_or(region_end);
            let Some(row) = positional_dimension(payload, replay, next_separator, cache) else {
                break;
            };
            rows.push(row);
            replay = next_separator;
        }
    }
    (!rows.is_empty()).then_some(FeatureDimensionTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

fn relation_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureRelationTable> {
    let table = find_bytes(payload, b"relat_ptr\0", start, end)?;
    let mut cursor = table + b"relat_ptr\0".len();
    if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x04]) {
        cursor += 2;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::reference_id(payload, cursor + 1).ok()?;
        cursor = next;
        Some(value)
    } else {
        None
    };
    if payload.get(cursor) == Some(&psb::token::ARRAY_CLOSE) {
        cursor += 1;
    }
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end)?;
    let (_, after_ref) = psb::reference_id(payload, close + 2).ok()?;
    (payload.get(after_ref) == Some(&0xe2)).then_some(())?;
    cursor = after_ref + 1;

    let mut rows = Vec::new();
    for _ in 0..declared_count.saturating_sub(2) {
        let row_end = payload[cursor..end].iter().position(|byte| *byte == 0xe2)? + cursor;
        let (relation_id, after_id) = psb::compact_int(payload, cursor);
        (after_id > cursor && after_id < row_end).then_some(())?;
        let (used, after_used) = psb::compact_int(payload, after_id);
        (after_used > after_id && after_used <= row_end).then_some(())?;
        rows.push(FeatureRelation {
            relation_id,
            used,
            body: payload[cursor..row_end].to_vec(),
            offset: cursor,
        });
        cursor = row_end + 1;
    }
    Some(FeatureRelationTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

fn saved_section_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize) {
    let Some(&prefix) = payload.get(offset).filter(|_| offset < end) else {
        return (None, offset);
    };
    if matches!(prefix, 0x90 | 0xd7) && offset + 7 <= end {
        return (None, offset + 7);
    }
    if prefix == 0x99 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0xc0, 0x0e]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0xd5 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0xbf;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    scalar::decode_in_lane(payload, offset, cache)
        .filter(|(_, next)| *next <= end)
        .map_or((None, offset + 1), |(value, next)| (Some(value), next))
}

fn saved_line_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    let Some(label) = find_bytes(payload, b"\xe0\x00entity(line)\0", start, end) else {
        return Vec::new();
    };
    let mut cursor = label + b"\xe0\x00entity(line)\0".len();
    let segment_end = find_bytes(payload, b"\xe0\x00entity(", cursor, end).unwrap_or(end);
    if payload.get(cursor) == Some(&0xe3)
        && payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)
    {
        if let Ok((_, after_first)) = psb::reference_id(payload, cursor + 2) {
            if payload.get(after_first) == Some(&0)
                && payload.get(after_first + 1) == Some(&psb::token::ENTITY_REF)
            {
                if let Ok((_, after_second)) = psb::reference_id(payload, after_first + 2) {
                    if payload.get(after_second) == Some(&0xe2) {
                        cursor = after_second + 1;
                    }
                }
            }
        }
    }
    let mut entities = Vec::new();
    while cursor < segment_end {
        if payload.get(cursor) == Some(&psb::token::NAMED_RECORD)
            || payload.get(cursor..cursor + 2) == Some(&[0xf1, 0xe1])
        {
            break;
        }
        let record_offset = cursor;
        let mut references = Vec::new();
        let mut attributes = Vec::new();
        loop {
            if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                references.push(reference);
                cursor = next;
            } else if payload.get(cursor) == Some(&0xeb) {
                let Some(bytes) = payload.get(cursor + 1..cursor + 6) else {
                    break;
                };
                let mut attribute = [0; 5];
                attribute.copy_from_slice(bytes);
                attributes.push(attribute);
                cursor += 6;
            } else {
                break;
            }
        }
        let (Some(entity_id), next) = segment_int(payload, cursor) else {
            cursor += 1;
            continue;
        };
        if payload.get(next) != Some(&0xe2) {
            cursor += 1;
            continue;
        }
        cursor = next + 1;
        let mut values = Vec::with_capacity(6);
        while cursor < segment_end && values.len() < 6 {
            if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                references.push(reference);
                cursor = next;
                continue;
            }
            if payload.get(cursor) == Some(&0xeb) {
                let Some(bytes) = payload.get(cursor + 1..cursor + 6) else {
                    break;
                };
                let mut attribute = [0; 5];
                attribute.copy_from_slice(bytes);
                attributes.push(attribute);
                cursor += 6;
                continue;
            }
            if payload.get(cursor) == Some(&0xe2) {
                cursor += 1;
                continue;
            }
            let (value, next) = saved_section_scalar(payload, cursor, segment_end, cache);
            if next <= cursor {
                break;
            }
            values.push(value);
            cursor = next;
        }
        if values.len() != 6 || payload.get(cursor) != Some(&0xe3) {
            break;
        }
        cursor += 1;
        entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
            entity_id,
            references,
            attributes,
            endpoints: [
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            ],
            offset: record_offset,
        }));
    }
    entities
}

fn saved_named_scalars<const N: usize>(
    payload: &[u8],
    field: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<[Option<f64>; N]> {
    let mut label = vec![0xe0, 0x02];
    label.extend_from_slice(field);
    label.push(0);
    let mut cursor = find_bytes(payload, &label, start, end)? + label.len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        (usize::try_from(count).ok()? == N).then_some(())?;
        cursor = next;
    }
    let mut values = [None; N];
    for value in &mut values {
        let (decoded, next) = saved_section_scalar(payload, cursor, end, cache);
        (next > cursor).then_some(())?;
        *value = decoded;
        cursor = next;
    }
    Some(values)
}

fn saved_entity_id(payload: &[u8], start: usize, end: usize) -> Option<u32> {
    named_compact_int(payload, b"\xe0\x01id\0", start, end)
}

fn saved_circular_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    let mut entities = Vec::new();
    for (kind, label) in [
        ("arc", b"\xe0\x00entity(arc)\0".as_slice()),
        ("circle", b"\xe0\x00entity(circle)\0".as_slice()),
    ] {
        let mut search = start;
        while let Some(entity_offset) = find_bytes(payload, label, search, end) {
            let body_start = entity_offset + label.len();
            let body_end = find_bytes(payload, b"\xe0\x00entity(", body_start, end).unwrap_or(end);
            let Some(entity_id) = saved_entity_id(payload, body_start, body_end) else {
                search = body_end;
                continue;
            };
            let Some(center) =
                saved_named_scalars::<3>(payload, b"center", body_start, body_end, cache)
            else {
                search = body_end;
                continue;
            };
            let Some([radius]) =
                saved_named_scalars::<1>(payload, b"radius", body_start, body_end, cache)
            else {
                search = body_end;
                continue;
            };
            if kind == "arc" {
                let Some(first) =
                    saved_named_scalars::<3>(payload, b"end1", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                let Some(second) =
                    saved_named_scalars::<3>(payload, b"end2", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                let Some([start_parameter]) =
                    saved_named_scalars::<1>(payload, b"t0", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                let Some([end_parameter]) =
                    saved_named_scalars::<1>(payload, b"t1", body_start, body_end, cache)
                else {
                    search = body_end;
                    continue;
                };
                entities.push(FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center,
                    radius,
                    endpoints: [first, second],
                    parameters: [start_parameter, end_parameter],
                    offset: entity_offset,
                }));
            } else {
                entities.push(FeatureSavedEntity::Circle(FeatureSavedCircle {
                    entity_id,
                    center,
                    radius,
                    offset: entity_offset,
                }));
            }
            search = body_end;
        }
    }
    entities
}

fn saved_dummy_entities(payload: &[u8], start: usize, end: usize) -> Vec<FeatureSavedEntity> {
    let label = b"\xe0\x00entity(dummy_ent)\0";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(entity_offset) = find_bytes(payload, label, search, end) {
        let body_start = entity_offset + label.len();
        let body_end = find_bytes(payload, b"\xe0\x00entity(", body_start, end).unwrap_or(end);
        entities.push(FeatureSavedEntity::Dummy(FeatureSavedDummy {
            entity_id: saved_entity_id(payload, body_start, body_end),
            offset: entity_offset,
        }));
        search = body_end;
    }
    entities
}

fn saved_entity_offset(entity: &FeatureSavedEntity) -> usize {
    match entity {
        FeatureSavedEntity::Line(entity) => entity.offset,
        FeatureSavedEntity::Arc(entity) => entity.offset,
        FeatureSavedEntity::Circle(entity) => entity.offset,
        FeatureSavedEntity::Dummy(entity) => entity.offset,
    }
}

fn saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureSavedSection> {
    let table = find_bytes(payload, b"\xe0\x00p_saved_result\0", start, end)?;
    let table_end = find_bytes(payload, b"\xe0\x02local_sys\0", table, end)
        .or_else(|| find_bytes(payload, b"\xe0\x00rigid_data\0", table, end))
        .unwrap_or(end);
    let mut entities = saved_line_entities(payload, table, table_end, cache);
    entities.extend(saved_circular_entities(payload, table, table_end, cache));
    entities.extend(saved_dummy_entities(payload, table, table_end));
    entities.sort_by_key(saved_entity_offset);
    (!entities.is_empty()).then_some(FeatureSavedSection {
        entities,
        offset: table,
    })
}

fn field_value(payload: &[u8]) -> FeatureFieldValue {
    if payload.is_empty() {
        return FeatureFieldValue::Empty;
    }
    if payload[0] == psb::token::SCALAR_BODY && payload.len() >= 3 {
        let slot_count = usize::from(payload[1]).checked_mul(usize::from(payload[2]));
        let cache = scalar::ScalarCache::from_section(payload);
        let decoded_values =
            slot_count.and_then(|slots| decode_exact_scalars(&payload[3..], slots, &cache));
        return FeatureFieldValue::ScalarArray {
            dimensions: payload[1],
            count: payload[2],
            body: payload[3..].to_vec(),
            decoded_values,
        };
    }
    if payload[0] == psb::token::ENTITY_REF {
        if let Ok((entity_id, end)) = psb::reference_id(payload, 1) {
            let terminated = end + 1 == payload.len() && payload[end] == psb::token::ARRAY_CLOSE;
            if end == payload.len() || terminated {
                return FeatureFieldValue::EntityReference {
                    entity_id,
                    terminated,
                };
            }
        }
    }
    if payload[0] == psb::token::ARRAY_OPEN {
        let (count, mut cursor) = psb::compact_int(payload, 1);
        let mut values = Vec::new();
        for _ in 0..count {
            let (value, next) = psb::compact_int(payload, cursor);
            if next == cursor {
                return FeatureFieldValue::Raw(payload.to_vec());
            }
            values.push(value);
            cursor = next;
        }
        if cursor == payload.len()
            || cursor + 1 == payload.len() && payload[cursor] == psb::token::ARRAY_CLOSE
        {
            return FeatureFieldValue::CompactIntArray(values);
        }
    }
    let (value, end) = psb::compact_int(payload, 0);
    if end == payload.len() {
        FeatureFieldValue::CompactInt(value)
    } else {
        FeatureFieldValue::Raw(payload.to_vec())
    }
}

/// Decode named fields and their context-independent value wrappers inside
/// procedural choice spans.
pub fn choice_fields(choices: &[FeatureChoice]) -> Vec<FeatureChoiceField> {
    let mut fields = Vec::new();
    for choice in choices {
        let mut headers = Vec::new();
        for offset in 0..choice.payload.len().saturating_sub(2) {
            if choice.payload[offset] != psb::token::NAMED_RECORD {
                continue;
            }
            let Some(nul) = choice.payload[offset + 2..]
                .iter()
                .position(|&byte| byte == 0)
                .map(|relative| offset + 2 + relative)
            else {
                continue;
            };
            if choice.payload[offset + 2..nul]
                .iter()
                .all(u8::is_ascii_graphic)
            {
                headers.push((offset, nul + 1));
            }
        }
        for (index, &(header, value_start)) in headers.iter().enumerate() {
            let end = headers
                .get(index + 1)
                .map_or(choice.payload.len(), |hit| hit.0);
            fields.push(FeatureChoiceField {
                feature_id: choice.feature_id,
                choice_label: choice.label.clone(),
                name: String::from_utf8_lossy(&choice.payload[header + 2..value_start - 1])
                    .into_owned(),
                type_byte: choice.payload[header + 1],
                value: field_value(&choice.payload[value_start..end]),
                offset: choice.payload_offset + header,
            });
        }
    }
    fields.sort_by_key(|field| field.offset);
    fields
}

/// Decode generated-geometry table headers from known feature rows.
pub fn geometry_tables(rows: &[FeatureRow]) -> Vec<FeatureGeometryTable> {
    const FIELDS: &[(&[u8], FeatureGeometryTableKind)] = &[
        (b"edg_id_tab_ptr", FeatureGeometryTableKind::EdgeIds),
        (b"lo_id_tab_ptr", FeatureGeometryTableKind::LoopIds),
        (b"bnd_type", FeatureGeometryTableKind::Boundaries),
        (b"used_bodies", FeatureGeometryTableKind::UsedBodies),
        (b"geom_lists", FeatureGeometryTableKind::GeometryLists),
        (b"dtm_id_tab", FeatureGeometryTableKind::DatumIds),
    ];
    let mut tables = Vec::new();
    for row in rows {
        for &(label, kind) in FIELDS {
            let needle = [label, b"\0"].concat();
            let Some(offset) = row
                .body
                .windows(needle.len())
                .position(|window| window == needle)
            else {
                continue;
            };
            let mut cursor = offset + needle.len();
            if row
                .body
                .get(cursor)
                .is_some_and(|byte| matches!(byte, 0xf1 | 0xf2))
            {
                cursor += 1;
            }
            if row.body.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
                continue;
            }
            let (count, after_count) = psb::compact_int(&row.body, cursor + 1);
            if after_count == cursor + 1
                || row.body.get(after_count) != Some(&psb::token::ENTITY_REF)
            {
                continue;
            }
            let Ok((entity_class, _)) = psb::reference_id(&row.body, after_count + 1) else {
                continue;
            };
            tables.push(FeatureGeometryTable {
                feature_id: row.feature_id,
                kind,
                count,
                entity_class,
                offset: row.body_offset + offset,
            });
        }
    }
    tables.sort_by_key(|table| table.offset);
    tables
}

/// Decode complete named affected-ID arrays from known feature rows.
pub fn affected_ids(rows: &[FeatureRow]) -> Vec<FeatureAffectedIds> {
    const FIELDS: &[(&[u8], AffectedIdKind)] = &[
        (b"geoms_affected", AffectedIdKind::Geometry),
        (b"edgs_affected", AffectedIdKind::Edges),
        (b"strong_parents", AffectedIdKind::StrongParents),
        (b"parent_table", AffectedIdKind::Parents),
        (b"contours", AffectedIdKind::Contours),
    ];
    let mut result = Vec::new();
    for row in rows {
        for &(label, kind) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(needle.len())
                    .position(|window| window == needle)
            }) {
                let label_offset = from + relative;
                from = label_offset + needle.len();
                if label_offset < 2
                    || row.body[label_offset - 2] != psb::token::NAMED_RECORD
                    || row.body.get(from) != Some(&psb::token::ARRAY_OPEN)
                {
                    continue;
                }
                let (count, mut cursor) = psb::compact_int(&row.body, from + 1);
                if cursor == from + 1 {
                    continue;
                }
                let mut ids = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let (id, next) = psb::compact_int(&row.body, cursor);
                    if next == cursor {
                        ids.clear();
                        break;
                    }
                    ids.push(id);
                    cursor = next;
                }
                if ids.len() == count as usize {
                    result.push(FeatureAffectedIds {
                        feature_id: row.feature_id,
                        kind,
                        ids,
                        offset: row.body_offset + label_offset - 2,
                    });
                }
            }
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode unlabeled positional replay affected-ID runs.
pub fn replay_affected_ids(rows: &[FeatureRow]) -> Vec<FeatureReplayAffectedIds> {
    const ANCHOR: &[u8] = &[0xf1, 0xf7, 0x42, 0xd8, 0x80, 0x01, 0xe3];
    const TERMINATOR: &[u8] = &[0xf5, 0x96, 0x92];
    const SKIP: &[u8] = &[0xf7, 0xf6, 0xf1, 0xf2, 0xfb, 0xe3, 0xe1, 0xe2];
    let mut result = Vec::new();
    for row in rows {
        let Some(anchor) = row
            .body
            .windows(ANCHOR.len())
            .rposition(|window| window == ANCHOR)
        else {
            continue;
        };
        let run_start = anchor + ANCHOR.len();
        let Some(term_relative) = row.body[run_start..]
            .windows(TERMINATOR.len())
            .position(|window| window == TERMINATOR)
        else {
            continue;
        };
        let run = &row.body[run_start..run_start + term_relative];
        let mut ids = Vec::new();
        let mut has_count_opener = false;
        let mut cursor = 0;
        while cursor < run.len() {
            if run[cursor] == psb::token::NAMED_RECORD {
                cursor = run[cursor + 1..]
                    .iter()
                    .position(|&byte| byte == 0)
                    .map_or(run.len(), |relative| cursor + relative + 2);
                continue;
            }
            if run[cursor] == psb::token::ARRAY_OPEN {
                has_count_opener = true;
                let (_, next) = psb::compact_int(run, cursor + 1);
                cursor = next.max(cursor + 1);
                continue;
            }
            if SKIP.contains(&run[cursor]) {
                cursor += 1;
                continue;
            }
            let (id, next) = psb::compact_int(run, cursor);
            if next == cursor {
                cursor += 1;
            } else {
                ids.push(id);
                cursor = next;
            }
        }
        if !ids.is_empty() {
            result.push(FeatureReplayAffectedIds {
                feature_id: row.feature_id,
                ids,
                has_count_opener,
                offset: row.body_offset + anchor,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode genuine named `direction` and `direction2` recipe bytes.
pub fn direction_bytes(rows: &[FeatureRow]) -> Vec<FeatureDirectionByte> {
    const FIELDS: &[(&[u8], DirectionLane)] = &[
        (b"direction", DirectionLane::Primary),
        (b"direction2", DirectionLane::Secondary),
    ];
    let mut result = Vec::new();
    for row in rows {
        for &(label, lane) in FIELDS {
            let needle = [label, b"\0"].concat();
            let mut from = 0;
            while let Some(relative) = row.body.get(from..).and_then(|tail| {
                tail.windows(needle.len())
                    .position(|window| window == needle)
            }) {
                let label_offset = from + relative;
                from = label_offset + needle.len();
                if label_offset < 2 || row.body[label_offset - 2] != psb::token::NAMED_RECORD {
                    continue;
                }
                let Some(&raw) = row.body.get(from) else {
                    continue;
                };
                let value = match raw {
                    0 => DirectionValue::SideFlag(false),
                    1 => DirectionValue::SideFlag(true),
                    value => DirectionValue::Raw(value),
                };
                result.push(FeatureDirectionByte {
                    feature_id: row.feature_id,
                    lane,
                    value,
                    offset: row.body_offset + label_offset - 2,
                });
            }
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
pub fn definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    const PREFIX: &[u8] = b"feat_defs_";
    let cache = scalar::ScalarCache::from_section(payload);
    let mut starts = Vec::new();
    for offset in 0..payload.len() {
        if payload.get(offset..offset + PREFIX.len()) != Some(PREFIX) {
            continue;
        }
        let digits_start = offset + PREFIX.len();
        let Some(nul_relative) = payload[digits_start..].iter().position(|&byte| byte == 0) else {
            continue;
        };
        let digits = &payload[digits_start..digits_start + nul_relative];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let Ok(id) = String::from_utf8_lossy(digits).parse::<u32>() else {
            continue;
        };
        starts.push((offset, id));
    }
    let mut result = Vec::new();
    for (index, &(start, id)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _)| offset);
        let mut parameter_frames = Vec::new();
        for &(label, kind) in &[
            (
                b"local_sys".as_slice(),
                FeatureParameterFrameKind::LocalSystem,
            ),
            (b"transf".as_slice(), FeatureParameterFrameKind::Transform),
        ] {
            let needle = [label, b"\0\xf9\x04\x03"].concat();
            let mut from = start;
            while let Some(relative) = payload[from..end]
                .windows(needle.len())
                .position(|window| window == needle)
            {
                let field_offset = from + relative;
                let body_start = field_offset + needle.len();
                let body_end = payload[body_start..end]
                    .windows(1)
                    .position(|window| window[0] == psb::token::NAMED_RECORD)
                    .map_or(end, |relative| body_start + relative);
                let body = payload[body_start..body_end].to_vec();
                parameter_frames.push(FeatureParameterFrame {
                    kind,
                    decoded_values: decode_exact_scalars(&body, 12, &cache),
                    body,
                    offset: field_offset,
                });
                from = body_start;
            }
        }
        parameter_frames.sort_by_key(|frame| frame.offset);
        let mut outlines = Vec::new();
        if let Some(info) = find_bytes(payload, b"\xe0\x00feat_outl_info\0", start, end) {
            if let Some(label) = find_bytes(payload, b"outline\0\xf9\x02\x03", info, end) {
                let scalar_start = label + b"outline\0\xf9\x02\x03".len();
                outlines.push(FeatureOutline {
                    phase: OutlinePhase::PreRollback,
                    local_values: decode_optional_scalars(payload, scalar_start, 6, &cache),
                    offset: label,
                });
            }
            for &(label, phase) in &[
                (
                    b"\xe0\x00post_roll_back\0".as_slice(),
                    OutlinePhase::PostRollback,
                ),
                (b"\xe0\x00post_regen\0".as_slice(), OutlinePhase::PostRegen),
            ] {
                let Some(label_offset) = find_bytes(payload, label, info, end) else {
                    continue;
                };
                let framing = label_offset + label.len();
                if payload.get(framing..framing + 2) != Some(&[0xe3, psb::token::ENTITY_REF]) {
                    continue;
                }
                let Ok((_, after_ref)) = psb::reference_id(payload, framing + 2) else {
                    continue;
                };
                if payload.get(after_ref..after_ref + 3) != Some(&[0xf5, 0x96, 0x92])
                    || after_ref + 4 > end
                {
                    continue;
                }
                outlines.push(FeatureOutline {
                    phase,
                    local_values: decode_optional_scalars(payload, after_ref + 4, 6, &cache),
                    offset: label_offset,
                });
            }
        }
        outlines.sort_by_key(|outline| outline.offset);
        let variables = variable_table(payload, start, end, &cache);
        let segments = segment_table(payload, start, end);
        let trim_entities = trim_entity_table(payload, start, end);
        let trim_vertices = trim_vertex_table(
            payload,
            start,
            end,
            trim_entities.as_ref(),
            segments.as_ref(),
            variables.as_ref(),
        );
        let order_table = order_table(payload, start, end);
        let section_3d = section_3d(payload, start, end);
        let dimensions = dimension_table(payload, start, end, &cache);
        let relations = relation_table(payload, start, end);
        let saved_section = saved_section(payload, start, end, &cache);
        let owner_ids = psb::tokens(&payload[start..end])
            .into_iter()
            .filter(|token| token.kind == psb::TokenKind::NamedRecord)
            .filter_map(|token| {
                let name_start = start + token.offset + 2;
                let name_end = start + token.offset + token.length - 1;
                (payload.get(name_start..name_end) == Some(b"feat_id".as_slice())).then(|| {
                    let value_start = start + token.offset + token.length;
                    let (value, after) = psb::compact_int(payload, value_start);
                    (after > value_start).then_some(value)
                })?
            })
            .collect::<BTreeSet<_>>();
        let owner_feature_id = owner_ids.first().copied().filter(|_| owner_ids.len() == 1);
        result.push(FeatureDefinition {
            id,
            owner_feature_id,
            body: payload[start..end].to_vec(),
            parameter_frames,
            outlines,
            variables,
            segments,
            trim_entities,
            trim_vertices,
            order_table,
            section_3d,
            dimensions,
            relations,
            saved_section,
            offset: start,
        });
    }
    result
}

/// Decode the implicit named-record entity table and every canonical `f7`
/// reference, preserving both source context and unresolved target IDs.
pub fn entity_graph(payload: &[u8]) -> (Vec<FeatureEntity>, Vec<FeatureEntityReference>) {
    let tokens = psb::tokens(payload);
    let mut entities = Vec::new();
    for token in &tokens {
        if token.kind != psb::TokenKind::NamedRecord || token.length < 3 {
            continue;
        }
        let name_start = token.offset + 2;
        let name_end = token.offset + token.length - 1;
        entities.push(FeatureEntity {
            entity_id: entities.len() as u32,
            type_byte: payload[token.offset + 1],
            name: String::from_utf8_lossy(&payload[name_start..name_end]).into_owned(),
            offset: token.offset,
        });
    }
    let entity_count = entities.len() as u32;
    let entity_by_offset = entities
        .iter()
        .map(|entity| (entity.offset, entity.entity_id))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut source = None;
    let mut references = Vec::new();
    for token in tokens {
        if token.kind == psb::TokenKind::NamedRecord {
            source = entity_by_offset.get(&token.offset).copied();
        } else if token.kind == psb::TokenKind::EntityReference {
            let Ok((target_entity_id, _)) = psb::reference_id(payload, token.offset + 1) else {
                continue;
            };
            references.push(FeatureEntityReference {
                source_entity_id: source,
                target_entity_id,
                target_resolved: target_entity_id < entity_count,
                offset: token.offset,
            });
        }
    }
    (entities, references)
}

fn read_entries(payload: &[u8], body_start: usize, count: u32) -> Option<Vec<u32>> {
    let mut entries = Vec::with_capacity(count as usize);
    let mut cursor = body_start;
    for _ in 0..count {
        if payload.get(cursor..cursor + 2) == Some(&[0xf7, 0x1e]) {
            cursor += 2;
        }
        let (id, after) = psb::reference_id(payload, cursor).ok()?;
        let close = payload
            .get(after..)?
            .iter()
            .position(|&byte| byte == 0xe3)?;
        entries.push(id);
        cursor = after + close + 1;
    }
    Some(entries)
}

/// Decode valid `AllFeatur` mixed generated-entity tables.
///
/// `feature_ids` must come from byte-decoded geometry ownership; no owner is
/// inferred from a table's neighbouring bytes or entity contents.
pub fn entity_tables(
    payload: &[u8],
    feature_ids: &BTreeSet<u32>,
    surface_ids: &BTreeSet<u32>,
) -> Vec<FeatureEntityTable> {
    let spans = row_spans(payload, feature_ids);
    let mut tables = Vec::new();
    for offset in 0..payload.len() {
        if payload[offset] != psb::token::ARRAY_OPEN {
            continue;
        }
        let (count, after_count) = psb::compact_int(payload, offset + 1);
        if !(1..=64).contains(&count)
            || payload.get(after_count..after_count + TABLE_TAG.len()) != Some(TABLE_TAG)
        {
            continue;
        }
        let Some(entry_ids) = read_entries(payload, after_count + TABLE_TAG.len(), count) else {
            continue;
        };
        let surface_ids = entry_ids
            .iter()
            .copied()
            .filter(|id| surface_ids.contains(id))
            .collect::<Vec<_>>();
        if surface_ids.is_empty() {
            continue;
        }
        let non_surface_entity_ids = entry_ids
            .iter()
            .copied()
            .filter(|id| !surface_ids.contains(id))
            .collect();
        let feature_id = spans
            .iter()
            .find(|&&(start, end, _)| start <= offset && offset < end)
            .map(|&(_, _, id)| id);
        tables.push(FeatureEntityTable {
            feature_id,
            entry_ids,
            surface_ids,
            non_surface_entity_ids,
            offset,
        });
    }
    tables
}
