// SPDX-License-Identifier: Apache-2.0
//! `FeatDefs` / DEPDB feature definitions and owner binding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::bounded_len;

use crate::psb;
use crate::scalar;

use super::entity::{generated_class_200_source_entity_ids, FeatureEntityTable};
use super::helpers::{decode_optional_scalars, find_bytes};
use super::operations::{FeatureOperation, FeatureRecipeKind};
use super::rows::{
    FeatureGeometryTable, FeatureGeometryTableKind, FeatureRevolutionExtent,
    FeatureRevolutionExtentKind,
};

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

/// One instantiated row from a feature definition's `place_instruction_ptrs`
/// table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePlacementInstruction {
    /// Stored placement instruction family.
    pub kind: u32,
    /// Whether the scalar offset lane stores exact zero.
    pub zero_offset: bool,
    /// Optional driving dimension identifier.
    pub dimension_id: Option<u32>,
    /// Optional referenced placement object.
    pub reference_id: Option<u32>,
    /// First optional geometry operand.
    pub geometry1_id: Option<u32>,
    /// Second optional geometry operand.
    pub geometry2_id: Option<u32>,
    /// First membership selector.
    pub member1: u32,
    /// Second membership selector.
    pub member2: u32,
    /// Byte offset of the positional row marker.
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
    /// Exact encoded scalar body of each feature-local slot.
    pub local_value_bodies: Vec<Vec<u8>>,
    /// Byte offset of the outline label in the original stream.
    pub offset: usize,
}

/// One positional solver-variable row from `var_arr`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVariableRow {
    /// Variable class: `1` is section `u`, `2` is section `v`, `3` is radius.
    pub variable_type: u32,
    /// Point or solver-variable key.
    pub key: u32,
    /// Solved value when the scalar token is defined inline.
    pub value: Option<f64>,
    /// Exact encoded scalar body of the stored value.
    pub value_body: Vec<u8>,
    /// Pre-solve estimate when defined inline.
    pub guess: Option<f64>,
    /// Exact encoded scalar body of the pre-solve estimate.
    pub guess_body: Vec<u8>,
    /// Whether the pre-solve estimate used the nine-byte dimension-driven
    /// sentinel.
    pub guess_dimension_driven: bool,
    /// Stored solver-known flag.
    pub known: Option<u32>,
    /// Stored solver homogeneity class.
    pub homogeneity: Option<u32>,
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

impl FeatureVariableTable {
    /// Whether every row declared by the table decoded.
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok() == Some(self.rows.len())
    }

    /// Reconcile repeated and complementary section-point rows by identity.
    pub fn reconciled_points(&self) -> (BTreeMap<u32, [Option<f64>; 2]>, BTreeSet<u32>) {
        let point_ids = self
            .points
            .iter()
            .map(|point| point.point_id)
            .chain(
                self.rows
                    .iter()
                    .filter_map(|row| matches!(row.variable_type, 1 | 2).then_some(row.key)),
            )
            .collect::<BTreeSet<_>>();
        let mut points = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        for point_id in point_ids {
            let mut point = [None; 2];
            let mut conflict = false;
            for coordinate in 0..2 {
                let variable_type = coordinate as u32 + 1;
                let raw_rows = self
                    .rows
                    .iter()
                    .filter(|row| row.key == point_id && row.variable_type == variable_type)
                    .collect::<Vec<_>>();
                let values = if raw_rows.is_empty() {
                    self.points
                        .iter()
                        .filter(|point| point.point_id == point_id)
                        .filter_map(|point| [point.u, point.v][coordinate])
                        .collect::<Vec<_>>()
                } else {
                    raw_rows
                        .into_iter()
                        .filter_map(|row| row.value)
                        .collect::<Vec<_>>()
                };
                let Some(first) = values.first().copied() else {
                    continue;
                };
                let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
                if values
                    .iter()
                    .all(|candidate| (*candidate - first).abs() <= 1e-9 * scale)
                {
                    point[coordinate] = Some(first);
                } else {
                    conflict = true;
                }
            }
            if conflict {
                ambiguous.insert(point_id);
            } else {
                points.insert(point_id, point);
            }
        }
        (points, ambiguous)
    }
}

/// One positional solver-equation row from `eqtn_arr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEquation {
    /// Equation identifier from the first positional field.
    pub equation_id: u32,
    /// Solver function identifier from the second positional field.
    pub function_id: u32,
    /// Explicit argument-slot count, when the row uses the counted form.
    pub explicit_argument_count: Option<u32>,
    /// Argument slots in stored order. Expansion markers occupy their
    /// documented number of slots; `None` is the native null slot.
    pub arguments: Vec<Option<u32>>,
    /// Exact encoded argument body between the argument-count marker and the
    /// auxiliary marker.
    pub arguments_body: Vec<u8>,
    /// Exact encoded auxiliary field body.
    pub auxiliary_body: Vec<u8>,
    /// Exact row bytes, including the `e2` row terminator when present.
    pub body: Vec<u8>,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// Solver-equation table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureEquationTable {
    /// Count declared by the `f8` opener. Its relationship to replay rows is
    /// retained without assuming whether it includes the prototype.
    pub declared_count: u32,
    /// Entity-table reference following the opener, when present.
    pub entity_ref: Option<u32>,
    /// Exact named prototype body, including its row-class reference.
    pub prototype_body: Vec<u8>,
    /// Positional equation rows in stored order.
    pub rows: Vec<FeatureEquation>,
    /// Byte offset of the `eqtn_arr` label in the original stream.
    pub offset: usize,
}

/// Defined positional segment family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureSegmentKind {
    /// Type `2` line segment.
    Line,
    /// Type `3` circular-arc segment.
    Arc,
    /// Type `5` isolated point entity.
    Point,
}

/// One positional `segtab_ptr` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegment {
    /// Line or arc discriminator.
    pub kind: FeatureSegmentKind,
    /// Three direction fields; control-range sentinels remain `None`.
    pub directions: [Option<u32>; 3],
    /// Endpoint IDs into the section variable table. Point entities normalize
    /// their single stored point identifier into both slots.
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
    /// Exact positional row bytes from the optional type wrapper or family
    /// discriminator through the `e2` row close. Empty for a labeled
    /// prototype row.
    pub body: Vec<u8>,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One circular type `10` `segtab_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureCircleSegment {
    /// Center point ID into the section variable table.
    pub center_id: u32,
    /// Radius reference into the section solver namespace.
    pub radius_ref: u32,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One point type `1` `segtab_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePointSegment {
    /// Point ID stored in the center-point field.
    pub point_id: u32,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One centered construction-line type `47` `segtab_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureCenteredLineSegment {
    /// Center point reference stored by the section solver.
    pub center_id: u32,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One type `25` section-reference line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureReferenceLineSegment {
    /// Three stored direction fields.
    pub directions: [Option<u32>; 3],
    /// Optional endpoint IDs into the section variable table.
    pub point_ids: [Option<u32>; 2],
    /// Vertical/horizontal constraint field.
    pub vertical_horizontal: Option<u32>,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One type `12` bounded section curve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureBoundedCurveSegment {
    /// Three stored direction fields.
    pub directions: [Option<u32>; 3],
    /// Endpoint IDs into the section variable table.
    pub point_ids: [u32; 2],
    /// Stored center-point field.
    pub center_id: Option<u32>,
    /// Stored arc-orientation field.
    pub arc_orientation: Option<u32>,
    /// Stored vertical/horizontal field.
    pub vertical_horizontal: Option<u32>,
    /// Stored radius-reference field.
    pub radius_ref: Option<u32>,
    /// Stored secondary-radius-reference field.
    pub radius2_ref: Option<u32>,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One type `58` saved-conic section row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureConicSegment {
    /// Center point reference stored by the section solver.
    pub center_id: u32,
    /// First coefficient reference stored by the section solver.
    pub first_coefficient_ref: u32,
    /// Second coefficient reference stored by the section solver.
    pub second_coefficient_ref: u32,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// One fully framed `segtab_ptr` row outside the core segment-family enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureOpaqueSegment {
    /// Stored segment-family discriminator.
    pub kind: u32,
    /// Three stored direction fields.
    pub directions: [Option<u32>; 3],
    /// Two stored point fields.
    pub point_ids: [Option<u32>; 2],
    /// Stored center-point field.
    pub center_id: Option<u32>,
    /// Stored arc-orientation field.
    pub arc_orientation: Option<u32>,
    /// Stored vertical/horizontal field.
    pub vertical_horizontal: Option<u32>,
    /// Stored radius-reference field.
    pub radius_ref: Option<u32>,
    /// Stored secondary-radius-reference field.
    pub radius2_ref: Option<u32>,
    /// External segment identifier used by section tables.
    pub external_id: u32,
    /// Exact positional row bytes from the optional type wrapper or family
    /// discriminator through the `e2` row close. Empty for a labeled
    /// prototype row.
    pub body: Vec<u8>,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// Defining-sketch segment table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSegmentTable {
    /// Count declared by the `f8` opener.
    pub declared_count: u32,
    /// Whether the declared count includes an inherited prototype omitted from
    /// the positional replay body.
    pub has_elided_prototype: bool,
    /// Entity-table reference following the opener.
    pub entity_ref: Option<u32>,
    /// Fully aligned line and arc rows.
    pub rows: Vec<FeatureSegment>,
    /// Fully aligned circular rows.
    pub circle_rows: Vec<FeatureCircleSegment>,
    /// Fully aligned type-1 point rows.
    pub point_rows: Vec<FeaturePointSegment>,
    /// Fully aligned centered construction-line rows.
    pub centered_line_rows: Vec<FeatureCenteredLineSegment>,
    /// Fully aligned type-25 section-reference lines.
    pub reference_line_rows: Vec<FeatureReferenceLineSegment>,
    /// Fully aligned type-12 bounded section curves.
    pub bounded_curve_rows: Vec<FeatureBoundedCurveSegment>,
    /// Fully aligned type-58 saved-conic rows.
    pub conic_rows: Vec<FeatureConicSegment>,
    /// Fully aligned rows with unsupported segment-family discriminators.
    pub opaque_rows: Vec<FeatureOpaqueSegment>,
    /// Byte offset of the `segtab_ptr` label in the original stream.
    pub offset: usize,
}

impl FeatureSegmentTable {
    /// Number of decoded rows retained across all segment families.
    pub(crate) fn retained_row_count(&self) -> usize {
        self.rows.len()
            + self.circle_rows.len()
            + self.point_rows.len()
            + self.centered_line_rows.len()
            + self.reference_line_rows.len()
            + self.bounded_curve_rows.len()
            + self.conic_rows.len()
            + self.opaque_rows.len()
    }

    /// Whether every row declared by the table decoded.
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok()
            == Some(usize::from(self.has_elided_prototype) + self.retained_row_count())
    }

    /// Number of decoded rows carrying one external identifier.
    pub(crate) fn external_id_count(&self, external_id: u32) -> usize {
        self.rows
            .iter()
            .map(|row| row.external_id)
            .chain(self.circle_rows.iter().map(|row| row.external_id))
            .chain(self.point_rows.iter().map(|row| row.external_id))
            .chain(self.centered_line_rows.iter().map(|row| row.external_id))
            .chain(self.reference_line_rows.iter().map(|row| row.external_id))
            .chain(self.bounded_curve_rows.iter().map(|row| row.external_id))
            .chain(self.conic_rows.iter().map(|row| row.external_id))
            .chain(self.opaque_rows.iter().map(|row| row.external_id))
            .filter(|candidate| *candidate == external_id)
            .count()
    }

    /// Resolve a uniquely identified defining-sketch segment.
    pub fn segment(&self, external_id: u32) -> Option<&FeatureSegment> {
        self.is_complete().then_some(())?;
        let segment = self
            .rows
            .iter()
            .find(|segment| segment.external_id == external_id)?;
        (self.external_id_count(external_id) == 1).then_some(segment)
    }
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

/// One stored hash bucket in a native trim table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimBucket {
    /// Zero-based bucket index.
    pub index: u32,
    /// Number of entries declared by the bucket array opener.
    pub declared_entry_count: u32,
    /// Number of structurally complete entries decoded within the bucket frame.
    pub decoded_entry_count: u32,
    /// Byte offset of the stored bucket index.
    pub offset: usize,
}

impl FeatureTrimBucket {
    /// Whether every declared entry has one complete stored body.
    pub fn is_complete(&self) -> bool {
        self.decoded_entry_count == self.declared_entry_count
    }
}

/// Solved/trimmed entity graph for one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTrimEntityTable {
    /// Count declared by the table opener when present.
    pub declared_count: Option<u32>,
    /// Native table-class reference when present.
    pub entity_ref: Option<u32>,
    /// Native row-class reference when present.
    pub entry_ref: Option<u32>,
    /// Explicit hash buckets decoded in stored order.
    pub buckets: Vec<FeatureTrimBucket>,
    /// Complete positional rows in stored order.
    pub rows: Vec<FeatureTrimEntity>,
    /// Sorted external IDs present in the trimmed profile.
    pub solved_external_ids: Vec<u32>,
    /// Byte offset of the `ent_tab` label in the original stream.
    pub offset: usize,
}

impl FeatureTrimEntityTable {
    /// Whether every declared hash-bucket index was decoded in order.
    pub fn has_complete_bucket_index_sequence(&self) -> bool {
        complete_bucket_index_sequence(self.declared_count, &self.buckets)
    }

    /// Whether every declared bucket and entry body is structurally complete.
    pub fn has_complete_bucket_frame(&self) -> bool {
        self.has_complete_bucket_index_sequence()
            && self.buckets.iter().all(FeatureTrimBucket::is_complete)
    }

    /// Whether each retained external entity identifier occurs once.
    pub fn has_unique_external_ids(&self) -> bool {
        let mut ids = BTreeSet::new();
        self.rows.iter().all(|row| ids.insert(row.external_id))
    }
}

/// One solved trim vertex and the two trimmed entities incident to it.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertex {
    /// Vertex identifier shared with `ent_tab` endpoint and center fields.
    pub vertex_id: u32,
    /// Distinct `ent_tab` external entity identifiers meeting at the vertex.
    pub entities: Vec<u32>,
    /// Solved section-frame coordinates for a uniquely resolved carrier junction.
    pub section_coordinates: Option<[f64; 2]>,
    /// Byte offset of the positional triple in the original stream.
    pub offset: usize,
}

/// Solved trim-vertex adjacency table for one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureTrimVertexTable {
    /// Count declared by the table opener when present.
    pub declared_count: Option<u32>,
    /// Native table-class reference when present.
    pub entity_ref: Option<u32>,
    /// Native row-class reference when present.
    pub entry_ref: Option<u32>,
    /// Explicit hash buckets decoded in stored order.
    pub buckets: Vec<FeatureTrimBucket>,
    /// Complete validated vertex rows in stored order.
    pub rows: Vec<FeatureTrimVertex>,
    /// Byte offset of the `vert_tab` label in the original stream.
    pub offset: usize,
}

impl FeatureTrimVertexTable {
    /// Whether every declared hash-bucket index was decoded in order.
    pub fn has_complete_bucket_index_sequence(&self) -> bool {
        complete_bucket_index_sequence(self.declared_count, &self.buckets)
    }

    /// Whether every declared bucket and entry body is structurally complete.
    pub fn has_complete_bucket_frame(&self) -> bool {
        self.has_complete_bucket_index_sequence()
            && self.buckets.iter().all(FeatureTrimBucket::is_complete)
    }
}

fn complete_bucket_index_sequence(
    declared_count: Option<u32>,
    buckets: &[FeatureTrimBucket],
) -> bool {
    declared_count.is_none_or(|count| {
        usize::try_from(count).ok() == Some(buckets.len())
            && buckets.iter().map(|bucket| bucket.index).eq(0..count)
    })
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
    /// Whether `declared_count` includes a structural prototype outside `rows`.
    pub has_prototype: bool,
    /// Entity-table class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional triples in stored order.
    pub rows: Vec<FeatureOrderRow>,
    /// Byte offset of the `order_table` label in the original stream.
    pub offset: usize,
}

impl FeatureOrderTable {
    /// Whether every entry declared by the table opener was decoded.
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok()
            == Some(usize::from(self.has_prototype) + self.rows.len())
    }

    /// Resolve a generated-entity position to its section entity identifier.
    pub fn external_id(&self, internal_id: u32) -> Option<u32> {
        self.is_complete().then_some(())?;
        let mut matches = self
            .rows
            .iter()
            .filter(|row| row.internal_id == internal_id);
        let row = matches.next()?;
        (matches.next().is_none()
            && self
                .rows
                .iter()
                .filter(|candidate| candidate.external_id == row.external_id)
                .count()
                == 1)
            .then_some(row.external_id)
    }

    /// Resolve a section entity identifier to its generated-entity position.
    pub fn internal_id(&self, external_id: u32) -> Option<u32> {
        self.is_complete().then_some(())?;
        let mut matches = self
            .rows
            .iter()
            .filter(|row| row.external_id == external_id);
        let row = matches.next()?;
        (matches.next().is_none()
            && self
                .rows
                .iter()
                .filter(|candidate| candidate.internal_id == row.internal_id)
                .count()
                == 1)
            .then_some(row.internal_id)
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

/// One positional gsec3d reference-plane row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSectionReferencePlane {
    /// Row `plane_id` entity identifier.
    pub plane_entity_id: u32,
    /// Row `ref_type` discriminator.
    pub reference_type: Option<u32>,
    /// Row `ext_ref_id` identifier.
    pub external_reference_id: Option<u32>,
    /// Row `seg_id` identifier.
    pub segment_id: Option<u32>,
    /// Row `sub_index` value.
    pub sub_index: Option<u32>,
    /// Row `flip_flag`.
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
    /// Complete positional reference-plane rows in stored order.
    pub reference_plane_rows: Vec<FeatureSectionReferencePlane>,
    /// Geometry identifier joining the reference plane to its datum surface.
    pub reference_plane_datum_geometry_id: Option<u32>,
    /// Singleton named-record orientation fields.
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
    /// Linear dimension value stored in model millimeters.
    Millimeters,
    /// Dimension type whose unit is defined by its enclosing section schema.
    SchemaDefined,
}

/// One row from a dimension's nested `dim_ref` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDimensionReference {
    /// Nullable item identifier stored by the reference row.
    pub item_id: Option<u32>,
    /// Nullable sense selector stored by the reference row.
    pub sense: Option<u32>,
    /// Nullable two-slot point selector stored by the reference row.
    pub point: [Option<u32>; 2],
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// Nested `dim_ref` table carried by a named `dimtab_ptr` prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureDimensionReferenceTable {
    /// Count declared by the nested table's `f8` opener.
    pub declared_count: u32,
    /// Entity-table class reference following the nested opener.
    pub entity_ref: Option<u32>,
    /// Named prototype and positional replay rows in stored order.
    pub rows: Vec<FeatureDimensionReference>,
    /// Byte offset of the `dim_ref` label in the original stream.
    pub offset: usize,
}

/// One dimension record from a gsec2d `dimtab_ptr` table.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDimension {
    /// Dimension type discriminator.
    pub dimension_type: u32,
    /// Decoded primary scalar, when its prefix is defined.
    pub value: Option<f64>,
    /// Exact encoded scalar body of the primary value.
    pub value_body: Vec<u8>,
    /// Exact bounded placeholder token when the primary scalar is unresolved.
    pub unresolved_value_token: Option<Vec<u8>>,
    /// Unit interpretation selected by the dimension type.
    pub value_unit: DimensionUnit,
    /// Stored direction byte.
    pub direction_byte: u8,
    /// Decoded auxiliary scalar, when its prefix is defined.
    pub auxiliary_value: Option<f64>,
    /// Exact encoded scalar body of the auxiliary value.
    pub auxiliary_body: Vec<u8>,
    /// External dimension identifier.
    pub external_id: u32,
    /// Nested named-prototype dimension references, when present.
    pub references: Option<FeatureDimensionReferenceTable>,
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
    /// Exact encoded `a`, `b`, and `c` operand-vector block.
    pub operands: Vec<u8>,
    /// Decoded four-slot `a`, `b`, and `c` operand vectors.
    pub operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    /// Stored relation sign selector.
    pub sign: u32,
    /// Stored dimension selector.
    pub dimension_id: u32,
    /// Stored relation-type discriminator.
    pub relation_type: u32,
    /// Complete positional fields before the `e2` row terminator.
    pub body: Vec<u8>,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
}

/// Counted `relat_ptr` constraint-relation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelationTable {
    /// Allocation count declared by the table's `f8` opener. One is the empty
    /// table form; larger counts include two structural entries.
    pub declared_count: u32,
    /// Relation entity-class reference following the opener.
    pub entity_ref: Option<u32>,
    /// Complete positional relation rows in stored order.
    pub rows: Vec<FeatureRelation>,
    /// Section-entity incidence records used by solver equations.
    pub skamps: Vec<FeatureSkamp>,
    /// Count, class, and source location of `skamp_ptr`.
    pub skamp_header: Option<FeatureSolverTableHeader>,
    /// Joins between relation, equation, and incidence identifiers.
    pub triples: Vec<FeatureRelationTriple>,
    /// Count, class, and source location of `triples_ptr`.
    pub triples_header: Option<FeatureSolverTableHeader>,
    /// Byte offset of the `relat_ptr` label in the original stream.
    pub offset: usize,
}

/// Header identity for a counted solver subtable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSolverTableHeader {
    /// Count declared by the table's `f8` opener.
    pub declared_count: u32,
    /// Table-class reference following the count.
    pub entity_ref: u32,
    /// Byte offset of the table label or positional array opener.
    pub offset: usize,
}

/// One entity incidence within a section solver `skamp_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSkampItem {
    /// External section-entity identifier.
    pub entity_id: u32,
    /// Stored endpoint or locus selector.
    pub sense: u32,
}

/// One counted section solver `skamp_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSkamp {
    /// Incidence identifier referenced by `triples_ptr`.
    pub id: u32,
    /// Stored incidence family.
    pub kind: u32,
    /// Stored flags.
    pub flags: u32,
    /// Stored solver status.
    pub status: u32,
    /// Counted entity incidences in stored order.
    pub items: Vec<FeatureSkampItem>,
    /// Byte offset of the row in the original stream.
    pub offset: usize,
}

