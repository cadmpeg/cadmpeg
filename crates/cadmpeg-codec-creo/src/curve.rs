// SPDX-License-Identifier: Apache-2.0
//! Curve namespace prototypes and topology rows.
//!
//! Prototype rows identify curves and their generating features. Topology rows
//! add the two face sides and successor curve for each native half-edge. Curve
//! parameter records decode scalar bodies. Relation evaluation resolves
//! assignments and recognizes exact cylindrical helix programs.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::num::NonZeroU32;

use cadmpeg_core::decode::{bounded_len, index_from_u32};

use crate::psb::{self, compact_int, reference_id};
use crate::scalar;

const EPS_RELATION_ROUND: f64 = 1.0e-9;
const EPS_ORDINATE_AGREEMENT: f64 = 1.0e-9;
const EPS_CIRCLE_RESIDUAL: f64 = 1.0e-9;
const EPS_ANGLE_AGREEMENT: f64 = 1.0e-6;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

mod solve;
#[cfg(test)]
mod test_support;
use solve::{
    evaluate_affine_program, infer_solve_variable_dimensions, solve_affine_expression_block,
    solve_nonlinear_expression_block, MAX_NONLINEAR_SOLVE_VARIABLES,
};

/// A labeled curve namespace entry.
///
/// `type_byte` remains raw because the namespace grammar does not define its
/// geometric interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurvePrototype {
    /// The row's `crv_id`: the curve's identifier in the `crv_array`
    /// namespace, referenced by `srf_array` and topology row `E0`/`E1`
    /// fields.
    pub(crate) id: u32,
    /// The row's raw `type` byte. Its geometric meaning is not identified by
    /// the namespace grammar alone ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)); the curve-body evaluator
    /// determines the interpretation.
    pub(crate) type_byte: u8,
    /// The `feat_id` compact integer, when the labeled row has one: the
    /// feature that generated this curve.
    pub(crate) feature_id: Option<u32>,
    /// The two named-prototype `crv_pnt_dir` orientation flags, when the
    /// prototype carries a complete direction array.
    directions: Option<[u8; 2]>,
    /// Byte offset of this prototype's `crv_array` label in the original
    /// stream.
    pub(crate) offset: usize,
}

#[cfg(test)]
pub(crate) fn dummy_curve_prototype() -> CurvePrototype {
    CurvePrototype {
        id: 8,
        type_byte: 1,
        feature_id: Some(2),
        directions: None,
        offset: 11,
    }
}

/// One source line in a curve-equation expression program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurveExpressionLine {
    /// UTF-8 source text without its NUL terminator.
    pub(crate) text: String,
    /// Byte offset of the first source byte.
    pub(crate) offset: usize,
}

/// Expression program stored by a curve-from-equation entity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveExpressionRecord {
    /// Entity identifier from the enclosing record.
    pub(crate) entity_id: u32,
    /// Whether the enclosing record is `backup_ents(crv_fr_eqn)`.
    pub(crate) backup: bool,
    /// Bounded native placement frame carried by the equation entity.
    pub(crate) local_system: Option<CurveExpressionLocalSystem>,
    /// Ordered source lines declared by the `f8` array.
    pub(crate) lines: Vec<CurveExpressionLine>,
    /// Assignment statements in source order.
    pub(crate) assignments: Vec<CurveExpressionAssignment>,
    /// Complete simultaneous-equation blocks in source order.
    pub(crate) solve_blocks: Vec<CurveExpressionSolveBlock>,
    /// Whether a `SOLVE`/`FOR` control sequence is malformed or incomplete.
    pub(crate) unresolved_solve_control: bool,
    /// Curve-equation constructs prohibited by the Creo expression grammar.
    pub(crate) prohibited_constructs: Vec<String>,
    /// Byte offset of the enclosing entity label.
    pub(crate) offset: usize,
    /// Byte offset of the `expression` field.
    pub(crate) expression_offset: usize,
}

/// Count-bounded `local_sys` payload carried by a curve-equation entity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveExpressionLocalSystem {
    /// Tuple dimensionality from the `f9` wrapper.
    pub(crate) dimensions: u32,
    /// Stored tuple count from the `f9` wrapper.
    pub(crate) count: u32,
    /// Exact stateful scalar body through the next named field.
    pub(crate) body: Vec<u8>,
    /// Twelve explicit scalar slots, absent when the body uses inheritance or
    /// contains a scalar form that is not decoded.
    pub(crate) explicit_slots: Option<cadmpeg_ir::units::FiniteVector<12>>,
    /// Byte offset of the `local_sys` named-record header.
    pub(crate) offset: usize,
}

/// One executable assignment in a curve expression program.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveExpressionAssignment {
    /// Typed relation target receiving the right-hand value.
    pub(crate) target: CurveExpressionTarget,
    /// Exact right-hand expression after surrounding ASCII whitespace removal.
    pub(crate) expression: String,
    /// Referenced identifiers in first-appearance order.
    pub(crate) dependencies: Vec<String>,
    /// Sequentially evaluated value when every dependency is resolved.
    pub(crate) value: Option<CurveExpressionValue>,
    /// Whether the source-ordered conditional program executes this assignment.
    pub(crate) activation: CurveExpressionActivation,
    /// Byte offset of the assignment source line.
    pub(crate) offset: usize,
}

/// One `SOLVE`/`FOR` simultaneous-equation block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveExpressionSolveBlock {
    /// Ordered equations between the `SOLVE` and `FOR` lines.
    pub(crate) equations: Vec<CurveExpressionEquation>,
    /// Ordered one-way relations in the block that do not involve an unknown.
    pub(crate) assignments: Vec<CurveExpressionAssignment>,
    /// Ordered unknowns declared by the terminating `FOR` line.
    pub(crate) unknowns: Vec<SolveUnknown>,
    /// Byte offset of the `SOLVE` line.
    pub(crate) offset: usize,
    /// Byte offset of the terminating `FOR` line.
    pub(crate) for_offset: usize,
}

/// One declared solve unknown and its optional solution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SolveUnknown {
    /// Declared variable name.
    pub(crate) name: String,
    /// Solved value, absent when the unknown remains unresolved.
    pub(crate) solution: Option<CurveExpressionValue>,
}

/// One equation in a simultaneous-equation block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurveExpressionEquation {
    /// Exact left-hand expression after surrounding ASCII whitespace removal.
    pub(crate) left: String,
    /// Exact right-hand expression after surrounding ASCII whitespace removal.
    pub(crate) right: String,
    /// Referenced identifiers in first-appearance order across both sides.
    pub(crate) dependencies: Vec<String>,
    /// Byte offset of the equation source line.
    pub(crate) offset: usize,
}

/// Target of one curve-expression assignment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CurveExpressionTarget {
    /// Scalar parameter target.
    Parameter {
        /// Assigned identifier.
        name: String,
        /// Unit expression declared on a newly created parameter target.
        declared_unit: Option<String>,
    },
    /// Dimension or parameter qualified by a relation scope.
    ScopedSymbol {
        /// Complete scoped relation identifier.
        name: String,
    },
    /// Unscoped Creo dimension, tolerance, or pattern system symbol.
    SystemSymbol {
        /// Complete system identifier.
        name: String,
        /// Namespace family selected by the identifier prefix.
        family: CurveExpressionSystemSymbolFamily,
    },
    /// Write invocation of a registered relation function.
    FunctionWrite {
        /// Registered function identifier.
        name: String,
        /// Exact argument expressions in source order.
        arguments: Vec<String>,
    },
    /// Cell of a series or list parameter.
    TableCell {
        /// Table-valued parameter identifier.
        parameter: String,
        /// Exact one-based row selector expression.
        row: String,
        /// Exact column selector expression when present.
        column: Option<String>,
    },
}

/// Namespace family of an unscoped Creo relation system symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CurveExpressionSystemSymbolFamily {
    /// Parent-model or assembly dimension (`d#`).
    Dimension,
    /// Section dimension (`sd#`).
    SectionDimension,
    /// Reference dimension (`rd#`).
    ReferenceDimension,
    /// Section reference dimension (`rsd#`).
    SectionReferenceDimension,
    /// Known parent dimension used in a section (`kd#`).
    KnownDimension,
    /// Driven dimension (`ad#`).
    DrivenDimension,
    /// Pattern instance count (`p#`).
    PatternCount,
    /// Plus, minus, or symmetric tolerance component.
    Tolerance,
}

impl CurveExpressionAssignment {
    pub(crate) fn parameter_target(&self) -> Option<(&str, Option<&str>)> {
        match &self.target {
            CurveExpressionTarget::Parameter {
                name,
                declared_unit,
            } => Some((name, declared_unit.as_deref())),
            CurveExpressionTarget::ScopedSymbol { .. }
            | CurveExpressionTarget::SystemSymbol { .. }
            | CurveExpressionTarget::FunctionWrite { .. }
            | CurveExpressionTarget::TableCell { .. } => None,
        }
    }

    fn scalar_target(&self) -> Option<(&str, Option<&str>)> {
        match &self.target {
            CurveExpressionTarget::Parameter {
                name,
                declared_unit,
            } => Some((name, declared_unit.as_deref())),
            CurveExpressionTarget::ScopedSymbol { name } => Some((name, None)),
            CurveExpressionTarget::SystemSymbol { name, .. } => Some((name, None)),
            CurveExpressionTarget::FunctionWrite { .. } => None,
            CurveExpressionTarget::TableCell { .. } => None,
        }
    }
}

/// A deterministic value produced by a curve relation expression.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub(crate) enum CurveExpressionValue {
    /// Dimensionless numeric value.
    Number(cadmpeg_ir::scalar::FiniteReal),
    /// Length in canonical millimeters.
    Length(cadmpeg_ir::scalar::FiniteReal),
    /// Angle in relation degrees.
    Angle(cadmpeg_ir::scalar::FiniteReal),
    /// Quantity whose physical dimension has no dedicated neutral value type.
    Quantity(CurveExpressionQuantity),
    /// UTF-8 string value.
    String(String),
}

impl CurveExpressionValue {
    fn truth(&self) -> Option<bool> {
        match self {
            Self::Number(value) => Some(value.get() != 0.0),
            Self::Length(_) | Self::Angle(_) | Self::Quantity(_) | Self::String(_) => None,
        }
    }
}

/// Canonically scaled relation quantity represented by physical base powers.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub(crate) struct CurveExpressionQuantity {
    value: cadmpeg_ir::scalar::FiniteReal,
    #[serde(flatten)]
    dimension: ResidualRelationDimension,
}

/// Physical powers that have no dedicated relation value variant.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
struct ResidualRelationDimension {
    #[serde(rename = "length_power")]
    length: i8,
    #[serde(rename = "mass_power")]
    mass: i8,
    #[serde(rename = "time_power")]
    time: i8,
    #[serde(rename = "angle_power")]
    angle: i8,
    #[serde(rename = "temperature_power")]
    temperature: i8,
}

impl ResidualRelationDimension {
    fn new(dimension: RelationDimension) -> Option<Self> {
        if [
            RelationDimension::default(),
            RelationDimension::LENGTH,
            RelationDimension::ANGLE,
        ]
        .contains(&dimension)
        {
            return None;
        }
        Some(Self {
            length: dimension.length,
            mass: dimension.mass,
            time: dimension.time,
            angle: dimension.angle,
            temperature: dimension.temperature,
        })
    }
}

impl CurveExpressionQuantity {
    pub(crate) fn new(value: f64, powers: [i8; 5]) -> Option<Self> {
        let [length, mass, time, angle, temperature] = powers;
        Some(Self {
            value: cadmpeg_ir::scalar::FiniteReal::new(value)?,
            dimension: ResidualRelationDimension::new(RelationDimension {
                length,
                mass,
                time,
                angle,
                temperature,
            })?,
        })
    }

    pub(crate) fn value(self) -> f64 {
        self.value.get()
    }

    pub(crate) fn powers(self) -> [i8; 5] {
        let dimension = self.dimension;
        [
            dimension.length,
            dimension.mass,
            dimension.time,
            dimension.angle,
            dimension.temperature,
        ]
    }

    fn dimension(self) -> RelationDimension {
        let [length, mass, time, angle, temperature] = self.powers();
        RelationDimension {
            length,
            mass,
            time,
            angle,
            temperature,
        }
    }
}

/// Evaluation state of an assignment inside relation conditionals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurveExpressionActivation {
    /// The assignment executes in the current source-ordered evaluation.
    Active,
    /// A resolved enclosing condition excludes the assignment.
    Inactive,
    /// An enclosing condition cannot be evaluated from available scalar values.
    Conditional,
}

impl CurveExpressionActivation {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Conditional => "conditional",
        }
    }
}

/// Exact cylindrical helix parameters from a `crv_fr_eqn` program.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CurveExpressionHelix {
    /// Constant cylindrical radius in model millimeters.
    pub(crate) radius: cadmpeg_ir::scalar::PositiveLength,
    /// Signed axial rise from `t = 0` through `t = 1`.
    pub(crate) height: cadmpeg_ir::scalar::FiniteReal,
    /// Native axial coordinate at `t = 0`.
    pub(crate) z_start: cadmpeg_ir::scalar::FiniteReal,
    /// Positive angular travel in revolutions.
    pub(crate) revolutions: cadmpeg_ir::scalar::PositiveReal,
    /// Angular position at `t = 0`, in radians.
    pub(crate) start_angle: cadmpeg_ir::scalar::Angle,
    /// Whether angular travel decreases as `t` increases.
    pub(crate) clockwise: bool,
}

impl CurveExpressionHelix {
    fn new(
        radius: f64,
        height: f64,
        z_start: f64,
        revolutions: f64,
        start_angle: f64,
        clockwise: bool,
    ) -> Option<Self> {
        Some(Self {
            radius: cadmpeg_ir::scalar::PositiveLength::new(radius)?,
            height: cadmpeg_ir::scalar::FiniteReal::new(height)?,
            z_start: cadmpeg_ir::scalar::FiniteReal::new(z_start)?,
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(revolutions)?,
            start_angle: cadmpeg_ir::scalar::Angle::new(start_angle)?,
            clockwise,
        })
    }
}

/// A curve row with a uniquely delimited topology suffix.
///
/// `faces` and `next_edges` preserve the two native sides in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurveTopologyRow {
    /// The row's `crv_id`, matching a [`CurvePrototype::id`] in the same
    /// `crv_array` namespace.
    pub(crate) id: u32,
    /// The row's raw `type` byte; see [`CurvePrototype::type_byte`].
    pub(crate) type_byte: u8,
    /// The `feat_id` compact integer: the feature that generated this
    /// curve.
    pub(crate) feature_id: u32,
    /// The two `crv_pnt_dir` orientation-flag bytes, one per half-edge side.
    /// These are per-side orientation flags, not a tangent vector
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub(crate) directions: [u8; 2],
    /// The `F0`/`F1` suffix fields: the `srf_array` face identifiers
    /// bounding the curve's two half-edge sides, absent where the side is
    /// unbounded.
    pub(crate) faces: [Option<NonZeroU32>; 2],
    /// The `E0`/`E1` suffix fields: the `crv_array` identifier of the next
    /// edge for each of the two half-edge sides, used to walk loops
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub(crate) next_edges: [u32; 2],
    /// Byte offset of the row's `crv_id` field in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for CurveTopologyRow {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (
                    &self.id,
                    &self.type_byte,
                    &self.feature_id,
                    &self.directions,
                ),
                (&self.faces, &self.next_edges, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

impl CurveTopologyRow {
    /// The face identifiers bounding the two half-edge sides, in side order,
    /// skipping sides that bound no face.
    pub(crate) fn bounded_face_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.faces.iter().flatten().map(|face| face.get())
    }

    /// Whether either half-edge side bounds `face_id`.
    pub(crate) fn bounds_face(&self, face_id: u32) -> bool {
        self.bounded_face_ids().any(|bounded| bounded == face_id)
    }

    /// The stored `F0`/`F1` fields, with `0` for a side that bounds no face.
    pub(crate) fn stored_face_ids(&self) -> [u32; 2] {
        self.faces.map(stored_face_reference)
    }
}

impl CurvePrototypeTopology {
    /// The stored `crv_hdr_geom_ptr[0/1]` fields, with `0` for a side that
    /// names no surface.
    pub(crate) fn stored_face_ids(&self) -> [u32; 2] {
        self.faces.map(stored_face_reference)
    }
}

/// One-sided DEPDB suffix, serialized as `[0, X1, F1, 0]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DepdbCurveSuffix {
    x1: u32,
    face_id: u32,
}

#[cfg(test)]
pub(crate) fn dummy_depdb_curve_suffix() -> DepdbCurveSuffix {
    DepdbCurveSuffix { x1: 0, face_id: 7 }
}

impl serde::Serialize for DepdbCurveSuffix {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&[0, self.x1, self.face_id, 0], serializer)
    }
}

/// One DEPDB cross-section curve row with its one-sided topology suffix.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DepdbCurveRow {
    /// Curve identifier in the cross-section `crv_array` namespace.
    pub(crate) id: u32,
    /// Raw curve-family discriminator.
    pub(crate) type_byte: u8,
    /// Owning feature identifier.
    pub(crate) feature_id: u32,
    /// Stored per-side direction flags.
    pub(crate) directions: [u8; 2],
    /// The `[0, X1, F1, 0]` one-sided suffix.
    pub(crate) suffix: DepdbCurveSuffix,
    /// Exact bytes between the fixed prefix and one-sided suffix.
    pub(crate) body: Vec<u8>,
    /// Decoded scalar tokens with exact body-relative spans.
    pub(crate) scalar_tokens: Vec<CurveParameterScalar>,
    /// Canonical entity references with exact body-relative spans.
    pub(crate) references: Vec<CurveParameterReference>,
    /// Maximal body spans not claimed by a scalar or reference token.
    pub(crate) opaque_spans: Vec<CurveParameterOpaqueSpan>,
    /// Byte offset of the row identifier.
    pub(crate) offset: usize,
}

/// Bounded analytic parameter body from one positional `crv_array` row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveParameterRecord {
    /// Owning curve identifier.
    pub(crate) curve_id: u32,
    /// Raw curve-family discriminator.
    pub(crate) type_byte: u8,
    /// Exact bytes between direction flags and the selected suffix boundary.
    pub(crate) body: Vec<u8>,
    /// Scalar tokens with exact body-relative spans.
    pub(crate) scalar_tokens: Vec<CurveParameterScalar>,
    /// Canonical entity references with exact body-relative spans.
    pub(crate) references: Vec<CurveParameterReference>,
    /// Maximal byte spans not claimed by scalar or reference tokens.
    pub(crate) opaque_spans: Vec<CurveParameterOpaqueSpan>,
    /// Positional `ref_geom[0]` and `ref_geom[1]` values following the four
    /// topology references.
    pub(crate) reference_geometry: [u32; 2],
    /// Byte offset of the positional row in the original stream.
    pub(crate) offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub(crate) body_offset: usize,
    /// Byte offset of the selected body/suffix boundary in the original stream.
    pub(crate) suffix_offset: usize,
}

#[cfg(test)]
impl CurveParameterRecord {
    /// Decoded scalar values in byte order.
    pub(crate) fn scalar_values(&self) -> Vec<f64> {
        self.scalar_tokens.iter().map(|token| token.value).collect()
    }

    /// Canonical entity references skipped while walking the scalar lane.
    pub(crate) fn skipped_references(&self) -> Vec<u32> {
        self.references
            .iter()
            .map(|reference| reference.entity_id)
            .collect()
    }
}

/// One decoded scalar token in a positional curve body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveParameterScalar {
    /// Decoded scalar value.
    pub(crate) value: f64,
    /// Exact token bytes.
    pub(crate) raw: Vec<u8>,
    /// Body-relative token offset.
    pub(crate) offset: usize,
}

/// One canonical entity reference in a positional curve body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurveParameterReference {
    /// Referenced entity identifier.
    pub(crate) entity_id: u32,
    /// Body-relative reference-token offset, including `f7`.
    pub(crate) offset: usize,
    /// Reference-token length in bytes, including `f7`.
    pub(crate) length: usize,
}

/// One maximal unclaimed byte span in a positional curve body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurveParameterOpaqueSpan {
    /// Exact unclaimed bytes.
    pub(crate) raw: Vec<u8>,
    /// Body-relative span offset.
    pub(crate) offset: usize,
}

/// Two pcurve endpoints represented in both adjacent face parameter frames.
/// Both reader routes admit finite coordinates before constructing this record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PcurveEndpoints {
    /// Owning curve identifier.
    pub(crate) curve_id: u32,
    /// Adjacent face identifiers corresponding to face frames zero and one.
    pub(crate) faces: [Option<NonZeroU32>; 2],
    /// Endpoint A then B in the first face's local UV frame.
    pub(crate) face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's local UV frame.
    pub(crate) face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source positional curve row.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for PcurveEndpoints {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.curve_id,
                &self.faces,
                &self.face_0_endpoints,
                &self.face_1_endpoints,
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl PcurveEndpoints {
    /// Stored face identifiers, with zero for an absent face.
    pub(crate) fn stored_face_ids(&self) -> [u32; 2] {
        self.faces.map(stored_face_reference)
    }
}

/// Ordered samples of one curve represented in both incident-face charts.
/// Every stored sample coordinate is finite at reader admission.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TwoChartPcurveSamples {
    /// Owning curve identifier.
    pub(crate) curve_id: u32,
    /// Adjacent face identifiers in sample-chart order.
    pub(crate) faces: [u32; 2],
    /// Pointwise-corresponding `[F0(u, v), F1(u, v)]` chart samples.
    pub(crate) samples: Vec<[[f64; 2]; 2]>,
    /// Byte offset of the source positional curve row.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for TwoChartPcurveSamples {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.curve_id, &self.faces, &self.samples, &self.offset),
            ctx,
            operation,
        )
    }
}

/// One-sided endpoint path from the complete short fc 02 curve body.
/// Every stored endpoint coordinate is finite at reader admission.
///
/// The body carries one path in the first topology face's parameter chart;
/// the second face remains a carrier-only join. The retained terminal operand
/// is deliberately not interpreted by this record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fc02ShortPcurveEndpoints {
    /// Owning curve identifier.
    pub(crate) curve_id: u32,
    /// Adjacent surface identifiers from the topology row.
    pub(crate) faces: [u32; 2],
    /// Endpoint A then B in the first face's parameter frame.
    pub(crate) face_0_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source positional curve row.
    pub(crate) offset: usize,
}

/// Ordered world-coordinate lane from an `fc <subtype>` dense curve body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FcCurveCoordinates {
    /// Owning curve identifier.
    pub(crate) curve_id: u32,
    /// Byte following the `fc` body prefix.
    pub(crate) subtype: u8,
    /// Exact complete curve parameter body, including the `fc` prefix.
    pub(crate) body: Vec<u8>,
    /// Ordered exact world-coordinate values, in mm.
    pub(crate) values_mm: Vec<f64>,
    /// World-coordinate tokens with exact body-relative spans.
    pub(crate) tokens: Vec<FcCurveCoordinateToken>,
    /// Maximal body spans not owned by a recognized coordinate token.
    pub(crate) opaque_spans: Vec<FcCurveOpaqueSpan>,
    /// Byte offset of the source positional curve row.
    pub(crate) offset: usize,
}

/// One recognized world-coordinate token in an `fc <subtype>` body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FcCurveCoordinateToken {
    /// Decoded model length in millimeters.
    pub(crate) value_mm: f64,
    /// Exact source bytes occupied by the token.
    pub(crate) raw: Vec<u8>,
    /// Token offset relative to the complete curve parameter body.
    pub(crate) offset: usize,
}

impl serde::Serialize for FcCurveCoordinateToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("FcCurveCoordinateToken", 4)?;
        wire.serialize_field("value_mm", &self.value_mm)?;
        wire.serialize_field("raw", &self.raw)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.serialize_field("length", &self.raw.len())?;
        wire.end()
    }
}

/// One maximal unclaimed span in an `fc <subtype>` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FcCurveOpaqueSpan {
    /// Exact source bytes in the span.
    raw: Vec<u8>,
    /// Span offset relative to the complete curve parameter body.
    offset: usize,
}

impl serde::Serialize for FcCurveOpaqueSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("FcCurveOpaqueSpan", 3)?;
        wire.serialize_field("raw", &self.raw)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.serialize_field("length", &self.raw.len())?;
        wire.end()
    }
}

/// Direction of stored parameters relative to row-frame polar angles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParameterSense {
    /// Parameters increase with polar angle.
    Increasing,
    /// Parameters decrease with polar angle.
    Decreasing,
}

impl ParameterSense {
    /// Signed parameter multiplier.
    pub(crate) fn as_i8(self) -> i8 {
        match self {
            Self::Increasing => 1,
            Self::Decreasing => -1,
        }
    }
}

/// Relation between stored circle parameters and row-frame polar angles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fc05AngleParameterRelation {
    /// Neither unique parameter sense establishes a reference direction.
    Inconsistent,
    /// A unique parameter sense establishes a reference direction.
    Consistent {
        /// Direction of increasing stored parameters.
        sense: ParameterSense,
        /// Unit radial direction at stored parameter zero.
        reference_direction_row_frame: [f64; 2],
    },
}

/// Circle proven by the decoded points of an `fc 05` curve body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fc05Circle {
    /// Owning curve identifier.
    pub(crate) curve_id: u32,
    /// Circle center in the FC row's in-plane coordinate frame.
    pub(crate) center_row_frame: [f64; 2],
    /// Exact radius in mm.
    pub(crate) radius_mm: f64,
    /// Finite radial quotient from the fitted center to the first stored sample.
    pub(crate) sample_direction_row_frame: cadmpeg_ir::units::HypotDirection2,
    /// Stored parameter relation and its reference direction.
    pub(crate) angle_parameter: Fc05AngleParameterRelation,
    /// Constant cap-plane ordinate when present in every point.
    pub(crate) cap_ordinate_row_frame: Option<f64>,
    /// Number of points participating in validation.
    pub(crate) point_count: usize,
    /// Maximum absolute radial residual.
    pub(crate) max_residual: f64,
    /// Byte offset of the source positional curve row.
    pub(crate) offset: usize,
}

/// One circle edge joining a cylinder to a cap plane.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fc05CapEdge {
    /// Circle curve identifier.
    pub(crate) curve_id: u32,
    /// Opposite cap plane identifier.
    pub(crate) cap_plane_id: u32,
    /// Cap ordinate in the owning feature's row frame.
    pub(crate) cap_ordinate_row_frame: f64,
}

/// Two or more topology-bound `fc 05` cap circles that establish one native
/// cylinder's radius and row-frame axis line, but not its model-space frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fc05CylinderCapPair {
    /// Cylinder surface identifier shared by every cap edge.
    pub(crate) surface_id: u32,
    /// Agreeing cap edges in source order.
    pub(crate) cap_edges: Vec<Fc05CapEdge>,
    /// Shared center in the owning feature's row frame.
    pub(crate) center_row_frame: [f64; 2],
    /// Shared exact radius in mm.
    pub(crate) radius_mm: f64,
    /// Unit radial direction at parameter zero in the row's `(x, z)` frame.
    pub(crate) reference_direction_row_frame: [f64; 2],
    /// Shared signed parameter-to-polar-angle relation.
    pub(crate) parameter_sense: ParameterSense,
    /// At least two distinct cap ordinates in the owning feature's row frame.
    pub(crate) cap_ordinates_row_frame: Vec<f64>,
    /// Byte offset of the first participating curve row.
    pub(crate) offset: usize,
}

/// Complete eight-slot pcurve endpoints from a labeled curve prototype.
/// Every stored endpoint coordinate is finite at reader admission.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrototypePcurveEndpoints {
    /// Prototype curve identifier.
    pub(crate) curve_id: u32,
    /// Endpoint A then B in schema face frame zero.
    pub(crate) face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in schema face frame one.
    pub(crate) face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the `crv_pnt_arr` label in the original stream.
    pub(crate) offset: usize,
}

/// Four labeled topology references of a curve prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurvePrototypeTopology {
    /// Prototype curve identifier.
    pub(crate) curve_id: u32,
    /// Adjacent surface identifiers from `crv_hdr_geom_ptr[0/1]`, absent
    /// where the side names no surface.
    pub(crate) faces: [Option<NonZeroU32>; 2],
    /// Per-face successor curve identifiers from `next_crv_hdr_ptr[0/1]`.
    pub(crate) next_edges: [u32; 2],
    /// Byte offset of the prototype namespace.
    pub(crate) offset: usize,
}

/// Prototype pcurve endpoints bound to their two labeled adjacent faces.
/// Binding preserves the reader's finite endpoint admission.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundPrototypePcurve {
    /// Prototype curve identifier.
    pub(crate) curve_id: u32,
    /// Adjacent face identifiers corresponding to UV frames zero and one.
    pub(crate) faces: [Option<NonZeroU32>; 2],
    /// Endpoint A then B in the first face's UV frame.
    pub(crate) face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's UV frame.
    pub(crate) face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source prototype pcurve.
    pub(crate) offset: usize,
}

impl BoundPrototypePcurve {
    /// Stored face identifiers, with zero for an absent face.
    pub(crate) fn stored_face_ids(&self) -> [u32; 2] {
        self.faces.map(stored_face_reference)
    }
}

/// Discover every labeled `crv_array` prototype. A label range ends at the
/// following `crv_array` label, so DEPDB-concatenated namespaces remain
/// independent.
pub(crate) fn prototypes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<CurvePrototype>, cadmpeg_core::CodecError> {
    let mut result = Vec::new();
    let mut namespaces = ctx.find_bytes_iter(payload, b"crv_array\0", "find Creo curve marker")?;
    let mut current = namespaces.next();
    while let Some(section_start) = current {
        let start = section_start + b"crv_array\0".len();
        current = namespaces.next();
        let section_end = current.unwrap_or(payload.len());
        let Some(id_label) = ctx.find_bytes_in(
            payload,
            b"crv_id\0",
            start,
            section_end,
            "find Creo curve marker",
        )?
        else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (id, id_end) = compact_int(payload, id_start);
        if id_end == id_start {
            continue;
        }
        let Some(type_label) = ctx.find_bytes_in(
            payload,
            b"type\0",
            id_end,
            section_end,
            "find Creo curve marker",
        )?
        else {
            continue;
        };
        let Some(&type_byte) = payload.get(type_label + b"type\0".len()) else {
            continue;
        };
        let feature_id = ctx
            .find_bytes_in(
                payload,
                b"feat_id\0",
                id_end,
                section_end,
                "find Creo curve marker",
            )?
            .and_then(|label| {
                let value_start = label + b"feat_id\0".len();
                let (value, end) = compact_int(payload, value_start);
                (end != value_start).then_some(value)
            });
        let directions = ctx
            .find_bytes_in(
                payload,
                b"crv_pnt_dir\0",
                id_end,
                section_end,
                "find Creo curve marker",
            )?
            .and_then(|label| {
                let value_start = label + b"crv_pnt_dir\0".len();
                (payload.get(value_start) == Some(&psb::token::ARRAY_OPEN)).then_some(())?;
                let (count, after_count) = compact_int(payload, value_start + 1);
                (count == 2).then_some(())?;
                let directions = [*payload.get(after_count)?, *payload.get(after_count + 1)?];
                directions
                    .iter()
                    .all(|direction| matches!(direction, 0x01 | 0xf6))
                    .then_some(directions)
            });
        ctx.reserve_vec(&mut result, 1, "creo curve prototypes")?;
        result.push(CurvePrototype {
            id,
            type_byte,
            feature_id,
            directions,
            offset: section_start,
        });
    }
    Ok(result)
}

fn unique_curve_index<'a, T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &'a [T],
    id: impl Fn(&T) -> u32,
    operation: &'static str,
) -> Result<std::collections::HashMap<u32, Option<&'a T>>, cadmpeg_core::CodecError> {
    let mut index = std::collections::HashMap::new();
    for row in ctx.admit_iter(rows, operation)? {
        match ctx.entry_hash_map(&mut index, id(row), operation)? {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some(row));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = None;
            }
        }
    }
    Ok(index)
}

/// Promote a uniquely referenced named-prototype topology record to a native
/// half-edge row when its positional successor references the prototype ID.
///
/// A named prototype is a schema record by default. A successor reference is
/// the byte-backed evidence that the prototype also supplies an edge identity
/// in the enclosing topology graph. The promotion remains withheld when the
/// prototype, topology record, or face namespace is ambiguous.
pub(crate) fn prototype_topology_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    prototypes: &[CurvePrototype],
    prototype_topology: &[CurvePrototypeTopology],
    positional_rows: &[CurveTopologyRow],
    face_ids: &BTreeSet<u32>,
) -> Result<Vec<CurveTopologyRow>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo prototype topology scratch")?;
    let prototype_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            prototypes,
            |row| row.id,
            "creo prototype ID count nodes",
        )
    })?;
    let topology_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            prototype_topology,
            |row| row.curve_id,
            "creo prototype topology count nodes",
        )
    })?;
    let mut positional_ids = std::collections::HashSet::new();
    let mut referenced_ids = std::collections::HashSet::new();
    for row in ctx.admit_iter(positional_rows, "creo positional topology traversal")? {
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut positional_ids,
                row.id,
                "creo positional topology ID nodes",
            )
        })?;
        for id in row.next_edges {
            if id != 0 {
                scratch.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut referenced_ids,
                        id,
                        "creo referenced topology ID nodes",
                    )
                })?;
            }
        }
    }
    for row in ctx.admit_iter(prototype_topology, "creo prototype reference traversal")? {
        for id in row.next_edges {
            if id != 0 {
                scratch.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut referenced_ids,
                        id,
                        "creo referenced topology ID nodes",
                    )
                })?;
            }
        }
    }
    let mut rows = Vec::new();
    for topology in ctx.admit_iter(prototype_topology, "creo prototype topology traversal")? {
        if positional_ids.contains(&topology.curve_id)
            || !matches!(topology_index.get(&topology.curve_id), Some(Some(_)))
            || !referenced_ids.contains(&topology.curve_id)
        {
            continue;
        }
        let mut valid_faces = true;
        for face in topology.faces.into_iter().flatten() {
            if !ctx.contains_btree_set(face_ids, &face.get(), "creo prototype face lookup")? {
                valid_faces = false;
                break;
            }
        }
        if !valid_faces {
            continue;
        }
        let Some(Some(prototype)) = prototype_index.get(&topology.curve_id) else {
            continue;
        };
        let Some(directions) = prototype.directions else {
            continue;
        };
        ctx.reserve_vec(&mut rows, 1, "creo promoted prototype topology rows")?;
        rows.push(CurveTopologyRow {
            id: topology.curve_id,
            type_byte: prototype.type_byte,
            feature_id: prototype.feature_id.unwrap_or(0),
            directions,
            faces: topology.faces,
            next_edges: topology.next_edges,
            offset: topology.offset,
        });
    }
    ctx.stable_sort_by(
        rows.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo prototype topology rows rows ordering",
    )?;
    Ok(rows)
}

/// Decode bounded curve-from-equation expression programs.
#[cfg(test)]
pub(crate) fn expression_records(payload: &[u8]) -> Vec<CurveExpressionRecord> {
    crate::decode::with_test_decode_ctx(|ctx| {
        expression_records_with_model_name(ctx, payload, None)
    })
    .expect("test curve expression records")
}

