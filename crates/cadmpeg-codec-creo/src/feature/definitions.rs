// SPDX-License-Identifier: Apache-2.0
//! `FeatDefs` / DEPDB feature definitions and owner binding.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_core::decode::{bounded_len, index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;

use crate::psb;
use crate::scalar;

use super::entity::{generated_class_200_source_entity_ids, FeatureEntityTable};
use super::operations::{FeatureOperation, FeatureRecipeKind};
use super::rows::{FeatureGeometryTable, FeatureRevolutionExtent};
use super::segment_rows::{SegmentRow, SegmentRows};

mod points;
mod trim;

use trim::{
    positional_trim_entity_table, positional_trim_vertex_table, trim_entity_table,
    trim_table_header, trim_vertex_table, TrimTableClasses,
};

const EPS_PARAMETER_AGREEMENT: f64 = 1.0e-9;
const TRIM_COORDINATE_EPS: f64 = 1.0e-9;

/// The byte before `offset`. There is no preceding byte at the start of the
/// payload, so a row at offset zero has no separator before it.
fn preceding_byte(payload: &[u8], offset: usize) -> Option<u8> {
    payload.get(offset.checked_sub(1)?).copied()
}

/// Definition-space parameter-frame field in a `FeatDefs` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureParameterFrameKind {
    /// `local_sys` frame field.
    LocalSystem,
    /// `transf` transform field.
    Transform,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureParameterFrameKind {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

/// One `f9 04 03` definition-space parameter frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureParameterFrame {
    /// Frame field kind.
    pub(crate) kind: FeatureParameterFrameKind,
    /// Exact scalar-body bytes after `f9 04 03`.
    pub(crate) body: Vec<u8>,
    /// Twelve values when the body consists entirely of defined scalar tokens.
    pub(crate) decoded_values: Option<cadmpeg_ir::units::FiniteVector<12>>,
    /// Byte offset of the field label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureParameterFrame {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.kind,
                &self.body,
                self.decoded_values
                    .as_ref()
                    .map(cadmpeg_ir::units::FiniteVector::as_raw),
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One instantiated row from a feature definition's `place_instruction_ptrs`
/// table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeaturePlacementInstruction {
    /// Stored placement instruction family.
    pub(crate) kind: u32,
    /// Whether the scalar offset lane stores exact zero.
    pub(crate) zero_offset: bool,
    /// Optional driving dimension identifier.
    pub(crate) dimension_id: Option<u32>,
    /// Optional referenced placement object.
    pub(crate) reference_id: Option<u32>,
    /// First optional geometry operand.
    pub(crate) geometry1_id: Option<u32>,
    /// Second optional geometry operand.
    pub(crate) geometry2_id: Option<u32>,
    /// First membership selector.
    pub(crate) member1: u32,
    /// Second membership selector.
    pub(crate) member2: u32,
    /// Byte offset of the positional row marker.
    pub(crate) offset: usize,
}

/// Feature-history phase associated with a local outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutlinePhase {
    /// Labeled `outline` before rollback.
    PreRollback,
    /// Positional replay after rollback.
    PostRollback,
    /// Positional replay after regeneration.
    PostRegen,
}

impl cadmpeg_core::decode::cost::DecodeCost for OutlinePhase {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

/// Six-slot feature-local outline bounds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureOutline {
    /// Feature-history phase.
    pub(crate) phase: OutlinePhase,
    /// Six scalar slots and their encoded bodies; undefined values remain `None`.
    pub(crate) local_scalars: [DecodedField<Option<f64>>; 6],
    /// Byte offset of the outline label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureOutline {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.phase, &self.local_scalars, &self.offset),
            ctx,
            operation,
        )
    }
}

fn outline_scalars(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<[DecodedField<Option<f64>>; 6], CodecError> {
    let mut cursor = 0;
    let mut fields = std::array::from_fn(|_| DecodedField {
        value: None,
        body: Vec::new(),
    });
    for field in &mut fields {
        if cursor >= payload.len() || payload.get(cursor) == Some(&psb::token::NAMED_RECORD) {
            break;
        }
        let start = cursor;
        let value = if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, cache) {
            cursor = next;
            Some(value)
        } else {
            cursor += 1;
            None
        };
        *field = DecodedField {
            value,
            body: ctx.copy_retained(&payload[start..cursor], "creo feature outline scalar body")?,
        };
    }
    Ok(fields)
}

/// Stored state of a solver scalar token.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScalarLane {
    Value(f64),
    DimensionDriven,
    Undefined,
}

impl cadmpeg_core::decode::cost::DecodeCost for ScalarLane {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Value(value) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(1_u8, value), ctx, operation)
            }
            Self::DimensionDriven | Self::Undefined => Ok(1),
        }
    }
}

impl ScalarLane {
    pub(crate) fn value(self) -> Option<f64> {
        match self {
            Self::Value(value) => Some(value),
            Self::DimensionDriven | Self::Undefined => None,
        }
    }
}

/// Solver-variable class carried by a compact integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum VariableType {
    Dimension,
    U,
    V,
    Radius,
    Parameter,
    Selector,
    Result,
    Auxiliary,
    Unknown(UnknownVariableType),
}

impl cadmpeg_core::decode::cost::DecodeCost for VariableType {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Unknown(value) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(1_u8, value.0),
                ctx,
                operation,
            ),
            Self::Dimension
            | Self::U
            | Self::V
            | Self::Radius
            | Self::Parameter
            | Self::Selector
            | Self::Result
            | Self::Auxiliary => Ok(1),
        }
    }
}

/// Unclassified code, constructed only by normalizing the encoded integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct UnknownVariableType(u32);

impl From<u32> for VariableType {
    fn from(code: u32) -> Self {
        match code {
            0 => Self::Dimension,
            1 => Self::U,
            2 => Self::V,
            3 => Self::Radius,
            4 => Self::Parameter,
            5 => Self::Selector,
            6 => Self::Result,
            7 => Self::Auxiliary,
            _ => Self::Unknown(UnknownVariableType(code)),
        }
    }
}

impl VariableType {
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::Dimension => 0,
            Self::U => 1,
            Self::V => 2,
            Self::Radius => 3,
            Self::Parameter => 4,
            Self::Selector => 5,
            Self::Result => 6,
            Self::Auxiliary => 7,
            Self::Unknown(UnknownVariableType(code)) => code,
        }
    }
}

/// One positional solver-variable row from `var_arr`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureVariableRow {
    /// Variable class: `1` is section `u`, `2` is section `v`, `3` is radius.
    pub(crate) variable_type: VariableType,
    /// Point or solver-variable key.
    pub(crate) key: u32,
    /// Solved value when the scalar token is defined inline.
    pub(crate) value: ScalarLane,
    /// Exact encoded scalar body of the stored value.
    pub(crate) value_body: Vec<u8>,
    /// Pre-solve estimate when defined inline.
    pub(crate) guess: ScalarLane,
    /// Exact encoded scalar body of the pre-solve estimate.
    pub(crate) guess_body: Vec<u8>,
    /// Stored solver-known flag.
    pub(crate) known: Option<u32>,
    /// Stored solver homogeneity class.
    pub(crate) homogeneity: Option<u32>,
    /// Solver unknown identifier from the third trailing compact field.
    pub(crate) uvar_id: Option<u32>,
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureVariableRow {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.variable_type,
                    &self.key,
                    &self.value,
                    &self.value_body,
                    &self.guess,
                    &self.guess_body,
                ),
                (&self.known, &self.homogeneity, &self.uvar_id, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

/// One section-frame point joined from `var_arr` type-1/type-2 rows.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSectionPoint {
    /// Shared variable-row key.
    pub(crate) point_id: u32,
    /// Section `u` coordinate.
    pub(crate) u: Option<f64>,
    /// Section `v` coordinate.
    pub(crate) v: Option<f64>,
}

/// Solved section-variable table from one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureVariableTable {
    /// Count declared by the `f8` opener.
    pub(crate) declared_count: u32,
    /// Entity-table reference following the opener.
    pub(crate) entity_ref: Option<u32>,
    /// Positional variable rows in stored order.
    pub(crate) rows: Vec<FeatureVariableRow>,
    /// Byte offset of the `var_arr` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureVariableTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.entity_ref,
                &self.rows,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

#[derive(Debug)]
pub(crate) struct ReconciledPoints<T> {
    pub(crate) points: BTreeMap<u32, T>,
    pub(crate) ambiguous: BTreeSet<u32>,
}

impl FeatureVariableTable {
    /// Whether every row declared by the table decoded.
    pub(crate) fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok() == Some(self.rows.len())
    }

    /// Join unique coordinate rows by point identity.
    #[cfg(test)]
    pub(crate) fn points(&self) -> Vec<FeatureSectionPoint> {
        let mut coordinates = BTreeMap::<u32, (Option<f64>, Option<f64>)>::new();
        for row in self
            .rows
            .iter()
            .filter(|row| matches!(row.variable_type, VariableType::U | VariableType::V))
        {
            coordinates.entry(row.key).or_insert((None, None));
        }
        for (&point_id, point) in &mut coordinates {
            let mut u_rows = self
                .rows
                .iter()
                .filter(|row| row.key == point_id && row.variable_type == VariableType::U);
            let u = u_rows.next();
            if u_rows.next().is_none() {
                point.0 = u.and_then(|row| row.value.value());
            }
            let mut v_rows = self
                .rows
                .iter()
                .filter(|row| row.key == point_id && row.variable_type == VariableType::V);
            let v = v_rows.next();
            if v_rows.next().is_none() {
                point.1 = v.and_then(|row| row.value.value());
            }
        }
        coordinates
            .into_iter()
            .map(|(point_id, (u, v))| FeatureSectionPoint { point_id, u, v })
            .collect()
    }


}

/// One positional solver-equation row from `eqtn_arr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEquation {
    /// Equation identifier from the first positional field.
    pub(crate) equation_id: u32,
    /// Solver function identifier from the second positional field.
    pub(crate) function_id: u32,
    /// Explicit argument-slot count, when the row uses the counted form.
    pub(crate) explicit_argument_count: Option<u32>,
    /// Argument slots in stored order. Expansion markers occupy their
    /// documented number of slots; `None` is the native null slot.
    pub(crate) arguments: Vec<Option<u32>>,
    /// Exact encoded argument body between the argument-count marker and the
    /// auxiliary marker.
    pub(crate) arguments_body: Vec<u8>,
    /// Exact encoded auxiliary field body.
    pub(crate) auxiliary_body: Vec<u8>,
    /// Exact row bytes, including the `e2` row terminator when present.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

/// Solver-equation table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureEquationTable {
    /// Count declared by the `f8` opener. Its relationship to replay rows is
    /// retained without assuming whether it includes the prototype.
    pub(crate) declared_count: u32,
    /// Entity-table reference following the opener, when present.
    pub(crate) entity_ref: Option<u32>,
    /// Exact named prototype body, including its row-class reference.
    pub(super) prototype_body: Vec<u8>,
    /// Positional equation rows in stored order.
    pub(crate) rows: Vec<FeatureEquation>,
    /// Byte offset of the `eqtn_arr` label in the original stream.
    pub(crate) offset: usize,
}

/// Defined positional segment family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureSegmentKind {
    /// Type `2` line segment with its endpoint IDs.
    Line([u32; 2]),
    /// Type `3` circular-arc segment with its endpoint IDs.
    Arc([u32; 2]),
    /// Type `5` isolated point entity with its point ID.
    Point(u32),
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSegmentKind {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Line(ends) | Self::Arc(ends) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(1_u8, ends), ctx, operation)
            }
            Self::Point(point) => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(1_u8, point), ctx, operation)
            }
        }
    }
}

/// One positional `segtab_ptr` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSegment {
    /// Segment family and its point identifiers.
    pub(crate) kind: FeatureSegmentKind,
    /// Three direction fields; control-range sentinels remain `None`.
    pub(crate) directions: [Option<u32>; 3],
    /// Arc center point ID, or `None` for the null sentinel.
    pub(crate) center_id: Option<u32>,
    /// Arc orientation field.
    pub(crate) arc_orientation: Option<u32>,
    /// Vertical/horizontal constraint field.
    pub(crate) vertical_horizontal: Option<u32>,
    /// Radius reference field.
    pub(crate) radius_ref: Option<u32>,
    /// Secondary radius reference field.
    pub(crate) radius2_ref: Option<u32>,
    /// External segment identifier used by the order table.
    pub(crate) external_id: u32,
    /// Exact positional row bytes from the optional type wrapper or family
    /// discriminator through the `e2` row close. Empty for a labeled
    /// prototype row.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.kind,
                    &self.directions,
                    &self.center_id,
                    &self.arc_orientation,
                    &self.vertical_horizontal,
                    &self.radius_ref,
                ),
                (
                    &self.radius2_ref,
                    &self.external_id,
                    &self.body,
                    &self.offset,
                ),
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureSegment {
    /// Endpoint slots into the section variable table. A point repeats its ID.
    pub(crate) fn point_ids(&self) -> [u32; 2] {
        match self.kind {
            FeatureSegmentKind::Line(points) | FeatureSegmentKind::Arc(points) => points,
            FeatureSegmentKind::Point(point) => [point; 2],
        }
    }
}

/// One circular type `10` `segtab_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureCircleSegment {
    /// Center point ID into the section variable table.
    pub(crate) center_id: u32,
    /// Radius reference into the section solver namespace.
    pub(crate) radius_ref: u32,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureCircleSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.center_id,
                &self.radius_ref,
                &self.external_id,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One point type `1` `segtab_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeaturePointSegment {
    /// Point ID stored in the center-point field.
    pub(crate) point_id: u32,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeaturePointSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.point_id, &self.external_id, &self.offset),
            ctx,
            operation,
        )
    }
}

/// One centered construction-line type `47` `segtab_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureCenteredLineSegment {
    /// Center point reference stored by the section solver.
    pub(crate) center_id: u32,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureCenteredLineSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.center_id, &self.external_id, &self.offset),
            ctx,
            operation,
        )
    }
}

/// One type `25` section-reference line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureReferenceLineSegment {
    /// Three stored direction fields.
    pub(crate) directions: [Option<u32>; 3],
    /// Optional endpoint IDs into the section variable table.
    pub(crate) point_ids: [Option<u32>; 2],
    /// Vertical/horizontal constraint field.
    pub(crate) vertical_horizontal: Option<u32>,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureReferenceLineSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.directions,
                &self.point_ids,
                &self.vertical_horizontal,
                &self.external_id,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One type `12` bounded section curve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureBoundedCurveSegment {
    /// Three stored direction fields.
    pub(crate) directions: [Option<u32>; 3],
    /// Endpoint IDs into the section variable table.
    pub(crate) point_ids: [u32; 2],
    /// Stored center-point field.
    pub(crate) center_id: Option<u32>,
    /// Stored arc-orientation field.
    pub(crate) arc_orientation: Option<u32>,
    /// Stored vertical/horizontal field.
    pub(crate) vertical_horizontal: Option<u32>,
    /// Stored radius-reference field.
    pub(crate) radius_ref: Option<u32>,
    /// Stored secondary-radius-reference field.
    pub(crate) radius2_ref: Option<u32>,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureBoundedCurveSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.directions,
                    &self.point_ids,
                    &self.center_id,
                    &self.arc_orientation,
                    &self.vertical_horizontal,
                    &self.radius_ref,
                ),
                (&self.radius2_ref, &self.external_id, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

/// One type `58` saved-conic section row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureConicSegment {
    /// Center point reference stored by the section solver.
    pub(crate) center_id: u32,
    /// First coefficient reference stored by the section solver.
    pub(crate) first_coefficient_ref: u32,
    /// Second coefficient reference stored by the section solver.
    pub(crate) second_coefficient_ref: u32,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureConicSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.center_id,
                &self.first_coefficient_ref,
                &self.second_coefficient_ref,
                &self.external_id,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One fully framed `segtab_ptr` row outside the core segment-family enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureOpaqueSegment {
    /// Stored segment-family discriminator.
    pub(crate) kind: u32,
    /// Three stored direction fields.
    pub(crate) directions: [Option<u32>; 3],
    /// Two stored point fields.
    pub(crate) point_ids: [Option<u32>; 2],
    /// Stored center-point field.
    pub(crate) center_id: Option<u32>,
    /// Stored arc-orientation field.
    pub(crate) arc_orientation: Option<u32>,
    /// Stored vertical/horizontal field.
    pub(crate) vertical_horizontal: Option<u32>,
    /// Stored radius-reference field.
    pub(crate) radius_ref: Option<u32>,
    /// Stored secondary-radius-reference field.
    pub(crate) radius2_ref: Option<u32>,
    /// External segment identifier used by section tables.
    pub(crate) external_id: u32,
    /// Exact positional row bytes from the optional type wrapper or family
    /// discriminator through the `e2` row close. Empty for a labeled
    /// prototype row.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureOpaqueSegment {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.kind,
                    &self.directions,
                    &self.point_ids,
                    &self.center_id,
                    &self.arc_orientation,
                    &self.vertical_horizontal,
                ),
                (
                    &self.radius_ref,
                    &self.radius2_ref,
                    &self.external_id,
                    &self.body,
                    &self.offset,
                ),
            ),
            ctx,
            operation,
        )
    }
}

/// Defining-sketch segment table from one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSegmentTable {
    /// Count declared by the `f8` opener.
    pub(crate) declared_count: u32,
    /// Whether the declared count includes an inherited prototype omitted from
    /// the positional replay body.
    pub(crate) has_elided_prototype: bool,
    /// Entity-table reference following the opener.
    pub(crate) entity_ref: Option<u32>,
    /// Source rows admitted by external identity across all segment families.
    pub(crate) rows: SegmentRows,
    /// Byte offset of the `segtab_ptr` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSegmentTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.has_elided_prototype,
                &self.entity_ref,
                &self.rows,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureSegmentTable {
    /// Whether every row declared by the table decoded.
    pub(crate) fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok()
            == Some(usize::from(self.has_elided_prototype) + self.rows.len())
    }

    /// Resolve a unique ordinary row without requiring whole-table completeness.
    pub(crate) fn unique_segment(&self, external_id: u32) -> Option<&FeatureSegment> {
        match self.rows.get(external_id)? {
            SegmentRow::Ordinary(row) => Some(row),
            _ => None,
        }
    }

    /// Resolve a uniquely identified defining-sketch segment from a complete table.
    pub(crate) fn segment(&self, external_id: u32) -> Option<&FeatureSegment> {
        self.is_complete().then_some(())?;
        self.unique_segment(external_id)
    }
}

/// Solved/trimmed section entity family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrimEntityKind {
    /// No center vertex: trimmed line.
    Line,
    /// Center vertex present: trimmed circular arc.
    Arc {
        /// Solved center vertex identifier.
        center_vertex: u32,
    },
}

impl cadmpeg_core::decode::cost::DecodeCost for TrimEntityKind {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Line => Ok(1),
            Self::Arc { center_vertex } => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(1_u8, center_vertex),
                ctx,
                operation,
            ),
        }
    }
}

/// One positional `ent_tab` replay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureTrimEntity {
    /// External ID matching a `segtab` row.
    pub(crate) external_id: u32,
    /// Entity mode field.
    pub(crate) mode: Option<u32>,
    /// Solved start and end vertex IDs.
    pub(crate) vertices: [u32; 2],
    /// Trimmed entity geometry.
    pub(crate) kind: TrimEntityKind,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureTrimEntity {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.external_id,
                &self.mode,
                &self.vertices,
                &self.kind,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureTrimEntity {
    /// Solved center vertex identifier for an arc.
    pub(crate) fn center_vertex(&self) -> Option<u32> {
        match self.kind {
            TrimEntityKind::Line => None,
            TrimEntityKind::Arc { center_vertex } => Some(center_vertex),
        }
    }
}

/// One stored hash bucket in a native trim table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureTrimBucket {
    /// Zero-based bucket index.
    pub(crate) index: u32,
    /// Number of entries declared by the bucket array opener.
    pub(crate) declared_entry_count: u32,
    /// Number of structurally complete entries decoded within the bucket
    /// frame. Absent when the scan decodes more entries than the stored `u32`
    /// count can be compared against.
    pub(crate) decoded_entry_count: Option<u32>,
    /// Byte offset of the stored bucket index.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureTrimBucket {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.index,
                &self.declared_entry_count,
                &self.decoded_entry_count,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureTrimBucket {
    /// Whether every declared entry has one complete stored body.
    fn is_complete(&self) -> bool {
        self.decoded_entry_count == Some(self.declared_entry_count)
    }
}

/// Solved/trimmed entity graph for one feature definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureTrimEntityTable {
    /// Count declared by the table opener when present.
    pub(crate) declared_count: Option<u32>,
    /// Native table-class reference when present.
    pub(crate) entity_ref: Option<u32>,
    /// Native row-class reference when present.
    pub(crate) entry_ref: Option<u32>,
    /// Explicit hash buckets decoded in stored order.
    pub(crate) buckets: Vec<FeatureTrimBucket>,
    /// Complete positional rows in stored order.
    pub(crate) rows: Vec<FeatureTrimEntity>,
    /// Sorted external IDs present in the trimmed profile.
    pub(crate) solved_external_ids: Vec<u32>,
    /// Byte offset of the `ent_tab` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureTrimEntityTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.declared_count,
                    &self.entity_ref,
                    &self.entry_ref,
                    &self.buckets,
                    &self.rows,
                    &self.solved_external_ids,
                ),
                (&self.offset,),
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureTrimEntityTable {
    /// Whether every declared bucket and entry body is structurally complete.
    pub(crate) fn has_complete_bucket_frame(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        complete_bucket_frame(ctx, self.declared_count, &self.buckets)
    }

    /// Whether each retained external entity identifier occurs once, from a
    /// sorted scratch copy of the identifiers.
    pub(crate) fn has_unique_external_ids(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        const OPERATION: &str = "creo trim external ID uniqueness";
        let (mut ids, _storage) = ctx.temporary_vec(self.rows.len(), OPERATION)?;
        for row in ctx.admit_iter(&self.rows, OPERATION)? {
            ids.push(row.external_id);
        }
        ctx.sort_unstable_by(&mut ids, |id| id, Ord::cmp, OPERATION)?;
        Ok(!ctx.any_by(ids.windows(2), |pair| Ok(pair[0] == pair[1]), OPERATION)?)
    }
}

/// One solved trim vertex and the two trimmed entities incident to it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureTrimVertex {
    /// Vertex identifier shared with `ent_tab` endpoint and center fields.
    pub(crate) vertex_id: u32,
    /// Distinct `ent_tab` external entity identifiers meeting at the vertex.
    pub(crate) entities: Vec<u32>,
    /// Solved section-frame coordinates for a uniquely resolved carrier junction.
    pub(crate) section_coordinates: Option<cadmpeg_ir::units::FinitePoint2>,
    /// Byte offset of the positional triple in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureTrimVertex {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.vertex_id,
                &self.entities,
                self.section_coordinates
                    .map(|value| (value.get().u, value.get().v)),
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// Solved trim-vertex adjacency table for one feature definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureTrimVertexTable {
    /// Count declared by the table opener when present.
    pub(crate) declared_count: Option<u32>,
    /// Native table-class reference when present.
    pub(crate) entity_ref: Option<u32>,
    /// Native row-class reference when present.
    pub(crate) entry_ref: Option<u32>,
    /// Explicit hash buckets decoded in stored order.
    pub(crate) buckets: Vec<FeatureTrimBucket>,
    /// Complete validated vertex rows in stored order.
    pub(crate) rows: Vec<FeatureTrimVertex>,
    /// Byte offset of the `vert_tab` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureTrimVertexTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.entity_ref,
                &self.entry_ref,
                &self.buckets,
                &self.rows,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureTrimVertexTable {
    /// Whether every declared bucket and entry body is structurally complete.
    pub(crate) fn has_complete_bucket_frame(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        complete_bucket_frame(ctx, self.declared_count, &self.buckets)
    }
}

/// Whether every declared hash-bucket index was decoded in order and every
/// bucket's entries are complete, charging each bucket visited.
fn complete_bucket_frame(
    ctx: &DecodeContext<'_>,
    declared_count: Option<u32>,
    buckets: &[FeatureTrimBucket],
) -> Result<bool, CodecError> {
    if declared_count.is_some_and(|count| usize::try_from(count).ok() != Some(buckets.len())) {
        return Ok(false);
    }
    let ordered = declared_count.is_none()
        || ctx.all_by(
            buckets.iter().enumerate(),
            |(position, bucket)| Ok(usize::try_from(bucket.index).ok() == Some(position)),
            "creo trim bucket index sequence",
        )?;
    Ok(ordered
        && ctx.all_by(
            buckets,
            |bucket| Ok(bucket.is_complete()),
            "creo trim bucket completeness",
        )?)
}