/// One `triples_ptr` join between solver namespaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRelationTriple {
    /// Relation identifier, or the native null sentinel.
    pub relation_id: Option<u32>,
    /// Equation identifier, or the native null sentinel.
    pub equation_id: Option<u32>,
    /// Incidence identifier, or the native null sentinel.
    pub skamp_id: Option<u32>,
    /// Byte offset of the row in the original stream.
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
    /// Exact row bytes through the final owned token, excluding the structural boundary.
    pub body: Vec<u8>,
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
    /// Exact entity-body or positional-row bytes, excluding the structural boundary.
    pub body: Vec<u8>,
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
    /// Exact entity-body bytes, excluding the following entity boundary.
    pub body: Vec<u8>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One solved conic retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedConic {
    /// Saved-section entity identifier.
    pub entity_id: u32,
    /// Two stored endpoint triples.
    pub endpoints: [[Option<f64>; 3]; 2],
    /// Start and end conic parameters.
    pub parameters: [Option<f64>; 2],
    /// Semi-axis coefficients.
    pub coefficients: [Option<f64>; 2],
    /// Two in-plane axes, positive normal, and origin.
    pub local_system: Option<[f64; 12]>,
    /// Exact entity-body bytes, excluding the following entity boundary.
    pub body: Vec<u8>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One saved interpolation spline retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureSavedSpline {
    /// Saved-section entity identifier, when stored.
    pub entity_id: Option<u32>,
    /// Declared interpolation-point count, when its extent is valid.
    pub declared_point_count: Option<u32>,
    /// Complete interpolation-point prefix in stored parameter order.
    pub interpolation_points: Vec<[f64; 3]>,
    /// Exact `i_pnts` value bytes through the last complete interpolation point.
    pub interpolation_points_body: Vec<u8>,
    /// Two stored endpoint tangent triples, when every scalar is defined.
    pub endpoint_tangents: Option<[[f64; 3]; 2]>,
    /// Exact complete `end_tangts` value bytes, including its array wrapper.
    pub endpoint_tangents_body: Option<Vec<u8>>,
    /// One stored interpolation parameter per point, when complete.
    pub parameters: Option<Vec<f64>>,
    /// Exact complete `params` value bytes, including its array wrapper.
    pub parameters_body: Option<Vec<u8>>,
    /// Byte offset of the entity label in the original stream.
    pub offset: usize,
}

/// One saved placeholder entity without analytic geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSavedDummy {
    /// Saved-section entity identifier, when stored.
    pub entity_id: Option<u32>,
    /// Exact entity-body bytes, excluding the following entity boundary.
    pub body: Vec<u8>,
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
    /// Saved conic entity.
    Conic(FeatureSavedConic),
    /// Saved interpolation-spline entity.
    Spline(FeatureSavedSpline),
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

/// One byte-bounded feature-definition template or instantiated saved section.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureDefinition {
    /// Numeric identifier embedded in `feat_defs_<id>`. A positional replay
    /// inherits that schema identifier until an exact owner join replaces it
    /// with the canonical feature identifier.
    pub id: u32,
    /// Canonical definition owner, joining the definition to its modeling
    /// feature.
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

fn decode_parameter_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    const DICT_PREFIXES: &[u8] = &[
        0x5e, 0x60, 0x68, 0x6f, 0x71, 0x74, 0x81, 0x85, 0x8b, 0x90, 0x91, 0x99, 0xa1, 0xa2, 0xb7,
    ];
    let prefix = *payload.get(offset)?;
    if DICT_PREFIXES.contains(&prefix) && offset + 7 <= end {
        let (first, second) = if prefix == 0xb7 {
            (0x3f, 0xe4)
        } else {
            let second = prefix.wrapping_sub(0x8b);
            (if second >= 0x80 { 0x3f } else { 0x40 }, second)
        };
        let mut raw = [0; 8];
        raw[0] = first;
        raw[1] = second;
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    if let Some((value, next)) =
        scalar::decode_in_lane(payload, offset, cache).filter(|(_, next)| *next <= end)
    {
        return Some((value, next));
    }
    None
}

fn variable_row_trailing_fields(payload: &[u8], mut cursor: usize, end: usize) -> Option<[u32; 3]> {
    let mut fields = [0; 3];
    for field in &mut fields {
        let &head = payload.get(cursor)?;
        if head >= 0xc0 {
            return None;
        }
        let (value, next) = psb::compact_int(payload, cursor);
        (next > cursor && next <= end).then_some(())?;
        *field = value;
        cursor = next;
    }
    (cursor == end).then_some(fields)
}

fn unresolved_variable_guess_end(payload: &[u8], offset: usize, end: usize) -> Option<usize> {
    let delimiter = payload
        .get(offset + 1..end)?
        .iter()
        .position(|&byte| byte == 0xe2)
        .map(|relative| offset + 1 + relative)?;
    let mut suffixes = (offset + 1..delimiter).filter(|&trailing_start| {
        variable_row_trailing_fields(payload, trailing_start, delimiter).is_some()
    });
    let suffix = suffixes.next()?;
    suffixes.next().is_none().then_some(suffix)
}

pub(crate) fn decode_variable_scalar(
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
    if prefix == 0xd5 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0xbf;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if prefix == 0x4f && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if matches!(prefix, 0x19 | 0x28 | 0x32 | 0x37 | 0x41) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8, false);
    }
    if prefix == 0x31 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    let variable_dict = match prefix {
        0x51 => Some([0x3f, 0xc6]),
        0x53..=0xa3 => Some((0x3f75_u16 + u16::from(prefix)).to_be_bytes()),
        0xad => Some([0x3f, 0xd9]),
        0xa7..=0xac | 0xae => Some([0xbf, prefix.wrapping_add(0x2c)]),
        0xb3 => Some([0xbf, 0xe0]),
        0xbd => Some([0xbf, 0xea]),
        0xc3 => Some([0xbf, 0xf0]),
        0xc6..=0xce => Some([0xbf, prefix.wrapping_add(0x2d)]),
        0xd0 => Some([0xbf, 0xfe]),
        0xd2 => Some([0xc0, 0x00]),
        0xd4 => Some([0xc0, 0x02]),
        0xd6 => Some([0xc0, 0x04]),
        0xd8 => Some([0xc0, 0x06]),
        0xda => Some([0xc0, 0x08]),
        0xdd => Some([0xc0, 0x0c]),
        _ => None,
    };
    if let (Some(head), Some(tail)) = (variable_dict, payload.get(offset + 1..offset + 7)) {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&head);
        raw[2..].copy_from_slice(tail);
        return (Some(f64::from_be_bytes(raw)), offset + 7, false);
    }
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x18 | 0xe0 | 0xe2 | 0xe3 | 0x10 | 0xe4 | 0xe6))
    {
        return (Some(0.0), offset + 1, false);
    }
    if prefix == 0x18 && unresolved_variable_guess_end(payload, offset + 1, end).is_some() {
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

pub(crate) fn decode_section_coordinate_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize, bool) {
    match payload.get(offset) {
        Some(0x00 | 0x34) if offset + 3 <= end => return (None, offset + 3, false),
        Some(0x01) if offset + 4 <= end => return (None, offset + 4, false),
        _ => {}
    }
    if payload.get(offset) == Some(&0x2d) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8, false);
    }
    decode_variable_scalar(payload, offset, end, cache)
}

fn decode_variable_guess(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize, bool) {
    if payload.get(offset) == Some(&0x18) {
        let mut trailing = offset + 1;
        let complete_suffix = (0..3).all(|_| {
            if trailing >= end || payload[trailing] >= 0xc0 {
                return false;
            }
            let (_, next) = psb::compact_int(payload, trailing);
            if next <= trailing {
                return false;
            }
            trailing = next;
            true
        });
        if complete_suffix
            && (trailing == end
                || payload
                    .get(trailing)
                    .is_some_and(|byte| matches!(byte, 0xe0..=0xe3 | 0xf1..=0xf3)))
        {
            return (Some(0.0), offset + 1, false);
        }
    }
    let decoded = decode_section_coordinate_scalar(payload, offset, end, cache);
    if decoded.0.is_none() && !decoded.2 {
        if let Some(next) = unresolved_variable_guess_end(payload, offset, end) {
            return (None, next, false);
        }
    }
    decoded
}

pub(crate) fn variable_table(
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
    let named_row = (|| {
        let type_label = find_bytes(payload, b"type\0", cursor, close)?;
        let variable_type = named_compact_int(payload, b"type\0", cursor, close)?;
        let key = named_compact_int(payload, b"key\0", cursor, close)?;
        let value_label = find_bytes(payload, b"value\0", cursor, close)? + b"value\0".len();
        let (value, value_end, dimension_driven) =
            decode_section_coordinate_scalar(payload, value_label, close, cache);
        let guess_label = find_bytes(payload, b"guess\0", cursor, close)? + b"guess\0".len();
        let (guess, guess_end, guess_dimension_driven) =
            decode_section_coordinate_scalar(payload, guess_label, close, cache);
        Some(FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body: payload[value_label..value_end].to_vec(),
            guess,
            guess_body: payload[guess_label..guess_end].to_vec(),
            guess_dimension_driven,
            known: named_compact_int(payload, b"known\0", cursor, close),
            homogeneity: named_compact_int(payload, b"homogeneity\0", cursor, close),
            uvar_id: named_compact_int(payload, b"uvar_id\0", cursor, close),
            dimension_driven,
            offset: type_label.saturating_sub(2),
        })
    })();
    let (_, after_close_ref) = psb::compact_int(payload, close + 2);
    cursor = after_close_ref;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = named_row.into_iter().collect::<Vec<_>>();
    let max_rows = usize::try_from(declared_count)
        .unwrap_or(usize::MAX)
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
        let value_start = cursor;
        let (value, next, dimension_driven) =
            decode_section_coordinate_scalar(payload, cursor, end, cache);
        cursor = next;
        let value_body = payload[value_start..cursor].to_vec();
        let guess_start = cursor;
        let (guess, next, guess_dimension_driven) =
            decode_variable_guess(payload, cursor, end, cache);
        cursor = next;
        let guess_body = payload[guess_start..cursor].to_vec();
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
        let row = FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body,
            guess,
            guess_body,
            guess_dimension_driven,
            known: trailing.first().copied(),
            homogeneity: trailing.get(1).copied(),
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        };
        let Some(delimiter) = payload[cursor..end].iter().position(|&byte| byte == 0xe2) else {
            break;
        };
        cursor += delimiter + 1;
        rows.push(row);
    }
    Some(variable_table_from_rows(
        declared_count,
        entity_ref,
        rows,
        table,
    ))
}

pub(crate) fn variable_table_from_rows(
    declared_count: u32,
    entity_ref: Option<u32>,
    rows: Vec<FeatureVariableRow>,
    offset: usize,
) -> FeatureVariableTable {
    let mut coordinates = BTreeMap::<u32, (Option<f64>, Option<f64>)>::new();
    for row in rows.iter().filter(|row| matches!(row.variable_type, 1 | 2)) {
        coordinates.entry(row.key).or_insert((None, None));
    }
    for (&point_id, point) in &mut coordinates {
        let mut u_rows = rows
            .iter()
            .filter(|row| row.key == point_id && row.variable_type == 1);
        let u = u_rows.next();
        if u_rows.next().is_none() {
            point.0 = u.and_then(|row| row.value);
        }
        let mut v_rows = rows
            .iter()
            .filter(|row| row.key == point_id && row.variable_type == 2);
        let v = v_rows.next();
        if v_rows.next().is_none() {
            point.1 = v.and_then(|row| row.value);
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
        offset,
    }
}

pub(crate) fn positional_variable_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Option<FeatureVariableTable> {
    let mut candidates = (start..end).filter_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then(|| {
            (
                table,
                declared_count,
                after_reference + 2,
                payload[after_count + 1..after_reference].to_vec(),
            )
        })
    });
    let (table, declared_count, mut cursor, reference_bytes) = candidates.next()?;
    // A positional definition has one variable array. Do not bind the first
    // header when another array in the same bounded definition matches it.
    candidates.next().is_none().then_some(())?;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_row_class) = psb::reference_id(payload, cursor + 1).ok()?;
    cursor = after_row_class;

    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    // Each row consumes at least one byte before its 0xe2 separator, so the row
    // count cannot exceed the unread bytes in the table window.
    let capacity =
        bounded_len(u64::from(declared_count), 1, end.saturating_sub(cursor)).unwrap_or(0);
    let mut rows = Vec::with_capacity(capacity);
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    'rows: while cursor < end && rows.len() < row_limit {
        let row_offset = cursor;
        let (variable_type, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (key, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let value_start = cursor;
        let (value, next, dimension_driven) =
            decode_section_coordinate_scalar(payload, cursor, end, cache);
        cursor = next;
        let value_body = payload[value_start..cursor].to_vec();
        let guess_start = cursor;
        let (guess, next, guess_dimension_driven) =
            decode_variable_guess(payload, cursor, end, cache);
        cursor = next;
        let guess_body = payload[guess_start..cursor].to_vec();
        let mut trailing = Vec::with_capacity(3);
        while cursor < end && payload[cursor] != 0xe2 && trailing.len() < 3 {
            if payload[cursor] >= 0xc0 {
                break 'rows;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            if next <= cursor {
                break 'rows;
            }
            trailing.push(field);
            cursor = next;
        }
        let row = FeatureVariableRow {
            variable_type,
            key,
            value,
            value_body,
            guess,
            guess_body,
            guess_dimension_driven,
            known: trailing.first().copied(),
            homogeneity: trailing.get(1).copied(),
            uvar_id: trailing.get(2).copied(),
            dimension_driven,
            offset: row_offset,
        };
        if rows.len() + 1 < row_limit {
            if rows.is_empty() {
                if payload.get(cursor..cursor + prototype_separator.len())
                    != Some(prototype_separator.as_slice())
                {
                    break;
                }
                cursor += prototype_separator.len();
            } else {
                if payload.get(cursor) != Some(&0xe2) {
                    break;
                }
                cursor += 1;
            }
        }
        rows.push(row);
    }
    Some(variable_table_from_rows(
        declared_count,
        Some(table_class),
        rows,
        table,
    ))
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

fn next_solver_int(payload: &[u8], offset: &mut usize) -> Option<u32> {
    let &head = payload.get(*offset)?;
    if (0xc0..=0xdf).contains(&head) {
        let high = *payload.get(*offset + 1)?;
        let low = *payload.get(*offset + 2)?;
        *offset += 3;
        return Some((u32::from(head - 0xc0) << 16) | (u32::from(high) << 8) | u32::from(low));
    }
    if head == 0xea {
        let low = *payload.get(*offset + 1)?;
        let middle = *payload.get(*offset + 2)?;
        let high = *payload.get(*offset + 3)?;
        *offset += 4;
        return Some(u32::from(low) | (u32::from(middle) << 8) | (u32::from(high) << 16));
    }
    next_segment_int(payload, offset)
}

fn next_bounded_compact_int(payload: &[u8], offset: usize) -> Option<(u32, usize)> {
    let head = *payload.get(offset)?;
    if (0x80..=0xbf).contains(&head) {
        payload.get(offset + 1)?;
    }
    let (value, next) = psb::compact_int(payload, offset);
    (next > offset).then_some((value, next))
}

fn next_nullable_segment_int(payload: &[u8], offset: &mut usize) -> Result<Option<u32>, ()> {
    if payload.get(*offset) == Some(&0xf6) {
        *offset += 1;
        return Ok(None);
    }
    next_segment_int(payload, offset).map(Some).ok_or(())
}

fn segment_slots(payload: &[u8], offset: &mut usize, count: usize) -> Option<Vec<Option<u32>>> {
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        match *payload.get(*offset)? {
            0xe4 => {
                values.push(Some(1));
                *offset += 1;
            }
            0xe5 => {
                (values.len() + 2 <= count).then_some(())?;
                values.extend([Some(0), Some(0)]);
                *offset += 1;
            }
            0xe6 => {
                (values.len() + 3 <= count).then_some(())?;
                values.extend([Some(0), Some(0), Some(0)]);
                *offset += 1;
            }
            0xf6 => {
                values.push(None);
                *offset += 1;
            }
            _ => values.push(Some(next_segment_int(payload, offset)?)),
        }
    }
    Some(values)
}

fn equation_argument_slots(payload: &[u8], offset: &mut usize) -> Option<Vec<Option<u32>>> {
    match *payload.get(*offset)? {
        0xe4 => {
            *offset += 1;
            Some(vec![Some(1)])
        }
        0xe5 => {
            *offset += 1;
            Some(vec![Some(0), Some(0)])
        }
        0xe6 => {
            *offset += 1;
            Some(vec![Some(0), Some(0), Some(0)])
        }
        0xf6 => {
            *offset += 1;
            Some(vec![None])
        }
        _ => Some(vec![Some(next_solver_int(payload, offset)?)]),
    }
}

fn equation_arguments(
    payload: &[u8],
    offset: &mut usize,
    end: usize,
    explicit_count: Option<usize>,
) -> Option<Vec<Option<u32>>> {
    let mut arguments = Vec::new();
    while match explicit_count {
        Some(count) => arguments.len() < count,
        None => *offset < end && payload.get(*offset) != Some(&0xf6),
    } {
        let before = *offset;
        let slots = equation_argument_slots(payload, offset)?;
        if *offset <= before
            || *offset > end
            || explicit_count.is_some_and(|count| arguments.len() + slots.len() > count)
        {
            return None;
        }
        arguments.extend(slots);
    }
    explicit_count
        .is_none_or(|count| arguments.len() == count)
        .then_some(arguments)
}

/// Decode the structurally framed `eqtn_arr` solver table in one bounded
/// feature definition.
pub fn equation_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureEquationTable> {
    if start > end || end > payload.len() {
        return None;
    }
    let table = find_bytes(payload, b"eqtn_arr\0", start, end)?;
    let mut cursor = table + b"eqtn_arr\0".len();
    if payload.get(cursor) == Some(&0xf2) {
        cursor += 1;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = next_bounded_compact_int(payload, cursor + 1)?;
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (entity_ref, next) = psb::reference_id(payload, cursor + 1).ok()?;
        cursor = next;
        Some(entity_ref)
    } else {
        None
    };
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return None;
    }
    cursor += 2;

    let rows_end = [
        b"\xe0\x02scale\0".as_slice(),
        b"\xe0\x02scales\0",
        b"\xe0\x02guesses\0",
    ]
    .into_iter()
    .filter_map(|label| find_bytes(payload, label, cursor, end))
    .min()
    .unwrap_or(end);
    let prototype_start = cursor;
    let prototype_reference = find_bytes(
        payload,
        &[0xf1, psb::token::ENTITY_REF],
        prototype_start,
        rows_end,
    )?;
    let (_, after_prototype_reference) =
        psb::reference_id(payload, prototype_reference + 2).ok()?;
    let prototype_end = (payload.get(after_prototype_reference) == Some(&0xe2))
        .then_some(after_prototype_reference + 1)?;
    let prototype_body = payload[prototype_start..prototype_end].to_vec();
    cursor = prototype_end;

    let mut rows = Vec::new();
    while cursor < rows_end {
        let row_start = cursor;
        let Some(equation_id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(function_id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let explicit_argument_count = if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = next_bounded_compact_int(payload, cursor + 1)?;
            cursor = next;
            Some(count)
        } else {
            None
        };
        let arguments_start = cursor;
        let explicit_argument_count_usize = match explicit_argument_count {
            Some(count) => Some(usize::try_from(count).ok()?),
            None => None,
        };
        let Some(arguments) = equation_arguments(
            payload,
            &mut cursor,
            rows_end,
            explicit_argument_count_usize,
        ) else {
            break;
        };
        let arguments_body_end = cursor;
        let auxiliary_start = cursor;
        if payload.get(cursor) != Some(&0xf6) {
            break;
        }
        cursor += 1;
        let auxiliary_body = payload[auxiliary_start..cursor].to_vec();
        let row_end = if payload.get(cursor) == Some(&0xe2) {
            cursor += 1;
            cursor
        } else if cursor == rows_end {
            cursor
        } else {
            break;
        };
        rows.push(FeatureEquation {
            equation_id,
            function_id,
            explicit_argument_count,
            arguments,
            arguments_body: payload[arguments_start..arguments_body_end].to_vec(),
            auxiliary_body,
            body: payload[row_start..row_end].to_vec(),
            offset: row_start,
        });
    }

    Some(FeatureEquationTable {
        declared_count,
        entity_ref,
        prototype_body,
        rows,
        offset: table,
    })
}

/// Decode instantiated placement-instruction rows from one bounded feature
/// definition.
pub fn placement_instructions(definition: &FeatureDefinition) -> Vec<FeaturePlacementInstruction> {
    placement_instruction_rows(&definition.body, definition.offset)
}