/// Decode curve-expression programs with an unambiguous current-model name.
pub(crate) fn expression_records_with_model_name(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    model_name: Option<&str>,
) -> Result<Vec<CurveExpressionRecord>, cadmpeg_core::CodecError> {
    const PRIMARY: &[u8] = b"entity(crv_fr_eqn)\0";
    const BACKUP: &[u8] = b"backup_ents(crv_fr_eqn)\0";
    const ID: &[u8] = b"\xe0\x01id\0";
    const EXPRESSION: &[u8] = b"\xe0\x0aexpression\0";

    let mut scratch = ctx.reserve_scoped(0, "creo expression record scratch")?;
    let mut labels = Vec::new();
    for (label, backup) in [(PRIMARY, false), (BACKUP, true)] {
        for offset in ctx.find_bytes_iter(payload, label, "find Creo curve marker")? {
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut labels,
                    (offset, label.len(), backup),
                    "creo expression record labels",
                )
            })?;
        }
    }
    ctx.sort_unstable_by(
        &mut labels,
        |value| &value.0,
        Ord::cmp,
        "creo expression record labels sort",
    )?;

    let cache = scratch.with_storage(|| scalar::ScalarCache::from_section_checked(ctx, payload))?;
    let mut records = Vec::new();
    for (index, &(offset, label_len, backup)) in ctx
        .admit_iter(&labels, "creo expression record traversal")?
        .enumerate()
    {
        let end = labels
            .get(index + 1)
            .map_or(payload.len(), |(next, _, _)| *next);
        let Some(id_label) = ctx.find_bytes_in(
            payload,
            ID,
            offset + label_len,
            end,
            "find Creo curve marker",
        )?
        else {
            continue;
        };
        let id_start = id_label + ID.len();
        let (entity_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let Some(expression_offset) =
            ctx.find_bytes_in(payload, EXPRESSION, after_id, end, "find Creo curve marker")?
        else {
            continue;
        };
        let opener = expression_offset + EXPRESSION.len();
        if payload.get(opener) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, mut cursor) = compact_int(payload, opener + 1);
        if cursor == opener + 1 || cursor > end {
            continue;
        }
        let mut line_storage = ctx.reserve_scoped(0, "creo expression record line text")?;
        let mut line_vector_storage = ctx.reserve_scoped(0, "creo expression record lines")?;
        let mut lines = Vec::new();
        let mut slots = 0..count;
        while slots.start < slots.end
            && ctx.next_charged(&mut slots, "creo expression line traversal")?
            .is_some()
        {
            let Some(relative_end) = ctx.position_by(
                &payload[cursor..end],
                |byte| Ok(*byte == 0),
                "creo expression line terminator scan",
            )?
            else {
                lines.clear();
                break;
            };
            let line_end = cursor + relative_end;
            let Ok(text) =
                ctx.validate_utf8(&payload[cursor..line_end], "creo UTF-8 validation")?
            else {
                lines.clear();
                break;
            };
            line_vector_storage
                .with_storage(|| ctx.reserve_vec(&mut lines, 1, "creo expression record lines"))?;
            lines.push(CurveExpressionLine {
                text: line_storage.with_storage(|| {
                    ctx.copy_retained_text(text, "creo expression record line text")
                })?,
                offset: cursor,
            });
            cursor = line_end + 1;
        }
        if lines.len() == index_from_u32(count) {
            let lines = line_storage.commit_value(lines)?;
            let lines = line_vector_storage.commit_value(lines)?;
            let local_system = expression_local_system(ctx, payload, after_id, end, &cache)?;
            let prohibited_constructs = curve_equation_prohibited_constructs(ctx, &lines)?;
            let mut program_index_storage =
                ctx.reserve_scoped(0, "creo solve program index scratch")?;
            let mut solve_program =
                curve_expression_solve_program(ctx, &lines, &mut program_index_storage)?;
            let mut solution_storage = ctx.reserve_scoped(0, "creo expression solution scratch")?;
            let mut evaluation = evaluate_expression_program_details(
                ctx,
                &lines,
                model_name,
                &ExternalRelationSymbols::default(),
                &mut solution_storage,
                &solve_program,
                prohibited_constructs.is_empty() && !solve_program.unresolved_control,
            )?;
            if !prohibited_constructs.is_empty() || solve_program.unresolved_control {
                for assignment in ctx.admit_iter(
                    &mut evaluation.assignments,
                    "creo disabled expression assignment traversal",
                )? {
                    assignment.value = None;
                }
                evaluation.solve_solutions.clear();
            }
            synchronize_solve_blocks(
                ctx,
                &mut solve_program.blocks,
                &evaluation.assignments,
                &evaluation.solve_solutions,
            )?;
            ctx.reserve_vec(&mut records, 1, "creo expression records")?;
            records.push(CurveExpressionRecord {
                entity_id,
                backup,
                local_system,
                lines,
                assignments: evaluation.assignments,
                solve_blocks: solve_program.blocks,
                unresolved_solve_control: solve_program.unresolved_control,
                prohibited_constructs,
                offset,
                expression_offset,
            });
        }
    }
    Ok(records)
}

fn expression_local_system(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    after_id: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<CurveExpressionLocalSystem>, cadmpeg_core::CodecError> {
    let Some(offset) = ctx.find_bytes_in(
        payload,
        b"\xe0\x02local_sys\0\xf9",
        after_id,
        end,
        "find Creo curve marker",
    )?
    else {
        return Ok(None);
    };
    let extents_start = offset + b"\xe0\x02local_sys\0\xf9".len();
    let (dimensions, dimensions_end) = compact_int(payload, extents_start);
    let (count, body_start) = compact_int(payload, dimensions_end);
    if dimensions_end <= extents_start || body_start <= dimensions_end || body_start > end {
        return Ok(None);
    }
    let body_end = ctx
        .position_by(
            &payload[body_start..end],
            |byte| Ok(*byte == psb::token::NAMED_RECORD),
            "creo expression local-system boundary scan",
        )?
        .map_or(end, |relative| body_start + relative);
    let body = ctx.copy_retained(
        &payload[body_start..body_end],
        "creo expression local-system body",
    )?;
    Ok(Some(CurveExpressionLocalSystem {
        dimensions,
        count,
        explicit_slots: ((dimensions, count) == (4, 3))
            .then(|| scalar::decode_explicit_local_system_slots(&body, cache))
            .flatten(),
        body,
        offset,
    }))
}

pub(crate) fn reevaluate_expression_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &mut [CurveExpressionRecord],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> Result<(), cadmpeg_core::CodecError> {
    for record in ctx.admit_iter(records, "creo expression reevaluation traversal")? {
        let mut program_storage = ctx.reserve_scoped(0, "creo solve program scratch")?;
        let mut program_index_storage =
            ctx.reserve_scoped(0, "creo solve program index scratch")?;
        let solve_program = program_storage.with_storage(|| {
            curve_expression_solve_program(ctx, &record.lines, &mut program_index_storage)
        })?;
        let mut solution_storage = ctx.reserve_scoped(0, "creo expression solution scratch")?;
        let mut evaluation = evaluate_expression_program_details(
            ctx,
            &record.lines,
            model_name,
            external_symbols,
            &mut solution_storage,
            &solve_program,
            record.prohibited_constructs.is_empty() && !record.unresolved_solve_control,
        )?;
        if !record.prohibited_constructs.is_empty() || record.unresolved_solve_control {
            for assignment in ctx.admit_iter(
                &mut evaluation.assignments,
                "creo disabled expression assignment traversal",
            )? {
                assignment.value = None;
            }
            evaluation.solve_solutions.clear();
        }
        synchronize_solve_blocks(
            ctx,
            &mut record.solve_blocks,
            &evaluation.assignments,
            &evaluation.solve_solutions,
        )?;
        record.assignments = evaluation.assignments;
    }
    Ok(())
}

fn synchronize_solve_blocks(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    blocks: &mut [CurveExpressionSolveBlock],
    assignments: &[CurveExpressionAssignment],
    solutions: &BTreeMap<usize, Vec<CurveExpressionValue>>,
) -> Result<(), cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if blocks.is_empty() {
        return Ok(());
    }
    let mut scratch = ctx.reserve_scoped(0, "creo synchronized assignment scratch")?;
    let mut by_offset = HashMap::new();
    for assignment in ctx.admit_iter(assignments, "creo synchronized assignment index traversal")? {
        scratch
            .with_storage(|| {
                ctx.entry_hash_map(
                    &mut by_offset,
                    assignment.offset,
                    "creo synchronized assignment index nodes",
                )
            })?
            .or_insert(assignment);
    }
    for block in ctx.admit_iter(blocks, "creo solve block synchronization traversal")? {
        for assignment in ctx.admit_iter(
            &mut block.assignments,
            "creo solve assignment synchronization traversal",
        )? {
            if let Some(evaluated) = by_offset.get(&assignment.offset) {
                assignment.value = evaluated
                    .value
                    .as_ref()
                    .map(|value| {
                        copy_expression_value(ctx, value, "creo synchronized assignment values")
                    })
                    .transpose()?;
                assignment.activation = evaluated.activation;
            }
        }
        let values =
            ctx.get_btree_map(solutions, &block.offset, "creo synchronized solve lookup")?;
        for (index, unknown) in ctx
            .admit_iter(
                &mut block.unknowns,
                "creo solve unknown synchronization traversal",
            )?
            .enumerate()
        {
            unknown.solution = values
                .and_then(|values| values.get(index))
                .map(|value| copy_expression_value(ctx, value, "creo synchronized solve values"))
                .transpose()?;
        }
    }
    Ok(())
}

fn curve_equation_prohibited_constructs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    const PROHIBITED_FUNCTIONS: &[&str] =
        &["abs", "ceil", "floor", "extract", "if", "itos", "search"];
    let mut node_storage = ctx.reserve_scoped(0, "creo prohibited keyword index scratch")?;
    let mut prohibited = BTreeSet::new();
    for line in ctx.admit_iter(lines, "creo prohibited construct line traversal")? {
        let source = ctx.trim_text(&line.text, "creo prohibited construct whitespace trim")?;
        if source.starts_with("/*") {
            continue;
        }
        for keyword in ["if", "else", "endif"] {
            if starts_relation_keyword(ctx, source, keyword)?
                && !ctx.contains_btree_set(
                    &prohibited,
                    keyword,
                    "creo prohibited keyword lookup",
                )?
            {
                let name = ctx.copy_retained_text(keyword, "creo prohibited construct names")?;
                node_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut prohibited, name, "creo prohibited construct nodes")
                })?;
            }
        }
        let bytes = source.as_bytes();
        let mut cursor = 0;
        while cursor < bytes.len() {
            ctx.next_charged(&mut bytes[cursor..].iter(), "creo relation dependency scan")?;
            if matches!(bytes[cursor], b'\'' | b'"') {
                let delimiter = bytes[cursor];
                cursor += 1;
                let tail = &bytes[cursor..];
                cursor += ctx
                    .position_by(
                        tail,
                        |byte| Ok(*byte == delimiter),
                        "creo relation quoted dependency scan",
                    )?
                    .unwrap_or(tail.len());
                cursor += usize::from(bytes.get(cursor) == Some(&delimiter));
                continue;
            }
            if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
                let start = cursor;
                let Some(end) = expression_identifier_end(ctx, bytes, start)? else {
                    cursor += 1;
                    continue;
                };
                cursor = end;
                let mut following = cursor;
                let tail = &bytes[following..];
                following += ctx
                    .position_by(
                        tail,
                        |byte| Ok(!byte.is_ascii_whitespace()),
                        "creo relation following whitespace scan",
                    )?
                    .unwrap_or(tail.len());
                let name = &source[start..end];
                if bytes.get(following) == Some(&b'(') {
                    if let Some(canonical) = PROHIBITED_FUNCTIONS
                        .iter()
                        .copied()
                        .find(|candidate| name.eq_ignore_ascii_case(candidate))
                    {
                        if !ctx.contains_btree_set(
                            &prohibited,
                            canonical,
                            "creo prohibited keyword lookup",
                        )? {
                            let name = ctx
                                .copy_retained_text(canonical, "creo prohibited construct names")?;
                            node_storage.with_storage(|| {
                                ctx.insert_btree_set(
                                    &mut prohibited,
                                    name,
                                    "creo prohibited construct nodes",
                                )
                            })?;
                        }
                    }
                }
                continue;
            }
            cursor += 1;
        }
    }
    let mut ordered = Vec::new();
    ctx.reserve_vec(
        &mut ordered,
        prohibited.len(),
        "creo prohibited construct records",
    )?;
    ordered.extend(prohibited);
    Ok(ordered)
}

#[derive(Default)]
pub(crate) struct ExternalRelationSymbols {
    values: BTreeMap<String, Option<CurveExpressionValue>>,
}

impl ExternalRelationSymbols {
    pub(crate) fn observe(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut name: String,
        value: Option<CurveExpressionValue>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        use std::collections::btree_map::Entry;

        ctx.make_ascii_lowercase(&mut name, "creo relation identifier case fold")?;
        match ctx.entry_btree_map(
            &mut self.values,
            name,
            "creo external relation symbol nodes",
        )? {
            Entry::Vacant(entry) => {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(entry.key().len()),
                    "creo external relation symbol names",
                )?;
                entry.insert(value);
            }
            Entry::Occupied(mut entry) => {
                let same = match (entry.get(), &value) {
                    (
                        Some(CurveExpressionValue::String(left)),
                        Some(CurveExpressionValue::String(right)),
                    ) => ctx.equal(left, right, "creo external relation value comparison")?,
                    (left, right) => left == right,
                };
                if !same {
                    entry.insert(None);
                }
            }
        }
        Ok(())
    }
}

fn expression_assignment(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    line: &CurveExpressionLine,
) -> Result<Option<CurveExpressionAssignment>, cadmpeg_core::CodecError> {
    let source = ctx.trim_text(&line.text, "creo assignment source line trim")?;
    if source.starts_with("/*") {
        return Ok(None);
    }
    let Some((name, expression)) = split_expression_assignment(ctx, source)? else {
        return Ok(None);
    };
    let expression = ctx.trim_text(expression, "creo assignment expression trim")?;
    if expression.is_empty() {
        return Ok(None);
    }
    let Some(target) =
        expression_assignment_target(ctx, ctx.trim_text(name, "creo assignment name trim")?)?
    else {
        return Ok(None);
    };
    let mut dependency_storage = ctx.reserve_scoped(0, "creo dependency name index scratch")?;
    let mut seen_dependencies = HashSet::new();
    let mut dependencies = Vec::<String>::new();
    if let CurveExpressionTarget::TableCell {
        parameter,
        row,
        column,
    } = &target
    {
        ctx.reserve_vec(&mut dependencies, 1, "creo expression dependency names")?;
        dependencies.push(ctx.copy_retained_text(parameter, "creo expression dependency text")?);
        let mut key = dependency_storage
            .with_storage(|| ctx.copy_retained_text(parameter, "creo dependency index key"))?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        dependency_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen_dependencies, key, "creo dependency index nodes")
        })?;
        if extend_expression_dependencies(
            ctx,
            &mut dependencies,
            &mut seen_dependencies,
            &mut dependency_storage,
            row,
        )?
        .is_none()
        {
            return Ok(None);
        }
        if let Some(column) = column {
            if extend_expression_dependencies(
                ctx,
                &mut dependencies,
                &mut seen_dependencies,
                &mut dependency_storage,
                column,
            )?
            .is_none()
            {
                return Ok(None);
            }
        }
    } else if let CurveExpressionTarget::FunctionWrite { arguments, .. } = &target {
        let mut argument_steps = arguments.iter();
        while argument_steps.len() != 0 {
            let Some(argument) = ctx.next_charged(
                &mut argument_steps,
                "creo function target dependency traversal",
            )? else {
                break;
            };
            if extend_expression_dependencies(
                ctx,
                &mut dependencies,
                &mut seen_dependencies,
                &mut dependency_storage,
                argument,
            )?
            .is_none()
            {
                return Ok(None);
            }
        }
    }
    if extend_expression_dependencies(
        ctx,
        &mut dependencies,
        &mut seen_dependencies,
        &mut dependency_storage,
        expression,
    )?
    .is_none()
    {
        return Ok(None);
    }
    Ok(Some(CurveExpressionAssignment {
        target,
        expression: ctx.copy_retained_text(expression, "creo expression assignment text")?,
        dependencies,
        value: None,
        activation: CurveExpressionActivation::Active,
        offset: line.offset,
    }))
}

fn extend_expression_dependencies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    dependencies: &mut Vec<String>,
    seen: &mut HashSet<String>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    expression: &str,
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        ctx.next_charged(&mut bytes[cursor..].iter(), "creo relation dependency scan")?;
        if matches!(bytes[cursor], b'\'' | b'"') {
            let delimiter = bytes[cursor];
            cursor += 1;
            let tail = &bytes[cursor..];
            cursor += ctx
                .position_by(
                    tail,
                    |byte| Ok(*byte == delimiter),
                    "creo relation quoted dependency scan",
                )?
                .unwrap_or(tail.len());
            if bytes.get(cursor) == Some(&delimiter) {
                cursor += 1;
            }
        } else if bytes[cursor].is_ascii_digit()
            || (bytes[cursor] == b'.' && bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit))
        {
            let tail = &bytes[cursor..];
            cursor += ctx
                .position_by(
                    tail,
                    |byte| Ok(!(byte.is_ascii_digit() || *byte == b'.')),
                    "creo dependency number scan",
                )?
                .unwrap_or(tail.len());
            if bytes
                .get(cursor)
                .is_some_and(|byte| matches!(byte, b'e' | b'E'))
                && bytes.get(cursor + 1).is_some_and(|byte| {
                    byte.is_ascii_digit()
                        || (matches!(byte, b'+' | b'-')
                            && bytes.get(cursor + 2).is_some_and(u8::is_ascii_digit))
                })
            {
                cursor += 1;
                if bytes
                    .get(cursor)
                    .is_some_and(|byte| matches!(byte, b'+' | b'-'))
                {
                    cursor += 1;
                }
                let tail = &bytes[cursor..];
                cursor += ctx
                    .position_by(
                        tail,
                        |byte| Ok(!byte.is_ascii_digit()),
                        "creo dependency exponent scan",
                    )?
                    .unwrap_or(tail.len());
            }
        } else if bytes[cursor] == b'[' {
            if let Some(end) = ctx.position_by(
                &bytes[cursor + 1..],
                |byte| Ok(*byte == b']'),
                "creo dependency unit bracket scan",
            )? {
                cursor += end + 2;
            } else {
                cursor += 1;
            }
        } else if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            let Some(end) = expression_identifier_end(ctx, bytes, start)? else {
                return Ok(None);
            };
            cursor = end;
            let dependency = &expression[start..cursor];
            let mut following = cursor;
            let tail = &bytes[following..];
            following += ctx
                .position_by(
                    tail,
                    |byte| Ok(!byte.is_ascii_whitespace()),
                    "creo relation following whitespace scan",
                )?
                .unwrap_or(tail.len());
            let function = bytes.get(following) == Some(&b'(')
                && creo_relation_function(ctx, dependency)?.is_some();
            let constant = reserved_relation_scalar(dependency).is_some();
            if !function && !constant {
                let key_owned_storage =
                    ctx.format_scoped(format_args!("{dependency}"), "creo dependency lookup key")?;
                let _key_storage = key_owned_storage.1;
                let mut key = key_owned_storage.0;
                ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
                if !ctx.contains_hash_set(seen, &key, "creo dependency index lookup")? {
                    storage.with_storage(|| {
                        ctx.insert_hash_set(
                            seen,
                            ctx.copy_retained_text(&key, "creo dependency index key")?,
                            "creo dependency index nodes",
                        )
                    })?;
                    ctx.push_vec(
                        dependencies,
                        ctx.copy_retained_text(dependency, "creo expression dependency text")?,
                        "creo expression dependency names",
                    )?;
                }
            }
        } else {
            cursor += 1;
        }
    }
    Ok(Some(()))
}

fn split_expression_assignment<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
) -> Result<Option<(&'a str, &'a str)>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let bytes = source.as_bytes();
    let mut nesting = 0usize;
    let mut delimiter = None;
    let mut input = bytes.iter().enumerate();
    while input.len() != 0 {
        let Some((cursor, &byte)) =
            ctx.next_charged(&mut input, "creo assignment separator scan")?
        else {
            break;
        };
        if let Some(quote) = delimiter {
            if byte == quote {
                delimiter = None;
            }
            continue;
        }
        match byte {
            quote @ (b'\'' | b'"') => delimiter = Some(quote),
            b'(' => {
                let Some(next) = nesting.checked_add(1) else {
                    return Ok(None);
                };
                nesting = next;
            }
            b')' => {
                let Some(next) = nesting.checked_sub(1) else {
                    return Ok(None);
                };
                nesting = next;
            }
            b'=' if nesting == 0
                && !bytes
                    .get(..cursor)
                    .and_then(|prefix| prefix.last())
                    .is_some_and(|byte| matches!(byte, b'=' | b'!' | b'~' | b'<' | b'>'))
                && bytes.get(cursor + 1) != Some(&b'=') =>
            {
                return Ok(Some((&source[..cursor], &source[cursor + 1..])));
            }
            _ => {}
        }
    }
    Ok(None)
}

#[derive(Default)]
struct CurveExpressionSolveProgram {
    blocks: Vec<CurveExpressionSolveBlock>,
    line_indices: BTreeSet<usize>,
    executable_line_indices: BTreeSet<usize>,
    unresolved_control: bool,
}

struct PendingCurveExpressionSolveBlock<'text, 'budget> {
    statements: Vec<PendingCurveExpressionSolveStatement<'text>>,
    storage: cadmpeg_core::decode::ScopedReservation<'budget>,
    offset: usize,
    valid: bool,
}

struct PendingCurveExpressionSolveStatement<'text> {
    left: &'text str,
    right: &'text str,
    line_index: usize,
}

enum SelectedCurveExpressionSolveStatement<'text, 'budget> {
    Equation {
        statement: PendingCurveExpressionSolveStatement<'text>,
        dependencies: Vec<String>,
        storage: cadmpeg_core::decode::ScopedReservation<'budget>,
    },
    Assignment {
        assignment: CurveExpressionAssignment,
        storage: cadmpeg_core::decode::ScopedReservation<'budget>,
        line_index: usize,
    },
}

fn curve_expression_solve_program(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
    index_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<CurveExpressionSolveProgram, cadmpeg_core::CodecError> {
    let mut program = CurveExpressionSolveProgram::default();
    let mut pending = None::<PendingCurveExpressionSolveBlock<'_, '_>>;
    for (index, line) in ctx
        .admit_iter(lines, "creo solve source line traversal")?
        .enumerate()
    {
        let source = ctx.trim_text(&line.text, "creo solve source line trim")?;
        let Some(block) = pending.as_mut() else {
            if starts_relation_keyword(ctx, source, "solve")? {
                index_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut program.line_indices,
                        index,
                        "creo solve line index nodes",
                    )
                })?;
                pending = Some(PendingCurveExpressionSolveBlock {
                    statements: Vec::new(),
                    storage: ctx.reserve_scoped(0, "creo pending solve statements")?,
                    offset: line.offset,
                    valid: source.eq_ignore_ascii_case("solve"),
                });
            } else if starts_relation_keyword(ctx, source, "for")? {
                index_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut program.line_indices,
                        index,
                        "creo solve line index nodes",
                    )
                })?;
                program.unresolved_control = true;
            }
            continue;
        };
        index_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut program.line_indices,
                index,
                "creo solve line index nodes",
            )
        })?;
        if starts_relation_keyword(ctx, source, "solve")? {
            program.unresolved_control = true;
            block.valid = false;
            continue;
        }
        if starts_relation_keyword(ctx, source, "for")? {
            if !block.valid {
                program.unresolved_control = true;
                pending = None;
                continue;
            }
            let (unknowns, unknown_storage) = ctx.with_scoped_storage(
                "creo solve unknown names",
                || -> Result<_, cadmpeg_core::CodecError> {
                    match conditional_keyword_expression(ctx, source, "for")? {
                        Some(unknowns) => curve_expression_solve_unknowns(ctx, unknowns),
                        None => Ok(None),
                    }
                },
            )?;
            let Some(unknowns) = unknowns else {
                program.unresolved_control = true;
                pending = None;
                continue;
            };
            let mut scratch = ctx.reserve_scoped(0, "creo solve block index scratch")?;
            let mut unknown_names = HashSet::new();
            for unknown in ctx.admit_iter(&unknowns, "creo solve unknown index traversal")? {
                let mut key = scratch.with_storage(|| {
                    ctx.copy_retained_text(&unknown.name, "creo solve unknown index key")
                })?;
                ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
                scratch.with_storage(|| {
                    ctx.insert_hash_set(&mut unknown_names, key, "creo solve unknown index nodes")
                })?;
            }
            let mut selected = Vec::new();
            let mut equation_count = 0;
            let mut assignment_count = 0;
            let mut statements = std::mem::take(&mut block.statements).into_iter();
            while statements.len() != 0 {
                let Some(statement) =
                    ctx.next_charged(&mut statements, "creo pending solve statement traversal")?
                else {
                    break;
                };
                let (dependencies, dependency_storage) = ctx.with_scoped_storage(
                    "creo solve equation dependencies",
                    || -> Result<_, cadmpeg_core::CodecError> {
                        let mut index_storage =
                            ctx.reserve_scoped(0, "creo dependency name index scratch")?;
                        let mut seen = HashSet::new();
                        let mut dependencies = Vec::new();
                        if extend_expression_dependencies(
                            ctx,
                            &mut dependencies,
                            &mut seen,
                            &mut index_storage,
                            statement.left,
                        )?
                        .is_none()
                            || extend_expression_dependencies(
                                ctx,
                                &mut dependencies,
                                &mut seen,
                                &mut index_storage,
                                statement.right,
                            )?
                            .is_none()
                        {
                            return Ok(None);
                        }
                        Ok(Some(dependencies))
                    },
                )?;
                let Some(dependencies) = dependencies else {
                    program.unresolved_control = true;
                    block.valid = false;
                    break;
                };
                let equation = ctx.any_by(
                    &dependencies,
                    |dependency| {
                        let key_owned_storage = ctx.format_scoped(
                            format_args!("{dependency}"),
                            "creo solve dependency lookup key",
                        )?;
                        let _storage = key_owned_storage.1;
                        let mut key = key_owned_storage.0;
                        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
                        ctx.contains_hash_set(
                            &unknown_names,
                            &key,
                            "creo solve dependency unknown lookup",
                        )
                    },
                    "creo relation comparison traversal",
                )?;
                let statement = if equation {
                    equation_count += 1;
                    SelectedCurveExpressionSolveStatement::Equation {
                        statement,
                        dependencies,
                        storage: dependency_storage,
                    }
                } else {
                    drop(dependencies);
                    drop(dependency_storage);
                    let (assignment, storage) = ctx
                        .with_scoped_storage("creo expression assignment text", || {
                            expression_assignment(ctx, &lines[statement.line_index])
                        })?;
                    let Some(assignment) = assignment else {
                        block.valid = false;
                        break;
                    };
                    assignment_count += 1;
                    SelectedCurveExpressionSolveStatement::Assignment {
                        assignment,
                        storage,
                        line_index: statement.line_index,
                    }
                };
                scratch.with_storage(|| {
                    ctx.push_vec(&mut selected, statement, "creo selected solve statements")
                })?;
            }
            drop(statements);
            if block.valid && equation_count != 0 {
                let mut equations = Vec::new();
                let mut assignments = Vec::new();
                ctx.reserve_vec(&mut equations, equation_count, "creo solve equations")?;
                ctx.reserve_vec(&mut assignments, assignment_count, "creo solve assignments")?;
                for statement in
                    ctx.admit_iter(selected, "creo selected solve statement traversal")?
                {
                    match statement {
                        SelectedCurveExpressionSolveStatement::Equation {
                            statement,
                            dependencies,
                            storage,
                        } => {
                            let dependencies = storage.commit_value(dependencies)?;
                            equations.push(CurveExpressionEquation {
                                left: ctx.copy_retained_text(
                                    statement.left,
                                    "creo solve equation left",
                                )?,
                                right: ctx.copy_retained_text(
                                    statement.right,
                                    "creo solve equation right",
                                )?,
                                dependencies,
                                offset: lines[statement.line_index].offset,
                            });
                        }
                        SelectedCurveExpressionSolveStatement::Assignment {
                            assignment,
                            storage,
                            line_index,
                        } => {
                            let assignment = storage.commit_value(assignment)?;
                            index_storage.with_storage(|| {
                                ctx.insert_btree_set(
                                    &mut program.executable_line_indices,
                                    line_index,
                                    "creo executable solve line index nodes",
                                )
                            })?;
                            assignments.push(assignment);
                        }
                    }
                }
                let unknowns = unknown_storage.commit_value(unknowns)?;
                ctx.push_vec(
                    &mut program.blocks,
                    CurveExpressionSolveBlock {
                        equations,
                        assignments,
                        unknowns,
                        offset: block.offset,
                        for_offset: line.offset,
                    },
                    "creo solve blocks",
                )?;
            } else {
                program.unresolved_control = true;
            }
            pending = None;
            continue;
        }
        if !block.valid || source.is_empty() || source.starts_with("/*") {
            continue;
        }
        let Some((left, right)) = split_expression_assignment(ctx, source)? else {
            program.unresolved_control = true;
            block.valid = false;
            continue;
        };
        let left = ctx.trim_text(left, "creo solve left operand trim")?;
        let right = ctx.trim_text(right, "creo solve right operand trim")?;
        if left.is_empty() || right.is_empty() || split_expression_assignment(ctx, right)?.is_some()
        {
            program.unresolved_control = true;
            block.valid = false;
            continue;
        }
        block.storage.with_storage(|| {
            ctx.push_vec(
                &mut block.statements,
                PendingCurveExpressionSolveStatement {
                    left,
                    right,
                    line_index: index,
                },
                "creo pending solve statements",
            )
        })?;
    }
    if pending.is_some() {
        program.unresolved_control = true;
    }
    Ok(program)
}

fn curve_expression_solve_unknowns(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &str,
) -> Result<Option<Vec<SolveUnknown>>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo solve unknown index scratch")?;
    let mut seen = HashSet::new();
    let mut name_storage = ctx.reserve_scoped(0, "creo solve unknown names")?;
    let mut row_storage = ctx.reserve_scoped(0, "creo solve unknowns")?;
    let mut unknowns = Vec::<SolveUnknown>::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let tail = &source.as_bytes()[cursor..];
        cursor += ctx
            .position_by(
                tail,
                |byte| Ok(*byte != b',' && !byte.is_ascii_whitespace()),
                "creo solve unknown separator scan",
            )?
            .unwrap_or(tail.len());
        if cursor == source.len() {
            break;
        }
        let start = cursor;
        let tail = &source.as_bytes()[cursor..];
        cursor += ctx
            .position_by(
                tail,
                |byte| Ok(*byte == b',' || byte.is_ascii_whitespace()),
                "creo solve unknown name scan",
            )?
            .unwrap_or(tail.len());
        let name = &source[start..cursor];
        if !valid_scoped_expression_identifier(ctx, name)? {
            return Ok(None);
        }
        let key_owned_storage =
            ctx.format_scoped(format_args!("{name}"), "creo solve unknown lookup key")?;
        let _key_storage = key_owned_storage.1;
        let mut key = key_owned_storage.0;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        if ctx.contains_hash_set(&seen, &key, "creo solve unknown duplicate checks")? {
            return Ok(None);
        }
        scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut seen,
                ctx.copy_retained_text(&key, "creo solve unknown index key")?,
                "creo solve unknown index nodes",
            )
        })?;
        let name = name_storage
            .with_storage(|| ctx.copy_retained_text(name, "creo solve unknown names"))?;
        row_storage.with_storage(|| {
            ctx.push_vec(
                &mut unknowns,
                SolveUnknown {
                    name,
                    solution: None,
                },
                "creo solve unknowns",
            )
        })?;
    }
    if unknowns.is_empty() {
        return Ok(None);
    }
    let unknowns = name_storage.commit_value(unknowns)?;
    let unknowns = row_storage.commit_value(unknowns)?;
    Ok(Some(unknowns))
}

fn expression_assignment_target(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &str,
) -> Result<Option<CurveExpressionTarget>, cadmpeg_core::CodecError> {
    let mut target_storage = ctx.reserve_scoped(0, "Creo parsed target arguments")?;
    if let Some((name, arguments)) =
        target_storage.with_storage(|| expression_target_function_call(ctx, source))?
    {
        if name.eq_ignore_ascii_case("value") {
            let [parameter, row, rest @ ..] = arguments.as_slice() else {
                return Ok(None);
            };
            if !valid_expression_identifier(ctx, parameter)? {
                return Ok(None);
            }
            let column = match rest {
                [] => None,
                [column] => Some(ctx.copy_retained_text(column, "creo expression table column")?),
                _ => return Ok(None),
            };
            return Ok(Some(CurveExpressionTarget::TableCell {
                parameter: ctx.copy_retained_text(parameter, "creo expression table parameter")?,
                row: ctx.copy_retained_text(row, "creo expression table row")?,
                column,
            }));
        }
        let mut retained_arguments = Vec::new();
        for argument in ctx.admit_iter(arguments, "creo function target argument traversal")? {
            ctx.reserve_vec(
                &mut retained_arguments,
                1,
                "creo expression target arguments",
            )?;
            retained_arguments
                .push(ctx.copy_retained_text(argument, "creo expression target argument text")?);
        }
        return Ok(Some(CurveExpressionTarget::FunctionWrite {
            name: ctx.copy_retained_text(name, "creo expression function target")?,
            arguments: retained_arguments,
        }));
    }
    let (name, declared_unit) = if source.ends_with(']') {
        let Some(unit_start) = ctx.rfind_text(source, "[", "creo target unit bracket scan")? else {
            return Ok(None);
        };
        let Some(unit) = source.get(unit_start + 1..source.len() - 1) else {
            return Ok(None);
        };
        let unit = ctx.trim_text(unit, "creo relation declared unit trim")?;
        if unit.is_empty() {
            return Ok(None);
        }
        let Some(name) = source.get(..unit_start) else {
            return Ok(None);
        };
        (
            ctx.trim_end_text(name, "creo target name trim")?,
            Some(unit),
        )
    } else {
        (source, None)
    };
    if ctx.contains_text(name, ":", "creo target scope scan")? {
        if declared_unit.is_some() || !valid_scoped_expression_identifier(ctx, name)? {
            return Ok(None);
        }
        Ok(Some(CurveExpressionTarget::ScopedSymbol {
            name: ctx.copy_retained_text(name, "creo expression scoped target")?,
        }))
    } else if let Some(family) = expression_system_symbol_family(ctx, name)? {
        if declared_unit.is_some() {
            return Ok(None);
        }
        Ok(Some(CurveExpressionTarget::SystemSymbol {
            name: ctx.copy_retained_text(name, "creo expression system target")?,
            family,
        }))
    } else {
        if !valid_expression_identifier(ctx, name)? {
            return Ok(None);
        }
        let declared_unit = match declared_unit {
            Some(unit) => Some(ctx.copy_retained_text(unit, "creo expression declared unit")?),
            None => None,
        };
        Ok(Some(CurveExpressionTarget::Parameter {
            name: ctx.copy_retained_text(name, "creo expression parameter target")?,
            declared_unit,
        }))
    }
}