/// One generated-entity ordering row from a gsec3d section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureOrderRow {
    /// Section entity identifier matching a defining-sketch segment.
    pub(crate) external_id: u32,
    /// One-based position in the feature's generated-entity table.
    pub(crate) internal_id: u32,
    /// Orientation and side flags stored for the generated entity.
    pub(crate) bitmask: u32,
    /// Byte offset of the positional triple in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureOrderRow {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.external_id,
                &self.internal_id,
                &self.bitmask,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// Generated-entity ordering table for one gsec3d section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureOrderTable {
    /// Count declared by the `f8` opener.
    pub(crate) declared_count: u32,
    /// Whether `declared_count` includes a structural prototype outside `rows`.
    pub(crate) has_prototype: bool,
    /// Entity-table class reference following the opener.
    pub(crate) entity_ref: Option<u32>,
    /// Complete positional triples in stored order.
    pub(crate) rows: super::order_rows::OrderRows,
    /// Byte offset of the `order_table` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureOrderTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.has_prototype,
                &self.entity_ref,
                &self.rows,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureOrderTable {
    /// Whether every entry declared by the table opener was decoded.
    pub(crate) fn is_complete(&self) -> bool {
        usize::try_from(self.declared_count).ok()
            == Some(usize::from(self.has_prototype) + self.rows.len())
    }

    /// Resolve a generated-entity position to its section entity identifier.
    pub(crate) fn external_id(&self, internal_id: u32) -> Option<u32> {
        if !self.is_complete() {
            return None;
        }
        let external_id = self.rows.by_internal(internal_id)?.external_id;
        self.rows.by_external(external_id).map(|_| external_id)
    }

    /// Resolve a section entity identifier to its generated-entity position.
    pub(crate) fn internal_id(&self, external_id: u32) -> Option<u32> {
        if !self.is_complete() {
            return None;
        }
        let internal_id = self.rows.by_external(external_id)?.internal_id;
        self.rows.by_internal(internal_id).map(|_| internal_id)
    }
}

/// Defined value of a one-byte binary section flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryFlag {
    /// Stored byte `00`.
    Clear,
    /// Stored byte `01`.
    Set,
}

impl cadmpeg_core::decode::cost::DecodeCost for BinaryFlag {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
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
pub(crate) struct FeatureSectionOrientation {
    /// Section-side flip.
    pub(crate) section_flip: Option<BinaryFlag>,
    /// Orientation-reference type discriminator.
    pub(crate) reference_type: Option<u32>,
    /// Referenced sketch segment identifier.
    pub(crate) segment_id: Option<u32>,
    /// Referenced-plane flip.
    pub(crate) reference_flip: Option<BinaryFlag>,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSectionOrientation {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.section_flip,
                &self.reference_type,
                &self.segment_id,
                &self.reference_flip,
            ),
            ctx,
            operation,
        )
    }
}

/// One positional gsec3d reference-plane row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSectionReferencePlane {
    /// Row `plane_id` entity identifier.
    pub(crate) plane_entity_id: u32,
    /// Row `ref_type` discriminator.
    pub(crate) reference_type: Option<u32>,
    /// Row `ext_ref_id` identifier.
    pub(crate) external_reference_id: Option<u32>,
    /// Row `seg_id` identifier.
    pub(crate) segment_id: Option<u32>,
    /// Row `sub_index` value.
    pub(crate) sub_index: Option<u32>,
    /// Row `flip_flag`.
    pub(crate) reference_flip: Option<BinaryFlag>,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSectionReferencePlane {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.plane_entity_id,
                &self.reference_type,
                &self.external_reference_id,
                &self.segment_id,
                &self.sub_index,
                &self.reference_flip,
            ),
            ctx,
            operation,
        )
    }
}

/// Byte-backed gsec3d placement and ordering inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSection3d {
    /// Sketch-plane entity identifier.
    pub(crate) sketch_plane_entity_id: Option<u32>,
    /// Sketch-plane side flag.
    pub(crate) sketch_plane_flip: Option<BinaryFlag>,
    /// Named entity references or complete positional rows in stored order.
    pub(crate) reference_planes: ReferencePlanes,
    /// Geometry identifier joining the reference plane to its datum surface.
    pub(crate) reference_plane_datum_geometry_id: Option<u32>,
    /// Singleton named-record orientation fields.
    pub(crate) orientation: FeatureSectionOrientation,
    /// Stored dimension identifiers in section order.
    pub(crate) dimension_ids: Vec<u32>,
    /// Byte offset of the gsec3d record header in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSection3d {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.sketch_plane_entity_id,
                    &self.sketch_plane_flip,
                    &self.reference_planes,
                    &self.reference_plane_datum_geometry_id,
                    &self.orientation,
                    &self.dimension_ids,
                ),
                (&self.offset,),
            ),
            ctx,
            operation,
        )
    }
}

/// Reference-plane representation selected by the section layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReferencePlanes {
    Named(Vec<u32>),
    Positional(Vec<FeatureSectionReferencePlane>),
}

impl cadmpeg_core::decode::cost::DecodeCost for ReferencePlanes {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Named(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Positional(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
        }
    }
}

impl ReferencePlanes {
    pub(crate) fn entity_ids(&self) -> impl Iterator<Item = u32> + '_ {
        let (named, positional): (&[u32], &[FeatureSectionReferencePlane]) = match self {
            Self::Named(ids) => (ids, &[]),
            Self::Positional(rows) => (&[], rows),
        };
        named
            .iter()
            .copied()
            .chain(positional.iter().map(|row| row.plane_entity_id))
    }
}

/// Interpretation of a stored feature-dimension value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DimensionUnit {
    /// Type `0x0a` angle value stored in radians.
    Radians,
    /// Linear dimension value stored in model millimeters.
    Millimeters,
    /// Dimension type whose unit is defined by its enclosing section schema.
    SchemaDefined,
}

/// One row from a dimension's nested `dim_ref` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureDimensionReference {
    /// Nullable item identifier stored by the reference row.
    pub(crate) item_id: Option<u32>,
    /// Nullable sense selector stored by the reference row.
    pub(crate) sense: Option<u32>,
    /// Nullable two-slot point selector stored by the reference row.
    pub(crate) point: [Option<u32>; 2],
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureDimensionReference {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.item_id, &self.sense, &self.point, &self.offset),
            ctx,
            operation,
        )
    }
}

/// Nested `dim_ref` table carried by a named `dimtab_ptr` prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureDimensionReferenceTable {
    /// Count declared by the nested table's `f8` opener.
    pub(crate) declared_count: u32,
    /// Entity-table class reference following the nested opener.
    pub(crate) entity_ref: Option<u32>,
    /// Named prototype and positional replay rows in stored order.
    pub(crate) rows: Vec<FeatureDimensionReference>,
    /// Byte offset of the `dim_ref` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureDimensionReferenceTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.entity_ref,
                &self.rows,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// Primary dimension scalar state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DimensionValue {
    Resolved(f64),
    UnresolvedToken(Vec<u8>),
    Undefined,
}

impl cadmpeg_core::decode::cost::DecodeCost for DimensionValue {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Resolved(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::UnresolvedToken(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Undefined => {
                cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(&0_u8,), ctx, operation)
            }
        }
    }
}

impl DimensionValue {
    fn decoded(
        ctx: &DecodeContext<'_>,
        value: Option<f64>,
        body: &[u8],
    ) -> Result<Self, CodecError> {
        Ok(match value {
            Some(value) => Self::Resolved(value),
            None => match body {
                [0x00, _, _] | [0x01, _, _, _] => Self::UnresolvedToken(
                    ctx.copy_retained(body, "creo dimension unresolved token")?,
                ),
                _ => Self::Undefined,
            },
        })
    }

    pub(crate) fn resolved(&self) -> Option<f64> {
        match self {
            Self::Resolved(value) => Some(*value),
            Self::UnresolvedToken(_) | Self::Undefined => None,
        }
    }

    pub(crate) fn unresolved_token(&self) -> Option<&[u8]> {
        match self {
            Self::UnresolvedToken(token) => Some(token),
            Self::Resolved(_) | Self::Undefined => None,
        }
    }
}

/// One dimension record from a gsec2d `dimtab_ptr` table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureDimension {
    /// Dimension type discriminator.
    pub(crate) dimension_type: u32,
    /// Decoded primary scalar or unresolved state.
    pub(crate) value: DimensionValue,
    /// Exact encoded scalar body of the primary value.
    pub(crate) value_body: Vec<u8>,
    /// Stored direction byte.
    pub(crate) direction_byte: u8,
    /// Decoded auxiliary scalar, when its prefix is defined.
    pub(crate) auxiliary_value: Option<f64>,
    /// Exact encoded scalar body of the auxiliary value.
    pub(crate) auxiliary_body: Vec<u8>,
    /// External dimension identifier.
    pub(crate) external_id: u32,
    /// Nested named-prototype dimension references, when present.
    pub(crate) references: Option<FeatureDimensionReferenceTable>,
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureDimension {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.dimension_type,
                    &self.value,
                    &self.value_body,
                    &self.direction_byte,
                    &self.auxiliary_value,
                    &self.auxiliary_body,
                ),
                (&self.external_id, &self.references, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

impl FeatureDimension {
    pub(crate) fn unit(&self) -> DimensionUnit {
        dimension_unit(self.dimension_type)
    }
}

/// Dimension table for one gsec2d section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureDimensionTable {
    /// Count declared by the `f8` opener.
    pub(crate) declared_count: u32,
    /// Entity-table class reference following the opener.
    pub(crate) entity_ref: Option<u32>,
    /// Labeled prototype followed by positional replay rows.
    pub(crate) rows: Vec<FeatureDimension>,
    /// Byte offset of the `dimtab_ptr` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureDimensionTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.entity_ref,
                &self.rows,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One positional constraint-relation row from `relat_ptr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureRelation {
    /// Relation identifier from the first positional field.
    pub(crate) relation_id: u32,
    /// Stored `used` field from the second positional field.
    pub(crate) used: u32,
    /// Exact encoded `a`, `b`, and `c` operand-vector block.
    pub(crate) operands: Vec<u8>,
    /// Decoded four-slot `a`, `b`, and `c` operand vectors.
    pub(crate) operand_vectors: Option<[[Option<u32>; 4]; 3]>,
    /// Stored relation sign selector.
    pub(crate) sign: u32,
    /// Stored dimension selector.
    pub(crate) dimension_id: u32,
    /// Stored relation-type discriminator.
    pub(crate) relation_type: u32,
    /// Complete positional fields before the `e2` row terminator.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureRelation {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.relation_id,
                    &self.used,
                    &self.operands,
                    &self.operand_vectors,
                    &self.sign,
                    &self.dimension_id,
                ),
                (&self.relation_type, &self.body, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

/// Counted `relat_ptr` constraint-relation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureRelationTable {
    /// Allocation count declared by the table's `f8` opener. One is the empty
    /// table form; larger counts include two structural entries.
    pub(crate) declared_count: u32,
    /// Relation entity-class reference following the opener.
    pub(crate) entity_ref: Option<u32>,
    /// Complete positional relation rows in stored order.
    pub(crate) rows: Vec<FeatureRelation>,
    /// Section-entity incidence records used by solver equations.
    pub(crate) skamps: Option<SolverSubtable<FeatureSkamp>>,
    /// Joins between relation, equation, and incidence identifiers.
    pub(crate) triples: Option<SolverSubtable<FeatureRelationTriple>>,
    /// Byte offset of the `relat_ptr` label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureRelationTable {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.declared_count,
                &self.entity_ref,
                &self.rows,
                &self.skamps,
                &self.triples,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// A solver table declaration with retained rows, or rows with no decoded declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SolverSubtable<T> {
    Declared {
        header: FeatureSolverTableHeader,
        rows: Vec<T>,
    },
    Unframed(NonEmptySolverRows<T>),
}

impl<T: cadmpeg_core::decode::cost::DecodeCost> cadmpeg_core::decode::cost::DecodeCost
    for SolverSubtable<T>
{
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Declared { header, rows } => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, header, rows),
                ctx,
                operation,
            ),
            Self::Unframed(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
        }
    }
}

/// Retained rows without a decoded table declaration. The collection is nonempty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonEmptySolverRows<T>(Vec<T>);

impl<T: cadmpeg_core::decode::cost::DecodeCost> cadmpeg_core::decode::cost::DecodeCost
    for NonEmptySolverRows<T>
{
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&(&self.0,), ctx, operation)
    }
}

impl<T> SolverSubtable<T> {
    fn from_parts(header: Option<FeatureSolverTableHeader>, rows: Vec<T>) -> Option<Self> {
        match header {
            Some(header) => Some(Self::Declared { header, rows }),
            None if rows.is_empty() => None,
            None => Some(Self::Unframed(NonEmptySolverRows(rows))),
        }
    }

    pub(crate) fn header(&self) -> Option<&FeatureSolverTableHeader> {
        match self {
            Self::Declared { header, .. } => Some(header),
            Self::Unframed(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn header_mut(&mut self) -> Option<&mut FeatureSolverTableHeader> {
        match self {
            Self::Declared { header, .. } => Some(header),
            Self::Unframed(_) => None,
        }
    }

    fn rows(&self) -> &[T] {
        match self {
            Self::Declared { rows, .. } => rows,
            Self::Unframed(rows) => &rows.0,
        }
    }

    #[cfg(test)]
    pub(crate) fn rows_mut(&mut self) -> &mut [T] {
        match self {
            Self::Declared { rows, .. } => rows,
            Self::Unframed(rows) => &mut rows.0,
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Declared { header, rows } => {
                cadmpeg_core::decode::index_from_u32(header.declared_count) == rows.len()
            }
            Self::Unframed(_) => false,
        }
    }

    /// Declared rows that did not decode. Rows decoded past the declaration are
    /// an over-run, not a shortfall, and `is_complete` reports that state.
    pub(crate) fn missing_rows(&self) -> usize {
        match self {
            Self::Declared { header, rows } => {
                let declared = cadmpeg_core::decode::index_from_u32(header.declared_count);
                // The two cases the doc sentence above names: a declaration
                // above the decoded count is the shortfall, and rows decoded
                // past the declaration are an over-run, not a shortfall.
                if declared > rows.len() {
                    declared - rows.len()
                } else {
                    0
                }
            }
            Self::Unframed(_) => 0,
        }
    }
}

/// A solver row with a mutable source offset.
pub(crate) trait HasOffset {
    fn offset_mut(&mut self) -> &mut usize;
}

impl HasOffset for FeatureSkamp {
    fn offset_mut(&mut self) -> &mut usize {
        &mut self.offset
    }
}

impl HasOffset for FeatureRelationTriple {
    fn offset_mut(&mut self) -> &mut usize {
        &mut self.offset
    }
}

impl<T: HasOffset> SolverSubtable<T> {
    /// Rebases the table header and row offsets.
    pub(crate) fn shift_offsets(&mut self, ctx: &DecodeContext<'_>, delta: usize) -> Result<(), CodecError> {
        let rows = match self {
            Self::Declared { header, rows } => {
                header.offset += delta;
                rows
            }
            Self::Unframed(rows) => &mut rows.0,
        };
        for row in ctx.admit_iter(rows, "creo solver offset traversal")? {
            *row.offset_mut() += delta;
        }
        Ok(())
    }
}

impl FeatureRelationTable {
    pub(crate) fn skamps(&self) -> &[FeatureSkamp] {
        self.skamps.as_ref().map_or(&[], SolverSubtable::rows)
    }

    pub(crate) fn triples(&self) -> &[FeatureRelationTriple] {
        self.triples.as_ref().map_or(&[], SolverSubtable::rows)
    }
}

/// Header identity for a counted solver subtable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSolverTableHeader {
    /// Count declared by the table's `f8` opener.
    pub(crate) declared_count: u32,
    /// Table-class reference following the count.
    pub(crate) entity_ref: u32,
    /// Byte offset of the table label or positional array opener.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSolverTableHeader {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.declared_count, &self.entity_ref, &self.offset),
            ctx,
            operation,
        )
    }
}

/// One entity incidence within a section solver `skamp_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSkampItem {
    /// External section-entity identifier.
    pub(crate) entity_id: u32,
    /// Stored endpoint or locus selector.
    pub(crate) sense: u32,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSkampItem {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.entity_id, &self.sense),
            ctx,
            operation,
        )
    }
}

/// One counted section solver `skamp_ptr` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSkamp {
    /// Incidence identifier referenced by `triples_ptr`.
    pub(crate) id: u32,
    /// Stored incidence family.
    pub(crate) kind: u32,
    /// Stored flags.
    pub(crate) flags: u32,
    /// Stored solver status.
    pub(crate) status: u32,
    /// Counted entity incidences in stored order.
    pub(crate) items: Vec<FeatureSkampItem>,
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSkamp {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.id,
                &self.kind,
                &self.flags,
                &self.status,
                &self.items,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One `triples_ptr` join between solver namespaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureRelationTriple {
    /// Relation identifier, or the native null sentinel.
    pub(crate) relation_id: Option<u32>,
    /// Equation identifier, or the native null sentinel.
    pub(crate) equation_id: Option<u32>,
    /// Incidence identifier, or the native null sentinel.
    pub(crate) skamp_id: Option<u32>,
    /// Byte offset of the row in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureRelationTriple {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.relation_id,
                &self.equation_id,
                &self.skamp_id,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One solved line retained in feature-definition section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSavedLine {
    /// Saved-section entity identifier.
    pub(crate) entity_id: u32,
    /// Entity references preceding or embedded in the record.
    pub(crate) references: Vec<u32>,
    /// Five-byte `eb` attribute payloads in stored order.
    pub(crate) attributes: Vec<[u8; 5]>,
    /// Two three-dimensional endpoints in the section sketch frame.
    pub(crate) endpoints: [[Option<f64>; 3]; 2],
    /// Exact row bytes through the final owned token, excluding the structural boundary.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the record preamble in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedLine {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.entity_id,
                &self.references,
                &self.attributes,
                &self.endpoints,
                &self.body,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One solved circular arc retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSavedArc {
    /// Saved-section entity identifier.
    pub(crate) entity_id: u32,
    /// Arc center in the section sketch frame.
    pub(crate) center: [Option<f64>; 3],
    /// Arc radius.
    pub(crate) radius: Option<f64>,
    /// Trimmed arc endpoints in the section sketch frame.
    pub(crate) endpoints: [[Option<f64>; 3]; 2],
    /// Start and end curve parameters.
    pub(crate) parameters: [Option<f64>; 2],
    /// Exact entity-body or positional-row bytes, excluding the structural boundary.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the entity label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedArc {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.entity_id,
                    &self.center,
                    &self.radius,
                    &self.endpoints,
                    &self.parameters,
                    &self.body,
                ),
                (&self.offset,),
            ),
            ctx,
            operation,
        )
    }
}

/// One solved circle retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSavedCircle {
    /// Saved-section entity identifier.
    pub(crate) entity_id: u32,
    /// Circle center in the section sketch frame.
    pub(crate) center: [Option<f64>; 3],
    /// Circle radius.
    pub(crate) radius: Option<f64>,
    /// Exact entity-body bytes, excluding the following entity boundary.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the entity label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedCircle {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.entity_id,
                &self.center,
                &self.radius,
                &self.body,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

/// One solved conic retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSavedConic {
    /// Saved-section entity identifier.
    pub(crate) entity_id: u32,
    /// Two stored endpoint triples.
    pub(crate) endpoints: [[Option<f64>; 3]; 2],
    /// Start and end conic parameters.
    pub(crate) parameters: [Option<f64>; 2],
    /// Semi-axis coefficients.
    pub(crate) coefficients: [Option<f64>; 2],
    /// Two in-plane axes, positive normal, and origin.
    pub(crate) local_system: Option<cadmpeg_ir::units::FiniteVector<12>>,
    /// Exact entity-body bytes, excluding the following entity boundary.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the entity label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedConic {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.entity_id,
                    &self.endpoints,
                    &self.parameters,
                    &self.coefficients,
                    self.local_system
                        .as_ref()
                        .map(cadmpeg_ir::units::FiniteVector::as_raw),
                    &self.body,
                ),
                (&self.offset,),
            ),
            ctx,
            operation,
        )
    }
}

/// A decoded field and its complete encoded value bytes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecodedField<T> {
    pub(crate) value: T,
    pub(crate) body: Vec<u8>,
}

impl<T: cadmpeg_core::decode::cost::DecodeCost> cadmpeg_core::decode::cost::DecodeCost
    for DecodedField<T>
{
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.value, &self.body),
            ctx,
            operation,
        )
    }
}

/// One saved interpolation spline retained in section coordinates.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSavedSpline {
    /// Saved-section entity identifier, when stored.
    pub(crate) entity_id: Option<u32>,
    /// Declared interpolation-point count, when its extent is valid.
    pub(crate) declared_point_count: Option<u32>,
    /// Complete interpolation-point prefix in stored parameter order.
    pub(crate) interpolation_points: Vec<[f64; 3]>,
    /// Exact `i_pnts` value bytes through the last complete interpolation point.
    pub(crate) interpolation_points_body: Vec<u8>,
    /// Complete endpoint tangent triples and `end_tangts` bytes with the array wrapper.
    pub(crate) endpoint_tangents: Option<DecodedField<[[f64; 3]; 2]>>,
    /// Complete interpolation parameters and `params` bytes with the array wrapper.
    pub(crate) parameters: Option<DecodedField<Vec<f64>>>,
    /// Byte offset of the entity label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedSpline {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.entity_id,
                    &self.declared_point_count,
                    &self.interpolation_points,
                    &self.interpolation_points_body,
                    &self.endpoint_tangents,
                    &self.parameters,
                ),
                (&self.offset,),
            ),
            ctx,
            operation,
        )
    }
}

/// One saved placeholder entity without analytic geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeatureSavedDummy {
    /// Saved-section entity identifier, when stored.
    pub(crate) entity_id: Option<u32>,
    /// Exact entity-body bytes, excluding the following entity boundary.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the entity label in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedDummy {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.entity_id, &self.body, &self.offset),
            ctx,
            operation,
        )
    }
}

/// Solved saved-section entity with kind-specific valid fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FeatureSavedEntity {
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

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedEntity {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Line(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Arc(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Circle(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Conic(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Spline(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Dummy(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
        }
    }
}

/// Solved entity table stored below `p_saved_result`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureSavedSection {
    /// Solved entities in stored table order.
    pub(crate) entities: Vec<FeatureSavedEntity>,
    /// Byte offset of the `p_saved_result` record header in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureSavedSection {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.entities, &self.offset),
            ctx,
            operation,
        )
    }
}

/// One byte-bounded feature-definition template or instantiated saved section.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeatureDefinition {
    /// Parsed definition identity and any established canonical owner.
    pub(crate) identity: DefinitionIdentity,
    /// Exact record bytes through the next feature definition or section end.
    pub(crate) body: Vec<u8>,
    /// Definition-space local-system and transform fields.
    pub(crate) parameter_frames: Vec<FeatureParameterFrame>,
    /// Feature-local outline records in history order.
    pub(crate) outlines: Vec<FeatureOutline>,
    /// Section solver-variable table, when present and structurally valid.
    pub(crate) variables: Option<FeatureVariableTable>,
    /// Defining-sketch segment table, when present and structurally valid.
    pub(crate) segments: Option<FeatureSegmentTable>,
    /// Solved/trimmed entity graph, when present and structurally valid.
    pub(crate) trim_entities: Option<FeatureTrimEntityTable>,
    /// Solved trim-vertex adjacency, when present and structurally valid.
    pub(crate) trim_vertices: Option<FeatureTrimVertexTable>,
    /// gsec3d generated-entity ordering, when present and structurally valid.
    pub(crate) order_table: Option<FeatureOrderTable>,
    /// gsec3d placement and ordering inputs, when present.
    pub(crate) section_3d: Option<FeatureSection3d>,
    /// gsec2d dimension table, when present and structurally valid.
    pub(crate) dimensions: Option<FeatureDimensionTable>,
    /// gsec2d constraint-relation table, when present and structurally valid.
    pub(crate) relations: Option<FeatureRelationTable>,
    /// Solved saved-section entities, when present and structurally valid.
    pub(crate) saved_section: Option<FeatureSavedSection>,
    /// Byte offset of the record name in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for FeatureDefinition {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.identity,
                    &self.body,
                    &self.parameter_frames,
                    &self.outlines,
                    &self.variables,
                    &self.segments,
                ),
                (
                    &self.trim_entities,
                    &self.trim_vertices,
                    &self.order_table,
                    &self.section_3d,
                    &self.dimensions,
                    &self.relations,
                ),
                (&self.saved_section, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

/// A position inside one definition's copied body.
pub(crate) struct DefinitionBodyPosition<'a> {
    definition: &'a FeatureDefinition,
    relative: usize,
}

/// A checked position in the source stream.
pub(crate) struct SourcePosition(usize);

impl FeatureDefinition {
    pub(crate) fn body_position(
        &self,
        relative: usize,
    ) -> Result<DefinitionBodyPosition<'_>, CodecError> {
        if relative >= self.body.len() {
            return Err(CodecError::malformed(
                "Creo definition body position is outside its body",
            ));
        }
        Ok(DefinitionBodyPosition {
            definition: self,
            relative,
        })
    }
}

impl DefinitionBodyPosition<'_> {
    pub(crate) fn source(self) -> Result<SourcePosition, CodecError> {
        self.definition
            .offset
            .checked_add(self.relative)
            .map(SourcePosition)
            .ok_or_else(|| CodecError::malformed("Creo definition source position overflows"))
    }
}

impl SourcePosition {
    pub(crate) fn get(self) -> usize {
        self.0
    }
}

/// Definition naming before and after a join selects the owner as its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionIdentity {
    /// A recorded or inherited identifier, with ownership independent of its name.
    Parsed {
        schema_id: Option<NonZeroU32>,
        owner_feature_id: Option<u32>,
    },
    /// A unique join selects the canonical owner for record naming.
    BoundOwner {
        schema_id: Option<NonZeroU32>,
        owner_feature_id: u32,
    },
}