pub(crate) fn placement_instruction_rows(
    payload: &[u8],
    definition_offset: usize,
) -> Vec<FeaturePlacementInstruction> {
    let Some(table_class) =
        named_array_class(payload, b"place_instruction_ptrs\0", 0, payload.len())
    else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for marker in 0..payload.len() {
        if payload.get(marker..marker + 2) != Some(&[0xf1, psb::token::ENTITY_REF]) {
            continue;
        }
        let Ok((class, after_class)) = psb::reference_id(payload, marker + 2) else {
            continue;
        };
        if class != table_class || payload.get(after_class) != Some(&psb::token::COMPOUND_CLOSE) {
            continue;
        }
        let mut cursor = after_class + 1;
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            continue;
        };
        let zero_offset = payload.get(cursor) == Some(&0x18);
        if !zero_offset {
            continue;
        }
        cursor += 1;
        let Ok(dimension_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Ok(reference_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Ok(geometry1_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Ok(geometry2_id) = next_nullable_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Some(member1) = next_segment_int(payload, &mut cursor) else {
            continue;
        };
        let Some(member2) = next_segment_int(payload, &mut cursor) else {
            continue;
        };
        rows.push(FeaturePlacementInstruction {
            kind,
            zero_offset,
            dimension_id,
            reference_id,
            geometry1_id,
            geometry2_id,
            member1,
            member2,
            offset: definition_offset + marker,
        });
    }
    rows
}

pub(crate) fn segment_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureSegmentTable> {
    let table = find_bytes(payload, b"segtab_ptr\0", start, end)?;
    let mut cursor = table + b"segtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    segment_table_body(payload, table, cursor, end, false)
}

pub(crate) fn positional_segment_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureSegmentTable> {
    let name_end = find_bytes(payload, b"S2D", start, start.saturating_add(256).min(end))?;
    let cursor = payload[name_end..end].iter().position(|&byte| byte == 0)? + name_end + 1;
    segment_table_body(payload, cursor, cursor, end, true)
}

pub(crate) fn segment_table_body(
    payload: &[u8],
    table: usize,
    mut cursor: usize,
    end: usize,
    has_elided_prototype: bool,
) -> Option<FeatureSegmentTable> {
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
    let (close, after_close_ref) = (cursor..end).find_map(|offset| {
        (payload.get(offset..offset + 2) == Some(&[0xf2, psb::token::ENTITY_REF])).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, offset + 2).ok()?;
        (entity_ref.is_none_or(|expected| class == expected)
            && payload.get(after_reference) == Some(&0xe2))
        .then_some((offset, after_reference))
    })?;
    let named_values = |label: &[u8], count: usize| -> Option<(usize, Vec<Option<u32>>)> {
        let offset = find_bytes(payload, label, cursor, close)?;
        let mut p = offset + label.len();
        if payload.get(p) == Some(&psb::token::ARRAY_OPEN) {
            let (declared, next) = psb::compact_int(payload, p + 1);
            (usize::try_from(declared).ok()? == count).then_some(())?;
            p = next;
        }
        if label == b"type\0" && payload.get(p..p + 2) == Some(&[0xc0, 0x80]) {
            p += 2;
        }
        let values = segment_slots(payload, &mut p, count)?;
        Some((offset, values))
    };
    let named_row = (|| {
        let (offset, kind) = named_values(b"type\0", 1)?;
        let (_, directions) = named_values(b"dir\0", 3)?;
        let (_, point_ids) = named_values(b"pointid\0", 2)?;
        let (_, center_id) = named_values(b"cntrid\0", 1)?;
        let (_, arc_orientation) = named_values(b"arcorient\0", 1)?;
        let (_, vertical_horizontal) = named_values(b"verhor\0", 1)?;
        let (_, radius_ref) = named_values(b"radius\0", 1)?;
        let (_, radius2_ref) = named_values(b"radius2\0", 1)?;
        let (_, external_id) = named_values(b"ext_id\0", 1)?;
        Some(FeatureOpaqueSegment {
            kind: kind[0]?,
            directions: [directions[0], directions[1], directions[2]],
            point_ids: [point_ids[0], point_ids[1]],
            center_id: center_id[0],
            arc_orientation: arc_orientation[0],
            vertical_horizontal: vertical_horizontal[0],
            radius_ref: radius_ref[0],
            radius2_ref: radius2_ref[0],
            external_id: external_id[0]?,
            body: Vec::new(),
            offset,
        })
    })();
    cursor = after_close_ref + 1;
    let region_end = [
        b"order_table".as_slice(),
        b"dimtab_ptr\0",
        b"relat_ptr\0",
        b"var_arr\0",
        b"gsec3d_ptr\0",
        b"order_ptr\0",
        b"p_saved_result\0",
        b"S2D",
    ]
    .into_iter()
    .filter_map(|label| find_bytes(payload, label, cursor, end))
    .min()
    .unwrap_or(end);
    let mut segments = FeatureSegmentTable {
        declared_count,
        has_elided_prototype,
        entity_ref,
        rows: Vec::new(),
        circle_rows: Vec::new(),
        point_rows: Vec::new(),
        centered_line_rows: Vec::new(),
        reference_line_rows: Vec::new(),
        bounded_curve_rows: Vec::new(),
        conic_rows: Vec::new(),
        opaque_rows: Vec::new(),
        offset: table,
    };
    if let Some(row) = named_row {
        retain_segment_row(row, &mut segments);
    }
    let first_row = cursor;
    let row_limit = usize::try_from(declared_count)
        .unwrap_or(usize::MAX)
        .saturating_sub(usize::from(has_elided_prototype));
    while cursor < region_end && segments.retained_row_count() < row_limit {
        let row_start = cursor;
        let kind_offset = if matches!(
            payload.get(cursor..cursor + 2),
            Some([0xc0, 0x80] | [0xc1, 0x00])
        ) {
            cursor + 2
        } else {
            cursor
        };
        if payload.get(kind_offset).is_none_or(|kind| *kind > 0x7f)
            || (row_start != first_row
                && payload.get(row_start.saturating_sub(1)) != Some(&0xe2)
                && payload.get(row_start.saturating_sub(1)) != Some(&0xe3))
        {
            cursor += 1;
            continue;
        }
        let mut p = kind_offset;
        let (kind_raw, next) = segment_int(payload, p);
        p = next;
        let Some(kind) = kind_raw else {
            cursor += 1;
            continue;
        };
        let Some(prefix) = segment_slots(payload, &mut p, 7)
            .and_then(|values| <[Option<u32>; 7]>::try_from(values).ok())
        else {
            cursor += 1;
            continue;
        };
        let directions = [prefix[0], prefix[1], prefix[2]];
        let point0 = prefix[3];
        let point1 = prefix[4];
        let center_id = prefix[5];
        let arc_orientation = prefix[6];
        let verhor_flag = payload.get(p) == Some(&0xf5);
        let vertical_horizontal = if verhor_flag {
            p += 1;
            if segment_slots(payload, &mut p, 1).is_none() {
                cursor += 1;
                continue;
            }
            None
        } else {
            let Some(values) = segment_slots(payload, &mut p, 1) else {
                cursor += 1;
                continue;
            };
            values[0]
        };
        let Some(suffix) = segment_slots(payload, &mut p, 3)
            .and_then(|values| <[Option<u32>; 3]>::try_from(values).ok())
        else {
            cursor += 1;
            continue;
        };
        let radius_ref = suffix[0];
        let radius2_ref = suffix[1];
        let Some(external_id) = suffix[2] else {
            cursor += 1;
            continue;
        };
        if payload.get(p) == Some(&0xe2) {
            retain_segment_row(
                FeatureOpaqueSegment {
                    kind,
                    directions,
                    point_ids: [point0, point1],
                    center_id,
                    arc_orientation,
                    vertical_horizontal,
                    radius_ref,
                    radius2_ref,
                    external_id,
                    body: payload[row_start..=p].to_vec(),
                    offset: row_start,
                },
                &mut segments,
            );
            cursor = p + 1;
        } else {
            cursor += 1;
        }
    }
    Some(segments)
}

fn retain_segment_row(row: FeatureOpaqueSegment, segments: &mut FeatureSegmentTable) {
    if row.kind == 10
        && row.directions == [Some(0); 3]
        && row.point_ids == [None, Some(1)]
        && row.arc_orientation == Some(0)
        && row.vertical_horizontal == Some(0)
        && row.radius2_ref.is_none()
    {
        if let (Some(center_id), Some(radius_ref)) = (row.center_id, row.radius_ref) {
            segments.circle_rows.push(FeatureCircleSegment {
                center_id,
                radius_ref,
                external_id: row.external_id,
                offset: row.offset,
            });
            return;
        }
    }
    if row.kind == 1
        && row.directions == [Some(0); 3]
        && row.point_ids == [None, Some(1)]
        && row.arc_orientation == Some(0)
        && row.vertical_horizontal == Some(0)
        && row.radius_ref.is_none()
        && row.radius2_ref.is_none()
    {
        if let Some(point_id) = row.center_id {
            segments.point_rows.push(FeaturePointSegment {
                point_id,
                external_id: row.external_id,
                offset: row.offset,
            });
            return;
        }
    }
    if row.kind == 47
        && row.directions == [Some(0); 3]
        && row.point_ids == [None, Some(1)]
        && row.arc_orientation == Some(0)
        && row.vertical_horizontal == Some(0)
        && row.radius_ref == Some(1)
        && row.radius2_ref.is_none()
    {
        if let Some(center_id) = row.center_id {
            segments
                .centered_line_rows
                .push(FeatureCenteredLineSegment {
                    center_id,
                    external_id: row.external_id,
                    offset: row.offset,
                });
            return;
        }
    }
    if row.kind == 25
        && row.center_id.is_none()
        && row.arc_orientation == Some(0)
        && row.radius_ref.is_none()
        && row.radius2_ref.is_none()
    {
        segments
            .reference_line_rows
            .push(FeatureReferenceLineSegment {
                directions: row.directions,
                point_ids: row.point_ids,
                vertical_horizontal: row.vertical_horizontal,
                external_id: row.external_id,
                offset: row.offset,
            });
        return;
    }
    if row.kind == 12 {
        if let [Some(first), Some(second)] = row.point_ids {
            segments
                .bounded_curve_rows
                .push(FeatureBoundedCurveSegment {
                    directions: row.directions,
                    point_ids: [first, second],
                    center_id: row.center_id,
                    arc_orientation: row.arc_orientation,
                    vertical_horizontal: row.vertical_horizontal,
                    radius_ref: row.radius_ref,
                    radius2_ref: row.radius2_ref,
                    external_id: row.external_id,
                    offset: row.offset,
                });
            return;
        }
    }
    if row.kind == 58
        && row.directions == [Some(0); 3]
        && row.point_ids == [None, Some(1)]
        && row.arc_orientation == Some(0)
        && row.vertical_horizontal == Some(2)
    {
        if let (Some(center_id), Some(first_coefficient_ref), Some(second_coefficient_ref)) =
            (row.center_id, row.radius_ref, row.radius2_ref)
        {
            segments.conic_rows.push(FeatureConicSegment {
                center_id,
                first_coefficient_ref,
                second_coefficient_ref,
                external_id: row.external_id,
                offset: row.offset,
            });
            return;
        }
    }
    let kind = match row.kind {
        2 => FeatureSegmentKind::Line,
        3 => FeatureSegmentKind::Arc,
        5 => FeatureSegmentKind::Point,
        _ => {
            segments.opaque_rows.push(row);
            return;
        }
    };
    let Some(point0) = row.point_ids[0] else {
        return;
    };
    let point1 = if kind == FeatureSegmentKind::Point {
        point0
    } else {
        let Some(point1) = row.point_ids[1] else {
            return;
        };
        point1
    };
    segments.rows.push(FeatureSegment {
        kind,
        directions: row.directions,
        point_ids: [point0, point1],
        center_id: row.center_id,
        arc_orientation: row.arc_orientation,
        vertical_horizontal: row.vertical_horizontal,
        radius_ref: row.radius_ref,
        radius2_ref: row.radius2_ref,
        external_id: row.external_id,
        body: row.body,
        offset: row.offset,
    });
}

fn trim_entity_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureTrimEntityTable> {
    let table = find_bytes(payload, b"ent_tab\0", start, end)?;
    let header = trim_table_header(payload, b"ent_tab\0", start, end);
    let prototype = find_bytes(payload, b"entry_ptr(entity_entry)", table, end)?;
    let mut cursor = header
        .and_then(|header| {
            (prototype..end).find_map(|offset| {
                (payload.get(offset..offset + 3) == Some(&[0xf4, 0x04, psb::token::ENTITY_REF]))
                    .then_some(())?;
                let (class, after_reference) = psb::reference_id(payload, offset + 3).ok()?;
                (class == header.classes.table && payload.get(after_reference) == Some(&0xe2))
                    .then_some(after_reference + 1)
            })
        })
        .or_else(|| {
            let close = find_bytes(payload, &[0xf2, psb::token::ENTITY_REF], prototype, end)?;
            let (_, after_reference) = psb::reference_id(payload, close + 2).ok()?;
            Some(after_reference)
        })?;
    if payload.get(cursor) == Some(&0xe3) {
        cursor += 1;
    }
    let first_row = cursor;
    let region_end = find_bytes(payload, b"vert_tab", cursor, end).unwrap_or(end);
    let buckets = header.map_or_else(Vec::new, |header| {
        trim_buckets(payload, table, region_end, header, TrimEntryKind::Entity)
    });
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    while cursor < region_end {
        if cursor != first_row && payload.get(cursor.saturating_sub(1)) != Some(&0xe3) {
            cursor += 1;
            continue;
        }
        let row_offset = cursor;
        let mut p = row_offset;
        let external_id = next_segment_int(payload, &mut p);
        let mode = next_segment_int(payload, &mut p);
        let start_vertex = next_segment_int(payload, &mut p);
        let end_vertex = next_segment_int(payload, &mut p);
        let center_vertex = next_segment_int(payload, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && payload.get(p) == Some(&0) {
                seen.insert(external_id);
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
                    offset: row_offset,
                });
            }
        }
        cursor += 1;
    }
    Some(FeatureTrimEntityTable {
        declared_count: header.map(|header| header.declared_count),
        entity_ref: header.map(|header| header.classes.table),
        entry_ref: header.map(|header| header.classes.entry),
        buckets,
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrimTableClasses {
    pub(crate) table: u32,
    pub(crate) bucket: u32,
    pub(crate) entry: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrimTableHeader {
    pub(crate) declared_count: u32,
    pub(crate) classes: TrimTableClasses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrimEntryKind {
    Entity,
    Vertex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrimBucketStart {
    pub(crate) index: u32,
    pub(crate) declared_entry_count: u32,
    pub(crate) offset: usize,
    pub(crate) body_start: usize,
}

pub(crate) fn trim_buckets(
    payload: &[u8],
    table: usize,
    end: usize,
    header: TrimTableHeader,
    kind: TrimEntryKind,
) -> Vec<FeatureTrimBucket> {
    if header.declared_count == 0 {
        return Vec::new();
    }
    let Some(label) = find_bytes(payload, b"bucket_index\0", table, end) else {
        return Vec::new();
    };
    let first_offset = label + b"bucket_index\0".len();
    let (Some(first), mut cursor) = segment_int(payload, first_offset) else {
        return Vec::new();
    };
    if first != 0 {
        return Vec::new();
    }
    let Some((first_count, first_body)) =
        named_trim_bucket_count(payload, cursor, end, header.classes.bucket)
    else {
        return Vec::new();
    };
    let mut starts = vec![TrimBucketStart {
        index: first,
        declared_entry_count: first_count,
        offset: first_offset,
        body_start: first_body,
    }];
    while starts.len() < usize::try_from(header.declared_count).unwrap_or(usize::MAX) {
        let expected = u32::try_from(starts.len()).unwrap_or(u32::MAX);
        let Some((offset, index, next)) = (cursor..end).find_map(|offset| {
            (payload.get(offset.saturating_sub(1)) == Some(&0xe2)).then_some(())?;
            let (Some(index), next) = segment_int(payload, offset) else {
                return None;
            };
            (index == expected).then_some((offset, index, next))
        }) else {
            break;
        };
        let Some((declared_entry_count, body_start)) =
            positional_trim_bucket_count(payload, next, end, header.classes)
        else {
            break;
        };
        starts.push(TrimBucketStart {
            index,
            declared_entry_count,
            offset,
            body_start,
        });
        cursor = next;
    }
    starts
        .iter()
        .enumerate()
        .map(|(position, start)| {
            let body_end = starts
                .get(position + 1)
                .map_or(end, |next| next.offset.saturating_sub(1));
            FeatureTrimBucket {
                index: start.index,
                declared_entry_count: start.declared_entry_count,
                decoded_entry_count: trim_bucket_entry_count(
                    payload,
                    start.body_start,
                    body_end,
                    header.classes,
                    kind,
                    position == 0,
                ),
                offset: start.offset,
            }
        })
        .collect()
}

fn named_trim_bucket_count(
    payload: &[u8],
    start: usize,
    end: usize,
    bucket_class: u32,
) -> Option<(u32, usize)> {
    let label = find_bytes(payload, b"bucket_xar\0", start, end)? + b"bucket_xar\0".len();
    let opener = (label..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    trim_bucket_array_count(payload, opener, bucket_class)
}

fn positional_trim_bucket_count(
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    classes: TrimTableClasses,
) -> Option<(u32, usize)> {
    match payload.get(cursor)? {
        &psb::token::ARRAY_OPEN => trim_bucket_array_count(payload, cursor, classes.bucket),
        0xf0 => {
            (payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (class, next) = psb::reference_id(payload, cursor + 2).ok()?;
            (class == classes.bucket).then_some(())?;
            cursor = next;
            (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
            trim_bucket_array_count(payload, cursor, classes.bucket)
        }
        0xf1 => {
            (payload.get(cursor + 1) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (class, next) = psb::reference_id(payload, cursor + 2).ok()?;
            (class == classes.table && payload.get(next) == Some(&0xe2)).then_some((0, next + 1))
        }
        0xe2 | 0xe0 if cursor < end => Some((0, cursor + 1)),
        _ => None,
    }
}

fn trim_bucket_array_count(
    payload: &[u8],
    opener: usize,
    bucket_class: u32,
) -> Option<(u32, usize)> {
    let (count, after_count) = psb::compact_int(payload, opener + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
    (class == bucket_class
        && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe3]))
    .then_some((count, after_reference + 2))
}

fn trim_bucket_entry_count(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    kind: TrimEntryKind,
    named_first: bool,
) -> u32 {
    match kind {
        TrimEntryKind::Entity => {
            let rows = (start..end)
                .filter(|&offset| {
                    payload.get(offset.saturating_sub(1)) == Some(&0xe3)
                        && complete_trim_entity_entry(payload, offset, end)
                })
                .count();
            let prototype = usize::from(
                named_first && named_trim_entity_prototype_complete(payload, start, end, classes),
            );
            u32::try_from(rows.saturating_add(prototype)).unwrap_or(u32::MAX)
        }
        TrimEntryKind::Vertex => {
            let mut rows = BTreeSet::new();
            for offset in start..end {
                if payload.get(offset) == Some(&psb::token::ENTITY_REF) {
                    if let Ok((class, row)) = psb::reference_id(payload, offset + 1) {
                        if class == classes.entry && trim_vertex_entry(payload, row, end).is_some()
                        {
                            rows.insert(row);
                        }
                    }
                }
                if payload.get(offset.saturating_sub(1)) == Some(&0xe3)
                    && trim_vertex_entry(payload, offset, end).is_some()
                {
                    rows.insert(offset);
                }
            }
            let prototype = usize::from(
                named_first && named_trim_vertex_prototype_complete(payload, start, end, classes),
            );
            u32::try_from(rows.len().saturating_add(prototype)).unwrap_or(u32::MAX)
        }
    }
}

fn complete_trim_entity_entry(payload: &[u8], offset: usize, end: usize) -> bool {
    let mut cursor = offset;
    for _ in 0..5 {
        let Some(next) = trim_entry_field(payload, cursor, end) else {
            return false;
        };
        cursor = next;
    }
    cursor < end && payload.get(cursor) == Some(&0)
}

pub(crate) fn trim_vertex_entry(
    payload: &[u8],
    offset: usize,
    end: usize,
) -> Option<(Vec<u32>, u32, usize)> {
    let mut cursor = offset;
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let mut entities = Vec::with_capacity(usize::try_from(count).ok()?);
        for _ in 0..count {
            let (value, next) = segment_int(payload, cursor);
            entities.push(value?);
            (next <= end).then_some(())?;
            cursor = next;
        }
        let (vertex_id, next) = segment_int(payload, cursor);
        let vertex_id = vertex_id?;
        return (next < end && payload.get(next) == Some(&0)).then_some((
            entities,
            vertex_id,
            next + 1,
        ));
    }
    let mut values = Vec::new();
    while cursor < end && payload.get(cursor) != Some(&0) {
        let (value, next) = segment_int(payload, cursor);
        values.push(value?);
        (next <= end).then_some(())?;
        cursor = next;
        if values.len() > 64 {
            return None;
        }
    }
    let vertex_id = values.pop()?;
    (values.len() >= 2 && cursor < end).then_some((values, vertex_id, cursor + 1))
}

fn trim_entry_field(payload: &[u8], offset: usize, end: usize) -> Option<usize> {
    let &head = payload.get(offset)?;
    let next = match head {
        0..=0x7f | 0xf6 => offset + 1,
        0x80..=0xbf if offset + 1 < end => offset + 2,
        _ => return None,
    };
    (next <= end).then_some(next)
}

fn named_trim_entity_prototype_complete(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
) -> bool {
    let entry_label = b"entry_ptr(entity_entry)\0";
    let Some(entry) = find_bytes(payload, entry_label, start, end) else {
        return false;
    };
    let mut cursor = entry + entry_label.len();
    if payload.get(cursor) != Some(&0xe3) {
        return false;
    }
    cursor += 1;
    let labels = [
        b"xid\0".as_slice(),
        b"ent_mode\0",
        b"start_vtx\0",
        b"end_vtx\0",
        b"center_vtx\0",
        b"pers_attribs\0",
    ];
    for label in labels {
        let Some(offset) = find_bytes(payload, label, cursor, end) else {
            return false;
        };
        let Some(next) = trim_entry_field(payload, offset + label.len(), end) else {
            return false;
        };
        cursor = next;
    }
    (cursor..end).any(|offset| {
        if payload.get(offset..offset + 3) != Some(&[0xf4, 0x04, psb::token::ENTITY_REF]) {
            return false;
        }
        psb::reference_id(payload, offset + 3)
            .is_ok_and(|(class, next)| class == classes.table && payload.get(next) == Some(&0xe2))
    })
}

fn named_trim_vertex_prototype_complete(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
) -> bool {
    let Some(entity_ids) = find_bytes(payload, b"ent_ids\0", start, end) else {
        return false;
    };
    let array = entity_ids + b"ent_ids\0".len();
    if payload.get(array) != Some(&psb::token::ARRAY_OPEN) {
        return false;
    }
    let (count, mut cursor) = psb::compact_int(payload, array + 1);
    if count < 2 {
        return false;
    }
    for _ in 0..count {
        let (value, next) = segment_int(payload, cursor);
        if value.is_none() || next > end {
            return false;
        }
        cursor = next;
    }
    let Some(vertex_id) = find_bytes(payload, b"vertex_id\0", cursor, end) else {
        return false;
    };
    let (vertex, next) = segment_int(payload, vertex_id + b"vertex_id\0".len());
    if vertex.is_none() || next > end {
        return false;
    }
    let Some(attributes) = find_bytes(payload, b"attribs\0", next, end) else {
        return false;
    };
    let Some(next) = trim_entry_field(payload, attributes + b"attribs\0".len(), end) else {
        return false;
    };
    (next..end).any(|offset| {
        if payload.get(offset..offset + 2) != Some(&[0xf3, psb::token::ENTITY_REF]) {
            return false;
        }
        psb::reference_id(payload, offset + 2)
            .is_ok_and(|(class, next)| class == classes.table && payload.get(next) == Some(&0xe2))
    })
}

pub(crate) fn trim_table_header(
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Option<TrimTableHeader> {
    let table = find_bytes(payload, label, start, end)? + label.len();
    let opener = (table..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    let (declared_count, after_count) = psb::compact_int(payload, opener + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (table_class, _) = psb::reference_id(payload, after_count + 1).ok()?;
    let bucket_label = find_bytes(payload, b"bucket_xar\0", table, end)? + b"bucket_xar\0".len();
    let bucket_opener =
        (bucket_label..end).find(|&offset| payload[offset] == psb::token::ARRAY_OPEN)?;
    let (_, after_bucket_count) = psb::compact_int(payload, bucket_opener + 1);
    (payload.get(after_bucket_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (bucket_class, _) = psb::reference_id(payload, after_bucket_count + 1).ok()?;
    let entry_class = (after_count..end).find_map(|offset| {
        (payload.get(offset) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, offset + 1).ok()?;
        if label == b"vert_tab\0" {
            let (first, next) = segment_int(payload, after_reference);
            let (second, next) = segment_int(payload, next);
            let (third, next) = segment_int(payload, next);
            return (class != table_class
                && first.is_some()
                && second.is_some()
                && third.is_some()
                && payload.get(next) == Some(&0))
            .then_some(class);
        }
        (payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])).then_some(class)
    })?;
    Some(TrimTableHeader {
        declared_count,
        classes: TrimTableClasses {
            table: table_class,
            bucket: bucket_class,
            entry: entry_class,
        },
    })
}

fn positional_table_region(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    next_table_class: Option<u32>,
) -> Option<(usize, u32, usize, usize)> {
    let (table, declared_count, rows_start) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then_some((table, declared_count, after_reference + 2))
    })?;
    let region_end = next_table_class
        .and_then(|next_class| {
            (rows_start..end).find(|&offset| {
                if payload.get(offset) != Some(&psb::token::ARRAY_OPEN) {
                    return false;
                }
                let (_, after_count) = psb::compact_int(payload, offset + 1);
                if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
                    return false;
                }
                psb::reference_id(payload, after_count + 1).is_ok_and(|(class, after_reference)| {
                    class == next_class
                        && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2])
                })
            })
        })
        .unwrap_or(end);
    Some((table, declared_count, rows_start, region_end))
}

pub(crate) fn positional_trim_entity_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    next_table_class: Option<u32>,
) -> Option<FeatureTrimEntityTable> {
    let TrimTableClasses {
        table: table_class,
        entry: entry_class,
        ..
    } = classes;
    let (table, declared_count, rows_start, region_end) =
        positional_table_region(payload, start, end, table_class, next_table_class)?;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    let has_entry_class = (rows_start..region_end).any(|offset| {
        if payload.get(offset) != Some(&psb::token::ENTITY_REF) {
            return false;
        }
        psb::reference_id(payload, offset + 1).is_ok_and(|(class, after_reference)| {
            class == entry_class
                && payload.get(after_reference..after_reference + 2) == Some(&[0, 0xe3])
        })
    });
    let mut cursor = if declared_count == 0 || has_entry_class {
        rows_start
    } else {
        region_end
    };
    while cursor < region_end {
        if cursor == rows_start || payload.get(cursor.saturating_sub(1)) != Some(&0xe3) {
            cursor += 1;
            continue;
        }
        let row_offset = cursor;
        let mut p = row_offset;
        let external_id = next_segment_int(payload, &mut p);
        let mode = next_segment_int(payload, &mut p);
        let start_vertex = next_segment_int(payload, &mut p);
        let end_vertex = next_segment_int(payload, &mut p);
        let center_vertex = next_segment_int(payload, &mut p);
        if let (Some(external_id), Some(start_vertex), Some(end_vertex)) =
            (external_id, start_vertex, end_vertex)
        {
            if external_id != 0 && payload.get(p) == Some(&0) {
                seen.insert(external_id);
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
                    offset: row_offset,
                });
            }
        }
        cursor += 1;
    }
    Some(FeatureTrimEntityTable {
        declared_count: Some(declared_count),
        entity_ref: Some(table_class),
        entry_ref: Some(entry_class),
        buckets: trim_buckets(
            payload,
            table,
            region_end,
            TrimTableHeader {
                declared_count,
                classes,
            },
            TrimEntryKind::Entity,
        ),
        solved_external_ids: seen.into_iter().collect(),
        rows,
        offset: table,
    })
}

fn trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let table = find_bytes(payload, b"vert_tab\0", start, end)?;
    let header = trim_table_header(payload, b"vert_tab\0", start, end);
    let region_end = [
        b"skamp_ptr\0".as_slice(),
        b"triples_ptr\0",
        b"order_table\0",
        b"dimtab_ptr\0",
        b"relat_ptr\0",
        b"p_saved_result\0",
        b"S2D",
    ]
    .into_iter()
    .filter_map(|label| find_bytes(payload, label, table + b"vert_tab\0".len(), end))
    .min()
    .unwrap_or(end);
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
    cursor = find_bytes(payload, &block_marker, reference_end, region_end)?;

    let mut rows = Vec::new();
    while cursor < region_end {
        if payload.get(cursor..cursor + block_marker.len()) == Some(block_marker.as_slice()) {
            cursor += block_marker.len();
            let (_, next) = segment_int(payload, cursor);
            cursor = next;
            continue;
        }
        match payload[cursor] {
            psb::token::ARRAY_OPEN => {
                if let Some((entities, vertex_id, next)) =
                    trim_vertex_entry(payload, cursor, region_end)
                {
                    rows.push(FeatureTrimVertex {
                        section_coordinates: trim_vertex_intersection(
                            &entities, segments, variables,
                        ),
                        vertex_id,
                        entities,
                        offset: cursor,
                    });
                    cursor = next;
                } else {
                    let (_, next) = psb::compact_int(payload, cursor + 1);
                    cursor = next;
                }
                continue;
            }
            psb::token::ENTITY_REF => {
                let Ok((class, next)) = psb::reference_id(payload, cursor + 1) else {
                    cursor += 1;
                    continue;
                };
                if header.is_some_and(|header| class == header.classes.entry) {
                    if let Some((entities, vertex_id, after_entry)) =
                        trim_vertex_entry(payload, next, region_end)
                    {
                        rows.push(FeatureTrimVertex {
                            section_coordinates: trim_vertex_intersection(
                                &entities, segments, variables,
                            ),
                            vertex_id,
                            entities,
                            offset: next,
                        });
                        cursor = after_entry;
                        continue;
                    }
                }
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
        let Some((entities, vertex_id, next)) = trim_vertex_entry(payload, cursor, region_end)
        else {
            cursor += 1;
            continue;
        };
        rows.push(FeatureTrimVertex {
            section_coordinates: trim_vertex_intersection(&entities, segments, variables),
            vertex_id,
            entities,
            offset: row_offset,
        });
        cursor = next;
    }
    Some(FeatureTrimVertexTable {
        declared_count: header.map(|header| header.declared_count),
        entity_ref: header.map(|header| header.classes.table),
        entry_ref: header.map(|header| header.classes.entry),
        buckets: header.map_or_else(Vec::new, |header| {
            trim_buckets(payload, table, region_end, header, TrimEntryKind::Vertex)
        }),
        rows,
        offset: table,
    })
}

pub(crate) fn positional_trim_vertex_table(
    payload: &[u8],
    start: usize,
    end: usize,
    classes: TrimTableClasses,
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<FeatureTrimVertexTable> {
    let TrimTableClasses {
        table: table_class,
        entry: entry_class,
        ..
    } = classes;
    let (table, declared_count, rows_start, region_end) =
        positional_table_region(payload, start, end, table_class, None)?;
    let mut rows = Vec::new();
    let mut cursor = rows_start;
    while cursor < region_end {
        if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
            cursor += 1;
            continue;
        }
        let Ok((class, after_reference)) = psb::reference_id(payload, cursor + 1) else {
            cursor += 1;
            continue;
        };
        if class != entry_class {
            cursor += 1;
            continue;
        }
        let row_offset = after_reference;
        let Some((entities, vertex_id, next)) = trim_vertex_entry(payload, row_offset, region_end)
        else {
            cursor += 1;
            continue;
        };
        rows.push(FeatureTrimVertex {
            section_coordinates: trim_vertex_intersection(&entities, segments, variables),
            vertex_id,
            entities,
            offset: row_offset,
        });
        cursor = next.max(cursor + 1);
    }
    Some(FeatureTrimVertexTable {
        declared_count: Some(declared_count),
        entity_ref: Some(table_class),
        entry_ref: Some(entry_class),
        buckets: trim_buckets(
            payload,
            table,
            region_end,
            TrimTableHeader {
                declared_count,
                classes,
            },
            TrimEntryKind::Vertex,
        ),
        rows,
        offset: table,
    })
}

const TRIM_COORDINATE_EPS: f64 = 1e-9;
const TRIM_INTERSECTION_EPS: f64 = 1e-12;

#[derive(Clone, Copy)]
enum TrimCarrier {
    Line { start: [f64; 2], end: [f64; 2] },
    Circle { center: [f64; 2], radius: f64 },
}

fn trim_vertex_intersection(
    entities: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    entity_intersection(entities, segments, variables)
}

fn resolved_trim_scalar(
    variables: &FeatureVariableTable,
    variable_type: u32,
    key: u32,
) -> Result<Option<f64>, ()> {
    let values = variables
        .rows
        .iter()
        .filter(|row| row.variable_type == variable_type && row.key == key)
        .map(|row| row.value)
        .collect::<Vec<_>>();
    if values.is_empty() || values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let values = values.into_iter().collect::<Option<Vec<_>>>().ok_or(())?;
    let first = *values.first().ok_or(())?;
    if !first.is_finite() {
        return Err(());
    }
    values
        .iter()
        .copied()
        .try_fold(first, |first, value| {
            if !value.is_finite() {
                return Err(());
            }
            let scale = first.abs().max(value.abs()).max(1.0);
            ((value - first).abs() <= TRIM_COORDINATE_EPS * scale)
                .then_some(first)
                .ok_or(())
        })
        .map(Some)
}

fn trim_endpoint_radius(
    segment: &FeatureSegment,
    center: [f64; 2],
    points: &BTreeMap<u32, [Option<f64>; 2]>,
) -> Result<Option<f64>, ()> {
    let mut radii = Vec::new();
    for point_id in segment.point_ids {
        let Some([Some(u), Some(v)]) = points.get(&point_id).copied() else {
            continue;
        };
        let radius = (u - center[0]).hypot(v - center[1]);
        if !radius.is_finite() || radius <= TRIM_INTERSECTION_EPS {
            return Err(());
        }
        radii.push(radius);
    }
    let Some(first) = radii.first().copied() else {
        return Ok(None);
    };
    radii
        .iter()
        .copied()
        .try_fold(first, |first, radius| {
            let scale = first.abs().max(radius.abs()).max(1.0);
            ((radius - first).abs() <= TRIM_COORDINATE_EPS * scale)
                .then_some(first)
                .ok_or(())
        })
        .map(Some)
}

fn trim_radius(
    segment: &FeatureSegment,
    center: [f64; 2],
    points: &BTreeMap<u32, [Option<f64>; 2]>,
    variables: &FeatureVariableTable,
) -> Option<f64> {
    let stored = resolved_trim_scalar(variables, 3, segment.radius_ref?).ok()?;
    let endpoint = trim_endpoint_radius(segment, center, points).ok()?;
    let radius = match (stored, endpoint) {
        (Some(stored), Some(endpoint)) => {
            let scale = stored.abs().max(endpoint.abs()).max(1.0);
            ((stored - endpoint).abs() <= TRIM_COORDINATE_EPS * scale).then_some(stored)?
        }
        (Some(stored), None) | (None, Some(stored)) => stored,
        (None, None) => return None,
    };
    radius
        .is_finite()
        .then_some(radius)
        .filter(|radius| *radius > 0.0)
}

fn trim_carrier(
    segment: &FeatureSegment,
    points: &BTreeMap<u32, [Option<f64>; 2]>,
    variables: &FeatureVariableTable,
) -> Option<TrimCarrier> {
    let point = |point_id| {
        let coordinates = points.get(&point_id).copied()?;
        let [Some(u), Some(v)] = coordinates else {
            return None;
        };
        (u.is_finite() && v.is_finite()).then_some([u, v])
    };
    match segment.kind {
        FeatureSegmentKind::Line => {
            let start = point(segment.point_ids[0])?;
            let end = point(segment.point_ids[1])?;
            let scale = start
                .into_iter()
                .chain(end)
                .map(f64::abs)
                .fold(1.0, f64::max);
            ((end[0] - start[0]).hypot(end[1] - start[1]) > TRIM_INTERSECTION_EPS * scale)
                .then_some(TrimCarrier::Line { start, end })
        }
        FeatureSegmentKind::Arc => {
            let center = point(segment.center_id?)?;
            let radius = trim_radius(segment, center, points, variables)?;
            Some(TrimCarrier::Circle { center, radius })
        }
        FeatureSegmentKind::Point => None,
    }
}

fn trim_line_line_intersection(
    first_start: [f64; 2],
    first_end: [f64; 2],
    second_start: [f64; 2],
    second_end: [f64; 2],
) -> Option<[f64; 2]> {
    let first_direction = [first_end[0] - first_start[0], first_end[1] - first_start[1]];
    let second_direction = [
        second_end[0] - second_start[0],
        second_end[1] - second_start[1],
    ];
    let denominator = first_direction[0].mul_add(
        second_direction[1],
        -(first_direction[1] * second_direction[0]),
    );
    let scale = first_direction
        .into_iter()
        .chain(second_direction)
        .map(f64::abs)
        .fold(1.0, f64::max);
    if denominator.abs() <= TRIM_INTERSECTION_EPS * scale * scale {
        return None;
    }
    let relative = [
        second_start[0] - first_start[0],
        second_start[1] - first_start[1],
    ];
    let first_parameter = relative[0]
        .mul_add(second_direction[1], -(relative[1] * second_direction[0]))
        / denominator;
    let coordinate = [
        first_start[0] + first_parameter * first_direction[0],
        first_start[1] + first_parameter * first_direction[1],
    ];
    coordinate
        .into_iter()
        .all(f64::is_finite)
        .then_some(coordinate)
}

fn trim_line_circle_intersection(
    start: [f64; 2],
    end: [f64; 2],
    center: [f64; 2],
    radius: f64,
) -> Option<[f64; 2]> {
    let direction = [end[0] - start[0], end[1] - start[1]];
    let relative = [start[0] - center[0], start[1] - center[1]];
    let quadratic = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    if quadratic <= 0.0 || !quadratic.is_finite() {
        return None;
    }
    let linear = 2.0 * relative[0].mul_add(direction[0], relative[1] * direction[1]);
    let constant = relative[0].mul_add(relative[0], relative[1] * relative[1]) - radius * radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    let tolerance = TRIM_INTERSECTION_EPS * scale * scale;
    if !discriminant.is_finite() || discriminant < -tolerance {
        return None;
    }
    let in_segment =
        |parameter: f64| (-TRIM_COORDINATE_EPS..=1.0 + TRIM_COORDINATE_EPS).contains(&parameter);
    let point = |parameter: f64| {
        let coordinate = [
            start[0] + parameter * direction[0],
            start[1] + parameter * direction[1],
        ];
        coordinate
            .into_iter()
            .all(f64::is_finite)
            .then_some(coordinate)
    };
    if discriminant.abs() <= tolerance {
        let parameter = -linear / (2.0 * quadratic);
        return in_segment(parameter)
            .then(|| point(parameter.clamp(0.0, 1.0)))
            .flatten();
    }
    let root = discriminant.sqrt();
    let parameters = [
        (-linear + root) / (2.0 * quadratic),
        (-linear - root) / (2.0 * quadratic),
    ];
    let mut matching = parameters
        .into_iter()
        .filter(|parameter| in_segment(*parameter));
    let parameter = matching.next()?;
    matching
        .next()
        .is_none()
        .then(|| point(parameter.clamp(0.0, 1.0)))
        .flatten()
}

fn trim_circle_circle_intersection(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
) -> Option<[f64; 2]> {
    let delta = [
        second_center[0] - first_center[0],
        second_center[1] - first_center[1],
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius).max(second_radius).max(1.0);
    if !distance.is_finite() || distance <= TRIM_INTERSECTION_EPS * scale {
        return None;
    }
    let external = first_radius + second_radius;
    let internal = (first_radius - second_radius).abs();
    if distance < internal - TRIM_COORDINATE_EPS * scale
        || distance > external + TRIM_COORDINATE_EPS * scale
    {
        return None;
    }
    let axial = (first_radius * first_radius - second_radius * second_radius + distance * distance)
        / (2.0 * distance);
    let height_squared = first_radius.mul_add(first_radius, -(axial * axial));
    let tolerance = TRIM_INTERSECTION_EPS * scale * scale;
    if !height_squared.is_finite() || height_squared.abs() > tolerance {
        return None;
    }
    let direction = [delta[0] / distance, delta[1] / distance];
    let coordinate = [
        first_center[0] + axial * direction[0],
        first_center[1] + axial * direction[1],
    ];
    coordinate
        .into_iter()
        .all(f64::is_finite)
        .then_some(coordinate)
}

pub(crate) fn entity_intersection(
    entity_ids: &[u32],
    segments: Option<&FeatureSegmentTable>,
    variables: Option<&FeatureVariableTable>,
) -> Option<[f64; 2]> {
    let segments = segments?;
    let variables = variables?;
    (variables.is_complete() && entity_ids.len() >= 2).then_some(())?;
    let mut unique_entities = BTreeSet::new();
    if !entity_ids
        .iter()
        .all(|entity_id| unique_entities.insert(*entity_id))
    {
        return None;
    }
    let (points, ambiguous_points) = variables.reconciled_points();
    let segments = entity_ids
        .iter()
        .map(|entity_id| segments.segment(*entity_id))
        .collect::<Option<Vec<_>>>()?;
    let common_point_ids = segments
        .iter()
        .skip(1)
        .fold(
            segments[0].point_ids.into_iter().collect::<BTreeSet<_>>(),
            |common, segment| {
                let segment_points = segment.point_ids.into_iter().collect::<BTreeSet<_>>();
                common.intersection(&segment_points).copied().collect()
            },
        )
        .into_iter()
        .collect::<Vec<_>>();
    if entity_ids.len() == 2 {
        if let [point_id] = common_point_ids.as_slice() {
            if !ambiguous_points.contains(point_id) {
                if let Some([Some(u), Some(v)]) = points.get(point_id).copied() {
                    if u.is_finite() && v.is_finite() {
                        return Some([u, v]);
                    }
                }
            }
        }
    }
    let carriers = segments
        .iter()
        .map(|segment| trim_carrier(segment, &points, variables))
        .collect::<Option<Vec<_>>>()?;
    let mut intersections = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            let coordinate = match (carriers[first], carriers[second]) {
                (
                    TrimCarrier::Line { start, end },
                    TrimCarrier::Line {
                        start: second_start,
                        end: second_end,
                    },
                ) => trim_line_line_intersection(start, end, second_start, second_end),
                (TrimCarrier::Line { start, end }, TrimCarrier::Circle { center, radius })
                | (TrimCarrier::Circle { center, radius }, TrimCarrier::Line { start, end }) => {
                    trim_line_circle_intersection(start, end, center, radius)
                }
                (
                    TrimCarrier::Circle { center, radius },
                    TrimCarrier::Circle {
                        center: second_center,
                        radius: second_radius,
                    },
                ) => trim_circle_circle_intersection(center, radius, second_center, second_radius),
            }?;
            intersections.push(coordinate);
        }
    }
    let first = *intersections.first()?;
    let rest = &intersections[1..];
    let scale = first
        .into_iter()
        .chain(
            rest.iter()
                .flat_map(|coordinate| coordinate.iter().copied()),
        )
        .map(f64::abs)
        .fold(1.0, f64::max);
    rest.iter()
        .all(|coordinate| {
            (coordinate[0] - first[0]).hypot(coordinate[1] - first[1])
                <= TRIM_COORDINATE_EPS * scale
        })
        .then_some(first)
}

pub(crate) fn order_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureOrderTable> {
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
    let prototype = (|| {
        let mut field = cursor;
        for label in [b"ext_id\0".as_slice(), b"int_id\0", b"bitmask\0"] {
            let offset = find_bytes(payload, label, field, close)?;
            let (_, next) = segment_int(payload, offset + label.len());
            (next > offset + label.len() && next <= close).then_some(())?;
            field = next;
        }
        Some(())
    })();
    let (_, next) = psb::reference_id(payload, close + 2).ok()?;
    cursor = next;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    let mut external_ids = BTreeSet::new();
    let mut internal_ids = BTreeSet::new();
    let row_limit = usize::try_from(declared_count.saturating_sub(u32::from(prototype.is_some())))
        .unwrap_or(usize::MAX);
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
        let row_separator = payload.get(next) == Some(&0xe2);
        let table_boundary = next == end
            || payload
                .get(next)
                .is_some_and(|byte| matches!(byte, 0xe0 | 0xf1 | 0xf3));
        if (!row_separator && !table_boundary)
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
        if !row_separator {
            break;
        }
        cursor = next + 1;
    }
    Some(FeatureOrderTable {
        declared_count,
        has_prototype: prototype.is_some(),
        entity_ref,
        rows,
        offset: table,
    })
}