fn expression_target_function_call<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
) -> Result<Option<(&'a str, Vec<&'a str>)>, cadmpeg_core::CodecError> {
    let Some(argument_start) = ctx.find_text(source, "(", "creo target function bracket scan")?
    else {
        return Ok(None);
    };
    if !source.ends_with(')') {
        return Ok(None);
    }
    let Some(name) = source.get(..argument_start) else {
        return Ok(None);
    };
    let name = ctx.trim_end_text(name, "creo target name trim")?;
    if !valid_expression_identifier(ctx, name)? {
        return Ok(None);
    }
    let Some(body) = source.get(argument_start + 1..source.len() - 1) else {
        return Ok(None);
    };
    let arguments = if ctx
        .trim_text(body, "creo function target body trim")?
        .is_empty()
    {
        Vec::new()
    } else {
        let Some(arguments) = split_assignment_target_arguments(ctx, body)? else {
            return Ok(None);
        };
        arguments
    };
    Ok(Some((name, arguments)))
}

fn valid_expression_identifier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    name: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    Ok(!name.is_empty()
        && ctx.all_by(
            name.bytes().enumerate(),
            |(index, byte)| {
                Ok(byte == b'_'
                    || byte.is_ascii_alphabetic()
                    || (index > 0 && byte.is_ascii_digit()))
            },
            "creo relation identifier validation",
        )?)
}

fn valid_scoped_expression_identifier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    name: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(expression_identifier_end(ctx, name.as_bytes(), 0)? == Some(name.len()))
}

fn expression_system_symbol_family(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    name: &str,
) -> Result<Option<CurveExpressionSystemSymbolFamily>, cadmpeg_core::CodecError> {
    let Some(digit_start) = ctx
        .position_by(
            name.bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "creo system symbol prefix scan",
        )?
        .filter(|digit_start| *digit_start != 0)
    else {
        return Ok(None);
    };
    let Some(digits) = name.get(digit_start..) else {
        return Ok(None);
    };
    if !ctx.all_by(
        digits.bytes(),
        |byte| Ok(byte.is_ascii_digit()),
        "creo system symbol suffix scan",
    )? {
        return Ok(None);
    }
    let Some(prefix) = name.get(..digit_start) else {
        return Ok(None);
    };
    Ok(if prefix.eq_ignore_ascii_case("d") {
        Some(CurveExpressionSystemSymbolFamily::Dimension)
    } else if prefix.eq_ignore_ascii_case("sd") {
        Some(CurveExpressionSystemSymbolFamily::SectionDimension)
    } else if prefix.eq_ignore_ascii_case("rd") {
        Some(CurveExpressionSystemSymbolFamily::ReferenceDimension)
    } else if prefix.eq_ignore_ascii_case("rsd") {
        Some(CurveExpressionSystemSymbolFamily::SectionReferenceDimension)
    } else if prefix.eq_ignore_ascii_case("kd") {
        Some(CurveExpressionSystemSymbolFamily::KnownDimension)
    } else if prefix.eq_ignore_ascii_case("ad") {
        Some(CurveExpressionSystemSymbolFamily::DrivenDimension)
    } else if prefix.eq_ignore_ascii_case("p") {
        Some(CurveExpressionSystemSymbolFamily::PatternCount)
    } else if ["tpm", "tp", "tm"]
        .iter()
        .any(|family| prefix.eq_ignore_ascii_case(family))
    {
        Some(CurveExpressionSystemSymbolFamily::Tolerance)
    } else {
        None
    })
}

fn split_assignment_target_arguments<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
) -> Result<Option<Vec<&'a str>>, cadmpeg_core::CodecError> {
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut nesting = 0usize;
    let mut delimiter = None;
    let mut input = source.bytes().enumerate();
    while input.len() != 0 {
        let Some((offset, byte)) = ctx.next_charged(&mut input, "creo target argument scan")? else {
            break;
        };
        if let Some(quote) = delimiter {
            if byte == quote {
                delimiter = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => delimiter = Some(byte),
            b'(' => {
                let Some(next) = nesting.checked_add(1) else {
                    return Ok(None);
                };
                nesting = next;
            }
            b')' => {
                let Some(next) = nesting.checked_sub(1) else {
                    return Ok(None);
                };
                nesting = next;
            }
            b',' if nesting == 0 => {
                let Some(argument) = source.get(start..offset) else {
                    return Ok(None);
                };
                let argument = ctx.trim_text(argument, "creo separated target argument trim")?;
                if argument.is_empty() {
                    return Ok(None);
                }
                ctx.reserve_vec(&mut arguments, 1, "creo expression parsed arguments")?;
                arguments.push(argument);
                start = offset + 1;
            }
            _ => {}
        }
    }
    if delimiter.is_some() || nesting != 0 {
        return Ok(None);
    }
    let Some(argument) = source.get(start..) else {
        return Ok(None);
    };
    let argument = ctx.trim_text(argument, "creo final target argument trim")?;
    if argument.is_empty() {
        return Ok(None);
    }
    ctx.reserve_vec(&mut arguments, 1, "creo expression parsed arguments")?;
    arguments.push(argument);
    Ok(Some(arguments))
}

fn reserved_relation_scalar(name: &str) -> Option<f64> {
    if name.eq_ignore_ascii_case("pi") {
        Some(std::f64::consts::PI)
    } else if name.eq_ignore_ascii_case("g") {
        Some(9_800.0)
    } else if name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("yes") {
        Some(1.0)
    } else if name.eq_ignore_ascii_case("false") || name.eq_ignore_ascii_case("no") {
        Some(0.0)
    } else {
        None
    }
}

fn expression_identifier_end(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &[u8],
    start: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if !source
        .get(start)
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic())
    {
        return Ok(None);
    }
    let mut cursor = start + 1;
    loop {
        let tail = &source[cursor..];
        cursor += ctx
            .position_by(
                tail,
                |byte| Ok(!(*byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit())),
                "creo relation identifier scan",
            )?
            .unwrap_or(tail.len());
        if source.get(cursor) != Some(&b':')
            || !source.get(cursor + 1).is_some_and(|byte| {
                *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit()
            })
        {
            return Ok(Some(cursor));
        }
        cursor += 2;
    }
}

#[derive(Debug, Clone)]
struct ConditionalFrame {
    parent: CurveExpressionActivation,
    condition: Option<bool>,
}

#[derive(Default)]
enum ConditionalStack {
    #[default]
    Empty,
    Open {
        frame: ConditionalFrame,
        parents: Vec<ConditionalFrame>,
    },
}

impl ConditionalStack {
    fn push(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        frame: ConditionalFrame,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        match self {
            Self::Empty => {
                *self = Self::Open {
                    frame,
                    parents: Vec::new(),
                };
            }
            Self::Open {
                frame: parent,
                parents,
            } => {
                ctx.reserve_vec(parents, 1, "creo expression conditional parents")?;
                parents.push(std::mem::replace(parent, frame));
            }
        }
        Ok(())
    }

    fn alternative(&self) -> CurveExpressionActivation {
        match self {
            Self::Empty => CurveExpressionActivation::Conditional,
            Self::Open { frame, .. } => branch_activation(frame.parent, frame.condition, true),
        }
    }

    fn end(&mut self) -> CurveExpressionActivation {
        match std::mem::take(self) {
            Self::Empty => CurveExpressionActivation::Conditional,
            Self::Open { frame, mut parents } => {
                if let Some(parent) = parents.pop() {
                    *self = Self::Open {
                        frame: parent,
                        parents,
                    };
                }
                frame.parent
            }
        }
    }
}

fn conditional_keyword_expression<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &'a str,
    keyword: &str,
) -> Result<Option<&'a str>, cadmpeg_core::CodecError> {
    let source = ctx.trim_text(source, "creo relation condition trim")?;
    let Some(prefix) = source.get(..keyword.len()) else {
        return Ok(None);
    };
    if !(prefix.eq_ignore_ascii_case(keyword)) {
        return Ok(None);
    }
    if !(source
        .as_bytes()
        .get(keyword.len())
        .is_some_and(u8::is_ascii_whitespace))
    {
        return Ok(None);
    }
    let Some(expression) = source.get(keyword.len()..) else {
        return Ok(None);
    };
    let expression = ctx.trim_start_text(expression, "creo relation condition expression trim")?;
    Ok((!expression.is_empty()).then_some(expression))
}

fn starts_relation_keyword(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &str,
    keyword: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let source = ctx.trim_text(source, "creo relation keyword trim")?;
    Ok(match source.get(..keyword.len()) {
        Some(prefix) => prefix.eq_ignore_ascii_case(keyword),
        None => false,
    } && source
        .as_bytes()
        .get(keyword.len())
        .is_none_or(u8::is_ascii_whitespace))
}

fn expression_program_control_is_valid(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo conditional validation scratch")?;
    let mut else_seen = Vec::new();
    let mut line_steps = lines.iter();
    while line_steps.len() != 0 {
        let Some(line) =
            ctx.next_charged(&mut line_steps, "creo relation control line traversal")?
        else {
            break;
        };
        let source = ctx.trim_text(&line.text, "creo relation control line trim")?;
        if starts_relation_keyword(ctx, source, "if")? {
            if conditional_keyword_expression(ctx, source, "if")?.is_none() {
                return Ok(false);
            }
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut else_seen, 1, "creo expression conditional validation")
            })?;
            else_seen.push(false);
        } else if starts_relation_keyword(ctx, source, "else")? {
            if !source.eq_ignore_ascii_case("else") {
                return Ok(false);
            }
            let Some(seen) = else_seen.last_mut() else {
                return Ok(false);
            };
            if *seen {
                return Ok(false);
            }
            *seen = true;
        } else if starts_relation_keyword(ctx, source, "endif")?
            && (!source.eq_ignore_ascii_case("endif") || else_seen.pop().is_none())
        {
            return Ok(false);
        }
    }
    Ok(else_seen.is_empty())
}

fn branch_activation(
    parent: CurveExpressionActivation,
    condition: Option<bool>,
    alternative: bool,
) -> CurveExpressionActivation {
    match parent {
        CurveExpressionActivation::Inactive => CurveExpressionActivation::Inactive,
        CurveExpressionActivation::Conditional => CurveExpressionActivation::Conditional,
        CurveExpressionActivation::Active => match condition {
            Some(selected) if selected != alternative => CurveExpressionActivation::Active,
            Some(_) => CurveExpressionActivation::Inactive,
            None => CurveExpressionActivation::Conditional,
        },
    }
}

struct CurveExpressionEvaluation {
    assignments: Vec<CurveExpressionAssignment>,
    solve_solutions: BTreeMap<usize, Vec<CurveExpressionValue>>,
}

fn copy_expression_value(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &CurveExpressionValue,
    operation: &'static str,
) -> Result<CurveExpressionValue, cadmpeg_core::CodecError> {
    match value {
        CurveExpressionValue::String(text) => Ok(CurveExpressionValue::String(
            ctx.copy_retained_text(text, operation)?,
        )),
        value => Ok(value.clone()),
    }
}

fn expression_program_symbols(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    external_symbols: &ExternalRelationSymbols,
    parsed_assignments: &[Option<CurveExpressionAssignment>],
    solve_program: &CurveExpressionSolveProgram,
) -> Result<BTreeSet<String>, cadmpeg_core::CodecError> {
    let mut existing_symbols = BTreeSet::new();
    for (name, _) in ctx.admit_iter(&external_symbols.values, "creo external symbol traversal")? {
        ctx.insert_btree_set(
            &mut existing_symbols,
            ctx.copy_retained_text(name, "creo existing external symbol names")?,
            "creo existing external symbol nodes",
        )?;
    }
    for assignment in ctx
        .admit_iter(parsed_assignments, "creo parsed assignment traversal")?
        .flatten()
    {
        if let Some((name, _)) = assignment.scalar_target() {
            let mut key = ctx.copy_retained_text(name, "creo existing assignment symbol names")?;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            ctx.insert_btree_set(
                &mut existing_symbols,
                key,
                "creo existing assignment symbol nodes",
            )?;
        }
    }
    for block in ctx.admit_iter(&solve_program.blocks, "creo solve symbol block traversal")? {
        for unknown in ctx.admit_iter(&block.unknowns, "creo solve symbol traversal")? {
            let mut key =
                ctx.copy_retained_text(&unknown.name, "creo existing solve symbol names")?;
            ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            ctx.insert_btree_set(
                &mut existing_symbols,
                key,
                "creo existing solve symbol nodes",
            )?;
        }
    }
    Ok(existing_symbols)
}

fn evaluate_expression_program_details(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
    solution_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    solve_program: &CurveExpressionSolveProgram,
    retain_assignment_values: bool,
) -> Result<CurveExpressionEvaluation, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo relation evaluation scratch")?;
    let solve_line_is_executable = |index: &usize| -> Result<bool, cadmpeg_core::CodecError> {
        Ok(
            !ctx.contains_btree_set(&solve_program.line_indices, index, "creo solve line lookup")?
                || ctx.contains_btree_set(
                    &solve_program.executable_line_indices,
                    index,
                    "creo executable solve line lookup",
                )?,
        )
    };
    let mut block_starts = std::collections::HashMap::new();
    let mut block_ends = std::collections::HashMap::new();
    for block in ctx.admit_iter(&solve_program.blocks, "creo solve block index traversal")? {
        scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
            ctx.entry_hash_map(&mut block_starts, block.offset, "creo solve start index")?
                .or_insert(block);
            ctx.entry_hash_map(&mut block_ends, block.for_offset, "creo solve end index")?
                .or_insert(block);
            Ok(())
        })?;
    }
    let mut assignment_indices = std::collections::HashMap::<String, Vec<usize>>::new();
    let control_is_valid = expression_program_control_is_valid(ctx, lines)?;
    let mut parsed_assignments = scratch.with_storage(|| {
        ctx.collect_indexed_vec(
            lines.len(),
            "creo parsed expression assignment slots",
            |_| Ok(None::<CurveExpressionAssignment>),
        )
    })?;
    for (index, line) in ctx
        .admit_iter(lines, "creo relation line traversal")?
        .enumerate()
    {
        if solve_line_is_executable(&index)? {
            parsed_assignments[index] = expression_assignment(ctx, line)?;
        }
    }
    if !control_is_valid {
        let mut assignments = Vec::new();
        for mut assignment in ctx
            .admit_iter(parsed_assignments, "creo conditional assignment traversal")?
            .flatten()
        {
            assignment.activation = CurveExpressionActivation::Conditional;
            ctx.reserve_vec(
                &mut assignments,
                1,
                "creo conditional expression assignments",
            )?;
            assignments.push(assignment);
        }
        return Ok(CurveExpressionEvaluation {
            assignments,
            solve_solutions: BTreeMap::new(),
        });
    }

    let existing_symbols = scratch.with_storage(|| {
        expression_program_symbols(ctx, external_symbols, &parsed_assignments, solve_program)
    })?;
    let context = RelationEvaluationContext {
        model_name,
        existing_symbols: Some(&existing_symbols),
    };
    let mut values = BTreeMap::new();
    let mut defined_symbols = BTreeSet::new();
    for (name, value) in
        ctx.admit_iter(&external_symbols.values, "creo external value traversal")?
    {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut defined_symbols,
                ctx.copy_retained_text(name, "creo defined external symbol names")?,
                "creo defined external symbol nodes",
            )
        })?;
        if let Some(value) = value {
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut values,
                    ctx.copy_retained_text(name, "creo external value names")?,
                    copy_expression_value(ctx, value, "creo external string values")?,
                    "creo external value nodes",
                )
            })?;
        }
    }
    let mut stack = ConditionalStack::default();
    let mut activity = CurveExpressionActivation::Active;
    let mut assignments = Vec::<CurveExpressionAssignment>::new();
    let mut solve_solutions = BTreeMap::new();
    let mut solve_block_dimensions = BTreeMap::new();
    let mut solve_block_initial_values = BTreeMap::new();
    for (index, line) in ctx
        .admit_iter(lines, "creo relation line traversal")?
        .enumerate()
    {
        if let Some(&block) = block_starts.get(&line.offset) {
            let mut dimensions = scratch.with_storage(|| {
                ctx.alloc_filled(block.unknowns.len(), None, "creo solve dimension snapshots")
            })?;
            let mut initial_values = scratch.with_storage(|| {
                ctx.collect_indexed_vec(
                    block.unknowns.len(),
                    "creo solve initial value snapshots",
                    |_| Ok(None),
                )
            })?;
            let mut snapshots = dimensions
                .iter_mut()
                .zip(&mut initial_values)
                .zip(&block.unknowns);
            while let Some(((dimension, initial), unknown)) =
                if block.unknowns.len() <= MAX_NONLINEAR_SOLVE_VARIABLES || snapshots.len() == 0 {
                    snapshots.next()
                } else {
                    ctx.next_charged(&mut snapshots, "creo solve snapshot traversal")?
                }
            {
                let key_owned_storage = ctx.format_scoped(
                    format_args!("{}", unknown.name),
                    "creo solve snapshot lookup",
                )?;
                let _key_guard = key_owned_storage.1;
                let mut key = key_owned_storage.0;
                ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
                let value = ctx.get_btree_map(&values, &key, "creo solve value lookup")?;
                *dimension = value
                    .and_then(quantity_parts_ref)
                    .map(|(_, dimension)| dimension);
                *initial = value
                    .map(|value| {
                        scratch.with_storage(|| {
                            copy_expression_value(ctx, value, "creo solve initial string values")
                        })
                    })
                    .transpose()?;
                ctx.remove_btree_map(&mut values, &key, "creo evaluated value removal")?;
                if !ctx.contains_btree_set(&defined_symbols, &key, "creo defined symbol lookup")? {
                    scratch.with_storage(|| {
                        ctx.insert_btree_set(
                            &mut defined_symbols,
                            ctx.copy_retained_text(&key, "creo defined solve symbol names")?,
                            "creo defined solve symbol nodes",
                        )
                    })?;
                }
                if let Some(indices) =
                    ctx.get_hash_map(&assignment_indices, &key, "creo prior assignment lookup")?
                {
                    for &index in ctx.admit_iter(indices, "creo prior assignment invalidation")? {
                        assignments[index].value = None;
                    }
                }
            }
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut solve_block_dimensions,
                    block.offset,
                    dimensions,
                    "creo solve dimension snapshot nodes",
                )
            })?;
            scratch.with_storage(|| {
                ctx.insert_btree_map(
                    &mut solve_block_initial_values,
                    block.offset,
                    initial_values,
                    "creo solve initial snapshot nodes",
                )
            })?;
        }
        if let Some(&block) = block_ends.get(&line.offset) {
            let affine_solution = match ctx.get_btree_map(
                &solve_block_dimensions,
                &block.offset,
                "creo solve dimension snapshot lookup",
            )? {
                Some(dimensions) => {
                    match scratch.with_storage(|| {
                        infer_solve_variable_dimensions(ctx, block, &values, dimensions, context)
                    })? {
                        Some(dimensions) => solution_storage.with_storage(|| {
                            solve_affine_expression_block(ctx, block, &values, &dimensions, context)
                        })?,
                        None => None,
                    }
                }
                None => None,
            };
            let solution = match affine_solution {
                Some(solution) => Some(solution),
                None => match (
                    ctx.get_btree_map(
                        &solve_block_dimensions,
                        &block.offset,
                        "creo solve dimension snapshot lookup",
                    )?,
                    ctx.get_btree_map(
                        &solve_block_initial_values,
                        &block.offset,
                        "creo solve initial snapshot lookup",
                    )?,
                ) {
                    (Some(dimensions), Some(initial_values)) => {
                        solution_storage.with_storage(|| {
                            solve_nonlinear_expression_block(
                                ctx,
                                block,
                                &values,
                                dimensions,
                                initial_values,
                                context,
                            )
                        })?
                    }
                    _ => None,
                },
            };
            if let Some(solution) = solution {
                install_solve_solution(
                    ctx,
                    block,
                    &solution,
                    &mut assignments,
                    &assignment_indices,
                    &mut values,
                    &mut scratch,
                )?;
                solution_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut solve_solutions,
                        block.offset,
                        solution,
                        "creo solve solution nodes",
                    )
                })?;
            }
        }
        if !solve_line_is_executable(&index)? {
            continue;
        }
        let source = ctx.trim_text(&line.text, "creo evaluated relation line trim")?;
        if let Some(condition_source) = conditional_keyword_expression(ctx, source, "if")? {
            let condition = if activity == CurveExpressionActivation::Active {
                scratch
                    .with_storage(|| {
                        parse_relation_expression::<CurveExpressionValue>(
                            ctx,
                            condition_source,
                            &values,
                            context,
                        )
                    })?
                    .and_then(|value| value.truth())
            } else {
                None
            };
            let parent = activity;
            activity = branch_activation(parent, condition, false);
            scratch.with_storage(|| stack.push(ctx, ConditionalFrame { parent, condition }))?;
            continue;
        }
        if source.eq_ignore_ascii_case("else") {
            activity = stack.alternative();
            continue;
        }
        if source.eq_ignore_ascii_case("endif") {
            activity = stack.end();
            continue;
        }
        let Some(mut assignment) = parsed_assignments.get_mut(index).and_then(Option::take) else {
            continue;
        };
        assignment.activation = activity;
        let Some((name, declared_unit)) = assignment.scalar_target() else {
            ctx.reserve_vec(&mut assignments, 1, "creo evaluated assignments")?;
            assignments.push(assignment);
            continue;
        };
        let mut key =
            scratch.with_storage(|| ctx.copy_retained_text(name, "creo evaluated symbol names"))?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        if !solve_program.blocks.is_empty() {
            scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                let name = ctx.copy_retained_text(&key, "creo assignment index key")?;
                let indices = ctx
                    .entry_hash_map(&mut assignment_indices, name, "creo assignment index nodes")?
                    .or_default();
                ctx.push_vec(indices, assignments.len(), "creo assignment index slots")?;
                Ok(())
            })?;
        }
        let declaration_is_valid = declared_unit.is_none()
            || !ctx.contains_btree_set(&defined_symbols, &key, "creo defined symbol lookup")?;
        if !ctx.contains_btree_set(&defined_symbols, &key, "creo defined symbol lookup")? {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut defined_symbols,
                    ctx.copy_retained_text(&key, "creo defined assignment symbol names")?,
                    "creo defined assignment symbol nodes",
                )
            })?;
        }
        match activity {
            CurveExpressionActivation::Active => {
                assignment.value = if declaration_is_valid {
                    let (value, storage) = ctx.with_scoped_storage(
                        "creo evaluated expression scratch",
                        || -> Result<_, cadmpeg_core::CodecError> {
                            parse_relation_expression::<CurveExpressionValue>(
                                ctx,
                                &assignment.expression,
                                &values,
                                context,
                            )?
                            .map(|value| apply_declared_relation_unit(ctx, value, declared_unit))
                            .transpose()
                            .map(Option::flatten)
                        },
                    )?;
                    let evaluated_string_bytes = match &value {
                        Some(CurveExpressionValue::String(text)) => {
                            Some(cadmpeg_core::decode::u64_from_index(text.len()))
                        }
                        _ => None,
                    };
                    // The parser reservation includes discarded intermediates and this result.
                    // Release it before accounting the escaping string at its destination.
                    drop(storage);
                    if let Some(bytes) = evaluated_string_bytes {
                        solution_storage.with_storage(|| {
                            ctx.charge_retained(bytes, "creo evaluated assignment string")
                        })?;
                    }
                    value
                } else {
                    None
                };
                if let Some(value) = assignment.value.as_ref() {
                    scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                        let entry =
                            ctx.entry_btree_map(&mut values, key, "creo evaluated value nodes")?;
                        let value =
                            copy_expression_value(ctx, value, "creo evaluated string values")?;
                        match entry {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(value);
                            }
                            std::collections::btree_map::Entry::Occupied(mut entry) => {
                                entry.insert(value);
                            }
                        }
                        Ok(())
                    })?;
                } else {
                    ctx.remove_btree_map(&mut values, &key, "creo evaluated value removal")?;
                }
            }
            CurveExpressionActivation::Inactive => {}
            CurveExpressionActivation::Conditional => {
                ctx.remove_btree_map(&mut values, &key, "creo evaluated value removal")?;
            }
        }
        ctx.reserve_vec(&mut assignments, 1, "creo evaluated assignments")?;
        assignments.push(assignment);
    }
    if retain_assignment_values {
        for assignment in
            ctx.admit_iter(&assignments, "creo retained assignment value traversal")?
        {
            if let Some(CurveExpressionValue::String(text)) = &assignment.value {
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(text.len()),
                    "creo evaluated assignment string",
                )?;
            }
        }
    }
    Ok(CurveExpressionEvaluation {
        assignments,
        solve_solutions,
    })
}

fn install_solve_solution(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    block: &CurveExpressionSolveBlock,
    solution: &[CurveExpressionValue],
    assignments: &mut [CurveExpressionAssignment],
    assignment_indices: &HashMap<String, Vec<usize>>,
    values: &mut BTreeMap<String, CurveExpressionValue>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (variable, value) in ctx
        .admit_iter(&block.unknowns, "creo solved variable traversal")?
        .map(|unknown| &unknown.name)
        .zip(solution)
    {
        let mut key =
            scratch.with_storage(|| ctx.copy_retained_text(variable, "creo solved value names"))?;
        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
        if let Some(indices) =
            ctx.get_hash_map(assignment_indices, &key, "creo prior assignment lookup")?
        {
            for &index in ctx.admit_iter(indices, "creo solved assignment traversal")? {
                assignments[index].value = Some(copy_expression_value(
                    ctx,
                    value,
                    "creo assigned solve string values",
                )?);
            }
        }
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                values,
                key,
                copy_expression_value(ctx, value, "creo solved string values")?,
                "creo solved value nodes",
            )
        })?;
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct RelationEvaluationContext<'a> {
    model_name: Option<&'a str>,
    existing_symbols: Option<&'a BTreeSet<String>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RelationDimension {
    length: i8,
    mass: i8,
    time: i8,
    angle: i8,
    temperature: i8,
}

impl RelationDimension {
    const AREA: Self = Self {
        length: 2,
        mass: 0,
        time: 0,
        angle: 0,
        temperature: 0,
    };
    const VOLUME: Self = Self {
        length: 3,
        mass: 0,
        time: 0,
        angle: 0,
        temperature: 0,
    };
    const ENERGY: Self = Self {
        length: 2,
        mass: 1,
        time: -2,
        angle: 0,
        temperature: 0,
    };
    const POWER: Self = Self {
        length: 2,
        mass: 1,
        time: -3,
        angle: 0,
        temperature: 0,
    };
    const PRESSURE: Self = Self {
        length: -1,
        mass: 1,
        time: -2,
        angle: 0,
        temperature: 0,
    };

    const LENGTH: Self = Self {
        length: 1,
        mass: 0,
        time: 0,
        angle: 0,
        temperature: 0,
    };
    const MASS: Self = Self {
        length: 0,
        mass: 1,
        time: 0,
        angle: 0,
        temperature: 0,
    };
    const TIME: Self = Self {
        length: 0,
        mass: 0,
        time: 1,
        angle: 0,
        temperature: 0,
    };
    const ANGLE: Self = Self {
        length: 0,
        mass: 0,
        time: 0,
        angle: 1,
        temperature: 0,
    };
    const TEMPERATURE: Self = Self {
        length: 0,
        mass: 0,
        time: 0,
        angle: 0,
        temperature: 1,
    };
    const FORCE: Self = Self {
        length: 1,
        mass: 1,
        time: -2,
        angle: 0,
        temperature: 0,
    };
    const ACCELERATION: Self = Self {
        length: 1,
        mass: 0,
        time: -2,
        angle: 0,
        temperature: 0,
    };

    fn combine(self, right: Self, subtract: bool) -> Option<Self> {
        let sign = if subtract { -1 } else { 1 };
        Some(Self {
            length: self.length.checked_add(right.length.checked_mul(sign)?)?,
            mass: self.mass.checked_add(right.mass.checked_mul(sign)?)?,
            time: self.time.checked_add(right.time.checked_mul(sign)?)?,
            angle: self.angle.checked_add(right.angle.checked_mul(sign)?)?,
            temperature: self
                .temperature
                .checked_add(right.temperature.checked_mul(sign)?)?,
        })
    }

    fn scale(self, exponent: i8) -> Option<Self> {
        Some(Self {
            length: self.length.checked_mul(exponent)?,
            mass: self.mass.checked_mul(exponent)?,
            time: self.time.checked_mul(exponent)?,
            angle: self.angle.checked_mul(exponent)?,
            temperature: self.temperature.checked_mul(exponent)?,
        })
    }

    fn root(self, degree: i8) -> Option<Self> {
        (degree > 0
            && self.length % degree == 0
            && self.mass % degree == 0
            && self.time % degree == 0
            && self.angle % degree == 0
            && self.temperature % degree == 0)
            .then_some(Self {
                length: self.length / degree,
                mass: self.mass / degree,
                time: self.time / degree,
                angle: self.angle / degree,
                temperature: self.temperature / degree,
            })
    }
}

#[derive(Clone, Copy)]
struct RelationUnit {
    scale: f64,
    offset: f64,
    dimension: RelationDimension,
}

impl RelationUnit {
    fn combine(self, right: Self, divide: bool) -> Option<Self> {
        (self.offset == 0.0 && right.offset == 0.0).then_some(())?;
        let scale = if divide {
            self.scale / right.scale
        } else {
            self.scale * right.scale
        };
        scale.is_finite().then_some(Self {
            scale,
            offset: 0.0,
            dimension: self.dimension.combine(right.dimension, divide)?,
        })
    }

    fn power(self, exponent: i8) -> Option<Self> {
        (self.offset == 0.0).then_some(())?;
        let scale = self.scale.powi(i32::from(exponent));
        scale.is_finite().then_some(Self {
            scale,
            offset: 0.0,
            dimension: self.dimension.scale(exponent)?,
        })
    }
}

fn relation_unit(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    source: &str,
) -> Result<Option<RelationUnit>, cadmpeg_core::CodecError> {
    let mut parser = RelationUnitParser {
        source: source.as_bytes(),
        cursor: 0,
        nesting: 0,
        ctx,
    };
    let unit = parser.expression()?;
    parser.whitespace()?;
    Ok(unit.filter(|_| parser.cursor == parser.source.len()))
}

struct RelationUnitParser<'a> {
    ctx: &'a cadmpeg_core::decode::DecodeContext<'a>,
    source: &'a [u8],
    cursor: usize,
    nesting: usize,
}

impl RelationUnitParser<'_> {
    fn expression(&mut self) -> Result<Option<RelationUnit>, cadmpeg_core::CodecError> {
        let Some(mut unit) = self.power()? else {
            return Ok(None);
        };
        loop {
            self.whitespace()?;
            let divide = match self.source.get(self.cursor) {
                Some(b'*') => false,
                Some(b'/') => true,
                _ => return Ok(Some(unit)),
            };
            self.cursor += 1;
            let Some(right) = self.power()? else {
                return Ok(None);
            };
            let Some(next) = unit.combine(right, divide) else {
                return Ok(None);
            };
            unit = next;
        }
    }

    fn power(&mut self) -> Result<Option<RelationUnit>, cadmpeg_core::CodecError> {
        let Some(unit) = self.primary()? else {
            return Ok(None);
        };
        self.whitespace()?;
        if self.source.get(self.cursor) != Some(&b'^') {
            return Ok(Some(unit));
        }
        self.cursor += 1;
        self.whitespace()?;
        let negative = self.source.get(self.cursor) == Some(&b'-');
        if negative || self.source.get(self.cursor) == Some(&b'+') {
            self.cursor += 1;
        }
        let start = self.cursor;
        {
            let tail = &self.source[self.cursor..];
            self.cursor += self
                .ctx
                .position_by(
                    tail,
                    |byte| Ok(!(byte.is_ascii_digit())),
                    "creo relation unit source scan",
                )?
                .unwrap_or(tail.len());
        }
        let Some(digits) = self.source.get(start..self.cursor) else {
            return Ok(None);
        };
        let Ok(digits) = self.ctx.validate_utf8(digits, "creo UTF-8 validation")? else {
            return Ok(None);
        };
        let Ok(magnitude) = self
            .ctx
            .parse_text::<i16>(digits, "creo relation unit exponent parsing")?
        else {
            return Ok(None);
        };
        let exponent = if negative { -magnitude } else { magnitude };
        let Ok(exponent) = i8::try_from(exponent) else {
            return Ok(None);
        };
        Ok(unit.power(exponent))
    }

    fn primary(&mut self) -> Result<Option<RelationUnit>, cadmpeg_core::CodecError> {
        self.whitespace()?;
        if self.source.get(self.cursor) == Some(&b'(') {
            if self.nesting >= MAX_EXPRESSION_NESTING {
                let error = self.ctx.refuse_codec_limit(
                    "creo relation unit depth",
                    cadmpeg_core::decode::u64_from_index(MAX_EXPRESSION_NESTING),
                    cadmpeg_core::decode::u64_from_index(self.nesting) + 1,
                );
                return Err(error);
            }
            let _depth = self.ctx.enter_nested("creo relation unit depth")?;
            self.cursor += 1;
            self.nesting += 1;
            let Some(unit) = self.expression()? else {
                return Ok(None);
            };
            self.nesting -= 1;
            self.whitespace()?;
            if self.source.get(self.cursor) != Some(&b')') {
                return Ok(None);
            }
            self.cursor += 1;
            return Ok(Some(unit));
        }
        let start = self.cursor;
        {
            let tail = &self.source[self.cursor..];
            self.cursor += self
                .ctx
                .position_by(
                    tail,
                    |byte| Ok(!(byte.is_ascii_alphabetic() || *byte == b'_')),
                    "creo relation unit source scan",
                )?
                .unwrap_or(tail.len());
        }
        let Some(bytes) = self.source.get(start..self.cursor) else {
            return Ok(None);
        };
        let Ok(symbol) = self.ctx.validate_utf8(bytes, "creo UTF-8 validation")? else {
            return Ok(None);
        };
        Ok(relation_unit_symbol(symbol))
    }

    fn whitespace(&mut self) -> Result<(), cadmpeg_core::CodecError> {
        let tail = &self.source[self.cursor..];
        self.cursor += self
            .ctx
            .position_by(
                tail,
                |byte| Ok(!byte.is_ascii_whitespace()),
                "creo relation unit source scan",
            )?
            .unwrap_or(tail.len());
        Ok(())
    }
}

fn relation_unit_symbol(symbol: &str) -> Option<RelationUnit> {
    let (scale, offset, dimension) = match symbol {
        _ if symbol.eq_ignore_ascii_case("k") => (1.0, 0.0, RelationDimension::TEMPERATURE),
        _ if symbol.eq_ignore_ascii_case("c") => (1.0, 273.15, RelationDimension::TEMPERATURE),
        _ if symbol.eq_ignore_ascii_case("f") => (
            5.0 / 9.0,
            459.67 * 5.0 / 9.0,
            RelationDimension::TEMPERATURE,
        ),
        _ if symbol.eq_ignore_ascii_case("r") => (5.0 / 9.0, 0.0, RelationDimension::TEMPERATURE),
        symbol => {
            let (scale, dimension) = multiplicative_relation_unit_symbol(symbol)?;
            (scale, 0.0, dimension)
        }
    };
    Some(RelationUnit {
        scale,
        offset,
        dimension,
    })
}