impl cadmpeg_core::decode::cost::DecodeCost for DefinitionIdentity {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Parsed {
                schema_id,
                owner_feature_id,
            } => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(1_u8, &(schema_id, owner_feature_id)),
                ctx,
                operation,
            ),
            Self::BoundOwner {
                schema_id,
                owner_feature_id,
            } => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(1_u8, &(schema_id, owner_feature_id)),
                ctx,
                operation,
            ),
        }
    }
}

impl DefinitionIdentity {
    /// Numeric record identity. Anonymous source definitions retain zero on the wire.
    pub(crate) fn id(self) -> u32 {
        match self {
            Self::Parsed { schema_id, .. } => schema_id.map_or(0, NonZeroU32::get),
            Self::BoundOwner {
                owner_feature_id, ..
            } => owner_feature_id,
        }
    }

    pub(crate) fn schema_id(self) -> Option<NonZeroU32> {
        match self {
            Self::Parsed { schema_id, .. } | Self::BoundOwner { schema_id, .. } => schema_id,
        }
    }

    pub(crate) fn owner_feature_id(self) -> Option<u32> {
        match self {
            Self::Parsed {
                owner_feature_id, ..
            } => owner_feature_id,
            Self::BoundOwner {
                owner_feature_id, ..
            } => Some(owner_feature_id),
        }
    }
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
            // wrapping-exception: DICT prefix remapping reconstructs the low IEEE byte modulo 256
            let second = prefix.wrapping_sub(0x8b);
            (if second >= 0x80 { 0x3f } else { 0x40 }, second)
        };
        return scalar::ieee7_with_prefix(payload, offset, first, second);
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

fn unresolved_variable_guess_end(
    ctx: &DecodeContext<'_>, payload: &[u8], offset: usize, end: usize,
) -> Result<Option<usize>, CodecError> {
    let Some(bytes) = payload.get(offset + 1..end) else { return Ok(None); };
    let Some(relative) = ctx.position_by(bytes, |byte| Ok(*byte == 0xe2), "creo variable guess delimiter")? else { return Ok(None); };
    let delimiter = offset + 1 + relative;
    // Three compact fields consume at most six bytes.
    let mut suffixes = (delimiter.saturating_sub(6).max(offset + 1)..delimiter)
        .filter(|start| variable_row_trailing_fields(payload, *start, delimiter).is_some());
    let Some(first) = suffixes.next() else { return Ok(None); };
    Ok(suffixes.next().is_none().then_some(first))
}

fn decode_variable_scalar(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<(ScalarLane, usize), CodecError> {
    let Some(&prefix) = payload.get(offset).filter(|_| offset < end) else {
        return Ok((ScalarLane::Undefined, offset));
    };
    if matches!(prefix, 0x90 | 0xd7) && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(if prefix == 0x90 {
            &[0x40, 0x05]
        } else {
            &[0xc0, 0x05]
        });
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 7));
    }
    if prefix == 0xd5 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0xbf;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 7));
    }
    if prefix == 0x4f && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 7));
    }
    if matches!(prefix, 0x19 | 0x28 | 0x32 | 0x37 | 0x41) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 8));
    }
    if prefix == 0x31 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 7));
    }
    let variable_dict = match prefix {
        0x51 => Some([0x3f, 0xc6]),
        0x53..=0xa3 => Some((0x3f75_u16 + u16::from(prefix)).to_be_bytes()),
        0xad => Some([0x3f, 0xd9]),
        0xa7..=0xac | 0xae => Some((0xbf2c_u16 + u16::from(prefix)).to_be_bytes()),
        0xb3 => Some([0xbf, 0xe0]),
        0xbd => Some([0xbf, 0xea]),
        0xc3 => Some([0xbf, 0xf0]),
        0xc6..=0xce => Some((0xbf2d_u16 + u16::from(prefix)).to_be_bytes()),
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
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 7));
    }
    if prefix == 0x18
        && payload
            .get(offset + 1)
            .is_some_and(|next| matches!(next, 0x18 | 0xe0 | 0xe2 | 0xe3 | 0x10 | 0xe4 | 0xe6))
    {
        return Ok((ScalarLane::Value(0.0), offset + 1));
    }
    if prefix == 0x18 && unresolved_variable_guess_end(ctx, payload, offset + 1, end)?.is_some() {
        return Ok((ScalarLane::Value(0.0), offset + 1));
    }
    if prefix == 0xed && offset + 9 <= end {
        return Ok((ScalarLane::DimensionDriven, offset + 9));
    }
    Ok(decode_parameter_scalar(payload, offset, end, cache)
        .map_or((ScalarLane::Undefined, offset + 1), |(value, next)| {
            (ScalarLane::Value(value), next)
        }))
}

fn decode_section_coordinate_scalar(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<(ScalarLane, usize), CodecError> {
    match payload.get(offset) {
        Some(0x00 | 0x34) if offset + 3 <= end => return Ok((ScalarLane::Undefined, offset + 3)),
        Some(0x01) if offset + 4 <= end => return Ok((ScalarLane::Undefined, offset + 4)),
        _ => {}
    }
    if payload.get(offset) == Some(&0x2d) && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        // endian-exception: reconstructed-scalar
        return Ok((ScalarLane::Value(f64::from_be_bytes(raw)), offset + 8));
    }
    decode_variable_scalar(ctx, payload, offset, end, cache)
}

fn decode_variable_guess(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<(ScalarLane, usize), CodecError> {
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
            return Ok((ScalarLane::Value(0.0), offset + 1));
        }
    }
    let decoded = decode_section_coordinate_scalar(ctx, payload, offset, end, cache)?;
    if decoded.0 == ScalarLane::Undefined {
        if let Some(next) = unresolved_variable_guess_end(ctx, payload, offset, end)? {
            return Ok((ScalarLane::Undefined, next));
        }
    }
    Ok(decoded)
}

struct VariablePrototype {
    row: FeatureVariableRow,
    value: (usize, usize),
    guess: (usize, usize),
}

fn variable_prototype(ctx: &DecodeContext<'_>, payload: &[u8], cursor: usize, close: usize, cache: &scalar::ScalarCache) -> Result<Option<VariablePrototype>, CodecError> {
    let Some(type_label) = ctx.find_bytes_in(
        payload,
        b"type\0",
        cursor,
        close,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let Some(variable_type) = named_compact_int(ctx, payload, b"type\0", cursor, close)? else {
        return Ok(None);
    };
    let Some(key) = named_compact_int(ctx, payload, b"key\0", cursor, close)? else {
        return Ok(None);
    };
    let Some(value_offset) = ctx.find_bytes_in(
        payload,
        b"value\0",
        cursor,
        close,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let value_label = value_offset + b"value\0".len();
    let (value, value_end) =
        decode_section_coordinate_scalar(ctx, payload, value_label, close, cache)?;
    let Some(guess_offset) = ctx.find_bytes_in(
        payload,
        b"guess\0",
        cursor,
        close,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let guess_label = guess_offset + b"guess\0".len();
    let (guess, guess_end) =
        decode_section_coordinate_scalar(ctx, payload, guess_label, close, cache)?;
    let known = named_compact_int(ctx, payload, b"known\0", cursor, close)?;
    let homogeneity = named_compact_int(ctx, payload, b"homogeneity\0", cursor, close)?;
    let uvar_id = named_compact_int(ctx, payload, b"uvar_id\0", cursor, close)?;
    let Some(offset) = type_label.checked_sub(2) else {
        return Ok(None);
    };
    Ok(Some(VariablePrototype {
        row: FeatureVariableRow {
            variable_type: variable_type.into(),
            key,
            value,
            value_body: Vec::new(),
            guess,
            guess_body: Vec::new(),
            known,
            homogeneity,
            uvar_id,
            // The row header is the two bytes before its type label. A label
            // below offset 2 states a header outside the payload and refuses
            // the row; the label sits at or after the table opener plus eight,
            // so the bound holds for every table this scanner reaches.
            offset,
        },
        value: (value_label, value_end),
        guess: (guess_label, guess_end),
    }))

}

fn variable_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureVariableTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"var_arr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"var_arr\0".len();
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let Some(close) = ctx.find_bytes_in(
        payload,
        &[0xf1, psb::token::ENTITY_REF],
        cursor,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let named_row = variable_prototype(ctx, payload, cursor, close, cache)?;
    let (_, after_close_ref) = psb::compact_int(payload, close + 2);
    cursor = after_close_ref;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = Vec::new();
    if let Some(VariablePrototype { mut row, value: (value_start, value_end), guess: (guess_start, guess_end) }) = named_row {
        row.value_body =
            ctx.copy_retained(&payload[value_start..value_end], "creo variable value body")?;
        row.guess_body =
            ctx.copy_retained(&payload[guess_start..guess_end], "creo variable guess body")?;
        ctx.reserve_vec(&mut rows, 1, "creo variable rows")?;
        rows.push(row);
    }
    // Each row consumes at least one byte, so the declared count cannot admit
    // more rows than the unread bytes in the table window.
    let window = payload.get(cursor..end).map_or(0, <[u8]>::len);
    let max_rows = bounded_len(u64::from(declared_count), 1, window).unwrap_or(window);
    while cursor < end && rows.len() < max_rows {
        ctx.next_charged(&mut (cursor..end), "creo variable row traversal")?;
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
        let (value, next) = decode_section_coordinate_scalar(ctx, payload, cursor, end, cache)?;
        cursor = next;
        let value_end = cursor;
        let guess_start = cursor;
        let (guess, next) = decode_variable_guess(ctx, payload, cursor, end, cache)?;
        cursor = next;
        let guess_end = cursor;
        let mut trailing = [None; 3];
        let mut trailing_count = 0;
        while cursor < end && payload[cursor] != 0xe2 && trailing_count < 3 {
            if payload[cursor] >= 0xc0 {
                break;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            if next == cursor {
                break;
            }
            trailing[trailing_count] = Some(field);
            trailing_count += 1;
            cursor = next;
        }
        let Some(delimiter) = ctx.position_by(&payload[cursor..end], |byte| Ok(*byte == 0xe2), "creo variable row separator")? else {
            break;
        };
        cursor += delimiter + 1;
        let row = FeatureVariableRow {
            variable_type: variable_type.into(),
            key,
            value,
            value_body: ctx
                .copy_retained(&payload[value_start..value_end], "creo variable value body")?,
            guess,
            guess_body: ctx
                .copy_retained(&payload[guess_start..guess_end], "creo variable guess body")?,
            known: trailing[0],
            homogeneity: trailing[1],
            uvar_id: trailing[2],
            offset: row_offset,
        };
        ctx.reserve_vec(&mut rows, 1, "creo variable rows")?;
        rows.push(row);
    }
    Ok(Some(FeatureVariableTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    }))
}

fn positional_variable_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureVariableTable>, CodecError> {
    let candidate_at = |table| {
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
                &payload[after_count + 1..after_reference],
            )
        })
    };
    let mut offsets = start..end;
    let Some((table, declared_count, mut cursor, reference_bytes)) = ctx.find_map(&mut offsets, |table| Ok(candidate_at(table)), "creo positional variable array search")? else { return Ok(None); };
    if ctx.find_map(offsets, |table| Ok(candidate_at(table)), "creo positional variable array uniqueness")?.is_some()
        || payload.get(cursor) != Some(&psb::token::ENTITY_REF) { return Ok(None); }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(None);
    };
    cursor = after_row_class;

    let row_limit = index_from_u32(declared_count);
    let mut rows = Vec::new();
    let prototype_separator_len = 2 + reference_bytes.len() + 1;
    'rows: while cursor < end && rows.len() < row_limit {
        ctx.next_charged(&mut (cursor..end), "creo positional variable row traversal")?;
        let row_offset = cursor;
        let (variable_type, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let (key, next) = psb::compact_int(payload, cursor);
        cursor = next;
        let value_start = cursor;
        let (value, next) = decode_section_coordinate_scalar(ctx, payload, cursor, end, cache)?;
        cursor = next;
        let value_end = cursor;
        let guess_start = cursor;
        let (guess, next) = decode_variable_guess(ctx, payload, cursor, end, cache)?;
        cursor = next;
        let guess_end = cursor;
        let mut trailing = [None; 3];
        let mut trailing_count = 0;
        while cursor < end && payload[cursor] != 0xe2 && trailing_count < 3 {
            if payload[cursor] >= 0xc0 {
                break 'rows;
            }
            let (field, next) = psb::compact_int(payload, cursor);
            if next <= cursor {
                break 'rows;
            }
            trailing[trailing_count] = Some(field);
            trailing_count += 1;
            cursor = next;
        }
        if rows.len() + 1 < row_limit {
            if rows.is_empty() {
                if payload.get(cursor..cursor + 2) != Some(&[0xf1, psb::token::ENTITY_REF])
                    || payload.get(cursor + 2..cursor + 2 + reference_bytes.len())
                        != Some(reference_bytes)
                    || payload.get(cursor + prototype_separator_len - 1) != Some(&0xe2)
                {
                    break;
                }
                cursor += prototype_separator_len;
            } else {
                if payload.get(cursor) != Some(&0xe2) {
                    break;
                }
                cursor += 1;
            }
        }
        let row = FeatureVariableRow {
            variable_type: variable_type.into(),
            key,
            value,
            value_body: ctx
                .copy_retained(&payload[value_start..value_end], "creo variable value body")?,
            guess,
            guess_body: ctx
                .copy_retained(&payload[guess_start..guess_end], "creo variable guess body")?,
            known: trailing[0],
            homogeneity: trailing[1],
            uvar_id: trailing[2],
            offset: row_offset,
        };
        ctx.push_vec(&mut rows, row, "creo variable rows")?;
    }
    Ok(Some(FeatureVariableTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        offset: table,
    }))
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

fn segment_slots(payload: &[u8], offset: &mut usize, count: usize) -> Option<[Option<u32>; 7]> {
    (count <= 7).then_some(())?;
    let mut values = [None; 7];
    let mut filled = 0;
    while filled < count {
        match *payload.get(*offset)? {
            0xe4 => {
                values[filled] = Some(1);
                filled += 1;
                *offset += 1;
            }
            0xe5 => {
                (filled + 2 <= count).then_some(())?;
                values[filled..filled + 2].fill(Some(0));
                filled += 2;
                *offset += 1;
            }
            0xe6 => {
                (filled + 3 <= count).then_some(())?;
                values[filled..filled + 3].fill(Some(0));
                filled += 3;
                *offset += 1;
            }
            0xf6 => {
                filled += 1;
                *offset += 1;
            }
            _ => {
                values[filled] = Some(next_segment_int(payload, offset)?);
                filled += 1;
            }
        }
    }
    Some(values)
}

fn equation_argument_slots(
    payload: &[u8],
    offset: &mut usize,
) -> Option<([Option<u32>; 3], usize)> {
    match *payload.get(*offset)? {
        0xe4 => {
            *offset += 1;
            Some(([Some(1), None, None], 1))
        }
        0xe5 => {
            *offset += 1;
            Some(([Some(0), Some(0), None], 2))
        }
        0xe6 => {
            *offset += 1;
            Some(([Some(0), Some(0), Some(0)], 3))
        }
        0xf6 => {
            *offset += 1;
            Some(([None; 3], 1))
        }
        _ => Some(([Some(next_solver_int(payload, offset)?), None, None], 1)),
    }
}

fn equation_arguments<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &[u8],
    offset: &mut usize,
    end: usize,
    explicit_count: Option<usize>,
) -> Result<Option<(Vec<Option<u32>>, cadmpeg_core::decode::ScopedReservation<'ctx>)>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo equation argument candidates")?;
    let mut arguments = Vec::new();
    while match explicit_count {
        Some(count) => arguments.len() < count,
        None => *offset < end && payload.get(*offset) != Some(&0xf6),
    } {
        ctx.next_charged(&mut (*offset..=end), "creo equation argument traversal")?;
        let before = *offset;
        let Some((slots, slot_count)) = equation_argument_slots(payload, offset) else {
            return Ok(None);
        };
        if *offset <= before
            || *offset > end
            || explicit_count.is_some_and(|count| arguments.len() + slot_count > count)
        {
            return Ok(None);
        }
        storage.with_storage(|| ctx.reserve_vec(&mut arguments, slot_count, "creo equation arguments"))?;
        arguments.extend_from_slice(&slots[..slot_count]);
    }
    if explicit_count.is_some_and(|count| arguments.len() != count) { return Ok(None); }
    Ok(Some((arguments, storage)))
}

/// Decode the structurally framed `eqtn_arr` solver table in one bounded
/// feature definition.
pub(crate) fn equation_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureEquationTable>, CodecError> {
    if start > end || end > payload.len() {
        return Ok(None);
    }
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"eqtn_arr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"eqtn_arr\0".len();
    if payload.get(cursor) == Some(&0xf2) {
        cursor += 1;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let Some((declared_count, after_count)) = next_bounded_compact_int(payload, cursor + 1) else {
        return Ok(None);
    };
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let Ok((entity_ref, next)) = psb::reference_id(payload, cursor + 1) else {
            return Ok(None);
        };
        cursor = next;
        Some(entity_ref)
    } else {
        None
    };
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Ok(None);
    }
    cursor += 2;
    let mut rows_end = end;
    for label in [
        b"\xe0\x02scale\0".as_slice(),
        b"\xe0\x02scales\0",
        b"\xe0\x02guesses\0",
    ] {
        if let Some(offset) = ctx.find_bytes_in(
            payload,
            label,
            cursor,
            end,
            "find Creo feature definition field",
        )? {
            rows_end = rows_end.min(offset);
        }
    }
    let prototype_start = cursor;
    let Some(prototype_reference) = ctx.find_bytes_in(
        payload,
        &[0xf1, psb::token::ENTITY_REF],
        prototype_start,
        rows_end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let Ok((_, after_prototype_reference)) = psb::reference_id(payload, prototype_reference + 2)
    else {
        return Ok(None);
    };
    if payload.get(after_prototype_reference) != Some(&0xe2) {
        return Ok(None);
    }
    let prototype_end = after_prototype_reference + 1;
    cursor = prototype_end;

    let mut prototypes = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "creo equation prototypes")?;
    while cursor < rows_end {
        ctx.next_charged(&mut (cursor..rows_end), "creo equation row traversal")?;
        let row_start = cursor;
        let Some(equation_id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let Some(function_id) = next_solver_int(payload, &mut cursor) else {
            break;
        };
        let explicit_argument_count = if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let Some((count, next)) = next_bounded_compact_int(payload, cursor + 1) else {
                return Ok(None);
            };
            cursor = next;
            Some(count)
        } else {
            None
        };
        let arguments_start = cursor;
        let explicit_argument_count_usize = match explicit_argument_count {
            Some(count) => {
                let Some(count) = usize::try_from(count).ok() else {
                    return Ok(None);
                };
                Some(count)
            }
            None => None,
        };
        let Some((arguments, argument_storage)) = equation_arguments(
            ctx,
            payload,
            &mut cursor,
            rows_end,
            explicit_argument_count_usize,
        )?
        else {
            break;
        };
        let arguments_body_end = cursor;
        let auxiliary_start = cursor;
        if payload.get(cursor) != Some(&0xf6) {
            break;
        }
        cursor += 1;
        let row_end = if payload.get(cursor) == Some(&0xe2) {
            cursor += 1;
            cursor
        } else if cursor == rows_end
            || payload.get(cursor..cursor + 2) == Some(&[0xf2, psb::token::ENTITY_REF])
        {
            cursor
        } else {
            break;
        };
        storage.with_storage(|| ctx.push_vec(&mut prototypes, EquationPrototype {
            equation_id, function_id, explicit_argument_count, arguments, argument_storage,
            arguments_start, arguments_end: arguments_body_end, auxiliary_start,
            row_start, row_end,
        }, "creo equation prototypes"))?;
    }

    let prototype_body = ctx.copy_retained(&payload[prototype_start..prototype_end], "creo equation prototype body")?;
    let mut rows = Vec::new();
    ctx.reserve_vec(&mut rows, prototypes.len(), "creo equation rows")?;
    for prototype in ctx.admit_iter(prototypes, "creo equation materialization")? {
        prototype.argument_storage.commit()?;
        rows.push(FeatureEquation {
            equation_id: prototype.equation_id,
            function_id: prototype.function_id,
            explicit_argument_count: prototype.explicit_argument_count,
            arguments: prototype.arguments,
            arguments_body: ctx.copy_retained(&payload[prototype.arguments_start..prototype.arguments_end], "creo equation argument body")?,
            auxiliary_body: ctx.copy_retained(&payload[prototype.auxiliary_start..=prototype.auxiliary_start], "creo equation auxiliary body")?,
            body: ctx.copy_retained(&payload[prototype.row_start..prototype.row_end], "creo equation row body")?,
            offset: prototype.row_start,
        });
    }
    Ok(Some(FeatureEquationTable {
        declared_count,
        entity_ref,
        prototype_body,
        rows,
        offset: table,
    }))
}


struct EquationPrototype<'ctx> {
    equation_id: u32,
    function_id: u32,
    explicit_argument_count: Option<u32>,
    arguments: Vec<Option<u32>>,
    argument_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    arguments_start: usize,
    arguments_end: usize,
    auxiliary_start: usize,
    row_start: usize,
    row_end: usize,
}

/// Decode instantiated placement-instruction rows from one bounded feature
/// definition.
pub(crate) fn placement_instructions<'a>(ctx: &DecodeContext<'_>, definition: &'a FeatureDefinition) -> Result<PlacementInstructions<'a>, CodecError> {
    placement_instruction_rows(ctx, &definition.body, definition.offset)
}

fn placement_instruction_rows<'a>(ctx: &DecodeContext<'_>, payload: &'a [u8], definition_offset: usize) -> Result<PlacementInstructions<'a>, CodecError> {
    let table_class = named_array_class(ctx, payload, b"place_instruction_ptrs\0", 0, payload.len())?;
    let end = if table_class.is_some() { payload.len() } else { 0 };
    Ok(PlacementInstructions { payload, definition_offset, table_class, markers: 0..end })
}

/// A position-preserving reader for instantiated placement rows.
pub(crate) struct PlacementInstructions<'a> {
    payload: &'a [u8],
    definition_offset: usize,
    table_class: Option<u32>,
    markers: std::ops::Range<usize>,
}

impl PlacementInstructions<'_> {
    pub(crate) fn next(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<FeaturePlacementInstruction>, CodecError> {
        let Some(table_class) = self.table_class else { return Ok(None); };
        ctx.find_map(&mut self.markers, |marker| Ok(placement_instruction_at(self.payload, self.definition_offset, table_class, marker)), "creo placement instruction byte traversal")
    }

    pub(crate) fn count(mut self, ctx: &DecodeContext<'_>) -> Result<usize, CodecError> {
        let mut count = 0;
        while self.next(ctx)?.is_some() { count += 1; }
        Ok(count)
    }

    #[cfg(test)]
    fn collect(mut self, ctx: &DecodeContext<'_>) -> Result<Vec<FeaturePlacementInstruction>, CodecError> {
        let mut rows = Vec::new();
        while let Some(row) = self.next(ctx)? { ctx.push_vec(&mut rows, row, "creo placement instruction test rows")?; }
        Ok(rows)
    }
}

fn placement_instruction_at(payload: &[u8], definition_offset: usize, table_class: u32, marker: usize) -> Option<FeaturePlacementInstruction> {
        if payload.get(marker..marker + 2) != Some(&[0xf1, psb::token::ENTITY_REF]) {
            return None;
        }
        let Ok((class, after_class)) = psb::reference_id(payload, marker + 2) else {
            return None;
        };
        if class != table_class || payload.get(after_class) != Some(&psb::token::COMPOUND_CLOSE) {
            return None;
        }
        let mut cursor = after_class + 1;
        let kind = next_solver_int(payload, &mut cursor)?;
        let zero_offset = payload.get(cursor) == Some(&0x18);
        if !zero_offset {
            return None;
        }
        cursor += 1;
        let Ok(dimension_id) = next_nullable_segment_int(payload, &mut cursor) else {
            return None;
        };
        let Ok(reference_id) = next_nullable_segment_int(payload, &mut cursor) else {
            return None;
        };
        let Ok(geometry1_id) = next_nullable_segment_int(payload, &mut cursor) else {
            return None;
        };
        let Ok(geometry2_id) = next_nullable_segment_int(payload, &mut cursor) else {
            return None;
        };
        let member1 = next_segment_int(payload, &mut cursor)?;
        let member2 = next_segment_int(payload, &mut cursor)?;
        Some(FeaturePlacementInstruction {
            kind,
            zero_offset,
            dimension_id,
            reference_id,
            geometry1_id,
            geometry2_id,
            member1,
            member2,
            offset: definition_offset + marker,
        })
}

fn segment_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureSegmentTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"segtab_ptr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"segtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo segment table cursor traversal")?;
        cursor += 1;
    }
    segment_table_body(ctx, payload, table, cursor, end, PrototypeRow::Present)
}

fn positional_segment_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureSegmentTable>, CodecError> {
    const NAME_WINDOW: usize = 256;
    let name_search_end = start
        .checked_add(NAME_WINDOW)
        .map_or(end, |window_end| window_end.min(end));
    let Some(name_end) = ctx.find_bytes_in(
        payload,
        b"S2D",
        start,
        name_search_end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let Some(nul) = ctx.position_by(&payload[name_end..end], |byte| Ok(*byte == 0), "creo positional segment name terminator")? else {
        return Ok(None);
    };
    let cursor = nul + name_end + 1;
    segment_table_body(ctx, payload, cursor, cursor, end, PrototypeRow::Elided)
}

/// Whether a segment table's declared count includes an elided prototype row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrototypeRow {
    /// The first declared row is elided from the body.
    Elided,
    /// Every declared row is present in the body.
    Present,
}