pub(crate) fn positional_order_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureOrderTable> {
    let (table, declared_count, cursor) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, after_count + 1).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then_some((table, declared_count, after_reference + 2))
    })?;
    let prototype = (|| {
        (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, mut prototype) = psb::reference_id(payload, cursor + 1).ok()?;
        for _ in 0..3 {
            let (_, next) = segment_int(payload, prototype);
            (next > prototype).then_some(())?;
            prototype = next;
        }
        (payload.get(prototype..prototype + 2) == Some(&[0xf1, psb::token::ENTITY_REF]))
            .then_some(())?;
        let (class, after_reference) = psb::reference_id(payload, prototype + 2).ok()?;
        (class == table_class && payload.get(after_reference) == Some(&0xe2)).then_some(())?;
        Some(after_reference + 1)
    })();
    let row_limit = usize::try_from(declared_count.saturating_sub(1)).unwrap_or(usize::MAX);
    // Each row consumes at least one byte before its 0xe2 separator, so the row
    // count cannot exceed the unread bytes in the table window.
    let capacity = bounded_len(
        u64::from(declared_count.saturating_sub(1)),
        1,
        end.saturating_sub(prototype.unwrap_or(end)),
    )
    .unwrap_or(0);
    let mut rows = Vec::with_capacity(capacity);
    let mut cursor = prototype.unwrap_or(end);
    let mut external_ids = BTreeSet::new();
    let mut internal_ids = BTreeSet::new();
    while cursor < end && rows.len() < row_limit {
        let row_offset = cursor;
        let (external_id, next) = segment_int(payload, cursor);
        let (internal_id, next) = segment_int(payload, next);
        let (bitmask, next) = segment_int(payload, next);
        let (Some(external_id), Some(internal_id), Some(bitmask)) =
            (external_id, internal_id, bitmask)
        else {
            break;
        };
        if !external_ids.insert(external_id) || !internal_ids.insert(internal_id) {
            break;
        }
        let row = FeatureOrderRow {
            external_id,
            internal_id,
            bitmask,
            offset: row_offset,
        };
        cursor = next;
        if rows.len() + 1 == row_limit {
            rows.push(row);
            break;
        }
        if payload.get(cursor) != Some(&0xe2) {
            break;
        }
        cursor += 1;
        rows.push(row);
    }
    Some(FeatureOrderTable {
        declared_count,
        has_prototype: prototype.is_some(),
        entity_ref: Some(table_class),
        rows,
        offset: table,
    })
}