fn multiplicative_relation_unit_symbol(symbol: &str) -> Option<(f64, RelationDimension)> {
    Some(match symbol {
        _ if symbol.eq_ignore_ascii_case("mm") => (1.0, RelationDimension::LENGTH),
        _ if symbol.eq_ignore_ascii_case("cm") => (10.0, RelationDimension::LENGTH),
        _ if symbol.eq_ignore_ascii_case("m") => (1_000.0, RelationDimension::LENGTH),
        _ if symbol.eq_ignore_ascii_case("in") || symbol.eq_ignore_ascii_case("inch") => {
            (25.4, RelationDimension::LENGTH)
        }
        _ if symbol.eq_ignore_ascii_case("ft") || symbol.eq_ignore_ascii_case("foot") => {
            (304.8, RelationDimension::LENGTH)
        }
        _ if symbol.eq_ignore_ascii_case("micron") => (0.001, RelationDimension::LENGTH),
        _ if symbol.eq_ignore_ascii_case("sq_mm") => (1.0, RelationDimension::AREA),
        _ if symbol.eq_ignore_ascii_case("sq_cm") => (100.0, RelationDimension::AREA),
        _ if symbol.eq_ignore_ascii_case("sq_m") => (1_000_000.0, RelationDimension::AREA),
        _ if symbol.eq_ignore_ascii_case("sq_in") => (645.16, RelationDimension::AREA),
        _ if symbol.eq_ignore_ascii_case("sq_ft") => (92_903.04, RelationDimension::AREA),
        _ if symbol.eq_ignore_ascii_case("cu_mm") => (1.0, RelationDimension::VOLUME),
        _ if symbol.eq_ignore_ascii_case("cu_cm") => (1_000.0, RelationDimension::VOLUME),
        _ if symbol.eq_ignore_ascii_case("cu_m") => (1_000_000_000.0, RelationDimension::VOLUME),
        _ if symbol.eq_ignore_ascii_case("cu_in") => (16_387.064, RelationDimension::VOLUME),
        _ if symbol.eq_ignore_ascii_case("cu_ft") => (28_316_846.592, RelationDimension::VOLUME),
        _ if symbol.eq_ignore_ascii_case("kg") => (1.0, RelationDimension::MASS),
        _ if symbol.eq_ignore_ascii_case("g") => (0.001, RelationDimension::MASS),
        _ if symbol.eq_ignore_ascii_case("mg") => (0.000_001, RelationDimension::MASS),
        _ if symbol.eq_ignore_ascii_case("lb") || symbol.eq_ignore_ascii_case("lbm") => {
            (0.453_592_37, RelationDimension::MASS)
        }
        _ if symbol.eq_ignore_ascii_case("slug") => (14.593_902_937_206_4, RelationDimension::MASS),
        _ if symbol.eq_ignore_ascii_case("tonne") => (1_000.0, RelationDimension::MASS),
        _ if symbol.eq_ignore_ascii_case("s")
            || symbol.eq_ignore_ascii_case("sec")
            || symbol.eq_ignore_ascii_case("second") =>
        {
            (1.0, RelationDimension::TIME)
        }
        _ if symbol.eq_ignore_ascii_case("msec") => (0.001, RelationDimension::TIME),
        _ if symbol.eq_ignore_ascii_case("min") || symbol.eq_ignore_ascii_case("minute") => {
            (60.0, RelationDimension::TIME)
        }
        _ if symbol.eq_ignore_ascii_case("hr") || symbol.eq_ignore_ascii_case("hour") => {
            (3_600.0, RelationDimension::TIME)
        }
        _ if symbol.eq_ignore_ascii_case("day") => (86_400.0, RelationDimension::TIME),
        _ if symbol.eq_ignore_ascii_case("deg") || symbol.eq_ignore_ascii_case("degree") => {
            (1.0, RelationDimension::ANGLE)
        }
        _ if symbol.eq_ignore_ascii_case("rad") || symbol.eq_ignore_ascii_case("radian") => {
            (180.0 / std::f64::consts::PI, RelationDimension::ANGLE)
        }
        _ if symbol.eq_ignore_ascii_case("n") || symbol.eq_ignore_ascii_case("newton") => {
            (1_000.0, RelationDimension::FORCE)
        }
        _ if symbol.eq_ignore_ascii_case("kn") => (1_000_000.0, RelationDimension::FORCE),
        _ if symbol.eq_ignore_ascii_case("dyne") => (0.01, RelationDimension::FORCE),
        _ if symbol.eq_ignore_ascii_case("lbf") => (4_448.221_615_260_5, RelationDimension::FORCE),
        _ if symbol.eq_ignore_ascii_case("ton") => (9_806_650.0, RelationDimension::FORCE),
        _ if symbol.eq_ignore_ascii_case("erg") => (0.1, RelationDimension::ENERGY),
        _ if symbol.eq_ignore_ascii_case("joule") => (1_000_000.0, RelationDimension::ENERGY),
        _ if symbol.eq_ignore_ascii_case("kw") => (1_000_000_000.0, RelationDimension::POWER),
        _ if symbol.eq_ignore_ascii_case("mw") => (1_000_000_000_000.0, RelationDimension::POWER),
        _ if symbol.eq_ignore_ascii_case("pa") => (0.001, RelationDimension::PRESSURE),
        _ if symbol.eq_ignore_ascii_case("mpa") => (1_000.0, RelationDimension::PRESSURE),
        _ if symbol.eq_ignore_ascii_case("gpa") => (1_000_000.0, RelationDimension::PRESSURE),
        _ if symbol.eq_ignore_ascii_case("psi") => {
            (6.894_757_293_168_361, RelationDimension::PRESSURE)
        }
        _ if symbol.eq_ignore_ascii_case("ksi") => {
            (6_894.757_293_168_361, RelationDimension::PRESSURE)
        }
        _ => return None,
    })
}

trait ExpressionValue: Sized {
    fn clone_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError>;
    fn number(value: f64) -> Option<Self>;
    fn reserved(
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(reserved_relation_scalar(name).and_then(Self::number))
    }
    fn string(_value: String) -> Option<Self> {
        None
    }
    fn with_unit_checked(
        self,
        unit: RelationUnit,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn add_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn subtract_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn multiply_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn divide_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn power_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn compare_checked(
        self,
        right: Self,
        operator: ComparisonOperator,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn logical_and_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn logical_or_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn logical_not_checked(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn function_checked(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn negate_checked(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError>;
    fn finite(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError>;
}

impl ExpressionValue for f64 {
    fn clone_admitted(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(*self)
    }

    fn number(value: f64) -> Option<Self> {
        Some(value)
    }

    fn add_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self + right))
    }

    fn with_unit_checked(
        self,
        unit: RelationUnit,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self * unit.scale + unit.offset))
    }

    fn subtract_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self - right))
    }

    fn multiply_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self * right))
    }

    fn divide_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self / right))
    }

    fn power_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self.powf(right)))
    }

    fn compare_checked(
        self,
        right: Self,
        operator: ComparisonOperator,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(f64::from(operator.evaluate(self, right))))
    }

    fn logical_and_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(f64::from(self != 0.0 && right != 0.0)))
    }

    fn logical_or_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(f64::from(self != 0.0 || right != 0.0)))
    }

    fn logical_not_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(f64::from(self == 0.0)))
    }

    fn function_checked(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        _context: RelationEvaluationContext<'_>,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if scope.is_some() {
            return Ok(None);
        }
        Ok(evaluate_creo_math_function(name, arguments))
    }

    fn negate_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(-self))
    }

    fn finite(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(self.is_finite())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AffineValue {
    constant: f64,
    linear: f64,
}

impl ExpressionValue for AffineValue {
    fn clone_admitted(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(*self)
    }

    fn number(value: f64) -> Option<Self> {
        Some(Self {
            constant: value,
            linear: 0.0,
        })
    }

    fn add_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(Self {
            constant: self.constant + right.constant,
            linear: self.linear + right.linear,
        }))
    }

    fn with_unit_checked(
        self,
        unit: RelationUnit,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(Self {
            constant: self.constant * unit.scale + unit.offset,
            linear: self.linear * unit.scale,
        }))
    }

    fn subtract_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(Self {
            constant: self.constant - right.constant,
            linear: self.linear - right.linear,
        }))
    }

    fn multiply_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok((self.linear == 0.0 || right.linear == 0.0).then_some(Self {
            constant: self.constant * right.constant,
            linear: self.constant * right.linear + self.linear * right.constant,
        }))
    }

    fn divide_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(
            (right.linear == 0.0 && right.constant != 0.0).then_some(Self {
                constant: self.constant / right.constant,
                linear: self.linear / right.constant,
            }),
        )
    }

    fn power_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if right.linear == 0.0 && right.constant == 1.0 {
            return Ok(Some(self));
        }
        if right.linear == 0.0 && right.constant == 0.0 {
            return Ok(Self::number(1.0));
        }
        Ok((self.linear == 0.0 && right.linear == 0.0)
            .then(|| self.constant.powf(right.constant))
            .filter(|value| value.is_finite())
            .and_then(Self::number))
    }

    fn compare_checked(
        self,
        right: Self,
        operator: ComparisonOperator,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok((self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(operator.evaluate(self.constant, right.constant))))
            .flatten())
    }

    fn logical_and_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok((self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant != 0.0 && right.constant != 0.0)))
            .flatten())
    }

    fn logical_or_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok((self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant != 0.0 || right.constant != 0.0)))
            .flatten())
    }

    fn logical_not_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok((self.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant == 0.0)))
            .flatten())
    }

    fn function_checked(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        _context: RelationEvaluationContext<'_>,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if scope.is_some() || arguments.len() > 3 {
            return Ok(None);
        }
        let mut constants = [0.0; 3];
        for (slot, argument) in constants.iter_mut().zip(arguments) {
            if argument.linear != 0.0 {
                return Ok(None);
            }
            *slot = argument.constant;
        }
        Ok(evaluate_creo_math_function(name, &constants[..arguments.len()]).and_then(Self::number))
    }

    fn negate_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(Self {
            constant: -self.constant,
            linear: -self.linear,
        }))
    }

    fn finite(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(self.constant.is_finite() && self.linear.is_finite())
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SimultaneousAffineValue {
    dimension: RelationDimension,
    constant: f64,
    coefficients: BTreeMap<String, f64>,
}

impl SimultaneousAffineValue {
    fn constant(value: f64, dimension: RelationDimension) -> Self {
        Self {
            dimension,
            constant: value,
            coefficients: BTreeMap::new(),
        }
    }

    fn scale(
        mut self,
        factor: f64,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        self.constant *= factor;
        if self.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
            for coefficient in self.coefficients.values_mut() {
                *coefficient *= factor;
            }
        } else {
            for (_, coefficient) in
                ctx.admit_iter(&mut self.coefficients, "creo affine arithmetic work")?
            {
                *coefficient *= factor;
            }
        }
        Ok(self)
    }

    fn combine_admitted(
        mut self,
        right: Self,
        subtract: bool,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if self.dimension != right.dimension {
            return Ok(None);
        }
        let sign = if subtract { -1.0 } else { 1.0 };
        self.constant += sign * right.constant;
        let bounded = right.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES;
        let mut coefficients = right.coefficients.into_iter();
        while let Some((variable, coefficient)) = if bounded || coefficients.len() == 0 {
            coefficients.next()
        } else {
            ctx.next_charged(
                &mut coefficients,
                "creo affine coefficient combination work",
            )?
        } {
            if coefficient == 0.0 {
                if ctx
                    .get_btree_map(
                        &self.coefficients,
                        &variable,
                        "creo affine combined coefficient nodes",
                    )?
                    .is_some_and(|value| *value == 0.0)
                {
                    ctx.remove_btree_map(
                        &mut self.coefficients,
                        &variable,
                        "creo affine combined coefficient nodes",
                    )?;
                }
                continue;
            }
            let remove = {
                let Some(existing) = ctx.get_mut_btree_map(
                    &mut self.coefficients,
                    &variable,
                    "creo affine combined coefficient nodes",
                )? else {
                    ctx.insert_btree_map(
                        &mut self.coefficients,
                        variable,
                        sign * coefficient,
                        "creo affine combined coefficient nodes",
                    )?;
                    continue;
                };
                let value = *existing + sign * coefficient;
                if value == 0.0 {
                    true
                } else {
                    *existing = value;
                    false
                }
            };
            if remove {
                ctx.remove_btree_map(
                    &mut self.coefficients,
                    &variable,
                    "creo affine combined coefficient removal work",
                )?;
            }
        }
        Ok(Some(self))
    }

    fn as_curve_value(&self) -> Option<CurveExpressionValue> {
        self.coefficients
            .is_empty()
            .then(|| quantity_value(self.constant, self.dimension))
            .flatten()
    }

    fn constant_difference_admitted(
        &self,
        right: &Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<f64>, cadmpeg_core::CodecError> {
        if self.dimension != right.dimension {
            return Ok(None);
        }
        let bounded = self.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
            && right.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES;
        let mut left_iter = self.coefficients.iter();
        let mut right_iter = right.coefficients.iter();
        let operation = "creo affine coefficient comparison work";
        let mut left = if bounded || left_iter.len() == 0 {
            left_iter.next()
        } else {
            ctx.next_charged(&mut left_iter, operation)?
        };
        let mut right_value = if bounded || right_iter.len() == 0 {
            right_iter.next()
        } else {
            ctx.next_charged(&mut right_iter, operation)?
        };
        while left.is_some() || right_value.is_some() {
            match (left, right_value) {
                (Some((left_name, left_value)), Some((right_name, right_coefficient))) => {
                    match ctx.compare(left_name, right_name, operation)? {
                        std::cmp::Ordering::Equal => {
                            if left_value != right_coefficient {
                                return Ok(None);
                            }
                            left = if bounded || left_iter.len() == 0 {
                                left_iter.next()
                            } else {
                                ctx.next_charged(&mut left_iter, operation)?
                            };
                            right_value = if bounded || right_iter.len() == 0 {
                                right_iter.next()
                            } else {
                                ctx.next_charged(&mut right_iter, operation)?
                            };
                        }
                        std::cmp::Ordering::Less => {
                            if *left_value != 0.0 {
                                return Ok(None);
                            }
                            left = if bounded || left_iter.len() == 0 {
                                left_iter.next()
                            } else {
                                ctx.next_charged(&mut left_iter, operation)?
                            };
                        }
                        std::cmp::Ordering::Greater => {
                            if *right_coefficient != 0.0 {
                                return Ok(None);
                            }
                            right_value = if bounded || right_iter.len() == 0 {
                                right_iter.next()
                            } else {
                                ctx.next_charged(&mut right_iter, operation)?
                            };
                        }
                    }
                }
                (Some((_, coefficient)), None) => {
                    if *coefficient != 0.0 {
                        return Ok(None);
                    }
                    left = if bounded || left_iter.len() == 0 {
                        left_iter.next()
                    } else {
                        ctx.next_charged(&mut left_iter, operation)?
                    };
                }
                (None, Some((_, coefficient))) => {
                    if *coefficient != 0.0 {
                        return Ok(None);
                    }
                    right_value = if bounded || right_iter.len() == 0 {
                        right_iter.next()
                    } else {
                        ctx.next_charged(&mut right_iter, operation)?
                    };
                }
                (None, None) => break,
            }
        }
        Ok((self.constant - right.constant)
            .is_finite()
            .then_some(self.constant - right.constant))
    }

    fn constant_truth(&self) -> Option<bool> {
        (self.dimension == RelationDimension::default() && self.coefficients.is_empty())
            .then_some(self.constant != 0.0)
    }
}

impl ExpressionValue for SimultaneousAffineValue {
    fn clone_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut coefficients = BTreeMap::new();
        let mut coefficient_entries = self.coefficients.iter();
        while let Some((name, value)) = if self.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
            || coefficient_entries.len() == 0
        {
            coefficient_entries.next()
        } else {
            ctx.next_charged(
                &mut coefficient_entries,
                "creo affine coefficient clone traversal",
            )?
        } {
            ctx.insert_btree_map(
                &mut coefficients,
                ctx.copy_retained_text(name, "creo relation affine clone coefficient names")?,
                *value,
                "creo relation affine clone coefficient nodes",
            )?;
        }
        Ok(Self {
            dimension: self.dimension,
            constant: self.constant,
            coefficients,
        })
    }

    fn number(value: f64) -> Option<Self> {
        Some(Self::constant(value, RelationDimension::default()))
    }

    fn reserved(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(value) = CurveExpressionValue::reserved(ctx, name)? else {
            return Ok(None);
        };
        let Some((value, dimension)) = quantity_parts_ref(&value) else {
            return Ok(None);
        };
        Ok(Some(Self::constant(value, dimension)))
    }

    fn function_checked(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if scope.is_some() {
            return Ok(None);
        }
        match (name, arguments) {
            (CreoMathFunction::If, [condition, when_true, when_false]) => {
                if when_true.constant_difference_admitted(when_false, ctx)? == Some(0.0) {
                    return Ok(Some(when_true.clone_admitted(ctx)?));
                }
                let Some(CurveExpressionValue::Number(condition)) = condition.as_curve_value()
                else {
                    return Ok(None);
                };
                return Ok(Some(if condition.get() == 0.0 {
                    when_false.clone_admitted(ctx)?
                } else {
                    when_true.clone_admitted(ctx)?
                }));
            }
            (name @ (CreoMathFunction::Min | CreoMathFunction::Max), [left, right]) => {
                let Some(difference) = left.constant_difference_admitted(right, ctx)? else {
                    return Ok(None);
                };
                let Some(selects_left) = extremum_selects_left(name, difference, 0.0) else {
                    return Ok(None);
                };
                return Ok(Some(if selects_left {
                    left.clone_admitted(ctx)?
                } else {
                    right.clone_admitted(ctx)?
                }));
            }
            (CreoMathFunction::Sign, [value, _])
                if value.coefficients.is_empty() && value.constant == 0.0 =>
            {
                return Ok(Some(value.clone_admitted(ctx)?));
            }
            (CreoMathFunction::Bound, [value, lower, upper]) => {
                let Some(bounds_difference) = lower.constant_difference_admitted(upper, ctx)?
                else {
                    return Ok(None);
                };
                if bounds_difference >= 0.0 {
                    return Ok(None);
                }
                let Some(lower_difference) = value.constant_difference_admitted(lower, ctx)? else {
                    return Ok(None);
                };
                if lower_difference < 0.0 {
                    return Ok(Some(lower.clone_admitted(ctx)?));
                }
                let Some(upper_difference) = value.constant_difference_admitted(upper, ctx)? else {
                    return Ok(None);
                };
                return Ok(Some(if upper_difference > 0.0 {
                    upper.clone_admitted(ctx)?
                } else {
                    value.clone_admitted(ctx)?
                }));
            }
            (CreoMathFunction::Dead, [value, lower, upper]) => {
                let Some(bounds_difference) = lower.constant_difference_admitted(upper, ctx)?
                else {
                    return Ok(None);
                };
                if bounds_difference > 0.0 {
                    return Ok(None);
                }
                let Some(lower_difference) = value.constant_difference_admitted(lower, ctx)? else {
                    return Ok(None);
                };
                if lower_difference < 0.0 {
                    return value.clone_admitted(ctx)?.combine_admitted(
                        lower.clone_admitted(ctx)?,
                        true,
                        ctx,
                    );
                }
                let Some(upper_difference) = value.constant_difference_admitted(upper, ctx)? else {
                    return Ok(None);
                };
                if upper_difference > 0.0 {
                    return value.clone_admitted(ctx)?.combine_admitted(
                        upper.clone_admitted(ctx)?,
                        true,
                        ctx,
                    );
                }
                return Ok(Some(Self::constant(0.0, value.dimension)));
            }
            (CreoMathFunction::Near | CreoMathFunction::DblInTol, [left, right, tolerance]) => {
                let Some(difference) = left.constant_difference_admitted(right, ctx)? else {
                    return Ok(None);
                };
                let Some(tolerance_value) = tolerance.as_curve_value() else {
                    return Ok(None);
                };
                let Some((tolerance, tolerance_dimension)) = quantity_parts_ref(&tolerance_value)
                else {
                    return Ok(None);
                };
                if left.dimension != tolerance_dimension || tolerance < 0.0 {
                    return Ok(None);
                }
                return Ok(Self::number(f64::from(difference.abs() <= tolerance)));
            }
            (CreoMathFunction::Pow, [base, exponent]) => {
                return base
                    .clone_admitted(ctx)?
                    .power_checked(exponent.clone_admitted(ctx)?, ctx);
            }
            _ => {}
        }
        if arguments.len() > 3 {
            return Ok(None);
        }
        let mut numeric_arguments: [CurveExpressionValue; 3] = std::array::from_fn(|_| {
            CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::ZERO)
        });
        for (slot, argument) in numeric_arguments.iter_mut().zip(arguments) {
            let Some(value) = argument.as_curve_value() else {
                return Ok(None);
            };
            *slot = value;
        }
        if matches!(
            name,
            CreoMathFunction::Itos
                | CreoMathFunction::Rtos
                | CreoMathFunction::RelModelName
                | CreoMathFunction::RelModelType
        ) {
            return Ok(None);
        }
        let Some(result) = CurveExpressionValue::function_checked(
            name,
            None,
            &numeric_arguments[..arguments.len()],
            context,
            ctx,
        )?
        else {
            return Ok(None);
        };
        let Some((value, dimension)) = quantity_parts_ref(&result) else {
            return Ok(None);
        };
        Ok(Some(Self::constant(value, dimension)))
    }

    fn with_unit_checked(
        self,
        unit: RelationUnit,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if self.dimension != RelationDimension::default() {
            return Ok(None);
        }
        let mut value = self.scale(unit.scale, ctx)?;
        value.dimension = unit.dimension;
        value.constant += unit.offset;
        Ok(Some(value))
    }

    fn add_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        self.combine_admitted(right, false, ctx)
    }

    fn subtract_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        self.combine_admitted(right, true, ctx)
    }

    fn multiply_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if self.coefficients.is_empty() {
            let Some(dimension) = self.dimension.combine(right.dimension, false) else {
                return Ok(None);
            };
            let mut result = right.scale(self.constant, ctx)?;
            result.dimension = dimension;
            Ok(Some(result))
        } else if right.coefficients.is_empty() {
            let Some(dimension) = self.dimension.combine(right.dimension, false) else {
                return Ok(None);
            };
            let mut result = self.scale(right.constant, ctx)?;
            result.dimension = dimension;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    fn divide_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if !right.coefficients.is_empty() || right.constant == 0.0 {
            return Ok(None);
        }
        let Some(dimension) = self.dimension.combine(right.dimension, true) else {
            return Ok(None);
        };
        let mut result = self.scale(1.0 / right.constant, ctx)?;
        result.dimension = dimension;
        Ok(Some(result))
    }

    fn power_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if !right.coefficients.is_empty() || right.dimension != RelationDimension::default() {
            return Ok(None);
        }
        if right.constant == 1.0 {
            return Ok(Some(self));
        }
        if right.constant == 0.0 {
            return Ok(Self::number(1.0));
        }
        let Some(value) = self.as_curve_value() else {
            return Ok(None);
        };
        let Some(exponent) = cadmpeg_ir::scalar::FiniteReal::new(right.constant) else {
            return Ok(None);
        };
        let Some(result) = quantity_power(&value, &CurveExpressionValue::Number(exponent)) else {
            return Ok(None);
        };
        let Some((value, dimension)) = quantity_parts_ref(&result) else {
            return Ok(None);
        };
        Ok(value.is_finite().then(|| Self::constant(value, dimension)))
    }

    fn compare_checked(
        self,
        right: Self,
        operator: ComparisonOperator,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(difference) = self.constant_difference_admitted(&right, ctx)? else {
            return Ok(None);
        };
        Ok(Self::number(f64::from(operator.evaluate(difference, 0.0))))
    }

    fn logical_and_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if self.constant_truth() == Some(false) || right.constant_truth() == Some(false) {
            return Ok(Self::number(0.0));
        }
        let Some(left) = self.as_curve_value() else {
            return Ok(None);
        };
        let Some(right) = right.as_curve_value() else {
            return Ok(None);
        };
        let Some(CurveExpressionValue::Number(value)) =
            numeric_binary(left, right, |left, right| {
                f64::from(left != 0.0 && right != 0.0)
            })
        else {
            return Ok(None);
        };
        Ok(Self::number(value.get()))
    }

    fn logical_or_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if self.constant_truth() == Some(true) || right.constant_truth() == Some(true) {
            return Ok(Self::number(1.0));
        }
        let Some(left) = self.as_curve_value() else {
            return Ok(None);
        };
        let Some(right) = right.as_curve_value() else {
            return Ok(None);
        };
        let Some(CurveExpressionValue::Number(value)) =
            numeric_binary(left, right, |left, right| {
                f64::from(left != 0.0 || right != 0.0)
            })
        else {
            return Ok(None);
        };
        Ok(Self::number(value.get()))
    }

    fn logical_not_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(value) = self.as_curve_value() else {
            return Ok(None);
        };
        let Some(CurveExpressionValue::Number(value)) = numeric_binary(
            value,
            CurveExpressionValue::Number(cadmpeg_ir::scalar::FiniteReal::ZERO),
            |left, _| f64::from(left == 0.0),
        ) else {
            return Ok(None);
        };
        Ok(Self::number(value.get()))
    }

    fn negate_checked(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(Some(self.scale(-1.0, ctx)?))
    }

    fn finite(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(self.constant.is_finite()
            && if self.coefficients.len() <= MAX_NONLINEAR_SOLVE_VARIABLES {
                self.coefficients.values().all(|value| value.is_finite())
            } else {
                ctx.all_by(
                    &self.coefficients,
                    |(_, value)| Ok(value.is_finite()),
                    "creo affine coefficient finite scan",
                )?
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DimensionRational {
    numerator: i64,
    denominator: i64,
}

impl Default for DimensionRational {
    fn default() -> Self {
        Self {
            numerator: 0,
            denominator: 1,
        }
    }
}

impl DimensionRational {
    fn new(numerator: i64, denominator: i64) -> Option<Self> {
        (denominator != 0).then_some(())?;
        let (numerator, denominator) = if denominator < 0 {
            (numerator.checked_neg()?, denominator.checked_neg()?)
        } else {
            (numerator, denominator)
        };
        let mut left = numerator.unsigned_abs();
        let mut right = denominator.unsigned_abs();
        while right != 0 {
            let remainder = left % right;
            left = right;
            right = remainder;
        }
        let divisor = i64::try_from(left).ok()?;
        Some(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    fn integer(value: i8) -> Self {
        Self {
            numerator: i64::from(value),
            denominator: 1,
        }
    }

    fn one() -> Self {
        Self {
            numerator: 1,
            denominator: 1,
        }
    }

    fn combine(self, right: Self, subtract: bool) -> Option<Self> {
        let sign = if subtract { -1 } else { 1 };
        let numerator = self.numerator.checked_mul(right.denominator)?.checked_add(
            right
                .numerator
                .checked_mul(self.denominator)?
                .checked_mul(sign)?,
        )?;
        let denominator = self.denominator.checked_mul(right.denominator)?;
        Self::new(numerator, denominator)
    }

    fn scale(self, factor: i8) -> Option<Self> {
        Self::new(
            self.numerator.checked_mul(i64::from(factor))?,
            self.denominator,
        )
    }

    fn divide(self, divisor: i16) -> Option<Self> {
        let divisor = i64::from(divisor);
        (divisor != 0).then_some(())?;
        Self::new(self.numerator, self.denominator.checked_mul(divisor)?)
    }

    fn as_f64(self) -> Option<f64> {
        Some(
            cadmpeg_core::convert::f64_from_i64(self.numerator)?
                / cadmpeg_core::convert::f64_from_i64(self.denominator)?,
        )
    }

    fn is_zero(self) -> bool {
        self.numerator == 0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DimensionForm {
    constant: DimensionRational,
    variables: BTreeMap<String, DimensionRational>,
}

impl DimensionForm {
    fn combine_admitted(
        mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        right: Self,
        subtract: bool,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(constant) = self.constant.combine(right.constant, subtract) else {
            return Ok(None);
        };
        self.constant = constant;
        let bounded = right.variables.len() <= MAX_NONLINEAR_SOLVE_VARIABLES;
        let mut variables = right.variables.into_iter();
        while let Some((name, coefficient)) = if bounded || variables.len() == 0 {
            variables.next()
        } else {
            ctx.next_charged(
                &mut variables,
                "creo dimension coefficient combination work",
            )?
        } {
            if coefficient.is_zero() {
                if ctx
                    .get_btree_map(
                        &self.variables,
                        &name,
                        "creo dimension difference variable nodes",
                    )?
                    .is_some_and(|value| value.is_zero())
                {
                    ctx.remove_btree_map(
                        &mut self.variables,
                        &name,
                        "creo dimension difference variable nodes",
                    )?;
                }
                continue;
            }
            let remove = {
                let Some(existing) = ctx.get_mut_btree_map(
                    &mut self.variables,
                    &name,
                    "creo dimension difference variable nodes",
                )? else {
                    let Some(value) =
                        DimensionRational::default().combine(coefficient, subtract)
                    else {
                        return Ok(None);
                    };
                    if !value.is_zero() {
                        ctx.insert_btree_map(
                            &mut self.variables,
                            name,
                            value,
                            "creo dimension difference variable nodes",
                        )?;
                    }
                    continue;
                };
                let Some(value) = (*existing).combine(coefficient, subtract) else {
                    return Ok(None);
                };
                if value.is_zero() {
                    true
                } else {
                    *existing = value;
                    false
                }
            };
            if remove {
                ctx.remove_btree_map(
                    &mut self.variables,
                    &name,
                    "creo dimension difference variable removal work",
                )?;
            }
        }
        Ok(Some(self))
    }

    fn copy_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut variables = BTreeMap::new();
        let mut variable_entries = self.variables.iter();
        while let Some((name, value)) = if self.variables.len() <= MAX_NONLINEAR_SOLVE_VARIABLES
            || variable_entries.len() == 0
        {
            variable_entries.next()
        } else {
            ctx.next_charged(
                &mut variable_entries,
                "creo dimension clone variable traversal",
            )?
        } {
            ctx.insert_btree_map(
                &mut variables,
                ctx.copy_retained_text(name, "creo relation dimension clone variable names")?,
                *value,
                "creo relation dimension clone variable nodes",
            )?;
        }
        Ok(Self {
            constant: self.constant,
            variables,
        })
    }

    fn constant(value: i8) -> Self {
        Self {
            constant: DimensionRational::integer(value),
            variables: BTreeMap::new(),
        }
    }

    fn variable(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: String,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut variables = BTreeMap::new();
        ctx.insert_btree_map(
            &mut variables,
            name,
            DimensionRational::one(),
            "creo dimension variable nodes",
        )?;
        Ok(Self {
            constant: DimensionRational::default(),
            variables,
        })
    }

    fn scale(
        mut self,
        factor: i8,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(constant) = self.constant.scale(factor) else {
            return Ok(None);
        };
        self.constant = constant;
        let bounded = self.variables.len() <= MAX_NONLINEAR_SOLVE_VARIABLES;
        let mut coefficients = self.variables.iter_mut();
        while let Some((_, coefficient)) = if bounded || coefficients.len() == 0 {
            coefficients.next()
        } else {
            ctx.next_charged(&mut coefficients, "creo dimension coefficient scaling")?
        } {
            let Some(value) = (*coefficient).scale(factor) else {
                return Ok(None);
            };
            *coefficient = value;
        }
        ctx.retain_btree_map(
            &mut self.variables,
            |_, coefficient| Ok::<_, cadmpeg_core::CodecError>(!coefficient.is_zero()),
            "creo dimension zero coefficient removal",
        )?;
        Ok(Some(self))
    }

    fn divide_exact(
        mut self,
        divisor: i16,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(constant) = self.constant.divide(divisor) else {
            return Ok(None);
        };
        self.constant = constant;
        let bounded = self.variables.len() <= MAX_NONLINEAR_SOLVE_VARIABLES;
        let mut coefficients = self.variables.iter_mut();
        while let Some((_, coefficient)) = if bounded || coefficients.len() == 0 {
            coefficients.next()
        } else {
            ctx.next_charged(&mut coefficients, "creo dimension coefficient scaling")?
        } {
            let Some(value) = (*coefficient).divide(divisor) else {
                return Ok(None);
            };
            *coefficient = value;
        }
        ctx.retain_btree_map(
            &mut self.variables,
            |_, coefficient| Ok::<_, cadmpeg_core::CodecError>(!coefficient.is_zero()),
            "creo dimension zero coefficient removal",
        )?;
        Ok(Some(self))
    }

    fn is_zero(&self) -> bool {
        self.constant.is_zero() && self.variables.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SymbolicRelationDimension {
    axes: [DimensionForm; 5],
}

impl SymbolicRelationDimension {
    fn copy_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            axes: [
                self.axes[0].copy_admitted(ctx)?,
                self.axes[1].copy_admitted(ctx)?,
                self.axes[2].copy_admitted(ctx)?,
                self.axes[3].copy_admitted(ctx)?,
                self.axes[4].copy_admitted(ctx)?,
            ],
        })
    }

    fn from_relation_dimension(dimension: RelationDimension) -> Self {
        Self {
            axes: [
                DimensionForm::constant(dimension.length),
                DimensionForm::constant(dimension.mass),
                DimensionForm::constant(dimension.time),
                DimensionForm::constant(dimension.angle),
                DimensionForm::constant(dimension.temperature),
            ],
        }
    }

    fn variable(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            axes: [
                DimensionForm::variable(
                    ctx,
                    dimension_variable_key(ctx, name, 0, "creo dimension variable names")?,
                )?,
                DimensionForm::variable(
                    ctx,
                    dimension_variable_key(ctx, name, 1, "creo dimension variable names")?,
                )?,
                DimensionForm::variable(
                    ctx,
                    dimension_variable_key(ctx, name, 2, "creo dimension variable names")?,
                )?,
                DimensionForm::variable(
                    ctx,
                    dimension_variable_key(ctx, name, 3, "creo dimension variable names")?,
                )?,
                DimensionForm::variable(
                    ctx,
                    dimension_variable_key(ctx, name, 4, "creo dimension variable names")?,
                )?,
            ],
        })
    }

    fn combine_admitted(
        self,
        right: Self,
        subtract: bool,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let [left_length, left_mass, left_time, left_angle, left_temperature] = self.axes;
        let [right_length, right_mass, right_time, right_angle, right_temperature] = right.axes;
        let Some(length) = left_length.combine_admitted(ctx, right_length, subtract)? else {
            return Ok(None);
        };
        let Some(mass) = left_mass.combine_admitted(ctx, right_mass, subtract)? else {
            return Ok(None);
        };
        let Some(time) = left_time.combine_admitted(ctx, right_time, subtract)? else {
            return Ok(None);
        };
        let Some(angle) = left_angle.combine_admitted(ctx, right_angle, subtract)? else {
            return Ok(None);
        };
        let Some(temperature) =
            left_temperature.combine_admitted(ctx, right_temperature, subtract)?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            axes: [length, mass, time, angle, temperature],
        }))
    }

    fn scale(
        self,
        factor: i8,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let [length, mass, time, angle, temperature] = self.axes;
        let Some(length) = length.scale(factor, ctx)? else {
            return Ok(None);
        };
        let Some(mass) = mass.scale(factor, ctx)? else {
            return Ok(None);
        };
        let Some(time) = time.scale(factor, ctx)? else {
            return Ok(None);
        };
        let Some(angle) = angle.scale(factor, ctx)? else {
            return Ok(None);
        };
        let Some(temperature) = temperature.scale(factor, ctx)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            axes: [length, mass, time, angle, temperature],
        }))
    }

    fn root(
        self,
        degree: i16,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if degree <= 0 {
            return Ok(None);
        }
        let [length, mass, time, angle, temperature] = self.axes;
        let Some(length) = length.divide_exact(degree, ctx)? else {
            return Ok(None);
        };
        let Some(mass) = mass.divide_exact(degree, ctx)? else {
            return Ok(None);
        };
        let Some(time) = time.divide_exact(degree, ctx)? else {
            return Ok(None);
        };
        let Some(angle) = angle.divide_exact(degree, ctx)? else {
            return Ok(None);
        };
        let Some(temperature) = temperature.divide_exact(degree, ctx)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            axes: [length, mass, time, angle, temperature],
        }))
    }

    fn is_zero(&self) -> bool {
        self.axes.iter().all(DimensionForm::is_zero)
    }
}