fn named_segment_values(ctx: &DecodeContext<'_>, payload: &[u8], cursor: usize, close: usize, label: &[u8], count: usize) -> Result<Option<(usize, [Option<u32>; 7])>, CodecError> {
        let Some(offset) = ctx.find_bytes_in(
            payload,
            label,
            cursor,
            close,
            "find Creo feature definition field",
        )?
        else {
            return Ok(None);
        };

            let mut p = offset + label.len();
            if payload.get(p) == Some(&psb::token::ARRAY_OPEN) {
                let (declared, next) = psb::compact_int(payload, p + 1);
                if usize::try_from(declared).ok() != Some(count) { return Ok(None); }
                p = next;
            }
            if label == b"type\0" && payload.get(p..p + 2) == Some(&[0xc0, 0x80]) {
                p += 2;
            }
            let Some(values) = segment_slots(payload, &mut p, count) else { return Ok(None); };
            Ok(Some((offset, values)))
}

fn named_segment_prototype(ctx: &DecodeContext<'_>, payload: &[u8], cursor: usize, close: usize) -> Result<Option<FeatureOpaqueSegment>, CodecError> {
        let Some((offset, kind)) = named_segment_values(ctx, payload, cursor, close, b"type\0", 1)? else {
            return Ok(None);
        };
        let Some((_, directions)) = named_segment_values(ctx, payload, cursor, close, b"dir\0", 3)? else {
            return Ok(None);
        };
        let Some((_, point_ids)) = named_segment_values(ctx, payload, cursor, close, b"pointid\0", 2)? else {
            return Ok(None);
        };
        let Some((_, center_id)) = named_segment_values(ctx, payload, cursor, close, b"cntrid\0", 1)? else {
            return Ok(None);
        };
        let Some((_, arc_orientation)) = named_segment_values(ctx, payload, cursor, close, b"arcorient\0", 1)? else {
            return Ok(None);
        };
        let Some((_, vertical_horizontal)) = named_segment_values(ctx, payload, cursor, close, b"verhor\0", 1)? else {
            return Ok(None);
        };
        let Some((_, radius_ref)) = named_segment_values(ctx, payload, cursor, close, b"radius\0", 1)? else {
            return Ok(None);
        };
        let Some((_, radius2_ref)) = named_segment_values(ctx, payload, cursor, close, b"radius2\0", 1)? else {
            return Ok(None);
        };
        let Some((_, external_id)) = named_segment_values(ctx, payload, cursor, close, b"ext_id\0", 1)? else {
            return Ok(None);
        };
        let Some(kind) = kind[0] else {
            return Ok(None);
        };
        let Some(external_id) = external_id[0] else {
            return Ok(None);
        };
        Ok(Some(FeatureOpaqueSegment {
            kind,
            directions: [directions[0], directions[1], directions[2]],
            point_ids: [point_ids[0], point_ids[1]],
            center_id: center_id[0],
            arc_orientation: arc_orientation[0],
            vertical_horizontal: vertical_horizontal[0],
            radius_ref: radius_ref[0],
            radius2_ref: radius2_ref[0],
            external_id,
            body: Vec::new(),
            offset,
        }))
}

fn segment_table_body(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    table: usize,
    mut cursor: usize,
    end: usize,
    prototype_row: PrototypeRow,
) -> Result<Option<FeatureSegmentTable>, CodecError> {
    let has_elided_prototype = prototype_row == PrototypeRow::Elided;
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (value, next) = psb::compact_int(payload, cursor + 1);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let Some((close, after_close_ref)) = ctx.find_map(cursor..end, |offset| {
        if payload.get(offset..offset + 2) != Some(&[0xf2, psb::token::ENTITY_REF]) { return Ok(None); }
        let Ok((class, after_reference)) = psb::reference_id(payload, offset + 2) else { return Ok(None); };
        Ok((entity_ref.is_none_or(|expected| class == expected) && payload.get(after_reference) == Some(&0xe2)).then_some((offset, after_reference)))
    }, "creo segment prototype close search")? else { return Ok(None); };
    let named_row = named_segment_prototype(ctx, payload, cursor, close)?;
    cursor = after_close_ref + 1;
    let mut region_end = end;
    for label in [
        b"order_table".as_slice(),
        b"dimtab_ptr\0",
        b"relat_ptr\0",
        b"var_arr\0",
        b"gsec3d_ptr\0",
        b"order_ptr\0",
        b"p_saved_result\0",
        b"S2D",
    ] {
        if let Some(offset) = ctx.find_bytes_in(
            payload,
            label,
            cursor,
            end,
            "find Creo feature definition field",
        )? {
            region_end = region_end.min(offset);
        }
    }
    // The declared count of an elided-prototype table counts the prototype row
    // that the body does not carry. A declared count below that one row states
    // a body-row count the table cannot hold, and refuses the table.
    let declared_body_rows = match prototype_row {
        PrototypeRow::Elided => {
            let Some(count) = declared_count.checked_sub(1) else {
                return Ok(None);
            };
            count
        }
        PrototypeRow::Present => declared_count,
    };
    let Ok(row_limit) = usize::try_from(declared_body_rows) else {
        return Ok(None);
    };
    let mut rows = Vec::new();
    if let Some(row) = named_row.and_then(typed_segment_row) {
        ctx.reserve_vec(&mut rows, 1, "creo segment rows")?;
        rows.push(row);
    }
    let first_row = cursor;
    while cursor < region_end && rows.len() < row_limit {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo segment table body cursor traversal")?;
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
                && !matches!(preceding_byte(payload, row_start), Some(0xe2 | 0xe3)))
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
        let Some(prefix) = segment_slots(payload, &mut p, 7) else {
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
        let Some(suffix) = segment_slots(payload, &mut p, 3) else {
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
            if let Some(mut row) = typed_segment_row(FeatureOpaqueSegment {
                kind,
                directions,
                point_ids: [point0, point1],
                center_id,
                arc_orientation,
                vertical_horizontal,
                radius_ref,
                radius2_ref,
                external_id,
                body: Vec::new(),
                offset: row_start,
            }) {
                match &mut row {
                    SegmentRow::Opaque(opaque) => {
                        opaque.body =
                            ctx.copy_retained(&payload[row_start..=p], "creo segment row body")?;
                    }
                    SegmentRow::Ordinary(segment) => {
                        segment.body =
                            ctx.copy_retained(&payload[row_start..=p], "creo segment row body")?;
                    }
                    _ => {}
                }
                ctx.reserve_vec(&mut rows, 1, "creo segment rows")?;
                rows.push(row);
            }
            cursor = p + 1;
        } else {
            cursor += 1;
        }
    }
    Ok(Some(FeatureSegmentTable {
        declared_count,
        has_elided_prototype,
        entity_ref,
        rows: SegmentRows::from_parsed_rows(ctx, rows)?,
        offset: table,
    }))
}

fn typed_segment_row(row: FeatureOpaqueSegment) -> Option<SegmentRow> {
    if row.kind == 10
        && row.directions == [Some(0); 3]
        && row.point_ids == [None, Some(1)]
        && row.arc_orientation == Some(0)
        && row.vertical_horizontal == Some(0)
        && row.radius2_ref.is_none()
    {
        if let (Some(center_id), Some(radius_ref)) = (row.center_id, row.radius_ref) {
            return Some(SegmentRow::Circle(FeatureCircleSegment {
                center_id,
                radius_ref,
                external_id: row.external_id,
                offset: row.offset,
            }));
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
            return Some(SegmentRow::Point(FeaturePointSegment {
                point_id,
                external_id: row.external_id,
                offset: row.offset,
            }));
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
            return Some(SegmentRow::CenteredLine(FeatureCenteredLineSegment {
                center_id,
                external_id: row.external_id,
                offset: row.offset,
            }));
        }
    }
    if row.kind == 25
        && row.center_id.is_none()
        && row.arc_orientation == Some(0)
        && row.radius_ref.is_none()
        && row.radius2_ref.is_none()
    {
        return Some(SegmentRow::ReferenceLine(FeatureReferenceLineSegment {
            directions: row.directions,
            point_ids: row.point_ids,
            vertical_horizontal: row.vertical_horizontal,
            external_id: row.external_id,
            offset: row.offset,
        }));
    }
    if row.kind == 12 {
        if let [Some(first), Some(second)] = row.point_ids {
            return Some(SegmentRow::BoundedCurve(FeatureBoundedCurveSegment {
                directions: row.directions,
                point_ids: [first, second],
                center_id: row.center_id,
                arc_orientation: row.arc_orientation,
                vertical_horizontal: row.vertical_horizontal,
                radius_ref: row.radius_ref,
                radius2_ref: row.radius2_ref,
                external_id: row.external_id,
                offset: row.offset,
            }));
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
            return Some(SegmentRow::Conic(FeatureConicSegment {
                center_id,
                first_coefficient_ref,
                second_coefficient_ref,
                external_id: row.external_id,
                offset: row.offset,
            }));
        }
    }
    if !matches!(row.kind, 2 | 3 | 5) {
        return Some(SegmentRow::Opaque(row));
    }
    let point0 = row.point_ids[0]?;
    let kind = if row.kind == 5 {
        FeatureSegmentKind::Point(point0)
    } else {
        let point1 = row.point_ids[1]?;
        if row.kind == 2 {
            FeatureSegmentKind::Line([point0, point1])
        } else {
            FeatureSegmentKind::Arc([point0, point1])
        }
    };
    Some(SegmentRow::Ordinary(FeatureSegment {
        kind,
        directions: row.directions,
        center_id: row.center_id,
        arc_orientation: row.arc_orientation,
        vertical_horizontal: row.vertical_horizontal,
        radius_ref: row.radius_ref,
        radius2_ref: row.radius2_ref,
        external_id: row.external_id,
        body: row.body,
        offset: row.offset,
    }))
}

fn order_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureOrderTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"order_table\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"order_table\0".len();
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let Ok((value, next)) = psb::reference_id(payload, cursor + 1) else {
            return Ok(None);
        };
        cursor = next;
        Some(value)
    } else {
        None
    };
    let Some(close) = ctx.find_bytes_in(
        payload,
        &[0xf1, psb::token::ENTITY_REF],
        cursor,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut prototype = true;
    let mut field = cursor;
    for label in [b"ext_id\0".as_slice(), b"int_id\0", b"bitmask\0"] {
        let Some(offset) = ctx.find_bytes_in(payload, label, field, close, "find Creo feature definition field")? else { prototype = false; break; };
        let (_, next) = segment_int(payload, offset + label.len());
        if next <= offset + label.len() || next > close { prototype = false; break; }
        field = next;
    }
    let Ok((_, next)) = psb::reference_id(payload, close + 2) else {
        return Ok(None);
    };
    cursor = next;
    if payload.get(cursor) == Some(&0xe2) {
        cursor += 1;
    }
    let mut rows = super::order_rows::OrderRows::default();
    // A decoded prototype row is one of the declared rows. A declared count of
    // zero with the prototype row present states a body-row count the table
    // cannot hold, and refuses the table.
    let declared_body_rows = if prototype {
        let Some(count) = declared_count.checked_sub(1) else {
            return Ok(None);
        };
        count
    } else {
        declared_count
    };
    let Ok(row_limit) = usize::try_from(declared_body_rows) else {
        return Ok(None);
    };
    while cursor < end && rows.len() < row_limit {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo order table cursor traversal")?;
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
        if !row_separator && !table_boundary {
            break;
        }
        if !rows.push_unique(
            ctx,
            FeatureOrderRow {
                external_id,
                internal_id,
                bitmask,
                offset: row_offset,
            },
        )? {
            break;
        }
        if !row_separator {
            break;
        }
        cursor = next + 1;
    }
    Ok(Some(FeatureOrderTable {
        declared_count,
        has_prototype: prototype,
        entity_ref,
        rows,
        offset: table,
    }))
}

fn positional_order_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Result<Option<FeatureOrderTable>, CodecError> {
    let Some((table, declared_count, cursor, _)) = first_positional_array(ctx, payload, start, end, table_class)? else { return Ok(None); };
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
    // The declared count of a positional order table counts its prototype row.
    // A declared count of zero with the prototype row present states a body-row
    // count the table cannot hold, and refuses the table. Each row consumes at
    // least one byte before its 0xe2 separator, so the row count cannot exceed
    // the unread bytes in the table window.
    let row_limit = match prototype.filter(|&rows_start| rows_start < end) {
        Some(_) => {
            let Some(declared_body_rows) = declared_count.checked_sub(1) else {
                return Ok(None);
            };
            let Ok(row_limit) = usize::try_from(declared_body_rows) else {
                return Ok(None);
            };
            row_limit
        }
        None => 0,
    };
    let mut rows = super::order_rows::OrderRows::default();
    let mut cursor = prototype.unwrap_or(end);
    while cursor < end && rows.len() < row_limit {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo positional order table cursor traversal")?;
        let row_offset = cursor;
        let (external_id, next) = segment_int(payload, cursor);
        let (internal_id, next) = segment_int(payload, next);
        let (bitmask, next) = segment_int(payload, next);
        let (Some(external_id), Some(internal_id), Some(bitmask)) =
            (external_id, internal_id, bitmask)
        else {
            break;
        };
        let row = FeatureOrderRow {
            external_id,
            internal_id,
            bitmask,
            offset: row_offset,
        };
        cursor = next;
        let last = rows.len() + 1 == row_limit;
        if !last && payload.get(cursor) != Some(&0xe2) {
            break;
        }
        if !rows.push_unique(ctx, row)? || last {
            break;
        }
        cursor += 1;
    }
    Ok(Some(FeatureOrderTable {
        declared_count,
        has_prototype: prototype.is_some(),
        entity_ref: Some(table_class),
        rows,
        offset: table,
    }))
}

fn named_compact_int(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<u32>, CodecError> {
    let Some(offset) = ctx.find_bytes_in(payload, label, start, end, "find Creo named integer")?
    else {
        return Ok(None);
    };
    let at = offset + label.len();
    let (value, next) = segment_int(payload, at);
    Ok(value.filter(|_| next <= end))
}

fn gsec3d_plane_id(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<u32>, CodecError> {
    let label = b"plane_id\0";
    let reference_planes = ctx
        .find_bytes_in(
            payload,
            b"\xe0\x00ref_planes\0",
            start,
            end,
            "find Creo feature definition field",
        )?
        .unwrap_or(end);
    let mut cursor = start;
    while let Some(at) = ctx.find_bytes_in(
        payload,
        label,
        cursor,
        reference_planes,
        "find Creo feature definition field",
    )? {
        cursor = at + label.len();
        let (value, next) = segment_int(payload, cursor);
        if next <= reference_planes && value.is_some() {
            return Ok(value);
        }
    }
    cursor = reference_planes;
    while let Some(at) = ctx.find_bytes_in(
        payload,
        label,
        cursor,
        end,
        "find Creo feature definition field",
    )? {
        cursor = at + label.len();
        if at
            .checked_sub(2)
            .and_then(|header| payload.get(header..at))
            .is_some_and(|header| header == [psb::token::NAMED_RECORD, 1].as_slice())
        {
            continue;
        }
        let (value, next) = segment_int(payload, cursor);
        if next <= end && value.is_some() {
            return Ok(value);
        }
    }
    Ok(None)
}

fn section_3d(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureSection3d>, CodecError> {
    const GSEC3D: &[u8] = b"\xe0\x00gsec3d_ptr\0";
    const SAVED_RESULT: &[u8] = b"\xe0\x00p_saved_result\0";
    let Some(section) = ctx.find_bytes_in(
        payload,
        GSEC3D,
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let record_end = ctx
        .find_bytes_in(
            payload,
            GSEC3D,
            section + GSEC3D.len(),
            end,
            "find Creo feature definition field",
        )?
        .unwrap_or(end);
    let placement_end = ctx
        .find_bytes_in(
            payload,
            SAVED_RESULT,
            section,
            record_end,
            "find Creo feature definition field",
        )?
        .unwrap_or(record_end);
    let sketch_plane_entity_id = gsec3d_plane_id(ctx, payload, section, placement_end)?;
    let sketch_plane_flip = ctx
        .find_bytes_in(
            payload,
            b"plane_flip\0",
            section,
            placement_end,
            "find Creo feature definition field",
        )?
        .and_then(|at| payload.get(at + b"plane_flip\0".len()).copied())
        .and_then(BinaryFlag::decode);

    let mut reference_plane_entity_ids = Vec::new();
    let mut reference_plane_datum_geometry_id = None;
    if let Some(references) = ctx.find_bytes_in(
        payload,
        b"\xe0\x00ref_planes\0",
        section,
        placement_end,
        "find Creo feature definition field",
    )? {
        let mut cursor = references + b"\xe0\x00ref_planes\0".len();
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            let mut counted_items = 0..count;

                while let Some(_) = ctx.next_charged(&mut counted_items, "creo section 3d traversal")? {if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
                    break;
                }
                let Ok((entity_id, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                ctx.reserve_vec(
                    &mut reference_plane_entity_ids,
                    1,
                    "creo named section reference planes",
                )?;
                reference_plane_entity_ids.push(entity_id);
                cursor = next;
            }
            let nested_end = placement_end;
            reference_plane_datum_geometry_id =
                named_compact_int(ctx, payload, b"\xe0\x01plane_id\0", cursor, nested_end)?;
        }
    }

    let named_flag = |label: &[u8]| -> Result<Option<BinaryFlag>, CodecError> {
        Ok(ctx
            .find_bytes_in(
                payload,
                label,
                section,
                placement_end,
                "find Creo feature definition field",
            )?
            .and_then(|at| payload.get(at + label.len()).copied())
            .and_then(BinaryFlag::decode))
    };
    let orientation = FeatureSectionOrientation {
        section_flip: named_flag(b"\xe0\x01flip\0")?,
        reference_type: named_compact_int(
            ctx,
            payload,
            b"\xe0\x01ref_type\0",
            section,
            placement_end,
        )?,
        segment_id: named_compact_int(ctx, payload, b"\xe0\x01seg_id\0", section, placement_end)?,
        reference_flip: named_flag(b"\xe0\x01flip_flag\0")?,
    };

    let mut dimension_ids = Vec::new();
    if let Some(table) = ctx.find_bytes_in(
        payload,
        b"dim_id_tab\0",
        section,
        end,
        "find Creo feature definition field",
    )? {
        let mut cursor = table + b"dim_id_tab\0".len();
        while payload
            .get(cursor)
            .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
        {
            ctx.next_charged(&mut (cursor..=payload.len()), "creo section 3d cursor traversal")?;
            cursor += 1;
        }
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            let (count, next) = psb::compact_int(payload, cursor + 1);
            cursor = next;
            let mut counted_items = 0..count;

                while let Some(_) = ctx.next_charged(&mut counted_items, "creo section 3d traversal")? {let (Some(value), next) = segment_int(payload, cursor) else {
                    break;
                };
                ctx.reserve_vec(&mut dimension_ids, 1, "creo section dimension IDs")?;
                dimension_ids.push(value);
                cursor = next;
            }
        }
    }
    Ok(Some(FeatureSection3d {
        sketch_plane_entity_id,
        sketch_plane_flip,
        reference_planes: ReferencePlanes::Named(reference_plane_entity_ids),
        reference_plane_datum_geometry_id,
        orientation,
        dimension_ids,
        offset: section,
    }))
}

fn positional_section_3d(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureSection3d>, CodecError> {
    let Some((section, name_end)) = ctx.find_map(start..end.saturating_sub(3), |section| {
        if payload.get(section..section + 4) != Some(b"\x07S2D") { return Ok(None); }
        Ok(ctx.position_by(&payload[section + 1..end], |byte| Ok(*byte == 0), "creo positional section name terminator")?.map(|relative| (section, section + relative + 1)))
    }, "creo positional section search")? else { return Ok(None); };
    let mut result = FeatureSection3d {
        sketch_plane_entity_id: None,
        sketch_plane_flip: None,
        reference_planes: ReferencePlanes::Positional(Vec::new()),
        reference_plane_datum_geometry_id: None,
        orientation: FeatureSectionOrientation::default(),
        dimension_ids: Vec::new(),
        offset: section,
    };
    let mut cursor = name_end + 1;
    let Some(section_flip) = payload.get(cursor).copied() else {
        return Ok(Some(result));
    };
    result.orientation.section_flip = BinaryFlag::decode(section_flip);
    cursor += 1;
    for _ in 0..3 {
        let (_, next) = segment_int(payload, cursor);
        if next <= cursor {
            return Ok(Some(result));
        }
        cursor = next;
    }
    let (sketch_plane_entity_id, next) = segment_int(payload, cursor);
    if next <= cursor {
        return Ok(Some(result));
    }
    result.sketch_plane_entity_id = sketch_plane_entity_id;
    cursor = next;
    let Some(sketch_plane_flip) = payload.get(cursor).copied() else {
        return Ok(Some(result));
    };
    result.sketch_plane_flip = BinaryFlag::decode(sketch_plane_flip);
    cursor += 1;
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(Some(result));
    }
    let (reference_count, next) = psb::compact_int(payload, cursor + 1);
    if next <= cursor + 1 {
        return Ok(Some(result));
    }
    cursor = next;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Ok(Some(result));
    }
    let table_reference_start = cursor + 1;
    let Ok((_, next)) = psb::reference_id(payload, table_reference_start) else {
        return Ok(Some(result));
    };
    let table_reference = &payload[table_reference_start - 1..next];
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[0xfb, 0xe2]) {
        return Ok(Some(result));
    }
    cursor += 2;
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Ok(Some(result));
    }
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(Some(result));
    };
    cursor = next;

    let row_count = index_from_u32(reference_count);
    let mut reference_plane_rows = Vec::new();
    let mut counted_items = 0..row_count;

        while let Some(row) = ctx.next_charged(&mut counted_items, "creo positional section 3d traversal")? {let (Some(plane_id), next) = segment_int(payload, cursor) else {
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
        ctx.reserve_vec(
            &mut reference_plane_rows,
            1,
            "creo positional section reference planes",
        )?;
        reference_plane_rows.push(FeatureSectionReferencePlane {
            plane_entity_id: plane_id,
            reference_type,
            external_reference_id,
            segment_id,
            sub_index,
            reference_flip,
        });
        if row + 1 < row_count {
            let Some(separator_at) = find_class_close(ctx, payload, cursor, end, 0xf2, table_reference)?
            else {
                break;
            };
            cursor = separator_at + table_reference.len() + 2;
        }
    }
    result.reference_planes = ReferencePlanes::Positional(reference_plane_rows);
    Ok(Some(result))
}

pub(super) fn dimension_unit(dimension_type: u32) -> DimensionUnit {
    match dimension_type {
        0x0a => DimensionUnit::Radians,
        0x01..=0x05 => DimensionUnit::Millimeters,
        _ => DimensionUnit::SchemaDefined,
    }
}

fn named_dimension_reference(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<(FeatureDimensionReference, usize)>, CodecError> {
    let Some(item_label) = ctx.find_bytes_in(
        payload,
        b"item_id\0",
        start,
        end,
        "find Creo dimension item",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = item_label + b"item_id\0".len();
    let Ok(item_id) = next_nullable_segment_int(payload, &mut cursor) else {
        return Ok(None);
    };
    let Some(sense_label) = ctx.find_bytes_in(
        payload,
        b"sense\0",
        cursor,
        end,
        "find Creo dimension sense",
    )?
    else {
        return Ok(None);
    };
    cursor = sense_label + b"sense\0".len();
    let Ok(sense) = next_nullable_segment_int(payload, &mut cursor) else {
        return Ok(None);
    };
    let Some(point_label) = ctx.find_bytes_in(
        payload,
        b"point\0",
        cursor,
        end,
        "find Creo dimension point",
    )?
    else {
        return Ok(None);
    };
    cursor = point_label + b"point\0".len();
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) { return Ok(None); }
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    if declared_count != 2 { return Ok(None); }
    cursor = after_count;
    let Some(point) = segment_slots(payload, &mut cursor, 2) else { return Ok(None); };
    Ok(Some((FeatureDimensionReference { item_id, sense, point: [point[0], point[1]], offset: item_label }, cursor)))
}

fn dimension_reference_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureDimensionReferenceTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"dim_ref\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"dim_ref\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo dimension reference table cursor traversal")?;
        cursor += 1;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (declared_count, after_count) = psb::compact_int(payload, cursor + 1);
    cursor = after_count;
    let mut reference_bytes = None;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let reference_start = cursor + 1;
        let Ok((value, next)) = psb::reference_id(payload, reference_start) else {
            return Ok(None);
        };
        reference_bytes = payload.get(reference_start..next);
        cursor = next;
        Some(value)
    } else {
        None
    };
    if payload.get(cursor..cursor + 2) == Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        cursor += 2;
    } else {
        return Ok(Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows: Vec::new(),
            offset: table,
        }));
    }
    if declared_count == 0 {
        return Ok(Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows: Vec::new(),
            offset: table,
        }));
    }

    let mut rows = Vec::new();
    let Some((prototype, prototype_end)) = named_dimension_reference(ctx, payload, cursor, end)?
    else {
        return Ok(Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows,
            offset: table,
        }));
    };
    ctx.reserve_vec(&mut rows, 1, "creo dimension reference rows")?;
    rows.push(prototype);
    let Some(reference_bytes) = reference_bytes else {
        return Ok(Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows,
            offset: table,
        }));
    };
    let separator_len = reference_bytes.len() + 3;
    let separator_matches = |offset, prefix| {
        payload.get(offset..offset + 2) == Some(&[prefix, psb::token::ENTITY_REF])
            && payload.get(offset + 2..offset + 2 + reference_bytes.len()) == Some(reference_bytes)
            && payload.get(offset + separator_len - 1) == Some(&0xe2)
    };
    if !separator_matches(prototype_end, 0xf1) {
        return Ok(Some(FeatureDimensionReferenceTable {
            declared_count,
            entity_ref,
            rows,
            offset: table,
        }));
    }
    cursor = prototype_end + separator_len;

    let row_limit = index_from_u32(declared_count);
    while rows.len() < row_limit && cursor < end {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo dimension reference table cursor traversal")?;
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
        let [first, second] = [point[0], point[1]];
        ctx.reserve_vec(&mut rows, 1, "creo dimension reference rows")?;
        rows.push(FeatureDimensionReference {
            item_id,
            sense,
            point: [first, second],
            offset: row_offset,
        });
        if rows.len() == row_limit {
            break;
        }
        if !separator_matches(cursor, 0xf3) {
            break;
        }
        cursor += separator_len;
    }
    Ok(Some(FeatureDimensionReferenceTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    }))
}