fn named_compact_int(payload: &[u8], label: &[u8], start: usize, end: usize) -> Option<u32> {
    let at = find_bytes(payload, label, start, end)? + label.len();
    let (value, next) = segment_int(payload, at);
    value.filter(|_| next <= end)
}

fn gsec3d_plane_id(payload: &[u8], start: usize, end: usize) -> Option<u32> {
    let label = b"plane_id\0";
    let reference_planes = find_bytes(payload, b"\xe0\x00ref_planes\0", start, end).unwrap_or(end);
    let mut cursor = start;
    while let Some(at) = find_bytes(payload, label, cursor, reference_planes) {
        cursor = at + label.len();
        let (value, next) = segment_int(payload, cursor);
        if next <= reference_planes && value.is_some() {
            return value;
        }
    }
    cursor = reference_planes;
    while let Some(at) = find_bytes(payload, label, cursor, end) {
        cursor = at + label.len();
        if payload.get(at.saturating_sub(2)..at) == Some(&[psb::token::NAMED_RECORD, 1]) {
            continue;
        }
        let (value, next) = segment_int(payload, cursor);
        if next <= end && value.is_some() {
            return value;
        }
    }
    None
}

fn section_3d(payload: &[u8], start: usize, end: usize) -> Option<FeatureSection3d> {
    const GSEC3D: &[u8] = b"\xe0\x00gsec3d_ptr\0";
    const SAVED_RESULT: &[u8] = b"\xe0\x00p_saved_result\0";
    let section = find_bytes(payload, GSEC3D, start, end)?;
    let record_end = find_bytes(payload, GSEC3D, section + GSEC3D.len(), end).unwrap_or(end);
    let placement_end =
        find_bytes(payload, SAVED_RESULT, section, record_end).unwrap_or(record_end);
    let sketch_plane_entity_id = gsec3d_plane_id(payload, section, placement_end);
    let sketch_plane_flip = find_bytes(payload, b"plane_flip\0", section, placement_end)
        .and_then(|at| payload.get(at + b"plane_flip\0".len()).copied())
        .and_then(BinaryFlag::decode);

    let mut reference_plane_entity_ids = Vec::new();
    let reference_plane_rows = Vec::new();
    let mut reference_plane_datum_geometry_id = None;
    if let Some(references) = find_bytes(payload, b"\xe0\x00ref_planes\0", section, placement_end) {
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
            let nested_end = placement_end;
            reference_plane_datum_geometry_id =
                named_compact_int(payload, b"\xe0\x01plane_id\0", cursor, nested_end);
        }
    }

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
        reference_plane_rows,
        reference_plane_datum_geometry_id,
        orientation,
        dimension_ids,
        offset: section,
    })
}

pub(crate) fn positional_section_3d(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureSection3d> {
    let (section, name_end) = payload[start..end]
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"\x07S2D")
        .find_map(|(relative, _)| {
            let section = start + relative;
            let name_end = payload[section + 1..end]
                .iter()
                .position(|&byte| byte == 0)?
                + section
                + 1;
            Some((section, name_end))
        })?;
    let mut result = FeatureSection3d {
        sketch_plane_entity_id: None,
        sketch_plane_flip: None,
        reference_plane_entity_ids: Vec::new(),
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: section,
    };
    let mut cursor = name_end + 1;
    let Some(section_flip) = payload.get(cursor).copied() else {
        return Some(result);
    };
    result.orientation.section_flip = BinaryFlag::decode(section_flip);
    cursor += 1;
    for _ in 0..3 {
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            return Some(result);
        }
        cursor = next;
    }
    let (sketch_plane_entity_id, next) = segment_int(payload, cursor);
    if next <= cursor {
        return Some(result);
    }
    result.sketch_plane_entity_id = sketch_plane_entity_id;
    cursor = next;
    let Some(sketch_plane_flip) = payload.get(cursor).copied() else {
        return Some(result);
    };
    result.sketch_plane_flip = BinaryFlag::decode(sketch_plane_flip);
    cursor += 1;
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Some(result);
    }
    let (reference_count, next) = psb::compact_int(payload, cursor + 1);
    if next <= cursor + 1 {
        return Some(result);
    }
    cursor = next;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Some(result);
    }
    let table_reference_start = cursor + 1;
    let Ok((_, next)) = psb::reference_id(payload, table_reference_start) else {
        return Some(result);
    };
    let table_reference = payload[table_reference_start..next].to_vec();
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[0xfb, 0xe2]) {
        return Some(result);
    }
    cursor += 2;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Some(result);
    }
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Some(result);
    };
    cursor = next;

    let row_count = usize::try_from(reference_count).unwrap_or(usize::MAX);
    let mut reference_plane_rows = Vec::new();
    let mut separator = vec![0xf2, psb::token::ENTITY_REF];
    separator.extend_from_slice(&table_reference);
    separator.push(0xe2);
    for row in 0..row_count {
        let (Some(plane_id), next) = segment_int(payload, cursor) else {
            break;
        };
        cursor = next;
        let (reference_type, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let (external_reference_id, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let (segment_id, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let (sub_index, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        let reference_flip = payload.get(cursor).copied().and_then(BinaryFlag::decode);
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            break;
        }
        cursor = next;
        reference_plane_rows.push(FeatureSectionReferencePlane {
            plane_entity_id: plane_id,
            reference_type,
            external_reference_id,
            segment_id,
            sub_index,
            reference_flip,
        });
        result.reference_plane_entity_ids.push(plane_id);
        if row + 1 < row_count {
            let Some(separator_at) = find_bytes(payload, &separator, cursor, end) else {
                break;
            };
            cursor = separator_at + separator.len();
        }
    }
    result.reference_plane_rows = reference_plane_rows;
    Some(result)
}

pub(crate) fn dimension_unit(dimension_type: u32) -> DimensionUnit {
    match dimension_type {
        0x0a => DimensionUnit::Radians,
        0x01..=0x05 => DimensionUnit::Millimeters,
        _ => DimensionUnit::SchemaDefined,
    }
}

fn unresolved_dimension_value_token(bytes: &[u8]) -> Option<Vec<u8>> {
    match bytes {
        [0x00, _, _] | [0x01, _, _, _] => Some(bytes.to_vec()),
        _ => None,
    }
}

fn named_dimension_reference(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<(FeatureDimensionReference, usize)> {
    let item_label = find_bytes(payload, b"item_id\0", start, end)?;
    let mut cursor = item_label + b"item_id\0".len();
    let item_id = next_nullable_segment_int(payload, &mut cursor).ok()?;
    let sense_label = find_bytes(payload, b"sense\0", cursor, end)?;
    cursor = sense_label + b"sense\0".len();
    let sense = next_nullable_segment_int(payload, &mut cursor).ok()?;
    let point_label = find_bytes(payload, b"point\0", cursor, end)?;
    cursor = point_label + b"point\0".len();
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    (declared_count == 2).then_some(())?;
    cursor = after_count;
    let point = segment_slots(payload, &mut cursor, 2)?;
    let [first, second] = point.try_into().ok()?;
    Some((
        FeatureDimensionReference {
            item_id,
            sense,
            point: [first, second],
            offset: item_label,
        },
        cursor,
    ))
}

fn dimension_reference_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureDimensionReferenceTable> {
    let table = find_bytes(payload, b"dim_ref\0", start, end)?;
    let mut cursor = table + b"dim_ref\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        cursor += 1;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
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
    if payload.get(cursor..cursor + 2) == Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        cursor += 2;
    } else {
        return Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows: Vec::new(),
            offset: table,
        });
    }
    if declared_count == 0 {
        return Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows: Vec::new(),
            offset: table,
        });
    }

    let mut rows = Vec::new();
    let Some((prototype, prototype_end)) = named_dimension_reference(payload, cursor, end) else {
        return Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows,
            offset: table,
        });
    };
    rows.push(prototype);
    let Some(reference_bytes) = reference_bytes else {
        return Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows,
            offset: table,
        });
    };
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    if payload.get(prototype_end..prototype_end + prototype_separator.len())
        != Some(prototype_separator.as_slice())
    {
        return Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows,
            offset: table,
        });
    }
    cursor = prototype_end + prototype_separator.len();

    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    while rows.len() < row_limit && cursor < end {
        let row_offset = cursor;
        let Ok(item_id) = next_nullable_segment_int(payload, &mut cursor) else {
            break;
        };
        let Ok(sense) = next_nullable_segment_int(payload, &mut cursor) else {
            break;
        };
        let Some(point) = segment_slots(payload, &mut cursor, 2) else {
            break;
        };
        if point.len() != 2 {
            break;
        }
        let [first, second] = [point[0], point[1]];
        rows.push(FeatureDimensionReference {
            item_id,
            sense,
            point: [first, second],
            offset: row_offset,
        });
        if rows.len() == row_limit {
            break;
        }
        let mut row_separator = vec![0xf3, psb::token::ENTITY_REF];
        row_separator.extend_from_slice(&reference_bytes);
        row_separator.push(0xe2);
        if payload.get(cursor..cursor + row_separator.len()) != Some(row_separator.as_slice()) {
            break;
        }
        cursor += row_separator.len();
    }
    Some(FeatureDimensionReferenceTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
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
    let value_start = value_label + b"value\0".len();
    let (value, after_value, _) = decode_variable_scalar(payload, value_start, end, cache);
    let value_body = payload.get(value_start..after_value)?.to_vec();
    let unresolved_value_token = value
        .is_none()
        .then_some(value_body.as_slice())
        .and_then(unresolved_dimension_value_token);
    let direction_label = find_bytes(payload, b"direct\0", after_value, end)?;
    let direction_byte = *payload.get(direction_label + b"direct\0".len())?;
    let auxiliary_label = find_bytes(payload, b"aux_value\0", direction_label, end)?;
    let auxiliary_start = auxiliary_label + b"aux_value\0".len();
    let (auxiliary_value, after_auxiliary, _) =
        decode_variable_scalar(payload, auxiliary_start, end, cache);
    let auxiliary_body = payload.get(auxiliary_start..after_auxiliary)?.to_vec();
    let external_label = find_bytes(payload, b"ext_id\0", after_auxiliary, end)?;
    let (external_id, after_external) = segment_int(payload, external_label + b"ext_id\0".len());
    let references = dimension_reference_table(payload, after_external, end);
    Some(FeatureDimension {
        dimension_type,
        value,
        value_body,
        unresolved_value_token,
        value_unit: dimension_unit(dimension_type),
        direction_byte,
        auxiliary_value,
        auxiliary_body,
        external_id: external_id?,
        references,
        offset: type_label,
    })
}

pub(crate) fn positional_dimension(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimension> {
    let (dimension_type, cursor) = segment_int(payload, start);
    let dimension_type = dimension_type?;
    let value_start = cursor;
    let (value, cursor, _) = match payload.get(cursor) {
        Some(0x00) if cursor + 3 <= end => (None, cursor + 3, true),
        Some(0x01) if cursor + 4 <= end => (None, cursor + 4, true),
        Some(0x0e) => (Some(-0.5), cursor + 1, false),
        Some(0x18) => (Some(0.0), cursor + 1, false),
        _ => decode_variable_scalar(payload, cursor, end, cache),
    };
    let value_body = payload.get(value_start..cursor)?.to_vec();
    let unresolved_value_token = value
        .is_none()
        .then_some(value_body.as_slice())
        .and_then(unresolved_dimension_value_token);
    let direction_byte = *payload.get(cursor).filter(|_| cursor < end)?;
    let auxiliary_start = cursor + 1;
    let (auxiliary_value, cursor) = if payload.get(auxiliary_start) == Some(&0x18) {
        (Some(0.0), auxiliary_start + 1)
    } else {
        let (value, next, _) = decode_variable_scalar(payload, auxiliary_start, end, cache);
        (value, next)
    };
    let auxiliary_body = payload.get(auxiliary_start..cursor)?.to_vec();
    let (external_id, _) = segment_int(payload, cursor);
    Some(FeatureDimension {
        dimension_type,
        value,
        value_body,
        unresolved_value_token,
        value_unit: dimension_unit(dimension_type),
        direction_byte,
        auxiliary_value,
        auxiliary_body,
        external_id: external_id?,
        references: None,
        offset: start,
    })
}

pub(crate) fn dimension_table(
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
    Some(FeatureDimensionTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    })
}

pub(crate) fn positional_dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimensionTable> {
    let (table, declared_count, mut cursor, reference_bytes) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let reference_start = after_count + 1;
        let (class, after_reference) = psb::reference_id(payload, reference_start).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then(|| {
            (
                table,
                declared_count,
                after_reference + 2,
                payload[reference_start..after_reference].to_vec(),
            )
        })
    })?;
    (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_row_class) = psb::reference_id(payload, cursor + 1).ok()?;
    cursor = after_row_class;

    let mut separator = vec![0xf3, psb::token::ENTITY_REF];
    separator.extend_from_slice(&reference_bytes);
    separator.push(0xe2);
    let mut rows = Vec::new();
    let row_limit = usize::try_from(declared_count).unwrap_or(usize::MAX);
    while cursor < end && rows.len() < row_limit {
        let row_end = find_bytes(payload, &separator, cursor, end).unwrap_or(end);
        let Some(row) = positional_dimension(payload, cursor, row_end, cache) else {
            break;
        };
        rows.push(row);
        if rows.len() == row_limit {
            break;
        }
        if payload.get(row_end..row_end + separator.len()) != Some(separator.as_slice()) {
            break;
        }
        cursor = row_end + separator.len();
    }
    Some(FeatureDimensionTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        offset: table,
    })
}

pub(crate) fn self_described_positional_dimension_table(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<FeatureDimensionTable> {
    let mut candidates = Vec::new();
    for table in start..end {
        if payload.get(table) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        if payload.get(after_count) != Some(&psb::token::ENTITY_REF) {
            continue;
        }
        let Ok((table_class, after_reference)) = psb::reference_id(payload, after_count + 1) else {
            continue;
        };
        if payload.get(after_reference..after_reference + 2) != Some(&[0xfb, 0xe2]) {
            continue;
        }
        let Some(candidate) = positional_dimension_table(payload, table, end, table_class, cache)
        else {
            continue;
        };
        if candidate.offset == table
            && declared_count > 1
            && candidate.declared_count == declared_count
            && usize::try_from(declared_count).ok() == Some(candidate.rows.len())
            && candidate
                .rows
                .iter()
                .all(|row| matches!(row.dimension_type, 0x01..=0x05 | 0x0a))
        {
            candidates.push(candidate);
        }
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(crate) fn feature_skamps(payload: &[u8], start: usize, end: usize) -> Vec<FeatureSkamp> {
    let Some(table) = find_bytes(payload, b"skamp_ptr\0", start, end) else {
        return Vec::new();
    };
    let mut cursor = table + b"skamp_ptr\0".len();
    if payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1 | 0xf3))
    {
        cursor += 1;
    } else if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x05]) {
        cursor += 2;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Vec::new();
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let class_start = cursor;
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    let class_encoding = &payload[class_start..next];
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Vec::new();
    }
    cursor += 2;
    let mut trailer = Vec::with_capacity(class_encoding.len() + 2);
    trailer.push(0xf3);
    trailer.extend_from_slice(class_encoding);
    trailer.push(0xe2);
    let Some(prototype_end) = find_bytes(payload, &trailer, cursor, end) else {
        return Vec::new();
    };
    let named_item = (|| {
        Some(FeatureSkampItem {
            entity_id: named_compact_int(payload, b"ent_id\0", cursor, prototype_end)?,
            sense: named_compact_int(payload, b"sense\0", cursor, prototype_end)?,
        })
    })();
    let Some(items_label) = find_bytes(payload, b"items\0", cursor, prototype_end) else {
        return Vec::new();
    };
    let mut item_cursor = items_label + b"items\0".len();
    if payload.get(item_cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Vec::new();
    }
    let (prototype_item_count, after_count) = psb::compact_int(payload, item_cursor + 1);
    item_cursor = after_count;
    let item_class_start = item_cursor;
    let Ok((_, after_item_class)) = psb::reference_id(payload, item_cursor + 1) else {
        return Vec::new();
    };
    let item_class_encoding = &payload[item_class_start..after_item_class];
    let mut item_close = Vec::with_capacity(item_class_encoding.len() + 2);
    item_close.push(0xf1);
    item_close.extend_from_slice(item_class_encoding);
    item_close.push(0xe2);
    let Some(named_item_end) = find_bytes(payload, &item_close, after_item_class, prototype_end)
    else {
        return Vec::new();
    };
    item_cursor = named_item_end + item_close.len();
    let mut prototype_items = named_item.into_iter().collect::<Vec<_>>();
    while prototype_items.len() < usize::try_from(prototype_item_count).unwrap_or(usize::MAX) {
        let (Some(entity_id), next) = segment_int(payload, item_cursor) else {
            return Vec::new();
        };
        item_cursor = next;
        let (Some(sense), next) = segment_int(payload, item_cursor) else {
            return Vec::new();
        };
        item_cursor = next;
        prototype_items.push(FeatureSkampItem { entity_id, sense });
    }
    if item_cursor != prototype_end {
        return Vec::new();
    }
    let Some(prototype) = (|| {
        Some(FeatureSkamp {
            id: named_compact_int(payload, b"id\0", cursor, prototype_end)?,
            kind: named_compact_int(payload, b"type\0", cursor, prototype_end)?,
            flags: named_compact_int(payload, b"flags\0", cursor, prototype_end)?,
            status: named_compact_int(payload, b"status\0", cursor, prototype_end)?,
            items: prototype_items,
            offset: cursor,
        })
    })() else {
        return Vec::new();
    };
    let mut rows = vec![prototype];
    cursor = prototype_end + trailer.len();
    'rows: while rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let Some(id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(flags) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(status) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
            break;
        }
        let (item_count, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
            break;
        };
        cursor = next;
        if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
            break;
        }
        cursor += 2;
        let mut items = Vec::new();
        while items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
            if !items.is_empty() && payload.get(cursor) == Some(&0xe2) {
                cursor += 1;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    break 'rows;
                };
                cursor = next;
            }
            let Some(entity_id) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            let Some(sense) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            items.push(FeatureSkampItem { entity_id, sense });
            if payload.get(cursor) == Some(&0xf1) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 2) else {
                    break 'rows;
                };
                cursor = next;
                if payload.get(cursor) != Some(&0xe2) {
                    break 'rows;
                }
                cursor += 1;
            }
        }
        if payload.get(cursor..cursor + trailer.len()) == Some(trailer.as_slice()) {
            cursor += trailer.len();
        } else if payload.get(cursor) == Some(&0xe2) {
            cursor += 1;
        } else if payload.get(cursor) == Some(&0xe0) {
            // The final row is terminated by the following named table.
        } else {
            break;
        }
        rows.push(FeatureSkamp {
            id,
            kind,
            flags,
            status,
            items,
            offset: row_offset,
        });
    }
    rows
}