fn dimension_variable_key(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    name: &str,
    axis: usize,
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    ctx.format_retained(format_args!("{name}#{axis}"), operation)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DimensionEquality {
    left: SymbolicRelationDimension,
    right: SymbolicRelationDimension,
}

impl DimensionEquality {
    fn copy_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            left: self.left.copy_admitted(ctx)?,
            right: self.right.copy_admitted(ctx)?,
        })
    }
}

#[derive(Debug, Clone)]
enum DimensionProbeKind {
    Numeric(Option<f64>),
    Text(Option<String>),
}

enum DimensionProbeNumber {
    Unknown,
    Known(f64),
}

impl DimensionProbeNumber {
    fn into_option(self) -> Option<f64> {
        match self {
            Self::Unknown => None,
            Self::Known(value) => Some(value),
        }
    }
}

#[derive(Debug, Clone)]
struct DimensionProbeValue {
    dimension: SymbolicRelationDimension,
    kind: DimensionProbeKind,
    constraints: Vec<DimensionEquality>,
}

impl DimensionProbeValue {
    fn numeric(value: Option<f64>) -> Self {
        Self {
            dimension: SymbolicRelationDimension::default(),
            kind: DimensionProbeKind::Numeric(value),
            constraints: Vec::new(),
        }
    }

    fn text(value: Option<String>) -> Self {
        Self {
            dimension: SymbolicRelationDimension::default(),
            kind: DimensionProbeKind::Text(value),
            constraints: Vec::new(),
        }
    }

    fn variable(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            dimension: SymbolicRelationDimension::variable(ctx, name)?,
            kind: DimensionProbeKind::Numeric(None),
            constraints: Vec::new(),
        })
    }

    fn from_relation_value(value: &CurveExpressionValue) -> Option<Self> {
        match value {
            CurveExpressionValue::String(_) => None,
            value => {
                let (value, dimension) = quantity_parts_ref(value)?;
                Some(Self {
                    dimension: SymbolicRelationDimension::from_relation_dimension(dimension),
                    kind: DimensionProbeKind::Numeric(Some(value)),
                    constraints: Vec::new(),
                })
            }
        }
    }

    fn numeric_value(&self) -> Option<f64> {
        match &self.kind {
            DimensionProbeKind::Numeric(value) => *value,
            DimensionProbeKind::Text(_) => None,
        }
    }

    fn text_value(&self) -> Option<&str> {
        match &self.kind {
            DimensionProbeKind::Text(Some(value)) => Some(value),
            DimensionProbeKind::Numeric(_) | DimensionProbeKind::Text(None) => None,
        }
    }

    fn with_constraint_admitted(
        mut self,
        left: SymbolicRelationDimension,
        right: SymbolicRelationDimension,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        ctx.reserve_vec(&mut self.constraints, 1, "creo dimension constraint growth")?;
        self.constraints.push(DimensionEquality { left, right });
        Ok(self)
    }

    fn constrain_to_admitted(
        self,
        dimension: SymbolicRelationDimension,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let current = self.dimension.copy_admitted(ctx)?;
        self.with_constraint_admitted(current, dimension, ctx)
    }

    fn merge_constraints_admitted(
        left: &Self,
        right: &Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<DimensionEquality>, cadmpeg_core::CodecError> {
        let mut constraints = Vec::new();
        for constraint in ctx
            .admit_iter(&left.constraints, "creo dimension constraint traversal")?
            .chain(ctx.admit_iter(&right.constraints, "creo dimension constraint traversal")?)
        {
            ctx.reserve_vec(&mut constraints, 1, "creo dimension merged constraints")?;
            constraints.push(constraint.copy_admitted(ctx)?);
        }
        Ok(constraints)
    }

    fn argument_constraints_admitted(
        arguments: &[Self],
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Vec<DimensionEquality>, cadmpeg_core::CodecError> {
        let mut constraints = Vec::new();
        for argument in ctx.admit_iter(arguments, "creo dimension argument traversal")? {
            for constraint in
                ctx.admit_iter(&argument.constraints, "creo dimension constraint traversal")?
            {
                ctx.reserve_vec(&mut constraints, 1, "creo dimension function constraints")?;
                constraints.push(constraint.copy_admitted(ctx)?);
            }
        }
        Ok(constraints)
    }

    fn push_constraint_admitted(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        constraints: &mut Vec<DimensionEquality>,
        left: &SymbolicRelationDimension,
        right: &SymbolicRelationDimension,
    ) -> Result<(), cadmpeg_core::CodecError> {
        ctx.reserve_vec(constraints, 1, "creo dimension operation constraints")?;
        constraints.push(DimensionEquality {
            left: left.copy_admitted(ctx)?,
            right: right.copy_admitted(ctx)?,
        });
        Ok(())
    }

    fn numeric_result(
        dimension: SymbolicRelationDimension,
        value: Option<f64>,
        constraints: Vec<DimensionEquality>,
    ) -> Self {
        Self {
            dimension,
            kind: DimensionProbeKind::Numeric(value),
            constraints,
        }
    }

    fn text_result(value: Option<String>, constraints: Vec<DimensionEquality>) -> Self {
        Self {
            dimension: SymbolicRelationDimension::default(),
            kind: DimensionProbeKind::Text(value),
            constraints,
        }
    }

    fn optional_math(name: CreoMathFunction, arguments: &[Self]) -> Option<DimensionProbeNumber> {
        let mut values = [0.0; 3];
        for (index, argument) in arguments.iter().enumerate() {
            let slot = values.get_mut(index)?;
            let Some(value) = argument.numeric_value() else {
                return Some(DimensionProbeNumber::Unknown);
            };
            *slot = value;
        }
        evaluate_creo_math_function(name, &values[..arguments.len()])
            .map(DimensionProbeNumber::Known)
    }

    fn optional_round(
        value: &Self,
        decimal_places: Option<&Self>,
        upward: bool,
    ) -> Option<DimensionProbeNumber> {
        let Some(value) = value.numeric_value() else {
            return Some(DimensionProbeNumber::Unknown);
        };
        let decimal_places = match decimal_places {
            Some(decimal_places) => {
                let Some(decimal_places) = decimal_places.numeric_value() else {
                    return Some(DimensionProbeNumber::Unknown);
                };
                decimal_places
            }
            None => 0.0,
        };
        relation_round(value, decimal_places, upward).map(DimensionProbeNumber::Known)
    }
}

impl DimensionProbeValue {
    fn numeric_function_checked(
        name: CreoMathFunction,
        arguments: &[Self],
        mut constraints: Vec<DimensionEquality>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let numeric =
            |name| Self::optional_math(name, arguments).map(DimensionProbeNumber::into_option);
        match (name, arguments) {
            (
                name @ (CreoMathFunction::Sin | CreoMathFunction::Cos | CreoMathFunction::Tan),
                [argument],
            ) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &argument.dimension,
                    &SymbolicRelationDimension::from_relation_dimension(RelationDimension::ANGLE),
                )?;
                let Some(value) = numeric(name) else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                )))
            }
            (
                name @ (CreoMathFunction::Asin | CreoMathFunction::Acos | CreoMathFunction::Atan),
                [argument],
            ) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &argument.dimension,
                    &SymbolicRelationDimension::default(),
                )?;
                let Some(value) = numeric(name) else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::from_relation_dimension(RelationDimension::ANGLE),
                    value,
                    constraints,
                )))
            }
            (CreoMathFunction::Atan2, [left, right]) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &left.dimension,
                    &right.dimension,
                )?;
                let Some(value) = numeric(CreoMathFunction::Atan2) else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::from_relation_dimension(RelationDimension::ANGLE),
                    value,
                    constraints,
                )))
            }
            (
                name @ (CreoMathFunction::Sinh
                | CreoMathFunction::Cosh
                | CreoMathFunction::Tanh
                | CreoMathFunction::Log
                | CreoMathFunction::Ln
                | CreoMathFunction::Exp),
                [argument],
            ) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &argument.dimension,
                    &SymbolicRelationDimension::default(),
                )?;
                let Some(value) = numeric(name) else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                )))
            }
            (CreoMathFunction::Sign, [value, sign]) => {
                let numeric_value =
                    value
                        .numeric_value()
                        .zip(sign.numeric_value())
                        .map(|(value, sign)| {
                            if sign < 0.0 {
                                -value.abs()
                            } else {
                                value.abs()
                            }
                        });
                Ok(Some(Self::numeric_result(
                    value.dimension.copy_admitted(ctx)?,
                    numeric_value,
                    constraints,
                )))
            }
            (CreoMathFunction::Mod, [left, right]) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &left.dimension,
                    &right.dimension,
                )?;
                if right.numeric_value().is_some_and(|value| value == 0.0) {
                    return Ok(None);
                }
                let value = left
                    .numeric_value()
                    .zip(right.numeric_value())
                    .map(|(left, right)| left % right);
                Ok(Some(Self::numeric_result(
                    left.dimension.copy_admitted(ctx)?,
                    value,
                    constraints,
                )))
            }
            (CreoMathFunction::If, [condition, when_true, when_false]) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &condition.dimension,
                    &SymbolicRelationDimension::default(),
                )?;
                match (&when_true.kind, &when_false.kind) {
                    (DimensionProbeKind::Text(left), DimensionProbeKind::Text(right)) => {
                        let value = match condition
                            .numeric_value()
                            .zip(left.as_ref().zip(right.as_ref()))
                        {
                            Some((condition, (left, right))) => {
                                let selected = if condition == 0.0 { right } else { left };
                                Some(ctx.copy_retained_text(
                                    selected,
                                    "creo dimension conditional text",
                                )?)
                            }
                            None => None,
                        };
                        Ok(Some(Self::text_result(value, constraints)))
                    }
                    (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right)) => {
                        Self::push_constraint_admitted(
                            ctx,
                            &mut constraints,
                            &when_true.dimension,
                            &when_false.dimension,
                        )?;
                        let value = condition.numeric_value().zip(left.zip(*right)).map(
                            |(condition, (left, right))| {
                                if condition == 0.0 {
                                    right
                                } else {
                                    left
                                }
                            },
                        );
                        Ok(Some(Self::numeric_result(
                            when_true.dimension.copy_admitted(ctx)?,
                            value,
                            constraints,
                        )))
                    }
                    _ => Ok(None),
                }
            }
            (CreoMathFunction::Bound | CreoMathFunction::Dead, [value, lower, upper]) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &value.dimension,
                    &lower.dimension,
                )?;
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &value.dimension,
                    &upper.dimension,
                )?;
                let numeric = value
                    .numeric_value()
                    .zip(lower.numeric_value())
                    .zip(upper.numeric_value())
                    .and_then(|((value, lower), upper)| {
                        if name == CreoMathFunction::Bound {
                            if lower >= upper {
                                return None;
                            }
                            Some(if value < lower {
                                lower
                            } else if value > upper {
                                upper
                            } else {
                                value
                            })
                        } else {
                            if lower > upper {
                                return None;
                            }
                            Some(if value < lower {
                                value - lower
                            } else if value > upper {
                                value - upper
                            } else {
                                0.0
                            })
                        }
                    });
                Ok(Some(Self::numeric_result(
                    value.dimension.copy_admitted(ctx)?,
                    numeric,
                    constraints,
                )))
            }
            (CreoMathFunction::Near | CreoMathFunction::DblInTol, [left, right, tolerance]) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &left.dimension,
                    &right.dimension,
                )?;
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &left.dimension,
                    &tolerance.dimension,
                )?;
                let numeric = left
                    .numeric_value()
                    .zip(right.numeric_value())
                    .zip(tolerance.numeric_value())
                    .and_then(|((left, right), tolerance)| {
                        (tolerance >= 0.0).then_some(f64::from((left - right).abs() <= tolerance))
                    });
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    numeric,
                    constraints,
                )))
            }
            (name @ (CreoMathFunction::Min | CreoMathFunction::Max), [left, right]) => {
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &left.dimension,
                    &right.dimension,
                )?;
                let numeric =
                    left.numeric_value()
                        .zip(right.numeric_value())
                        .map(|(left, right)| {
                            if extremum_selects_left(name, left, right) == Some(true) {
                                left
                            } else {
                                right
                            }
                        });
                Ok(Some(Self::numeric_result(
                    left.dimension.copy_admitted(ctx)?,
                    numeric,
                    constraints,
                )))
            }
            (CreoMathFunction::Pow, [base, exponent]) => base
                .clone_admitted(ctx)?
                .power_checked(exponent.clone_admitted(ctx)?, ctx),
            (CreoMathFunction::Sqrt, [argument]) => {
                let Some(dimension) = argument.dimension.copy_admitted(ctx)?.root(2, ctx)? else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    dimension,
                    argument.numeric_value().map(f64::sqrt),
                    constraints,
                )))
            }
            (CreoMathFunction::Abs, [argument]) => Ok(Some(Self::numeric_result(
                argument.dimension.copy_admitted(ctx)?,
                argument.numeric_value().map(f64::abs),
                constraints,
            ))),
            (CreoMathFunction::Ceil | CreoMathFunction::Floor, [argument]) => {
                let Some(value) =
                    Self::optional_round(argument, None, name == CreoMathFunction::Ceil)
                else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    argument.dimension.copy_admitted(ctx)?,
                    value.into_option(),
                    constraints,
                )))
            }
            (CreoMathFunction::Ceil | CreoMathFunction::Floor, [argument, decimal_places]) => {
                let decimal_places = decimal_places
                    .clone_admitted(ctx)?
                    .constrain_to_admitted(SymbolicRelationDimension::default(), ctx)?;
                for constraint in ctx.admit_iter(
                    &decimal_places.constraints,
                    "creo dimension control constraint traversal",
                )? {
                    ctx.reserve_vec(
                        &mut constraints,
                        1,
                        "creo dimension function control constraints",
                    )?;
                    constraints.push(constraint.copy_admitted(ctx)?);
                }
                let Some(value) = Self::optional_round(
                    argument,
                    Some(&decimal_places),
                    name == CreoMathFunction::Ceil,
                ) else {
                    return Ok(None);
                };
                Ok(Some(Self::numeric_result(
                    argument.dimension.copy_admitted(ctx)?,
                    value.into_option(),
                    constraints,
                )))
            }
            _ => Ok(None),
        }
    }
}

impl ExpressionValue for DimensionProbeValue {
    fn clone_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let kind = match &self.kind {
            DimensionProbeKind::Numeric(value) => DimensionProbeKind::Numeric(*value),
            DimensionProbeKind::Text(Some(value)) => DimensionProbeKind::Text(Some(
                ctx.copy_retained_text(value, "creo relation dimension clone text")?,
            )),
            DimensionProbeKind::Text(None) => DimensionProbeKind::Text(None),
        };
        let mut constraints = Vec::new();
        ctx.reserve_vec(
            &mut constraints,
            self.constraints.len(),
            "creo relation dimension clone constraints",
        )?;
        for constraint in
            ctx.admit_iter(&self.constraints, "creo dimension constraint traversal")?
        {
            constraints.push(constraint.copy_admitted(ctx)?);
        }
        Ok(Self {
            dimension: self.dimension.copy_admitted(ctx)?,
            kind,
            constraints,
        })
    }

    fn number(value: f64) -> Option<Self> {
        Some(Self::numeric(Some(value)))
    }

    fn reserved(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(value) = CurveExpressionValue::reserved(ctx, name)? else {
            return Ok(None);
        };
        Ok(Self::from_relation_value(&value))
    }

    fn string(value: String) -> Option<Self> {
        Some(Self::text(Some(value)))
    }

    fn with_unit_checked(
        self,
        unit: RelationUnit,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Self {
            dimension,
            kind: DimensionProbeKind::Numeric(value),
            mut constraints,
        } = self
        else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut constraints, 1, "creo dimension unit constraints")?;
        constraints.push(DimensionEquality {
            left: dimension,
            right: SymbolicRelationDimension::default(),
        });
        let value = value
            .map(|value| value * unit.scale + unit.offset)
            .filter(|value| value.is_finite());
        Ok(Some(Self::numeric_result(
            SymbolicRelationDimension::from_relation_dimension(unit.dimension),
            value,
            constraints,
        )))
    }

    fn add_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        match (&self.kind, &right.kind) {
            (DimensionProbeKind::Text(left), DimensionProbeKind::Text(right_value)) => {
                let constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
                let value = match left.as_ref().zip(right_value.as_ref()) {
                    Some((left, right)) => {
                        let mut value =
                            ctx.copy_retained_text(left, "creo dimension text sum left")?;
                        ctx.append_retained(&mut value, right, "creo dimension text sum right")?;
                        Some(value)
                    }
                    None => None,
                };
                Ok(Some(Self::text_result(value, constraints)))
            }
            (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) => {
                let mut constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &self.dimension,
                    &right.dimension,
                )?;
                Ok(Some(Self::numeric_result(
                    self.dimension.copy_admitted(ctx)?,
                    (*left).zip(*right_value).map(|(left, right)| left + right),
                    constraints,
                )))
            }
            _ => Ok(None),
        }
    }

    fn subtract_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return Ok(None);
        };
        let mut constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
        Self::push_constraint_admitted(ctx, &mut constraints, &self.dimension, &right.dimension)?;
        Ok(Some(Self::numeric_result(
            self.dimension.copy_admitted(ctx)?,
            (*left).zip(*right_value).map(|(left, right)| left - right),
            constraints,
        )))
    }

    fn multiply_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return Ok(None);
        };
        let constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
        let value = (*left).zip(*right_value).map(|(left, right)| left * right);
        let Some(dimension) = self
            .dimension
            .combine_admitted(right.dimension, false, ctx)?
        else {
            return Ok(None);
        };
        Ok(Some(Self::numeric_result(dimension, value, constraints)))
    }

    fn divide_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return Ok(None);
        };
        if right_value.is_some_and(|value| value == 0.0) {
            return Ok(None);
        }
        let constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
        let value = (*left).zip(*right_value).map(|(left, right)| left / right);
        let Some(dimension) = self
            .dimension
            .combine_admitted(right.dimension, true, ctx)?
        else {
            return Ok(None);
        };
        Ok(Some(Self::numeric_result(dimension, value, constraints)))
    }

    fn power_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let DimensionProbeKind::Numeric(exponent) = &right.kind else {
            return Ok(None);
        };
        let mut constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
        Self::push_constraint_admitted(
            ctx,
            &mut constraints,
            &right.dimension,
            &SymbolicRelationDimension::default(),
        )?;
        let value = self
            .numeric_value()
            .zip(*exponent)
            .map(|(value, exponent)| value.powf(exponent));
        let base_dimension = self.dimension;
        let dimension = match exponent {
            Some(exponent) if exponent.fract() == 0.0 => {
                if *exponent < f64::from(i8::MIN) || *exponent > f64::from(i8::MAX) {
                    return Ok(None);
                }
                let Some(dimension) = base_dimension.scale(
                    i8::try_from(
                        cadmpeg_core::convert::truncate_f64_to_i32(*exponent).ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo numeric value cannot be represented exactly",
                            )
                        })?,
                    )
                    .map_err(|_| {
                        cadmpeg_core::CodecError::malformed(
                            "Creo numeric value exceeds dimension range",
                        )
                    })?,
                    ctx,
                )?
                else {
                    return Ok(None);
                };
                dimension
            }
            Some(_) if base_dimension.is_zero() => SymbolicRelationDimension::default(),
            Some(_) => return Ok(None),
            None if base_dimension.is_zero() => SymbolicRelationDimension::default(),
            None => return Ok(None),
        };
        Ok(Some(Self::numeric_result(dimension, value, constraints)))
    }

    fn compare_checked(
        self,
        right: Self,
        operator: ComparisonOperator,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        match (&self.kind, &right.kind) {
            (DimensionProbeKind::Text(left), DimensionProbeKind::Text(right_value)) => {
                let value = match operator {
                    ComparisonOperator::Equal => left
                        .as_ref()
                        .zip(right_value.as_ref())
                        .map(|(left, right)| {
                            ctx.equal(left, right, "creo dimension text equality")
                                .map(f64::from)
                        })
                        .transpose()?,
                    ComparisonOperator::NotEqual => left
                        .as_ref()
                        .zip(right_value.as_ref())
                        .map(|(left, right)| {
                            ctx.equal(left, right, "creo dimension text equality")
                                .map(|equal| f64::from(!equal))
                        })
                        .transpose()?,
                    _ => return Ok(None),
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    Self::merge_constraints_admitted(&self, &right, ctx)?,
                )))
            }
            (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) => {
                let mut constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
                Self::push_constraint_admitted(
                    ctx,
                    &mut constraints,
                    &self.dimension,
                    &right.dimension,
                )?;
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    (*left)
                        .zip(*right_value)
                        .map(|(left, right)| f64::from(operator.evaluate(left, right))),
                    constraints,
                )))
            }
            _ => Ok(None),
        }
    }

    fn logical_and_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return Ok(None);
        };
        let mut constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
        Self::push_constraint_admitted(
            ctx,
            &mut constraints,
            &self.dimension,
            &SymbolicRelationDimension::default(),
        )?;
        Self::push_constraint_admitted(
            ctx,
            &mut constraints,
            &right.dimension,
            &SymbolicRelationDimension::default(),
        )?;
        Ok(Some(Self::numeric_result(
            SymbolicRelationDimension::default(),
            (*left)
                .zip(*right_value)
                .map(|(left, right)| f64::from(left != 0.0 && right != 0.0)),
            constraints,
        )))
    }

    fn logical_or_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return Ok(None);
        };
        let mut constraints = Self::merge_constraints_admitted(&self, &right, ctx)?;
        Self::push_constraint_admitted(
            ctx,
            &mut constraints,
            &self.dimension,
            &SymbolicRelationDimension::default(),
        )?;
        Self::push_constraint_admitted(
            ctx,
            &mut constraints,
            &right.dimension,
            &SymbolicRelationDimension::default(),
        )?;
        Ok(Some(Self::numeric_result(
            SymbolicRelationDimension::default(),
            (*left)
                .zip(*right_value)
                .map(|(left, right)| f64::from(left != 0.0 || right != 0.0)),
            constraints,
        )))
    }

    fn logical_not_checked(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let DimensionProbeKind::Numeric(value) = self.kind else {
            return Ok(None);
        };
        let mut constraints = self.constraints;
        ctx.reserve_vec(&mut constraints, 1, "creo dimension negation constraints")?;
        constraints.push(DimensionEquality {
            left: self.dimension,
            right: SymbolicRelationDimension::default(),
        });
        Ok(Some(Self::numeric_result(
            SymbolicRelationDimension::default(),
            value.map(|value| f64::from(value == 0.0)),
            constraints,
        )))
    }

    fn function_checked(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if scope.is_some() {
            return Ok(None);
        }
        let mut constraints = Self::argument_constraints_admitted(arguments, ctx)?;
        match (name, arguments) {
            (CreoMathFunction::Itos, [argument]) => {
                let value = match argument.numeric_value().map(f64::round) {
                    Some(0.0) => Some(String::new()),
                    Some(value) => Some(ctx.format_retained(
                        format_args!("{value:.0}"),
                        "creo dimension integer text",
                    )?),
                    None => None,
                };
                Ok(Some(Self::text_result(value, constraints)))
            }
            (CreoMathFunction::Rtos, [argument, controls @ ..]) => {
                let mut values = [0.0; 2];
                for (index, control) in controls.iter().enumerate() {
                    let Some(slot) = values.get_mut(index) else {
                        return Ok(None);
                    };
                    let control = control
                        .clone_admitted(ctx)?
                        .constrain_to_admitted(SymbolicRelationDimension::default(), ctx)?;
                    for constraint in ctx.admit_iter(
                        &control.constraints,
                        "creo dimension control constraint traversal",
                    )? {
                        ctx.reserve_vec(
                            &mut constraints,
                            1,
                            "creo dimension function control constraints",
                        )?;
                        constraints.push(constraint.copy_admitted(ctx)?);
                    }
                    let Some(value) = control.numeric_value() else {
                        return Ok(None);
                    };
                    *slot = value;
                }
                let (decimals, scientific) = match controls.len() {
                    0 => (None, false),
                    1 => {
                        let Some(decimals) = relation_precision(values[0]) else {
                            return Ok(None);
                        };
                        (Some(decimals), false)
                    }
                    2 => {
                        let Some(decimals) = relation_precision(values[0]) else {
                            return Ok(None);
                        };
                        (Some(decimals), values[1] != 0.0)
                    }
                    _ => return Ok(None),
                };
                let value = match argument.numeric_value() {
                    Some(value) => format_relation_real_admitted(ctx, value, decimals, scientific)?,
                    None => None,
                };
                Ok(Some(Self::text_result(value, constraints)))
            }
            (CreoMathFunction::RelModelName, []) => Ok(Some(Self::text_result(
                context
                    .model_name
                    .map(|name| ctx.copy_retained_text(name, "creo dimension model name text"))
                    .transpose()?,
                constraints,
            ))),
            (CreoMathFunction::RelModelType, []) => Ok(Some(Self::text_result(
                Some(ctx.copy_retained_text("part", "creo dimension model type text")?),
                constraints,
            ))),
            (CreoMathFunction::Exists, [argument]) => {
                let value = match (argument.text_value(), context.existing_symbols) {
                    (Some(name), Some(symbols)) => {
                        let key_owned_storage = ctx.format_scoped(
                            format_args!("{name}"),
                            "creo dimension exists lookup key",
                        )?;
                        let _reservation = key_owned_storage.1;
                        let mut key = key_owned_storage.0;
                        ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
                        ctx.contains_btree_set(symbols, &key, "creo dimension exists lookup")?
                            .then_some(1.0)
                    }
                    _ => None,
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                )))
            }
            (CreoMathFunction::Search, [value, needle]) => {
                let value = match value.text_value().zip(needle.text_value()) {
                    Some((value, needle)) => Some(
                        cadmpeg_core::convert::f64_from_index(
                            match ctx.find_text(value, needle, "creo dimension text search work")? {
                                Some(byte) => ctx
                                    .admit_iter(
                                        &value[..byte],
                                        "creo dimension text search prefix count",
                                    )?
                                    .count()
                                    .checked_add(1)
                                    .ok_or_else(|| {
                                        ctx.refuse_codec_limit(
                                            "creo dimension text search prefix count",
                                            u64::MAX,
                                            u64::MAX,
                                        )
                                    })?,
                                None => 0,
                            },
                        )
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo numeric value cannot be represented exactly",
                            )
                        })?,
                    ),
                    None => None,
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                )))
            }
            (CreoMathFunction::Extract, [value, position, length]) => {
                for control in [position, length] {
                    let control = control
                        .clone_admitted(ctx)?
                        .constrain_to_admitted(SymbolicRelationDimension::default(), ctx)?;
                    for constraint in ctx.admit_iter(
                        &control.constraints,
                        "creo dimension control constraint traversal",
                    )? {
                        ctx.reserve_vec(
                            &mut constraints,
                            1,
                            "creo dimension function control constraints",
                        )?;
                        constraints.push(constraint.copy_admitted(ctx)?);
                    }
                }
                let value = match value
                    .text_value()
                    .zip(position.numeric_value())
                    .zip(length.numeric_value())
                {
                    Some(((value, position), length)) => {
                        if !position.is_finite()
                            || !length.is_finite()
                            || position.fract() != 0.0
                            || length.fract() != 0.0
                            || position <= 0.0
                            || length < 0.0
                        {
                            return Ok(None);
                        }
                        Some(extract_relation_text(
                            ctx,
                            value,
                            position,
                            length,
                            "creo dimension text extract work",
                            "creo dimension extracted text",
                        )?)
                    }
                    None => None,
                };
                Ok(Some(Self::text_result(value, constraints)))
            }
            (CreoMathFunction::StringLength, [value]) => {
                let value = match value.text_value() {
                    Some(value) => Some(
                        cadmpeg_core::convert::f64_from_index(
                            ctx.admit_iter(value, "creo dimension text length work")?
                                .count(),
                        )
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "Creo numeric value cannot be represented exactly",
                            )
                        })?,
                    ),
                    None => None,
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                )))
            }
            (CreoMathFunction::StringStarts, [value, prefix]) => Ok(Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(prefix.text_value())
                    .map(|(value, prefix)| {
                        ctx.starts_with(value, prefix, "creo dimension text prefix")
                            .map(f64::from)
                    })
                    .transpose()?,
                constraints,
            ))),
            (CreoMathFunction::StringEnds, [value, suffix]) => Ok(Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(suffix.text_value())
                    .map(|(value, suffix)| {
                        ctx.ends_with(value, suffix, "creo dimension text suffix")
                            .map(f64::from)
                    })
                    .transpose()?,
                constraints,
            ))),
            (CreoMathFunction::StringMatch, [value, expected]) => Ok(Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(expected.text_value())
                    .map(|(value, expected)| {
                        ctx.equal(value, expected, "creo dimension text match")
                            .map(f64::from)
                    })
                    .transpose()?,
                constraints,
            ))),
            (CreoMathFunction::StringPattern, [value, pattern]) => {
                let value = match value.text_value().zip(pattern.text_value()) {
                    Some((value, pattern)) => {
                        relation_string_pattern_admitted(ctx, value, pattern)?.map(f64::from)
                    }
                    None => None,
                };
                Ok(Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                )))
            }
            _ => Self::numeric_function_checked(name, arguments, constraints, ctx),
        }
    }

    fn negate_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let kind = match self.kind {
            DimensionProbeKind::Numeric(value) => {
                DimensionProbeKind::Numeric(value.map(|value| -value))
            }
            DimensionProbeKind::Text(_) => return Ok(None),
        };
        Ok(Some(Self {
            dimension: self.dimension,
            kind,
            constraints: self.constraints,
        }))
    }

    fn finite(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(match &self.kind {
            DimensionProbeKind::Numeric(Some(value)) => value.is_finite(),
            DimensionProbeKind::Numeric(None) | DimensionProbeKind::Text(_) => true,
        })
    }
}

impl ExpressionValue for CurveExpressionValue {
    fn finite(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        Ok(true)
    }

    fn clone_admitted(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        copy_expression_value(ctx, self, "creo relation referenced string value")
    }

    fn number(value: f64) -> Option<Self> {
        cadmpeg_ir::scalar::FiniteReal::new(value).map(Self::Number)
    }

    fn string(value: String) -> Option<Self> {
        Some(Self::String(value))
    }

    fn reserved(
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(if name.eq_ignore_ascii_case("g") {
            quantity_value(9_800.0, RelationDimension::ACCELERATION)
        } else {
            reserved_relation_scalar(name).and_then(Self::number)
        })
    }

    fn with_unit_checked(
        self,
        unit: RelationUnit,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Self::Number(value) = self else {
            return Ok(None);
        };
        Ok(quantity_value(
            value.get() * unit.scale + unit.offset,
            unit.dimension,
        ))
    }

    fn add_checked(
        self,
        right: Self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(match (self, right) {
            (Self::String(mut left), Self::String(right)) => {
                ctx.append_retained(&mut left, &right, "creo relation string concatenation")?;
                Some(Self::String(left))
            }
            (left, right) => quantity_additive(&left, &right, |left, right| left + right),
        })
    }