fn labeled_dimension(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureDimension>, CodecError> {
    let Some(type_label) = ctx.find_bytes_in(
        payload,
        b"type\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let (dimension_type, after_type) = segment_int(payload, type_label + b"type\0".len());
    let Some(dimension_type) = dimension_type else {
        return Ok(None);
    };
    let Some(value_label) = ctx.find_bytes_in(
        payload,
        b"value\0",
        after_type,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let value_start = value_label + b"value\0".len();
    let (value, after_value) = decode_variable_scalar(ctx, payload, value_start, end, cache)?;
    let Some(value_bytes) = payload.get(value_start..after_value) else {
        return Ok(None);
    };
    let Some(direction_label) = ctx.find_bytes_in(
        payload,
        b"direct\0",
        after_value,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let Some(&direction_byte) = payload.get(direction_label + b"direct\0".len()) else {
        return Ok(None);
    };
    let Some(auxiliary_label) = ctx.find_bytes_in(
        payload,
        b"aux_value\0",
        direction_label,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let auxiliary_start = auxiliary_label + b"aux_value\0".len();
    let (auxiliary_value, after_auxiliary) =
        decode_variable_scalar(ctx, payload, auxiliary_start, end, cache)?;
    let Some(auxiliary_bytes) = payload.get(auxiliary_start..after_auxiliary) else {
        return Ok(None);
    };
    let Some(external_label) = ctx.find_bytes_in(
        payload,
        b"ext_id\0",
        after_auxiliary,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let (external_id, after_external) = segment_int(payload, external_label + b"ext_id\0".len());
    let Some(external_id) = external_id else {
        return Ok(None);
    };
    let references = dimension_reference_table(ctx, payload, after_external, end)?;
    let value_body = ctx.copy_retained(value_bytes, "creo dimension value body")?;
    let value = DimensionValue::decoded(ctx, value.value(), &value_body)?;
    let auxiliary_body = ctx.copy_retained(auxiliary_bytes, "creo dimension auxiliary body")?;
    Ok(Some(FeatureDimension {
        dimension_type,
        value,
        value_body,
        direction_byte,
        auxiliary_value: auxiliary_value.value(),
        auxiliary_body,
        external_id,
        references,
        offset: type_label,
    }))
}

fn positional_dimension(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureDimension>, CodecError> {
    let (dimension_type, cursor) = segment_int(payload, start);
    let Some(dimension_type) = dimension_type else {
        return Ok(None);
    };
    let value_start = cursor;
    let (value, cursor) = match payload.get(cursor) {
        Some(0x00) if cursor + 3 <= end => (ScalarLane::Undefined, cursor + 3),
        Some(0x01) if cursor + 4 <= end => (ScalarLane::Undefined, cursor + 4),
        Some(0x0e) => (ScalarLane::Value(-0.5), cursor + 1),
        Some(0x18) => (ScalarLane::Value(0.0), cursor + 1),
        _ => decode_variable_scalar(ctx, payload, cursor, end, cache)?,
    };
    let Some(value_bytes) = payload.get(value_start..cursor) else {
        return Ok(None);
    };
    let Some(&direction_byte) = payload.get(cursor).filter(|_| cursor < end) else {
        return Ok(None);
    };
    let auxiliary_start = cursor + 1;
    let (auxiliary_value, cursor) = if payload.get(auxiliary_start) == Some(&0x18) {
        (Some(0.0), auxiliary_start + 1)
    } else {
        let (value, next) = decode_variable_scalar(ctx, payload, auxiliary_start, end, cache)?;
        (value.value(), next)
    };
    let Some(auxiliary_bytes) = payload.get(auxiliary_start..cursor) else {
        return Ok(None);
    };
    let (external_id, _) = segment_int(payload, cursor);
    let Some(external_id) = external_id else {
        return Ok(None);
    };
    let value_body = ctx.copy_retained(value_bytes, "creo dimension value body")?;
    let value = DimensionValue::decoded(ctx, value.value(), &value_body)?;
    let auxiliary_body = ctx.copy_retained(auxiliary_bytes, "creo dimension auxiliary body")?;
    Ok(Some(FeatureDimension {
        dimension_type,
        value,
        value_body,
        direction_byte,
        auxiliary_value,
        auxiliary_body,
        external_id,
        references: None,
        offset: start,
    }))
}

fn dimension_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureDimensionTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"dimtab_ptr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"dimtab_ptr\0".len();
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo dimension table cursor traversal")?;
        cursor += 1;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let mut reference_bytes = None;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let reference_start = cursor + 1;
        let Ok((value, next)) = psb::reference_id(payload, reference_start) else {
            return Ok(None);
        };
        reference_bytes = payload.get(cursor..next);
        cursor = next;
        Some(value)
    } else {
        None
    };
    let region_end = ctx
        .find_bytes_in(
            payload,
            b"\xe0\x00relat_ptr\0",
            cursor,
            end,
            "find Creo feature definition field",
        )?
        .unwrap_or(end);
    let first_end = if let Some(class) = reference_bytes {
        find_class_close(ctx, payload, cursor, region_end, 0xf3, class)?.unwrap_or(region_end)
    } else {
        region_end
    };
    let mut rows = Vec::new();
    if let Some(row) = labeled_dimension(ctx, payload, cursor, first_end, cache)? {
        ctx.reserve_vec(&mut rows, 1, "creo dimension rows")?;
        rows.push(row);
    }
    if let Some(class) = reference_bytes {
        let separator_len = class.len() + 2;
        let mut replay = first_end;
        while replay < region_end && rows.len() < index_from_u32(declared_count) {
            ctx.next_charged(&mut (replay..region_end), "creo dimension table cursor traversal")?;
            if !class_close_at(payload, replay, 0xf3, class) {
                break;
            }
            replay += separator_len;
            let next_separator =
                find_class_close(ctx, payload, replay, region_end, 0xf3, class)?.unwrap_or(region_end);
            let Some(row) = positional_dimension(ctx, payload, replay, next_separator, cache)?
            else {
                break;
            };
            ctx.reserve_vec(&mut rows, 1, "creo dimension rows")?;
            rows.push(row);
            replay = next_separator;
        }
    }
    Ok(Some(FeatureDimensionTable {
        declared_count,
        entity_ref,
        rows,
        offset: table,
    }))
}

fn positional_dimension_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureDimensionTable>, CodecError> {
    let Some((table, declared_count, mut cursor, reference_bytes)) = first_positional_array(ctx, payload, start, end, table_class)? else { return Ok(None); };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Ok(None);
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(None);
    };
    cursor = after_row_class;

    let separator_len = reference_bytes.len() + 2;
    let mut rows = Vec::new();
    let row_limit = index_from_u32(declared_count);
    while cursor < end && rows.len() < row_limit {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo positional dimension table cursor traversal")?;
        let row_end = find_class_close(ctx, payload, cursor, end, 0xf3, reference_bytes)?.unwrap_or(end);
        let Some(row) = positional_dimension(ctx, payload, cursor, row_end, cache)? else {
            break;
        };
        ctx.reserve_vec(&mut rows, 1, "creo dimension rows")?;
        rows.push(row);
        if rows.len() == row_limit {
            break;
        }
        if !class_close_at(payload, row_end, 0xf3, reference_bytes) {
            break;
        }
        cursor = row_end + separator_len;
    }
    Ok(Some(FeatureDimensionTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        offset: table,
    }))
}

fn self_described_positional_dimension_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<FeatureDimensionTable>, CodecError> {
    let mut candidate = None;
    let mut offsets = start..end;
    while let Some(table) = ctx.next_charged(&mut offsets, "creo self described dimension traversal")? {
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
        let mut candidate_storage = ctx.reserve_scoped(0, "creo self described dimension candidate")?;
        let Some(found) = candidate_storage.with_storage(|| positional_dimension_table(ctx, payload, table, end, table_class, cache))?
        else {
            continue;
        };
        if found.offset == table
            && declared_count > 1
            && found.declared_count == declared_count
            && usize::try_from(declared_count).ok() == Some(found.rows.len())
            && ctx.all_by(&found.rows, |row| Ok(matches!(row.dimension_type, 0x01..=0x05 | 0x0a)), "creo self described dimension kinds")?
        {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some((found, candidate_storage));
        }
    }
    match candidate { Some((table, storage)) => { storage.commit()?; Ok(Some(table)) }, None => Ok(None) }
}

fn feature_skamps(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Vec<FeatureSkamp>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"skamp_ptr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(Vec::new());
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
        return Ok(Vec::new());
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let class_start = cursor;
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(Vec::new());
    };
    let class_encoding = &payload[class_start..next];
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Ok(Vec::new());
    }
    cursor += 2;
    let Some(prototype_end) = find_class_close(ctx, payload, cursor, end, 0xf3, class_encoding)? else {
        return Ok(Vec::new());
    };
    let named_item = match named_compact_int(ctx, payload, b"ent_id\0", cursor, prototype_end)? {
        Some(entity_id) => named_compact_int(ctx, payload, b"sense\0", cursor, prototype_end)?
            .map(|sense| FeatureSkampItem { entity_id, sense }),
        None => None,
    };
    let Some(items_label) = ctx.find_bytes_in(
        payload,
        b"items\0",
        cursor,
        prototype_end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(Vec::new());
    };
    let mut item_cursor = items_label + b"items\0".len();
    if payload.get(item_cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(Vec::new());
    }
    let (prototype_item_count, after_count) = psb::compact_int(payload, item_cursor + 1);
    item_cursor = after_count;
    let item_class_start = item_cursor;
    let Ok((_, after_item_class)) = psb::reference_id(payload, item_cursor + 1) else {
        return Ok(Vec::new());
    };
    let item_class_encoding = &payload[item_class_start..after_item_class];
    let named_item_end = find_class_close(
        ctx, payload,
        after_item_class,
        prototype_end,
        0xf1,
        item_class_encoding,
    )?;
    let (named_item_end, named_item_close_len) = match named_item_end {
        Some(offset) => (offset, item_class_encoding.len() + 2),
        None if prototype_item_count == 1 && named_item.is_some() => {
            // Some named prototypes store the one named item directly in the
            // array body. The outer table trailer closes both the prototype
            // and its one-item schema; there is no inner item trailer.
            (prototype_end, 0)
        }
        None => return Ok(Vec::new()),
    };
    item_cursor = named_item_end + named_item_close_len;
    let mut prototype_items = Vec::new();
    let mut prototype_storage = ctx.reserve_scoped(0, "creo skamp prototype items")?;
    if let Some(item) = named_item {
        prototype_storage.with_storage(|| ctx.reserve_vec(&mut prototype_items, 1, "creo skamp prototype items"))?;
        prototype_items.push(item);
    }
    while prototype_items.len() < index_from_u32(prototype_item_count) {
        ctx.next_charged(&mut (prototype_items.len()..index_from_u32(prototype_item_count)), "creo feature skamps cursor traversal")?;
        let (Some(entity_id), next) = segment_int(payload, item_cursor) else {
            return Ok(Vec::new());
        };
        item_cursor = next;
        let (Some(sense), next) = segment_int(payload, item_cursor) else {
            return Ok(Vec::new());
        };
        item_cursor = next;
        prototype_storage.with_storage(|| ctx.reserve_vec(&mut prototype_items, 1, "creo skamp prototype items"))?;
        prototype_items.push(FeatureSkampItem { entity_id, sense });
    }
    if item_cursor != prototype_end {
        return Ok(Vec::new());
    }
    let Some(id) = named_compact_int(ctx, payload, b"id\0", cursor, prototype_end)? else {
        return Ok(Vec::new());
    };
    let Some(kind) = named_compact_int(ctx, payload, b"type\0", cursor, prototype_end)? else {
        return Ok(Vec::new());
    };
    let Some(flags) = named_compact_int(ctx, payload, b"flags\0", cursor, prototype_end)? else {
        return Ok(Vec::new());
    };
    let Some(status) = named_compact_int(ctx, payload, b"status\0", cursor, prototype_end)? else {
        return Ok(Vec::new());
    };
    prototype_storage.commit()?;
    let prototype = FeatureSkamp {
        id,
        kind,
        flags,
        status,
        items: prototype_items,
        offset: cursor,
    };
    let mut rows = Vec::new();
    ctx.reserve_vec(&mut rows, 1, "creo skamp rows")?;
    rows.push(prototype);
    cursor = prototype_end + class_encoding.len() + 2;
    'rows: while rows.len() < index_from_u32(declared_count) {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo feature skamps cursor traversal")?;
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
        let mut item_storage = ctx.reserve_scoped(0, "creo skamp items")?;
        while items.len() < index_from_u32(item_count) {
            ctx.next_charged(&mut (items.len()..index_from_u32(item_count)), "creo feature skamps cursor traversal")?;
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
            item_storage.with_storage(|| ctx.reserve_vec(&mut items, 1, "creo skamp items"))?;
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
        if class_close_at(payload, cursor, 0xf3, class_encoding) {
            cursor += class_encoding.len() + 2;
        } else if payload.get(cursor) == Some(&0xe2) {
            cursor += 1;
        } else if payload.get(cursor) == Some(&0xe0) {
            // The final row is terminated by the following named table.
        } else {
            break;
        }
        item_storage.commit()?;
        ctx.reserve_vec(&mut rows, 1, "creo skamp rows")?;
        rows.push(FeatureSkamp {
            id,
            kind,
            flags,
            status,
            items,
            offset: row_offset,
        });
    }
    Ok(rows)
}

fn named_array_class(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<u32>, CodecError> {
    let Some(offset) = ctx.find_bytes_in(payload, label, start, end, "find Creo array class")?
    else {
        return Ok(None);
    };
    let label = offset + label.len();
    let Some(array) = ctx.find_by(label..end, |&offset| Ok(payload.get(offset) == Some(&psb::token::ARRAY_OPEN)), "creo named class array search")? else { return Ok(None); };
    let (_, after_count) = psb::compact_int(payload, array + 1);
    if payload.get(after_count) != Some(&psb::token::ENTITY_REF) { return Ok(None); }
    Ok(psb::reference_id(payload, after_count + 1).ok().map(|(class, _)| class))
}

fn named_solver_table_header(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    label: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureSolverTableHeader>, CodecError> {
    let Some(offset) = ctx.find_bytes_in(payload, label, start, end, "find Creo solver table")?
    else {
        return Ok(None);
    };
    Ok((|| {
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
    })())
}


fn positional_array_candidate(payload: &[u8], offset: usize, table_class: u32) -> Option<(usize, u32, usize, &[u8])> {
    if payload.get(offset) != Some(&psb::token::ARRAY_OPEN) { return None; }
    let (count, after_count) = psb::compact_int(payload, offset + 1);
    if payload.get(after_count) != Some(&psb::token::ENTITY_REF) { return None; }
    let (class, after_class) = psb::reference_id(payload, after_count + 1).ok()?;
    (class == table_class && payload.get(after_class..after_class + 2) == Some(&[0xfb, 0xe2]))
        .then_some((offset, count, after_class + 2, &payload[after_count..after_class]))
}

fn first_positional_array<'a>(ctx: &DecodeContext<'_>, payload: &'a [u8], start: usize, end: usize, table_class: u32) -> Result<Option<(usize, u32, usize, &'a [u8])>, CodecError> {
    ctx.find_map(start..end, |offset| Ok(positional_array_candidate(payload, offset, table_class)), "creo positional array header search")
}

fn positional_array_header<'a>(ctx: &DecodeContext<'_>, payload: &'a [u8], start: usize, end: usize, table_class: u32) -> Result<Option<(usize, u32, usize, &'a [u8])>, CodecError> {
    let mut offsets = start..end;
    let Some(candidate) = ctx.find_map(&mut offsets, |offset| Ok(positional_array_candidate(payload, offset, table_class)), "creo positional array header search")? else { return Ok(None); };
    let second = ctx.find_map(offsets, |offset| Ok(positional_array_candidate(payload, offset, table_class)), "creo positional array header uniqueness")?;
    Ok(second.is_none().then_some(candidate))
}

fn class_close_at(payload: &[u8], offset: usize, prefix: u8, class: &[u8]) -> bool {
    let Some(length) = class.len().checked_add(2) else {
        return false;
    };
    let Some(close) = offset.checked_add(length) else {
        return false;
    };
    payload.get(offset..close).is_some_and(|window| {
        window.first() == Some(&prefix)
            && window.get(1..length - 1) == Some(class)
            && window.last() == Some(&0xe2)
    })
}

fn find_class_close(
    ctx: &DecodeContext<'_>, payload: &[u8], start: usize, end: usize, prefix: u8, class: &[u8],
) -> Result<Option<usize>, CodecError> {
    if payload.get(start..end).is_none() { return Ok(None); }
    let Some(length) = class.len().checked_add(2) else { return Ok(None); };
    let Some(last) = end.checked_sub(length) else { return Ok(None); };
    ctx.find_by(start..=last, |&offset| Ok(class_close_at(payload, offset, prefix, class)), "creo class close search")
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

fn positional_feature_skamps(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Result<Option<SolverSubtable<FeatureSkamp>>, CodecError> {
    let Some((table, count, mut cursor, table_class_encoding)) =
        positional_array_header(ctx, payload, start, end, table_class)?
    else {
        return Ok(None);
    };
    let header = FeatureSolverTableHeader { declared_count: count, entity_ref: table_class, offset: table };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Ok(Some(SolverSubtable::Declared { header, rows: Vec::new() }));
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(Some(SolverSubtable::Declared { header, rows: Vec::new() }));
    };
    cursor = after_row_class;
    let mut rows = Vec::new();
    let mut item_classes = None::<(&[u8], &[u8])>;
    'rows: while rows.len() < index_from_u32(count) {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo positional feature skamps cursor traversal")?;
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
        let expected_item_classes = item_classes;
        let Some((item_count, after_item_row_class, item_table_class, item_row_class, row_end)) =
            positional_skamp_item_array(
                ctx, payload,
                cursor,
                end,
                table_class_encoding,
                item_classes.as_ref().map(|classes| classes.0),
                item_classes.as_ref().map(|classes| classes.1),
            )?
        else {
            break;
        };
        let classes = item_classes.get_or_insert((item_table_class, item_row_class));
        cursor = after_item_row_class;
        let mut items = Vec::new();
        let mut item_storage = ctx.reserve_scoped(0, "creo skamp items")?;
        while items.len() < index_from_u32(item_count) {
            ctx.next_charged(&mut (items.len()..index_from_u32(item_count)), "creo positional feature skamps cursor traversal")?;
            let Some(entity_id) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            let Some(sense) = next_solver_int(payload, &mut cursor) else {
                break 'rows;
            };
            item_storage.with_storage(|| ctx.reserve_vec(&mut items, 1, "creo skamp items"))?;
            items.push(FeatureSkampItem { entity_id, sense });
            if items.len() < index_from_u32(item_count) {
                let Some(next) =
                    consume_positional_separator(payload, cursor, end, classes.0, &[0xf1])
                else {
                    break 'rows;
                };
                cursor = next;
            }
        }
        if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
            if let Some(second) = positional_skamp_item_array_candidate(payload, cursor, row_end, expected_item_classes.map(|classes| classes.0), expected_item_classes.map(|classes| classes.1)) {
                if positional_skamp_item_array_body_end(ctx, payload, second.1, second.0, second.2, end)?.is_some() { break; }
            }
        }
        if positional_skamp_item_array_has_valid_boundary(payload, cursor, item_table_class, table_class_encoding, end).is_none() { break; }
        let row = FeatureSkamp {
            id,
            kind,
            flags,
            status,
            items,
            offset: row_offset,
        };
        if rows.len() + 1 < index_from_u32(count) {
            let Some(next) =
                consume_positional_separator(payload, cursor, end, table_class_encoding, &[0xf3])
            else {
                break;
            };
            cursor = next;
        }
        item_storage.commit()?;
        ctx.reserve_vec(&mut rows, 1, "creo skamp rows")?;
        rows.push(row);
    }
    Ok(Some(SolverSubtable::Declared { header, rows }))
}

fn positional_skamp_item_array<'a>(
    ctx: &DecodeContext<'_>,
    payload: &'a [u8],
    start: usize,
    end: usize,
    outer_table_class: &[u8],
    expected_table_class: Option<&[u8]>,
    expected_row_class: Option<&[u8]>,
) -> Result<Option<(u32, usize, &'a [u8], &'a [u8], usize)>, CodecError> {
    let row_end = find_class_close(ctx, payload, start, end, 0xf3, outer_table_class)?.unwrap_or(end);
    let Some(candidate) = ctx.find_map(start..row_end, |array| Ok(positional_skamp_item_array_candidate(payload, array, row_end, expected_table_class, expected_row_class)), "creo skamp item array search")? else { return Ok(None); };
    Ok(Some((candidate.0, candidate.1, candidate.2, candidate.3, row_end)))
}