fn named_array_class(payload: &[u8], label: &[u8], start: usize, end: usize) -> Option<u32> {
    let label = find_bytes(payload, label, start, end)? + label.len();
    let array =
        (label..end).find(|offset| payload.get(*offset) == Some(&psb::token::ARRAY_OPEN))?;
    let (_, after_count) = psb::compact_int(payload, array + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    psb::reference_id(payload, after_count + 1)
        .ok()
        .map(|(class, _)| class)
}

pub(crate) fn named_solver_table_header(
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureSolverTableHeader> {
    let offset = find_bytes(payload, label, start, end)?;
    let mut cursor = offset + label.len();
    if payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1 | 0xf3))
    {
        cursor += 1;
    } else if payload
        .get(cursor..cursor + 2)
        .is_some_and(|wrapper| matches!(wrapper, [0xf4, 0x04 | 0x05]))
    {
        cursor += 2;
    }
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (entity_ref, _) = psb::reference_id(payload, after_count + 1).ok()?;
    Some(FeatureSolverTableHeader {
        declared_count,
        entity_ref,
        offset,
    })
}

fn positional_solver_table_header(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureSolverTableHeader> {
    let (offset, declared_count, _, _) = positional_array_header(payload, start, end, table_class)?;
    Some(FeatureSolverTableHeader {
        declared_count,
        entity_ref: table_class,
        offset,
    })
}

fn positional_array_header(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<(usize, u32, usize, Vec<u8>)> {
    let candidates = (start..end)
        .filter_map(|offset| {
            (payload.get(offset) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
            let (count, after_count) = psb::compact_int(payload, offset + 1);
            (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let reference_start = after_count + 1;
            let (class, after_class) = psb::reference_id(payload, reference_start).ok()?;
            (class == table_class
                && payload.get(after_class..after_class + 2) == Some(&[0xfb, 0xe2]))
            .then(|| {
                (
                    offset,
                    count,
                    after_class + 2,
                    payload[after_count..after_class].to_vec(),
                )
            })
        })
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

fn consume_positional_separator(
    payload: &[u8],
    cursor: usize,
    end: usize,
    class_encoding: &[u8],
    class_prefixes: &[u8],
) -> Option<usize> {
    if payload.get(cursor) == Some(&0xe2) {
        return Some(cursor + 1);
    }
    let length = class_encoding.len() + 2;
    (cursor + length <= end
        && payload
            .get(cursor)
            .is_some_and(|prefix| class_prefixes.contains(prefix))
        && payload.get(cursor + 1..cursor + 1 + class_encoding.len()) == Some(class_encoding)
        && payload.get(cursor + length - 1) == Some(&0xe2))
    .then_some(cursor + length)
}

pub(crate) fn positional_feature_skamps(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Vec<FeatureSkamp> {
    let Some((_, count, mut cursor, table_class_encoding)) =
        positional_array_header(payload, start, end, table_class)
    else {
        return Vec::new();
    };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Vec::new();
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    cursor = after_row_class;
    let mut rows = Vec::new();
    let mut item_classes = None::<(Vec<u8>, Vec<u8>)>;
    'rows: while rows.len() < usize::try_from(count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let Some(id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(kind) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(flags) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(status) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some((item_count, after_item_row_class, item_table_class, item_row_class)) =
            positional_skamp_item_array(
                payload,
                cursor,
                end,
                &table_class_encoding,
                item_classes.as_ref().map(|classes| classes.0.as_slice()),
                item_classes.as_ref().map(|classes| classes.1.as_slice()),
            )
        else {
            break;
        };
        item_classes.get_or_insert((item_table_class, item_row_class));
        cursor = after_item_row_class;
        let mut items = Vec::new();
        while items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
            let Some(entity_id) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            let Some(sense) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            items.push(FeatureSkampItem { entity_id, sense });
            if items.len() < usize::try_from(item_count).unwrap_or(usize::MAX) {
                let Some(next) = consume_positional_separator(
                    payload,
                    cursor,
                    end,
                    item_classes
                        .as_ref()
                        .expect("item classes established")
                        .0
                        .as_slice(),
                    &[0xf1],
                ) else {
                    break 'rows;
                };
                cursor = next;
            }
        }
        let row = FeatureSkamp {
            id,
            kind,
            flags,
            status,
            items,
            offset: row_offset,
        };
        if rows.len() + 1 < usize::try_from(count).unwrap_or(usize::MAX) {
            let Some(next) =
                consume_positional_separator(payload, cursor, end, &table_class_encoding, &[0xf3])
            else {
                break;
            };
            cursor = next;
        }
        rows.push(row);
    }
    rows
}

fn positional_skamp_item_array(
    payload: &[u8],
    start: usize,
    end: usize,
    outer_table_class: &[u8],
    expected_table_class: Option<&[u8]>,
    expected_row_class: Option<&[u8]>,
) -> Option<(u32, usize, Vec<u8>, Vec<u8>)> {
    let mut row_separator = Vec::with_capacity(outer_table_class.len() + 2);
    row_separator.push(0xf3);
    row_separator.extend_from_slice(outer_table_class);
    row_separator.push(0xe2);
    let row_end = find_bytes(payload, &row_separator, start, end).unwrap_or(end);
    let candidate = (start..row_end).find_map(|array| {
        (payload.get(array) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (count, after_count) = psb::compact_int(payload, array + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, after_table_class) = psb::reference_id(payload, after_count + 1).ok()?;
        let table_class = payload.get(after_count..after_table_class)?;
        expected_table_class
            .is_none_or(|expected| expected == table_class)
            .then_some(())?;
        (payload.get(after_table_class..after_table_class + 2) == Some(&[0xfb, 0xe2]))
            .then_some(())?;
        let row_class_start = after_table_class + 2;
        (payload.get(row_class_start) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, after_row_class) = psb::reference_id(payload, row_class_start + 1).ok()?;
        let row_class = payload.get(row_class_start..after_row_class)?;
        expected_row_class
            .is_none_or(|expected| expected == row_class)
            .then_some(())?;
        Some((
            count,
            after_row_class,
            table_class.to_vec(),
            row_class.to_vec(),
        ))
    })?;
    positional_skamp_item_array_has_valid_boundary(
        payload,
        candidate.1,
        candidate.0,
        &candidate.2,
        outer_table_class,
        end,
    )?;
    Some(candidate)
}

fn positional_skamp_item_array_has_valid_boundary(
    payload: &[u8],
    mut cursor: usize,
    item_count: u32,
    item_table_class: &[u8],
    outer_table_class: &[u8],
    end: usize,
) -> Option<()> {
    let item_limit = usize::try_from(item_count).unwrap_or(usize::MAX);
    let mut items = 0;
    while items < item_limit {
        next_solver_int(payload, &mut cursor)?;
        next_solver_int(payload, &mut cursor)?;
        items += 1;
        if items < item_limit {
            cursor = consume_positional_separator(payload, cursor, end, item_table_class, &[0xf1])?;
        }
    }

    let mut row_separator = Vec::with_capacity(outer_table_class.len() + 2);
    row_separator.push(0xf3);
    row_separator.extend_from_slice(outer_table_class);
    row_separator.push(0xe2);
    if cursor == end {
        return Some(());
    }
    if payload.get(cursor..cursor + row_separator.len()) == Some(row_separator.as_slice()) {
        return Some(());
    }
    if payload.get(cursor) == Some(&0xe2) {
        return Some(());
    }
    if positional_skamp_following_table_header(payload, cursor, end, item_table_class).is_some() {
        return Some(());
    }
    payload
        .get(cursor)
        .is_some_and(|byte| *byte == 0xe0)
        .then_some(())
        .or_else(|| {
            let Some([0xf4, 0x04 | 0x05]) = payload.get(cursor..cursor + 2) else {
                return None;
            };
            (payload.get(cursor + 2) == Some(&psb::token::ENTITY_REF)).then_some(())?;
            let (_, after_table_class) = psb::reference_id(payload, cursor + 3).ok()?;
            (after_table_class <= end).then_some(())?;
            if payload.get(after_table_class) == Some(&psb::token::ARRAY_OPEN) {
                let (_, after_count) = psb::compact_int(payload, after_table_class + 1);
                (after_count < end && payload.get(after_count) == Some(&psb::token::ENTITY_REF))
                    .then_some(())?;
                let (_, after_next_table_class) =
                    psb::reference_id(payload, after_count + 1).ok()?;
                (after_next_table_class + 2 <= end
                    && payload.get(after_next_table_class..after_next_table_class + 2)
                        == Some(&[0xfb, 0xe2]))
                .then_some(())
            } else {
                payload
                    .get(after_table_class)
                    .is_some_and(|byte| matches!(byte, 0xe0..=0xe3))
                    .then_some(())
            }
        })
}

fn positional_skamp_following_table_header(
    payload: &[u8],
    cursor: usize,
    end: usize,
    item_table_class: &[u8],
) -> Option<()> {
    (payload.get(cursor) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (_, after_count) = psb::compact_int(payload, cursor + 1);
    (after_count < end && payload.get(after_count) == Some(&psb::token::ENTITY_REF))
        .then_some(())?;
    let reference_start = after_count + 1;
    let (_, after_table_class) = psb::reference_id(payload, reference_start).ok()?;
    let table_class = payload.get(after_count..after_table_class)?;
    (table_class != item_table_class
        && after_table_class + 2 <= end
        && payload.get(after_table_class..after_table_class + 2) == Some(&[0xfb, 0xe2]))
    .then_some(())?;
    let (_, after_row_class) = psb::reference_id(payload, after_table_class + 3).ok()?;
    (after_row_class <= end).then_some(())
}

pub(crate) fn feature_relation_triples(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Vec<FeatureRelationTriple> {
    let Some(table) = find_bytes(payload, b"triples_ptr\0", start, end) else {
        return Vec::new();
    };
    let mut cursor = table + b"triples_ptr\0".len();
    if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x04]) {
        cursor += 2;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Vec::new();
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Vec::new();
    }
    cursor += 2;
    let Some(close) = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, end) else {
        return Vec::new();
    };
    let prototype = FeatureRelationTriple {
        relation_id: named_compact_int(payload, b"rel_id\0", cursor, close),
        equation_id: named_compact_int(payload, b"eqn_id\0", cursor, close),
        skamp_id: named_compact_int(payload, b"skamp_id\0", cursor, close),
        offset: cursor,
    };
    let Ok((_, next)) = psb::reference_id(payload, close + 2) else {
        return Vec::new();
    };
    cursor = next;
    if payload.get(cursor) != Some(&0xe2) {
        return Vec::new();
    }
    cursor += 1;
    let mut rows = vec![prototype];
    while rows.len() < usize::try_from(declared_count).unwrap_or(usize::MAX) {
        let row_offset = cursor;
        let relation_id = next_solver_int(payload, &mut cursor);
        let equation_id = next_solver_int(payload, &mut cursor);
        let skamp_id = next_solver_int(payload, &mut cursor);
        let terminal_named_boundary = rows.len() + 1
            == usize::try_from(declared_count).unwrap_or(usize::MAX)
            && payload.get(cursor).is_some_and(|byte| *byte >= 0xe0);
        if payload.get(cursor) != Some(&0xe2) && !terminal_named_boundary {
            break;
        }
        if !terminal_named_boundary {
            cursor += 1;
        }
        rows.push(FeatureRelationTriple {
            relation_id,
            equation_id,
            skamp_id,
            offset: row_offset,
        });
    }
    rows
}

pub(crate) fn positional_relation_triples(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Vec<FeatureRelationTriple> {
    let Some((_, count, mut cursor, class_encoding)) =
        positional_array_header(payload, start, end, table_class)
    else {
        return Vec::new();
    };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Vec::new();
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Vec::new();
    };
    cursor = after_row_class;
    let mut rows = Vec::new();
    while rows.len() < usize::try_from(count).unwrap_or(usize::MAX) {
        let offset = cursor;
        let before_relation = cursor;
        let relation_id = next_solver_int(payload, &mut cursor);
        if cursor <= before_relation {
            break;
        }
        let before_equation = cursor;
        let equation_id = next_solver_int(payload, &mut cursor);
        if cursor <= before_equation {
            break;
        }
        let before_skamp = cursor;
        let skamp_id = next_solver_int(payload, &mut cursor);
        if cursor <= before_skamp {
            break;
        }
        let row = FeatureRelationTriple {
            relation_id,
            equation_id,
            skamp_id,
            offset,
        };
        if rows.len() + 1 < usize::try_from(count).unwrap_or(usize::MAX) {
            let Some(next) =
                consume_positional_separator(payload, cursor, end, &class_encoding, &[0xf1])
            else {
                break;
            };
            cursor = next;
        }
        rows.push(row);
    }
    rows
}

fn relation_operand_vectors(bytes: &[u8]) -> Option<[[Option<u32>; 4]; 3]> {
    let mut values = Vec::with_capacity(12);
    let mut cursor = 0;
    while cursor < bytes.len() && values.len() < 12 {
        match bytes[cursor] {
            0xe4 => {
                values.push(Some(1));
                cursor += 1;
            }
            0xe5 => {
                values.extend([Some(0); 2]);
                cursor += 1;
            }
            0xe6 => {
                values.extend([Some(0); 3]);
                cursor += 1;
            }
            0xf6 => {
                values.push(None);
                cursor += 1;
            }
            _ => {
                let value = next_solver_int(bytes, &mut cursor)?;
                values.push(Some(value));
            }
        }
    }
    if cursor != bytes.len() || values.len() != 12 {
        return None;
    }
    let mut chunks = values.chunks_exact(4);
    let result = [
        chunks.next()?.try_into().ok()?,
        chunks.next()?.try_into().ok()?,
        chunks.next()?.try_into().ok()?,
    ];
    chunks.next().is_none().then_some(result)
}

pub(crate) fn relation_table(
    payload: &[u8],
    start: usize,
    end: usize,
) -> Option<FeatureRelationTable> {
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
    let rows_end = [b"skamp_ptr\0".as_slice(), b"triples_ptr\0"]
        .into_iter()
        .filter_map(|label| find_bytes(payload, label, cursor, end))
        .min()
        .unwrap_or(end);
    let rows_start = (|| {
        let close = find_bytes(payload, &[0xf1, psb::token::ENTITY_REF], cursor, rows_end)?;
        let (_, after_ref) = psb::reference_id(payload, close + 2).ok()?;
        (payload.get(after_ref) == Some(&0xe2)).then_some(after_ref + 1)
    })();
    let rows = rows_start.map_or_else(Vec::new, |rows_start| {
        positional_relation_rows(
            payload,
            rows_start,
            rows_end,
            declared_count.saturating_sub(2),
        )
    });
    Some(FeatureRelationTable {
        declared_count,
        entity_ref,
        rows,
        skamps: feature_skamps(payload, start, end),
        skamp_header: named_solver_table_header(payload, b"skamp_ptr\0", start, end),
        triples: feature_relation_triples(payload, start, end),
        triples_header: named_solver_table_header(payload, b"triples_ptr\0", start, end),
        offset: table,
    })
}

fn positional_relation_rows(
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    row_count: u32,
) -> Vec<FeatureRelation> {
    if cursor > end || end > payload.len() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for _ in 0..row_count {
        let Some(row_end) = payload[cursor..end]
            .iter()
            .position(|byte| *byte == 0xe2)
            .map(|relative| relative + cursor)
        else {
            break;
        };
        let (relation_id, after_id) = psb::compact_int(payload, cursor);
        if after_id <= cursor || after_id >= row_end {
            break;
        }
        let (used, after_used) = psb::compact_int(payload, after_id);
        if after_used <= after_id || after_used >= row_end {
            break;
        }
        let mut suffixes = Vec::new();
        for suffix_start in after_used..row_end {
            let (sign, after_sign) = psb::compact_int(payload, suffix_start);
            let (dimension_id, after_dimension) = psb::compact_int(payload, after_sign);
            let (relation_type, after_type) = psb::compact_int(payload, after_dimension);
            if after_sign > suffix_start
                && after_dimension > after_sign
                && after_type > after_dimension
                && after_type == row_end
            {
                suffixes.push((suffix_start, sign, dimension_id, relation_type));
            }
        }
        let [(suffix_start, sign, dimension_id, relation_type)] = suffixes.as_slice() else {
            break;
        };
        let operands = payload[after_used..*suffix_start].to_vec();
        rows.push(FeatureRelation {
            relation_id,
            used,
            operand_vectors: relation_operand_vectors(&operands),
            operands,
            sign: *sign,
            dimension_id: *dimension_id,
            relation_type: *relation_type,
            body: payload[cursor..row_end].to_vec(),
            offset: cursor,
        });
        cursor = row_end + 1;
    }
    rows
}

pub(crate) fn positional_relation_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureRelationTable> {
    let (table, declared_count, cursor, reference_bytes) = (start..end).find_map(|table| {
        (payload.get(table) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
        let (declared_count, after_count) = psb::compact_int(payload, table + 1);
        (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let reference_start = after_count + 1;
        let (class, after_reference) = psb::reference_id(payload, reference_start).ok()?;
        (class == table_class
            && payload.get(after_reference..after_reference + 2) == Some(&[0xfb, 0xe2]))
        .then(|| {
            (
                table,
                declared_count,
                after_reference + 2,
                payload[reference_start..after_reference].to_vec(),
            )
        })
    })?;
    let mut prototype_separator = vec![0xf1, psb::token::ENTITY_REF];
    prototype_separator.extend_from_slice(&reference_bytes);
    prototype_separator.push(0xe2);
    let rows_start = (|| {
        (payload.get(cursor) == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, prototype) = psb::reference_id(payload, cursor + 1).ok()?;
        let prototype_end = find_bytes(payload, &prototype_separator, prototype, end)?;
        Some(prototype_end + prototype_separator.len())
    })();
    let rows = rows_start.map_or_else(Vec::new, |rows_start| {
        positional_relation_rows(payload, rows_start, end, declared_count.saturating_sub(2))
    });
    Some(FeatureRelationTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        skamps: Vec::new(),
        skamp_header: None,
        triples: Vec::new(),
        triples_header: None,
        offset: table,
    })
}

pub(crate) fn saved_section_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize) {
    let Some(&prefix) = payload.get(offset).filter(|_| offset < end) else {
        return (None, offset);
    };
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x18 | 0x81 | 0xe0 | 0xe3 | 0xf0 | 0xf1))
    {
        return (Some(0.0), offset + 1);
    }
    if matches!(prefix, 0x90 | 0xd7) && offset + 7 <= end {
        return (None, offset + 7);
    }
    if prefix == 0x41 && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    if prefix == 0x2d && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    if matches!(prefix, 0x74 | 0x75) && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1] = prefix.wrapping_sub(0x8b);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0x99 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0xc0, 0x0e]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0xdd && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0x40, 0x0c]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    let supplied_head = match prefix {
        0xb3 => Some([0xbf, 0xe0]),
        0xcb => Some([0xbf, 0xf8]),
        0xd6 => Some([0xc0, 0x04]),
        _ => None,
    };
    if let Some(head) = supplied_head.filter(|_| offset + 7 <= end) {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&head);
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