    fn subtract_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(quantity_additive(&self, &right, |left, right| left - right))
    }

    fn multiply_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some((left, left_dimension)) = quantity_parts_ref(&self) else {
            return Ok(None);
        };
        let Some((right, right_dimension)) = quantity_parts_ref(&right) else {
            return Ok(None);
        };
        let Some(dimension) = left_dimension.combine(right_dimension, false) else {
            return Ok(None);
        };
        Ok(quantity_value(left * right, dimension))
    }

    fn divide_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some((left, left_dimension)) = quantity_parts_ref(&self) else {
            return Ok(None);
        };
        let Some((right, right_dimension)) = quantity_parts_ref(&right) else {
            return Ok(None);
        };
        let Some(dimension) = left_dimension.combine(right_dimension, true) else {
            return Ok(None);
        };
        Ok(quantity_value(left / right, dimension))
    }

    fn power_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(quantity_power(&self, &right))
    }

    fn compare_checked(
        self,
        right: Self,
        operator: ComparisonOperator,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        if let (Self::String(left), Self::String(right)) = (&self, &right) {
            let equal = match operator {
                ComparisonOperator::Equal | ComparisonOperator::NotEqual => {
                    ctx.equal(left, right, "creo relation text equality")?
                }
                _ => return Ok(None),
            };
            return Ok(Self::number(f64::from(
                if matches!(operator, ComparisonOperator::Equal) {
                    equal
                } else {
                    !equal
                },
            )));
        }
        let result = match (self, right) {
            (Self::Number(left), Self::Number(right)) => operator.evaluate(left.get(), right.get()),
            (left, right) => {
                let Some((left, left_dimension)) = quantity_parts_ref(&left) else {
                    return Ok(None);
                };
                let Some((right, right_dimension)) = quantity_parts_ref(&right) else {
                    return Ok(None);
                };
                if left_dimension != right_dimension {
                    return Ok(None);
                }
                operator.evaluate(left, right)
            }
        };
        Ok(Self::number(f64::from(result)))
    }

    fn logical_and_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(numeric_binary(self, right, |left, right| {
            f64::from(left != 0.0 && right != 0.0)
        }))
    }

    fn logical_or_checked(
        self,
        right: Self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(numeric_binary(self, right, |left, right| {
            f64::from(left != 0.0 || right != 0.0)
        }))
    }

    fn logical_not_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Self::Number(value) = self else {
            return Ok(None);
        };
        Ok(Self::number(f64::from(value.get() == 0.0)))
    }

    fn function_checked(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        use CurveExpressionValue::{Number, String};
        if scope.is_some() {
            return Ok(None);
        }
        match (name, arguments) {
            (CreoMathFunction::Itos, [argument]) => {
                let Some((value, _)) = quantity_parts_ref(argument) else {
                    return Ok(None);
                };
                let rounded = value.round();
                if rounded == 0.0 {
                    return Ok(Some(String(std::string::String::new())));
                }
                Ok(Some(String(ctx.format_retained(
                    format_args!("{rounded:.0}"),
                    "creo relation integer text",
                )?)))
            }
            (CreoMathFunction::Rtos, [argument, controls @ ..]) => {
                let Some((value, _)) = quantity_parts_ref(argument) else {
                    return Ok(None);
                };
                let (decimals, scientific) = match controls {
                    [] => (None, false),
                    [Number(decimals)] => {
                        let Some(decimals) = relation_precision(decimals.get()) else {
                            return Ok(None);
                        };
                        (Some(decimals), false)
                    }
                    [Number(decimals), Number(scientific)] => {
                        let Some(decimals) = relation_precision(decimals.get()) else {
                            return Ok(None);
                        };
                        (Some(decimals), scientific.get() != 0.0)
                    }
                    _ => return Ok(None),
                };
                Ok(format_relation_real_admitted(ctx, value, decimals, scientific)?.map(String))
            }
            (CreoMathFunction::RelModelName, []) => match context.model_name {
                Some(name) => Ok(Some(String(
                    ctx.copy_retained_text(name, "creo relation model name value")?,
                ))),
                None => Ok(None),
            },
            (CreoMathFunction::RelModelType, []) => Ok(Some(String(
                ctx.copy_retained_text("part", "creo relation model type value")?,
            ))),
            (CreoMathFunction::Exists, [String(name)]) => {
                let Some(symbols) = context.existing_symbols else {
                    return Ok(None);
                };
                let key_owned_storage =
                    ctx.format_scoped(format_args!("{name}"), "creo relation exists lookup key")?;
                let _reservation = key_owned_storage.1;
                let mut key = key_owned_storage.0;
                ctx.make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
                Ok(ctx
                    .contains_btree_set(symbols, &key, "creo relation existing symbol lookup")?
                    .then_some(Number(cadmpeg_ir::scalar::FiniteReal::ONE)))
            }
            (CreoMathFunction::Search, [String(value), String(needle)]) => {
                let position =
                    match ctx.find_text(value, needle, "creo relation text search work")? {
                        Some(byte) => ctx
                            .admit_iter(&value[..byte], "creo relation text search prefix count")?
                            .count()
                            .checked_add(1)
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "creo relation text search prefix count",
                                    u64::MAX,
                                    u64::MAX,
                                )
                            })?,
                        None => 0,
                    };
                Ok(Self::number(
                    cadmpeg_core::convert::f64_from_index(position).ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Creo numeric value cannot be represented exactly",
                        )
                    })?,
                ))
            }
            (CreoMathFunction::Extract, [String(value), Number(position), Number(length)]) => {
                if position.get().fract() != 0.0
                    || length.get().fract() != 0.0
                    || position.get() <= 0.0
                    || length.get() < 0.0
                {
                    return Ok(None);
                }
                Ok(Some(String(extract_relation_text(
                    ctx,
                    value,
                    position.get(),
                    length.get(),
                    "creo relation extract scan",
                    "creo relation extracted text",
                )?)))
            }
            (CreoMathFunction::If, [Number(condition), String(when_true), String(when_false)]) => {
                let selected = if condition.get() == 0.0 {
                    when_false
                } else {
                    when_true
                };
                Ok(Some(String(ctx.copy_retained_text(
                    selected,
                    "creo relation conditional string",
                )?)))
            }
            (CreoMathFunction::StringLength, [String(value)]) => Ok(Self::number(
                cadmpeg_core::convert::f64_from_index(
                    ctx.admit_iter(value.as_str(), "creo relation text length work")?
                        .count(),
                )
                .ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Creo numeric value cannot be represented exactly",
                    )
                })?,
            )),
            (CreoMathFunction::StringStarts, [String(value), String(prefix)]) => Ok(Self::number(
                f64::from(ctx.starts_with(value, prefix, "creo relation text prefix work")?),
            )),
            (CreoMathFunction::StringEnds, [String(value), String(suffix)]) => Ok(Self::number(
                f64::from(ctx.ends_with(value, suffix, "creo relation text suffix work")?),
            )),
            (CreoMathFunction::StringMatch, [String(value), String(expected)]) => Ok(Self::number(
                f64::from(ctx.equal(value, expected, "creo relation text match work")?),
            )),
            (CreoMathFunction::StringPattern, [String(value), String(pattern)]) => {
                Ok(relation_string_pattern_admitted(ctx, value, pattern)?
                    .and_then(|matched| Self::number(f64::from(matched))))
            }
            (CreoMathFunction::Pow, [base, Number(exponent)]) => {
                Ok(quantity_power(base, &Number(*exponent)))
            }
            _ => Ok(evaluate_creo_numeric_relation_function(name, arguments)),
        }
    }

    fn negate_checked(
        self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        Ok(quantity_parts_ref(&self)
            .and_then(|(value, dimension)| quantity_value(-value, dimension)))
    }
}

fn quantity_additive(
    left: &CurveExpressionValue,
    right: &CurveExpressionValue,
    operation: impl FnOnce(f64, f64) -> f64,
) -> Option<CurveExpressionValue> {
    let (left, left_dimension) = quantity_parts_ref(left)?;
    let (right, right_dimension) = quantity_parts_ref(right)?;
    (left_dimension == right_dimension).then_some(())?;
    quantity_value(operation(left, right), left_dimension)
}

fn quantity_parts_ref(value: &CurveExpressionValue) -> Option<(f64, RelationDimension)> {
    match value {
        CurveExpressionValue::Number(value) => Some((value.get(), RelationDimension::default())),
        CurveExpressionValue::Length(value) => Some((value.get(), RelationDimension::LENGTH)),
        CurveExpressionValue::Angle(value) => Some((value.get(), RelationDimension::ANGLE)),
        CurveExpressionValue::Quantity(value) => Some((value.value(), value.dimension())),
        CurveExpressionValue::String(_) => None,
    }
}

fn quantity_value(value: f64, dimension: RelationDimension) -> Option<CurveExpressionValue> {
    let value = cadmpeg_ir::scalar::FiniteReal::new(value)?;
    Some(if dimension == RelationDimension::default() {
        CurveExpressionValue::Number(value)
    } else if dimension == RelationDimension::LENGTH {
        CurveExpressionValue::Length(value)
    } else if dimension == RelationDimension::ANGLE {
        CurveExpressionValue::Angle(value)
    } else {
        CurveExpressionValue::Quantity(CurveExpressionQuantity::new(
            value.get(),
            [
                dimension.length,
                dimension.mass,
                dimension.time,
                dimension.angle,
                dimension.temperature,
            ],
        )?)
    })
}

fn numeric_binary(
    left: CurveExpressionValue,
    right: CurveExpressionValue,
    operation: impl FnOnce(f64, f64) -> f64,
) -> Option<CurveExpressionValue> {
    match (left, right) {
        (CurveExpressionValue::Number(left), CurveExpressionValue::Number(right)) => {
            CurveExpressionValue::number(operation(left.get(), right.get()))
        }
        _ => None,
    }
}

struct ExpressionParser<'a, V> {
    source: &'a [u8],
    cursor: usize,
    values: &'a BTreeMap<String, V>,
    context: RelationEvaluationContext<'a>,
    ctx: &'a cadmpeg_core::decode::DecodeContext<'a>,
    nesting: usize,
}

const MAX_EXPRESSION_NESTING: usize = 128;

/// A parse step's value, or the refusal that stopped it. The refusal is boxed
/// so each recursive frame holds a pointer rather than the whole error: the
/// recursion is bounded by `MAX_EXPRESSION_NESTING`, and its stack use by the
/// size of these frames.
type Parsed<V> = Result<Option<V>, Box<cadmpeg_core::CodecError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonOperator {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

impl ComparisonOperator {
    fn evaluate(self, left: f64, right: f64) -> bool {
        match self {
            Self::Equal => left == right,
            Self::NotEqual => left != right,
            Self::Greater => left > right,
            Self::GreaterOrEqual => left >= right,
            Self::Less => left < right,
            Self::LessOrEqual => left <= right,
        }
    }
}

impl<V: ExpressionValue> ExpressionParser<'_, V> {
    fn finite_value(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: V,
    ) -> Result<Option<V>, cadmpeg_core::CodecError> {
        Ok(value.finite(ctx)?.then_some(value))
    }

    /// The finite value of a checked operation.
    fn finite_result(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        result: Result<Option<V>, cadmpeg_core::CodecError>,
    ) -> Parsed<V> {
        Ok(result?
            .map(|value| Self::finite_value(ctx, value))
            .transpose()?
            .flatten())
    }

    fn whitespace(&mut self) -> Result<(), Box<cadmpeg_core::CodecError>> {
        let tail = &self.source[self.cursor..];
        self.cursor += self
            .ctx
            .position_by(
                tail,
                |byte| Ok(!byte.is_ascii_whitespace()),
                "creo relation source scan",
            )?
            .unwrap_or(tail.len());
        Ok(())
    }

    fn logical_or(&mut self) -> Parsed<V> {
        let Some(mut value) = self.logical_and()? else {
            return Ok(None);
        };
        loop {
            self.whitespace()?;
            if self.source.get(self.cursor) != Some(&b'|') {
                return Ok(Some(value));
            }
            self.cursor += 1;
            let Some(right) = self.logical_and()? else {
                return Ok(None);
            };
            let Some(next) =
                Self::finite_result(self.ctx, value.logical_or_checked(right, self.ctx))?
            else {
                return Ok(None);
            };
            value = next;
        }
    }

    fn logical_and(&mut self) -> Parsed<V> {
        let Some(mut value) = self.comparison()? else {
            return Ok(None);
        };
        loop {
            self.whitespace()?;
            if self.source.get(self.cursor) != Some(&b'&') {
                return Ok(Some(value));
            }
            self.cursor += 1;
            let Some(right) = self.comparison()? else {
                return Ok(None);
            };
            let Some(next) =
                Self::finite_result(self.ctx, value.logical_and_checked(right, self.ctx))?
            else {
                return Ok(None);
            };
            value = next;
        }
    }

    fn comparison(&mut self) -> Parsed<V> {
        let Some(value) = self.expression()? else {
            return Ok(None);
        };
        self.whitespace()?;
        let (operator, width) = match self.source.get(self.cursor..) {
            Some([b'=', b'=', ..]) => (ComparisonOperator::Equal, 2),
            Some([b'!' | b'~', b'=', ..] | [b'<', b'>', ..]) => (ComparisonOperator::NotEqual, 2),
            Some([b'>', b'=', ..]) => (ComparisonOperator::GreaterOrEqual, 2),
            Some([b'<', b'=', ..]) => (ComparisonOperator::LessOrEqual, 2),
            Some([b'>', ..]) => (ComparisonOperator::Greater, 1),
            Some([b'<', ..]) => (ComparisonOperator::Less, 1),
            _ => return Ok(Some(value)),
        };
        self.cursor += width;
        let Some(right) = self.expression()? else {
            return Ok(None);
        };
        Self::finite_result(self.ctx, value.compare_checked(right, operator, self.ctx))
    }

    fn expression(&mut self) -> Parsed<V> {
        let Some(mut value) = self.term()? else {
            return Ok(None);
        };
        loop {
            self.whitespace()?;
            let subtract = match self.source.get(self.cursor) {
                Some(b'+') => false,
                Some(b'-') => true,
                _ => return Ok(Some(value)),
            };
            self.cursor += 1;
            let Some(right) = self.term()? else {
                return Ok(None);
            };
            let result = if subtract {
                value.subtract_checked(right, self.ctx)
            } else {
                value.add_checked(right, self.ctx)
            };
            let Some(next) = Self::finite_result(self.ctx, result)? else {
                return Ok(None);
            };
            value = next;
        }
    }

    fn term(&mut self) -> Parsed<V> {
        let Some(mut value) = self.unary()? else {
            return Ok(None);
        };
        loop {
            self.whitespace()?;
            let divide = match self.source.get(self.cursor) {
                Some(b'*') => false,
                Some(b'/') => true,
                _ => return Ok(Some(value)),
            };
            self.cursor += 1;
            let Some(right) = self.unary()? else {
                return Ok(None);
            };
            let result = if divide {
                value.divide_checked(right, self.ctx)
            } else {
                value.multiply_checked(right, self.ctx)
            };
            let Some(next) = Self::finite_result(self.ctx, result)? else {
                return Ok(None);
            };
            value = next;
        }
    }

    fn unary(&mut self) -> Parsed<V> {
        self.whitespace()?;
        let start = self.cursor;
        while let Some(b'+' | b'-' | b'!' | b'~') = self.source.get(self.cursor) {
            self.ctx.next_charged(
                &mut self.source[self.cursor..].iter(),
                "creo relation source scan",
            )?;
            self.cursor += 1;
            self.whitespace()?;
        }
        let end = self.cursor;
        let Some(mut value) = self.power()? else {
            return Ok(None);
        };
        let mut operators = (start..end).rev();
        while operators.len() != 0 {
            let Some(index) = self
                .ctx
                .next_charged(&mut operators, "creo relation unary replay")?
            else {
                break;
            };
            let result = match self.source[index] {
                b'-' => value.negate_checked(self.ctx),
                b'!' | b'~' => value.logical_not_checked(self.ctx),
                _ => continue,
            };
            let Some(next) = Self::finite_result(self.ctx, result)? else {
                return Ok(None);
            };
            value = next;
        }
        Ok(Some(value))
    }

    /// Refuses one more level of nesting past the parser's ceiling.
    fn nesting_ceiling(&self) -> Result<(), Box<cadmpeg_core::CodecError>> {
        if self.nesting < MAX_EXPRESSION_NESTING {
            return Ok(());
        }
        Err(Box::new(self.ctx.refuse_codec_limit(
            "creo relation nesting ceiling",
            cadmpeg_core::decode::u64_from_index(MAX_EXPRESSION_NESTING),
            cadmpeg_core::decode::u64_from_index(self.nesting) + 1,
        )))
    }

    fn power(&mut self) -> Parsed<V> {
        let Some(value) = self.primary()? else {
            return Ok(None);
        };
        self.whitespace()?;
        if self.source.get(self.cursor) != Some(&b'^') {
            return Ok(Some(value));
        }
        self.nesting_ceiling()?;
        let _depth = self.ctx.enter_nested("creo relation exponent depth")?;
        self.cursor += 1;
        self.nesting += 1;
        let Some(exponent) = self.unary()? else {
            return Ok(None);
        };
        self.nesting -= 1;
        Self::finite_result(self.ctx, value.power_checked(exponent, self.ctx))
    }

    fn primary(&mut self) -> Parsed<V> {
        self.whitespace()?;
        let Some(&first) = self.source.get(self.cursor) else {
            return Ok(None);
        };
        let value = match first {
            b'(' => self.group()?,
            byte if byte.is_ascii_digit() || byte == b'.' => self.number()?,
            b'\'' | b'"' => self.string()?,
            byte if byte.is_ascii_alphabetic() || byte == b'_' => self.identifier_or_function()?,
            _ => None,
        };
        let Some(mut value) = value else {
            return Ok(None);
        };
        self.whitespace()?;
        if self.source.get(self.cursor) == Some(&b'[') {
            let unit_start = self.cursor + 1;
            let Some(unit_length) = self.ctx.position_by(
                &self.source[unit_start..],
                |byte| Ok(*byte == b']'),
                "creo relation unit bracket scan",
            )?
            else {
                return Ok(None);
            };
            let unit_end = unit_start + unit_length;
            let Ok(unit) = self
                .ctx
                .validate_utf8(&self.source[unit_start..unit_end], "creo UTF-8 validation")?
            else {
                return Ok(None);
            };
            let Some(unit) = relation_unit(self.ctx, unit)? else {
                return Ok(None);
            };
            let Some(next) =
                Self::finite_result(self.ctx, value.with_unit_checked(unit, self.ctx))?
            else {
                return Ok(None);
            };
            value = next;
            self.cursor = unit_end + 1;
        }
        Ok(Self::finite_value(self.ctx, value)?)
    }

    /// A parenthesized expression, one nesting level deeper.
    fn group(&mut self) -> Parsed<V> {
        self.nesting_ceiling()?;
        let _depth = self.ctx.enter_nested("creo relation group depth")?;
        self.cursor += 1;
        self.nesting += 1;
        let Some(value) = self.logical_or()? else {
            return Ok(None);
        };
        self.nesting -= 1;
        self.whitespace()?;
        if self.source.get(self.cursor) != Some(&b')') {
            return Ok(None);
        }
        self.cursor += 1;
        Ok(Some(value))
    }

    fn string(&mut self) -> Parsed<V> {
        let Some(delimiter) = self.source.get(self.cursor).copied() else {
            return Ok(None);
        };
        self.cursor += 1;
        let start = self.cursor;
        let tail = &self.source[self.cursor..];
        self.cursor += self
            .ctx
            .position_by(
                tail,
                |byte| Ok(*byte == delimiter),
                "creo relation source scan",
            )?
            .unwrap_or(tail.len());
        if self.source.get(self.cursor) != Some(&delimiter) {
            return Ok(None);
        }
        let Ok(source) = self
            .ctx
            .validate_utf8(&self.source[start..self.cursor], "creo UTF-8 validation")?
        else {
            return Ok(None);
        };
        let value = self
            .ctx
            .copy_retained_text(source, "creo relation literal text")?;
        self.cursor += 1;
        Ok(V::string(value))
    }

    fn number(&mut self) -> Parsed<V> {
        let start = self.cursor;
        {
            let tail = &self.source[self.cursor..];
            self.cursor += self
                .ctx
                .position_by(
                    tail,
                    |byte| Ok(!(byte.is_ascii_digit() || *byte == b'.')),
                    "creo relation source scan",
                )?
                .unwrap_or(tail.len());
        }
        if self
            .source
            .get(self.cursor)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            self.cursor += 1;
            if self
                .source
                .get(self.cursor)
                .is_some_and(|byte| matches!(byte, b'+' | b'-'))
            {
                self.cursor += 1;
            }
            {
                let tail = &self.source[self.cursor..];
                self.cursor += self
                    .ctx
                    .position_by(
                        tail,
                        |byte| Ok(!(byte.is_ascii_digit())),
                        "creo relation source scan",
                    )?
                    .unwrap_or(tail.len());
            }
        }
        let Ok(text) = self
            .ctx
            .validate_utf8(&self.source[start..self.cursor], "creo UTF-8 validation")?
        else {
            return Ok(None);
        };
        let Ok(value) = self.ctx.parse_text(text, "creo scalar text parsing")? else {
            return Ok(None);
        };
        Ok(V::number(value))
    }

    fn identifier_or_function(&mut self) -> Parsed<V> {
        let start = self.cursor;
        let Some(end) = expression_identifier_end(self.ctx, self.source, start)? else {
            return Ok(None);
        };
        self.cursor = end;
        let Ok(name) = self
            .ctx
            .validate_utf8(&self.source[start..end], "creo UTF-8 validation")?
        else {
            return Ok(None);
        };
        self.whitespace()?;
        if self.source.get(self.cursor) != Some(&b'(') {
            if let Some(value) = V::reserved(self.ctx, name)? {
                return Ok(Some(value));
            }
            let key_owned_storage = self
                .ctx
                .format_scoped(format_args!("{name}"), "creo relation lookup key")?;
            let _reservation = key_owned_storage.1;
            let mut key = key_owned_storage.0;
            self.ctx
                .make_ascii_lowercase(&mut key, "creo relation identifier case fold")?;
            let Some(value) =
                self.ctx
                    .get_btree_map(self.values, key.as_str(), "creo relation symbol lookup")?
            else {
                return Ok(None);
            };
            return Ok(Some(value.clone_admitted(self.ctx)?));
        }
        self.nesting_ceiling()?;
        let Some((function, scope)) = creo_relation_function(self.ctx, name)? else {
            return Ok(None);
        };
        let _depth = self.ctx.enter_nested("creo relation function depth")?;
        self.cursor += 1;
        self.nesting += 1;
        self.whitespace()?;
        let mut argument_storage = self
            .ctx
            .reserve_scoped(0, "Creo relation argument storage")?;
        let mut arguments = Vec::new();
        if self.source.get(self.cursor) != Some(&b')') {
            loop {
                let Some(argument) = argument_storage.with_storage(|| self.logical_or())? else {
                    return Ok(None);
                };
                argument_storage.with_storage(|| {
                    self.ctx
                        .reserve_vec(&mut arguments, 1, "creo relation function arguments")
                })?;
                arguments.push(argument);
                self.whitespace()?;
                if self.source.get(self.cursor) != Some(&b',') {
                    break;
                }
                self.cursor += 1;
            }
        }
        self.whitespace()?;
        if self.source.get(self.cursor) != Some(&b')') {
            return Ok(None);
        }
        self.cursor += 1;
        self.nesting -= 1;
        Ok(V::function_checked(
            function,
            scope,
            &arguments,
            self.context,
            self.ctx,
        )?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreoMathFunction {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Sign,
    Mod,
    If,
    Bound,
    Dead,
    Near,
    Min,
    Max,
    Log,
    Ln,
    Exp,
    Pow,
    Sqrt,
    Abs,
    Ceil,
    Floor,
    DblInTol,
    Itos,
    Rtos,
    RelModelName,
    RelModelType,
    Exists,
    Search,
    Extract,
    StringLength,
    StringStarts,
    StringEnds,
    StringMatch,
    StringPattern,
    ContextDependent,
}

fn creo_math_function(name: &str) -> Option<CreoMathFunction> {
    const FUNCTIONS: &[(&str, CreoMathFunction)] = &[
        ("sin", CreoMathFunction::Sin),
        ("cos", CreoMathFunction::Cos),
        ("tan", CreoMathFunction::Tan),
        ("asin", CreoMathFunction::Asin),
        ("acos", CreoMathFunction::Acos),
        ("atan", CreoMathFunction::Atan),
        ("atan2", CreoMathFunction::Atan2),
        ("sinh", CreoMathFunction::Sinh),
        ("cosh", CreoMathFunction::Cosh),
        ("tanh", CreoMathFunction::Tanh),
        ("sign", CreoMathFunction::Sign),
        ("mod", CreoMathFunction::Mod),
        ("if", CreoMathFunction::If),
        ("bound", CreoMathFunction::Bound),
        ("dead", CreoMathFunction::Dead),
        ("near", CreoMathFunction::Near),
        ("min", CreoMathFunction::Min),
        ("max", CreoMathFunction::Max),
        ("log", CreoMathFunction::Log),
        ("ln", CreoMathFunction::Ln),
        ("exp", CreoMathFunction::Exp),
        ("pow", CreoMathFunction::Pow),
        ("sqrt", CreoMathFunction::Sqrt),
        ("abs", CreoMathFunction::Abs),
        ("ceil", CreoMathFunction::Ceil),
        ("floor", CreoMathFunction::Floor),
        ("dbl_in_tol", CreoMathFunction::DblInTol),
        ("itos", CreoMathFunction::Itos),
        ("rtos", CreoMathFunction::Rtos),
        ("rel_model_name", CreoMathFunction::RelModelName),
        ("rel_model_type", CreoMathFunction::RelModelType),
        ("exists", CreoMathFunction::Exists),
        ("search", CreoMathFunction::Search),
        ("extract", CreoMathFunction::Extract),
        ("string_length", CreoMathFunction::StringLength),
        ("string_starts", CreoMathFunction::StringStarts),
        ("string_ends", CreoMathFunction::StringEnds),
        ("string_match", CreoMathFunction::StringMatch),
        ("string_pattern", CreoMathFunction::StringPattern),
    ];
    const CONTEXT_DEPENDENT: &[&str] = &[
        "cable_len",
        "cable_thick",
        "cbl_logical_file",
        "eang",
        "elen",
        "edistk",
        "ecoordx",
        "ecoordy",
        "evalgraph",
        "trajpar_of_pnt",
        "massprop_param",
        "material_param",
        "mp_mass",
        "mp_assigned_mass",
        "mp_surf_area",
        "mp_volume",
        "mp_cg_x",
        "mp_cg_y",
        "mp_cg_z",
        "has_value",
        "match_value",
        "average",
        "value_by_argument",
        "weighted_average",
        "value",
        "count_rows",
    ];
    for (spelling, function) in FUNCTIONS {
        if name.eq_ignore_ascii_case(spelling) {
            return Some(*function);
        }
    }
    for spelling in CONTEXT_DEPENDENT {
        if name.eq_ignore_ascii_case(spelling) {
            return Some(CreoMathFunction::ContextDependent);
        }
    }
    None
}

fn creo_relation_function<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    name: &'a str,
) -> Result<Option<(CreoMathFunction, Option<&'a str>)>, cadmpeg_core::CodecError> {
    Ok(
        if let Some((function, scope)) =
            ctx.split_once(name, ":", "creo relation function scope scan")?
        {
            (function.eq_ignore_ascii_case("rel_model_name")
                && !scope.is_empty()
                && ctx.all_by(
                    scope.bytes(),
                    |byte| Ok(byte.is_ascii_digit()),
                    "creo relation function scope validation",
                )?)
            .then_some((CreoMathFunction::RelModelName, Some(scope)))
        } else {
            creo_math_function(name).map(|function| (function, None))
        },
    )
}

fn evaluate_creo_math_function(name: CreoMathFunction, arguments: &[f64]) -> Option<f64> {
    let value = match (name, arguments) {
        (CreoMathFunction::Sin, [x]) => x.to_radians().sin(),
        (CreoMathFunction::Cos, [x]) => x.to_radians().cos(),
        (CreoMathFunction::Tan, [x]) => {
            let principal_degrees = x.rem_euclid(180.0);
            (principal_degrees != 90.0).then_some(())?;
            x.to_radians().tan()
        }
        (CreoMathFunction::Asin, [x]) => x.asin().to_degrees(),
        (CreoMathFunction::Acos, [x]) => x.acos().to_degrees(),
        (CreoMathFunction::Atan, [x]) => x.atan().to_degrees(),
        (CreoMathFunction::Atan2, [y, x]) if *x != 0.0 || *y != 0.0 => y.atan2(*x).to_degrees(),
        (CreoMathFunction::Sinh, [x]) => x.sinh(),
        (CreoMathFunction::Cosh, [x]) => x.cosh(),
        (CreoMathFunction::Tanh, [x]) => x.tanh(),
        (CreoMathFunction::Sign, [x, y]) => {
            if *y < 0.0 {
                -x.abs()
            } else {
                x.abs()
            }
        }
        (CreoMathFunction::Mod, [x, y]) if *y != 0.0 => x % y,
        (CreoMathFunction::If, [condition, when_true, when_false]) => {
            if *condition == 0.0 {
                *when_false
            } else {
                *when_true
            }
        }
        (CreoMathFunction::Bound, [x, lower, upper]) if lower < upper => x.clamp(*lower, *upper),
        (CreoMathFunction::Dead, [x, lower, upper]) if lower <= upper => {
            if x < lower {
                x - lower
            } else if x > upper {
                x - upper
            } else {
                0.0
            }
        }
        (CreoMathFunction::Near, [x, y, delta]) if *delta >= 0.0 => {
            f64::from(u8::from((x - y).abs() <= *delta))
        }
        (name @ (CreoMathFunction::Min | CreoMathFunction::Max), [x, y]) => {
            if extremum_selects_left(name, *x, *y)? {
                *x
            } else {
                *y
            }
        }
        (CreoMathFunction::Log, [x]) => x.log10(),
        (CreoMathFunction::Ln, [x]) => x.ln(),
        (CreoMathFunction::Exp, [x]) => x.exp(),
        (CreoMathFunction::Pow, [base, exponent]) => base.powf(*exponent),
        (CreoMathFunction::Sqrt, [x]) => x.sqrt(),
        (CreoMathFunction::Abs, [x]) => x.abs(),
        (CreoMathFunction::Ceil, [x]) => relation_round(*x, 0.0, true)?,
        (CreoMathFunction::Ceil, [x, decimal_places]) => relation_round(*x, *decimal_places, true)?,
        (CreoMathFunction::Floor, [x]) => relation_round(*x, 0.0, false)?,
        (CreoMathFunction::Floor, [x, decimal_places]) => {
            relation_round(*x, *decimal_places, false)?
        }
        (CreoMathFunction::DblInTol, [first, second, tolerance]) if *tolerance >= 0.0 => {
            f64::from(u8::from((first - second).abs() <= *tolerance))
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn relation_round(value: f64, decimal_places: f64, upward: bool) -> Option<f64> {
    (value.is_finite() && decimal_places.is_finite()).then_some(())?;
    let decimal_places = decimal_places.trunc();
    if decimal_places > 8.0 {
        return Some(value);
    }
    (decimal_places >= f64::from(i32::MIN)).then_some(())?;
    let scale = 10_f64.powi(cadmpeg_core::convert::truncate_f64_to_i32(decimal_places)?);
    (scale.is_finite() && scale > 0.0).then_some(())?;
    let scaled = (value
        + if upward {
            -EPS_RELATION_ROUND
        } else {
            EPS_RELATION_ROUND
        })
        * scale;
    if !scaled.is_finite() {
        return Some(value);
    }
    let rounded = if upward {
        scaled.ceil()
    } else {
        scaled.floor()
    } / scale;
    rounded.is_finite().then_some(rounded)
}

fn extremum_selects_left(name: CreoMathFunction, left: f64, right: f64) -> Option<bool> {
    match name {
        CreoMathFunction::Min => Some(left < right),
        CreoMathFunction::Max => Some(left > right),
        _ => None,
    }
}

fn quantity_power(
    value: &CurveExpressionValue,
    right: &CurveExpressionValue,
) -> Option<CurveExpressionValue> {
    let CurveExpressionValue::Number(exponent) = right else {
        return None;
    };
    let (value, dimension) = quantity_parts_ref(value)?;
    let exponent = exponent.get();
    if dimension == RelationDimension::default() {
        return CurveExpressionValue::number(value.powf(exponent));
    }
    let integer = exponent.trunc();
    (integer == exponent).then_some(())?;
    let exponent =
        i8::try_from(i16::try_from(cadmpeg_core::convert::truncate_f64_to_i32(integer)?).ok()?)
            .ok()?;
    quantity_value(value.powi(i32::from(exponent)), dimension.scale(exponent)?)
}

fn evaluate_creo_numeric_relation_function(
    name: CreoMathFunction,
    arguments: &[CurveExpressionValue],
) -> Option<CurveExpressionValue> {
    use CurveExpressionValue::{Angle, Number, String};
    let value = match (name, arguments) {
        (
            name @ (CreoMathFunction::Sin | CreoMathFunction::Cos | CreoMathFunction::Tan),
            [Angle(value)],
        ) => CurveExpressionValue::number(evaluate_creo_math_function(name, &[value.get()])?)?,
        (
            name @ (CreoMathFunction::Asin | CreoMathFunction::Acos | CreoMathFunction::Atan),
            [Number(value)],
        ) => {
            cadmpeg_ir::scalar::FiniteReal::new(evaluate_creo_math_function(name, &[value.get()])?)
                .map(Angle)?
        }
        (CreoMathFunction::Atan2, [left, right]) => {
            let (left, left_dimension) = quantity_parts_ref(left)?;
            let (right, right_dimension) = quantity_parts_ref(right)?;
            (left_dimension == right_dimension).then_some(())?;
            cadmpeg_ir::scalar::FiniteReal::new(evaluate_creo_math_function(
                CreoMathFunction::Atan2,
                &[left, right],
            )?)
            .map(Angle)?
        }
        (CreoMathFunction::If, [Number(condition), when_true, when_false]) => {
            match (when_true, when_false) {
                (String(_), String(_)) => {}
                (when_true, when_false) => {
                    let (_, true_dimension) = quantity_parts_ref(when_true)?;
                    let (_, false_dimension) = quantity_parts_ref(when_false)?;
                    (true_dimension == false_dimension).then_some(())?;
                }
            }
            let selected = if condition.get() == 0.0 {
                when_false
            } else {
                when_true
            };
            let (value, dimension) = quantity_parts_ref(selected)?;
            quantity_value(value, dimension)?
        }
        (CreoMathFunction::Sign, [value, sign]) => {
            let (value, dimension) = quantity_parts_ref(value)?;
            let (sign, _) = quantity_parts_ref(sign)?;
            quantity_value(
                if sign < 0.0 {
                    -value.abs()
                } else {
                    value.abs()
                },
                dimension,
            )?
        }
        (CreoMathFunction::Mod, [left, right]) => {
            let (left, left_dimension) = quantity_parts_ref(left)?;
            let (right, right_dimension) = quantity_parts_ref(right)?;
            (left_dimension == right_dimension && right != 0.0).then_some(())?;
            quantity_value(left % right, left_dimension)?
        }
        (CreoMathFunction::Bound, [value, lower, upper]) => {
            let (value, value_dimension) = quantity_parts_ref(value)?;
            let (lower, lower_dimension) = quantity_parts_ref(lower)?;
            let (upper, upper_dimension) = quantity_parts_ref(upper)?;
            (value_dimension == lower_dimension
                && value_dimension == upper_dimension
                && lower < upper)
                .then_some(())?;
            quantity_value(
                if value < lower {
                    lower
                } else if value > upper {
                    upper
                } else {
                    value
                },
                value_dimension,
            )?
        }
        (CreoMathFunction::Dead, [value, lower, upper]) => {
            let (value, value_dimension) = quantity_parts_ref(value)?;
            let (lower, lower_dimension) = quantity_parts_ref(lower)?;
            let (upper, upper_dimension) = quantity_parts_ref(upper)?;
            (value_dimension == lower_dimension
                && value_dimension == upper_dimension
                && lower <= upper)
                .then_some(())?;
            let value = if value < lower {
                value - lower
            } else if value > upper {
                value - upper
            } else {
                0.0
            };
            quantity_value(value, value_dimension)?
        }
        (CreoMathFunction::Pow, [base, Number(exponent)]) => {
            quantity_power(base, &Number(*exponent))?
        }
        (CreoMathFunction::Sqrt, [argument]) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            (value >= 0.0).then_some(())?;
            quantity_value(value.sqrt(), dimension.root(2)?)?
        }
        (CreoMathFunction::Abs, [argument]) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            quantity_value(value.abs(), dimension)?
        }
        (name @ (CreoMathFunction::Ceil | CreoMathFunction::Floor), [argument]) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            quantity_value(
                relation_round(value, 0.0, matches!(name, CreoMathFunction::Ceil))?,
                dimension,
            )?
        }
        (
            name @ (CreoMathFunction::Ceil | CreoMathFunction::Floor),
            [argument, Number(decimal_places)],
        ) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            quantity_value(
                relation_round(
                    value,
                    decimal_places.get(),
                    matches!(name, CreoMathFunction::Ceil),
                )?,
                dimension,
            )?
        }
        (name @ (CreoMathFunction::Min | CreoMathFunction::Max), [left, right]) => {
            let (left_value, left_dimension) = quantity_parts_ref(left)?;
            let (right_value, right_dimension) = quantity_parts_ref(right)?;
            (left_dimension == right_dimension).then_some(())?;
            if extremum_selects_left(name, left_value, right_value)? {
                left.clone()
            } else {
                right.clone()
            }
        }
        (CreoMathFunction::Near | CreoMathFunction::DblInTol, [left, right, tolerance]) => {
            let (left, left_dimension) = quantity_parts_ref(left)?;
            let (right, right_dimension) = quantity_parts_ref(right)?;
            let (tolerance, tolerance_dimension) = quantity_parts_ref(tolerance)?;
            (left_dimension == right_dimension
                && left_dimension == tolerance_dimension
                && tolerance >= 0.0)
                .then_some(())?;
            CurveExpressionValue::number(f64::from((left - right).abs() <= tolerance))?
        }
        _ => {
            let mut numbers = [0.0; 3];
            for (index, argument) in arguments.iter().enumerate() {
                let slot = numbers.get_mut(index)?;
                let Number(value) = argument else {
                    return None;
                };
                *slot = value.get();
            }
            CurveExpressionValue::number(evaluate_creo_math_function(
                name,
                &numbers[..arguments.len()],
            )?)?
        }
    };
    Some(value)
}