fn positional_skamp_item_array_candidate<'a>(
    payload: &'a [u8],
    array: usize,
    end: usize,
    expected_table_class: Option<&[u8]>,
    expected_row_class: Option<&[u8]>,
) -> Option<(u32, usize, &'a [u8], &'a [u8])> {
    (payload.get(array) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
    let (count, after_count) = psb::compact_int(payload, array + 1);
    (payload.get(after_count) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_table_class) = psb::reference_id(payload, after_count + 1).ok()?;
    let table_class = payload.get(after_count..after_table_class)?;
    expected_table_class
        .is_none_or(|expected| expected == table_class)
        .then_some(())?;
    (payload.get(after_table_class..after_table_class + 2) == Some(&[0xfb, 0xe2])).then_some(())?;
    let row_class_start = after_table_class + 2;
    (payload.get(row_class_start) == Some(&psb::token::ENTITY_REF)).then_some(())?;
    let (_, after_row_class) = psb::reference_id(payload, row_class_start + 1).ok()?;
    (after_row_class <= end).then_some(())?;
    let row_class = payload.get(row_class_start..after_row_class)?;
    expected_row_class
        .is_none_or(|expected| expected == row_class)
        .then_some(())?;
    Some((count, after_row_class, table_class, row_class))
}

fn positional_skamp_item_array_body_end(
    ctx: &DecodeContext<'_>, payload: &[u8], mut cursor: usize, item_count: u32, item_table_class: &[u8], end: usize,
) -> Result<Option<usize>, CodecError> {
    let mut items = 0..item_count;
    while let Some(index) = ctx.next_charged(&mut items, "creo skamp item boundary traversal")? {
        if next_solver_int(payload, &mut cursor).is_none() || next_solver_int(payload, &mut cursor).is_none() { return Ok(None); }
        if index + 1 < item_count {
            let Some(next) = consume_positional_separator(payload, cursor, end, item_table_class, &[0xf1]) else { return Ok(None); };
            cursor = next;
        }
    }
    Ok(Some(cursor))
}

fn positional_skamp_item_array_has_valid_boundary(
    payload: &[u8],
    cursor: usize,
    item_table_class: &[u8],
    outer_table_class: &[u8],
    end: usize,
) -> Option<()> {
    if cursor == end {
        return Some(());
    }
    if class_close_at(payload, cursor, 0xf3, outer_table_class) {
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

fn feature_relation_triples(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Vec<FeatureRelationTriple>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"triples_ptr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(Vec::new());
    };
    let mut cursor = table + b"triples_ptr\0".len();
    if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x04]) {
        cursor += 2;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(Vec::new());
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let Ok((_, next)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(Vec::new());
    };
    cursor = next;
    if payload.get(cursor..cursor + 2) != Some(&[psb::token::ARRAY_CLOSE, 0xe2]) {
        return Ok(Vec::new());
    }
    cursor += 2;
    let Some(close) = ctx.find_bytes_in(
        payload,
        &[0xf1, psb::token::ENTITY_REF],
        cursor,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(Vec::new());
    };
    let prototype = FeatureRelationTriple {
        relation_id: named_compact_int(ctx, payload, b"rel_id\0", cursor, close)?,
        equation_id: named_compact_int(ctx, payload, b"eqn_id\0", cursor, close)?,
        skamp_id: named_compact_int(ctx, payload, b"skamp_id\0", cursor, close)?,
        offset: cursor,
    };
    let Ok((_, next)) = psb::reference_id(payload, close + 2) else {
        return Ok(Vec::new());
    };
    cursor = next;
    if payload.get(cursor) != Some(&0xe2) {
        return Ok(Vec::new());
    }
    cursor += 1;
    let mut rows = Vec::new();
    ctx.reserve_vec(&mut rows, 1, "creo relation triples")?;
    rows.push(prototype);
    while rows.len() < index_from_u32(declared_count) {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo feature relation triples cursor traversal")?;
        let row_offset = cursor;
        let relation_id = next_solver_int(payload, &mut cursor);
        let equation_id = next_solver_int(payload, &mut cursor);
        let skamp_id = next_solver_int(payload, &mut cursor);
        let terminal_named_boundary = rows.len() + 1 == index_from_u32(declared_count)
            && payload.get(cursor).is_some_and(|byte| *byte >= 0xe0);
        if payload.get(cursor) != Some(&0xe2) && !terminal_named_boundary {
            break;
        }
        if !terminal_named_boundary {
            cursor += 1;
        }
        ctx.reserve_vec(&mut rows, 1, "creo relation triples")?;
        rows.push(FeatureRelationTriple {
            relation_id,
            equation_id,
            skamp_id,
            offset: row_offset,
        });
    }
    Ok(rows)
}

fn positional_relation_triples(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Result<Option<SolverSubtable<FeatureRelationTriple>>, CodecError> {
    let Some((table, count, mut cursor, class_encoding)) =
        positional_array_header(ctx, payload, start, end, table_class)?
    else {
        return Ok(None);
    };
    let header = FeatureSolverTableHeader { declared_count: count, entity_ref: table_class, offset: table };
    if payload.get(cursor) != Some(&psb::token::ENTITY_REF) {
        return Ok(Some(SolverSubtable::Declared { header, rows: Vec::new() }));
    }
    let Ok((_, after_row_class)) = psb::reference_id(payload, cursor + 1) else {
        return Ok(Some(SolverSubtable::Declared { header, rows: Vec::new() }));
    };
    cursor = after_row_class;
    let mut rows = Vec::new();
    while rows.len() < index_from_u32(count) {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo positional relation triples cursor traversal")?;
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
        if rows.len() + 1 < index_from_u32(count) {
            let Some(next) =
                consume_positional_separator(payload, cursor, end, class_encoding, &[0xf1])
            else {
                break;
            };
            cursor = next;
        }
        ctx.reserve_vec(&mut rows, 1, "creo relation triples")?;
        rows.push(row);
    }
    Ok(Some(SolverSubtable::Declared { header, rows }))
}

fn relation_operand_vectors(bytes: &[u8]) -> Option<[[Option<u32>; 4]; 3]> {
    let mut values = [None; 14];
    let mut filled = 0;
    let mut cursor = 0;
    while cursor < bytes.len() && filled < 12 {
        match bytes[cursor] {
            0xe4 => {
                values[filled] = Some(1);
                filled += 1;
                cursor += 1;
            }
            0xe5 => {
                values[filled..filled + 2].fill(Some(0));
                filled += 2;
                cursor += 1;
            }
            0xe6 => {
                values[filled..filled + 3].fill(Some(0));
                filled += 3;
                cursor += 1;
            }
            0xf6 => {
                filled += 1;
                cursor += 1;
            }
            _ => {
                let value = next_solver_int(bytes, &mut cursor)?;
                values[filled] = Some(value);
                filled += 1;
            }
        }
    }
    if cursor != bytes.len() || filled != 12 {
        return None;
    }
    Some([
        [values[0], values[1], values[2], values[3]],
        [values[4], values[5], values[6], values[7]],
        [values[8], values[9], values[10], values[11]],
    ])
}

fn relation_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<FeatureRelationTable>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"relat_ptr\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let mut cursor = table + b"relat_ptr\0".len();
    if payload.get(cursor..cursor + 2) == Some(&[0xf4, 0x04]) {
        cursor += 2;
    }
    if payload.get(cursor) != Some(&psb::token::ARRAY_OPEN) {
        return Ok(None);
    }
    let (declared_count, next) = psb::compact_int(payload, cursor + 1);
    cursor = next;
    let entity_ref = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let Ok((value, next)) = psb::reference_id(payload, cursor + 1) else {
            return Ok(None);
        };
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
    let mut rows_end = end;
    for label in [b"skamp_ptr\0".as_slice(), b"triples_ptr\0"] {
        if let Some(offset) = ctx.find_bytes_in(
            payload,
            label,
            cursor,
            end,
            "find Creo feature definition field",
        )? {
            rows_end = rows_end.min(offset);
        }
    }
    let rows_start = ctx
        .find_bytes_in(
            payload,
            &[0xf1, psb::token::ENTITY_REF],
            cursor,
            rows_end,
            "find Creo feature definition field",
        )?
        .and_then(|close| {
            let (_, after_ref) = psb::reference_id(payload, close + 2).ok()?;
            (payload.get(after_ref) == Some(&0xe2)).then_some(after_ref + 1)
        });
    let rows = match rows_start {
        Some(rows_start) => positional_relation_rows(
            ctx,
            payload,
            rows_start,
            rows_end,
            RelationBodyRows::from_declared(declared_count),
        )?,
        None => Vec::new(),
    };
    Ok(Some(FeatureRelationTable {
        declared_count,
        entity_ref,
        rows,
        skamps: SolverSubtable::from_parts(
            named_solver_table_header(ctx, payload, b"skamp_ptr\0", start, end)?,
            feature_skamps(ctx, payload, start, end)?,
        ),
        triples: SolverSubtable::from_parts(
            named_solver_table_header(ctx, payload, b"triples_ptr\0", start, end)?,
            feature_relation_triples(ctx, payload, start, end)?,
        ),
        offset: table,
    }))
}

/// Body rows stated by a `relat_ptr` allocation count. A count of one is the
/// empty table form, and a count of zero is the invalid form the decoder
/// reports from the retained declared count. Every larger count states two
/// structural entries before its body rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationBodyRows {
    InvalidZero,
    Count(u32),
}

impl RelationBodyRows {
    /// Structural entries inside a relation table's declared count.
    const STRUCTURAL_ENTRIES: u32 = 2;

    fn from_declared(declared_count: u32) -> Self {
        match declared_count {
            0 => Self::InvalidZero,
            1 | 2 => Self::Count(0),
            declared_count => Self::Count(declared_count - Self::STRUCTURAL_ENTRIES),
        }
    }

    fn get(self) -> Option<u32> {
        match self {
            Self::InvalidZero => None,
            Self::Count(count) => Some(count),
        }
    }
}

fn positional_relation_rows(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    end: usize,
    row_count: RelationBodyRows,
) -> Result<Vec<FeatureRelation>, CodecError> {
    if cursor > end || end > payload.len() {
        return Ok(Vec::new());
    }
    let Some(row_count) = row_count.get() else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    let mut counted_items = 0..row_count;

        while let Some(_) = ctx.next_charged(&mut counted_items, "creo positional relation rows traversal")? {let Some(row_end) = ctx.position_by(&payload[cursor..end], |byte| Ok(*byte == 0xe2), "creo relation row end")?
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
        let mut suffix = None;
        for suffix_start in after_used.max(row_end.saturating_sub(6))..row_end {
            let (sign, after_sign) = psb::compact_int(payload, suffix_start);
            let (dimension_id, after_dimension) = psb::compact_int(payload, after_sign);
            let (relation_type, after_type) = psb::compact_int(payload, after_dimension);
            if after_sign > suffix_start
                && after_dimension > after_sign
                && after_type > after_dimension
                && after_type == row_end
            {
                if suffix.is_some() {
                    suffix = None;
                    break;
                }
                suffix = Some((suffix_start, sign, dimension_id, relation_type));
            }
        }
        let Some((suffix_start, sign, dimension_id, relation_type)) = suffix else {
            break;
        };
        let operands =
            ctx.copy_retained(&payload[after_used..suffix_start], "creo relation operands")?;
        let body = ctx.copy_retained(&payload[cursor..row_end], "creo relation row body")?;
        ctx.reserve_vec(&mut rows, 1, "creo relation rows")?;
        rows.push(FeatureRelation {
            relation_id,
            used,
            operand_vectors: relation_operand_vectors(&operands),
            operands,
            sign,
            dimension_id,
            relation_type,
            body,
            offset: cursor,
        });
        cursor = row_end + 1;
    }
    Ok(rows)
}

fn positional_relation_table(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Result<Option<FeatureRelationTable>, CodecError> {
    let Some((table, declared_count, cursor, reference_bytes)) = first_positional_array(ctx, payload, start, end, table_class)? else { return Ok(None); };
    let rows_start = if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
        match psb::reference_id(payload, cursor + 1) {
            Ok((_, prototype)) => find_class_close(ctx, payload, prototype, end, 0xf1, reference_bytes)?.map(|offset| offset + reference_bytes.len() + 2),
            Err(_) => None,
        }
    } else { None };
    let rows = match rows_start {
        Some(rows_start) => positional_relation_rows(
            ctx,
            payload,
            rows_start,
            end,
            RelationBodyRows::from_declared(declared_count),
        )?,
        None => Vec::new(),
    };
    Ok(Some(FeatureRelationTable {
        declared_count,
        entity_ref: Some(table_class),
        rows,
        skamps: None,
        triples: None,
        offset: table,
    }))
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
        // endian-exception: reconstructed-scalar
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    if prefix == 0x2d && offset + 8 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x40;
        raw[1..].copy_from_slice(&payload[offset + 1..offset + 8]);
        // endian-exception: reconstructed-scalar
        return (Some(f64::from_be_bytes(raw)), offset + 8);
    }
    if matches!(prefix, 0x74 | 0x75) && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0x3f;
        // wrapping-exception: DICT prefix remapping reconstructs the low IEEE byte modulo 256
        raw[1] = prefix.wrapping_sub(0x8b);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0x99 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0xc0, 0x0e]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0xdd && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[..2].copy_from_slice(&[0x40, 0x0c]);
        raw[2..].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
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
        // endian-exception: reconstructed-scalar
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    if prefix == 0xd5 && offset + 7 <= end {
        let mut raw = [0; 8];
        raw[0] = 0xbf;
        raw[1..7].copy_from_slice(&payload[offset + 1..offset + 7]);
        // endian-exception: reconstructed-scalar
        return (Some(f64::from_be_bytes(raw)), offset + 7);
    }
    scalar::decode_in_lane(payload, offset, cache)
        .filter(|(_, next)| *next <= end)
        .map_or((None, offset + 1), |(value, next)| (Some(value), next))
}

fn saved_line_block(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    segment_end: usize,
    cache: &scalar::ScalarCache,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    if payload.get(cursor) == Some(&0xf1) {
        cursor = ctx.position_by(&payload[cursor..segment_end], |byte| Ok(*byte == 0xe3), "creo saved line prototype close")?
            .map_or(segment_end, |relative| cursor + relative + 1);
    }
    while cursor < segment_end {
        ctx.next_charged(&mut (cursor..=payload.len()), "creo saved line block cursor traversal")?;
        if payload.get(cursor) == Some(&0xe3) {
            cursor += 1;
        }
        let point_label = b"\xe0\x00entity(point)\0";
        if payload.get(cursor..cursor + point_label.len()) == Some(point_label) {
            let Some(close) = ctx.find_bytes_in(
                payload,
                &[0xf1, psb::token::ENTITY_REF],
                cursor + point_label.len(),
                segment_end,
                "find Creo feature definition field",
            )?
            else {
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
        let mut storage = ctx.reserve_scoped(0, "creo saved line record scratch")?;
        let mut attributes = Vec::new();
        loop {
            ctx.next_charged(&mut (cursor..=payload.len()), "creo saved line block cursor traversal")?;
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                storage.with_storage(|| ctx.reserve_vec(&mut references, 1, "creo saved line references"))?;
                references.push(reference);
                cursor = next;
            } else if payload
                .get(cursor..cursor + 2)
                .is_some_and(|bytes| matches!(bytes, [0xf0 | 0xf1, 0xf7]))
            {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 2) else {
                    break;
                };
                storage.with_storage(|| ctx.reserve_vec(&mut references, 1, "creo saved line references"))?;
                references.push(reference);
                cursor = next;
            } else if payload.get(cursor) == Some(&0xeb) {
                let Some(bytes) = payload.get(cursor + 1..cursor + 6) else {
                    break;
                };
                let mut attribute = [0; 5];
                attribute.copy_from_slice(bytes);
                storage.with_storage(|| ctx.reserve_vec(&mut attributes, 1, "creo saved line attributes"))?;
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
        let mut values = [None; 8];
        let mut filled = 0;
        while cursor < segment_end && filled < 6 {
            ctx.next_charged(&mut (cursor..=payload.len()), "creo saved line block cursor traversal")?;
            if payload.get(cursor) == Some(&0xe3)
                || payload.get(cursor) == Some(&psb::token::NAMED_RECORD)
            {
                break;
            }
            if payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
                values[filled..filled + 3].copy_from_slice(&[Some(0.0), Some(1.0), Some(0.0)]);
                filled += 3;
                cursor += 2;
                continue;
            }
            if payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    break;
                };
                storage.with_storage(|| ctx.reserve_vec(&mut references, 1, "creo saved line references"))?;
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
                storage.with_storage(|| ctx.reserve_vec(&mut references, 1, "creo saved line references"))?;
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
                storage.with_storage(|| ctx.reserve_vec(&mut attributes, 1, "creo saved line attributes"))?;
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
            values[filled] = value;
            filled += 1;
            cursor = next;
        }
        loop {
            ctx.next_charged(&mut (cursor..=payload.len()), "creo saved line block cursor traversal")?;
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
            storage.with_storage(|| ctx.reserve_vec(&mut references, 1, "creo saved line references"))?;
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
        storage.commit()?;
        let body =
            ctx.copy_retained(&payload[record_offset..record_end], "creo saved line body")?;
        ctx.reserve_vec(entities, 1, "creo saved line block entities")?;
        entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
            entity_id,
            references,
            attributes,
            endpoints: [
                [values[0], values[1], values[2]],
                [values[3], values[4], values[5]],
            ],
            body,
            offset: record_offset,
        }));
    }
    Ok(())
}

fn saved_line_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    let label = b"\xe0\x00entity(line)\0";
    let mut search = start;
    while let Some(label_offset) = ctx.find_bytes_in(
        payload,
        label,
        search,
        end,
        "find Creo feature definition field",
    )? {
        let body_start = label_offset + label.len();
        let mut body_end = end;
        for next_label in [
            b"\xe0\x00entity(arc)\0".as_slice(),
            b"\xe0\x00entity(circle)\0".as_slice(),
            b"\xe0\x00entity(dummy_ent)\0".as_slice(),
        ] {
            if let Some(offset) = ctx.find_bytes_in(
                payload,
                next_label,
                body_start,
                end,
                "find Creo feature definition field",
            )? {
                body_end = body_end.min(offset);
            }
        }
        saved_line_block(ctx, payload, body_start, body_end, cache, entities)?;
        search = body_end;
    }
    Ok(())
}

fn saved_named_scalars<const N: usize>(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    field: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<[Option<f64>; N]>, CodecError> {
    let Some(window) = payload.get(start..end) else { return Ok(None); };
    let Some(label_start) = ctx.find_by(0..window.len(), |&offset| Ok(
        window.get(offset..offset + 2) == Some(&[0xe0, 0x02])
            && window.get(offset + 2..offset + 2 + field.len()) == Some(field)
            && window.get(offset + 2 + field.len()) == Some(&0)
    ), "creo saved scalar label search")? else { return Ok(None); };
    let mut cursor = start + label_start + field.len() + 3;
    while payload
        .get(cursor)
        .is_some_and(|byte| matches!(byte, 0xf1..=0xf3))
    {
        ctx.next_charged(&mut (cursor..payload.len()), "creo saved scalar prefix traversal")?;
        cursor += 1;
    }
    if payload.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let (count, next) = psb::compact_int(payload, cursor + 1);
        if usize::try_from(count).ok() != Some(N) { return Ok(None); }
        cursor = next;
    }
    if N == 3 && payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
        return Ok(Some(std::array::from_fn(|index| {
            Some(if index == 1 { 1.0 } else { 0.0 })
        })));
    }
    let mut values = [None; N];
    for value in &mut values {
        let (decoded, next) = saved_section_scalar(payload, cursor, end, cache);
        if next <= cursor { return Ok(None); }
        *value = decoded;
        cursor = next;
    }
    Ok(Some(values))
}

fn saved_arc_scalar(
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
        // endian-exception: reconstructed-scalar
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
        // endian-exception: reconstructed-scalar
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

fn saved_positional_generated_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    const HEADER_WINDOW: usize = 24;
    let (Some(order_table), Some(segments)) = (order_table, segments) else {
        return Ok(());
    };
    let mut generated_storage = ctx.reserve_scoped(0, "Creo saved generated lookup storage")?;
    let mut starts = Vec::new();
    for separator in ctx.admit_iter(start..end, "creo saved generated start traversal")? {
        if payload.get(separator) != Some(&0xe3) {
            continue;
        }
        let row_start = separator + 1;
        let (Some(entity_id), after_id) = segment_int(payload, row_start) else {
            continue;
        };
        if order_table.external_id(entity_id).filter(|&id| order_table.internal_id(id) == Some(entity_id)).and_then(|id| segments.unique_segment(id)).is_none() {
            continue;
        }
        let Some(header_end) = after_id
            .checked_add(HEADER_WINDOW)
            .map(|window_end| window_end.min(end))
        else {
            continue;
        };
        if after_id > header_end {
            continue;
        }
        if payload[after_id..header_end].contains(&0xe2) {
            generated_storage.with_storage(|| {
                ctx.reserve_vec(&mut starts, 1, "creo saved generated row starts")
            })?;
            starts.push(row_start);
        }
    }
    for (index, row_start) in ctx.admit_iter(&starts, "creo saved generated row traversal")?.copied().enumerate() {
        // Every row start follows an 0xe3 separator, so it has a preceding byte.
        let row_end = starts
            .get(index + 1)
            .and_then(|next| next.checked_sub(1))
            .unwrap_or(end);
        let (Some(entity_id), after_id) = segment_int(payload, row_start) else {
            continue;
        };
        let Some(segment) = order_table.external_id(entity_id).filter(|&id| order_table.internal_id(id) == Some(entity_id)).and_then(|id| segments.unique_segment(id)) else { continue; };
        let value_count = match segment.kind {
            FeatureSegmentKind::Line(_) => 6,
            FeatureSegmentKind::Arc(_) => 12,
            FeatureSegmentKind::Point(_) => continue,
        };
        if after_id > row_end {
            continue;
        }
        let Some(header_size) = ctx.position_by(&payload[after_id..row_end], |byte| Ok(*byte == 0xe2), "creo saved generated row header")?
        else {
            continue;
        };
        let mut cursor = after_id + header_size + 1;
        let mut values = [None; 14];
        let mut filled = 0;
        while cursor < row_end && filled < value_count {
            ctx.next_charged(&mut (cursor..=payload.len()), "creo saved positional generated entities cursor traversal")?;
            if payload.get(cursor) == Some(&0xe3) {
                break;
            }
            if payload.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
                values[filled..filled + 3].copy_from_slice(&[Some(0.0), Some(1.0), Some(0.0)]);
                filled += 3;
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
            values[filled] = value;
            filled += 1;
            cursor = next;
        }
        if filled != value_count
            && (!matches!(segment.kind, FeatureSegmentKind::Arc(_))
                || filled > value_count
                || (cursor != row_end && payload.get(cursor) != Some(&0xe3)))
        {
            continue;
        }
        match segment.kind {
            FeatureSegmentKind::Line(_) => {
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
                        (first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale
                    }
                    (Some(1), _, Some(first), _, Some(second)) => {
                        let scale = first.abs().max(second.abs()).max(1.0);
                        (first - second).abs() <= EPS_PARAMETER_AGREEMENT * scale
                    }
                    _ => false,
                };
                if orientation_matches {
                    let body_end = saved_positional_body_end(payload, row_end);
                    let body = ctx.copy_retained(
                        &payload[row_start..body_end],
                        "creo saved generated line body",
                    )?;
                    ctx.reserve_vec(entities, 1, "creo saved generated entities")?;
                    entities.push(FeatureSavedEntity::Line(FeatureSavedLine {
                        entity_id,
                        references: Vec::new(),
                        attributes: Vec::new(),
                        endpoints,
                        body,
                        offset: row_start,
                    }));
                }
            }
            FeatureSegmentKind::Arc(_) => {
                let body_end = saved_positional_body_end(payload, row_end);
                let body = ctx.copy_retained(
                    &payload[row_start..body_end],
                    "creo saved generated arc body",
                )?;
                ctx.reserve_vec(entities, 1, "creo saved generated entities")?;
                entities.push(FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center: [values[0], values[1], values[2]],
                    radius: values[3],
                    endpoints: [
                        [values[4], values[5], values[6]],
                        [values[7], values[8], values[9]],
                    ],
                    parameters: [values[10], values[11]],
                    body,
                    offset: row_start,
                }));
            }
            FeatureSegmentKind::Point(_) => {}
        }
    }
    Ok(())
}

fn saved_positional_body_end(payload: &[u8], row_end: usize) -> usize {
    row_end
        .checked_sub(1)
        .filter(|&before| payload.get(before) == Some(&0xe3))
        .unwrap_or(row_end)
}

fn saved_circular_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    for (kind, label) in [
        ("arc", b"\xe0\x00entity(arc)\0".as_slice()),
        ("circle", b"\xe0\x00entity(circle)\0".as_slice()),
    ] {
        let mut search = start;
        while let Some(entity_offset) = ctx.find_bytes_in(
            payload,
            label,
            search,
            end,
            "find Creo feature definition field",
        )? {
            let body_start = entity_offset + label.len();
            let body_end = ctx
                .find_bytes_in(
                    payload,
                    b"\xe0\x00entity(",
                    body_start,
                    end,
                    "find Creo feature definition field",
                )?
                .unwrap_or(end);
            let Some(entity_id) =
                named_compact_int(ctx, payload, b"\xe0\x01id\0", body_start, body_end)?
            else {
                search = body_end;
                continue;
            };
            let center = saved_named_scalars::<3>(ctx, payload, b"center", body_start, body_end, cache)?
                .unwrap_or([None; 3]);
            let radius = saved_named_scalars::<1>(ctx, payload, b"radius", body_start, body_end, cache)?
                .unwrap_or([None])[0];
            if kind == "arc" {
                let first_positional = entities.len();
                saved_positional_generated_entities(
                    ctx,
                    payload,
                    body_start,
                    body_end,
                    cache,
                    order_table,
                    segments,
                    entities,
                )?;
                let named_body_end = entities.get(first_positional).map(saved_entity_offset).map_or(body_end, |row_start| saved_positional_body_end(payload, row_start));
                let first = saved_named_scalars::<3>(ctx, payload, b"end1", body_start, body_end, cache)?
                    .unwrap_or([None; 3]);
                let second =
                    saved_named_scalars::<3>(ctx, payload, b"end2", body_start, body_end, cache)?
                        .unwrap_or([None; 3]);
                let start_parameter =
                    saved_named_scalars::<1>(ctx, payload, b"t0", body_start, body_end, cache)?
                        .unwrap_or([None])[0];
                let end_parameter =
                    saved_named_scalars::<1>(ctx, payload, b"t1", body_start, body_end, cache)?
                        .unwrap_or([None])[0];
                let body =
                    ctx.copy_retained(&payload[body_start..named_body_end], "creo saved arc body")?;
                ctx.insert_vec(entities, first_positional, FeatureSavedEntity::Arc(FeatureSavedArc {
                    entity_id,
                    center,
                    radius,
                    endpoints: [first, second],
                    parameters: [start_parameter, end_parameter],
                    body,
                    offset: entity_offset,
                }), "creo saved circular entities")?;
            } else {
                let body =
                    ctx.copy_retained(&payload[body_start..body_end], "creo saved circle body")?;
                ctx.reserve_vec(entities, 1, "creo saved circular entities")?;
                entities.push(FeatureSavedEntity::Circle(FeatureSavedCircle {
                    entity_id,
                    center,
                    radius,
                    body,
                    offset: entity_offset,
                }));
            }
            search = body_end;
        }
    }
    Ok(())
}

fn saved_conic_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    let label = b"\xe0\x00entity(conic)\0";
    let local_system_label = b"\xe0\x02local_sys\0";
    let mut search = start;
    while let Some(entity_offset) = ctx.find_bytes_in(
        payload,
        label,
        search,
        end,
        "find Creo feature definition field",
    )? {
        let body_start = entity_offset + label.len();
        let body_end = ctx
            .find_bytes_in(
                payload,
                b"\xe0\x00entity(",
                body_start,
                end,
                "find Creo feature definition field",
            )?
            .unwrap_or(end);
        let Some(entity_id) =
            named_compact_int(ctx, payload, b"\xe0\x01id\0", body_start, body_end)?
        else {
            search = body_end;
            continue;
        };
        if named_compact_int(ctx, payload, b"\xe0\x01type\0", body_start, body_end)? != Some(58) {
            search = body_end;
            continue;
        }
        let first = saved_named_scalars::<3>(ctx, payload, b"end1", body_start, body_end, cache)?
            .unwrap_or([None; 3]);
        let second = saved_named_scalars::<3>(ctx, payload, b"end2", body_start, body_end, cache)?
            .unwrap_or([None; 3]);
        let start_parameter = saved_named_scalars::<1>(ctx, payload, b"t0", body_start, body_end, cache)?
            .unwrap_or([None])[0];
        let end_parameter = saved_named_scalars::<1>(ctx, payload, b"t1", body_start, body_end, cache)?
            .unwrap_or([None])[0];
        let first_coefficient =
            saved_named_scalars::<1>(ctx, payload, b"c1", body_start, body_end, cache)?.unwrap_or([None])
                [0];
        let second_coefficient =
            saved_named_scalars::<1>(ctx, payload, b"c2", body_start, body_end, cache)?.unwrap_or([None])
                [0];
        let local_system = ctx
            .find_bytes_in(
                payload,
                local_system_label,
                body_start,
                body_end,
                "find Creo feature definition field",
            )?
            .and_then(|offset| {
                let frame_start = offset + local_system_label.len();
                scalar::decode_saved_conic_local_system_prefix(
                    &payload[frame_start..body_end],
                    cache,
                )
                .map(|(frame, _)| frame)
            });
        let body = ctx.copy_retained(&payload[body_start..body_end], "creo saved conic body")?;
        ctx.reserve_vec(entities, 1, "creo saved conic entities")?;
        entities.push(FeatureSavedEntity::Conic(FeatureSavedConic {
            entity_id,
            endpoints: [first, second],
            parameters: [start_parameter, end_parameter],
            coefficients: [first_coefficient, second_coefficient],
            local_system,
            body,
            offset: entity_offset,
        }));
        search = body_end;
    }
    Ok(())
}