fn saved_line_block(
    payload: &[u8],
    mut cursor: usize,
    segment_end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    if payload.get(cursor) == Some(&0xf1) {
        cursor = payload[cursor..segment_end]
            .iter()
            .position(|byte| *byte == 0xe3)
            .map_or(segment_end, |relative| cursor + relative + 1);
    }
    let mut entities = Vec::new();
    while cursor < segment_end {
        if payload.get(cursor) == Some(&0xe3) {
            cursor += 1;
        }
        let point_label = b"\xe0\x00entity(point)\0";
        if payload.get(cursor..cursor + point_label.len()) == Some(point_label) {
            let Some(close) = find_bytes(
                payload,
                &[0xf1, psb::token::ENTITY_REF],
                cursor + point_label.len(),
                segment_end,
            ) else {
                break;
            };
            let Ok((_, after_reference)) = psb::reference_id(payload, close + 2) else {
                break;
            };
            if payload.get(after_reference) != Some(&0xe3) {
                break;
            }
            cursor = after_reference + 1;
            continue;
        }
        if payload.get(cursor) == Some(&psb::token::NAMED_RECORD)
            || payload.get(cursor..cursor + 2) == Some(&[0xf1, 0xe1])
        {
            break;
        }
        let record_offset = cursor;
        let mut references = Vec::new();
        let mut attributes = Vec::new();
        loop {
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                references.push(reference);
                cursor = next;
            } else if payload
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
            if payload.get(cursor) == Some(&0xe3)
                || payload.get(cursor) == Some(&psb::token::NAMED_RECORD)
            {
                break;
            }
            if payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
                values.extend([Some(0.0), Some(1.0), Some(0.0)]);
                cursor += 2;
                continue;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                references.push(reference);
                cursor = next;
                continue;
            }
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
        loop {
            if payload
                .get(cursor)
                .is_some_and(|prefix| matches!(prefix, 0x0f | 0x18 | 0xe6))
            {
                cursor += 1;
                continue;
            }
            if payload
                .get(cursor)
                .is_some_and(|prefix| matches!(prefix, 0x82..=0x8f))
                && cursor + 6 <= segment_end
            {
                cursor += 6;
                continue;
            }
            let reference_start = match payload.get(cursor..cursor + 2) {
                Some([0xf0 | 0xf1, 0xf7]) => Some(cursor + 2),
                _ if payload.get(cursor) == Some(&psb::token::ENTITY_REF) => Some(cursor + 1),
                _ => None,
            };
            let Some(reference_start) = reference_start else {
                break;
            };
            let Ok((reference, next)) = psb::reference_id(payload, reference_start) else {
                break;
            };
            references.push(reference);
            cursor = next;
        }
        let row_separator = payload.get(cursor) == Some(&0xe3);
        let named_boundary = payload.get(cursor) == Some(&psb::token::NAMED_RECORD);
        let section_boundary = cursor == segment_end;
        if !row_separator && !named_boundary && !section_boundary {
            cursor = record_offset + 1;
            continue;
        }
        let record_end = cursor;
        if row_separator {
            cursor += 1;
        }
        values.resize(6, None);
        entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
            entity_id,
            references,
            attributes,
            endpoints: [
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            ],
            body: payload[record_offset..record_end].to_vec(),
            offset: record_offset,
        }));
    }
    entities
}

pub(crate) fn saved_line_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    let label = b"\xe0\x00entity(line)\0";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(label_offset) = find_bytes(payload, label, search, end) {
        let body_start = label_offset + label.len();
        let body_end = [
            b"\xe0\x00entity(arc)\0".as_slice(),
            b"\xe0\x00entity(circle)\0".as_slice(),
            b"\xe0\x00entity(dummy_ent)\0".as_slice(),
        ]
        .into_iter()
        .filter_map(|next_label| find_bytes(payload, next_label, body_start, end))
        .min()
        .unwrap_or(end);
        entities.extend(saved_line_block(payload, body_start, body_end, cache));
        search = body_end;
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
    if N == 3 && payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
        return Some(std::array::from_fn(|index| {
            Some(if index == 1 { 1.0 } else { 0.0 })
        }));
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

pub(crate) fn saved_arc_scalar(
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> (Option<f64>, usize) {
    if payload.get(offset) == Some(&0x18)
        && payload.get(offset + 1).is_some_and(|next| {
            matches!(
                next,
                0x28 | 0x5e | 0x60 | 0x64 | 0x9b
                    ..=0xa0 | 0xad | 0xcc | 0xd0 | 0xd2 | 0xd5 | 0xde | 0xdf
            )
        })
    {
        return (Some(0.0), offset + 1);
    }
    if payload.get(offset) == Some(&0x28) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    let arc_dict = match payload.get(offset).copied() {
        Some(0x9b) => Some([0x40, 0x10]),
        Some(0x9c) => Some([0x40, 0x11]),
        Some(0x9d) => Some([0x40, 0x12]),
        Some(0x9e) => Some([0x40, 0x13]),
        Some(0x9f) => Some([0x40, 0x14]),
        Some(0xa0) => Some([0x40, 0x15]),
        Some(0x5e) => Some([0x3f, 0xd3]),
        Some(0x60) => Some([0x3f, 0xd5]),
        Some(0x64) => Some([0x3f, 0xd9]),
        Some(0xad) => Some([0x3f, 0xd9]),
        Some(0xcc) => Some([0xbf, 0xf9]),
        Some(0xd0) => Some([0xbf, 0xfe]),
        Some(0xd2) => Some([0xc0, 0x00]),
        Some(0xd5) => Some([0xc0, 0x03]),
        Some(0xde) => Some([0xc0, 0x10]),
        Some(0xdf) => Some([0xc0, 0x11]),
        _ => None,
    };
    if let (Some(head), Some(tail)) = (arc_dict, payload.get(offset + 1..offset + 7)) {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&head);
        raw[2..].copy_from_slice(tail);
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    let decoded = saved_section_scalar(payload, offset, end, cache);
    if decoded.1 > offset + 1 || decoded.0.is_some() {
        return decoded;
    }
    if payload
        .get(offset)
        .is_some_and(|prefix| matches!(prefix, 0x80..=0xdf))
        && offset + 7 <= end
    {
        return (None, offset + 7);
    }
    decoded
}

pub(crate) fn saved_positional_generated_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Vec<FeatureSavedEntity> {
    let (Some(order_table), Some(segments)) = (order_table, segments) else {
        return Vec::new();
    };
    let generated_segments = order_table
        .rows
        .iter()
        .filter_map(|row| {
            (order_table.internal_id(row.external_id) == Some(row.internal_id)
                && order_table.external_id(row.internal_id) == Some(row.external_id))
            .then_some(())?;
            let segment = segments.segment(row.external_id)?;
            Some((row.internal_id, segment))
        })
        .collect::<BTreeMap<_, _>>();
    let mut starts = Vec::new();
    for separator in start..end {
        if payload.get(separator) != Some(&0xe3) {
            continue;
        }
        let row_start = separator + 1;
        let (Some(entity_id), after_id) = segment_int(payload, row_start) else {
            continue;
        };
        if !generated_segments.contains_key(&entity_id) {
            continue;
        }
        let header_end = after_id.saturating_add(24).min(end);
        if after_id > header_end {
            continue;
        }
        if payload[after_id..header_end].contains(&0xe2) {
            starts.push(row_start);
        }
    }
    starts.sort_unstable();
    starts.dedup();

    let mut entities = Vec::new();
    for (index, row_start) in starts.iter().copied().enumerate() {
        let row_end = starts
            .get(index + 1)
            .map_or(end, |next| next.saturating_sub(1));
        let (Some(entity_id), after_id) = segment_int(payload, row_start) else {
            continue;
        };
        let segment = generated_segments[&entity_id];
        let value_count = match segment.kind {
            FeatureSegmentKind::Line => 6,
            FeatureSegmentKind::Arc => 12,
            FeatureSegmentKind::Point => continue,
        };
        if after_id > row_end {
            continue;
        }
        let Some(header_size) = payload[after_id..row_end]
            .iter()
            .position(|byte| *byte == 0xe2)
        else {
            continue;
        };
        let mut cursor = after_id + header_size + 1;
        let mut values = Vec::with_capacity(value_count);
        while cursor < row_end && values.len() < value_count {
            if payload.get(cursor) == Some(&0xe3) {
                break;
            }
            if payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
                values.extend([Some(0.0), Some(1.0), Some(0.0)]);
                cursor += 2;
                continue;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                cursor = next;
                continue;
            }
            if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((_, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                cursor = next;
                continue;
            }
            if payload.get(cursor) == Some(&0xeb) {
                cursor += 6;
                continue;
            }
            if matches!(payload.get(cursor), Some(0xf6)) {
                cursor += 1;
                continue;
            }
            let (value, next) = saved_arc_scalar(payload, cursor, row_end, cache);
            if next <= cursor {
                break;
            }
            values.push(value);
            cursor = next;
        }
        if values.len() != value_count {
            if segment.kind != FeatureSegmentKind::Arc
                || values.len() > value_count
                || (cursor != row_end && payload.get(cursor) != Some(&0xe3))
            {
                continue;
            }
            values.resize(value_count, None);
        }
        match segment.kind {
            FeatureSegmentKind::Line => {
                let endpoints = [
                    [values[0], values[1], values[2]],
                    [values[3], values[4], values[5]],
                ];
                let orientation_matches = match (
                    segment.vertical_horizontal,
                    endpoints[0][0],
                    endpoints[0][1],
                    endpoints[1][0],
                    endpoints[1][1],
                ) {
                    (Some(0), Some(first), _, Some(second), _) => {
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= 1e-9 * scale
                    }
                    (Some(1), _, Some(first), _, Some(second)) => {
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= 1e-9 * scale
                    }
                    _ => false,
                };
                if orientation_matches {
                    let body_end = saved_positional_body_end(payload, row_end);
                    entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
                        entity_id,
                        references: Vec::new(),
                        attributes: Vec::new(),
                        endpoints,
                        body: payload[row_start..body_end].to_vec(),
                        offset: row_start,
                    }));
                }
            }
            FeatureSegmentKind::Arc => {
                let body_end = saved_positional_body_end(payload, row_end);
                entities.push(FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center: [values[0], values[1], values[2]],
                    radius: values[3],
                    endpoints: [
                        [values[4], values[5], values[6]],
                        [values[7], values[8], values[9]],
                    ],
                    parameters: [values[10], values[11]],
                    body: payload[row_start..body_end].to_vec(),
                    offset: row_start,
                }));
            }
            FeatureSegmentKind::Point => {}
        }
    }
    entities
}

fn saved_positional_body_end(payload: &[u8], row_end: usize) -> usize {
    if payload.get(row_end.saturating_sub(1)) == Some(&0xe3) {
        row_end.saturating_sub(1)
    } else {
        row_end
    }
}

pub(crate) fn saved_circular_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
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
            let center = saved_named_scalars::<3>(payload, b"center", body_start, body_end, cache)
                .unwrap_or([None; 3]);
            let radius = saved_named_scalars::<1>(payload, b"radius", body_start, body_end, cache)
                .unwrap_or([None])[0];
            if kind == "arc" {
                let positional = saved_positional_generated_entities(
                    payload,
                    body_start,
                    body_end,
                    cache,
                    order_table,
                    segments,
                );
                let named_body_end = positional.iter().map(saved_entity_offset).min().map_or(
                    body_end,
                    |row_start| {
                        if payload.get(row_start.saturating_sub(1)) == Some(&0xe3) {
                            row_start.saturating_sub(1)
                        } else {
                            row_start
                        }
                    },
                );
                let first = saved_named_scalars::<3>(payload, b"end1", body_start, body_end, cache)
                    .unwrap_or([None; 3]);
                let second =
                    saved_named_scalars::<3>(payload, b"end2", body_start, body_end, cache)
                        .unwrap_or([None; 3]);
                let start_parameter =
                    saved_named_scalars::<1>(payload, b"t0", body_start, body_end, cache)
                        .unwrap_or([None])[0];
                let end_parameter =
                    saved_named_scalars::<1>(payload, b"t1", body_start, body_end, cache)
                        .unwrap_or([None])[0];
                entities.push(FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center,
                    radius,
                    endpoints: [first, second],
                    parameters: [start_parameter, end_parameter],
                    body: payload[body_start..named_body_end].to_vec(),
                    offset: entity_offset,
                }));
                entities.extend(positional);
            } else {
                entities.push(FeatureSavedEntity::Circle(FeatureSavedCircle {
                    entity_id,
                    center,
                    radius,
                    body: payload[body_start..body_end].to_vec(),
                    offset: entity_offset,
                }));
            }
            search = body_end;
        }
    }
    entities
}

pub(crate) fn saved_conic_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    let label = b"\xe0\x00entity(conic)\0";
    let local_system_label = b"\xe0\x02local_sys\0";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(entity_offset) = find_bytes(payload, label, search, end) {
        let body_start = entity_offset + label.len();
        let body_end = find_bytes(payload, b"\xe0\x00entity(", body_start, end).unwrap_or(end);
        let Some(entity_id) = saved_entity_id(payload, body_start, body_end) else {
            search = body_end;
            continue;
        };
        if named_compact_int(payload, b"\xe0\x01type\0", body_start, body_end) != Some(58) {
            search = body_end;
            continue;
        }
        let first = saved_named_scalars::<3>(payload, b"end1", body_start, body_end, cache)
            .unwrap_or([None; 3]);
        let second = saved_named_scalars::<3>(payload, b"end2", body_start, body_end, cache)
            .unwrap_or([None; 3]);
        let start_parameter = saved_named_scalars::<1>(payload, b"t0", body_start, body_end, cache)
            .unwrap_or([None])[0];
        let end_parameter = saved_named_scalars::<1>(payload, b"t1", body_start, body_end, cache)
            .unwrap_or([None])[0];
        let first_coefficient =
            saved_named_scalars::<1>(payload, b"c1", body_start, body_end, cache).unwrap_or([None])
                [0];
        let second_coefficient =
            saved_named_scalars::<1>(payload, b"c2", body_start, body_end, cache).unwrap_or([None])
                [0];
        let local_system =
            find_bytes(payload, local_system_label, body_start, body_end).and_then(|offset| {
                let frame_start = offset + local_system_label.len();
                scalar::decode_saved_conic_local_system_prefix(
                    &payload[frame_start..body_end],
                    cache,
                )
                .map(|(frame, _)| frame)
            });
        entities.push(FeatureSavedEntity::Conic(FeatureSavedConic {
            entity_id,
            endpoints: [first, second],
            parameters: [start_parameter, end_parameter],
            coefficients: [first_coefficient, second_coefficient],
            local_system,
            body: payload[body_start..body_end].to_vec(),
            offset: entity_offset,
        }));
        search = body_end;
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
            body: payload[body_start..body_end].to_vec(),
            offset: entity_offset,
        }));
        search = body_end;
    }
    entities
}

pub(crate) fn saved_spline_entities(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Vec<FeatureSavedEntity> {
    const LABEL: &[u8] = b"\xe0\x00save_entity_ptr(spline)\0";
    const POINTS_LABEL: &[u8] = b"\xe0\x02i_pnts\0";
    const POINTS: &[u8] = b"\xe0\x02i_pnts\0\xf9";
    const TANGENTS_LABEL: &[u8] = b"\xe0\x02end_tangts\0";
    const TANGENTS: &[u8] = b"\xe0\x02end_tangts\0\xf9\x02\x03";
    const PARAMETERS_LABEL: &[u8] = b"\xe0\x02params\0";
    const PARAMETERS: &[u8] = b"\xe0\x02params\0\xf8";
    let mut entities = Vec::new();
    let mut search = start;
    while let Some(entity_offset) = find_bytes(payload, LABEL, search, end) {
        let body_start = entity_offset + LABEL.len();
        let body_end = find_bytes(payload, LABEL, body_start, end).unwrap_or(end);
        let points_label = find_bytes(payload, POINTS, body_start, body_end);
        let entity_id_end = points_label.unwrap_or(body_end);
        let mut declared_point_count = None;
        let mut point_count = None;
        let mut points = Vec::new();
        let mut interpolation_points_body = Vec::new();
        let mut fields_start = body_start;
        if let Some(points_label) = points_label {
            let value_start = points_label + POINTS_LABEL.len();
            let extents_start = points_label + POINTS.len();
            let (declared, dimensions_end) = psb::compact_int(payload, extents_start);
            let (coordinate_count, mut cursor) = psb::compact_int(payload, dimensions_end);
            if dimensions_end > extents_start && cursor > dimensions_end && coordinate_count == 3 {
                declared_point_count = Some(declared);
                interpolation_points_body = payload[value_start..cursor].to_vec();
                point_count = usize::try_from(declared).ok().filter(|point_count| {
                    point_count.saturating_mul(3)
                        <= body_end.saturating_sub(cursor).saturating_mul(16).max(12)
                });
                if let Some(point_count) = point_count {
                    points.reserve(point_count);
                    for _ in 0..point_count {
                        let mut point = [0.0; 3];
                        let mut next_cursor = cursor;
                        let mut complete = true;
                        for coordinate in &mut point {
                            let Some((value, next)) =
                                scalar::decode_in_lane(payload, next_cursor, cache)
                                    .filter(|(_, next)| *next <= body_end)
                            else {
                                complete = false;
                                break;
                            };
                            *coordinate = value;
                            next_cursor = next;
                        }
                        if !complete {
                            break;
                        }
                        points.push(point);
                        cursor = next_cursor;
                    }
                    fields_start = cursor;
                    interpolation_points_body = payload[value_start..cursor].to_vec();
                }
            }
        }
        let decoded_tangents =
            find_bytes(payload, TANGENTS, fields_start, body_end).and_then(|label| {
                let value_start = label + TANGENTS_LABEL.len();
                let mut at = label + TANGENTS.len();
                let mut tangents = [[0.0; 3]; 2];
                for tangent in &mut tangents {
                    for coordinate in tangent {
                        let (value, next) = scalar::decode_in_lane(payload, at, cache)?;
                        (next <= body_end).then_some(())?;
                        *coordinate = value;
                        at = next;
                    }
                }
                Some((tangents, payload[value_start..at].to_vec()))
            });
        let (endpoint_tangents, endpoint_tangents_body) = decoded_tangents
            .map_or((None, None), |(tangents, body)| {
                (Some(tangents), Some(body))
            });
        let decoded_parameters = point_count.and_then(|point_count| {
            find_bytes(payload, PARAMETERS, fields_start, body_end).and_then(|label| {
                let value_start = label + PARAMETERS_LABEL.len();
                let count_at = label + PARAMETERS.len();
                let (count, mut at) = psb::compact_int(payload, count_at);
                (usize::try_from(count).ok() == Some(point_count) && at > count_at).then_some(())?;
                let mut values = Vec::with_capacity(point_count);
                for _ in 0..count {
                    let (value, next) = saved_spline_parameter(payload, at, cache)?;
                    (next <= body_end).then_some(())?;
                    values.push(value);
                    at = next;
                }
                Some((values, payload[value_start..at].to_vec()))
            })
        });
        let (parameters, parameters_body) = decoded_parameters
            .map_or((None, None), |(parameters, body)| {
                (Some(parameters), Some(body))
            });
        entities.push(FeatureSavedEntity::Spline(FeatureSavedSpline {
            entity_id: saved_entity_id(payload, body_start, entity_id_end),
            declared_point_count,
            interpolation_points: points,
            interpolation_points_body,
            endpoint_tangents,
            endpoint_tangents_body,
            parameters,
            parameters_body,
            offset: entity_offset,
        }));
        search = body_start;
    }
    entities
}

pub(crate) fn saved_spline_parameter(
    payload: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    let prefix = *payload.get(offset)?;
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x2d | 0x6d | 0x85 | 0x93 | 0x9e))
    {
        return Some((0.0, offset + 1));
    }
    if matches!(prefix, 0x6d | 0x85 | 0x93 | 0x9e) {
        let tail = payload.get(offset + 1..offset + 7)?;
        let second = prefix.wrapping_sub(0x8b);
        let mut raw = [0; 8];
        raw[0] = if second >= 0x80 { 0x3f } else { 0x40 };
        raw[1] = second;
        raw[2..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 7));
    }
    if prefix == 0x2d {
        let tail = payload.get(offset + 1..offset + 8)?;
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(tail);
        return Some((f64::from_be_bytes(raw), offset + 8));
    }
    scalar::decode_in_lane(payload, offset, cache)
}

pub(crate) fn saved_entity_offset(entity: &FeatureSavedEntity) -> usize {
    match entity {
        FeatureSavedEntity::Line(entity) => entity.offset,
        FeatureSavedEntity::Arc(entity) => entity.offset,
        FeatureSavedEntity::Circle(entity) => entity.offset,
        FeatureSavedEntity::Conic(entity) => entity.offset,
        FeatureSavedEntity::Spline(entity) => entity.offset,
        FeatureSavedEntity::Dummy(entity) => entity.offset,
    }
}

pub(crate) fn saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<FeatureSavedSection> {
    let table = find_bytes(payload, b"\xe0\x00p_saved_result\0", start, end)?;
    let table_end = find_bytes(payload, b"\xe0\x02local_sys\0", table, end)
        .or_else(|| find_bytes(payload, b"\xe0\x00rigid_data\0", table, end))
        .unwrap_or(end);
    let mut entities = saved_line_entities(payload, table, table_end, cache);
    entities.extend(saved_circular_entities(
        payload,
        table,
        table_end,
        cache,
        order_table,
        segments,
    ));
    entities.extend(saved_conic_entities(payload, table, end, cache));
    entities.extend(saved_dummy_entities(payload, table, table_end));
    entities.extend(saved_spline_entities(payload, start, end, cache));
    entities.sort_by_key(saved_entity_offset);
    Some(FeatureSavedSection {
        entities,
        offset: table,
    })
}