fn extract_relation_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
    position: f64,
    length: f64,
    scan_operation: &'static str,
    copy_operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    let mut characters = value.char_indices().enumerate();
    let Some((_, (start, _))) = ctx.find_by(
        characters.by_ref(),
        |(ordinal, _)| {
            Ok(
                cadmpeg_core::convert::f64_from_index(*ordinal + 1).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "Creo numeric value cannot be represented exactly",
                    )
                })? == position,
            )
        },
        scan_operation,
    )?
    else {
        return Ok(String::new());
    };
    let end = if length == 0.0 {
        start
    } else {
        ctx.find_by(
            characters,
            |(ordinal, _)| {
                Ok(
                    cadmpeg_core::convert::f64_from_index(*ordinal + 1).ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "Creo numeric value cannot be represented exactly",
                        )
                    })? - position
                        >= length,
                )
            },
            scan_operation,
        )?
        .map_or(value.len(), |(_, (end, _))| end)
    };
    ctx.copy_retained_text(&value[start..end], copy_operation)
}

const RELATION_REGEX_SIZE_LIMIT: usize = 1 << 20;

fn relation_string_pattern_admitted(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
    pattern: &str,
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(pattern.len()),
        "creo relation regex compile work",
    )?;
    let expression_owned_storage = ctx.format_scoped(
        format_args!(r"\A(?:{pattern})\z"),
        "creo relation regex source text",
    )?;
    let _source_reservation = expression_owned_storage.1;
    let expression = expression_owned_storage.0;
    let _compiler_reservation = ctx.reserve_scoped(
        cadmpeg_core::decode::u64_from_index(RELATION_REGEX_SIZE_LIMIT),
        "creo relation regex compiler scratch",
    )?;
    let compiled = regex::RegexBuilder::new(&expression)
        .size_limit(RELATION_REGEX_SIZE_LIMIT)
        .dfa_size_limit(RELATION_REGEX_SIZE_LIMIT)
        .build();
    let Ok(compiled) = compiled else {
        return Ok(None);
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(value.len()),
        "creo relation regex match work",
    )?;
    Ok(Some(compiled.is_match(value)))
}

const MAX_RELATION_STRING_PRECISION: f64 = 128.0;

fn relation_precision(value: f64) -> Option<usize> {
    (value.is_finite()
        && value.fract() == 0.0
        && (0.0..=MAX_RELATION_STRING_PRECISION).contains(&value))
    .then(|| cadmpeg_core::convert::truncate_f64_to_usize(value))
    .flatten()
}

fn format_relation_real_admitted(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: f64,
    decimals: Option<usize>,
    scientific: bool,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if !value.is_finite() {
        return Ok(None);
    }
    if value == 0.0 {
        return Ok(Some(String::new()));
    }
    let Some(decimals) = decimals else {
        return Ok(Some(ctx.format_retained(
            format_args!("{value}"),
            "creo relation real text",
        )?));
    };
    if !scientific {
        return Ok(Some(ctx.format_retained(
            format_args!("{value:.decimals$}"),
            "creo relation real text",
        )?));
    }
    let formatted_owned_storage = ctx.format_scoped(
        format_args!("{value:.decimals$e}"),
        "creo relation scientific scratch",
    )?;
    let _reservation = formatted_owned_storage.1;
    let formatted = formatted_owned_storage.0;
    let Some((mantissa, exponent)) =
        ctx.split_once(&formatted, "e", "creo relation scientific exponent scan")?
    else {
        return Ok(None);
    };
    let Ok(exponent) = ctx.parse_text::<i32>(exponent, "creo scalar text parsing")? else {
        return Ok(None);
    };
    Ok(Some(ctx.format_retained(
        format_args!(
            "{mantissa}e{}{magnitude:02}",
            if exponent < 0 { "-" } else { "" },
            magnitude = exponent.unsigned_abs()
        ),
        "creo relation real text",
    )?))
}

fn parse_relation_expression<V: ExpressionValue>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    expression: &str,
    values: &BTreeMap<String, V>,
    context: RelationEvaluationContext<'_>,
) -> Result<Option<V>, cadmpeg_core::CodecError> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context,
        ctx,
        nesting: 0,
    };
    let value = parser.logical_or().map_err(|error| *error)?;
    parser.whitespace().map_err(|error| *error)?;
    match value {
        Some(value) if parser.cursor == parser.source.len() => {
            Ok(value.finite(ctx)?.then_some(value))
        }
        _ => Ok(None),
    }
}

fn apply_declared_relation_unit(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: CurveExpressionValue,
    declared_unit: Option<&str>,
) -> Result<Option<CurveExpressionValue>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(declared_unit) = declared_unit else {
        return Ok(Some(value));
    };
    let Some(unit) = relation_unit(ctx, declared_unit)? else {
        return Ok(None);
    };
    Ok(match (value, unit.dimension) {
        (CurveExpressionValue::Number(value), _) => {
            quantity_value(value.get() * unit.scale + unit.offset, unit.dimension)
        }
        (value @ CurveExpressionValue::Length(_), RelationDimension::LENGTH)
        | (value @ CurveExpressionValue::Angle(_), RelationDimension::ANGLE) => Some(value),
        (CurveExpressionValue::Quantity(value), dimension) if value.dimension() == dimension => {
            Some(CurveExpressionValue::Quantity(value))
        }
        _ => None,
    })
}

/// Recognize an exact cylindrical helix program expressed by the conventional
/// Creo outputs `r`, `theta` (degrees), and `z` over `t` in `[0, 1]`.
pub(crate) fn expression_helix(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &CurveExpressionRecord,
) -> Result<Option<CurveExpressionHelix>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if !record.prohibited_constructs.is_empty()
        || !record.solve_blocks.is_empty()
        || record.unresolved_solve_control
    {
        return Ok(None);
    }
    let mut present = [false; 3];
    let mut assignments = record.assignments.iter();
    while assignments.len() != 0 {
        let Some(assignment) =
            ctx.next_charged(&mut assignments, "creo helix output scan work")?
        else {
            break;
        };
        if let Some((name, _)) = assignment.parameter_target() {
            for (slot, output) in present.iter_mut().zip(["r", "theta", "z"]) {
                if !*slot {
                    *slot = name.eq_ignore_ascii_case(output);
                }
            }
        }
        if present == [true; 3] {
            break;
        }
    }
    if present != [true; 3] {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo helix affine scratch")?;
    let values = scratch.with_storage(|| evaluate_affine_program(ctx, record))?;
    let Some(radius) = ctx.get_btree_map(&values, "r", "creo helix radius lookup")? else {
        return Ok(None);
    };
    let Some(theta) = ctx.get_btree_map(&values, "theta", "creo helix angle lookup")? else {
        return Ok(None);
    };
    let Some(z) = ctx.get_btree_map(&values, "z", "creo helix height lookup")? else {
        return Ok(None);
    };
    if radius.linear != 0.0 {
        return Ok(None);
    }
    let angular_travel = theta.linear;
    Ok(CurveExpressionHelix::new(
        radius.constant,
        z.linear,
        z.constant,
        angular_travel.abs() / 360.0,
        theta.constant.to_radians(),
        angular_travel < 0.0,
    ))
}

/// Decode positional `crv_array` rows whose terminal suffix has one
/// syntactically valid boundary. Callers that have decoded the enclosing
/// `srf_array` should use [`topology_rows_with_face_ids`] so an ambiguous
/// reference boundary can be resolved by its face roles.
#[cfg(test)]
fn topology_rows(payload: &[u8]) -> Vec<CurveTopologyRow> {
    crate::decode::with_test_decode_ctx(|ctx| topology_rows_with_face_ids(ctx, payload, None))
        .expect("test curve topology rows admitted")
}

/// Decode standard topology rows using the enclosing `srf_array` identifier
/// set to resolve variable-width reference boundaries.
pub(crate) fn topology_rows_with_face_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Result<Vec<CurveTopologyRow>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo topology frame scratch")?;
    let mut rows = Vec::new();
    let framed_rows = scratch.with_storage(|| framed_rows_with_face_ids(ctx, payload, face_ids))?;
    for row in ctx.admit_iter(framed_rows, "creo topology frame traversal")? {
        if let Some(parsed) = parse_topology_row(
            &payload[row.start..row.end],
            row.start,
            row.suffix_start,
            row.suffix,
        ) {
            ctx.reserve_vec(&mut rows, 1, "creo topology curve rows")?;
            rows.push(parsed);
        }
    }
    ctx.stable_sort_by(
        rows.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo topology rows with face ids rows ordering",
    )?;
    ctx.dedup_by_key(
        &mut rows,
        |row| Ok(row.offset),
        "creo topology curve row deduplication",
    )?;
    Ok(rows)
}

/// Decode a complete DEPDB `crv_array\0 f2 f8 <count>` cross-section array.
/// Any malformed row or count mismatch withholds the entire array.
pub(crate) fn depdb_cross_section_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<DepdbCurveRow>, cadmpeg_core::CodecError> {
    let Some(array) = ctx.find_bytes_from(payload, b"crv_array\0", 0, "find Creo curve marker")?
    else {
        return Ok(Vec::new());
    };
    let header = array + b"crv_array\0".len();
    if payload.get(header..header + 2) != Some(&[0xf2, psb::token::ARRAY_OPEN]) {
        return Ok(Vec::new());
    }
    let (count, after_count) = compact_int(payload, header + 2);
    if after_count == header + 2 {
        return Ok(Vec::new());
    }
    let Ok(count) = usize::try_from(count) else {
        return Ok(Vec::new());
    };
    let mut scratch = ctx.reserve_scoped(0, "creo curve parser scratch")?;
    if count == 0 || scratch.with_storage(|| prototypes(ctx, payload))?.len() != 1 {
        return Ok(Vec::new());
    }
    let Some(topology) = ctx.find_bytes_from(
        payload,
        b"topol_ref_data\0",
        after_count,
        "find Creo curve marker",
    )?
    else {
        return Ok(Vec::new());
    };
    let mut cursor = topology + b"topol_ref_data\0".len();
    let cache = scratch.with_storage(|| scalar::ScalarCache::from_section_checked(ctx, payload))?;
    let positional_count = count - 1;
    // Each row consumes at least one payload byte past the topology cursor
    // before its terminator, so the row count cannot exceed the unread bytes.
    let Some(capacity) = bounded_len(
        cadmpeg_core::decode::u64_from_index(positional_count),
        1,
        payload.get(cursor..).map_or(0, <[u8]>::len),
    ) else {
        return Ok(Vec::new());
    };
    let mut row_storage = ctx.reserve_scoped(0, "creo cross-section curve rows")?;
    let mut rows = Vec::new();
    row_storage
        .with_storage(|| ctx.reserve_vec(&mut rows, capacity, "creo cross-section curve rows"))?;
    let mut boundaries = Vec::new();
    for (marker, length) in [
        (b"\xe1\xe3".as_slice(), 2),
        (b"\xe1\xf5\x05\xf6\xe3", 5),
        (b"\xe1\xe0", 1),
    ] {
        let mut search = cursor;
        while let Some(offset) =
            ctx.find_bytes_from(payload, marker, search, "find Creo curve marker")?
        {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut boundaries, 1, "creo cross-section row boundaries")
            })?;
            boundaries.push((offset, length));
            search = offset + marker.len();
        }
    }
    ctx.sort_unstable_by(
        &mut boundaries,
        |value| value,
        Ord::cmp,
        "creo cross-section row boundaries sort",
    )?;
    ctx.dedup_vec(&mut boundaries, "creo curve boundary deduplication")?;
    while rows.len() < positional_count {
        let first_candidate = ctx.partition_point(
            &boundaries,
            |(end, _)| Ok(*end < cursor),
            "creo curve boundary search",
        )?;
        let mut selected = None;
        let mut candidates = boundaries[first_candidate..].iter().copied();
        while candidates.len() != 0 {
            let Some((end, length)) = ctx.next_charged(
                &mut candidates,
                "creo cross-section boundary candidate traversal",
            )? else {
                break;
            };
            if let Some(row) = row_storage.with_storage(|| {
                parse_depdb_curve_segment(ctx, &payload[cursor..end], cursor, &cache)
            })? {
                selected = Some((row, end, length));
                break;
            }
        }
        let Some((row, terminator, length)) = selected else {
            return Ok(Vec::new());
        };
        rows.push(row);
        cursor = terminator + length;
    }
    if rows.len() == positional_count {
        row_storage.commit_value(rows)
    } else {
        Ok(Vec::new())
    }
}

fn parse_depdb_curve_segment(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segment: &[u8],
    absolute_offset: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<DepdbCurveRow>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut suffix_candidate = None;
    for suffix_length in 4..=11 {
        let Some(start) = segment.len().checked_sub(suffix_length) else {
            continue;
        };
        let (zero0, p1) = compact_int(segment, start);
        let (x1, p2) = compact_int(segment, p1);
        let (f1, p3) = compact_int(segment, p2);
        let (zero1, end) = compact_int(segment, p3);
        if p1 > start && p2 > p1 && p3 > p2 && end == segment.len() && zero0 == 0 && zero1 == 0 {
            if suffix_candidate.is_some() {
                return Ok(None);
            }
            suffix_candidate = Some((start, [zero0, x1, f1, zero1]));
        }
    }
    let Some((suffix_start, suffix)) = suffix_candidate else {
        return Ok(None);
    };
    let mut prefix_candidate: Option<(usize, TopologyPrefix)> = None;
    let mut starts = 0..suffix_start;
    while starts.len() != 0 {
        let Some(start) = ctx.next_charged(&mut starts, "creo curve prefix scan")? else {
            break;
        };
        let Some(prefix) = topology_prefix_fields(segment, start) else {
            continue;
        };
        if prefix.end > suffix_start {
            continue;
        }
        match prefix_candidate {
            None => prefix_candidate = Some((start, prefix)),
            Some((_, known)) if known.end == prefix.end => {}
            Some(_) => return Ok(None),
        }
    }
    let Some((row_start, prefix)) = prefix_candidate else {
        return Ok(None);
    };
    let body = ctx.copy_retained(&segment[prefix.end..suffix_start], "creo curve row body")?;
    let CurveScalarLane {
        scalar_tokens,
        references,
        opaque_spans,
    } = curve_scalar_lane(ctx, &body, prefix.type_byte, cache)?;
    Ok(Some(DepdbCurveRow {
        id: prefix.id,
        type_byte: prefix.type_byte,
        feature_id: prefix.feature_id,
        directions: prefix.directions,
        suffix: DepdbCurveSuffix {
            x1: suffix[1],
            face_id: suffix[2],
        },
        body,
        scalar_tokens,
        references,
        opaque_spans,
        offset: absolute_offset + row_start,
    }))
}

#[derive(Debug, Clone, Copy)]
struct FramedRow {
    namespace_start: usize,
    start: usize,
    end: usize,
    suffix_start: usize,
    suffix: [u32; 4],
    reference_geometry: [u32; 2],
}

impl cadmpeg_core::decode::cost::DecodeCost for FramedRow {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TopologySuffixCandidate {
    start: usize,
    faces: [Option<NonZeroU32>; 2],
    next_edges: [u32; 2],
    reference_geometry: [u32; 2],
}

impl TopologySuffixCandidate {
    fn stored_references(self) -> [u32; 4] {
        let [f0, f1] = self.faces.map(stored_face_reference);
        let [e0, e1] = self.next_edges;
        [f0, f1, e0, e1]
    }
}

#[derive(Debug, Clone, Copy)]
struct TopologyPrefix {
    id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    end: usize,
}

fn row_terminator(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<(usize, usize)>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(bytes) = payload.get(start..end) else {
        return Ok(None);
    };
    ctx.find_map(
        bytes.windows(2).enumerate(),
        |(offset, prefix)| {
            let length = if prefix == b"\xe1\xe3" {
                2
            } else if bytes.get(offset..offset + 5) == Some(b"\xe1\xf5\x05\xf6\xe3") {
                5
            } else {
                return Ok(None);
            };
            Ok(Some((start + offset, length)))
        },
        "find Creo curve row terminator",
    )
}

const CURVE_NAMESPACE_BOUNDARIES: [&[u8]; 4] = [
    b"crv_array\0",
    b"lo_array\0",
    b"qlt_array\0",
    b"srf_array\0",
];

fn framed_rows_with_face_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Result<Vec<FramedRow>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo curve namespace scratch")?;
    let mut result = Vec::new();
    let mut arrays = Vec::new();
    for array in ctx.find_bytes_iter(payload, b"crv_array\0", "find Creo curve marker")? {
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut arrays,
                array + b"crv_array\0".len(),
                "creo curve namespace starts",
            )
        })?;
    }
    if arrays.is_empty() {
        scratch.with_storage(|| ctx.reserve_vec(&mut arrays, 1, "creo curve namespace starts"))?;
        arrays.push(0);
    }
    for (index, &namespace_start) in ctx
        .admit_iter(&arrays, "creo curve namespace traversal")?
        .enumerate()
    {
        let namespace_end = match arrays.get(index + 1) {
            Some(next) => next - b"crv_array\0".len(),
            None => {
                let mut end = payload.len();
                for label in CURVE_NAMESPACE_BOUNDARIES {
                    if let Some(offset) = ctx.find_bytes_from(
                        payload,
                        label,
                        namespace_start,
                        "find Creo curve namespace boundary",
                    )? {
                        end = end.min(offset);
                    }
                }
                end
            }
        };
        let Some(label) = ctx.find_bytes_in(
            payload,
            b"topol_ref_data\0",
            namespace_start,
            namespace_end,
            "find Creo curve marker",
        )?
        else {
            continue;
        };
        let mut cursor = label + b"topol_ref_data\0".len();
        let mut segment_storage = ctx.reserve_scoped(0, "creo curve segment scratch")?;
        let mut boundary_anchored = false;
        let mut segments = Vec::new();
        while let Some((terminator, length)) = row_terminator(ctx, payload, cursor, namespace_end)?
        {
            segment_storage
                .with_storage(|| ctx.reserve_vec(&mut segments, 1, "creo framed curve segments"))?;
            segments.push((cursor, terminator, boundary_anchored));
            cursor = terminator + length;
            boundary_anchored = true;
        }
        if cursor < namespace_end {
            segment_storage
                .with_storage(|| ctx.reserve_vec(&mut segments, 1, "creo framed curve segments"))?;
            segments.push((cursor, namespace_end, boundary_anchored));
        }
        let known_face_ids = if let Some(face_ids) = face_ids {
            let mut known = BTreeSet::new();
            for id in ctx.admit_iter(face_ids, "creo known curve face traversal")? {
                segment_storage.with_storage(|| {
                    ctx.insert_btree_set(&mut known, *id, "creo known curve face ID nodes")
                })?;
            }
            for &(start, end, _) in
                ctx.admit_iter(&segments, "creo curve segment evidence traversal")?
            {
                let Some(suffix) = unique_topology_suffix_in_segment(ctx, &payload[start..end])?
                else {
                    continue;
                };
                for id in suffix.faces.into_iter().flatten().map(NonZeroU32::get) {
                    segment_storage.with_storage(|| {
                        ctx.insert_btree_set(&mut known, id, "creo known curve face ID nodes")
                    })?;
                }
            }
            Some(known)
        } else {
            None
        };
        for &(start, end, boundary_anchored) in
            ctx.admit_iter(&segments, "creo curve segment traversal")?
        {
            if let Some(row) = framed_segment_with_face_ids(
                ctx,
                payload,
                namespace_start,
                (start, end),
                boundary_anchored,
                face_ids,
                known_face_ids.as_ref(),
            )? {
                ctx.reserve_vec(&mut result, 1, "creo framed curve rows")?;
                result.push(row);
            }
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.start,
        Ord::cmp,
        "creo framed rows with face ids result ordering",
    )?;
    ctx.dedup_by_key(
        &mut result,
        |row| Ok(row.start),
        "creo framed rows with face ids result deduplication",
    )?;
    Ok(result)
}

fn framed_segment_with_face_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    namespace_start: usize,
    bounds: (usize, usize),
    boundary_anchored: bool,
    materialized_face_ids: Option<&BTreeSet<u32>>,
    known_face_ids: Option<&BTreeSet<u32>>,
) -> Result<Option<FramedRow>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let (start, end) = bounds;

    let Some(segment) = payload.get(start..end) else {
        return Ok(None);
    };
    let mut scratch = ctx.reserve_scoped(0, "creo framed prefix scratch")?;
    let mut prefixes = Vec::new();
    for row_start in ctx.admit_iter(0..segment.len(), "creo framed curve prefix scan")? {
        if let Some(prefix) = topology_prefix_fields(segment, row_start) {
            scratch
                .with_storage(|| ctx.reserve_vec(&mut prefixes, 1, "creo framed curve prefixes"))?;
            prefixes.push((row_start, prefix.end));
        }
    }
    ctx.sort_unstable_by(
        &mut prefixes,
        |value| &value.1,
        Ord::cmp,
        "creo framed curve prefixes sort",
    )?;
    let mut closes = segment.iter().enumerate().rev();
    while closes.len() != 0 {
        let Some((close, &byte)) =
            ctx.next_charged(&mut closes, "creo framed curve close scan")?
        else {
            break;
        };
        if byte != psb::token::COMPOUND_CLOSE {
            continue;
        }
        let row_end = close + 1;
        if !complete_curve_row_linkage(ctx, &segment[row_end..])? {
            continue;
        }
        let Some(candidate) = topology_suffix_with_face_ids(
            ctx,
            &segment[..row_end],
            materialized_face_ids,
            known_face_ids,
        )?
        else {
            continue;
        };
        let suffix_start = candidate.start;
        let suffix = candidate.stored_references();
        let reference_geometry = candidate.reference_geometry;
        if boundary_anchored
            && topology_prefix_fields(segment, 0).is_some_and(|prefix| prefix.end <= suffix_start)
        {
            return Ok(Some(FramedRow {
                namespace_start,
                start,
                end: start + row_end,
                suffix_start,
                suffix,
                reference_geometry,
            }));
        }
        let eligible = ctx.partition_point(
            &prefixes,
            |(_, prefix_end)| Ok(*prefix_end <= suffix_start),
            "creo eligible curve prefix search",
        )?;
        if eligible == 1 {
            return Ok(Some(FramedRow {
                namespace_start,
                start: start + prefixes[0].0,
                end: start + row_end,
                suffix_start: suffix_start - prefixes[0].0,
                suffix,
                reference_geometry,
            }));
        }
    }
    Ok(None)
}

fn generic_compact_at(bytes: &[u8], offset: usize) -> Option<(u32, usize)> {
    (*bytes.get(offset)? <= 0xbf).then_some(())?;
    let (value, next) = compact_int(bytes, offset);
    (next > offset).then_some((value, next))
}

/// Validate the array-item linkage between a curve row's compound close and
/// its row terminator. The linkage has an optional entity link, an optional
/// counted link list, and up to four terminal compact links. The final row may
/// append the enclosing array close before the next namespace boundary.
fn complete_curve_row_linkage(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
) -> Result<bool, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let bytes = bytes
        .strip_suffix(&[0xe1, 0xf5, 0x05, 0xf6, 0xe0, 0x00])
        .or_else(|| bytes.strip_suffix(&[0xe1, 0xe0, 0x00]))
        .unwrap_or(bytes);
    let mut cursor = 0;
    if bytes.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let Some((_, next)) = generic_compact_at(bytes, cursor + 1) else {
            return Ok(false);
        };
        cursor = next;
    }
    if bytes.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let Some((count, next)) = generic_compact_at(bytes, cursor + 1) else {
            return Ok(false);
        };
        let Some(remaining) = bytes.len().checked_sub(next) else {
            return Ok(false);
        };
        let Some(count) = bounded_len(count.into(), 1, remaining) else {
            return Ok(false);
        };
        cursor = next;
        let mut links = 0..count;
        while links.start < links.end
            && ctx.next_charged(&mut links, "creo counted curve row linkage")?
            .is_some()
        {
            let Some((_, next)) = generic_compact_at(bytes, cursor) else {
                return Ok(false);
            };
            cursor = next;
        }
    }
    let mut terminal_count = 0;
    while cursor < bytes.len() {
        if terminal_count == 4 {
            return Ok(false);
        }
        let Some((_, next)) = generic_compact_at(bytes, cursor) else {
            return Ok(false);
        };
        cursor = next;
        terminal_count += 1;
    }
    Ok(true)
}

#[derive(Debug)]
struct CurveScalarLane {
    scalar_tokens: Vec<CurveParameterScalar>,
    references: Vec<CurveParameterReference>,
    opaque_spans: Vec<CurveParameterOpaqueSpan>,
}

fn curve_scalar_lane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: &[u8],
    type_byte: u8,
    cache: &scalar::ScalarCache,
) -> Result<CurveScalarLane, cadmpeg_core::CodecError> {
    enum SelectedToken {
        Reference(u32, usize),
        Scalar(f64, usize),
    }
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut scalars = Vec::new();
    let mut references = Vec::new();
    let mut opaque_spans = Vec::new();
    let mut opaque_start = 0;
    let mut cursor = 0;
    while cursor < body.len() {
        ctx.next_charged(&mut body[cursor..].iter(), "creo curve scalar dispatch")?;
        let reference = if body[cursor] == psb::token::ENTITY_REF {
            reference_id(body, cursor + 1).ok()
        } else {
            None
        };
        let zero = body[cursor] == 0x18
            && cursor + 1 == body.len()
            && matches!(type_byte, 0x00 | 0x01 | 0x06 | 0x08)
            && scalars.len() < 8;
        let decoded = if reference.is_some() {
            None
        } else if zero {
            Some((0.0, cursor + 1))
        } else if matches!(type_byte, 0x00 | 0x01 | 0x06 | 0x08) {
            scalar::decode_in_pcurve_lane(body, cursor, cache)
        } else {
            scalar::decode_in_row_lane(body, cursor, cache)
        };
        let selected = match (reference, decoded) {
            (Some((id, next)), _) => SelectedToken::Reference(id, next),
            (None, Some((value, next))) => SelectedToken::Scalar(value, next),
            (None, None) => {
                cursor += 1;
                continue;
            }
        };
        if opaque_start < cursor {
            let raw =
                ctx.copy_retained(&body[opaque_start..cursor], "creo curve opaque raw span")?;
            ctx.push_vec(
                &mut opaque_spans,
                CurveParameterOpaqueSpan {
                    raw,
                    offset: opaque_start,
                },
                "creo curve opaque spans",
            )?;
        }
        let next = match selected {
            SelectedToken::Reference(entity_id, next) => {
            ctx.push_vec(
                &mut references,
                CurveParameterReference {
                    entity_id,
                    offset: cursor,
                    length: next - cursor,
                },
                "creo curve parameter references",
            )?;
            next
            }
            SelectedToken::Scalar(value, next) => {
            let raw = ctx.copy_retained(
                &body[cursor..next],
                if zero {
                    "creo curve zero raw token"
                } else {
                    "creo curve scalar raw token"
                },
            )?;
            ctx.push_vec(
                &mut scalars,
                CurveParameterScalar {
                    value,
                    raw,
                    offset: cursor,
                },
                "creo curve parameter scalars",
            )?;
            next
            }
        };
        cursor = next;
        opaque_start = next;
    }
    if opaque_start < body.len() {
        let raw = ctx.copy_retained(&body[opaque_start..], "creo curve opaque raw span")?;
        ctx.push_vec(
            &mut opaque_spans,
            CurveParameterOpaqueSpan {
                raw,
                offset: opaque_start,
            },
            "creo curve opaque spans",
        )?;
    }
    Ok(CurveScalarLane {
        scalar_tokens: scalars,
        references,
        opaque_spans,
    })
}

/// Decode analytic bodies from positional curve rows with one valid terminal
/// topology suffix. Use [`parameter_records_with_face_ids`] when the enclosing
/// `srf_array` identifiers are available.
#[cfg(test)]
fn parameter_records(payload: &[u8]) -> Vec<CurveParameterRecord> {
    crate::decode::with_test_decode_ctx(|ctx| parameter_records_with_face_ids(ctx, payload, None))
        .expect("test curve parameter records")
}

/// Decode analytic bodies using the enclosing `srf_array` identifier set to
/// resolve variable-width reference boundaries.
pub(crate) fn parameter_records_with_face_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Result<Vec<CurveParameterRecord>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo curve parser scratch")?;
    let cache = scratch.with_storage(|| scalar::ScalarCache::from_section_checked(ctx, payload))?;
    let mut records = Vec::new();
    let framed_rows = scratch.with_storage(|| framed_rows_with_face_ids(ctx, payload, face_ids))?;
    for framed in ctx.admit_iter(framed_rows, "creo curve parameter frame traversal")? {
        let row = &payload[framed.start..framed.end];
        let (curve_id, after_id) = compact_int(row, 0);
        let Some(&type_byte) = row.get(after_id) else {
            continue;
        };
        let (_, after_feature) = compact_int(row, after_id + 1);
        let body_start = after_feature + 2;
        let Some(close) = row.len().checked_sub(1) else {
            continue;
        };
        if row.get(close) != Some(&psb::token::COMPOUND_CLOSE) || body_start > close {
            continue;
        }
        let suffix_start = framed.suffix_start;
        if suffix_start < body_start {
            continue;
        }
        let body =
            ctx.copy_retained(&row[body_start..suffix_start], "creo curve parameter body")?;
        let CurveScalarLane {
            scalar_tokens,
            references,
            opaque_spans,
        } = curve_scalar_lane(ctx, &body, type_byte, &cache)?;
        ctx.reserve_vec(&mut records, 1, "creo curve parameter records")?;
        records.push(CurveParameterRecord {
            curve_id,
            type_byte,
            body,
            scalar_tokens,
            references,
            opaque_spans,
            reference_geometry: framed.reference_geometry,
            offset: framed.start,
            body_offset: framed.start + body_start,
            suffix_offset: framed.start + suffix_start,
        });
    }
    Ok(records)
}

fn complete_pcurve_values(record: &CurveParameterRecord) -> Option<[f64; 8]> {
    const HELD_SCALAR_OPEN: &[u8] = &[0xd7, 0xe8, 0x03];
    const HELD_SCALAR_CLOSE: u8 = 0x1e;

    record.references.is_empty().then_some(())?;
    let mut tokens = record.scalar_tokens.iter().peekable();
    let mut values = [0.0; 8];
    let mut value_count = 0;
    let mut cursor = 0;
    while cursor < record.body.len() {
        if record.body.get(cursor..cursor + HELD_SCALAR_OPEN.len()) == Some(HELD_SCALAR_OPEN) {
            cursor += HELD_SCALAR_OPEN.len();
            let token = tokens.next().filter(|token| token.offset == cursor)?;
            (!token.raw.is_empty()
                && record.body.get(cursor..cursor + token.raw.len()) == Some(token.raw.as_slice()))
            .then_some(())?;
            *values.get_mut(value_count)? = token.value;
            value_count += 1;
            cursor += token.raw.len();
            (record.body.get(cursor) == Some(&HELD_SCALAR_CLOSE)).then_some(())?;
            cursor += 1;
            continue;
        }
        if let Some(token) = tokens.peek().filter(|token| token.offset == cursor) {
            (!token.raw.is_empty()
                && record.body.get(cursor..cursor + token.raw.len()) == Some(token.raw.as_slice()))
            .then_some(())?;
            *values.get_mut(value_count)? = token.value;
            value_count += 1;
            cursor += token.raw.len();
            tokens.next();
        } else if record.body[cursor] == 0x12 {
            *values.get_mut(value_count)? = 0.0;
            value_count += 1;
            cursor += 1;
        } else {
            return None;
        }
    }
    tokens.next().is_none().then_some(())?;
    (value_count == values.len() && values.iter().all(|value| value.is_finite())).then_some(values)
}

/// Interpret complete eight-slot parameter lanes for pcurve-family rows.
pub(crate) fn pcurve_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Result<Vec<PcurveEndpoints>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo pcurve topology scratch")?;
    let topology_index = scratch.with_storage(|| {
        unique_curve_index(ctx, topology, |row| row.id, "creo pcurve topology index")
    })?;
    let mut result = Vec::new();
    let parameter_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            parameters,
            |record| record.curve_id,
            "creo unique-row count nodes",
        )
    })?;
    for record in ctx.admit_iter(parameters, "creo unique parameter traversal")? {
        if !parameter_index
            .get(&record.curve_id)
            .is_some_and(Option::is_some)
        {
            continue;
        }
        if !matches!(record.type_byte, 0x00 | 0x01 | 0x06 | 0x08) {
            continue;
        }
        let Some(values) = complete_pcurve_values(record) else {
            continue;
        };
        let Some(Some(topology)) = topology_index.get(&record.curve_id) else {
            continue;
        };
        if topology.type_byte != record.type_byte {
            continue;
        }
        ctx.reserve_vec(&mut result, 1, "creo pcurve endpoint rows")?;
        result.push(PcurveEndpoints {
            curve_id: record.curve_id,
            faces: topology.faces,
            face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
            face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
            offset: record.offset,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo pcurve endpoints result ordering",
    )?;
    Ok(result)
}

fn decode_two_chart_scalar(
    body: &[u8],
    cursor: usize,
    first_coordinate: bool,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    if body.get(cursor) == Some(&0x18) {
        return Some((0.0, cursor + 1));
    }
    if first_coordinate {
        scalar::decode_two_chart_first_coordinate(body, cursor, cache)
    } else {
        scalar::decode_two_chart_second_coordinate(body, cursor, cache)
    }
}

fn complete_two_chart_samples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    body: &[u8],
    start: usize,
    count: Option<u32>,
    sample_operation: &'static str,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<[[f64; 2]; 2]>>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(remaining) = body.len().checked_sub(start) else {
        return Ok(None);
    };
    let limit = match count {
        Some(count) => match bounded_len(u64::from(count), 4, remaining) {
            Some(count) => count,
            None => return Ok(None),
        },
        None => remaining / 4,
    };
    if limit < 2 {
        return Ok(None);
    }
    let mut cursor = start;
    let mut samples = Vec::new();
    let mut steps = 0..limit;
    while cursor < body.len() && samples.len() < limit {
        ctx.next_charged(&mut steps, "creo two-chart sample traversal")?;
        let mut sample = [[0.0; 2]; 2];
        for (slot, value) in sample.iter_mut().flatten().enumerate() {
            let Some((decoded, next)) = decode_two_chart_scalar(body, cursor, slot % 2 == 0, cache)
            else {
                return Ok(None);
            };
            if next <= cursor || !decoded.is_finite() {
                return Ok(None);
            }
            *value = decoded;
            cursor = next;
        }
        ctx.push_vec(&mut samples, sample, sample_operation)?;
    }
    Ok((cursor == body.len()
        && samples.len() >= 2
        && count.is_none_or(|count| samples.len() == index_from_u32(count)))
    .then_some(samples))
}