fn saved_dummy_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    let label = b"\xe0\x00entity(dummy_ent)\0";
    let mut search = start;
    while let Some(entity_offset) = ctx.find_bytes_in(
        payload,
        label,
        search,
        end,
        "find Creo feature definition field",
    )? {
        let body_start = entity_offset + label.len();
        let body_end = ctx
            .find_bytes_in(
                payload,
                b"\xe0\x00entity(",
                body_start,
                end,
                "find Creo feature definition field",
            )?
            .unwrap_or(end);
        let body = ctx.copy_retained(&payload[body_start..body_end], "creo saved dummy body")?;
        ctx.reserve_vec(entities, 1, "creo saved dummy entities")?;
        entities.push(FeatureSavedEntity::Dummy(FeatureSavedDummy {
            entity_id: named_compact_int(ctx, payload, b"\xe0\x01id\0", body_start, body_end)?,
            body,
            offset: entity_offset,
        }));
        search = body_end;
    }
    Ok(())
}

/// Interpolation points a saved-spline body of `remaining` bytes can state.
///
/// A point is three coordinate lanes and every answer
/// [`scalar::decode_in_lane`] gives advances the cursor by at least one byte,
/// so `declared` points need at least `declared * 3` bytes. A declaration that
/// asks for more is not a count this body states, and the body decodes no
/// points at all. There is no floor: a body with no bytes left states no
/// point.
fn admitted_interpolation_point_count(declared: u32, remaining: usize) -> Option<usize> {
    let declared = usize::try_from(declared).ok()?;
    (declared.checked_mul(3)? <= remaining).then_some(declared)
}