pub(crate) fn positional_saved_section(
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Option<FeatureSavedSection> {
    let mut entities =
        saved_positional_generated_entities(payload, start, end, cache, order_table, segments);
    entities.extend(saved_conic_entities(payload, start, end, cache));
    entities.sort_by_key(saved_entity_offset);
    let offset = entities.first().map(saved_entity_offset)?;
    Some(FeatureSavedSection { entities, offset })
}

/// Decode full-turn termination stored inside an owned DEPDB section
/// definition. The owning current-state recipe must independently select a
/// rotational sweep.
pub fn definition_revolution_extents(
    definitions: &[FeatureDefinition],
    operations: &[FeatureOperation],
) -> Vec<FeatureRevolutionExtent> {
    const FULL_TURN: &[u8] = &[
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let mut result = Vec::new();
    for definition in definitions {
        let Some(feature_id) = definition.owner_feature_id else {
            continue;
        };
        let recipe_matches = operations.iter().any(|operation| {
            operation.feature_id == feature_id
                && operation
                    .recipe
                    .is_some_and(|recipe| recipe.kind() == FeatureRecipeKind::Revolve)
        });
        if !recipe_matches {
            continue;
        }
        let offsets = definition
            .body
            .windows(FULL_TURN.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == FULL_TURN).then_some(offset))
            .collect::<Vec<_>>();
        result.extend(offsets.into_iter().map(|offset| FeatureRevolutionExtent {
            feature_id,
            kind: FeatureRevolutionExtentKind::FullTurn,
            offset: definition.offset + offset + 6,
        }));
    }
    result.sort_by_key(|record| record.offset);
    result
}

pub(crate) fn definitions_in_ranges(
    payload: &[u8],
    starts: &[(usize, u32, Option<u32>, bool)],
) -> Vec<FeatureDefinition> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut replay_dimension_class = None;
    let mut replay_variable_class = None;
    let mut replay_relation_class = None;
    let mut replay_skamp_class = None;
    let mut replay_triples_class = None;
    let mut replay_trim_entity_classes: Option<TrimTableClasses> = None;
    let mut replay_trim_vertex_classes: Option<TrimTableClasses> = None;
    let mut replay_order_class = None;
    for (index, &(start, id, owner_override, positional)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _, _, _)| offset);
        let schema_end = starts[index + 1..]
            .iter()
            .find(|(_, _, _, positional)| !positional)
            .map_or(payload.len(), |(offset, _, _, _)| *offset);
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
                    decoded_values: scalar::decode_feature_local_system_slots(&body, &cache)
                        .map(|slots| slots.to_vec()),
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
                let (local_values, local_value_bodies) =
                    decode_optional_scalars(&payload[scalar_start..end], 6, &cache);
                outlines.push(FeatureOutline {
                    phase: OutlinePhase::PreRollback,
                    local_values,
                    local_value_bodies,
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
                let (local_values, local_value_bodies) =
                    decode_optional_scalars(&payload[after_ref + 4..end], 6, &cache);
                outlines.push(FeatureOutline {
                    phase,
                    local_values,
                    local_value_bodies,
                    offset: label_offset,
                });
            }
        }
        outlines.sort_by_key(|outline| outline.offset);
        let variables = variable_table(payload, start, end, &cache).or_else(|| {
            positional
                .then(|| {
                    positional_variable_table(payload, start, end, replay_variable_class?, &cache)
                })
                .flatten()
        });
        if !positional {
            replay_variable_class = variables.as_ref().and_then(|table| table.entity_ref);
        }
        let segments = segment_table(payload, start, end).or_else(|| {
            positional
                .then(|| positional_segment_table(payload, start, end))
                .flatten()
        });
        let trim_entities = trim_entity_table(payload, start, end).or_else(|| {
            if positional {
                positional_trim_entity_table(
                    payload,
                    start,
                    end,
                    replay_trim_entity_classes?,
                    replay_trim_vertex_classes.map(|classes| classes.table),
                )
            } else {
                None
            }
        });
        if !positional {
            replay_trim_entity_classes =
                trim_table_header(payload, b"ent_tab\0", start, end).map(|header| header.classes);
        }
        let trim_vertices =
            trim_vertex_table(payload, start, end, segments.as_ref(), variables.as_ref()).or_else(
                || {
                    if positional {
                        positional_trim_vertex_table(
                            payload,
                            start,
                            end,
                            replay_trim_vertex_classes?,
                            segments.as_ref(),
                            variables.as_ref(),
                        )
                    } else {
                        None
                    }
                },
            );
        if !positional {
            replay_trim_vertex_classes =
                trim_table_header(payload, b"vert_tab\0", start, end).map(|header| header.classes);
        }
        let order_table = order_table(payload, start, end).or_else(|| {
            positional
                .then(|| positional_order_table(payload, start, end, replay_order_class?))
                .flatten()
        });
        if !positional {
            replay_order_class = order_table.as_ref().and_then(|table| table.entity_ref);
        }
        let section_3d = section_3d(payload, start, end).or_else(|| {
            positional
                .then(|| positional_section_3d(payload, start, end))
                .flatten()
        });
        let dimensions = dimension_table(payload, start, end, &cache).or_else(|| {
            positional.then_some(()).and_then(|()| {
                replay_dimension_class
                    .and_then(|table_class| {
                        positional_dimension_table(payload, start, end, table_class, &cache)
                    })
                    .or_else(|| {
                        self_described_positional_dimension_table(payload, start, end, &cache)
                    })
            })
        });
        if !positional {
            replay_dimension_class = dimensions.as_ref().and_then(|table| table.entity_ref);
        }
        let mut relations = relation_table(payload, start, end).or_else(|| {
            positional
                .then(|| positional_relation_table(payload, start, end, replay_relation_class?))
                .flatten()
        });
        if !positional {
            replay_relation_class = relations.as_ref().and_then(|table| table.entity_ref);
            replay_skamp_class = named_array_class(payload, b"skamp_ptr\0", start, schema_end);
            replay_triples_class = named_array_class(payload, b"triples_ptr\0", start, schema_end);
        } else if let Some(table) = &mut relations {
            if table.skamp_header.is_none() {
                if let Some(header) = named_solver_table_header(payload, b"skamp_ptr\0", start, end)
                {
                    table.skamps = feature_skamps(payload, start, end);
                    table.skamp_header = Some(header);
                } else {
                    table.skamps = replay_skamp_class.map_or_else(Vec::new, |table_class| {
                        positional_feature_skamps(payload, start, end, table_class)
                    });
                    table.skamp_header = replay_skamp_class.and_then(|table_class| {
                        positional_solver_table_header(payload, start, end, table_class)
                    });
                }
            }
            if table.triples_header.is_none() {
                if let Some(header) =
                    named_solver_table_header(payload, b"triples_ptr\0", start, end)
                {
                    table.triples = feature_relation_triples(payload, start, end);
                    table.triples_header = Some(header);
                } else {
                    table.triples = replay_triples_class.map_or_else(Vec::new, |table_class| {
                        positional_relation_triples(payload, start, end, table_class)
                    });
                    table.triples_header = replay_triples_class.and_then(|table_class| {
                        positional_solver_table_header(payload, start, end, table_class)
                    });
                }
            }
        }
        let saved_section = saved_section(
            payload,
            start,
            end,
            &cache,
            order_table.as_ref(),
            segments.as_ref(),
        )
        .or_else(|| {
            if positional {
                positional_saved_section(
                    payload,
                    start,
                    end,
                    &cache,
                    order_table.as_ref(),
                    segments.as_ref(),
                )
            } else {
                None
            }
        });
        let owner_feature_id = owner_override.or_else(|| {
            let ids = contextual_references(payload, start, end, b"feat_id", b"gsec2d_ptr")
                .into_iter()
                .map(|(_, id)| id)
                .collect::<BTreeSet<_>>();
            ids.first().copied().filter(|_| ids.len() == 1)
        });
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

fn contextual_references(
    payload: &[u8],
    start: usize,
    end: usize,
    field: &[u8],
    following_record: &[u8],
) -> Vec<(usize, u32)> {
    let needle = [&[psb::token::NAMED_RECORD, 1][..], field, &[0]].concat();
    payload[start..end]
        .windows(needle.len())
        .enumerate()
        .filter_map(|(relative, window)| {
            if window != needle {
                return None;
            }
            let record_start = start + relative;
            let value_start = record_start + needle.len();
            let (value, after_value) = psb::reference_id(payload, value_start).ok()?;
            let following_end = after_value.checked_add(3 + following_record.len())?;
            (following_end <= end
                && payload.get(after_value..after_value + 2)
                    == Some(&[psb::token::NAMED_RECORD, 0])
                && payload.get(after_value + 2..following_end - 1) == Some(following_record)
                && payload.get(following_end - 1) == Some(&0))
            .then_some((record_start, value))
        })
        .collect()
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
fn definition_starts(payload: &[u8]) -> Vec<(usize, u32, Option<u32>, bool)> {
    const PREFIX: &[u8] = b"feat_defs_";
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
        starts.push((offset, id, None, false));
    }
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    let labeled_starts = starts.clone();
    for (index, &(start, _, _, _)) in labeled_starts.iter().enumerate() {
        let end = labeled_starts
            .get(index + 1)
            .map_or(payload.len(), |&(offset, _, _, _)| offset);
        for (offset, owner) in
            contextual_references(payload, start, end, b"feat_id", b"ref_model_info")
        {
            starts.push((offset, owner, Some(owner), true));
        }
    }
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    starts
}

fn depdb_gsec2d_starts(payload: &[u8]) -> Vec<(usize, u32, Option<u32>, bool)> {
    const GSEC: &[u8] = b"gsec2d_ptr\0";
    const NAME: &[u8] = b"name\0S2D";
    payload
        .windows(GSEC.len())
        .enumerate()
        .filter_map(|(start, window)| {
            (window == GSEC).then_some(())?;
            let search_end = start.saturating_add(128).min(payload.len());
            let digits_start = find_bytes(payload, NAME, start, search_end)? + NAME.len();
            let digits_end = payload[digits_start..search_end]
                .iter()
                .position(|byte| *byte == 0)?
                + digits_start;
            let digits = payload.get(digits_start..digits_end)?;
            if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                return None;
            }
            let id = String::from_utf8_lossy(digits).parse::<u32>().ok()?;
            Some((start, id, None, false))
        })
        .collect()
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
pub fn definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    let mut starts = definition_starts(payload);
    let retained_offsets = starts
        .iter()
        .map(|(offset, _, _, _)| *offset)
        .collect::<BTreeSet<_>>();
    let replay_markers = s2d_replay_starts(payload);
    let claimed_markers = claimed_s2d_replay_markers(payload, &starts, &replay_markers);
    let replay_starts = replay_markers
        .into_iter()
        .filter(|offset| !claimed_markers.contains(offset))
        .map(|offset| (offset, inherited_definition_id(&starts, offset), None, true))
        .collect::<Vec<_>>();
    starts.extend(replay_starts);
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
        .into_iter()
        .filter(|definition| retained_offsets.contains(&definition.offset))
        .collect()
}

/// Decode labelled and positional feature definitions embedded directly in a
/// DEPDB section. A labelled `gsec2d_ptr` definition supplies the table schema
/// for its following positional `S2D` instances.
pub fn depdb_definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    let mut starts = definition_starts(payload);
    starts.extend(depdb_gsec2d_starts(payload));
    let replay_markers = s2d_replay_starts(payload);
    let claimed_markers = claimed_s2d_replay_markers(payload, &starts, &replay_markers);
    let replay_starts = replay_markers
        .into_iter()
        .filter(|offset| !claimed_markers.contains(offset))
        .map(|offset| (offset, inherited_definition_id(&starts, offset), None, true))
        .collect::<Vec<_>>();
    starts.extend(replay_starts);
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
}

fn s2d_replay_starts(payload: &[u8]) -> Vec<usize> {
    const PREFIX: &[u8] = b"\xe3S2D";
    payload
        .windows(PREFIX.len())
        .enumerate()
        .filter_map(|(offset, window)| {
            if window != PREFIX {
                return None;
            }
            let suffix = payload.get(offset + PREFIX.len()..)?;
            let nul = suffix.iter().take(12).position(|byte| *byte == 0)?;
            (nul > 0 && suffix[..nul].iter().all(u8::is_ascii_digit)).then_some(offset)
        })
        .collect()
}

fn inherited_definition_id(
    starts: &[(usize, u32, Option<u32>, bool)],
    replay_offset: usize,
) -> u32 {
    starts
        .iter()
        .filter(|(offset, _, _, positional)| !positional && *offset < replay_offset)
        .max_by_key(|(offset, _, _, _)| *offset)
        .map_or(0, |(_, id, _, _)| *id)
}

fn claimed_s2d_replay_markers(
    payload: &[u8],
    starts: &[(usize, u32, Option<u32>, bool)],
    replay_markers: &[usize],
) -> BTreeSet<usize> {
    starts
        .iter()
        .enumerate()
        .filter(|(_, (_, _, _, positional))| *positional)
        .filter_map(|(index, (start, _, _, _))| {
            let end = starts
                .get(index + 1)
                .map_or(payload.len(), |(offset, _, _, _)| *offset);
            replay_markers
                .iter()
                .copied()
                .find(|marker| marker >= start && *marker < end)
        })
        .collect()
}

/// Decode unlabeled positional `S2D` replay instances without assigning an
/// owner. Ownership remains absent unless an independent entity join proves it.
pub fn positional_replay_definitions(payload: &[u8]) -> Vec<FeatureDefinition> {
    let mut starts = definition_starts(payload);
    let replay_markers = s2d_replay_starts(payload);
    let claimed_markers = claimed_s2d_replay_markers(payload, &starts, &replay_markers);
    let pending_offsets = replay_markers
        .into_iter()
        .filter(|offset| !claimed_markers.contains(offset))
        .collect::<BTreeSet<_>>();
    let replay_starts = pending_offsets
        .iter()
        .copied()
        .map(|offset| (offset, inherited_definition_id(&starts, offset), None, true))
        .collect::<Vec<_>>();
    starts.extend(replay_starts);
    starts.sort_unstable_by_key(|&(offset, _, _, _)| offset);
    starts.dedup_by_key(|entry| entry.0);
    definitions_in_ranges(payload, &starts)
        .into_iter()
        .filter(|definition| pending_offsets.contains(&definition.offset))
        .collect()
}

/// Decode one standalone DEPDB `gsec2d_ptr` section whose owner is established
/// by the section's unique procedural-recipe record.
pub fn depdb_section_definition(
    payload: &[u8],
    owner_feature_id: u32,
) -> Option<FeatureDefinition> {
    const GSEC: &[u8] = b"gsec2d_ptr\0";
    const NAME: &[u8] = b"name\0S2D";
    const PREFIX: &[u8] = b"feat_defs_";
    let starts = payload
        .windows(GSEC.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == GSEC).then_some(offset))
        .collect::<Vec<_>>();
    let [start] = starts.as_slice() else {
        return None;
    };
    let name_search_end = start.saturating_add(128).min(payload.len());
    let name = find_bytes(payload, NAME, *start, name_search_end)? + NAME.len();
    let name_end = payload[name..name_search_end]
        .iter()
        .position(|byte| *byte == 0)?
        + name;
    let digits = payload.get(name..name_end)?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let section_id = String::from_utf8_lossy(digits).parse::<u32>().ok()?;
    let end =
        find_bytes(payload, PREFIX, *start + GSEC.len(), payload.len()).unwrap_or(payload.len());
    definitions_in_ranges(
        &payload[..end],
        &[(*start, section_id, Some(owner_feature_id), true)],
    )
    .pop()
}

/// Bind an owner omitted by `feat_id` through the section's unique generated
/// datum entry. An explicit canonical `feat_id` remains authoritative.
pub fn bind_definition_owners(
    definitions: &mut [FeatureDefinition],
    geometry_tables: &[FeatureGeometryTable],
) {
    for definition in definitions
        .iter_mut()
        .filter(|definition| definition.owner_feature_id.is_none())
    {
        let Some(sketch_plane) = definition
            .section_3d
            .as_ref()
            .and_then(|section| section.sketch_plane_entity_id)
        else {
            continue;
        };
        let owners = geometry_tables
            .iter()
            .filter(|table| table.kind == FeatureGeometryTableKind::DatumIds)
            .filter(|table| {
                table
                    .entry_ids
                    .as_ref()
                    .is_some_and(|ids| ids.contains(&sketch_plane))
            })
            .map(|table| table.feature_id)
            .collect::<BTreeSet<_>>();
        if let [owner] = owners.into_iter().collect::<Vec<_>>().as_slice() {
            definition.owner_feature_id = Some(*owner);
        }
    }
}

/// Bind instantiated saved sections through the exact set of trimmed section
/// entities copied into the owning feature's generated-entity table. Schema
/// identifiers remain unchanged; only the omitted canonical owner is filled.
pub fn bind_trimmed_definition_owners(
    definitions: &mut [FeatureDefinition],
    entity_tables: &[FeatureEntityTable],
) {
    let claimed_owner_ids = definitions
        .iter()
        .filter_map(|definition| definition.owner_feature_id)
        .collect::<BTreeSet<_>>();
    let candidates = definitions
        .iter()
        .map(|definition| {
            let external_ids = unique_trimmed_external_ids(definition);
            if definition.owner_feature_id.is_some() || external_ids.is_empty() {
                return BTreeSet::new();
            }
            entity_tables
                .iter()
                .filter_map(|table| {
                    let owner = table.feature_id?;
                    if claimed_owner_ids.contains(&owner) {
                        return None;
                    }
                    let source_ids = generated_class_200_source_entity_ids(table);
                    (source_ids == external_ids).then_some(owner)
                })
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    let mut owner_candidate_counts = BTreeMap::new();
    for owner in candidates.iter().flat_map(|owners| owners.iter()) {
        *owner_candidate_counts.entry(*owner).or_insert(0usize) += 1;
    }
    for (definition, owners) in definitions.iter_mut().zip(candidates) {
        let Some(owner) = owners
            .first()
            .copied()
            .filter(|_| owners.len() == 1)
            .filter(|owner| owner_candidate_counts.get(owner) == Some(&1))
        else {
            continue;
        };
        definition.owner_feature_id = Some(owner);
    }
}

/// Bind unlabeled positional definitions through section-entity IDs in the
/// owning generated-entity table. A uniquely keyed trimmed-entity roster is
/// exact; otherwise the generated IDs must be a nonempty subset of the order
/// table. Empty and non-unique joins remain unbound.
pub fn bind_replay_definition_owners(
    definitions: &mut [FeatureDefinition],
    entity_tables: &[FeatureEntityTable],
    claimed_owner_ids: &BTreeSet<u32>,
) {
    let candidates = definitions
        .iter()
        .map(|definition| {
            if definition.owner_feature_id.is_some() {
                return BTreeSet::new();
            }
            let trimmed_external_ids = unique_trimmed_external_ids(definition);
            let order_external_ids = definition
                .order_table
                .as_ref()
                .map(|table| {
                    table
                        .rows
                        .iter()
                        .map(|row| row.external_id)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            if trimmed_external_ids.is_empty() && order_external_ids.is_empty() {
                return BTreeSet::new();
            }
            let exact_candidates = entity_tables
                .iter()
                .filter_map(|table| {
                    let owner = table.feature_id?;
                    if claimed_owner_ids.contains(&owner) {
                        return None;
                    }
                    let source_ids = generated_class_200_source_entity_ids(table);
                    (!trimmed_external_ids.is_empty() && source_ids == trimmed_external_ids)
                        .then_some(owner)
                })
                .collect::<BTreeSet<_>>();
            if !exact_candidates.is_empty() {
                return exact_candidates;
            }
            entity_tables
                .iter()
                .filter_map(|table| {
                    let owner = table.feature_id?;
                    if claimed_owner_ids.contains(&owner) {
                        return None;
                    }
                    let source_ids = generated_class_200_source_entity_ids(table);
                    (!source_ids.is_empty() && source_ids.is_subset(&order_external_ids))
                        .then_some(owner)
                })
                .collect()
        })
        .collect::<Vec<_>>();
    let mut owner_candidate_counts = BTreeMap::new();
    for owner in candidates.iter().flat_map(|owners| owners.iter()) {
        *owner_candidate_counts.entry(*owner).or_insert(0usize) += 1;
    }
    for (definition, owners) in definitions.iter_mut().zip(candidates) {
        let Some(owner) = owners
            .first()
            .copied()
            .filter(|_| owners.len() == 1)
            .filter(|owner| owner_candidate_counts.get(owner) == Some(&1))
        else {
            continue;
        };
        definition.id = owner;
        definition.owner_feature_id = Some(owner);
    }
}

fn unique_trimmed_external_ids(definition: &FeatureDefinition) -> BTreeSet<u32> {
    definition
        .trim_entities
        .as_ref()
        .filter(|table| table.has_unique_external_ids())
        .map(|table| table.rows.iter().map(|row| row.external_id).collect())
        .unwrap_or_default()
}

/// Bind bounded section definitions through the consecutive recipe, internal
/// datum, and sketch-plane identifier chain. Repeated definitions for one
/// plane remain unowned because the current regeneration snapshot is not
/// established.
pub fn bind_section_owners(
    definitions: &mut [FeatureDefinition],
    operations: &[FeatureOperation],
    section_ranges: &[(usize, usize)],
) {
    let in_section_range = |offset: usize| {
        section_ranges
            .iter()
            .any(|(start, end)| offset >= *start && offset < *end)
    };
    let claimed_owner_ids = definitions
        .iter()
        .filter_map(|definition| definition.owner_feature_id)
        .collect::<BTreeSet<_>>();
    let mut definitions_per_plane = BTreeMap::new();
    for plane_id in definitions.iter().filter_map(|definition| {
        (definition.owner_feature_id.is_none() && in_section_range(definition.offset))
            .then_some(definition.section_3d.as_ref()?.sketch_plane_entity_id?)
    }) {
        *definitions_per_plane.entry(plane_id).or_insert(0usize) += 1;
    }
    let mut ordered_operations = operations.iter().collect::<Vec<_>>();
    ordered_operations.sort_by_key(|operation| operation.offset);
    for definition in definitions.iter_mut().filter(|definition| {
        definition.owner_feature_id.is_none() && in_section_range(definition.offset)
    }) {
        let Some(plane_id) = definition
            .section_3d
            .as_ref()
            .and_then(|section| section.sketch_plane_entity_id)
            .filter(|plane_id| *plane_id >= 2)
        else {
            continue;
        };
        if definitions_per_plane.get(&plane_id) != Some(&1) {
            continue;
        }
        let owner_id = plane_id - 2;
        let datum_id = plane_id - 1;
        if claimed_owner_ids.contains(&owner_id) {
            continue;
        }
        let matches = ordered_operations
            .windows(2)
            .filter(|pair| {
                pair[0].feature_id == owner_id
                    && pair[0].recipe.is_some()
                    && pair[1].feature_id == datum_id
                    && pair[1].recipe.is_none()
            })
            .count();
        if matches == 1 {
            if definition.id == 0 {
                definition.id = owner_id;
            }
            definition.owner_feature_id = Some(owner_id);
        }
    }
}