/// Decode byte-complete two-chart sample bodies from one curve namespace.
///
/// A canonical body supplies `fc <count>`. Later rows in the same feature and
/// raw curve family replay the canonical sample extent without the prefix.
/// Every admitted row consumes exactly four finite scalars per sample.
pub(crate) fn two_chart_pcurve_samples(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Result<Vec<TwoChartPcurveSamples>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo two-chart namespace scratch")?;
    let cache = scratch.with_storage(|| scalar::ScalarCache::from_section_checked(ctx, payload))?;
    let framed = scratch.with_storage(|| framed_rows_with_face_ids(ctx, payload, face_ids))?;
    let mut canonical_counts = HashMap::<(usize, u32, u8), HashSet<u32>>::new();
    let mut candidates = Vec::new();
    for row in ctx.admit_iter(&framed, "creo two-chart canonical row traversal")? {
        let bytes = &payload[row.start..row.end];
        let Some(prefix) = topology_prefix(bytes, 0, row.suffix_start) else {
            continue;
        };
        let body = &bytes[prefix.end..row.suffix_start];
        if body.first() != Some(&0xfc) || body.get(1) == Some(&0x05) {
            continue;
        }
        let (count, start) = compact_int(body, 1);
        if start <= 1 {
            continue;
        }
        let (samples, storage) = ctx.with_scoped_storage(
            "creo two-chart counted sample points",
            || {
                complete_two_chart_samples(
                    ctx,
                    body,
                    start,
                    Some(count),
                    "creo two-chart counted sample points",
                    &cache,
                )
            },
        )?;
        let Some(samples) = samples else {
            continue;
        };
        if prefix.feature_id != 0 {
            scratch.with_storage(|| -> Result<(), cadmpeg_core::CodecError> {
                let counts = ctx
                    .entry_hash_map(
                        &mut canonical_counts,
                        (row.namespace_start, prefix.feature_id, prefix.type_byte),
                        "creo two-chart canonical group nodes",
                    )?
                    .or_default();
                ctx.insert_hash_set(counts, count, "creo two-chart canonical count nodes")?;
                Ok(())
            })?;
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut candidates,
                (
                    TwoChartPcurveSamples {
                        curve_id: prefix.id,
                        faces: [row.suffix[0], row.suffix[1]],
                        samples,
                        offset: row.start,
                    },
                    storage,
                ),
                "creo two-chart candidate rows",
            )
        })?;
    }
    for row in ctx.admit_iter(&framed, "creo two-chart replay row traversal")? {
        let bytes = &payload[row.start..row.end];
        let Some(prefix) = topology_prefix(bytes, 0, row.suffix_start) else {
            continue;
        };
        let body = &bytes[prefix.end..row.suffix_start];
        if body.first() == Some(&0xfc) {
            continue;
        }
        let Some(counts) =
            canonical_counts.get(&(row.namespace_start, prefix.feature_id, prefix.type_byte))
        else {
            continue;
        };
        let (samples, storage) = ctx.with_scoped_storage(
            "creo two-chart replay sample points",
            || {
                complete_two_chart_samples(
                    ctx,
                    body,
                    0,
                    None,
                    "creo two-chart replay sample points",
                    &cache,
                )
            },
        )?;
        let Some(samples) = samples else {
            continue;
        };
        let Ok(count) = u32::try_from(samples.len()) else {
            continue;
        };
        if !counts.contains(&count) {
            continue;
        }
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut candidates,
                (
                    TwoChartPcurveSamples {
                        curve_id: prefix.id,
                        faces: [row.suffix[0], row.suffix[1]],
                        samples,
                        offset: row.start,
                    },
                    storage,
                ),
                "creo two-chart candidate rows",
            )
        })?;
    }
    let mut counts = HashMap::new();
    for (record, _) in ctx.admit_iter(&candidates, "creo two-chart result count traversal")? {
        let count = scratch
            .with_storage(|| {
                ctx.entry_hash_map(
                    &mut counts,
                    record.curve_id,
                    "creo two-chart result count nodes",
                )
            })?
            .or_insert(0usize);
        *count += 1;
    }
    let mut result = Vec::new();
    for record_owned_storage in
        ctx.admit_iter(candidates, "creo two-chart selected row traversal")?
    {
        let storage = record_owned_storage.1;
        let record = record_owned_storage.0;
        if counts.get(&record.curve_id) != Some(&1) {
            continue;
        }
        let record = storage.commit_value(record)?;
        ctx.push_vec(&mut result, record, "creo two-chart sample rows")?;
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo two chart pcurve samples result ordering",
    )?;
    Ok(result)
}

fn complete_fc02_short_pcurve_values(record: &CurveParameterRecord) -> Option<[[f64; 2]; 2]> {
    const ZERO_MARKER: &[u8] = &[0x18];
    const ONE_MARKER: &[u8] = &[0xe4];
    const TWO_MARKER: &[u8] = &[0x29, 0xff, 0xff];

    (record.body.get(..2) == Some(&[0xfc, 0x02])).then_some(())?;
    record.references.is_empty().then_some(())?;
    let tokens: &[CurveParameterScalar; 7] = record.scalar_tokens.as_slice().try_into().ok()?;
    let [prefix, terminal] = record.opaque_spans.as_slice() else {
        return None;
    };
    (prefix.offset == 0
        && prefix.raw == [0xfc, 0x02]
        && terminal.raw.first() == Some(&0x34)
        && terminal.raw.len() == 3)
        .then_some(())?;
    let mut cursor = prefix.raw.len();
    for token in tokens {
        (token.offset == cursor
            && !token.raw.is_empty()
            && record.body.get(cursor..cursor + token.raw.len()) == Some(token.raw.as_slice()))
        .then_some(())?;
        cursor += token.raw.len();
    }
    (terminal.offset == cursor && terminal.offset + terminal.raw.len() == record.body.len())
        .then_some(())?;
    let values = tokens.each_ref().map(|token| token.value);
    (values.iter().all(|value| value.is_finite())
        && values[2] == 0.0
        && values[3] == 1.0
        && tokens[2].raw.as_slice() == ZERO_MARKER
        && tokens[3].raw.as_slice() == ONE_MARKER
        && tokens[6].raw.as_slice() == TWO_MARKER)
        .then_some(())?;
    Some([[values[0], values[1]], [values[4], values[5]]])
}

/// Decode complete one-sided endpoint paths from the short fc 02 body.
///
/// A path is admitted only when the body has one unique topology row, a
/// complete seven-scalar lane, and the bounded terminal operand. Other fc 02
/// bodies remain native parameter records until their grammar is settled.
pub(crate) fn fc02_short_pcurve_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Result<Vec<Fc02ShortPcurveEndpoints>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo pcurve topology scratch")?;
    let topology_index = scratch.with_storage(|| {
        unique_curve_index(ctx, topology, |row| row.id, "creo pcurve topology index")
    })?;
    let mut result = Vec::new();
    let parameter_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            parameters,
            |record| record.curve_id,
            "creo unique-row count nodes",
        )
    })?;
    for record in ctx.admit_iter(parameters, "creo unique parameter traversal")? {
        if !parameter_index
            .get(&record.curve_id)
            .is_some_and(Option::is_some)
        {
            continue;
        }
        let Some(face_0_endpoints) = complete_fc02_short_pcurve_values(record) else {
            continue;
        };
        let Some(Some(topology)) = topology_index.get(&record.curve_id) else {
            continue;
        };
        if topology.type_byte != record.type_byte {
            continue;
        }
        ctx.reserve_vec(&mut result, 1, "creo FC02 short pcurve endpoints")?;
        result.push(Fc02ShortPcurveEndpoints {
            curve_id: record.curve_id,
            faces: topology.faces.map(stored_face_reference),
            face_0_endpoints,
            offset: record.offset,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo fc02 short pcurve endpoints result ordering",
    )?;
    Ok(result)
}

/// Decode exact world-coordinate tokens from FC-prefixed dense curve bodies.
pub(crate) fn fc_coordinates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &[CurveParameterRecord],
) -> Result<Vec<FcCurveCoordinates>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo curve parameter index scratch")?;
    let mut result = Vec::new();
    let parameter_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            parameters,
            |record| record.curve_id,
            "creo unique-row count nodes",
        )
    })?;
    for record in ctx.admit_iter(parameters, "creo unique parameter traversal")? {
        if !parameter_index
            .get(&record.curve_id)
            .is_some_and(Option::is_some)
        {
            continue;
        }
        let Some((&0xfc, tail)) = record.body.split_first() else {
            continue;
        };
        let Some((&subtype, lane)) = tail.split_first() else {
            continue;
        };
        let mut token_storage = ctx.reserve_scoped(0, "creo fc coordinate candidate scratch")?;
        let mut tokens = Vec::new();
        let mut cursor = 0;
        while cursor < lane.len() {
            ctx.next_charged(&mut lane[cursor..].iter(), "creo fc coordinate scan")?;
            if matches!(lane[cursor], 0x46 | 0x2d) {
                if let Some((value, next)) = scalar::decode(lane, cursor) {
                    token_storage.with_storage(|| {
                        ctx.reserve_vec(&mut tokens, 1, "creo fc coordinate tokens")
                    })?;
                    tokens.push(FcCurveCoordinateToken {
                        value_mm: value,
                        raw: token_storage.with_storage(|| {
                            ctx.copy_retained(&lane[cursor..next], "creo fc coordinate token bytes")
                        })?,
                        offset: cursor + 2,
                    });
                    cursor = next;
                    continue;
                }
            }
            cursor += 1;
        }
        if tokens.len() >= 4 {
            let tokens = token_storage.commit_value(tokens)?;
            let mut opaque_spans = Vec::new();
            let mut unclaimed = 0;
            for token in ctx.admit_iter(&tokens, "creo fc coordinate opaque span traversal")? {
                if unclaimed < token.offset {
                    ctx.reserve_vec(&mut opaque_spans, 1, "creo fc opaque spans")?;
                    opaque_spans.push(FcCurveOpaqueSpan {
                        raw: ctx.copy_retained(
                            &record.body[unclaimed..token.offset],
                            "creo fc opaque span bytes",
                        )?,
                        offset: unclaimed,
                    });
                }
                unclaimed = token.offset + token.raw.len();
            }
            if unclaimed < record.body.len() {
                ctx.reserve_vec(&mut opaque_spans, 1, "creo fc opaque spans")?;
                opaque_spans.push(FcCurveOpaqueSpan {
                    raw: ctx
                        .copy_retained(&record.body[unclaimed..], "creo fc opaque span bytes")?,
                    offset: unclaimed,
                });
            }
            let mut values_mm = Vec::new();
            ctx.reserve_vec(&mut values_mm, tokens.len(), "creo fc coordinate values")?;
            for token in ctx.admit_iter(&tokens, "creo fc coordinate value traversal")? {
                values_mm.push(token.value_mm);
            }
            let body = ctx.copy_retained(&record.body, "creo fc coordinate body")?;
            ctx.reserve_vec(&mut result, 1, "creo fc coordinate rows")?;
            result.push(FcCurveCoordinates {
                curve_id: record.curve_id,
                subtype,
                body,
                values_mm,
                tokens,
                opaque_spans,
                offset: record.offset,
            });
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo fc coordinates result ordering",
    )?;
    Ok(result)
}

fn fc05_scalar(body: &[u8], offset: usize) -> Option<(f64, usize)> {
    let prefix = *body.get(offset)?;
    if prefix == 0x18 {
        return Some((0.0, offset + 1));
    }
    if let Some(decoded) = scalar::decode_positive_dict(body, offset) {
        return Some(decoded);
    }
    if let Some(decoded) = scalar::decode(body, offset) {
        return Some(decoded);
    }
    if matches!(prefix, 0xe0..=0xe3 | 0xf7 | 0xf8) || offset + 7 > body.len() {
        return None;
    }
    // wrapping-exception: DICT prefix remapping reconstructs the low IEEE byte modulo 256
    let byte_1 = prefix.wrapping_sub(0x8b);
    scalar::ieee7_with_prefix(
        body,
        offset,
        if byte_1 >= 0x80 { 0x3f } else { 0x40 },
        byte_1,
    )
}

/// Validate FC05 point lanes against their exact circle identity.
pub(crate) fn fc05_circles(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &[CurveParameterRecord],
) -> Result<Vec<Fc05Circle>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo fc05 parameter index scratch")?;
    let mut circles = Vec::new();
    let parameter_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            parameters,
            |record| record.curve_id,
            "creo unique-row count nodes",
        )
    })?;
    for record in ctx.admit_iter(parameters, "creo unique parameter traversal")? {
        if !parameter_index
            .get(&record.curve_id)
            .is_some_and(Option::is_some)
        {
            continue;
        }
        if record.body.get(..2) != Some(&[0xfc, 0x05]) {
            continue;
        }
        let mut point_storage = ctx.reserve_scoped(0, "creo fc05 point scratch")?;
        let mut points = Vec::new();
        let mut cursor = 2;
        while cursor < record.body.len() {
            ctx.next_charged(&mut record.body[cursor..].iter(), "creo fc05 point scan")?;
            if !matches!(record.body[cursor], 0x46 | 0x2d) {
                break;
            }
            let Some((x, next)) = fc05_scalar(&record.body, cursor) else {
                break;
            };
            let Some((z, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            let parameter_start = next;
            let Some((decoded_parameter, decoded_next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            let (parameter, next) = if matches!(record.body.get(decoded_next), Some(0x46 | 0x2d)) {
                (Some(decoded_parameter), decoded_next)
            } else {
                let following = (parameter_start + 1..(parameter_start + 9).min(record.body.len()))
                    .find(|offset| matches!(record.body[*offset], 0x46 | 0x2d));
                let Some(following) = following else {
                    break;
                };
                (None, following)
            };
            let Some((ordinate, next)) = fc05_scalar(&record.body, next) else {
                break;
            };
            point_storage
                .with_storage(|| ctx.reserve_vec(&mut points, 1, "creo fc05 point rows"))?;
            points.push((x, z, parameter, ordinate));
            cursor = next;
        }
        if cursor != record.body.len() && record.body.get(cursor..) != Some(&[0xff]) {
            continue;
        }
        if points.len() < 4 {
            continue;
        }
        let ordinate = points[0].3;
        if ctx.any_by(
            &points,
            |point| Ok((point.3 - ordinate).abs() > EPS_ORDINATE_AGREEMENT),
            "creo fc05 ordinate agreement scan",
        )? {
            continue;
        }
        let first = points[0];
        let middle = points[points.len() / 2];
        let last = points[points.len() - 1];
        let middle_delta = [middle.0 - first.0, middle.1 - first.1];
        let last_delta = [last.0 - first.0, last.1 - first.1];
        let scale = middle_delta
            .into_iter()
            .chain(last_delta)
            .map(f64::abs)
            .fold(0.0, f64::max);
        if !scale.is_finite() || scale == 0.0 {
            continue;
        }
        let middle_delta = middle_delta.map(|value| value / scale);
        let last_delta = last_delta.map(|value| value / scale);
        let determinant = middle_delta[0].mul_add(last_delta[1], -middle_delta[1] * last_delta[0]);
        if determinant.abs() <= 64.0 * f64::EPSILON {
            continue;
        }
        // Fit in a translated chart so subtracting squared world positions
        // cannot erase a small circle's radius.
        let middle_squared =
            middle_delta[0].mul_add(middle_delta[0], middle_delta[1] * middle_delta[1]);
        let last_squared = last_delta[0].mul_add(last_delta[0], last_delta[1] * last_delta[1]);
        let center_u = 0.5 * middle_squared.mul_add(last_delta[1], -middle_delta[1] * last_squared)
            / determinant;
        let center_v = 0.5 * middle_delta[0].mul_add(last_squared, -middle_squared * last_delta[0])
            / determinant;
        let center_x = center_u.mul_add(scale, first.0);
        let center_z = center_v.mul_add(scale, first.1);
        let radius = (first.0 - center_x).hypot(first.1 - center_z);
        if ![center_x, center_z, radius].into_iter().all(f64::is_finite) || radius <= 0.0 {
            continue;
        }
        let max_residual = ctx
            .admit_iter(&points, "creo fc05 residual traversal")?
            .map(|point| ((point.0 - center_x).hypot(point.1 - center_z) - radius).abs())
            .fold(0.0, f64::max);
        if max_residual > EPS_CIRCLE_RESIDUAL * radius {
            continue;
        }
        let angle_0 = (first.1 - center_z).atan2(first.0 - center_x);
        let parameter_0 = first.2;
        let wrapped_distance = |left: f64, right: f64| {
            let difference = left - right;
            difference
                .is_finite()
                .then(|| difference.rem_euclid(std::f64::consts::TAU))
                .map_or(f64::INFINITY, |wrapped| {
                    wrapped.min(std::f64::consts::TAU - wrapped)
                })
        };
        let sign_matches = |sign: f64| {
            ctx.all_by(
                &points,
                |point| {
                    let (Some(parameter), Some(parameter_0)) = (point.2, parameter_0) else {
                        return Ok(false);
                    };
                    let angle = (point.1 - center_z).atan2(point.0 - center_x);
                    let expected = angle_0 + sign * (parameter - parameter_0);
                    Ok(wrapped_distance(angle, expected) <= EPS_ANGLE_AGREEMENT)
                },
                "creo fc05 angle parameter scan",
            )
        };
        let positive = sign_matches(1.0)?;
        let negative = sign_matches(-1.0)?;
        let angle_parameter = match (positive, negative, parameter_0) {
            (true, false, Some(parameter_0)) | (false, true, Some(parameter_0)) => {
                let sense = if positive {
                    ParameterSense::Increasing
                } else {
                    ParameterSense::Decreasing
                };
                let reference_angle = angle_0 - f64::from(sense.as_i8()) * parameter_0;
                Fc05AngleParameterRelation::Consistent {
                    sense,
                    reference_direction_row_frame: [reference_angle.cos(), reference_angle.sin()],
                }
            }
            _ => Fc05AngleParameterRelation::Inconsistent,
        };
        let Some((sample_direction_row_frame, _)) =
            cadmpeg_ir::units::HypotDirection2::normalized_with_length([
                first.0 - center_x,
                first.1 - center_z,
            ])
        else {
            continue;
        };
        ctx.reserve_vec(&mut circles, 1, "creo fc05 circles")?;
        circles.push(Fc05Circle {
            curve_id: record.curve_id,
            center_row_frame: [center_x, center_z],
            radius_mm: radius,
            sample_direction_row_frame,
            angle_parameter,
            cap_ordinate_row_frame: Some(ordinate),
            point_count: points.len(),
            max_residual,
            offset: record.offset,
        });
    }
    ctx.stable_sort_by(
        circles.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo fc05 circles circles ordering",
    )?;
    Ok(circles)
}

/// Bind validated `fc 05` circles to typed cylinder/plane face pairs and retain
/// only groups that agree on radius and center at two distinct cap ordinates.
pub(crate) fn fc05_cylinder_cap_pairs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    circles: &[Fc05Circle],
    topology: &[CurveTopologyRow],
    surfaces: &crate::surface::SurfaceRows,
) -> Result<Vec<Fc05CylinderCapPair>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo fc05 cap group scratch")?;
    let faces = scratch.with_storage(|| {
        unique_curve_index(ctx, topology, |row| row.id, "creo fc05 topology-face nodes")
    })?;
    let circle_counts = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            circles,
            |circle| circle.curve_id,
            "creo fc05 circle-count nodes",
        )
    })?;
    let mut groups = BTreeMap::<u32, Vec<(&Fc05Circle, u32, f64)>>::new();
    for circle in ctx.admit_iter(circles, "creo fc05 circle grouping traversal")? {
        if !circle_counts
            .get(&circle.curve_id)
            .is_some_and(Option::is_some)
        {
            continue;
        }
        let Some(Some(topology)) = faces.get(&circle.curve_id) else {
            continue;
        };
        let adjacent = &topology.faces;
        let mut cylinders = adjacent
            .iter()
            .flatten()
            .map(|face| face.get())
            .filter(|face| {
                crate::surface::unique_surface_row(surfaces, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            });
        let mut planes = adjacent
            .iter()
            .flatten()
            .map(|face| face.get())
            .filter(|face| {
                crate::surface::unique_surface_row(surfaces, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            });
        let (Some(cylinder), None, Some(plane), None, Some(ordinate)) = (
            cylinders.next(),
            cylinders.next(),
            planes.next(),
            planes.next(),
            circle.cap_ordinate_row_frame,
        ) else {
            continue;
        };
        let group = scratch
            .with_storage(|| {
                ctx.entry_btree_map(&mut groups, cylinder, "creo fc05 cylinder group nodes")
            })?
            .or_default();
        scratch.with_storage(|| ctx.reserve_vec(group, 1, "creo fc05 cylinder group members"))?;
        group.push((circle, plane, ordinate));
    }

    let mut result = Vec::new();
    for (surface_id, mut group) in ctx.admit_iter(groups, "creo fc05 cap group traversal")? {
        ctx.stable_sort_by(
            group.as_mut_slice(),
            |value| &value.0.offset,
            Ord::cmp,
            "creo fc05 cylinder cap pairs group ordering",
        )?;
        let first = group[0].0;
        let Fc05AngleParameterRelation::Consistent {
            sense: parameter_sense,
            reference_direction_row_frame,
        } = first.angle_parameter
        else {
            continue;
        };
        let tolerance = EPS_RADIUS_AGREEMENT * first.radius_mm;
        if !ctx.all_by(
            &group,
            |(circle, _, _)| {
                let Fc05AngleParameterRelation::Consistent {
                    sense,
                    reference_direction_row_frame: direction,
                } = circle.angle_parameter
                else {
                    return Ok(false);
                };
                Ok((circle.radius_mm - first.radius_mm).abs() <= tolerance
                    && (circle.center_row_frame[0] - first.center_row_frame[0]).abs() <= tolerance
                    && (circle.center_row_frame[1] - first.center_row_frame[1]).abs() <= tolerance
                    && sense == parameter_sense
                    && (direction[0] - reference_direction_row_frame[0]).abs()
                        <= EPS_RADIUS_AGREEMENT
                    && (direction[1] - reference_direction_row_frame[1]).abs()
                        <= EPS_RADIUS_AGREEMENT)
            },
            "creo fc05 cap agreement scan",
        )? {
            continue;
        }
        let mut ordinate_storage = ctx.reserve_scoped(0, "creo fc05 ordinate scratch")?;
        let mut ordinates = Vec::new();
        for ordinate in ctx
            .admit_iter(&group, "creo fc05 ordinate traversal")?
            .map(|(_, _, ordinate)| *ordinate)
        {
            if ctx.all_by(
                &ordinates,
                |existing: &f64| Ok((*existing - ordinate).abs() > tolerance),
                "creo fc05 distinct ordinate scan",
            )? {
                ordinate_storage.with_storage(|| {
                    ctx.reserve_vec(&mut ordinates, 1, "creo fc05 distinct cap ordinates")
                })?;
                ordinates.push(ordinate);
            }
        }
        if ordinates.len() < 2 {
            continue;
        }
        let ordinates = ordinate_storage.commit_value(ordinates)?;
        let mut cap_edges = Vec::new();
        ctx.reserve_vec(&mut cap_edges, group.len(), "creo fc05 cap edges")?;
        cap_edges.extend(ctx.admit_iter(&group, "creo fc05 cap edge traversal")?.map(
            |(circle, plane, ordinate)| Fc05CapEdge {
                curve_id: circle.curve_id,
                cap_plane_id: *plane,
                cap_ordinate_row_frame: *ordinate,
            },
        ));
        ctx.reserve_vec(&mut result, 1, "creo fc05 cylinder cap pairs")?;
        result.push(Fc05CylinderCapPair {
            surface_id,
            cap_edges,
            center_row_frame: first.center_row_frame,
            radius_mm: first.radius_mm,
            reference_direction_row_frame,
            parameter_sense,
            cap_ordinates_row_frame: ordinates,
            offset: first.offset,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo fc05 cylinder cap pairs result ordering",
    )?;
    Ok(result)
}

/// Decode labeled `crv_pnt_arr f9 02 04` prototype pcurve endpoints.
pub(crate) fn prototype_pcurve_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<PrototypePcurveEndpoints>, cadmpeg_core::CodecError> {
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut result = Vec::new();
    let mut namespaces = ctx.find_bytes_iter(payload, b"crv_array\0", "find Creo curve marker")?;
    let mut current = namespaces.next();
    while let Some(namespace) = current {
        let start = namespace + b"crv_array\0".len();
        current = namespaces.next();
        let end = current.unwrap_or(payload.len());
        let Some(id_label) =
            ctx.find_bytes_in(payload, b"crv_id\0", start, end, "find Creo curve marker")?
        else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (curve_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let prototype_end = ctx
            .find_bytes_in(
                payload,
                b"topol_ref_data\0",
                after_id,
                end,
                "find Creo curve marker",
            )?
            .unwrap_or(end);
        let Some(points_label) =
            unique_find_in(ctx, payload, b"crv_pnt_arr\0", after_id, prototype_end)?
        else {
            continue;
        };
        let header = points_label + b"crv_pnt_arr\0".len();
        if payload.get(header..header + 3) != Some(&[psb::token::SCALAR_BODY, 0x02, 0x04]) {
            continue;
        }
        let mut cursor = header + 3;
        let mut values = [0.0; 8];
        let mut value_count = 0;
        while cursor < prototype_end && value_count < values.len() {
            if payload[cursor] == 0x12
                || (payload[cursor] == 0x18
                    && value_count == 7
                    && (cursor + 1 == prototype_end || payload.get(cursor + 1) == Some(&0xe0)))
            {
                values[value_count] = 0.0;
                value_count += 1;
                cursor += 1;
            } else if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, &cache) {
                values[value_count] = value;
                value_count += 1;
                cursor = next;
            } else {
                break;
            }
        }
        let array_is_bounded = cursor == prototype_end || payload.get(cursor) == Some(&0xe0);
        if value_count == values.len()
            && values.iter().all(|value| value.is_finite())
            && array_is_bounded
        {
            ctx.reserve_vec(&mut result, 1, "creo prototype pcurve endpoints")?;
            result.push(PrototypePcurveEndpoints {
                curve_id,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: points_label,
            });
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo prototype pcurve endpoints result ordering",
    )?;
    Ok(result)
}

/// Decode the four labeled topology pointers of each curve prototype.
pub(crate) fn prototype_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<CurvePrototypeTopology>, cadmpeg_core::CodecError> {
    let mut result = Vec::new();
    let mut namespaces = ctx.find_bytes_iter(payload, b"crv_array\0", "find Creo curve marker")?;
    let mut current = namespaces.next();
    while let Some(namespace) = current {
        let start = namespace + b"crv_array\0".len();
        current = namespaces.next();
        let end = current.unwrap_or(payload.len());
        let Some(id_label) =
            ctx.find_bytes_in(payload, b"crv_id\0", start, end, "find Creo curve marker")?
        else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let Ok((curve_id, _)) = reference_id(payload, id_start) else {
            continue;
        };
        let prototype_end = ctx
            .find_bytes_in(
                payload,
                b"topol_ref_data\0",
                id_start,
                end,
                "find Creo curve marker",
            )?
            .unwrap_or(end);
        let reference = |label: &[u8]| -> Result<Option<u32>, cadmpeg_core::CodecError> {
            let Some(offset) = unique_find_in(ctx, payload, label, id_start, prototype_end)? else {
                return Ok(None);
            };
            let at = offset + label.len();
            Ok(reference_id(payload, at).ok().map(|(value, _)| value))
        };
        let Some(face_0) = reference(b"crv_hdr_geom_ptr[0]\0")? else {
            continue;
        };
        let Some(face_1) = reference(b"crv_hdr_geom_ptr[1]\0")? else {
            continue;
        };
        let Some(next_0) = reference(b"next_crv_hdr_ptr[0]\0")? else {
            continue;
        };
        let Some(next_1) = reference(b"next_crv_hdr_ptr[1]\0")? else {
            continue;
        };
        ctx.reserve_vec(&mut result, 1, "creo prototype topology rows")?;
        result.push(CurvePrototypeTopology {
            curve_id,
            faces: [face_0, face_1].map(NonZeroU32::new),
            next_edges: [next_0, next_1],
            offset: namespace,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo prototype topology result ordering",
    )?;
    Ok(result)
}

/// Bind complete prototype UV endpoints to labeled prototype topology.
pub(crate) fn bind_prototype_pcurves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurves: &[PrototypePcurveEndpoints],
    topology: &[CurvePrototypeTopology],
) -> Result<Vec<BoundPrototypePcurve>, cadmpeg_core::CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo bound prototype pcurve scratch")?;
    let pcurve_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            pcurves,
            |row| row.curve_id,
            "creo prototype pcurve count nodes",
        )
    })?;
    let topology_index = scratch.with_storage(|| {
        unique_curve_index(
            ctx,
            topology,
            |row| row.curve_id,
            "creo prototype topology count nodes",
        )
    })?;
    let mut result = Vec::new();
    for pcurve in ctx.admit_iter(pcurves, "creo prototype pcurve binding traversal")? {
        if !matches!(pcurve_index.get(&pcurve.curve_id), Some(Some(_))) {
            continue;
        }
        let Some(Some(topology)) = topology_index.get(&pcurve.curve_id) else {
            continue;
        };
        ctx.reserve_vec(&mut result, 1, "creo bound prototype pcurves")?;
        result.push(BoundPrototypePcurve {
            curve_id: pcurve.curve_id,
            faces: topology.faces,
            face_0_endpoints: pcurve.face_0_endpoints,
            face_1_endpoints: pcurve.face_1_endpoints,
            offset: pcurve.offset,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo bind prototype pcurves result ordering",
    )?;
    Ok(result)
}

/// The stored face identifier of a bounded half-edge side; `0` is unbounded.
fn stored_face_reference(face: Option<NonZeroU32>) -> u32 {
    face.map_or(0, NonZeroU32::get)
}

fn parse_topology_row(
    row: &[u8],
    absolute_offset: usize,
    suffix_start: usize,
    [f0, f1, e0, e1]: [u32; 4],
) -> Option<CurveTopologyRow> {
    let prefix = topology_prefix(row, 0, suffix_start)?;
    Some(CurveTopologyRow {
        id: prefix.id,
        type_byte: prefix.type_byte,
        feature_id: prefix.feature_id,
        directions: prefix.directions,
        faces: [f0, f1].map(NonZeroU32::new),
        next_edges: [e0, e1],
        offset: absolute_offset,
    })
}

fn topology_prefix(row: &[u8], start: usize, suffix_start: usize) -> Option<TopologyPrefix> {
    let fields = topology_prefix_fields(row, start)?;
    (fields.end <= suffix_start).then_some(fields)
}

fn topology_prefix_fields(row: &[u8], start: usize) -> Option<TopologyPrefix> {
    let (id, after_id) = compact_int(row, start);
    (after_id > start).then_some(())?;
    let type_byte = *row.get(after_id)?;
    let (feature_id, after_feature) = compact_int(row, after_id + 1);
    (after_feature > after_id + 1).then_some(())?;
    let directions = [*row.get(after_feature)?, *row.get(after_feature + 1)?];
    directions
        .iter()
        .all(|direction| matches!(direction, 0x01 | 0xf6))
        .then_some(TopologyPrefix {
            id,
            type_byte,
            feature_id,
            directions,
            end: after_feature + 2,
        })
}

fn topology_suffix_with_face_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    row: &[u8],
    materialized_face_ids: Option<&BTreeSet<u32>>,
    known_face_ids: Option<&BTreeSet<u32>>,
) -> Result<Option<TopologySuffixCandidate>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(candidates) = topology_suffix_candidates(row) else {
        return Ok(None);
    };
    let mut initial = candidates.iter().flatten().copied();
    if let (Some(candidate), None) = (initial.next(), initial.next()) {
        return Ok(Some(candidate));
    }
    // The fixed candidate array has at most 24 rows and two faces per row.
    for ids in [materialized_face_ids, known_face_ids]
        .into_iter()
        .flatten()
        .filter(|ids| !ids.is_empty())
    {
        let mut selected = None;
        for candidate in candidates.iter().flatten() {
            let mut valid = true;
            for id in candidate.faces.into_iter().flatten() {
                if !ctx.contains_btree_set(ids, &id.get(), "creo topology suffix face lookup")? {
                    valid = false;
                    break;
                }
            }
            if valid {
                if selected.is_some() {
                    return Ok(None);
                }
                selected = Some(*candidate);
            }
        }
        if selected.is_some() {
            return Ok(selected);
        }
    }
    Ok(None)
}

fn unique_topology_suffix_in_segment(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    segment: &[u8],
) -> Result<Option<TopologySuffixCandidate>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut closes = segment.windows(3).enumerate().rev();
    while closes.len() != 0 {
        let Some((close, bytes)) =
            ctx.next_charged(&mut closes, "creo unique topology suffix close scan")?
        else {
            break;
        };
        if bytes != [0, 0, psb::token::COMPOUND_CLOSE] {
            continue;
        }
        let Some(candidates) = topology_suffix_candidates(&segment[..close + 3]) else {
            continue;
        };
        let mut unique = candidates.into_iter().flatten();
        if let (Some(candidate), None) = (unique.next(), unique.next()) {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn topology_suffix_candidates(row: &[u8]) -> Option<[Option<TopologySuffixCandidate>; 24]> {
    let close = row.len().checked_sub(1)?;
    (row.get(close) == Some(&psb::token::COMPOUND_CLOSE)).then_some(())?;
    let mut reference_geometry_candidates = [None; 3];
    let mut reference_count = 0;
    if close.checked_sub(2).and_then(|start| row.get(start..close)) == Some(&[0, 0]) {
        reference_geometry_candidates[0] = Some((close - 2, [0, 0]));
        reference_count = 1;
    } else {
        for length in 2..=4 {
            let Some(start) = close.checked_sub(length) else {
                continue;
            };
            let Some((first, next)) = generic_compact_at(row, start) else {
                continue;
            };
            let Some((second, end)) = generic_compact_at(row, next) else {
                continue;
            };
            if end == close {
                reference_geometry_candidates[reference_count] = Some((start, [first, second]));
                reference_count += 1;
            }
        }
    }
    let mut candidates = [None; 24];
    let mut candidate_count = 0;
    for (reference_geometry_start, reference_geometry) in reference_geometry_candidates
        .into_iter()
        .take(reference_count)
        .flatten()
    {
        for length in 4..=11 {
            let Some(start) = reference_geometry_start.checked_sub(length) else {
                continue;
            };
            let Ok((f0, p1)) = reference_id(row, start) else {
                continue;
            };
            let Ok((f1, p2)) = reference_id(row, p1) else {
                continue;
            };
            let Ok((e0, p3)) = reference_id(row, p2) else {
                continue;
            };
            let Ok((e1, end)) = reference_id(row, p3) else {
                continue;
            };
            if end == reference_geometry_start {
                candidates[candidate_count] = Some(TopologySuffixCandidate {
                    start,
                    faces: [f0, f1].map(NonZeroU32::new),
                    next_edges: [e0, e1],
                    reference_geometry,
                });
                candidate_count += 1;
            }
        }
    }
    Some(candidates)
}

fn unique_find_in(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    needle: &[u8],
    from: usize,
    end: usize,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let Some(offset) =
        ctx.find_bytes_in(data, needle, from, end, "find Creo unique curve field")?
    else {
        return Ok(None);
    };
    let Some(next) = offset.checked_add(1) else {
        return Ok(None);
    };
    Ok(ctx
        .find_bytes_in(data, needle, next, end, "find Creo unique curve field")?
        .is_none()
        .then_some(offset))
}

#[cfg(test)]
mod tests;