fn saved_spline_entities(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    entities: &mut Vec<FeatureSavedEntity>,
) -> Result<(), CodecError> {
    const LABEL: &[u8] = b"\xe0\x00save_entity_ptr(spline)\0";
    const POINTS_LABEL: &[u8] = b"\xe0\x02i_pnts\0";
    const POINTS: &[u8] = b"\xe0\x02i_pnts\0\xf9";
    const TANGENTS_LABEL: &[u8] = b"\xe0\x02end_tangts\0";
    const TANGENTS: &[u8] = b"\xe0\x02end_tangts\0\xf9\x02\x03";
    let mut search = start;
    while let Some(entity_offset) = ctx.find_bytes_in(
        payload,
        LABEL,
        search,
        end,
        "find Creo feature definition field",
    )? {
        let body_start = entity_offset + LABEL.len();
        let body_end = ctx
            .find_bytes_in(
                payload,
                LABEL,
                body_start,
                end,
                "find Creo feature definition field",
            )?
            .unwrap_or(end);
        let points_label = ctx.find_bytes_in(
            payload,
            POINTS,
            body_start,
            body_end,
            "find Creo feature definition field",
        )?;
        let entity_id_end = points_label.unwrap_or(body_end);
        let mut declared_point_count = None;
        let mut point_count = None;
        let mut points = Vec::new();
        let mut interpolation_body_range = None;
        let mut fields_start = body_start;
        if let Some(points_label) = points_label {
            let value_start = points_label + POINTS_LABEL.len();
            let extents_start = points_label + POINTS.len();
            let (declared, dimensions_end) = psb::compact_int(payload, extents_start);
            let (coordinate_count, mut cursor) = psb::compact_int(payload, dimensions_end);
            if dimensions_end > extents_start && cursor > dimensions_end && coordinate_count == 3 {
                declared_point_count = Some(declared);
                interpolation_body_range = Some((value_start, cursor));
                point_count = payload.get(cursor..body_end).and_then(|remaining| {
                    admitted_interpolation_point_count(declared, remaining.len())
                });
                if let Some(point_count) = point_count {
                    ctx.reserve_vec(&mut points, point_count, "creo saved spline points")?;
                    let mut counted_items = 0..point_count;

                        while let Some(_) = ctx.next_charged(&mut counted_items, "creo saved spline entities traversal")? {let mut point = [0.0; 3];
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
                    interpolation_body_range = Some((value_start, cursor));
                }
            }
        }
        let interpolation_points_body = match interpolation_body_range {
            Some((body_start, body_end)) => ctx.copy_retained(
                &payload[body_start..body_end],
                "creo saved spline point body",
            )?,
            None => Vec::new(),
        };
        let endpoint_tangents = ctx
            .find_bytes_in(
                payload,
                TANGENTS,
                fields_start,
                body_end,
                "find Creo feature definition field",
            )?
            .and_then(|label| {
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
                Some((tangents, value_start, at))
            });
        let endpoint_tangents = endpoint_tangents
            .map(|(value, body_start, body_end)| -> Result<_, CodecError> {
                Ok(DecodedField {
                    value,
                    body: ctx.copy_retained(
                        &payload[body_start..body_end],
                        "creo saved spline tangent body",
                    )?,
                })
            })
            .transpose()?;
        let parameters = match point_count {
            Some(point_count) => {
                saved_spline_parameters(ctx, payload, fields_start, body_end, point_count, cache)?
            }
            None => None,
        };
        ctx.reserve_vec(entities, 1, "creo saved spline entities")?;
        entities.push(FeatureSavedEntity::Spline(FeatureSavedSpline {
            entity_id: named_compact_int(ctx, payload, b"\xe0\x01id\0", body_start, entity_id_end)?,
            declared_point_count,
            interpolation_points: points,
            interpolation_points_body,
            endpoint_tangents,
            parameters,
            offset: entity_offset,
        }));
        search = body_start;
    }
    Ok(())
}

fn saved_spline_parameters(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    point_count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<DecodedField<Vec<f64>>>, CodecError> {
    const PARAMETERS_LABEL: &[u8] = b"\xe0\x02params\0";
    const PARAMETERS: &[u8] = b"\xe0\x02params\0\xf8";
    let Some(label) = ctx.find_bytes_in(
        payload,
        PARAMETERS,
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let value_start = label + PARAMETERS_LABEL.len();
    let count_at = label + PARAMETERS.len();
    let (count, mut cursor) = psb::compact_int(payload, count_at);
    if usize::try_from(count).ok() != Some(point_count) || cursor <= count_at {
        return Ok(None);
    }
    let mut values = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "creo saved spline parameters")?;
    storage.with_storage(|| ctx.reserve_vec(&mut values, point_count, "creo saved spline parameters"))?;
    let mut counted_items = 0..count;

        while let Some(_) = ctx.next_charged(&mut counted_items, "creo saved spline parameters traversal")? {let Some((value, next)) = saved_spline_parameter(payload, cursor, cache) else {
            return Ok(None);
        };
        if next > end {
            return Ok(None);
        }
        values.push(value);
        cursor = next;
    }
    storage.commit()?;
    Ok(Some(DecodedField {
        value: values,
        body: ctx.copy_retained(
            &payload[value_start..cursor],
            "creo saved spline parameter body",
        )?,
    }))
}

fn saved_spline_parameter(
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
        // wrapping-exception: DICT prefix remapping reconstructs the low IEEE byte modulo 256
        let second = prefix.wrapping_sub(0x8b);
        return scalar::ieee7_with_prefix(
            payload,
            offset,
            if second >= 0x80 { 0x3f } else { 0x40 },
            second,
        );
    }
    if prefix == 0x2d {
        return scalar::ieee8(payload, offset, 0x40);
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

fn saved_section(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Result<Option<FeatureSavedSection>, CodecError> {
    let Some(table) = ctx.find_bytes_in(
        payload,
        b"\xe0\x00p_saved_result\0",
        start,
        end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let table_end = match ctx.find_bytes_in(
        payload,
        b"\xe0\x02local_sys\0",
        table,
        end,
        "find Creo feature definition field",
    )? {
        Some(offset) => offset,
        None => ctx
            .find_bytes_in(
                payload,
                b"\xe0\x00rigid_data\0",
                table,
                end,
                "find Creo feature definition field",
            )?
            .unwrap_or(end),
    };
    let mut entities = Vec::new();
    saved_line_entities(ctx, payload, table, table_end, cache, &mut entities)?;
    saved_circular_entities(ctx, payload, table, table_end, cache, order_table, segments, &mut entities)?;
    saved_conic_entities(ctx, payload, table, end, cache, &mut entities)?;
    saved_dummy_entities(ctx, payload, table, table_end, &mut entities)?;
    saved_spline_entities(ctx, payload, start, end, cache, &mut entities)?;
    ctx.stable_sort_by_key(
        entities.as_mut_slice(),
        saved_entity_offset,
        Ord::cmp,
        "creo saved section entities ordering",
    )?;
    Ok(Some(FeatureSavedSection {
        entities,
        offset: table,
    }))
}

fn positional_saved_section(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
    order_table: Option<&FeatureOrderTable>,
    segments: Option<&FeatureSegmentTable>,
) -> Result<Option<FeatureSavedSection>, CodecError> {
    let mut entities = Vec::new();
    saved_positional_generated_entities(ctx, payload, start, end, cache, order_table, segments, &mut entities)?;
    saved_conic_entities(ctx, payload, start, end, cache, &mut entities)?;
    ctx.stable_sort_by_key(
        entities.as_mut_slice(),
        saved_entity_offset,
        Ord::cmp,
        "creo positional saved section entities ordering",
    )?;
    let Some(offset) = entities.first().map(saved_entity_offset) else {
        return Ok(None);
    };
    Ok(Some(FeatureSavedSection { entities, offset }))
}

/// Decode full-turn termination stored inside an owned DEPDB section
/// definition. The owning current-state recipe must independently select a
/// rotational sweep.
pub(crate) fn definition_revolution_extents(
    ctx: &DecodeContext<'_>,
    definitions: &[FeatureDefinition],
    operations: &[FeatureOperation],
) -> Result<Vec<FeatureRevolutionExtent>, CodecError> {
    const FULL_TURN: &[u8] = &[
        0x83, 0xdf, 0xf6, 0xe3, 0x00, 0x00, 0xea, 0x44, 0x00, 0x00, 0xf6, 0xf6, 0xf6, 0x00, 0x00,
        0x00, 0x00,
    ];
    let mut result = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "creo revolution owner index")?;
    let mut owners = std::collections::HashSet::new();
    for operation in ctx.admit_iter(operations, "creo revolution operation traversal")? {
        if operation.recipe.resolved().is_some_and(|recipe| recipe.kind() == FeatureRecipeKind::Revolve) {
            storage.with_storage(|| ctx.insert_hash_set(&mut owners, operation.feature_id, "creo revolution owner nodes"))?;
        }
    }
    for definition in ctx.admit_iter(definitions, "creo revolution definition traversal")? {
        let Some(feature_id) = definition.identity.owner_feature_id() else {
            continue;
        };
        if !owners.contains(&feature_id) { continue; }
        for offset in ctx.admit_iter(0..definition.body.len().saturating_sub(FULL_TURN.len() - 1), "creo revolution marker traversal")? {
            let window = &definition.body[offset..offset + FULL_TURN.len()];
            if window == FULL_TURN {
                ctx.reserve_vec(&mut result, 1, "creo definition revolution extents")?;
                result.push(FeatureRevolutionExtent {
                    feature_id,
                    offset: definition.offset + offset + 6,
                });
            }
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo definition revolution extents result ordering",
    )?;
    Ok(result)
}

fn definitions_in_ranges(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    starts: &[DefinitionStart],
    selected_offsets: Option<&BTreeSet<usize>>,
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut result = Vec::new();
    let mut replay = ReplaySchemas::default();
    let (mut schema_ends, _storage) = ctx.temporary_vec(starts.len(), "creo definition schema ends")?;
    let mut next_schema = payload.len();
    for entry in ctx.admit_iter(starts, "creo schema range traversal")?.rev() {
        schema_ends.push(next_schema);
        if !entry.positional { next_schema = entry.offset; }
    }
    for (
        index,
        &DefinitionStart {
            offset: start,
            positional,
            ..
        },
    ) in ctx.admit_iter(starts, "creo definition range traversal")?.enumerate()
    {
        let end = starts
            .get(index + 1)
            .map_or(payload.len(), |entry| entry.offset);
        let schema_end = schema_ends[starts.len() - index - 1];
        let selected = match selected_offsets {
            Some(offsets) => ctx.contains_btree_set(offsets, &start, "creo definition output selection")?,
            None => true,
        };
        if !selected && positional { continue; }
        let range = DefinitionRange { start: starts[index], end, schema_end };
        let definition = if selected {
            definition_in_range(ctx, payload, range, &cache, &mut replay, DefinitionRead::Output)?
        } else {
            let mut storage = ctx.reserve_scoped(0, "creo inherited schema scratch")?;
            storage.with_storage(|| definition_in_range(ctx, payload, range, &cache, &mut replay, DefinitionRead::Schema))?
        };
        if let Some(definition) = definition {
            ctx.push_vec(&mut result, definition, "creo parsed feature definitions")?;
        }
    }
    Ok(result)
}


#[derive(Default)]
struct ReplaySchemas {
    dimension: Option<u32>,
    variable: Option<u32>,
    relation: Option<u32>,
    skamp: Option<u32>,
    triples: Option<u32>,
    trim_entity: Option<TrimTableClasses>,
    trim_vertex: Option<TrimTableClasses>,
    order: Option<u32>,
}

enum DefinitionRead { Output, Schema }

struct DefinitionRange {
    start: DefinitionStart,
    end: usize,
    schema_end: usize,
}

fn definition_in_range(ctx: &DecodeContext<'_>, payload: &[u8], range: DefinitionRange, cache: &scalar::ScalarCache, replay: &mut ReplaySchemas, mode: DefinitionRead) -> Result<Option<FeatureDefinition>, CodecError> {
    let selected = matches!(mode, DefinitionRead::Output);
    let DefinitionRange { start: DefinitionStart { offset: start, id, owner_override, positional }, end, schema_end } = range;
        let mut parameter_frames = Vec::new();
        if selected {
        for &(label, kind) in &[
            (
                b"local_sys".as_slice(),
                FeatureParameterFrameKind::LocalSystem,
            ),
            (b"transf".as_slice(), FeatureParameterFrameKind::Transform),
        ] {
            let needle_len = label.len() + 4;
            let mut from = start;
            while let Some(field_offset) = ctx.find_by(from..end.saturating_sub(needle_len - 1), |&offset| {
                let window = &payload[offset..offset + needle_len];
                Ok(window.get(..label.len()) == Some(label) && window.get(label.len()..) == Some(b"\0\xf9\x04\x03"))
            }, "creo parameter frame search")? {
                let body_start = field_offset + needle_len;
                let body_end = ctx.position_by(&payload[body_start..end], |byte| Ok(*byte == psb::token::NAMED_RECORD), "creo parameter frame body end")?
                    .map_or(end, |relative| body_start + relative);
                let body = ctx.copy_retained(
                    &payload[body_start..body_end],
                    "creo feature parameter frame body",
                )?;
                ctx.reserve_vec(&mut parameter_frames, 1, "creo feature parameter frames")?;
                parameter_frames.push(FeatureParameterFrame {
                    kind,
                    decoded_values: scalar::decode_feature_local_system_slots(&body, cache),
                    body,
                    offset: field_offset,
                });
                from = body_start;
            }
        }
        ctx.stable_sort_by(
            parameter_frames.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo definitions in ranges parameter frames ordering",
        )?;
        }
        let mut outlines = Vec::new();
        if selected {
        if let Some(info) = ctx.find_bytes_in(
            payload,
            b"\xe0\x00feat_outl_info\0",
            start,
            end,
            "find Creo feature definition field",
        )? {
            if let Some(label) = ctx.find_bytes_in(
                payload,
                b"outline\0\xf9\x02\x03",
                info,
                end,
                "find Creo feature definition field",
            )? {
                let scalar_start = label + b"outline\0\xf9\x02\x03".len();
                let local_scalars = outline_scalars(ctx, &payload[scalar_start..end], cache)?;
                ctx.reserve_vec(&mut outlines, 1, "creo feature outlines")?;
                outlines.push(FeatureOutline {
                    phase: OutlinePhase::PreRollback,
                    local_scalars,
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
                let Some(label_offset) = ctx.find_bytes_in(
                    payload,
                    label,
                    info,
                    end,
                    "find Creo feature definition field",
                )?
                else {
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
                let local_scalars = outline_scalars(ctx, &payload[after_ref + 4..end], cache)?;
                ctx.reserve_vec(&mut outlines, 1, "creo feature outlines")?;
                outlines.push(FeatureOutline {
                    phase,
                    local_scalars,
                    offset: label_offset,
                });
            }
        }
        ctx.stable_sort_by(
            outlines.as_mut_slice(),
            |value| &value.offset,
            Ord::cmp,
            "creo definitions in ranges outlines ordering",
        )?;
        }
        let variables = match variable_table(ctx, payload, start, end, cache)? {
            Some(variables) => Some(variables),
            None if positional => match replay.variable {
                Some(table_class) => {
                    positional_variable_table(ctx, payload, start, end, table_class, cache)?
                }
                None => None,
            },
            None => None,
        };
        if !positional {
            replay.variable = variables.as_ref().and_then(|table| table.entity_ref);
        }
        let segments = if selected { match segment_table(ctx, payload, start, end)? {
            Some(segments) => Some(segments),
            None if positional => positional_segment_table(ctx, payload, start, end)?,
            None => None,
        } } else { None };
        let trim_entities = if selected { match trim_entity_table(ctx, payload, start, end)? {
            Some(table) => Some(table),
            None if positional => match replay.trim_entity {
                Some(classes) => positional_trim_entity_table(
                    ctx,
                    payload,
                    start,
                    end,
                    classes,
                    replay.trim_vertex.map(|classes| classes.table),
                )?,
                None => None,
            },
            None => None,
        } } else { None };
        if !positional {
            replay.trim_entity = trim_table_header(ctx, payload, b"ent_tab\0", start, end)?
                .map(|header| header.classes);
        }
        let trim_vertices = if selected { match trim_vertex_table(
            ctx,
            payload,
            start,
            end,
            segments.as_ref(),
            variables.as_ref(),
        )? {
            Some(table) => Some(table),
            None if positional => match replay.trim_vertex {
                Some(classes) => positional_trim_vertex_table(
                    ctx,
                    payload,
                    start,
                    end,
                    classes,
                    segments.as_ref(),
                    variables.as_ref(),
                )?,
                None => None,
            },
            None => None,
        } } else { None };
        if !positional {
            replay.trim_vertex =
                trim_table_header(ctx, payload, b"vert_tab\0", start, end)?
                    .map(|header| header.classes);
        }
        let order_table = match order_table(ctx, payload, start, end)? {
            Some(table) => Some(table),
            None if positional => match replay.order {
                Some(class) => positional_order_table(ctx, payload, start, end, class)?,
                None => None,
            },
            None => None,
        };
        if !positional {
            replay.order = order_table.as_ref().and_then(|table| table.entity_ref);
        }
        let section_3d = if selected { match section_3d(ctx, payload, start, end)? {
            Some(section) => Some(section),
            None if positional => positional_section_3d(ctx, payload, start, end)?,
            None => None,
        } } else { None };
        let dimensions = if let Some(table) = dimension_table(ctx, payload, start, end, cache)? {
            Some(table)
        } else if positional {
            match replay.dimension {
                Some(table_class) => {
                    match positional_dimension_table(ctx, payload, start, end, table_class, cache)?
                    {
                        Some(table) => Some(table),
                        None => self_described_positional_dimension_table(
                            ctx, payload, start, end, cache,
                        )?,
                    }
                }
                None => {
                    self_described_positional_dimension_table(ctx, payload, start, end, cache)?
                }
            }
        } else {
            None
        };
        if !positional {
            replay.dimension = dimensions.as_ref().and_then(|table| table.entity_ref);
        }
        let mut relations = match relation_table(ctx, payload, start, end)? {
            Some(table) => Some(table),
            None if positional => match replay.relation {
                Some(table_class) => {
                    positional_relation_table(ctx, payload, start, end, table_class)?
                }
                None => None,
            },
            None => None,
        };
        if !positional {
            replay.relation = relations.as_ref().and_then(|table| table.entity_ref);
            replay.skamp =
                named_array_class(ctx, payload, b"skamp_ptr\0", start, schema_end)?;
            replay.triples =
                named_array_class(ctx, payload, b"triples_ptr\0", start, schema_end)?;
        } else if let Some(table) = &mut relations {
            if table
                .skamps
                .as_ref()
                .and_then(SolverSubtable::header)
                .is_none()
            {
                if let Some(header) =
                    named_solver_table_header(ctx, payload, b"skamp_ptr\0", start, end)?
                {
                    table.skamps = Some(SolverSubtable::Declared {
                        header,
                        rows: feature_skamps(ctx, payload, start, end)?,
                    });
                } else {
                    table.skamps = match replay.skamp {
                        Some(table_class) => positional_feature_skamps(ctx, payload, start, end, table_class)?,
                        None => None,
                    };
                }
            }
            if table
                .triples
                .as_ref()
                .and_then(SolverSubtable::header)
                .is_none()
            {
                if let Some(header) =
                    named_solver_table_header(ctx, payload, b"triples_ptr\0", start, end)?
                {
                    table.triples = Some(SolverSubtable::Declared {
                        header,
                        rows: feature_relation_triples(ctx, payload, start, end)?,
                    });
                } else {
                    table.triples = match replay.triples {
                        Some(table_class) => positional_relation_triples(ctx, payload, start, end, table_class)?,
                        None => None,
                    };
                }
            }
        }
        if !selected { return Ok(None); }
        let named_saved_section = saved_section(
            ctx,
            payload,
            start,
            end,
            cache,
            order_table.as_ref(),
            segments.as_ref(),
        )?;
        let saved_section = match named_saved_section {
            Some(section) => Some(section),
            None if positional => positional_saved_section(
                ctx,
                payload,
                start,
                end,
                cache,
                order_table.as_ref(),
                segments.as_ref(),
            )?,
            None => None,
        };
        let owner_feature_id = if owner_override.is_some() { owner_override } else {
            let mut offsets = start..end.saturating_sub(2 + b"feat_id".len());
            let first = ctx.find_map(&mut offsets, |offset| Ok(contextual_reference(payload, offset, end, b"feat_id", b"gsec2d_ptr")), "creo contextual owner search")?;
            match first {
                Some(first) if ctx.all_by(offsets, |offset| Ok(contextual_reference(payload, offset, end, b"feat_id", b"gsec2d_ptr").is_none_or(|id| id == first)), "creo contextual owner agreement")? => Some(first),
                _ => None,
            }
        };
        let body = ctx.copy_retained(&payload[start..end], "creo feature definition body")?;
        Ok(Some(FeatureDefinition {
            identity: DefinitionIdentity::Parsed {
                schema_id: id,
                owner_feature_id,
            },
            body,
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
        }))
}

fn contextual_reference(payload: &[u8], record_start: usize, end: usize, field: &[u8], following_record: &[u8]) -> Option<u32> {
    let needle_len = 2 + field.len() + 1;
    let window = payload.get(record_start..record_start.checked_add(needle_len)?)?;
    if window.get(..2) != Some(&[psb::token::NAMED_RECORD, 1])
        || window.get(2..2 + field.len()) != Some(field) || window.last() != Some(&0) { return None; }
    let value_start = record_start + needle_len;
    let (value, after_value) = psb::reference_id(payload, value_start).ok()?;
    let following_end = after_value.checked_add(3 + following_record.len())?;
    (following_end <= end
        && payload.get(after_value..after_value + 2) == Some(&[psb::token::NAMED_RECORD, 0])
        && payload.get(after_value + 2..following_end - 1) == Some(following_record)
        && payload.get(following_end - 1) == Some(&0)).then_some(value)
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
#[derive(Debug, Clone, Copy)]
struct DefinitionStart {
    offset: usize,
    id: Option<NonZeroU32>,
    owner_override: Option<u32>,
    positional: bool,
}

impl cadmpeg_core::decode::cost::DecodeCost for DefinitionStart {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.offset,
                &self.id,
                &self.owner_override,
                &self.positional,
            ),
            ctx,
            operation,
        )
    }
}

fn definition_starts(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<DefinitionStart>, CodecError> {
    const PREFIX: &[u8] = b"feat_defs_";
    let mut starts = Vec::new();
    for offset in ctx.admit_iter(0..payload.len(), "creo definition label traversal")? {
        if payload.get(offset..offset + PREFIX.len()) != Some(PREFIX) {
            continue;
        }
        let digits_start = offset + PREFIX.len();
        let Some(nul_relative) = ctx.position_by(&payload[digits_start..], |byte| Ok(*byte == 0), "creo definition label terminator")? else {
            continue;
        };
        let digits = &payload[digits_start..digits_start + nul_relative];
        if digits.is_empty() || !ctx.all_by(digits, |byte| Ok(byte.is_ascii_digit()), "creo definition label digits")? {
            continue;
        }
        let Ok(digits) = ctx.validate_utf8(digits, "creo UTF-8 validation")? else {
            continue;
        };
        let Ok(id) = ctx.parse_text::<u32>(digits, "creo scalar text parsing")? else {
            continue;
        };
        ctx.reserve_vec(&mut starts, 1, "creo feature definition starts")?;
        starts.push(DefinitionStart {
            offset,
            id: NonZeroU32::new(id),
            owner_override: None,
            positional: false,
        });
    }
    let labeled_count = starts.len();
    for index in ctx.admit_iter(0..labeled_count, "creo contextual definition range traversal")? {
        let start = starts[index].offset;
        let end = if index + 1 < labeled_count {
            starts[index + 1].offset
        } else {
            payload.len()
        };
        for offset in ctx.admit_iter(start..end.saturating_sub(2 + b"feat_id".len()), "creo contextual definition traversal")? {
            let Some(owner) = contextual_reference(payload, offset, end, b"feat_id", b"ref_model_info") else { continue; };
            ctx.reserve_vec(&mut starts, 1, "creo feature definition starts")?;
            starts.push(DefinitionStart {
                offset,
                id: NonZeroU32::new(owner),
                owner_override: Some(owner),
                positional: true,
            });
        }
    }
    ctx.sort_unstable_by(
        &mut starts,
        |value| &value.offset,
        Ord::cmp,
        "creo feature definition starts sort",
    )?;
    ctx.dedup_by_key(
        &mut starts,
        |entry| Ok(entry.offset),
        "creo definition starts starts deduplication",
    )?;
    Ok(starts)
}

fn depdb_gsec2d_starts(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<DefinitionStart>, CodecError> {
    const GSEC: &[u8] = b"gsec2d_ptr\0";
    const NAME: &[u8] = b"name\0S2D";
    const NAME_WINDOW: usize = 128;
    let mut starts = Vec::new();
    for start in ctx.admit_iter(0..payload.len().saturating_sub(GSEC.len() - 1), "creo DEPDB section marker traversal")? {
        let window = &payload[start..start + GSEC.len()];
        if window != GSEC {
            continue;
        }
        let search_end = start
            .checked_add(NAME_WINDOW)
            .map_or(payload.len(), |window_end| window_end.min(payload.len()));
        let Some(name_offset) = ctx.find_bytes_in(
            payload,
            NAME,
            start,
            search_end,
            "find Creo feature definition field",
        )?
        else {
            continue;
        };
        let digits_start = name_offset + NAME.len();
        let Some(nul) = payload[digits_start..search_end].iter().position(|byte| *byte == 0) else { continue; };
        let digits = &payload[digits_start..digits_start + nul];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) { continue; }
        let Ok(text) = ctx.validate_utf8(digits, "creo UTF-8 validation")? else { continue; };
        let Ok(id) = ctx.parse_text::<u32>(text, "creo scalar text parsing")? else { continue; };
        let candidate = DefinitionStart { offset: start, id: NonZeroU32::new(id), owner_override: None, positional: false };
        ctx.reserve_vec(&mut starts, 1, "creo DEPDB section starts")?;
        starts.push(candidate);
    }
    Ok(starts)
}

/// Decode `FeatDefs` feature-definition records and their `f9 04 03`
/// definition-space parameter frames.
pub(crate) fn definitions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo definition start scratch")?;
    let mut starts = storage.with_storage(|| definition_starts(ctx, payload))?;
    let mut retained_offsets = BTreeSet::new();
    for DefinitionStart { offset, .. } in ctx.admit_iter(&starts, "creo retained definition index traversal")? {
        storage.with_storage(|| ctx.insert_btree_set(
            &mut retained_offsets,
            *offset,
            "creo retained definition offset nodes",
        ))?;
    }
    let (mut labeled, mut labeled_storage) = ctx.temporary_vec(starts.len(), "creo inherited definition index")?;
    for entry in ctx.admit_iter(&starts, "creo inherited definition index traversal")? {
        if !entry.positional { labeled.push(*entry); }
    }
    labeled_storage.with_storage(|| ctx.stable_sort_by_key(&mut labeled, |entry| entry.offset, Ord::cmp, "creo inherited definition ordering"))?;
    let replay_markers = storage.with_storage(|| s2d_replay_starts(ctx, payload))?;
    let claimed_markers = storage.with_storage(|| claimed_s2d_replay_markers(ctx, payload, &starts, &replay_markers))?;
    for offset in ctx.admit_iter(replay_markers, "creo replay range traversal")? {
        if !ctx.contains_btree_set(&claimed_markers, &offset, "creo claimed marker membership")? {
            let id = inherited_definition_id(ctx, &labeled, offset)?;
            storage.with_storage(|| ctx.reserve_vec(&mut starts, 1, "creo definition replay starts"))?;
            starts.push(DefinitionStart {
                offset,
                id,
                owner_override: None,
                positional: true,
            });
        }
    }
    storage.with_storage(|| ctx.sort_unstable_by(
        &mut starts,
        |value| &value.offset,
        Ord::cmp,
        "creo feature definition starts sort",
    ))?;
    storage.with_storage(|| ctx.dedup_by_key(
        &mut starts,
        |entry| Ok(entry.offset),
        "creo definitions starts deduplication",
    ))?;
    definitions_in_ranges(ctx, payload, &starts, Some(&retained_offsets))
}

/// Decode labelled and positional feature definitions embedded directly in a
/// DEPDB section. A labelled `gsec2d_ptr` definition supplies the table schema
/// for its following positional `S2D` instances.
pub(crate) fn depdb_definitions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo definition start scratch")?;
    let mut starts = storage.with_storage(|| definition_starts(ctx, payload))?;
    let depdb_starts = storage.with_storage(|| depdb_gsec2d_starts(ctx, payload))?;
    storage.with_storage(|| ctx.reserve_vec(
        &mut starts,
        depdb_starts.len(),
        "creo DEPDB definition starts",
    ))?;
    starts.extend(ctx.admit_iter(depdb_starts, "creo DEPDB start extension")?);
    let (mut labeled, mut labeled_storage) = ctx.temporary_vec(starts.len(), "creo inherited definition index")?;
    for entry in ctx.admit_iter(&starts, "creo inherited definition index traversal")? {
        if !entry.positional { labeled.push(*entry); }
    }
    labeled_storage.with_storage(|| ctx.stable_sort_by_key(&mut labeled, |entry| entry.offset, Ord::cmp, "creo inherited definition ordering"))?;
    let replay_markers = storage.with_storage(|| s2d_replay_starts(ctx, payload))?;
    let claimed_markers = storage.with_storage(|| claimed_s2d_replay_markers(ctx, payload, &starts, &replay_markers))?;
    for offset in ctx.admit_iter(replay_markers, "creo replay range traversal")? {
        if !ctx.contains_btree_set(&claimed_markers, &offset, "creo claimed marker membership")? {
            let id = inherited_definition_id(ctx, &labeled, offset)?;
            storage.with_storage(|| ctx.reserve_vec(&mut starts, 1, "creo definition replay starts"))?;
            starts.push(DefinitionStart {
                offset,
                id,
                owner_override: None,
                positional: true,
            });
        }
    }
    storage.with_storage(|| ctx.sort_unstable_by(
        &mut starts,
        |value| &value.offset,
        Ord::cmp,
        "creo feature definition starts sort",
    ))?;
    storage.with_storage(|| ctx.dedup_by_key(
        &mut starts,
        |entry| Ok(entry.offset),
        "creo depdb definitions starts deduplication",
    ))?;
    definitions_in_ranges(ctx, payload, &starts, None)
}

fn s2d_replay_starts(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Vec<usize>, CodecError> {
    const PREFIX: &[u8] = b"\xe3S2D";
    let candidates = ctx.admit_iter(0..payload.len().saturating_sub(PREFIX.len() - 1), "creo replay marker traversal")?
        .filter_map(|offset| {
            let window = &payload[offset..offset + PREFIX.len()];
            if window != PREFIX {
                return None;
            }
            let suffix = payload.get(offset + PREFIX.len()..)?;
            let nul = suffix.iter().take(12).position(|byte| *byte == 0)?;
            (nul > 0 && suffix[..nul].iter().all(u8::is_ascii_digit)).then_some(offset)
        });
    let mut starts = Vec::new();
    for offset in candidates {
        ctx.reserve_vec(&mut starts, 1, "creo S2D replay starts")?;
        starts.push(offset);
    }
    Ok(starts)
}

fn inherited_definition_id(ctx: &DecodeContext<'_>, labeled: &[DefinitionStart], replay_offset: usize) -> Result<Option<NonZeroU32>, CodecError> {
    let index = ctx.partition_point(labeled, |entry| Ok(entry.offset < replay_offset), "creo inherited definition lookup")?;
    Ok(index.checked_sub(1).and_then(|index| labeled[index].id))
}

fn claimed_s2d_replay_markers(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    starts: &[DefinitionStart],
    replay_markers: &[usize],
) -> Result<BTreeSet<usize>, CodecError> {
    let mut markers = BTreeSet::new();
    for (index, entry) in ctx.admit_iter(starts, "creo claimed marker range traversal")?.enumerate() {
        if !entry.positional { continue; }
        let end = starts.get(index + 1).map_or(payload.len(), |entry| entry.offset);
        let marker_index = ctx.partition_point(replay_markers, |marker| Ok(*marker < entry.offset), "creo claimed marker lookup")?;
        if let Some(&marker) = replay_markers.get(marker_index).filter(|&&marker| marker < end) {
            ctx.insert_btree_set(&mut markers, marker, "creo claimed S2D marker nodes")?;
        }
    }
    Ok(markers)
}

/// Decode unlabeled positional `S2D` replay instances without assigning an
/// owner. Ownership remains absent unless an independent entity join proves it.
pub(crate) fn positional_replay_definitions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo definition start scratch")?;
    let mut starts = storage.with_storage(|| definition_starts(ctx, payload))?;
    let (mut labeled, mut labeled_storage) = ctx.temporary_vec(starts.len(), "creo inherited definition index")?;
    for entry in ctx.admit_iter(&starts, "creo inherited definition index traversal")? {
        if !entry.positional { labeled.push(*entry); }
    }
    labeled_storage.with_storage(|| ctx.stable_sort_by_key(&mut labeled, |entry| entry.offset, Ord::cmp, "creo inherited definition ordering"))?;
    let replay_markers = storage.with_storage(|| s2d_replay_starts(ctx, payload))?;
    let claimed_markers = storage.with_storage(|| claimed_s2d_replay_markers(ctx, payload, &starts, &replay_markers))?;
    let mut pending_offsets = BTreeSet::new();
    for offset in ctx.admit_iter(replay_markers, "creo replay range traversal")? {
        if !ctx.contains_btree_set(&claimed_markers, &offset, "creo claimed marker membership")? {
            storage.with_storage(|| ctx.insert_btree_set(
                &mut pending_offsets,
                offset,
                "creo pending S2D marker nodes",
            ))?;
            let id = inherited_definition_id(ctx, &labeled, offset)?;
            storage.with_storage(|| ctx.reserve_vec(&mut starts, 1, "creo definition replay starts"))?;
            starts.push(DefinitionStart {
                offset,
                id,
                owner_override: None,
                positional: true,
            });
        }
    }
    storage.with_storage(|| ctx.sort_unstable_by(
        &mut starts,
        |value| &value.offset,
        Ord::cmp,
        "creo feature definition starts sort",
    ))?;
    storage.with_storage(|| ctx.dedup_by_key(
        &mut starts,
        |entry| Ok(entry.offset),
        "creo positional replay definitions starts deduplication",
    ))?;
    definitions_in_ranges(ctx, payload, &starts, Some(&pending_offsets))
}

/// Decode one standalone DEPDB `gsec2d_ptr` section with an optional proven owner.
/// The sole `gsec2d_ptr` starts the range, so no contextual owner pair occurs inside it.
pub(crate) fn depdb_section_definition(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    owner_feature_id: Option<u32>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    const GSEC: &[u8] = b"gsec2d_ptr\0";
    const NAME: &[u8] = b"name\0S2D";
    const NAME_WINDOW: usize = 128;
    const PREFIX: &[u8] = b"feat_defs_";
    let mut offsets = 0..payload.len().saturating_sub(GSEC.len() - 1);
    let Some(start) = ctx.find_by(&mut offsets, |&offset| Ok(&payload[offset..offset + GSEC.len()] == GSEC), "creo standalone section search")? else { return Ok(None); };
    if ctx.any_by(offsets, |offset| Ok(&payload[offset..offset + GSEC.len()] == GSEC), "creo standalone section uniqueness")? { return Ok(None); }
    let name_search_end = start
        .checked_add(NAME_WINDOW)
        .map_or(payload.len(), |window_end| window_end.min(payload.len()));
    let Some(name_offset) = ctx.find_bytes_in(
        payload,
        NAME,
        start,
        name_search_end,
        "find Creo feature definition field",
    )?
    else {
        return Ok(None);
    };
    let name = name_offset + NAME.len();
    let Some(nul) = payload[name..name_search_end].iter().position(|byte| *byte == 0) else { return Ok(None); };
    let digits = &payload[name..name + nul];
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) { return Ok(None); }
    let Ok(text) = ctx.validate_utf8(digits, "creo UTF-8 validation")? else { return Ok(None); };
    let Ok(section_id) = ctx.parse_text::<u32>(text, "creo scalar text parsing")? else { return Ok(None); };
    let end = ctx
        .find_bytes_in(
            payload,
            PREFIX,
            start + GSEC.len(),
            payload.len(),
            "find Creo feature definition field",
        )?
        .unwrap_or(payload.len());
    Ok(definitions_in_ranges(
        ctx,
        &payload[..end],
        &[DefinitionStart {
            offset: start,
            id: NonZeroU32::new(section_id),
            owner_override: owner_feature_id,
            positional: true,
        }],
        None,
    )?
    .pop())
}

/// Bind an omitted owner through the section's unique generated datum entry.
pub(crate) fn bind_definition_owners(
    ctx: &DecodeContext<'_>,
    mut definitions: Vec<FeatureDefinition>,
    geometry_tables: &[FeatureGeometryTable],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo datum owner index")?;
    let mut owners = None;
    for definition in ctx.admit_iter(&mut definitions, "creo datum definition traversal")? {
        if definition.identity.owner_feature_id().is_some() { continue; }
        let Some(plane) = definition.section_3d.as_ref().and_then(|section| section.sketch_plane_entity_id) else { continue; };
        if owners.is_none() {
    let mut index = std::collections::HashMap::<u32, Option<u32>>::new();
    for table in ctx.admit_iter(geometry_tables, "creo datum owner table traversal")? {
        let Some(ids) = table.kind.datum_ids() else { continue; };
        for id in ctx.admit_iter(ids, "creo datum owner ID traversal")? {
            storage.with_storage(|| ctx.entry_hash_map(&mut index, *id, "creo datum owner nodes"))?
                .and_modify(|owner| { if *owner != Some(table.feature_id) { *owner = None; } })
                .or_insert(Some(table.feature_id));
        }
    }
            owners = Some(index);
        }
        let Some(owners) = owners.as_ref() else { continue; };
        if let Some(owner) = owners.get(&plane).copied().flatten() {
            definition.identity = DefinitionIdentity::Parsed { schema_id: definition.identity.schema_id(), owner_feature_id: Some(owner) };
        }
    }
    Ok(definitions)
}

/// Bind instantiated saved sections through the exact trimmed source set.
pub(crate) fn bind_trimmed_definition_owners(
    ctx: &DecodeContext<'_>,
    mut definitions: Vec<FeatureDefinition>,
    entity_tables: &[FeatureEntityTable],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo trimmed owner scratch")?;
    let mut claimed_owner_ids = BTreeSet::new();
    for owner in ctx.admit_iter(&definitions, "creo trimmed claimed definition traversal")?.filter_map(|definition| definition.identity.owner_feature_id()) {
        storage.with_storage(|| ctx.insert_btree_set(&mut claimed_owner_ids, owner, "creo trimmed claimed owner nodes"))?;
    }
    let mut sources = None;
    let mut candidates = Vec::new();
    for definition in ctx.admit_iter(&definitions, "creo trimmed definition candidate traversal")? {
        let external_ids = unique_trimmed_external_ids(ctx, definition)?;
        let mut owners = BTreeSet::new();
        if definition.identity.owner_feature_id().is_none() && !external_ids.is_empty() {
            if sources.is_none() {
                sources = Some(storage.with_storage(|| generated_owner_sources(ctx, entity_tables, &claimed_owner_ids))?);
            }
            let Some(source_rows) = sources.as_ref() else { continue; };
            for (owner, source_ids) in ctx.admit_iter(source_rows, "creo trimmed source set traversal")? {
                if source_ids.len() == external_ids.len()
                    && ctx.all_by(source_ids.iter().zip(external_ids), |(a,b)| Ok(a == b), "creo trimmed source set agreement")? {
                    storage.with_storage(|| ctx.insert_btree_set(&mut owners, *owner, "creo trimmed owner candidate nodes"))?;
                }
            }
        }
        storage.with_storage(|| ctx.push_vec(&mut candidates, owners, "creo trimmed owner candidate rows"))?;
    }
    let mut owner_counts = std::collections::HashMap::new();
    for owners in ctx.admit_iter(&candidates, "creo trimmed candidate row traversal")? {
        for owner in ctx.admit_iter(owners, "creo trimmed candidate owner traversal")? {
            *storage.with_storage(|| ctx.entry_hash_map(&mut owner_counts, *owner, "creo trimmed owner count nodes"))?.or_insert(0_usize) += 1;
        }
    }
    for (definition, owners) in ctx.admit_iter(&mut definitions, "creo trimmed bound definition traversal")?.zip(candidates) {
        // A singleton tree has a constant-size first-element lookup.
        if owners.len() != 1 { continue; }
        let Some(&owner) = owners.first() else { continue; };
        if owner_counts.get(&owner) != Some(&1) { continue; }
        definition.identity = DefinitionIdentity::Parsed { schema_id: definition.identity.schema_id(), owner_feature_id: Some(owner) };
    }
    Ok(definitions)
}

/// Bind unlabeled positional definitions by exact trim set or order subset.
pub(crate) fn bind_replay_definition_owners(
    ctx: &DecodeContext<'_>,
    mut definitions: Vec<FeatureDefinition>,
    entity_tables: &[FeatureEntityTable],
    claimed_owner_ids: &BTreeSet<u32>,
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo replay owner scratch")?;
    let mut sources = None;
    let mut candidates = Vec::new();
    for definition in ctx.admit_iter(&definitions, "creo replay definition candidate traversal")? {
        let mut owners = BTreeSet::new();
        if definition.identity.owner_feature_id().is_none() {
            let trimmed = unique_trimmed_external_ids(ctx, definition)?;
            let order = definition.order_table.as_ref().map(|table| &table.rows);
            if !trimmed.is_empty() || order.is_some_and(|rows| !rows.is_empty()) {
                if sources.is_none() {
                    sources = Some(storage.with_storage(|| generated_owner_sources(ctx, entity_tables, claimed_owner_ids))?);
                }
                let Some(source_rows) = sources.as_ref() else { continue; };
                if !trimmed.is_empty() {
                    for (owner, ids) in ctx.admit_iter(source_rows, "creo replay exact source traversal")? {
                        if ids.len() == trimmed.len() && ctx.all_by(ids.iter().zip(trimmed), |(a,b)| Ok(a == b), "creo replay exact source agreement")? {
                            storage.with_storage(|| ctx.insert_btree_set(&mut owners, *owner, "creo replay exact owner nodes"))?;
                        }
                    }
                }
                if owners.is_empty() {
                    if let Some(order) = order {
                        for (owner, ids) in ctx.admit_iter(source_rows, "creo replay subset source traversal")? {
                            if !ids.is_empty() && ctx.all_by(ids, |id| Ok(order.contains_external_id(*id)), "creo replay subset membership")? {
                                storage.with_storage(|| ctx.insert_btree_set(&mut owners, *owner, "creo replay subset owner nodes"))?;
                            }
                        }
                    }
                }
            }
        }
        storage.with_storage(|| ctx.push_vec(&mut candidates, owners, "creo replay owner candidate rows"))?;
    }
    let mut owner_counts = std::collections::HashMap::new();
    for owners in ctx.admit_iter(&candidates, "creo replay candidate row traversal")? {
        for owner in ctx.admit_iter(owners, "creo replay candidate owner traversal")? {
            *storage.with_storage(|| ctx.entry_hash_map(&mut owner_counts, *owner, "creo replay owner count nodes"))?.or_insert(0_usize) += 1;
        }
    }
    for (definition, owners) in ctx.admit_iter(&mut definitions, "creo replay bound definition traversal")?.zip(candidates) {
        if owners.len() != 1 { continue; }
        let Some(&owner) = owners.first() else { continue; };
        if owner_counts.get(&owner) != Some(&1) { continue; }
        definition.identity = DefinitionIdentity::BoundOwner { schema_id: definition.identity.schema_id(), owner_feature_id: owner };
    }
    Ok(definitions)
}

fn generated_owner_sources(
    ctx: &DecodeContext<'_>, tables: &[FeatureEntityTable], claimed: &BTreeSet<u32>,
) -> Result<Vec<(u32, BTreeSet<u32>)>, CodecError> {
    let mut sources = Vec::new();
    for table in ctx.admit_iter(tables, "creo generated owner table traversal")? {
        if ctx.contains_btree_set(claimed, &table.feature_id, "creo generated owner claimed lookup")? { continue; }
        let ids = generated_class_200_source_entity_ids(ctx, table)?;
        ctx.push_vec(&mut sources, (table.feature_id, ids), "creo generated owner source rows")?;
    }
    Ok(sources)
}

fn unique_trimmed_external_ids<'a>(ctx: &DecodeContext<'_>, definition: &'a FeatureDefinition) -> Result<&'a [u32], CodecError> {
    Ok(match definition.trim_entities.as_ref() {
        Some(table) if table.has_unique_external_ids(ctx)? => table.solved_external_ids.as_slice(),
        _ => &[],
    })
}

/// Bind section definitions by the consecutive recipe, datum and plane chain.
pub(crate) fn bind_section_owners(
    ctx: &DecodeContext<'_>, mut definitions: Vec<FeatureDefinition>, operations: &[FeatureOperation], section_ranges: &[(usize, usize)],
) -> Result<Vec<FeatureDefinition>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo section owner scratch")?;
    let mut claimed = BTreeSet::new();
    for owner in ctx.admit_iter(&definitions, "creo section claimed definition traversal")?.filter_map(|definition| definition.identity.owner_feature_id()) {
        storage.with_storage(|| ctx.insert_btree_set(&mut claimed, owner, "creo section claimed owner nodes"))?;
    }
    let mut planes = BTreeMap::new();
    let mut eligible = Vec::new();
    for definition in ctx.admit_iter(&definitions, "creo section eligible definition traversal")? {
        let in_range = definition.identity.owner_feature_id().is_none() && ctx.any_by(section_ranges, |(start,end)| Ok(definition.offset >= *start && definition.offset < *end), "creo section range search")?;
        storage.with_storage(|| ctx.push_vec(&mut eligible, in_range, "creo section eligibility rows"))?;
        if in_range {
            if let Some(plane) = definition.section_3d.as_ref().and_then(|section| section.sketch_plane_entity_id) {
                *storage.with_storage(|| ctx.entry_btree_map(&mut planes, plane, "creo section plane count nodes"))?.or_insert(0_usize) += 1;
            }
        }
    }
    let mut ordered = storage.with_storage(|| ctx.collect_vec(operations.iter(), "creo section ordered operations"))?;
    ctx.stable_sort_by(ordered.as_mut_slice(), |operation| &operation.offset, Ord::cmp, "creo bind section owners ordered operations ordering")?;
    let mut pairs = std::collections::HashMap::new();
    for index in ctx.admit_iter(0..ordered.len().saturating_sub(1), "creo section owner operation count")? {
        let owner = ordered[index];
        let datum = ordered[index + 1];
        if owner.recipe.resolved().is_some() && datum.recipe.resolved().is_none() && owner.feature_id.checked_add(1) == Some(datum.feature_id) {
            *storage.with_storage(|| ctx.entry_hash_map(&mut pairs, owner.feature_id, "creo section recipe pair nodes"))?.or_insert(0_usize) += 1;
        }
    }
    for (definition, in_range) in ctx.admit_iter(&mut definitions, "creo section bound definition traversal")?.zip(eligible) {
        if !in_range { continue; }
        let Some(plane) = definition.section_3d.as_ref().and_then(|section| section.sketch_plane_entity_id).filter(|plane| *plane >= 2) else { continue; };
        if ctx.get_btree_map(&planes, &plane, "creo section plane count lookup")? != Some(&1) { continue; }
        let owner = plane - 2;
        if ctx.contains_btree_set(&claimed, &owner, "creo section claimed owner lookup")? || pairs.get(&owner) != Some(&1) { continue; }
        definition.identity = match definition.identity.schema_id() {
            Some(schema_id) => DefinitionIdentity::Parsed { schema_id: Some(schema_id), owner_feature_id: Some(owner) },
            None => DefinitionIdentity::BoundOwner { schema_id: None, owner_feature_id: owner },
        };
    }
    Ok(definitions)
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod owners_tests;

#[cfg(test)]
mod saved_tests;

#[cfg(test)]
mod tables_tests;

#[cfg(test)]
mod tests;


#[cfg(test)]
mod selection_tests;
