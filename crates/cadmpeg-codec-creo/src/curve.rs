// SPDX-License-Identifier: Apache-2.0
//! Curve namespace prototypes and topology rows.
//!
//! Prototype rows identify curves and their generating features. Topology rows
//! add the two face sides and successor curve for each native half-edge. Curve
//! parameter bodies are not interpreted here.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::bytes::{find_from as find, find_in};
use cadmpeg_core::decode::{alloc_filled, bounded_len};

use crate::psb::{self, compact_int, reference_id};
use crate::scalar;

const EPS_RELATION_ROUND: f64 = 1.0e-9;
const EPS_DIMENSION_SOLUTION: f64 = 1.0e-9;
const EPS_LINEAR_SYSTEM_COEFFICIENT: f64 = 1.0e-12;
const EPS_LINEAR_SYSTEM_RESIDUAL: f64 = 1.0e-9;
const EPS_ORDINATE_AGREEMENT: f64 = 1.0e-9;
const EPS_CIRCLE_RESIDUAL: f64 = 1.0e-9;
const EPS_ANGLE_AGREEMENT: f64 = 1.0e-6;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;

/// A labeled curve namespace entry.
///
/// `type_byte` remains raw because the namespace grammar does not define its
/// geometric interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototype {
    /// The row's `crv_id`: the curve's identifier in the `crv_array`
    /// namespace, referenced by `srf_array` and topology row `E0`/`E1`
    /// fields.
    pub id: u32,
    /// The row's raw `type` byte. Its geometric meaning is not identified by
    /// the namespace grammar alone ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)); the curve-body evaluator
    /// determines the interpretation.
    pub type_byte: u8,
    /// The `feat_id` compact integer, when the labeled row has one: the
    /// feature that generated this curve.
    pub feature_id: Option<u32>,
    /// The two named-prototype `crv_pnt_dir` orientation flags, when the
    /// prototype carries a complete direction array.
    pub directions: Option<[u8; 2]>,
    /// Byte offset of this prototype's `crv_array` label in the original
    /// stream.
    pub offset: usize,
}

/// One source line in a curve-equation expression program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveExpressionLine {
    /// UTF-8 source text without its NUL terminator.
    pub text: String,
    /// Byte offset of the first source byte.
    pub offset: usize,
}

/// Expression program stored by a curve-from-equation entity.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionRecord {
    /// Entity identifier from the enclosing record.
    pub entity_id: u32,
    /// Whether the enclosing record is `backup_ents(crv_fr_eqn)`.
    pub backup: bool,
    /// Bounded native placement frame carried by the equation entity.
    pub local_system: Option<CurveExpressionLocalSystem>,
    /// Ordered source lines declared by the `f8` array.
    pub lines: Vec<CurveExpressionLine>,
    /// Assignment statements in source order.
    pub assignments: Vec<CurveExpressionAssignment>,
    /// Complete simultaneous-equation blocks in source order.
    pub solve_blocks: Vec<CurveExpressionSolveBlock>,
    /// Whether a `SOLVE`/`FOR` control sequence is malformed or incomplete.
    pub unresolved_solve_control: bool,
    /// Curve-equation constructs prohibited by the Creo expression grammar.
    pub prohibited_constructs: Vec<String>,
    /// Byte offset of the enclosing entity label.
    pub offset: usize,
    /// Byte offset of the `expression` field.
    pub expression_offset: usize,
}

/// Count-bounded `local_sys` payload carried by a curve-equation entity.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionLocalSystem {
    /// Tuple dimensionality from the `f9` wrapper.
    pub dimensions: u32,
    /// Stored tuple count from the `f9` wrapper.
    pub count: u32,
    /// Exact stateful scalar body through the next named field.
    pub body: Vec<u8>,
    /// Twelve explicit scalar slots, absent when the body uses inheritance or
    /// contains a scalar form that is not decoded.
    pub explicit_slots: Option<[f64; 12]>,
    /// Byte offset of the `local_sys` named-record header.
    pub offset: usize,
}

/// One executable assignment in a curve expression program.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionAssignment {
    /// Typed relation target receiving the right-hand value.
    pub target: CurveExpressionTarget,
    /// Exact right-hand expression after surrounding ASCII whitespace removal.
    pub expression: String,
    /// Referenced identifiers in first-appearance order.
    pub dependencies: Vec<String>,
    /// Sequentially evaluated value when every dependency is resolved.
    pub value: Option<CurveExpressionValue>,
    /// Whether the source-ordered conditional program executes this assignment.
    pub activation: CurveExpressionActivation,
    /// Byte offset of the assignment source line.
    pub offset: usize,
}

/// One `SOLVE`/`FOR` simultaneous-equation block.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveExpressionSolveBlock {
    /// Ordered equations between the `SOLVE` and `FOR` lines.
    pub equations: Vec<CurveExpressionEquation>,
    /// Ordered one-way relations in the block that do not involve an unknown.
    pub assignments: Vec<CurveExpressionAssignment>,
    /// Ordered unknowns declared by the terminating `FOR` line.
    pub variables: Vec<String>,
    /// Solved scalar values aligned with `variables`; absent entries remain
    /// nonlinear, underdetermined, inconsistent, or dependency-unresolved.
    pub solutions: Vec<Option<CurveExpressionValue>>,
    /// Byte offset of the `SOLVE` line.
    pub offset: usize,
    /// Byte offset of the terminating `FOR` line.
    pub for_offset: usize,
}

/// One equation in a simultaneous-equation block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveExpressionEquation {
    /// Exact left-hand expression after surrounding ASCII whitespace removal.
    pub left: String,
    /// Exact right-hand expression after surrounding ASCII whitespace removal.
    pub right: String,
    /// Referenced identifiers in first-appearance order across both sides.
    pub dependencies: Vec<String>,
    /// Byte offset of the equation source line.
    pub offset: usize,
}

/// Target of one curve-expression assignment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveExpressionTarget {
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
pub enum CurveExpressionSystemSymbolFamily {
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
pub enum CurveExpressionValue {
    /// Dimensionless numeric value.
    Number(f64),
    /// Length in canonical millimeters.
    Length(f64),
    /// Angle in relation degrees.
    Angle(f64),
    /// Quantity whose physical dimension has no dedicated neutral value type.
    Quantity(CurveExpressionQuantity),
    /// UTF-8 string value.
    String(String),
}

impl CurveExpressionValue {
    fn truth(&self) -> Option<bool> {
        match self {
            Self::Number(value) => Some(*value != 0.0),
            Self::Length(_) | Self::Angle(_) | Self::Quantity(_) | Self::String(_) => None,
        }
    }
}

/// Canonically scaled relation quantity represented by physical base powers.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CurveExpressionQuantity {
    /// Numeric value in canonical millimeter-kilogram-second-degree-kelvin units.
    pub value: f64,
    /// Power of length.
    pub length_power: i8,
    /// Power of mass.
    pub mass_power: i8,
    /// Power of time.
    pub time_power: i8,
    /// Power of plane angle.
    pub angle_power: i8,
    /// Power of temperature.
    pub temperature_power: i8,
}

impl CurveExpressionQuantity {
    fn dimension(self) -> RelationDimension {
        RelationDimension {
            length: self.length_power,
            mass: self.mass_power,
            time: self.time_power,
            angle: self.angle_power,
            temperature: self.temperature_power,
        }
    }
}

/// Evaluation state of an assignment inside relation conditionals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveExpressionActivation {
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
pub struct CurveExpressionHelix {
    /// Constant cylindrical radius in model millimeters.
    pub radius: f64,
    /// Signed axial rise from `t = 0` through `t = 1`.
    pub height: f64,
    /// Native axial coordinate at `t = 0`.
    pub z_start: f64,
    /// Positive angular travel in revolutions.
    pub revolutions: f64,
    /// Angular position at `t = 0`, in radians.
    pub start_angle: f64,
    /// Whether angular travel decreases as `t` increases.
    pub clockwise: bool,
}

/// A curve row with a uniquely delimited topology suffix.
///
/// `faces` and `next_edges` preserve the two native sides in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveTopologyRow {
    /// The row's `crv_id`, matching a [`CurvePrototype::id`] in the same
    /// `crv_array` namespace.
    pub id: u32,
    /// The row's raw `type` byte; see [`CurvePrototype::type_byte`].
    pub type_byte: u8,
    /// The `feat_id` compact integer: the feature that generated this
    /// curve.
    pub feature_id: u32,
    /// The two `crv_pnt_dir` orientation-flag bytes, one per half-edge side.
    /// These are per-side orientation flags, not a tangent vector
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub directions: [u8; 2],
    /// The `F0`/`F1` suffix fields: the `srf_array` face identifiers
    /// bounding the curve's two half-edge sides.
    pub faces: [u32; 2],
    /// The `E0`/`E1` suffix fields: the `crv_array` identifier of the next
    /// edge for each of the two half-edge sides, used to walk loops
    /// ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#4-curve-namespace-crv_array)).
    pub next_edges: [u32; 2],
    /// Byte offset of the row's `crv_id` field in the original stream.
    pub offset: usize,
}

/// One DEPDB cross-section curve row with its one-sided topology suffix.
#[derive(Debug, Clone, PartialEq)]
pub struct DepdbCurveRow {
    /// Curve identifier in the cross-section `crv_array` namespace.
    pub id: u32,
    /// Raw curve-family discriminator.
    pub type_byte: u8,
    /// Owning feature identifier.
    pub feature_id: u32,
    /// Stored per-side direction flags.
    pub directions: [u8; 2],
    /// The `[0, X1, F1, 0]` one-sided suffix.
    pub suffix: [u32; 4],
    /// Exact bytes between the fixed prefix and one-sided suffix.
    pub body: Vec<u8>,
    /// Decoded scalar tokens with exact body-relative spans.
    pub scalar_tokens: Vec<CurveParameterScalar>,
    /// Canonical entity references with exact body-relative spans.
    pub references: Vec<CurveParameterReference>,
    /// Maximal body spans not claimed by a scalar or reference token.
    pub opaque_spans: Vec<CurveParameterOpaqueSpan>,
    /// Byte offset of the row identifier.
    pub offset: usize,
}

/// Resolution state of a curve row's four-reference topology suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveSuffixStatus {
    /// Exactly one canonical suffix boundary exists.
    Unique,
    /// Multiple canonical suffix boundaries exist; connectivity is withheld.
    #[allow(dead_code)]
    // Parser currently emits Unique only; Ambiguous is the withheld-connectivity state for multiple suffix boundaries.
    Ambiguous {
        /// Number of byte-valid suffix boundaries.
        candidate_count: usize,
    },
}

/// Bounded analytic parameter body from one positional `crv_array` row.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveParameterRecord {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Raw curve-family discriminator.
    pub type_byte: u8,
    /// Exact bytes between direction flags and the selected suffix boundary.
    pub body: Vec<u8>,
    /// Decoded scalar values in byte order.
    pub scalar_values: Vec<f64>,
    /// Scalar tokens with exact body-relative spans.
    pub scalar_tokens: Vec<CurveParameterScalar>,
    /// Canonical entity references skipped while walking the scalar lane.
    pub skipped_references: Vec<u32>,
    /// Canonical entity references with exact body-relative spans.
    pub references: Vec<CurveParameterReference>,
    /// Maximal byte spans not claimed by scalar or reference tokens.
    pub opaque_spans: Vec<CurveParameterOpaqueSpan>,
    /// Positional `ref_geom[0]` and `ref_geom[1]` values following the four
    /// topology references.
    pub reference_geometry: [u32; 2],
    /// Whether the topology suffix boundary is unique.
    pub suffix: CurveSuffixStatus,
    /// Byte offset of the positional row in the original stream.
    pub offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub body_offset: usize,
    /// Byte offset of the selected body/suffix boundary in the original stream.
    pub suffix_offset: usize,
}

/// One decoded scalar token in a positional curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveParameterScalar {
    /// Decoded scalar value.
    pub value: f64,
    /// Exact token bytes.
    pub raw: Vec<u8>,
    /// Body-relative token offset.
    pub offset: usize,
    /// Token length in bytes.
    pub length: usize,
}

/// One canonical entity reference in a positional curve body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveParameterReference {
    /// Referenced entity identifier.
    pub entity_id: u32,
    /// Body-relative reference-token offset, including `f7`.
    pub offset: usize,
    /// Reference-token length in bytes, including `f7`.
    pub length: usize,
}

/// One maximal unclaimed byte span in a positional curve body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveParameterOpaqueSpan {
    /// Exact unclaimed bytes.
    pub raw: Vec<u8>,
    /// Body-relative span offset.
    pub offset: usize,
    /// Span length in bytes.
    pub length: usize,
}

/// Two pcurve endpoints represented in both adjacent face parameter frames.
#[derive(Debug, Clone, PartialEq)]
pub struct PcurveEndpoints {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers corresponding to face frames zero and one.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's local UV frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's local UV frame.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Ordered samples of one curve represented in both incident-face charts.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoChartPcurveSamples {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers in sample-chart order.
    pub faces: [u32; 2],
    /// Pointwise-corresponding `[F0(u, v), F1(u, v)]` chart samples.
    pub samples: Vec<[[f64; 2]; 2]>,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// One-sided endpoint path from the complete short fc 02 curve body.
///
/// The body carries one path in the first topology face's parameter chart;
/// the second face remains a carrier-only join. The retained terminal operand
/// is deliberately not interpreted by this record.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc02ShortPcurveEndpoints {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Adjacent surface identifiers from the topology row.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's parameter frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Ordered world-coordinate lane from an `fc <subtype>` dense curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct FcCurveCoordinates {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Byte following the `fc` body prefix.
    pub subtype: u8,
    /// Exact complete curve parameter body, including the `fc` prefix.
    pub body: Vec<u8>,
    /// Ordered exact world-coordinate values, in mm.
    pub values_mm: Vec<f64>,
    /// World-coordinate tokens with exact body-relative spans.
    pub tokens: Vec<FcCurveCoordinateToken>,
    /// Maximal body spans not owned by a recognized coordinate token.
    pub opaque_spans: Vec<FcCurveOpaqueSpan>,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// One recognized world-coordinate token in an `fc <subtype>` body.
#[derive(Debug, Clone, PartialEq)]
pub struct FcCurveCoordinateToken {
    /// Decoded model length in millimeters.
    pub value_mm: f64,
    /// Exact source bytes occupied by the token.
    pub raw: Vec<u8>,
    /// Token offset relative to the complete curve parameter body.
    pub offset: usize,
    /// Number of source bytes occupied by the token.
    pub length: usize,
}

/// One maximal unclaimed span in an `fc <subtype>` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcCurveOpaqueSpan {
    /// Exact source bytes in the span.
    pub raw: Vec<u8>,
    /// Span offset relative to the complete curve parameter body.
    pub offset: usize,
    /// Number of source bytes in the span.
    pub length: usize,
}

/// Circle proven by the decoded points of an `fc 05` curve body.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc05Circle {
    /// Owning curve identifier.
    pub curve_id: u32,
    /// Circle center in the FC row's in-plane coordinate frame.
    pub center_row_frame: [f64; 2],
    /// Exact radius in mm.
    pub radius_mm: f64,
    /// Unit radial direction from the fitted center to the first stored sample.
    pub sample_direction_row_frame: [f64; 2],
    /// Unit radial direction at stored curve parameter zero in the row's
    /// `(x, z)` frame.
    pub reference_direction_row_frame: Option<[f64; 2]>,
    /// Signed relation from stored parameter to row-frame polar angle.
    /// `1` increases polar angle and `-1` decreases it.
    pub parameter_sign: Option<i8>,
    /// Constant cap-plane ordinate when present in every point.
    pub cap_ordinate_row_frame: Option<f64>,
    /// Number of points participating in validation.
    pub point_count: usize,
    /// Maximum absolute radial residual.
    pub max_residual: f64,
    /// Whether stored parameters match angular deltas around the circle.
    pub angle_parameter_consistent: bool,
    /// Byte offset of the source positional curve row.
    pub offset: usize,
}

/// Two or more topology-bound `fc 05` cap circles that establish one native
/// cylinder's radius and row-frame axis line, but not its model-space frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Fc05CylinderCapPair {
    /// Cylinder surface identifier shared by every cap edge.
    pub surface_id: u32,
    /// Curve identifiers of the agreeing cap circles in source order.
    pub curve_ids: Vec<u32>,
    /// Plane surface identifier opposite the cylinder on each cap edge.
    pub cap_plane_ids: Vec<u32>,
    /// Cap ordinate aligned with each `curve_ids`/`cap_plane_ids` entry.
    pub curve_cap_ordinates_row_frame: Vec<f64>,
    /// Shared center in the owning feature's row frame.
    pub center_row_frame: [f64; 2],
    /// Shared exact radius in mm.
    pub radius_mm: f64,
    /// Unit radial direction at parameter zero in the row's `(x, z)` frame.
    pub reference_direction_row_frame: [f64; 2],
    /// Shared signed parameter-to-polar-angle relation.
    pub parameter_sign: i8,
    /// At least two distinct cap ordinates in the owning feature's row frame.
    pub cap_ordinates_row_frame: Vec<f64>,
    /// Byte offset of the first participating curve row.
    pub offset: usize,
}

/// Complete eight-slot pcurve endpoints from a labeled curve prototype.
#[derive(Debug, Clone, PartialEq)]
pub struct PrototypePcurveEndpoints {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Endpoint A then B in schema face frame zero.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in schema face frame one.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the `crv_pnt_arr` label in the original stream.
    pub offset: usize,
}

/// Four labeled topology references of a curve prototype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurvePrototypeTopology {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Adjacent surface identifiers from `crv_hdr_geom_ptr[0/1]`.
    pub faces: [u32; 2],
    /// Per-face successor curve identifiers from `next_crv_hdr_ptr[0/1]`.
    pub next_edges: [u32; 2],
    /// Byte offset of the prototype namespace.
    pub offset: usize,
}

/// Prototype pcurve endpoints bound to their two labeled adjacent faces.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundPrototypePcurve {
    /// Prototype curve identifier.
    pub curve_id: u32,
    /// Adjacent face identifiers corresponding to UV frames zero and one.
    pub faces: [u32; 2],
    /// Endpoint A then B in the first face's UV frame.
    pub face_0_endpoints: [[f64; 2]; 2],
    /// Endpoint A then B in the second face's UV frame.
    pub face_1_endpoints: [[f64; 2]; 2],
    /// Byte offset of the source prototype pcurve.
    pub offset: usize,
}

/// Discover every labeled `crv_array` prototype. A label range ends at the
/// following `crv_array` label, so DEPDB-concatenated namespaces remain
/// independent.
pub fn prototypes(payload: &[u8]) -> Vec<CurvePrototype> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(relative) = find(payload, b"crv_array\0", start) {
        let section_start = relative;
        start = relative + b"crv_array\0".len();
        let section_end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        let Some(id_label) = find_in(payload, b"crv_id\0", start, section_end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (id, id_end) = compact_int(payload, id_start);
        if id_end == id_start {
            continue;
        }
        let Some(type_label) = find_in(payload, b"type\0", id_end, section_end) else {
            continue;
        };
        let Some(&type_byte) = payload.get(type_label + b"type\0".len()) else {
            continue;
        };
        let feature_id = find_in(payload, b"feat_id\0", id_end, section_end).and_then(|label| {
            let value_start = label + b"feat_id\0".len();
            let (value, end) = compact_int(payload, value_start);
            (end != value_start).then_some(value)
        });
        let directions =
            find_in(payload, b"crv_pnt_dir\0", id_end, section_end).and_then(|label| {
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
        result.push(CurvePrototype {
            id,
            type_byte,
            feature_id,
            directions,
            offset: section_start,
        });
    }
    result
}

/// Promote a uniquely referenced named-prototype topology record to a native
/// half-edge row when its positional successor references the prototype ID.
///
/// A named prototype is a schema record by default. A successor reference is
/// the byte-backed evidence that the prototype also supplies an edge identity
/// in the enclosing topology graph. The promotion remains withheld when the
/// prototype, topology record, or face namespace is ambiguous.
pub fn prototype_topology_rows(
    prototypes: &[CurvePrototype],
    prototype_topology: &[CurvePrototypeTopology],
    positional_rows: &[CurveTopologyRow],
    face_ids: &BTreeSet<u32>,
) -> Vec<CurveTopologyRow> {
    let mut prototype_counts = BTreeMap::<u32, usize>::new();
    for prototype in prototypes {
        *prototype_counts.entry(prototype.id).or_default() += 1;
    }
    let mut topology_counts = BTreeMap::<u32, usize>::new();
    for topology in prototype_topology {
        *topology_counts.entry(topology.curve_id).or_default() += 1;
    }
    let positional_ids = positional_rows
        .iter()
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    let referenced_ids = positional_rows
        .iter()
        .flat_map(|row| row.next_edges)
        .chain(prototype_topology.iter().flat_map(|row| row.next_edges))
        .filter(|id| *id != 0)
        .collect::<BTreeSet<_>>();
    let mut rows = Vec::new();
    for topology in prototype_topology {
        if positional_ids.contains(&topology.curve_id)
            || prototype_counts.get(&topology.curve_id) != Some(&1)
            || topology_counts.get(&topology.curve_id) != Some(&1)
            || !referenced_ids.contains(&topology.curve_id)
            || !topology
                .faces
                .iter()
                .all(|face_id| *face_id == 0 || face_ids.contains(face_id))
        {
            continue;
        }
        let Some(prototype) = prototypes
            .iter()
            .find(|prototype| prototype.id == topology.curve_id)
        else {
            continue;
        };
        let Some(directions) = prototype.directions else {
            continue;
        };
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
    rows.sort_by_key(|row| row.offset);
    rows
}

/// Decode bounded curve-from-equation expression programs.
pub fn expression_records(payload: &[u8]) -> Vec<CurveExpressionRecord> {
    expression_records_with_model_name(payload, None)
}

/// Decode curve-expression programs with an unambiguous current-model name.
pub(crate) fn expression_records_with_model_name(
    payload: &[u8],
    model_name: Option<&str>,
) -> Vec<CurveExpressionRecord> {
    const PRIMARY: &[u8] = b"entity(crv_fr_eqn)\0";
    const BACKUP: &[u8] = b"backup_ents(crv_fr_eqn)\0";
    const ID: &[u8] = b"\xe0\x01id\0";
    const EXPRESSION: &[u8] = b"\xe0\x0aexpression\0";
    const LOCAL_SYSTEM: &[u8] = b"\xe0\x02local_sys\0\xf9";

    let mut labels = Vec::new();
    for (label, backup) in [(PRIMARY, false), (BACKUP, true)] {
        let mut start = 0;
        while let Some(offset) = find(payload, label, start) {
            labels.push((offset, label.len(), backup));
            start = offset + label.len();
        }
    }
    labels.sort_unstable_by_key(|(offset, _, _)| *offset);

    let cache = scalar::ScalarCache::from_section(payload);
    let mut records = Vec::new();
    for (index, &(offset, label_len, backup)) in labels.iter().enumerate() {
        let end = labels
            .get(index + 1)
            .map_or(payload.len(), |(next, _, _)| *next);
        let Some(id_label) = find_in(payload, ID, offset + label_len, end) else {
            continue;
        };
        let id_start = id_label + ID.len();
        let (entity_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let local_system = find_in(payload, LOCAL_SYSTEM, after_id, end).and_then(|offset| {
            let extents_start = offset + LOCAL_SYSTEM.len();
            let (dimensions, dimensions_end) = compact_int(payload, extents_start);
            let (count, body_start) = compact_int(payload, dimensions_end);
            (dimensions_end > extents_start && body_start > dimensions_end && body_start <= end)
                .then_some(())?;
            let body_end = payload[body_start..end]
                .windows(1)
                .position(|window| window[0] == psb::token::NAMED_RECORD)
                .map_or(end, |relative| body_start + relative);
            let body = payload[body_start..body_end].to_vec();
            Some(CurveExpressionLocalSystem {
                dimensions,
                count,
                explicit_slots: ((dimensions, count) == (4, 3))
                    .then(|| scalar::decode_explicit_local_system_slots(&body, &cache))
                    .flatten(),
                body,
                offset,
            })
        });
        let Some(expression_offset) = find_in(payload, EXPRESSION, after_id, end) else {
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
        let mut lines = Vec::new();
        for _ in 0..count {
            let Some(relative_end) = payload[cursor..end].iter().position(|byte| *byte == 0) else {
                lines.clear();
                break;
            };
            let line_end = cursor + relative_end;
            let Ok(text) = std::str::from_utf8(&payload[cursor..line_end]) else {
                lines.clear();
                break;
            };
            lines.push(CurveExpressionLine {
                text: text.to_owned(),
                offset: cursor,
            });
            cursor = line_end + 1;
        }
        if lines.len() == usize::try_from(count).unwrap_or(usize::MAX) {
            let prohibited_constructs = curve_equation_prohibited_constructs(&lines);
            let mut solve_program = curve_expression_solve_program(&lines);
            let mut evaluation = evaluate_expression_program_details(
                &lines,
                model_name,
                &ExternalRelationSymbols::default(),
            );
            if !prohibited_constructs.is_empty() || solve_program.unresolved_control {
                for assignment in &mut evaluation.assignments {
                    assignment.value = None;
                }
                evaluation.solve_solutions.clear();
            }
            if !synchronize_solve_blocks(
                &mut solve_program.blocks,
                &evaluation.assignments,
                &evaluation.solve_solutions,
            ) {
                continue;
            }
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
    records
}

pub(crate) fn reevaluate_expression_records(
    records: &mut [CurveExpressionRecord],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) {
    for record in records {
        let mut evaluation =
            evaluate_expression_program_details(&record.lines, model_name, external_symbols);
        if !record.prohibited_constructs.is_empty() || record.unresolved_solve_control {
            for assignment in &mut evaluation.assignments {
                assignment.value = None;
            }
            evaluation.solve_solutions.clear();
        }
        if !synchronize_solve_blocks(
            &mut record.solve_blocks,
            &evaluation.assignments,
            &evaluation.solve_solutions,
        ) {
            for block in &mut record.solve_blocks {
                block.solutions.clear();
            }
        }
        record.assignments = evaluation.assignments;
    }
}

fn synchronize_solve_blocks(
    blocks: &mut [CurveExpressionSolveBlock],
    assignments: &[CurveExpressionAssignment],
    solutions: &BTreeMap<usize, Vec<CurveExpressionValue>>,
) -> bool {
    for block in blocks {
        for assignment in &mut block.assignments {
            if let Some(evaluated) = assignments
                .iter()
                .find(|evaluated| evaluated.offset == assignment.offset)
            {
                *assignment = evaluated.clone();
            }
        }
        let Ok(empty_solutions) = alloc_filled(
            block.variables.len(),
            None,
            "creo curve-expression solve solutions",
        ) else {
            return false;
        };
        block.solutions = solutions
            .get(&block.offset)
            .map_or(empty_solutions, |values| {
                values.iter().cloned().map(Some).collect()
            });
        if block.solutions.len() != block.variables.len() {
            let Ok(empty_solutions) = alloc_filled(
                block.variables.len(),
                None,
                "creo curve-expression solve solutions",
            ) else {
                return false;
            };
            block.solutions = empty_solutions;
        }
    }
    true
}

fn curve_equation_prohibited_constructs(lines: &[CurveExpressionLine]) -> Vec<String> {
    const PROHIBITED_FUNCTIONS: &[&str] =
        &["abs", "ceil", "floor", "extract", "if", "itos", "search"];
    let mut prohibited = BTreeSet::new();
    for line in lines {
        let source = line.text.trim();
        if source.starts_with("/*") {
            continue;
        }
        for keyword in ["if", "else", "endif"] {
            if starts_relation_keyword(source, keyword) {
                prohibited.insert(keyword.to_string());
            }
        }
        let bytes = source.as_bytes();
        let mut cursor = 0;
        while cursor < bytes.len() {
            if matches!(bytes[cursor], b'\'' | b'"') {
                let delimiter = bytes[cursor];
                cursor += 1;
                while bytes.get(cursor).is_some_and(|byte| *byte != delimiter) {
                    cursor += 1;
                }
                cursor += usize::from(bytes.get(cursor) == Some(&delimiter));
                continue;
            }
            if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
                let start = cursor;
                let Some(end) = expression_identifier_end(bytes, start) else {
                    cursor += 1;
                    continue;
                };
                cursor = end;
                let mut following = cursor;
                while bytes.get(following).is_some_and(u8::is_ascii_whitespace) {
                    following += 1;
                }
                let name = &source[start..end];
                if bytes.get(following) == Some(&b'(')
                    && PROHIBITED_FUNCTIONS
                        .iter()
                        .any(|candidate| name.eq_ignore_ascii_case(candidate))
                {
                    prohibited.insert(name.to_ascii_lowercase());
                }
                continue;
            }
            cursor += 1;
        }
    }
    prohibited.into_iter().collect()
}

#[derive(Default)]
pub(crate) struct ExternalRelationSymbols {
    values: BTreeMap<String, Option<CurveExpressionValue>>,
}

impl ExternalRelationSymbols {
    pub(crate) fn observe(&mut self, name: &str, value: Option<CurveExpressionValue>) {
        use std::collections::btree_map::Entry;

        match self.values.entry(expression_identifier_key(name)) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(mut entry) if entry.get() != &value => {
                entry.insert(None);
            }
            Entry::Occupied(_) => {}
        }
    }
}

fn expression_assignment(line: &CurveExpressionLine) -> Option<CurveExpressionAssignment> {
    let source = line.text.trim();
    if source.starts_with("/*") {
        return None;
    }
    let (name, expression) = split_expression_assignment(source)?;
    let target = expression_assignment_target(name.trim())?;
    let expression = expression.trim();
    if expression.is_empty() {
        return None;
    }
    let mut dependencies = Vec::<String>::new();
    if let CurveExpressionTarget::TableCell {
        parameter,
        row,
        column,
    } = &target
    {
        dependencies.push(parameter.clone());
        extend_expression_dependencies(&mut dependencies, row)?;
        if let Some(column) = column {
            extend_expression_dependencies(&mut dependencies, column)?;
        }
    } else if let CurveExpressionTarget::FunctionWrite { arguments, .. } = &target {
        for argument in arguments {
            extend_expression_dependencies(&mut dependencies, argument)?;
        }
    }
    extend_expression_dependencies(&mut dependencies, expression)?;
    Some(CurveExpressionAssignment {
        target,
        expression: expression.to_owned(),
        dependencies,
        value: None,
        activation: CurveExpressionActivation::Active,
        offset: line.offset,
    })
}

fn extend_expression_dependencies(dependencies: &mut Vec<String>, expression: &str) -> Option<()> {
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\'' | b'"') {
            let delimiter = bytes[cursor];
            cursor += 1;
            while bytes.get(cursor).is_some_and(|byte| *byte != delimiter) {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&delimiter) {
                cursor += 1;
            }
        } else if bytes[cursor].is_ascii_digit()
            || (bytes[cursor] == b'.' && bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit))
        {
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
            {
                cursor += 1;
            }
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
                while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    cursor += 1;
                }
            }
        } else if bytes[cursor] == b'[' {
            if let Some(end) = bytes[cursor + 1..].iter().position(|byte| *byte == b']') {
                cursor += end + 2;
            } else {
                cursor += 1;
            }
        } else if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            cursor = expression_identifier_end(bytes, start)?;
            let dependency = &expression[start..cursor];
            let mut following = cursor;
            while bytes.get(following).is_some_and(u8::is_ascii_whitespace) {
                following += 1;
            }
            let function =
                bytes.get(following) == Some(&b'(') && creo_relation_function(dependency).is_some();
            let constant = reserved_relation_scalar(dependency).is_some();
            if !function
                && !constant
                && !dependencies
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(dependency))
            {
                dependencies.push(dependency.to_owned());
            }
        } else {
            cursor += 1;
        }
    }
    Some(())
}

fn split_expression_assignment(source: &str) -> Option<(&str, &str)> {
    let bytes = source.as_bytes();
    let mut nesting = 0usize;
    let mut delimiter = None;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some(quote) = delimiter {
            if bytes[cursor] == quote {
                delimiter = None;
            }
            cursor += 1;
            continue;
        }
        match bytes[cursor] {
            byte @ (b'\'' | b'"') => delimiter = Some(byte),
            b'(' => nesting = nesting.checked_add(1)?,
            b')' => nesting = nesting.checked_sub(1)?,
            b'=' if nesting == 0
                && !bytes
                    .get(..cursor)
                    .and_then(|prefix| prefix.last())
                    .is_some_and(|byte| matches!(byte, b'=' | b'!' | b'~' | b'<' | b'>'))
                && bytes.get(cursor + 1) != Some(&b'=') =>
            {
                return Some((source.get(..cursor)?, source.get(cursor + 1..)?));
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

#[derive(Default)]
struct CurveExpressionSolveProgram {
    blocks: Vec<CurveExpressionSolveBlock>,
    line_indices: BTreeSet<usize>,
    executable_line_indices: BTreeSet<usize>,
    unresolved_control: bool,
}

struct PendingCurveExpressionSolveBlock {
    statements: Vec<PendingCurveExpressionSolveStatement>,
    offset: usize,
    valid: bool,
}

struct PendingCurveExpressionSolveStatement {
    equation: CurveExpressionEquation,
    assignment: Option<CurveExpressionAssignment>,
    line_index: usize,
}

fn curve_expression_solve_program(lines: &[CurveExpressionLine]) -> CurveExpressionSolveProgram {
    let mut program = CurveExpressionSolveProgram::default();
    let mut pending = None::<PendingCurveExpressionSolveBlock>;
    for (index, line) in lines.iter().enumerate() {
        let source = line.text.trim();
        if pending.is_none() {
            if starts_relation_keyword(source, "solve") {
                program.line_indices.insert(index);
                pending = Some(PendingCurveExpressionSolveBlock {
                    statements: Vec::new(),
                    offset: line.offset,
                    valid: source.eq_ignore_ascii_case("solve"),
                });
            } else if starts_relation_keyword(source, "for") {
                program.line_indices.insert(index);
                program.unresolved_control = true;
            }
            continue;
        }

        program.line_indices.insert(index);
        if starts_relation_keyword(source, "solve") {
            program.unresolved_control = true;
            pending.as_mut().expect("pending solve block").valid = false;
            continue;
        }
        if starts_relation_keyword(source, "for") {
            let variables = conditional_keyword_expression(source, "for")
                .and_then(curve_expression_solve_variables);
            let mut block = pending.take().expect("pending solve block");
            let mut equations = Vec::new();
            let mut assignments = Vec::new();
            let mut assignment_line_indices = Vec::new();
            if let Some(variables) = &variables {
                for statement in block.statements {
                    if statement.equation.dependencies.iter().any(|dependency| {
                        variables
                            .iter()
                            .any(|variable| variable.eq_ignore_ascii_case(dependency))
                    }) {
                        equations.push(statement.equation);
                    } else if let Some(assignment) = statement.assignment {
                        assignment_line_indices.push(statement.line_index);
                        assignments.push(assignment);
                    } else {
                        block.valid = false;
                    }
                }
            }
            if let Some(variables) = variables.filter(|_| block.valid && !equations.is_empty()) {
                program
                    .executable_line_indices
                    .extend(assignment_line_indices);
                let Ok(solutions) = alloc_filled(
                    variables.len(),
                    None,
                    "creo curve-expression solve solutions",
                ) else {
                    program.unresolved_control = true;
                    continue;
                };
                program.blocks.push(CurveExpressionSolveBlock {
                    equations,
                    assignments,
                    solutions,
                    variables,
                    offset: block.offset,
                    for_offset: line.offset,
                });
            } else {
                program.unresolved_control = true;
            }
            continue;
        }
        if source.is_empty() || source.starts_with("/*") {
            continue;
        }
        let Some((left, right)) = split_expression_assignment(source) else {
            program.unresolved_control = true;
            pending.as_mut().expect("pending solve block").valid = false;
            continue;
        };
        let (left, right) = (left.trim(), right.trim());
        if left.is_empty() || right.is_empty() || split_expression_assignment(right).is_some() {
            program.unresolved_control = true;
            pending.as_mut().expect("pending solve block").valid = false;
            continue;
        }
        let mut dependencies = Vec::new();
        if extend_expression_dependencies(&mut dependencies, left).is_none()
            || extend_expression_dependencies(&mut dependencies, right).is_none()
        {
            program.unresolved_control = true;
            pending.as_mut().expect("pending solve block").valid = false;
            continue;
        }
        pending
            .as_mut()
            .expect("pending solve block")
            .statements
            .push(PendingCurveExpressionSolveStatement {
                equation: CurveExpressionEquation {
                    left: left.to_owned(),
                    right: right.to_owned(),
                    dependencies,
                    offset: line.offset,
                },
                assignment: expression_assignment(line),
                line_index: index,
            });
    }
    if pending.is_some() {
        program.unresolved_control = true;
    }
    program
}

fn curve_expression_solve_variables(source: &str) -> Option<Vec<String>> {
    let variables = source
        .split(|character: char| character == ',' || character.is_ascii_whitespace())
        .filter(|variable| !variable.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let keys = variables
        .iter()
        .map(|variable| expression_identifier_key(variable))
        .collect::<BTreeSet<_>>();
    (!variables.is_empty()
        && keys.len() == variables.len()
        && variables
            .iter()
            .all(|variable| valid_scoped_expression_identifier(variable)))
    .then_some(variables)
}

fn expression_assignment_target(source: &str) -> Option<CurveExpressionTarget> {
    if let Some((name, arguments)) = expression_target_function_call(source) {
        if name.eq_ignore_ascii_case("value") {
            let [parameter, row, rest @ ..] = arguments.as_slice() else {
                return None;
            };
            let column = match rest {
                [] => None,
                [column] => Some((*column).to_owned()),
                _ => return None,
            };
            valid_expression_identifier(parameter).then_some(())?;
            return Some(CurveExpressionTarget::TableCell {
                parameter: (*parameter).to_owned(),
                row: (*row).to_owned(),
                column,
            });
        }
        return Some(CurveExpressionTarget::FunctionWrite {
            name: name.to_owned(),
            arguments: arguments.into_iter().map(str::to_owned).collect(),
        });
    }
    let (name, declared_unit) = if source.ends_with(']') {
        let unit_start = source.rfind('[')?;
        let unit = source.get(unit_start + 1..source.len() - 1)?.trim();
        (!unit.is_empty()).then_some(())?;
        (source.get(..unit_start)?.trim_end(), Some(unit))
    } else {
        (source, None)
    };
    if name.contains(':') {
        declared_unit.is_none().then_some(())?;
        valid_scoped_expression_identifier(name).then(|| CurveExpressionTarget::ScopedSymbol {
            name: name.to_owned(),
        })
    } else if let Some(family) = expression_system_symbol_family(name) {
        declared_unit.is_none().then_some(())?;
        Some(CurveExpressionTarget::SystemSymbol {
            name: name.to_owned(),
            family,
        })
    } else {
        valid_expression_identifier(name).then(|| CurveExpressionTarget::Parameter {
            name: name.to_owned(),
            declared_unit: declared_unit.map(str::to_owned),
        })
    }
}

fn expression_target_function_call(source: &str) -> Option<(&str, Vec<&str>)> {
    let argument_start = source.find('(')?;
    source.ends_with(')').then_some(())?;
    let name = source.get(..argument_start)?.trim_end();
    valid_expression_identifier(name).then_some(())?;
    let body = source.get(argument_start + 1..source.len().checked_sub(1)?)?;
    let arguments = if body.trim().is_empty() {
        Vec::new()
    } else {
        split_assignment_target_arguments(body)?
    };
    Some((name, arguments))
}

fn valid_expression_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

fn valid_scoped_expression_identifier(name: &str) -> bool {
    expression_identifier_end(name.as_bytes(), 0) == Some(name.len())
}

fn expression_system_symbol_family(name: &str) -> Option<CurveExpressionSystemSymbolFamily> {
    let digit_start = name
        .bytes()
        .position(|byte| byte.is_ascii_digit())
        .filter(|digit_start| *digit_start != 0)?;
    name.get(digit_start..)?
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then_some(())?;
    match name.get(..digit_start)?.to_ascii_lowercase().as_str() {
        "d" => Some(CurveExpressionSystemSymbolFamily::Dimension),
        "sd" => Some(CurveExpressionSystemSymbolFamily::SectionDimension),
        "rd" => Some(CurveExpressionSystemSymbolFamily::ReferenceDimension),
        "rsd" => Some(CurveExpressionSystemSymbolFamily::SectionReferenceDimension),
        "kd" => Some(CurveExpressionSystemSymbolFamily::KnownDimension),
        "ad" => Some(CurveExpressionSystemSymbolFamily::DrivenDimension),
        "p" => Some(CurveExpressionSystemSymbolFamily::PatternCount),
        "tpm" | "tp" | "tm" => Some(CurveExpressionSystemSymbolFamily::Tolerance),
        _ => None,
    }
}

fn split_assignment_target_arguments(source: &str) -> Option<Vec<&str>> {
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut nesting = 0usize;
    let mut delimiter = None;
    for (offset, byte) in source.bytes().enumerate() {
        if let Some(quote) = delimiter {
            if byte == quote {
                delimiter = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => delimiter = Some(byte),
            b'(' => nesting = nesting.checked_add(1)?,
            b')' => nesting = nesting.checked_sub(1)?,
            b',' if nesting == 0 => {
                let argument = source.get(start..offset)?.trim();
                (!argument.is_empty()).then_some(())?;
                arguments.push(argument);
                start = offset + 1;
            }
            _ => {}
        }
    }
    (delimiter.is_none() && nesting == 0).then_some(())?;
    let argument = source.get(start..)?.trim();
    (!argument.is_empty()).then_some(())?;
    arguments.push(argument);
    Some(arguments)
}

pub(crate) fn expression_identifier_key(name: &str) -> String {
    name.to_ascii_lowercase()
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

fn expression_identifier_end(source: &[u8], start: usize) -> Option<usize> {
    source
        .get(start)
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic())
        .then_some(())?;
    let mut cursor = start + 1;
    while source
        .get(cursor)
        .is_some_and(|byte| *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit())
    {
        cursor += 1;
    }
    while source.get(cursor) == Some(&b':')
        && source.get(cursor + 1).is_some_and(|byte| {
            *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit()
        })
    {
        cursor += 2;
        while source.get(cursor).is_some_and(|byte| {
            *byte == b'_' || byte.is_ascii_alphabetic() || byte.is_ascii_digit()
        }) {
            cursor += 1;
        }
    }
    Some(cursor)
}

#[derive(Debug, Clone)]
struct ConditionalFrame {
    parent: CurveExpressionActivation,
    condition: Option<bool>,
}

fn conditional_keyword_expression<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    let source = source.trim();
    let prefix = source.get(..keyword.len())?;
    prefix.eq_ignore_ascii_case(keyword).then_some(())?;
    source
        .as_bytes()
        .get(keyword.len())
        .is_some_and(u8::is_ascii_whitespace)
        .then_some(())?;
    let expression = source.get(keyword.len()..)?.trim_start();
    (!expression.is_empty()).then_some(expression)
}

fn starts_relation_keyword(source: &str, keyword: &str) -> bool {
    let source = source.trim();
    source
        .get(..keyword.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(keyword))
        && source
            .as_bytes()
            .get(keyword.len())
            .is_none_or(u8::is_ascii_whitespace)
}

fn expression_program_control_is_valid(lines: &[CurveExpressionLine]) -> bool {
    let mut else_seen = Vec::new();
    for line in lines {
        let source = line.text.trim();
        if starts_relation_keyword(source, "if") {
            if conditional_keyword_expression(source, "if").is_none() {
                return false;
            }
            else_seen.push(false);
        } else if starts_relation_keyword(source, "else") {
            if !source.eq_ignore_ascii_case("else") {
                return false;
            }
            let Some(seen) = else_seen.last_mut() else {
                return false;
            };
            if *seen {
                return false;
            }
            *seen = true;
        } else if starts_relation_keyword(source, "endif")
            && (!source.eq_ignore_ascii_case("endif") || else_seen.pop().is_none())
        {
            return false;
        }
    }
    else_seen.is_empty()
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

fn evaluate_expression_program_details(
    lines: &[CurveExpressionLine],
    model_name: Option<&str>,
    external_symbols: &ExternalRelationSymbols,
) -> CurveExpressionEvaluation {
    let solve_program = curve_expression_solve_program(lines);
    let solve_line_is_executable = |index: &usize| {
        !solve_program.line_indices.contains(index)
            || solve_program.executable_line_indices.contains(index)
    };
    if !expression_program_control_is_valid(lines) {
        return CurveExpressionEvaluation {
            assignments: lines
                .iter()
                .enumerate()
                .filter(|(index, _)| solve_line_is_executable(index))
                .map(|(_, line)| line)
                .filter_map(expression_assignment)
                .map(|mut assignment| {
                    assignment.activation = CurveExpressionActivation::Conditional;
                    assignment
                })
                .collect(),
            solve_solutions: BTreeMap::new(),
        };
    }

    let mut existing_symbols = external_symbols
        .values
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    existing_symbols.extend(
        lines
            .iter()
            .enumerate()
            .filter(|(index, _)| solve_line_is_executable(index))
            .filter_map(|(_, line)| expression_assignment(line))
            .filter_map(|assignment| {
                assignment
                    .scalar_target()
                    .map(|(name, _)| expression_identifier_key(name))
            }),
    );
    existing_symbols.extend(
        solve_program
            .blocks
            .iter()
            .flat_map(|block| &block.variables)
            .map(|variable| expression_identifier_key(variable)),
    );
    let context = RelationEvaluationContext {
        model_name,
        existing_symbols: Some(&existing_symbols),
    };
    let mut values = external_symbols
        .values
        .iter()
        .filter_map(|(name, value)| value.clone().map(|value| (name.clone(), value)))
        .collect::<BTreeMap<_, _>>();
    let mut defined_symbols = external_symbols
        .values
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut stack = Vec::<ConditionalFrame>::new();
    let mut activity = CurveExpressionActivation::Active;
    let mut assignments = Vec::<CurveExpressionAssignment>::new();
    let mut solve_solutions = BTreeMap::new();
    let mut solve_block_dimensions = BTreeMap::new();
    let mut solve_block_initial_values = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(block) = solve_program
            .blocks
            .iter()
            .find(|block| block.offset == line.offset)
        {
            let dimensions = block
                .variables
                .iter()
                .map(|variable| {
                    values
                        .get(&expression_identifier_key(variable))
                        .and_then(quantity_parts_ref)
                        .map(|(_, dimension)| dimension)
                })
                .collect::<Vec<_>>();
            solve_block_dimensions.insert(block.offset, dimensions);
            solve_block_initial_values.insert(
                block.offset,
                block
                    .variables
                    .iter()
                    .map(|variable| values.get(&expression_identifier_key(variable)).cloned())
                    .collect::<Vec<_>>(),
            );
            for variable in &block.variables {
                let key = expression_identifier_key(variable);
                values.remove(&key);
                defined_symbols.insert(key.clone());
                for assignment in &mut assignments {
                    if assignment
                        .scalar_target()
                        .is_some_and(|(name, _)| expression_identifier_key(name) == key)
                    {
                        assignment.value = None;
                    }
                }
            }
        }
        if let Some(block) = solve_program
            .blocks
            .iter()
            .find(|block| block.for_offset == line.offset)
        {
            if let Some(solution) = solve_block_dimensions
                .get(&block.offset)
                .and_then(|dimensions| {
                    infer_solve_variable_dimensions(block, &values, dimensions, context)
                })
                .and_then(|dimensions| {
                    solve_affine_expression_block(block, &values, &dimensions, context)
                })
                .or_else(|| {
                    let dimensions = solve_block_dimensions.get(&block.offset)?;
                    let initial_values = solve_block_initial_values.get(&block.offset)?;
                    solve_nonlinear_expression_block(
                        block,
                        &values,
                        dimensions,
                        initial_values,
                        context,
                    )
                })
            {
                for (variable, value) in block.variables.iter().zip(&solution) {
                    let key = expression_identifier_key(variable);
                    values.insert(key.clone(), value.clone());
                    for assignment in &mut assignments {
                        if assignment
                            .scalar_target()
                            .is_some_and(|(name, _)| expression_identifier_key(name) == key)
                        {
                            assignment.value = Some(value.clone());
                        }
                    }
                }
                solve_solutions.insert(block.offset, solution);
            }
        }
        if !solve_line_is_executable(&index) {
            continue;
        }
        let source = line.text.trim();
        if let Some(condition_source) = conditional_keyword_expression(source, "if") {
            let condition = (activity == CurveExpressionActivation::Active)
                .then(|| evaluate_relation_expression(condition_source, &values, context))
                .flatten()
                .and_then(|value| value.truth());
            let parent = activity;
            activity = branch_activation(parent, condition, false);
            stack.push(ConditionalFrame { parent, condition });
            continue;
        }
        if source.eq_ignore_ascii_case("else") {
            let frame = stack.last().expect("validated conditional stack");
            activity = branch_activation(frame.parent, frame.condition, true);
            continue;
        }
        if source.eq_ignore_ascii_case("endif") {
            let frame = stack.pop().expect("validated conditional stack");
            activity = frame.parent;
            continue;
        }
        let Some(mut assignment) = expression_assignment(line) else {
            continue;
        };
        assignment.activation = activity;
        let Some((name, declared_unit)) = assignment
            .scalar_target()
            .map(|(name, unit)| (name.to_owned(), unit.map(str::to_owned)))
        else {
            assignments.push(assignment);
            continue;
        };
        let key = expression_identifier_key(&name);
        let declaration_is_valid = declared_unit.is_none() || !defined_symbols.contains(&key);
        defined_symbols.insert(key.clone());
        match activity {
            CurveExpressionActivation::Active => {
                assignment.value = declaration_is_valid
                    .then(|| evaluate_relation_expression(&assignment.expression, &values, context))
                    .flatten()
                    .and_then(|value| {
                        apply_declared_relation_unit(value, declared_unit.as_deref())
                    });
                if let Some(value) = assignment.value.clone() {
                    values.insert(key, value);
                } else {
                    values.remove(&key);
                }
            }
            CurveExpressionActivation::Inactive => {}
            CurveExpressionActivation::Conditional => {
                values.remove(&key);
            }
        }
        assignments.push(assignment);
    }
    CurveExpressionEvaluation {
        assignments,
        solve_solutions,
    }
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

fn relation_unit(source: &str) -> Option<RelationUnit> {
    let mut parser = RelationUnitParser {
        source: source.as_bytes(),
        cursor: 0,
        nesting: 0,
    };
    let unit = parser.expression()?;
    parser.whitespace();
    (parser.cursor == parser.source.len()).then_some(unit)
}

struct RelationUnitParser<'a> {
    source: &'a [u8],
    cursor: usize,
    nesting: usize,
}

impl RelationUnitParser<'_> {
    fn expression(&mut self) -> Option<RelationUnit> {
        let mut unit = self.power()?;
        loop {
            self.whitespace();
            let divide = match self.source.get(self.cursor) {
                Some(b'*') => false,
                Some(b'/') => true,
                _ => return Some(unit),
            };
            self.cursor += 1;
            unit = unit.combine(self.power()?, divide)?;
        }
    }

    fn power(&mut self) -> Option<RelationUnit> {
        let unit = self.primary()?;
        self.whitespace();
        if self.source.get(self.cursor) != Some(&b'^') {
            return Some(unit);
        }
        self.cursor += 1;
        self.whitespace();
        let negative = self.source.get(self.cursor) == Some(&b'-');
        if negative || self.source.get(self.cursor) == Some(&b'+') {
            self.cursor += 1;
        }
        let start = self.cursor;
        while self.source.get(self.cursor).is_some_and(u8::is_ascii_digit) {
            self.cursor += 1;
        }
        let magnitude = std::str::from_utf8(self.source.get(start..self.cursor)?)
            .ok()?
            .parse::<i16>()
            .ok()?;
        let exponent = if negative { -magnitude } else { magnitude };
        unit.power(i8::try_from(exponent).ok()?)
    }

    fn primary(&mut self) -> Option<RelationUnit> {
        self.whitespace();
        if self.source.get(self.cursor) == Some(&b'(') {
            (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
            self.cursor += 1;
            self.nesting += 1;
            let unit = self.expression()?;
            self.nesting -= 1;
            self.whitespace();
            (self.source.get(self.cursor) == Some(&b')')).then_some(())?;
            self.cursor += 1;
            return Some(unit);
        }
        let start = self.cursor;
        while self
            .source
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            self.cursor += 1;
        }
        let symbol = std::str::from_utf8(self.source.get(start..self.cursor)?).ok()?;
        relation_unit_symbol(symbol)
    }

    fn whitespace(&mut self) {
        while self
            .source
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }
}

fn relation_unit_symbol(symbol: &str) -> Option<RelationUnit> {
    let normalized = symbol.to_ascii_lowercase();
    let (scale, offset, dimension) = match normalized.as_str() {
        "k" => (1.0, 0.0, RelationDimension::TEMPERATURE),
        "c" => (1.0, 273.15, RelationDimension::TEMPERATURE),
        "f" => (
            5.0 / 9.0,
            459.67 * 5.0 / 9.0,
            RelationDimension::TEMPERATURE,
        ),
        "r" => (5.0 / 9.0, 0.0, RelationDimension::TEMPERATURE),
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
        "mm" => (1.0, RelationDimension::LENGTH),
        "cm" => (10.0, RelationDimension::LENGTH),
        "m" => (1_000.0, RelationDimension::LENGTH),
        "in" | "inch" => (25.4, RelationDimension::LENGTH),
        "ft" | "foot" => (304.8, RelationDimension::LENGTH),
        "micron" => (0.001, RelationDimension::LENGTH),
        "sq_mm" => (1.0, RelationDimension::LENGTH.scale(2)?),
        "sq_cm" => (100.0, RelationDimension::LENGTH.scale(2)?),
        "sq_m" => (1_000_000.0, RelationDimension::LENGTH.scale(2)?),
        "sq_in" => (645.16, RelationDimension::LENGTH.scale(2)?),
        "sq_ft" => (92_903.04, RelationDimension::LENGTH.scale(2)?),
        "cu_mm" => (1.0, RelationDimension::LENGTH.scale(3)?),
        "cu_cm" => (1_000.0, RelationDimension::LENGTH.scale(3)?),
        "cu_m" => (1_000_000_000.0, RelationDimension::LENGTH.scale(3)?),
        "cu_in" => (16_387.064, RelationDimension::LENGTH.scale(3)?),
        "cu_ft" => (28_316_846.592, RelationDimension::LENGTH.scale(3)?),
        "kg" => (1.0, RelationDimension::MASS),
        "g" => (0.001, RelationDimension::MASS),
        "mg" => (0.000_001, RelationDimension::MASS),
        "lb" | "lbm" => (0.453_592_37, RelationDimension::MASS),
        "slug" => (14.593_902_937_206_4, RelationDimension::MASS),
        "tonne" => (1_000.0, RelationDimension::MASS),
        "s" | "sec" | "second" => (1.0, RelationDimension::TIME),
        "msec" => (0.001, RelationDimension::TIME),
        "min" | "minute" => (60.0, RelationDimension::TIME),
        "hr" | "hour" => (3_600.0, RelationDimension::TIME),
        "day" => (86_400.0, RelationDimension::TIME),
        "deg" | "degree" => (1.0, RelationDimension::ANGLE),
        "rad" | "radian" => (180.0 / std::f64::consts::PI, RelationDimension::ANGLE),
        "n" | "newton" => (1_000.0, RelationDimension::FORCE),
        "kn" => (1_000_000.0, RelationDimension::FORCE),
        "dyne" => (0.01, RelationDimension::FORCE),
        "lbf" => (4_448.221_615_260_5, RelationDimension::FORCE),
        "ton" => (9_806_650.0, RelationDimension::FORCE),
        "erg" => (
            0.1,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH, false)?,
        ),
        "joule" => (
            1_000_000.0,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH, false)?,
        ),
        "kw" => (
            1_000_000_000.0,
            RelationDimension::FORCE
                .combine(RelationDimension::LENGTH, false)?
                .combine(RelationDimension::TIME, true)?,
        ),
        "mw" => (
            1_000_000_000_000.0,
            RelationDimension::FORCE
                .combine(RelationDimension::LENGTH, false)?
                .combine(RelationDimension::TIME, true)?,
        ),
        "pa" => (
            0.001,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH.scale(2)?, true)?,
        ),
        "mpa" => (
            1_000.0,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH.scale(2)?, true)?,
        ),
        "gpa" => (
            1_000_000.0,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH.scale(2)?, true)?,
        ),
        "psi" => (
            6.894_757_293_168_361,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH.scale(2)?, true)?,
        ),
        "ksi" => (
            6_894.757_293_168_361,
            RelationDimension::FORCE.combine(RelationDimension::LENGTH.scale(2)?, true)?,
        ),
        _ => return None,
    })
}

trait ExpressionValue: Clone {
    fn number(value: f64) -> Self;
    fn reserved(name: &str) -> Option<Self> {
        reserved_relation_scalar(name).map(Self::number)
    }
    fn string(_value: String) -> Option<Self> {
        None
    }
    fn with_unit(self, unit: RelationUnit) -> Option<Self>;
    fn add(self, right: Self) -> Option<Self>;
    fn subtract(self, right: Self) -> Option<Self>;
    fn multiply(self, right: Self) -> Option<Self>;
    fn divide(self, right: Self) -> Option<Self>;
    fn power(self, right: Self) -> Option<Self>;
    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self>;
    fn logical_and(self, right: Self) -> Option<Self>;
    fn logical_or(self, right: Self) -> Option<Self>;
    fn logical_not(self) -> Option<Self>;
    fn function(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
    ) -> Option<Self>;
    fn negate(self) -> Option<Self>;
    fn finite(&self) -> bool;
}

impl ExpressionValue for f64 {
    fn number(value: f64) -> Self {
        value
    }

    fn add(self, right: Self) -> Option<Self> {
        Some(self + right)
    }

    fn with_unit(self, unit: RelationUnit) -> Option<Self> {
        Some(self * unit.scale + unit.offset)
    }

    fn subtract(self, right: Self) -> Option<Self> {
        Some(self - right)
    }

    fn multiply(self, right: Self) -> Option<Self> {
        Some(self * right)
    }

    fn divide(self, right: Self) -> Option<Self> {
        Some(self / right)
    }

    fn power(self, right: Self) -> Option<Self> {
        Some(self.powf(right))
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        Some(f64::from(operator.evaluate(self, right)))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        Some(f64::from(self != 0.0 && right != 0.0))
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        Some(f64::from(self != 0.0 || right != 0.0))
    }

    fn logical_not(self) -> Option<Self> {
        Some(f64::from(self == 0.0))
    }

    fn function(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        _context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        scope.is_none().then_some(())?;
        evaluate_creo_math_function(name, arguments)
    }

    fn negate(self) -> Option<Self> {
        Some(-self)
    }

    fn finite(&self) -> bool {
        self.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AffineValue {
    constant: f64,
    linear: f64,
}

impl ExpressionValue for AffineValue {
    fn number(value: f64) -> Self {
        Self {
            constant: value,
            linear: 0.0,
        }
    }

    fn add(self, right: Self) -> Option<Self> {
        Some(Self {
            constant: self.constant + right.constant,
            linear: self.linear + right.linear,
        })
    }

    fn with_unit(self, unit: RelationUnit) -> Option<Self> {
        Some(Self {
            constant: self.constant * unit.scale + unit.offset,
            linear: self.linear * unit.scale,
        })
    }

    fn subtract(self, right: Self) -> Option<Self> {
        Some(Self {
            constant: self.constant - right.constant,
            linear: self.linear - right.linear,
        })
    }

    fn multiply(self, right: Self) -> Option<Self> {
        (self.linear == 0.0 || right.linear == 0.0).then_some(Self {
            constant: self.constant * right.constant,
            linear: self.constant * right.linear + self.linear * right.constant,
        })
    }

    fn divide(self, right: Self) -> Option<Self> {
        (right.linear == 0.0 && right.constant != 0.0).then_some(Self {
            constant: self.constant / right.constant,
            linear: self.linear / right.constant,
        })
    }

    fn power(self, right: Self) -> Option<Self> {
        if right.linear == 0.0 && right.constant == 1.0 {
            return Some(self);
        }
        if right.linear == 0.0 && right.constant == 0.0 {
            return Some(Self::number(1.0));
        }
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| self.constant.powf(right.constant))
            .filter(|value| value.is_finite())
            .map(Self::number)
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(operator.evaluate(self.constant, right.constant))))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant != 0.0 && right.constant != 0.0)))
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        (self.linear == 0.0 && right.linear == 0.0)
            .then(|| Self::number(f64::from(self.constant != 0.0 || right.constant != 0.0)))
    }

    fn logical_not(self) -> Option<Self> {
        (self.linear == 0.0).then(|| Self::number(f64::from(self.constant == 0.0)))
    }

    fn function(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        _context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        scope.is_none().then_some(())?;
        let constants = arguments
            .iter()
            .map(|argument| (argument.linear == 0.0).then_some(argument.constant))
            .collect::<Option<Vec<_>>>()?;
        evaluate_creo_math_function(name, &constants).map(Self::number)
    }

    fn negate(self) -> Option<Self> {
        Some(Self {
            constant: -self.constant,
            linear: -self.linear,
        })
    }

    fn finite(&self) -> bool {
        self.constant.is_finite() && self.linear.is_finite()
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

    fn scale(mut self, factor: f64) -> Self {
        self.constant *= factor;
        for coefficient in self.coefficients.values_mut() {
            *coefficient *= factor;
        }
        self
    }

    fn combine(mut self, right: Self, subtract: bool) -> Option<Self> {
        (self.dimension == right.dimension).then_some(())?;
        let sign = if subtract { -1.0 } else { 1.0 };
        self.constant += sign * right.constant;
        for (variable, coefficient) in right.coefficients {
            let remove = {
                let value = self.coefficients.entry(variable.clone()).or_default();
                *value += sign * coefficient;
                *value == 0.0
            };
            if remove {
                self.coefficients.remove(&variable);
            }
        }
        Some(self)
    }

    fn as_curve_value(&self) -> Option<CurveExpressionValue> {
        self.coefficients
            .is_empty()
            .then(|| quantity_value(self.constant, self.dimension))
    }

    fn constant_difference(&self, right: &Self) -> Option<f64> {
        let difference = self.clone().combine(right.clone(), true)?;
        difference
            .coefficients
            .values()
            .all(|coefficient| *coefficient == 0.0)
            .then_some(difference.constant)
            .filter(|constant| constant.is_finite())
    }

    fn constant_truth(&self) -> Option<bool> {
        (self.dimension == RelationDimension::default() && self.coefficients.is_empty())
            .then_some(self.constant != 0.0)
    }
}

impl ExpressionValue for SimultaneousAffineValue {
    fn number(value: f64) -> Self {
        Self::constant(value, RelationDimension::default())
    }

    fn reserved(name: &str) -> Option<Self> {
        let value = CurveExpressionValue::reserved(name)?;
        let (value, dimension) = quantity_parts_ref(&value)?;
        Some(Self::constant(value, dimension))
    }

    fn with_unit(self, unit: RelationUnit) -> Option<Self> {
        (self.dimension == RelationDimension::default()).then_some(())?;
        let mut value = self.scale(unit.scale);
        value.dimension = unit.dimension;
        value.constant += unit.offset;
        Some(value)
    }

    fn add(self, right: Self) -> Option<Self> {
        self.combine(right, false)
    }

    fn subtract(self, right: Self) -> Option<Self> {
        self.combine(right, true)
    }

    fn multiply(self, right: Self) -> Option<Self> {
        if self.coefficients.is_empty() {
            let dimension = self.dimension.combine(right.dimension, false)?;
            let mut result = right.scale(self.constant);
            result.dimension = dimension;
            Some(result)
        } else if right.coefficients.is_empty() {
            let dimension = self.dimension.combine(right.dimension, false)?;
            let mut result = self.scale(right.constant);
            result.dimension = dimension;
            Some(result)
        } else {
            None
        }
    }

    fn divide(self, right: Self) -> Option<Self> {
        (right.coefficients.is_empty() && right.constant != 0.0).then_some(())?;
        let dimension = self.dimension.combine(right.dimension, true)?;
        let mut result = self.scale(1.0 / right.constant);
        result.dimension = dimension;
        Some(result)
    }

    fn power(self, right: Self) -> Option<Self> {
        if !right.coefficients.is_empty() || right.dimension != RelationDimension::default() {
            return None;
        }
        if right.constant == 1.0 {
            return Some(self);
        }
        if right.constant == 0.0 {
            return Some(Self::number(1.0));
        }
        let value = self.as_curve_value()?;
        let exponent = CurveExpressionValue::Number(right.constant);
        let result = value.power(exponent)?;
        let (value, dimension) = quantity_parts_ref(&result)?;
        value.is_finite().then(|| Self::constant(value, dimension))
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        let difference = self.constant_difference(&right)?;
        Some(Self::number(f64::from(operator.evaluate(difference, 0.0))))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        if self.constant_truth() == Some(false) || right.constant_truth() == Some(false) {
            return Some(Self::number(0.0));
        }
        let left = self.as_curve_value()?;
        let right = right.as_curve_value()?;
        let CurveExpressionValue::Number(value) = left.logical_and(right)? else {
            return None;
        };
        Some(Self::number(value))
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        if self.constant_truth() == Some(true) || right.constant_truth() == Some(true) {
            return Some(Self::number(1.0));
        }
        let left = self.as_curve_value()?;
        let right = right.as_curve_value()?;
        let CurveExpressionValue::Number(value) = left.logical_or(right)? else {
            return None;
        };
        Some(Self::number(value))
    }

    fn logical_not(self) -> Option<Self> {
        let value = self.as_curve_value()?;
        let CurveExpressionValue::Number(value) = value.logical_not()? else {
            return None;
        };
        Some(Self::number(value))
    }

    fn function(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        scope.is_none().then_some(())?;
        match (name, arguments) {
            (CreoMathFunction::If, [condition, when_true, when_false]) => {
                if when_true.constant_difference(when_false) == Some(0.0) {
                    return Some(when_true.clone());
                }
                let condition = condition.as_curve_value()?;
                let CurveExpressionValue::Number(condition) = condition else {
                    return None;
                };
                return Some(if condition == 0.0 {
                    when_false.clone()
                } else {
                    when_true.clone()
                });
            }
            (name @ (CreoMathFunction::Min | CreoMathFunction::Max), [left, right]) => {
                let difference = left.constant_difference(right)?;
                return Some(if extremum_selects_left(name, difference, 0.0)? {
                    left.clone()
                } else {
                    right.clone()
                });
            }
            (CreoMathFunction::Sign, [value, _])
                if value.coefficients.is_empty() && value.constant == 0.0 =>
            {
                return Some(value.clone());
            }
            (CreoMathFunction::Bound, [value, lower, upper]) => {
                (lower.constant_difference(upper)? < 0.0).then_some(())?;
                return Some(if value.constant_difference(lower)? < 0.0 {
                    lower.clone()
                } else if value.constant_difference(upper)? > 0.0 {
                    upper.clone()
                } else {
                    value.clone()
                });
            }
            (CreoMathFunction::Dead, [value, lower, upper]) => {
                (lower.constant_difference(upper)? <= 0.0).then_some(())?;
                let result = if value.constant_difference(lower)? < 0.0 {
                    value.clone().combine(lower.clone(), true)?
                } else if value.constant_difference(upper)? > 0.0 {
                    value.clone().combine(upper.clone(), true)?
                } else {
                    Self::constant(0.0, value.dimension)
                };
                return Some(result);
            }
            (CreoMathFunction::Near | CreoMathFunction::DblInTol, [left, right, tolerance]) => {
                let difference = left.constant_difference(right)?;
                let tolerance = tolerance.as_curve_value()?;
                let (tolerance, tolerance_dimension) = quantity_parts_ref(&tolerance)?;
                (left.dimension == tolerance_dimension && tolerance >= 0.0).then_some(())?;
                return Some(Self::number(f64::from(difference.abs() <= tolerance)));
            }
            (CreoMathFunction::Pow, [base, exponent]) => {
                return base.clone().power(exponent.clone());
            }
            _ => {}
        }
        let arguments = arguments
            .iter()
            .map(Self::as_curve_value)
            .collect::<Option<Vec<_>>>()?;
        let result = CurveExpressionValue::function(name, None, &arguments, context)?;
        let (value, dimension) = quantity_parts_ref(&result)?;
        Some(Self::constant(value, dimension))
    }

    fn negate(self) -> Option<Self> {
        Some(self.scale(-1.0))
    }

    fn finite(&self) -> bool {
        self.constant.is_finite() && self.coefficients.values().all(|value| value.is_finite())
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

    fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
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
    fn constant(value: i8) -> Self {
        Self {
            constant: DimensionRational::integer(value),
            variables: BTreeMap::new(),
        }
    }

    fn variable(name: &str) -> Self {
        Self {
            constant: DimensionRational::default(),
            variables: BTreeMap::from([(name.to_owned(), DimensionRational::one())]),
        }
    }

    fn combine(mut self, right: Self, subtract: bool) -> Option<Self> {
        self.constant = self.constant.combine(right.constant, subtract)?;
        for (name, coefficient) in right.variables {
            let is_zero = {
                let entry = self.variables.entry(name.clone()).or_default();
                *entry = (*entry).combine(coefficient, subtract)?;
                entry.is_zero()
            };
            if is_zero {
                self.variables.remove(&name);
            }
        }
        Some(self)
    }

    fn scale(mut self, factor: i8) -> Option<Self> {
        self.constant = self.constant.scale(factor)?;
        for coefficient in self.variables.values_mut() {
            *coefficient = (*coefficient).scale(factor)?;
        }
        self.variables
            .retain(|_, coefficient| !coefficient.is_zero());
        Some(self)
    }

    fn divide_exact(mut self, divisor: i16) -> Option<Self> {
        self.constant = self.constant.divide(divisor)?;
        for coefficient in self.variables.values_mut() {
            *coefficient = (*coefficient).divide(divisor)?;
        }
        self.variables
            .retain(|_, coefficient| !coefficient.is_zero());
        Some(self)
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

    fn variable(name: &str) -> Self {
        Self {
            axes: std::array::from_fn(|axis| {
                DimensionForm::variable(&dimension_variable_key(name, axis))
            }),
        }
    }

    fn combine(self, right: Self, subtract: bool) -> Option<Self> {
        let mut axes = std::array::from_fn(|_| DimensionForm::default());
        for (axis, (left, right)) in self.axes.into_iter().zip(right.axes).enumerate() {
            axes[axis] = left.combine(right, subtract)?;
        }
        Some(Self { axes })
    }

    fn scale(self, factor: i8) -> Option<Self> {
        Some(Self {
            axes: self
                .axes
                .into_iter()
                .map(|axis| axis.scale(factor))
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?,
        })
    }

    fn root(self, degree: i16) -> Option<Self> {
        (degree > 0).then_some(())?;
        Some(Self {
            axes: self
                .axes
                .into_iter()
                .map(|axis| axis.divide_exact(degree))
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?,
        })
    }

    fn is_zero(&self) -> bool {
        self.axes.iter().all(DimensionForm::is_zero)
    }
}

fn dimension_variable_key(name: &str, axis: usize) -> String {
    format!("{name}#{axis}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DimensionEquality {
    left: SymbolicRelationDimension,
    right: SymbolicRelationDimension,
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

    fn variable(name: &str) -> Self {
        Self {
            dimension: SymbolicRelationDimension::variable(name),
            kind: DimensionProbeKind::Numeric(None),
            constraints: Vec::new(),
        }
    }

    fn from_relation_value(value: &CurveExpressionValue) -> Option<Self> {
        match value {
            CurveExpressionValue::String(value) => Some(Self::text(Some(value.clone()))),
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

    fn with_constraint(
        mut self,
        left: SymbolicRelationDimension,
        right: SymbolicRelationDimension,
    ) -> Self {
        self.constraints.push(DimensionEquality { left, right });
        self
    }

    fn constrain_to(self, dimension: SymbolicRelationDimension) -> Self {
        let current = self.dimension.clone();
        self.with_constraint(current, dimension)
    }

    fn merge_constraints(left: &Self, right: &Self) -> Vec<DimensionEquality> {
        left.constraints
            .iter()
            .cloned()
            .chain(right.constraints.iter().cloned())
            .collect()
    }

    fn argument_constraints(arguments: &[Self]) -> Vec<DimensionEquality> {
        arguments
            .iter()
            .flat_map(|argument| argument.constraints.iter().cloned())
            .collect()
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
        let Some(values) = arguments
            .iter()
            .map(Self::numeric_value)
            .collect::<Option<Vec<_>>>()
        else {
            return Some(DimensionProbeNumber::Unknown);
        };
        evaluate_creo_math_function(name, &values).map(DimensionProbeNumber::Known)
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

impl ExpressionValue for DimensionProbeValue {
    fn number(value: f64) -> Self {
        Self::numeric(Some(value))
    }

    fn reserved(name: &str) -> Option<Self> {
        Self::from_relation_value(&CurveExpressionValue::reserved(name)?)
    }

    fn string(value: String) -> Option<Self> {
        Some(Self::text(Some(value)))
    }

    fn with_unit(self, unit: RelationUnit) -> Option<Self> {
        let Self {
            dimension,
            kind: DimensionProbeKind::Numeric(value),
            mut constraints,
        } = self
        else {
            return None;
        };
        constraints.push(DimensionEquality {
            left: dimension,
            right: SymbolicRelationDimension::default(),
        });
        let value = value
            .map(|value| value * unit.scale + unit.offset)
            .filter(|value| value.is_finite());
        Some(Self::numeric_result(
            SymbolicRelationDimension::from_relation_dimension(unit.dimension),
            value,
            constraints,
        ))
    }

    fn add(self, right: Self) -> Option<Self> {
        match (&self.kind, &right.kind) {
            (DimensionProbeKind::Text(left), DimensionProbeKind::Text(right_value)) => {
                let value = left
                    .as_ref()
                    .zip(right_value.as_ref())
                    .map(|(left, right)| {
                        let mut value = left.clone();
                        value.push_str(right);
                        value
                    });
                Some(Self::text_result(
                    value,
                    Self::merge_constraints(&self, &right),
                ))
            }
            (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) => {
                let mut constraints = Self::merge_constraints(&self, &right);
                constraints.push(DimensionEquality {
                    left: self.dimension.clone(),
                    right: right.dimension.clone(),
                });
                Some(Self::numeric_result(
                    self.dimension.clone(),
                    (*left).zip(*right_value).map(|(left, right)| left + right),
                    constraints,
                ))
            }
            _ => None,
        }
    }

    fn subtract(self, right: Self) -> Option<Self> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return None;
        };
        let mut constraints = Self::merge_constraints(&self, &right);
        constraints.push(DimensionEquality {
            left: self.dimension.clone(),
            right: right.dimension.clone(),
        });
        Some(Self::numeric_result(
            self.dimension.clone(),
            (*left).zip(*right_value).map(|(left, right)| left - right),
            constraints,
        ))
    }

    fn multiply(self, right: Self) -> Option<Self> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return None;
        };
        let constraints = Self::merge_constraints(&self, &right);
        let dimension = self
            .dimension
            .clone()
            .combine(right.dimension.clone(), false)?;
        Some(Self::numeric_result(
            dimension,
            (*left).zip(*right_value).map(|(left, right)| left * right),
            constraints,
        ))
    }

    fn divide(self, right: Self) -> Option<Self> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return None;
        };
        if right_value.is_some_and(|value| value == 0.0) {
            return None;
        }
        let constraints = Self::merge_constraints(&self, &right);
        let dimension = self
            .dimension
            .clone()
            .combine(right.dimension.clone(), true)?;
        Some(Self::numeric_result(
            dimension,
            (*left).zip(*right_value).map(|(left, right)| left / right),
            constraints,
        ))
    }

    fn power(self, right: Self) -> Option<Self> {
        let DimensionProbeKind::Numeric(exponent) = &right.kind else {
            return None;
        };
        let mut constraints = Self::merge_constraints(&self, &right);
        constraints.push(DimensionEquality {
            left: right.dimension.clone(),
            right: SymbolicRelationDimension::default(),
        });
        let base_dimension = self.dimension.clone();
        let value = self
            .numeric_value()
            .zip(*exponent)
            .map(|(value, exponent)| value.powf(exponent));
        let dimension = match exponent {
            Some(exponent) if exponent.fract() == 0.0 => {
                base_dimension.scale(i8::try_from(*exponent as i16).ok()?)?
            }
            Some(_) => base_dimension
                .is_zero()
                .then_some(SymbolicRelationDimension::default())?,
            None if base_dimension.is_zero() => SymbolicRelationDimension::default(),
            None => return None,
        };
        Some(Self::numeric_result(dimension, value, constraints))
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        match (&self.kind, &right.kind) {
            (DimensionProbeKind::Text(left), DimensionProbeKind::Text(right_value)) => {
                let value = match operator {
                    ComparisonOperator::Equal => left
                        .as_ref()
                        .zip(right_value.as_ref())
                        .map(|(left, right)| f64::from(left == right)),
                    ComparisonOperator::NotEqual => left
                        .as_ref()
                        .zip(right_value.as_ref())
                        .map(|(left, right)| f64::from(left != right)),
                    _ => return None,
                };
                Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    Self::merge_constraints(&self, &right),
                ))
            }
            (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) => {
                let mut constraints = Self::merge_constraints(&self, &right);
                constraints.push(DimensionEquality {
                    left: self.dimension.clone(),
                    right: right.dimension.clone(),
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    (*left)
                        .zip(*right_value)
                        .map(|(left, right)| f64::from(operator.evaluate(left, right))),
                    constraints,
                ))
            }
            _ => None,
        }
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return None;
        };
        let mut constraints = Self::merge_constraints(&self, &right);
        constraints.extend([
            DimensionEquality {
                left: self.dimension.clone(),
                right: SymbolicRelationDimension::default(),
            },
            DimensionEquality {
                left: right.dimension.clone(),
                right: SymbolicRelationDimension::default(),
            },
        ]);
        Some(Self::numeric_result(
            SymbolicRelationDimension::default(),
            (*left)
                .zip(*right_value)
                .map(|(left, right)| f64::from(left != 0.0 && right != 0.0)),
            constraints,
        ))
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        let (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right_value)) =
            (&self.kind, &right.kind)
        else {
            return None;
        };
        let mut constraints = Self::merge_constraints(&self, &right);
        constraints.extend([
            DimensionEquality {
                left: self.dimension.clone(),
                right: SymbolicRelationDimension::default(),
            },
            DimensionEquality {
                left: right.dimension.clone(),
                right: SymbolicRelationDimension::default(),
            },
        ]);
        Some(Self::numeric_result(
            SymbolicRelationDimension::default(),
            (*left)
                .zip(*right_value)
                .map(|(left, right)| f64::from(left != 0.0 || right != 0.0)),
            constraints,
        ))
    }

    fn logical_not(self) -> Option<Self> {
        let DimensionProbeKind::Numeric(value) = self.kind else {
            return None;
        };
        let mut constraints = self.constraints;
        constraints.push(DimensionEquality {
            left: self.dimension,
            right: SymbolicRelationDimension::default(),
        });
        Some(Self::numeric_result(
            SymbolicRelationDimension::default(),
            value.map(|value| f64::from(value == 0.0)),
            constraints,
        ))
    }

    fn function(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        scope.is_none().then_some(())?;
        let constraints = Self::argument_constraints(arguments);
        let numeric = |name| Self::optional_math(name, arguments);
        let numeric_args = |arguments: &[Self]| {
            arguments
                .iter()
                .map(Self::numeric_value)
                .collect::<Option<Vec<_>>>()
        };
        match (name, arguments) {
            (CreoMathFunction::Itos, [argument]) => {
                let value = argument.numeric_value().map(f64::round).map(|value| {
                    if value == 0.0 {
                        String::new()
                    } else {
                        format!("{value:.0}")
                    }
                });
                Some(Self::text_result(value, constraints))
            }
            (CreoMathFunction::Rtos, [argument, controls @ ..]) => {
                let mut constraints = constraints;
                let controls = controls
                    .iter()
                    .map(|control| {
                        let control = control
                            .clone()
                            .constrain_to(SymbolicRelationDimension::default());
                        constraints.extend(control.constraints.iter().cloned());
                        control.numeric_value()
                    })
                    .collect::<Option<Vec<_>>>()?;
                let (decimals, scientific) = match controls.as_slice() {
                    [] => (None, false),
                    [decimals] => (Some(relation_precision(*decimals)?), false),
                    [decimals, scientific] => {
                        (Some(relation_precision(*decimals)?), *scientific != 0.0)
                    }
                    _ => return None,
                };
                let value = match argument.numeric_value() {
                    Some(value) => Some(format_relation_real(value, decimals, scientific)?),
                    None => None,
                };
                Some(Self::text_result(value, constraints))
            }
            (CreoMathFunction::RelModelName, []) => Some(Self::text_result(
                context.model_name.map(str::to_owned),
                constraints,
            )),
            (CreoMathFunction::RelModelType, []) => {
                Some(Self::text_result(Some("part".to_owned()), constraints))
            }
            (CreoMathFunction::Exists, [argument]) => {
                let value = argument.text_value().and_then(|value| {
                    context
                        .existing_symbols?
                        .contains(&expression_identifier_key(value))
                        .then_some(1.0)
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    value,
                    constraints,
                ))
            }
            (CreoMathFunction::Search, [value, needle]) => Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(needle.text_value())
                    .map(|(value, needle)| {
                        value
                            .find(needle)
                            .map_or(0, |byte| value[..byte].chars().count() + 1)
                            as f64
                    }),
                constraints,
            )),
            (CreoMathFunction::Extract, [value, position, length]) => {
                let mut constraints = constraints;
                let position = position
                    .clone()
                    .constrain_to(SymbolicRelationDimension::default());
                constraints.extend(position.constraints.iter().cloned());
                let position = position.numeric_value();
                let length = length
                    .clone()
                    .constrain_to(SymbolicRelationDimension::default());
                constraints.extend(length.constraints.iter().cloned());
                let length = length.numeric_value();
                let extracted = value.text_value().zip(position).zip(length).map(
                    |((value, position), length)| {
                        if !position.is_finite()
                            || !length.is_finite()
                            || position.fract() != 0.0
                            || length.fract() != 0.0
                            || position <= 0.0
                            || length < 0.0
                        {
                            return None;
                        }
                        let character_count = value.chars().count();
                        if position > character_count as f64 {
                            Some(String::new())
                        } else {
                            let start = position as usize - 1;
                            let remaining = character_count - start;
                            let count = if length >= remaining as f64 {
                                remaining
                            } else {
                                length as usize
                            };
                            Some(value.chars().skip(start).take(count).collect())
                        }
                    },
                );
                let value = match extracted {
                    Some(Some(value)) => Some(value),
                    Some(None) => return None,
                    None => None,
                };
                Some(Self::text_result(value, constraints))
            }
            (CreoMathFunction::StringLength, [value]) => Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value.text_value().map(|value| value.chars().count() as f64),
                constraints,
            )),
            (CreoMathFunction::StringStarts, [value, prefix]) => Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(prefix.text_value())
                    .map(|(value, prefix)| f64::from(value.starts_with(prefix))),
                constraints,
            )),
            (CreoMathFunction::StringEnds, [value, suffix]) => Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(suffix.text_value())
                    .map(|(value, suffix)| f64::from(value.ends_with(suffix))),
                constraints,
            )),
            (CreoMathFunction::StringMatch, [value, expected]) => Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(expected.text_value())
                    .map(|(value, expected)| f64::from(value == expected)),
                constraints,
            )),
            (CreoMathFunction::StringPattern, [value, pattern]) => Some(Self::numeric_result(
                SymbolicRelationDimension::default(),
                value
                    .text_value()
                    .zip(pattern.text_value())
                    .and_then(|(value, pattern)| relation_string_pattern(value, pattern))
                    .map(f64::from),
                constraints,
            )),
            (
                name @ (CreoMathFunction::Sin | CreoMathFunction::Cos | CreoMathFunction::Tan),
                [argument],
            ) => {
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: argument.dimension.clone(),
                    right: SymbolicRelationDimension::from_relation_dimension(
                        RelationDimension::ANGLE,
                    ),
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    numeric(name)?.into_option(),
                    constraints,
                ))
            }
            (
                name @ (CreoMathFunction::Asin | CreoMathFunction::Acos | CreoMathFunction::Atan),
                [argument],
            ) => {
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: argument.dimension.clone(),
                    right: SymbolicRelationDimension::default(),
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::from_relation_dimension(RelationDimension::ANGLE),
                    numeric(name)?.into_option(),
                    constraints,
                ))
            }
            (CreoMathFunction::Atan2, [left, right]) => {
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: left.dimension.clone(),
                    right: right.dimension.clone(),
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::from_relation_dimension(RelationDimension::ANGLE),
                    numeric(CreoMathFunction::Atan2)?.into_option(),
                    constraints,
                ))
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
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: argument.dimension.clone(),
                    right: SymbolicRelationDimension::default(),
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    numeric(name)?.into_option(),
                    constraints,
                ))
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
                Some(Self::numeric_result(
                    value.dimension.clone(),
                    numeric_value,
                    constraints,
                ))
            }
            (CreoMathFunction::Mod, [left, right]) => {
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: left.dimension.clone(),
                    right: right.dimension.clone(),
                });
                let value = left.numeric_value().zip(right.numeric_value());
                if right.numeric_value().is_some_and(|value| value == 0.0) {
                    return None;
                }
                Some(Self::numeric_result(
                    left.dimension.clone(),
                    value.map(|(left, right)| left % right),
                    constraints,
                ))
            }
            (CreoMathFunction::If, [condition, when_true, when_false]) => {
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: condition.dimension.clone(),
                    right: SymbolicRelationDimension::default(),
                });
                match (&when_true.kind, &when_false.kind) {
                    (DimensionProbeKind::Text(left), DimensionProbeKind::Text(right)) => {
                        let value = condition
                            .numeric_value()
                            .zip(left.as_ref().zip(right.as_ref()))
                            .map(|(condition, (left, right))| {
                                if condition == 0.0 {
                                    right.clone()
                                } else {
                                    left.clone()
                                }
                            });
                        Some(Self::text_result(value, constraints))
                    }
                    (DimensionProbeKind::Numeric(left), DimensionProbeKind::Numeric(right)) => {
                        constraints.push(DimensionEquality {
                            left: when_true.dimension.clone(),
                            right: when_false.dimension.clone(),
                        });
                        let value = condition.numeric_value().zip(left.zip(*right)).map(
                            |(condition, (left, right))| {
                                if condition == 0.0 {
                                    right
                                } else {
                                    left
                                }
                            },
                        );
                        Some(Self::numeric_result(
                            when_true.dimension.clone(),
                            value,
                            constraints,
                        ))
                    }
                    _ => None,
                }
            }
            (CreoMathFunction::Bound, [value, lower, upper]) => {
                let mut constraints = constraints;
                constraints.extend([
                    DimensionEquality {
                        left: value.dimension.clone(),
                        right: lower.dimension.clone(),
                    },
                    DimensionEquality {
                        left: value.dimension.clone(),
                        right: upper.dimension.clone(),
                    },
                ]);
                let numeric = numeric_args(arguments).map(|values| {
                    let [value, lower, upper] = values.as_slice() else {
                        return None;
                    };
                    (lower < upper).then(|| value.clamp(*lower, *upper))
                });
                Some(Self::numeric_result(
                    arguments[0].dimension.clone(),
                    numeric.flatten(),
                    constraints,
                ))
            }
            (CreoMathFunction::Dead, [value, lower, upper]) => {
                let mut constraints = constraints;
                constraints.extend([
                    DimensionEquality {
                        left: value.dimension.clone(),
                        right: lower.dimension.clone(),
                    },
                    DimensionEquality {
                        left: value.dimension.clone(),
                        right: upper.dimension.clone(),
                    },
                ]);
                let numeric = numeric_args(arguments).map(|values| {
                    let [value, lower, upper] = values.as_slice() else {
                        return None;
                    };
                    (lower <= upper).then(|| {
                        if value < lower {
                            value - lower
                        } else if value > upper {
                            value - upper
                        } else {
                            0.0
                        }
                    })
                });
                Some(Self::numeric_result(
                    arguments[0].dimension.clone(),
                    numeric.flatten(),
                    constraints,
                ))
            }
            (CreoMathFunction::Near | CreoMathFunction::DblInTol, [left, right, tolerance]) => {
                let mut constraints = constraints;
                constraints.extend([
                    DimensionEquality {
                        left: left.dimension.clone(),
                        right: right.dimension.clone(),
                    },
                    DimensionEquality {
                        left: left.dimension.clone(),
                        right: tolerance.dimension.clone(),
                    },
                ]);
                let numeric = numeric_args(arguments).map(|values| {
                    let [left, right, tolerance] = values.as_slice() else {
                        return None;
                    };
                    (*tolerance >= 0.0).then(|| f64::from((*left - *right).abs() <= *tolerance))
                });
                Some(Self::numeric_result(
                    SymbolicRelationDimension::default(),
                    numeric.flatten(),
                    constraints,
                ))
            }
            (name @ (CreoMathFunction::Min | CreoMathFunction::Max), [left, right]) => {
                let mut constraints = constraints;
                constraints.push(DimensionEquality {
                    left: left.dimension.clone(),
                    right: right.dimension.clone(),
                });
                let numeric =
                    left.numeric_value()
                        .zip(right.numeric_value())
                        .map(|(left, right)| {
                            if extremum_selects_left(name, left, right).unwrap_or(false) {
                                left
                            } else {
                                right
                            }
                        });
                Some(Self::numeric_result(
                    left.dimension.clone(),
                    numeric,
                    constraints,
                ))
            }
            (CreoMathFunction::Pow, [base, exponent]) => base.clone().power(exponent.clone()),
            (CreoMathFunction::Sqrt, [argument]) => {
                let value = argument.numeric_value().map(f64::sqrt);
                Some(Self::numeric_result(
                    argument.dimension.clone().root(2)?,
                    value,
                    constraints,
                ))
            }
            (
                name @ (CreoMathFunction::Abs | CreoMathFunction::Ceil | CreoMathFunction::Floor),
                [argument],
            ) => {
                let value = match name {
                    CreoMathFunction::Abs => argument.numeric_value().map(f64::abs),
                    CreoMathFunction::Ceil => {
                        Self::optional_round(argument, None, true)?.into_option()
                    }
                    CreoMathFunction::Floor => {
                        Self::optional_round(argument, None, false)?.into_option()
                    }
                    _ => unreachable!(),
                };
                Some(Self::numeric_result(
                    argument.dimension.clone(),
                    value,
                    constraints,
                ))
            }
            (
                name @ (CreoMathFunction::Ceil | CreoMathFunction::Floor),
                [argument, decimal_places],
            ) => {
                let decimal_places = decimal_places
                    .clone()
                    .constrain_to(SymbolicRelationDimension::default());
                let mut constraints = constraints;
                constraints.extend(decimal_places.constraints.iter().cloned());
                let value = Self::optional_round(
                    argument,
                    Some(&decimal_places),
                    matches!(name, CreoMathFunction::Ceil),
                )?
                .into_option();
                Some(Self::numeric_result(
                    argument.dimension.clone(),
                    value,
                    constraints,
                ))
            }
            _ => None,
        }
    }

    fn negate(self) -> Option<Self> {
        let kind = match self.kind {
            DimensionProbeKind::Numeric(value) => DimensionProbeKind::Numeric(value.map(|v| -v)),
            DimensionProbeKind::Text(_) => return None,
        };
        Some(Self {
            dimension: self.dimension,
            kind,
            constraints: self.constraints,
        })
    }

    fn finite(&self) -> bool {
        match &self.kind {
            DimensionProbeKind::Numeric(Some(value)) => value.is_finite(),
            DimensionProbeKind::Numeric(None) | DimensionProbeKind::Text(_) => true,
        }
    }
}

impl ExpressionValue for CurveExpressionValue {
    fn number(value: f64) -> Self {
        Self::Number(value)
    }

    fn string(value: String) -> Option<Self> {
        Some(Self::String(value))
    }

    fn reserved(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("g") {
            Some(quantity_value(9_800.0, RelationDimension::ACCELERATION))
        } else {
            reserved_relation_scalar(name).map(Self::number)
        }
    }

    fn with_unit(self, unit: RelationUnit) -> Option<Self> {
        let Self::Number(value) = self else {
            return None;
        };
        Some(quantity_value(
            value * unit.scale + unit.offset,
            unit.dimension,
        ))
    }

    fn add(self, right: Self) -> Option<Self> {
        match (self, right) {
            (Self::String(mut left), Self::String(right)) => {
                left.push_str(&right);
                Some(Self::String(left))
            }
            (left, right) => quantity_additive(&left, &right, |left, right| left + right),
        }
    }

    fn subtract(self, right: Self) -> Option<Self> {
        quantity_additive(&self, &right, |left, right| left - right)
    }

    fn multiply(self, right: Self) -> Option<Self> {
        let (left, left_dimension) = quantity_parts_ref(&self)?;
        let (right, right_dimension) = quantity_parts_ref(&right)?;
        Some(quantity_value(
            left * right,
            left_dimension.combine(right_dimension, false)?,
        ))
    }

    fn divide(self, right: Self) -> Option<Self> {
        let (left, left_dimension) = quantity_parts_ref(&self)?;
        let (right, right_dimension) = quantity_parts_ref(&right)?;
        Some(quantity_value(
            left / right,
            left_dimension.combine(right_dimension, true)?,
        ))
    }

    fn power(self, right: Self) -> Option<Self> {
        let Self::Number(exponent) = right else {
            return None;
        };
        let (value, dimension) = quantity_parts_ref(&self)?;
        if dimension == RelationDimension::default() {
            return Some(Self::Number(value.powf(exponent)));
        }
        let integer = exponent.trunc();
        (integer == exponent).then_some(())?;
        let exponent = i8::try_from(integer as i16).ok()?;
        Some(quantity_value(
            value.powi(i32::from(exponent)),
            dimension.scale(exponent)?,
        ))
    }

    fn compare(self, right: Self, operator: ComparisonOperator) -> Option<Self> {
        let result = match (self, right) {
            (Self::Number(left), Self::Number(right)) => operator.evaluate(left, right),
            (Self::String(left), Self::String(right)) => match operator {
                ComparisonOperator::Equal => left == right,
                ComparisonOperator::NotEqual => left != right,
                _ => return None,
            },
            (left, right) => {
                let (left, left_dimension) = quantity_parts_ref(&left)?;
                let (right, right_dimension) = quantity_parts_ref(&right)?;
                (left_dimension == right_dimension).then_some(())?;
                operator.evaluate(left, right)
            }
        };
        Some(Self::Number(f64::from(result)))
    }

    fn logical_and(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| {
            f64::from(left != 0.0 && right != 0.0)
        })
    }

    fn logical_or(self, right: Self) -> Option<Self> {
        numeric_binary(self, right, |left, right| {
            f64::from(left != 0.0 || right != 0.0)
        })
    }

    fn logical_not(self) -> Option<Self> {
        let Self::Number(value) = self else {
            return None;
        };
        Some(Self::Number(f64::from(value == 0.0)))
    }

    fn function(
        name: CreoMathFunction,
        scope: Option<&str>,
        arguments: &[Self],
        context: RelationEvaluationContext<'_>,
    ) -> Option<Self> {
        scope.is_none().then_some(())?;
        evaluate_creo_relation_function(name, arguments, context)
    }

    fn negate(self) -> Option<Self> {
        match self {
            Self::Number(value) => Some(Self::Number(-value)),
            Self::Length(value) => Some(Self::Length(-value)),
            Self::Angle(value) => Some(Self::Angle(-value)),
            Self::Quantity(mut value) => {
                value.value = -value.value;
                Some(Self::Quantity(value))
            }
            Self::String(_) => None,
        }
    }

    fn finite(&self) -> bool {
        match self {
            Self::Number(value) | Self::Length(value) | Self::Angle(value) => value.is_finite(),
            Self::Quantity(value) => value.value.is_finite(),
            Self::String(_) => true,
        }
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
    Some(quantity_value(operation(left, right), left_dimension))
}

fn quantity_parts_ref(value: &CurveExpressionValue) -> Option<(f64, RelationDimension)> {
    match value {
        CurveExpressionValue::Number(value) => Some((*value, RelationDimension::default())),
        CurveExpressionValue::Length(value) => Some((*value, RelationDimension::LENGTH)),
        CurveExpressionValue::Angle(value) => Some((*value, RelationDimension::ANGLE)),
        CurveExpressionValue::Quantity(value) => Some((value.value, value.dimension())),
        CurveExpressionValue::String(_) => None,
    }
}

fn quantity_value(value: f64, dimension: RelationDimension) -> CurveExpressionValue {
    if dimension == RelationDimension::default() {
        CurveExpressionValue::Number(value)
    } else if dimension == RelationDimension::LENGTH {
        CurveExpressionValue::Length(value)
    } else if dimension == RelationDimension::ANGLE {
        CurveExpressionValue::Angle(value)
    } else {
        CurveExpressionValue::Quantity(CurveExpressionQuantity {
            value,
            length_power: dimension.length,
            mass_power: dimension.mass,
            time_power: dimension.time,
            angle_power: dimension.angle,
            temperature_power: dimension.temperature,
        })
    }
}

fn numeric_binary(
    left: CurveExpressionValue,
    right: CurveExpressionValue,
    operation: impl FnOnce(f64, f64) -> f64,
) -> Option<CurveExpressionValue> {
    match (left, right) {
        (CurveExpressionValue::Number(left), CurveExpressionValue::Number(right)) => {
            Some(CurveExpressionValue::Number(operation(left, right)))
        }
        _ => None,
    }
}

struct ExpressionParser<'a, V> {
    source: &'a [u8],
    cursor: usize,
    values: &'a BTreeMap<String, V>,
    context: RelationEvaluationContext<'a>,
    nesting: usize,
}

const MAX_EXPRESSION_NESTING: usize = 128;

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
    fn finite_value(value: V) -> Option<V> {
        value.finite().then_some(value)
    }

    fn whitespace(&mut self) {
        while self
            .source
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn logical_or(&mut self) -> Option<V> {
        let mut value = self.logical_and()?;
        loop {
            self.whitespace();
            if self.source.get(self.cursor) != Some(&b'|') {
                return Some(value);
            }
            self.cursor += 1;
            value = Self::finite_value(value.logical_or(self.logical_and()?)?)?;
        }
    }

    fn logical_and(&mut self) -> Option<V> {
        let mut value = self.comparison()?;
        loop {
            self.whitespace();
            if self.source.get(self.cursor) != Some(&b'&') {
                return Some(value);
            }
            self.cursor += 1;
            value = Self::finite_value(value.logical_and(self.comparison()?)?)?;
        }
    }

    fn comparison(&mut self) -> Option<V> {
        let value = self.expression()?;
        self.whitespace();
        let (operator, width) = match self.source.get(self.cursor..) {
            Some([b'=', b'=', ..]) => (ComparisonOperator::Equal, 2),
            Some([b'!' | b'~', b'=', ..] | [b'<', b'>', ..]) => (ComparisonOperator::NotEqual, 2),
            Some([b'>', b'=', ..]) => (ComparisonOperator::GreaterOrEqual, 2),
            Some([b'<', b'=', ..]) => (ComparisonOperator::LessOrEqual, 2),
            Some([b'>', ..]) => (ComparisonOperator::Greater, 1),
            Some([b'<', ..]) => (ComparisonOperator::Less, 1),
            _ => return Some(value),
        };
        self.cursor += width;
        Self::finite_value(value.compare(self.expression()?, operator)?)
    }

    fn expression(&mut self) -> Option<V> {
        let mut value = self.term()?;
        loop {
            self.whitespace();
            match self.source.get(self.cursor) {
                Some(b'+') => {
                    self.cursor += 1;
                    value = Self::finite_value(value.add(self.term()?)?)?;
                }
                Some(b'-') => {
                    self.cursor += 1;
                    value = Self::finite_value(value.subtract(self.term()?)?)?;
                }
                _ => return Some(value),
            }
        }
    }

    fn term(&mut self) -> Option<V> {
        let mut value = self.unary()?;
        loop {
            self.whitespace();
            match self.source.get(self.cursor) {
                Some(b'*') => {
                    self.cursor += 1;
                    value = Self::finite_value(value.multiply(self.unary()?)?)?;
                }
                Some(b'/') => {
                    self.cursor += 1;
                    value = Self::finite_value(value.divide(self.unary()?)?)?;
                }
                _ => return Some(value),
            }
        }
    }

    fn unary(&mut self) -> Option<V> {
        self.whitespace();
        let mut operators = Vec::new();
        loop {
            match self.source.get(self.cursor) {
                Some(b'+') => self.cursor += 1,
                Some(b'-') => {
                    operators.push(b'-');
                    self.cursor += 1;
                }
                Some(b'!' | b'~') => {
                    operators.push(b'!');
                    self.cursor += 1;
                }
                _ => break,
            }
            self.whitespace();
        }
        let mut value = self.power()?;
        for operator in operators.into_iter().rev() {
            value = Self::finite_value(if operator == b'-' {
                value.negate()?
            } else {
                value.logical_not()?
            })?;
        }
        Some(value)
    }

    fn power(&mut self) -> Option<V> {
        let value = self.primary()?;
        self.whitespace();
        if self.source.get(self.cursor) != Some(&b'^') {
            return Some(value);
        }
        (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
        self.cursor += 1;
        self.nesting += 1;
        let exponent = self.unary()?;
        self.nesting -= 1;
        Self::finite_value(value.power(exponent)?)
    }

    fn primary(&mut self) -> Option<V> {
        self.whitespace();
        let mut value = match self.source.get(self.cursor)? {
            b'(' => {
                (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
                self.cursor += 1;
                self.nesting += 1;
                let value = self.logical_or()?;
                self.nesting -= 1;
                self.whitespace();
                (self.source.get(self.cursor) == Some(&b')')).then(|| {
                    self.cursor += 1;
                    value
                })
            }
            byte if byte.is_ascii_digit() || *byte == b'.' => self.number(),
            b'\'' | b'"' => self.string(),
            byte if byte.is_ascii_alphabetic() || *byte == b'_' => self.identifier_or_function(),
            _ => None,
        }?;
        self.whitespace();
        if self.source.get(self.cursor) == Some(&b'[') {
            let unit_start = self.cursor + 1;
            let unit_length = self.source[unit_start..]
                .iter()
                .position(|byte| *byte == b']')?;
            let unit_end = unit_start + unit_length;
            let unit = std::str::from_utf8(&self.source[unit_start..unit_end]).ok()?;
            value = Self::finite_value(value.with_unit(relation_unit(unit)?)?)?;
            self.cursor = unit_end + 1;
        }
        Self::finite_value(value)
    }

    fn string(&mut self) -> Option<V> {
        let delimiter = *self.source.get(self.cursor)?;
        self.cursor += 1;
        let start = self.cursor;
        while self
            .source
            .get(self.cursor)
            .is_some_and(|byte| *byte != delimiter)
        {
            self.cursor += 1;
        }
        (self.source.get(self.cursor) == Some(&delimiter)).then_some(())?;
        let value = std::str::from_utf8(&self.source[start..self.cursor])
            .ok()?
            .to_owned();
        self.cursor += 1;
        V::string(value)
    }

    fn number(&mut self) -> Option<V> {
        let start = self.cursor;
        while self
            .source
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.cursor += 1;
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
            while self.source.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
        }
        let value = std::str::from_utf8(&self.source[start..self.cursor])
            .ok()?
            .parse()
            .ok()?;
        Some(V::number(value))
    }

    fn identifier_or_function(&mut self) -> Option<V> {
        let start = self.cursor;
        self.cursor = expression_identifier_end(self.source, start)?;
        let name = std::str::from_utf8(&self.source[start..self.cursor]).ok()?;
        self.whitespace();
        if self.source.get(self.cursor) != Some(&b'(') {
            if let Some(value) = V::reserved(name) {
                return Some(value);
            }
            return self.values.get(&expression_identifier_key(name)).cloned();
        }
        (self.nesting < MAX_EXPRESSION_NESTING).then_some(())?;
        let (function, scope) = creo_relation_function(name)?;
        self.cursor += 1;
        self.nesting += 1;
        self.whitespace();
        let mut arguments = Vec::new();
        if self.source.get(self.cursor) != Some(&b')') {
            loop {
                arguments.push(self.logical_or()?);
                self.whitespace();
                if self.source.get(self.cursor) != Some(&b',') {
                    break;
                }
                self.cursor += 1;
            }
        }
        self.whitespace();
        (self.source.get(self.cursor) == Some(&b')')).then_some(())?;
        self.cursor += 1;
        self.nesting -= 1;
        V::function(function, scope, &arguments, self.context)
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
    match name.to_ascii_lowercase().as_str() {
        "sin" => Some(CreoMathFunction::Sin),
        "cos" => Some(CreoMathFunction::Cos),
        "tan" => Some(CreoMathFunction::Tan),
        "asin" => Some(CreoMathFunction::Asin),
        "acos" => Some(CreoMathFunction::Acos),
        "atan" => Some(CreoMathFunction::Atan),
        "atan2" => Some(CreoMathFunction::Atan2),
        "sinh" => Some(CreoMathFunction::Sinh),
        "cosh" => Some(CreoMathFunction::Cosh),
        "tanh" => Some(CreoMathFunction::Tanh),
        "sign" => Some(CreoMathFunction::Sign),
        "mod" => Some(CreoMathFunction::Mod),
        "if" => Some(CreoMathFunction::If),
        "bound" => Some(CreoMathFunction::Bound),
        "dead" => Some(CreoMathFunction::Dead),
        "near" => Some(CreoMathFunction::Near),
        "min" => Some(CreoMathFunction::Min),
        "max" => Some(CreoMathFunction::Max),
        "log" => Some(CreoMathFunction::Log),
        "ln" => Some(CreoMathFunction::Ln),
        "exp" => Some(CreoMathFunction::Exp),
        "pow" => Some(CreoMathFunction::Pow),
        "sqrt" => Some(CreoMathFunction::Sqrt),
        "abs" => Some(CreoMathFunction::Abs),
        "ceil" => Some(CreoMathFunction::Ceil),
        "floor" => Some(CreoMathFunction::Floor),
        "dbl_in_tol" => Some(CreoMathFunction::DblInTol),
        "itos" => Some(CreoMathFunction::Itos),
        "rtos" => Some(CreoMathFunction::Rtos),
        "rel_model_name" => Some(CreoMathFunction::RelModelName),
        "rel_model_type" => Some(CreoMathFunction::RelModelType),
        "exists" => Some(CreoMathFunction::Exists),
        "search" => Some(CreoMathFunction::Search),
        "extract" => Some(CreoMathFunction::Extract),
        "string_length" => Some(CreoMathFunction::StringLength),
        "string_starts" => Some(CreoMathFunction::StringStarts),
        "string_ends" => Some(CreoMathFunction::StringEnds),
        "string_match" => Some(CreoMathFunction::StringMatch),
        "string_pattern" => Some(CreoMathFunction::StringPattern),
        "cable_len" | "cable_thick" | "cbl_logical_file" | "eang" | "elen" | "edistk"
        | "ecoordx" | "ecoordy" | "evalgraph" | "trajpar_of_pnt" | "massprop_param"
        | "material_param" | "mp_mass" | "mp_assigned_mass" | "mp_surf_area" | "mp_volume"
        | "mp_cg_x" | "mp_cg_y" | "mp_cg_z" | "has_value" | "match_value" | "average"
        | "value_by_argument" | "weighted_average" | "value" | "count_rows" => {
            Some(CreoMathFunction::ContextDependent)
        }
        _ => None,
    }
}

fn creo_relation_function(name: &str) -> Option<(CreoMathFunction, Option<&str>)> {
    if let Some((function, scope)) = name.split_once(':') {
        (function.eq_ignore_ascii_case("rel_model_name")
            && !scope.is_empty()
            && scope.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some((CreoMathFunction::RelModelName, Some(scope)))
    } else {
        creo_math_function(name).map(|function| (function, None))
    }
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
            ((x - y).abs() <= *delta) as u8 as f64
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
            ((first - second).abs() <= *tolerance) as u8 as f64
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
    (decimal_places >= i32::MIN as f64).then_some(())?;
    let scale = 10_f64.powi(decimal_places as i32);
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

fn evaluate_creo_relation_function(
    name: CreoMathFunction,
    arguments: &[CurveExpressionValue],
    context: RelationEvaluationContext<'_>,
) -> Option<CurveExpressionValue> {
    use CurveExpressionValue::{Angle, Length, Number, Quantity, String};
    let value = match (name, arguments) {
        (CreoMathFunction::Itos, [argument]) => {
            let (value, _) = quantity_parts_ref(argument)?;
            let rounded = value.round();
            if rounded == 0.0 {
                String(std::string::String::new())
            } else {
                String(format!("{rounded:.0}"))
            }
        }
        (CreoMathFunction::Rtos, [argument, controls @ ..]) => {
            let (value, _) = quantity_parts_ref(argument)?;
            let (decimals, scientific) = match controls {
                [] => (None, false),
                [Number(decimals)] => (Some(relation_precision(*decimals)?), false),
                [Number(decimals), Number(scientific)] => {
                    (Some(relation_precision(*decimals)?), *scientific != 0.0)
                }
                _ => return None,
            };
            String(format_relation_real(value, decimals, scientific)?)
        }
        (CreoMathFunction::RelModelName, []) => String(context.model_name?.to_owned()),
        (CreoMathFunction::RelModelType, []) => String("part".to_owned()),
        (CreoMathFunction::Exists, [String(name)])
            if context
                .existing_symbols?
                .contains(&expression_identifier_key(name)) =>
        {
            Number(1.0)
        }
        (CreoMathFunction::Search, [String(value), String(needle)]) => {
            let position = value
                .find(needle)
                .map_or(0, |byte| value[..byte].chars().count() + 1);
            Number(position as f64)
        }
        (CreoMathFunction::Extract, [String(value), Number(position), Number(length)]) => {
            if !position.is_finite()
                || !length.is_finite()
                || position.fract() != 0.0
                || length.fract() != 0.0
                || *position <= 0.0
                || *length < 0.0
            {
                return None;
            }
            let character_count = value.chars().count();
            if *position > character_count as f64 {
                String(std::string::String::new())
            } else {
                let start = *position as usize - 1;
                let remaining = character_count - start;
                let length = if *length >= remaining as f64 {
                    remaining
                } else {
                    *length as usize
                };
                String(value.chars().skip(start).take(length).collect())
            }
        }
        (CreoMathFunction::StringLength, [String(value)]) => Number(value.chars().count() as f64),
        (CreoMathFunction::StringStarts, [String(value), String(prefix)]) => {
            Number(f64::from(value.starts_with(prefix)))
        }
        (CreoMathFunction::StringEnds, [String(value), String(suffix)]) => {
            Number(f64::from(value.ends_with(suffix)))
        }
        (CreoMathFunction::StringMatch, [String(value), String(expected)]) => {
            Number(f64::from(value == expected))
        }
        (CreoMathFunction::StringPattern, [String(value), String(pattern)]) => {
            Number(f64::from(relation_string_pattern(value, pattern)?))
        }
        (
            name @ (CreoMathFunction::Sin | CreoMathFunction::Cos | CreoMathFunction::Tan),
            [Angle(value)],
        ) => Number(evaluate_creo_math_function(name, &[*value])?),
        (
            name @ (CreoMathFunction::Asin | CreoMathFunction::Acos | CreoMathFunction::Atan),
            [Number(value)],
        ) => Angle(evaluate_creo_math_function(name, &[*value])?),
        (CreoMathFunction::Atan2, [left, right]) => {
            let (left, left_dimension) = quantity_parts_ref(left)?;
            let (right, right_dimension) = quantity_parts_ref(right)?;
            (left_dimension == right_dimension).then_some(())?;
            Angle(evaluate_creo_math_function(
                CreoMathFunction::Atan2,
                &[left, right],
            )?)
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
            if *condition == 0.0 {
                when_false.clone()
            } else {
                when_true.clone()
            }
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
            )
        }
        (CreoMathFunction::Mod, [left, right]) => {
            let (left, left_dimension) = quantity_parts_ref(left)?;
            let (right, right_dimension) = quantity_parts_ref(right)?;
            (left_dimension == right_dimension && right != 0.0).then_some(())?;
            quantity_value(left % right, left_dimension)
        }
        (CreoMathFunction::Bound, [value, lower, upper]) => {
            let (value, value_dimension) = quantity_parts_ref(value)?;
            let (lower, lower_dimension) = quantity_parts_ref(lower)?;
            let (upper, upper_dimension) = quantity_parts_ref(upper)?;
            (value_dimension == lower_dimension
                && value_dimension == upper_dimension
                && lower < upper)
                .then_some(())?;
            quantity_value(value.clamp(lower, upper), value_dimension)
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
            quantity_value(value, value_dimension)
        }
        (CreoMathFunction::Pow, [base, Number(exponent)]) => {
            base.clone().power(Number(*exponent))?
        }
        (CreoMathFunction::Sqrt, [argument]) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            (value >= 0.0).then_some(())?;
            quantity_value(value.sqrt(), dimension.root(2)?)
        }
        (CreoMathFunction::Abs, [argument]) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            quantity_value(value.abs(), dimension)
        }
        (name @ (CreoMathFunction::Ceil | CreoMathFunction::Floor), [argument]) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            quantity_value(
                relation_round(value, 0.0, matches!(name, CreoMathFunction::Ceil))?,
                dimension,
            )
        }
        (
            name @ (CreoMathFunction::Ceil | CreoMathFunction::Floor),
            [argument, Number(decimal_places)],
        ) => {
            let (value, dimension) = quantity_parts_ref(argument)?;
            quantity_value(
                relation_round(
                    value,
                    *decimal_places,
                    matches!(name, CreoMathFunction::Ceil),
                )?,
                dimension,
            )
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
            Number(f64::from((left - right).abs() <= tolerance))
        }
        _ => {
            let numbers = arguments
                .iter()
                .map(|argument| match argument {
                    Number(value) => Some(*value),
                    Length(_) | Angle(_) | Quantity(_) | String(_) => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Number(evaluate_creo_math_function(name, &numbers)?)
        }
    };
    value.finite().then_some(value)
}

fn relation_string_pattern(value: &str, pattern: &str) -> Option<bool> {
    regex::RegexBuilder::new(&format!(r"\A(?:{pattern})\z"))
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .ok()
        .map(|pattern| pattern.is_match(value))
}

const MAX_RELATION_STRING_PRECISION: usize = 128;

fn relation_precision(value: f64) -> Option<usize> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= 0.0
        && value <= MAX_RELATION_STRING_PRECISION as f64)
        .then_some(value as usize)
}

fn format_relation_real(value: f64, decimals: Option<usize>, scientific: bool) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(String::new());
    }
    let Some(decimals) = decimals else {
        return Some(value.to_string());
    };
    if !scientific {
        return Some(format!("{value:.decimals$}"));
    }
    let formatted = format!("{value:.decimals$e}");
    let (mantissa, exponent) = formatted.split_once('e')?;
    let exponent = exponent.parse::<i32>().ok()?;
    Some(format!(
        "{mantissa}e{}{magnitude:02}",
        if exponent < 0 { "-" } else { "" },
        magnitude = exponent.unsigned_abs()
    ))
}

fn evaluate_relation_expression(
    expression: &str,
    values: &BTreeMap<String, CurveExpressionValue>,
    context: RelationEvaluationContext<'_>,
) -> Option<CurveExpressionValue> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context,
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn apply_declared_relation_unit(
    value: CurveExpressionValue,
    declared_unit: Option<&str>,
) -> Option<CurveExpressionValue> {
    let Some(declared_unit) = declared_unit else {
        return Some(value);
    };
    let unit = relation_unit(declared_unit)?;
    match (value, unit.dimension) {
        (CurveExpressionValue::Number(value), _) => {
            CurveExpressionValue::Number(value).with_unit(unit)
        }
        (value @ CurveExpressionValue::Length(_), RelationDimension::LENGTH)
        | (value @ CurveExpressionValue::Angle(_), RelationDimension::ANGLE) => Some(value),
        (CurveExpressionValue::Quantity(value), dimension) if value.dimension() == dimension => {
            Some(CurveExpressionValue::Quantity(value))
        }
        _ => None,
    }
}

fn evaluate_affine_expression(
    expression: &str,
    values: &BTreeMap<String, AffineValue>,
) -> Option<AffineValue> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context: RelationEvaluationContext::default(),
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn evaluate_simultaneous_affine_expression(
    expression: &str,
    values: &BTreeMap<String, SimultaneousAffineValue>,
    context: RelationEvaluationContext<'_>,
) -> Option<SimultaneousAffineValue> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context,
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn evaluate_dimension_expression(
    expression: &str,
    values: &BTreeMap<String, DimensionProbeValue>,
    context: RelationEvaluationContext<'_>,
) -> Option<DimensionProbeValue> {
    let mut parser = ExpressionParser {
        source: expression.as_bytes(),
        cursor: 0,
        values,
        context,
        nesting: 0,
    };
    let value = parser.logical_or()?;
    parser.whitespace();
    (parser.cursor == parser.source.len() && value.finite()).then_some(value)
}

fn infer_solve_variable_dimensions(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    known_dimensions: &[Option<RelationDimension>],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<RelationDimension>> {
    (known_dimensions.len() == block.variables.len()).then_some(())?;
    let variable_keys = block
        .variables
        .iter()
        .map(|variable| expression_identifier_key(variable))
        .collect::<Vec<_>>();
    let unique_keys = variable_keys.iter().collect::<BTreeSet<_>>();
    (unique_keys.len() == variable_keys.len()).then_some(())?;

    let mut probe_values = values
        .iter()
        .filter_map(|(name, value)| {
            DimensionProbeValue::from_relation_value(value).map(|value| (name.clone(), value))
        })
        .collect::<BTreeMap<_, _>>();
    for (key, dimension) in variable_keys.iter().zip(known_dimensions) {
        let value = dimension.map_or_else(
            || DimensionProbeValue::variable(key),
            |dimension| DimensionProbeValue {
                dimension: SymbolicRelationDimension::from_relation_dimension(dimension),
                kind: DimensionProbeKind::Numeric(None),
                constraints: Vec::new(),
            },
        );
        probe_values.insert(key.clone(), value);
    }

    let mut constraints = Vec::new();
    for equation in &block.equations {
        let left = evaluate_dimension_expression(&equation.left, &probe_values, context)?;
        let right = evaluate_dimension_expression(&equation.right, &probe_values, context)?;
        constraints.extend(left.constraints.iter().cloned());
        constraints.extend(right.constraints.iter().cloned());
        constraints.push(DimensionEquality {
            left: left.dimension,
            right: right.dimension,
        });
    }

    let mut axis_rows: [Vec<AffineEquationRow>; 5] =
        std::array::from_fn(|_| Vec::<AffineEquationRow>::new());
    let axis_variable_keys: [Vec<String>; 5] = std::array::from_fn(|axis| {
        variable_keys
            .iter()
            .map(|variable| dimension_variable_key(variable, axis))
            .collect()
    });
    for equality in constraints {
        for (axis, rows) in axis_rows.iter_mut().enumerate() {
            let difference = equality.left.axes[axis]
                .clone()
                .combine(equality.right.axes[axis].clone(), true)?;
            let coefficients = axis_variable_keys[axis]
                .iter()
                .map(|variable| {
                    difference
                        .variables
                        .get(variable)
                        .copied()
                        .unwrap_or_default()
                        .as_f64()
                })
                .collect::<Vec<_>>();
            let rhs = -difference.constant.as_f64();
            if coefficients.iter().any(|coefficient| *coefficient != 0.0) || rhs != 0.0 {
                rows.push(AffineEquationRow { coefficients, rhs });
            }
        }
    }

    let axis_len = variable_keys.len();
    let alloc_axis = || alloc_filled(axis_len, 0i8, "creo_solve_dimension_components").ok();
    let mut components: [Vec<i8>; 5] = [
        alloc_axis()?,
        alloc_axis()?,
        alloc_axis()?,
        alloc_axis()?,
        alloc_axis()?,
    ];
    for (index, dimension) in known_dimensions.iter().enumerate() {
        if let Some(dimension) = dimension {
            components[0][index] = dimension.length;
            components[1][index] = dimension.mass;
            components[2][index] = dimension.time;
            components[3][index] = dimension.angle;
            components[4][index] = dimension.temperature;
        }
    }
    let required_columns = known_dimensions
        .iter()
        .enumerate()
        .filter_map(|(index, dimension)| dimension.is_none().then_some(index))
        .collect::<BTreeSet<_>>();
    for (axis, rows) in axis_rows.iter_mut().enumerate() {
        let solution = solve_dimension_axis(rows, variable_keys.len(), &required_columns)?;
        for (index, value) in solution.into_iter().enumerate() {
            if known_dimensions[index].is_some() {
                continue;
            }
            let rounded = value.round();
            (value.is_finite() && (value - rounded).abs() <= EPS_DIMENSION_SOLUTION)
                .then_some(())?;
            components[axis][index] = i8::try_from(rounded as i16).ok()?;
        }
    }
    Some(
        (0..variable_keys.len())
            .map(|index| RelationDimension {
                length: components[0][index],
                mass: components[1][index],
                time: components[2][index],
                angle: components[3][index],
                temperature: components[4][index],
            })
            .collect(),
    )
}

fn solve_dimension_axis(
    rows: &mut [AffineEquationRow],
    variable_count: usize,
    required_columns: &BTreeSet<usize>,
) -> Option<Vec<f64>> {
    let mut pivot_row = 0;
    let mut pivot_rows = Vec::new();
    let coefficient_tolerance = EPS_LINEAR_SYSTEM_COEFFICIENT;
    for column in 0..variable_count {
        let Some(selected) = (pivot_row..rows.len()).max_by(|&first, &second| {
            rows[first].coefficients[column]
                .abs()
                .total_cmp(&rows[second].coefficients[column].abs())
        }) else {
            break;
        };
        let divisor = rows[selected].coefficients[column];
        if divisor.abs() <= coefficient_tolerance {
            continue;
        }
        rows.swap(pivot_row, selected);
        for coefficient in &mut rows[pivot_row].coefficients {
            *coefficient /= divisor;
        }
        rows[pivot_row].rhs /= divisor;
        let pivot_coefficients = rows[pivot_row].coefficients.clone();
        let pivot_rhs = rows[pivot_row].rhs;
        for (row_index, row) in rows.iter_mut().enumerate() {
            if row_index == pivot_row {
                continue;
            }
            let factor = row.coefficients[column];
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (coefficient, pivot) in row.coefficients.iter_mut().zip(&pivot_coefficients) {
                *coefficient -= factor * pivot;
                if coefficient.abs() <= coefficient_tolerance {
                    *coefficient = 0.0;
                }
            }
            row.rhs -= factor * pivot_rhs;
        }
        pivot_rows.push((column, pivot_row));
        pivot_row += 1;
    }
    let residual_tolerance =
        EPS_LINEAR_SYSTEM_RESIDUAL * rows.iter().map(|row| row.rhs.abs()).fold(1.0, f64::max);
    rows.iter()
        .all(|row| {
            let has_coefficients = row
                .coefficients
                .iter()
                .any(|coefficient| coefficient.abs() > coefficient_tolerance);
            has_coefficients || row.rhs.abs() <= residual_tolerance
        })
        .then_some(())?;
    required_columns
        .iter()
        .all(|required| pivot_rows.iter().any(|(column, _)| column == required))
        .then_some(())?;
    let mut solution = alloc_filled(variable_count, 0.0, "creo_solve_dimension_axis").ok()?;
    for (column, row) in pivot_rows {
        solution[column] = rows[row].rhs;
    }
    Some(solution)
}

fn solve_affine_expression_block(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<CurveExpressionValue>> {
    (variable_dimensions.len() == block.variables.len()).then_some(())?;
    let variable_keys = block
        .variables
        .iter()
        .map(|variable| expression_identifier_key(variable))
        .collect::<Vec<_>>();
    let mut affine_values = values
        .iter()
        .filter_map(|(name, value)| {
            let (value, dimension) = quantity_parts_ref(value)?;
            Some((
                name.clone(),
                SimultaneousAffineValue::constant(value, dimension),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    for (variable, dimension) in variable_keys.iter().zip(variable_dimensions) {
        affine_values.insert(
            variable.clone(),
            SimultaneousAffineValue {
                dimension: *dimension,
                constant: 0.0,
                coefficients: BTreeMap::from([(variable.clone(), 1.0)]),
            },
        );
    }
    let mut rows = block
        .equations
        .iter()
        .map(|equation| {
            let left =
                evaluate_simultaneous_affine_expression(&equation.left, &affine_values, context)?;
            let right =
                evaluate_simultaneous_affine_expression(&equation.right, &affine_values, context)?;
            let difference = left.combine(right, true)?;
            let coefficients = variable_keys
                .iter()
                .map(|variable| {
                    difference
                        .coefficients
                        .get(variable)
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect::<Vec<_>>();
            Some(AffineEquationRow {
                coefficients,
                rhs: -difference.constant,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let solution = solve_unique_affine_system(&mut rows, variable_keys.len())?;
    Some(
        solution
            .into_iter()
            .zip(variable_dimensions)
            .map(|(value, dimension)| quantity_value(value, *dimension))
            .collect(),
    )
}

const MAX_NONLINEAR_SOLVE_VARIABLES: usize = 8;
const MAX_NONLINEAR_SOLVE_ITERATIONS: usize = 64;
const MAX_NONLINEAR_SOLVE_LINE_SEARCH_STEPS: usize = 16;
const NONLINEAR_SOLVE_RESIDUAL_TOLERANCE: f64 = 1.0e-8;
const NONLINEAR_SOLVE_DERIVATIVE_STEP: f64 = 1.0e-6;
const NONLINEAR_SOLVE_SOLUTION_TOLERANCE: f64 = 1.0e-7;
const NONLINEAR_SOLVE_STEP_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, Copy)]
struct SolveResidual {
    value: f64,
    scale: f64,
    dimension: RelationDimension,
}

fn solve_nonlinear_expression_block(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    known_dimensions: &[Option<RelationDimension>],
    initial_values: &[Option<CurveExpressionValue>],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<CurveExpressionValue>> {
    nonlinear_equations_are_smooth(block).then_some(())?;
    let variable_dimensions =
        infer_solve_variable_dimensions(block, values, known_dimensions, context)?;
    let variable_count = block.variables.len();
    (variable_count > 0
        && variable_count <= MAX_NONLINEAR_SOLVE_VARIABLES
        && block.equations.len() >= variable_count)
        .then_some(())?;
    let mut seeds = nonlinear_initial_guesses(initial_values, &variable_dimensions)?.into_iter();
    let initial_seed = seeds.next()?;
    let solution =
        refine_nonlinear_solution(block, values, &variable_dimensions, &initial_seed, context)?;
    for seed in seeds {
        let Some(candidate) =
            refine_nonlinear_solution(block, values, &variable_dimensions, &seed, context)
        else {
            continue;
        };
        if !nonlinear_solutions_close(&solution, &candidate) {
            return None;
        }
    }
    Some(
        solution
            .into_iter()
            .zip(variable_dimensions)
            .map(|(value, dimension)| quantity_value(value, dimension))
            .collect(),
    )
}

fn nonlinear_equations_are_smooth(block: &CurveExpressionSolveBlock) -> bool {
    block.equations.iter().all(|equation| {
        [equation.left.as_str(), equation.right.as_str()]
            .into_iter()
            .all(nonlinear_expression_is_smooth)
    })
}

fn nonlinear_expression_is_smooth(expression: &str) -> bool {
    let bytes = expression.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\'' | b'"') {
            let delimiter = bytes[cursor];
            cursor += 1;
            while bytes.get(cursor).is_some_and(|byte| *byte != delimiter) {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&delimiter) {
                return false;
            }
            cursor += 1;
            continue;
        }
        if matches!(
            bytes[cursor],
            b'=' | b'!' | b'~' | b'<' | b'>' | b'&' | b'|'
        ) {
            return false;
        }
        if bytes[cursor] == b'_' || bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            let Some(end) = expression_identifier_end(bytes, start) else {
                return false;
            };
            cursor = end;
            let mut following = cursor;
            while bytes.get(following).is_some_and(u8::is_ascii_whitespace) {
                following += 1;
            }
            if bytes.get(following) == Some(&b'(') {
                let name = &expression[start..end];
                let smooth = [
                    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh",
                    "log", "ln", "exp", "pow", "sqrt",
                ]
                .into_iter()
                .any(|candidate| name.eq_ignore_ascii_case(candidate));
                if !smooth {
                    return false;
                }
            }
            continue;
        }
        cursor += 1;
    }
    true
}

fn nonlinear_initial_guesses(
    initial_values: &[Option<CurveExpressionValue>],
    variable_dimensions: &[RelationDimension],
) -> Option<Vec<Vec<f64>>> {
    let variable_count = variable_dimensions.len();
    let mut seeds = Vec::new();
    let mut add_seed = |seed: Vec<f64>| {
        if seed.iter().all(|value| value.is_finite()) && !seeds.iter().any(|known| known == &seed) {
            seeds.push(seed);
        }
    };
    (initial_values.len() == variable_count).then_some(())?;
    let initial = initial_values
        .iter()
        .zip(variable_dimensions)
        .map(|(value, dimension)| {
            value.as_ref().and_then(|value| {
                let (value, value_dimension) = quantity_parts_ref(value)?;
                (value_dimension == *dimension).then_some(value)
            })
        })
        .collect::<Option<Vec<_>>>()?;
    add_seed(initial);
    add_seed(alloc_filled(variable_count, 0.0, "creo_solve_seed_zero").ok()?);
    for magnitude in [0.01, 0.1, 1.0, 10.0, 100.0] {
        add_seed(alloc_filled(variable_count, magnitude, "creo_solve_seed_magnitude").ok()?);
        add_seed(alloc_filled(variable_count, -magnitude, "creo_solve_seed_magnitude").ok()?);
    }
    for index in 0..variable_count {
        for magnitude in [0.1, 1.0, 10.0] {
            let mut positive = alloc_filled(variable_count, 0.0, "creo_solve_seed_axis").ok()?;
            positive[index] = magnitude;
            add_seed(positive);
            let mut negative = alloc_filled(variable_count, 0.0, "creo_solve_seed_axis").ok()?;
            negative[index] = -magnitude;
            add_seed(negative);
        }
    }
    Some(seeds)
}

fn refine_nonlinear_solution(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    seed: &[f64],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<f64>> {
    let variable_count = variable_dimensions.len();
    let mut point = seed.to_vec();
    let mut residuals =
        evaluate_nonlinear_residuals(block, values, variable_dimensions, &point, context)?;
    for _ in 0..MAX_NONLINEAR_SOLVE_ITERATIONS {
        if nonlinear_residuals_converged(&residuals) {
            let mut rank_rows = nonlinear_jacobian_rows(
                block,
                values,
                variable_dimensions,
                &point,
                &residuals,
                context,
            )?;
            solve_unique_affine_system(&mut rank_rows, variable_count)?;
            return Some(point);
        }
        let mut rows = nonlinear_jacobian_rows(
            block,
            values,
            variable_dimensions,
            &point,
            &residuals,
            context,
        )?;
        for (row, residual) in rows.iter_mut().zip(&residuals) {
            row.rhs = -residual.value;
        }
        let delta = solve_unique_affine_system(&mut rows, variable_count)?;
        let maximum_delta = delta.iter().map(|value| value.abs()).fold(0.0, f64::max);
        let point_scale = point.iter().map(|value| value.abs()).fold(1.0, f64::max);
        (maximum_delta.is_finite() && maximum_delta <= 1e12 * point_scale).then_some(())?;
        let base_norm = nonlinear_residual_norm(&residuals);
        let mut accepted = None;
        let mut scale = 1.0;
        for _ in 0..MAX_NONLINEAR_SOLVE_LINE_SEARCH_STEPS {
            let candidate = point
                .iter()
                .zip(&delta)
                .map(|(value, change)| value + scale * change)
                .collect::<Vec<_>>();
            if candidate.iter().all(|value| value.is_finite()) {
                if let Some(candidate_residuals) = evaluate_nonlinear_residuals(
                    block,
                    values,
                    variable_dimensions,
                    &candidate,
                    context,
                ) {
                    let candidate_norm = nonlinear_residual_norm(&candidate_residuals);
                    if nonlinear_residuals_converged(&candidate_residuals)
                        || candidate_norm < base_norm
                    {
                        accepted = Some((candidate, candidate_residuals));
                        break;
                    }
                }
            }
            scale *= 0.5;
        }
        let (candidate, candidate_residuals) = accepted?;
        point = candidate;
        residuals = candidate_residuals;
        if maximum_delta * scale <= NONLINEAR_SOLVE_STEP_TOLERANCE * point_scale
            && !nonlinear_residuals_converged(&residuals)
        {
            return None;
        }
    }
    if !nonlinear_residuals_converged(&residuals) {
        return None;
    }
    let mut rank_rows = nonlinear_jacobian_rows(
        block,
        values,
        variable_dimensions,
        &point,
        &residuals,
        context,
    )?;
    solve_unique_affine_system(&mut rank_rows, variable_count)?;
    Some(point)
}

fn nonlinear_jacobian_rows(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    point: &[f64],
    residuals: &[SolveResidual],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<AffineEquationRow>> {
    let variable_count = variable_dimensions.len();
    let mut rows = Vec::with_capacity(residuals.len());
    for (row_index, residual) in residuals.iter().enumerate() {
        let mut coefficients = Vec::with_capacity(variable_count);
        for column in 0..variable_count {
            let step = NONLINEAR_SOLVE_DERIVATIVE_STEP * point[column].abs().max(1.0);
            let mut plus = point.to_vec();
            let mut minus = point.to_vec();
            plus[column] += step;
            minus[column] -= step;
            let plus_residuals =
                evaluate_nonlinear_residuals(block, values, variable_dimensions, &plus, context)?;
            let minus_residuals =
                evaluate_nonlinear_residuals(block, values, variable_dimensions, &minus, context)?;
            let plus_residual = plus_residuals.get(row_index)?;
            let minus_residual = minus_residuals.get(row_index)?;
            (plus_residual.dimension == residual.dimension
                && minus_residual.dimension == residual.dimension)
                .then_some(())?;
            let derivative = (plus_residual.value - minus_residual.value) / (2.0 * step);
            derivative.is_finite().then_some(())?;
            coefficients.push(derivative);
        }
        rows.push(AffineEquationRow {
            coefficients,
            rhs: 0.0,
        });
    }
    Some(rows)
}

fn evaluate_nonlinear_residuals(
    block: &CurveExpressionSolveBlock,
    values: &BTreeMap<String, CurveExpressionValue>,
    variable_dimensions: &[RelationDimension],
    point: &[f64],
    context: RelationEvaluationContext<'_>,
) -> Option<Vec<SolveResidual>> {
    (variable_dimensions.len() == block.variables.len()
        && point.len() == variable_dimensions.len())
    .then_some(())?;
    let mut evaluation_values = values.clone();
    for ((variable, dimension), value) in block.variables.iter().zip(variable_dimensions).zip(point)
    {
        value.is_finite().then_some(())?;
        evaluation_values.insert(
            expression_identifier_key(variable),
            quantity_value(*value, *dimension),
        );
    }
    block
        .equations
        .iter()
        .map(|equation| {
            let left = evaluate_relation_expression(&equation.left, &evaluation_values, context)?;
            let right = evaluate_relation_expression(&equation.right, &evaluation_values, context)?;
            let (left, left_dimension) = quantity_parts_ref(&left)?;
            let (right, right_dimension) = quantity_parts_ref(&right)?;
            (left_dimension == right_dimension).then_some(())?;
            let value = left - right;
            let scale = left.abs().max(right.abs()).max(1.0);
            (value.is_finite() && scale.is_finite()).then_some(SolveResidual {
                value,
                scale,
                dimension: left_dimension,
            })
        })
        .collect()
}

fn nonlinear_residual_norm(residuals: &[SolveResidual]) -> f64 {
    residuals
        .iter()
        .map(|residual| (residual.value / residual.scale).abs())
        .fold(0.0, f64::max)
}

fn nonlinear_residuals_converged(residuals: &[SolveResidual]) -> bool {
    residuals
        .iter()
        .all(|residual| residual.value.abs() <= NONLINEAR_SOLVE_RESIDUAL_TOLERANCE * residual.scale)
}

fn nonlinear_solutions_close(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            (left - right).abs()
                <= NONLINEAR_SOLVE_SOLUTION_TOLERANCE * left.abs().max(right.abs()).max(1.0)
        })
}

struct AffineEquationRow {
    coefficients: Vec<f64>,
    rhs: f64,
}

fn solve_unique_affine_system(
    rows: &mut [AffineEquationRow],
    variable_count: usize,
) -> Option<Vec<f64>> {
    (variable_count > 0 && rows.len() >= variable_count).then_some(())?;
    for row in rows.iter_mut() {
        let scale = row
            .coefficients
            .iter()
            .map(|value| value.abs())
            .fold(0.0, f64::max);
        if scale > 0.0 {
            for coefficient in &mut row.coefficients {
                *coefficient /= scale;
            }
            row.rhs /= scale;
        }
    }
    let rhs_scale = rows.iter().map(|row| row.rhs.abs()).fold(1.0, f64::max);
    let coefficient_tolerance = EPS_LINEAR_SYSTEM_COEFFICIENT;
    let residual_tolerance = EPS_LINEAR_SYSTEM_RESIDUAL * rhs_scale;
    let mut pivot_row = 0;
    for column in 0..variable_count {
        let selected = (pivot_row..rows.len()).max_by(|&first, &second| {
            rows[first].coefficients[column]
                .abs()
                .total_cmp(&rows[second].coefficients[column].abs())
        })?;
        let divisor = rows[selected].coefficients[column];
        (divisor.abs() > coefficient_tolerance).then_some(())?;
        rows.swap(pivot_row, selected);
        for coefficient in &mut rows[pivot_row].coefficients {
            *coefficient /= divisor;
        }
        rows[pivot_row].rhs /= divisor;
        let pivot_coefficients = rows[pivot_row].coefficients.clone();
        let pivot_rhs = rows[pivot_row].rhs;
        for (row_index, row) in rows.iter_mut().enumerate() {
            if row_index == pivot_row {
                continue;
            }
            let factor = row.coefficients[column];
            if factor.abs() <= coefficient_tolerance {
                continue;
            }
            for (coefficient, pivot) in row.coefficients.iter_mut().zip(&pivot_coefficients) {
                *coefficient -= factor * pivot;
                if coefficient.abs() <= coefficient_tolerance {
                    *coefficient = 0.0;
                }
            }
            row.rhs -= factor * pivot_rhs;
        }
        pivot_row += 1;
    }
    rows.iter()
        .skip(variable_count)
        .all(|row| {
            row.coefficients
                .iter()
                .all(|coefficient| coefficient.abs() <= coefficient_tolerance)
                && row.rhs.abs() <= residual_tolerance
        })
        .then_some(())?;
    let solution = rows
        .iter()
        .take(variable_count)
        .map(|row| row.rhs)
        .collect::<Vec<_>>();
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}

fn evaluate_affine_program(record: &CurveExpressionRecord) -> BTreeMap<String, AffineValue> {
    let mut values = BTreeMap::from([(
        "t".to_string(),
        AffineValue {
            constant: 0.0,
            linear: 1.0,
        },
    )]);
    let mut defined_symbols = BTreeSet::from(["t".to_string()]);
    for assignment in &record.assignments {
        let Some((name, declared_unit)) = assignment.parameter_target() else {
            continue;
        };
        let key = expression_identifier_key(name);
        let declaration_is_valid = declared_unit.is_none() || !defined_symbols.contains(&key);
        defined_symbols.insert(key.clone());
        match assignment.activation {
            CurveExpressionActivation::Active => {
                let value = declaration_is_valid
                    .then(|| evaluate_affine_expression(&assignment.expression, &values))
                    .flatten()
                    .and_then(|value| {
                        declared_unit
                            .map_or(Some(value), |unit| value.with_unit(relation_unit(unit)?))
                    });
                if let Some(value) = value {
                    values.insert(key, value);
                } else {
                    values.remove(&key);
                }
            }
            CurveExpressionActivation::Inactive => {}
            CurveExpressionActivation::Conditional => {
                values.remove(&key);
            }
        }
    }
    values
}

/// Recognize an exact cylindrical helix program expressed by the conventional
/// Creo outputs `r`, `theta` (degrees), and `z` over `t` in `[0, 1]`.
pub fn expression_helix(record: &CurveExpressionRecord) -> Option<CurveExpressionHelix> {
    record.prohibited_constructs.is_empty().then_some(())?;
    record.solve_blocks.is_empty().then_some(())?;
    (!record.unresolved_solve_control).then_some(())?;
    let values = evaluate_affine_program(record);
    let radius = values.get("r")?;
    let theta = values.get("theta")?;
    let z = values.get("z")?;
    if radius.constant <= 0.0 || radius.linear != 0.0 {
        return None;
    }
    let angular_travel = theta.linear;
    let revolutions = angular_travel.abs() / 360.0;
    (revolutions > 0.0).then_some(CurveExpressionHelix {
        radius: radius.constant,
        height: z.linear,
        z_start: z.constant,
        revolutions,
        start_angle: theta.constant.to_radians(),
        clockwise: angular_travel < 0.0,
    })
}

/// Decode positional `crv_array` rows whose terminal suffix has one
/// syntactically valid boundary. Callers that have decoded the enclosing
/// `srf_array` should use [`topology_rows_with_face_ids`] so an ambiguous
/// reference boundary can be resolved by its face roles.
#[cfg(test)]
pub fn topology_rows(payload: &[u8]) -> Vec<CurveTopologyRow> {
    topology_rows_with_face_ids(payload, None)
}

/// Decode standard topology rows using the enclosing `srf_array` identifier
/// set to resolve variable-width reference boundaries.
pub fn topology_rows_with_face_ids(
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Vec<CurveTopologyRow> {
    let mut rows = framed_rows_with_face_ids(payload, face_ids)
        .into_iter()
        .filter_map(|row| {
            parse_topology_row(
                &payload[row.start..row.end],
                row.start,
                row.suffix_start,
                row.suffix,
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.offset);
    rows.dedup_by_key(|row| row.offset);
    rows
}

/// Decode a complete DEPDB `crv_array\0 f2 f8 <count>` cross-section array.
/// Any malformed row or count mismatch withholds the entire array.
#[must_use]
pub fn depdb_cross_section_rows(payload: &[u8]) -> Vec<DepdbCurveRow> {
    let Some(array) = find(payload, b"crv_array\0", 0) else {
        return Vec::new();
    };
    let header = array + b"crv_array\0".len();
    if payload.get(header..header + 2) != Some(&[0xf2, psb::token::ARRAY_OPEN]) {
        return Vec::new();
    }
    let (count, after_count) = compact_int(payload, header + 2);
    if after_count == header + 2 {
        return Vec::new();
    }
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    if count == 0 || prototypes(payload).len() != 1 {
        return Vec::new();
    }
    let Some(topology) = find(payload, b"topol_ref_data\0", after_count) else {
        return Vec::new();
    };
    let mut cursor = topology + b"topol_ref_data\0".len();
    let cache = scalar::ScalarCache::from_section(payload);
    let positional_count = count - 1;
    // Each row consumes at least one payload byte past the topology cursor
    // before its terminator, so the row count cannot exceed the unread bytes.
    let capacity = bounded_len(
        positional_count as u64,
        1,
        payload.len().saturating_sub(cursor),
    )
    .unwrap_or(0);
    let mut rows = Vec::with_capacity(capacity);
    let mut boundaries = Vec::new();
    for (marker, length) in [
        (b"\xe1\xe3".as_slice(), 2),
        (b"\xe1\xf5\x05\xf6\xe3", 5),
        (b"\xe1\xe0", 1),
    ] {
        let mut search = cursor;
        while let Some(offset) = find(payload, marker, search) {
            boundaries.push((offset, length));
            search = offset + marker.len();
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    while rows.len() < positional_count {
        let first_candidate = boundaries.partition_point(|(end, _)| *end < cursor);
        let Some((row, terminator, length)) = boundaries[first_candidate..]
            .iter()
            .copied()
            .find_map(|(end, length)| {
                let row = parse_depdb_curve_segment(&payload[cursor..end], cursor, &cache)?;
                Some((row, end, length))
            })
        else {
            return Vec::new();
        };
        rows.push(row);
        cursor = terminator + length;
    }
    if rows.len() == positional_count {
        rows
    } else {
        Vec::new()
    }
}

fn parse_depdb_curve_segment(
    segment: &[u8],
    absolute_offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<DepdbCurveRow> {
    let suffixes = (4..=11)
        .filter_map(|suffix_length| {
            let start = segment.len().checked_sub(suffix_length)?;
            let (zero0, p1) = compact_int(segment, start);
            let (x1, p2) = compact_int(segment, p1);
            let (f1, p3) = compact_int(segment, p2);
            let (zero1, end) = compact_int(segment, p3);
            (p1 > start && p2 > p1 && p3 > p2 && end == segment.len())
                .then_some((start, [zero0, x1, f1, zero1]))
        })
        .filter(|(_, suffix)| suffix[0] == 0 && suffix[3] == 0)
        .collect::<Vec<_>>();
    let [(suffix_start, suffix)] = suffixes.as_slice() else {
        return None;
    };
    let prefixes = (0..*suffix_start).filter_map(|start| {
        let prefix = topology_prefix_fields(segment, start)?;
        (prefix.end <= *suffix_start).then_some((start, prefix))
    });
    let prefixes = prefixes
        .fold(BTreeMap::new(), |mut by_end, (start, prefix)| {
            by_end
                .entry(prefix.end)
                .and_modify(|(known_start, known_prefix)| {
                    if start < *known_start {
                        *known_start = start;
                        *known_prefix = prefix;
                    }
                })
                .or_insert((start, prefix));
            by_end
        })
        .into_values()
        .collect::<Vec<_>>();
    let [(row_start, prefix)] = prefixes.as_slice() else {
        return None;
    };
    let body = segment[prefix.end..*suffix_start].to_vec();
    let (scalar_tokens, references, opaque_spans) =
        curve_scalar_lane(&body, prefix.type_byte, cache)?;
    Some(DepdbCurveRow {
        id: prefix.id,
        type_byte: prefix.type_byte,
        feature_id: prefix.feature_id,
        directions: prefix.directions,
        suffix: *suffix,
        body,
        scalar_tokens,
        references,
        opaque_spans,
        offset: absolute_offset + row_start,
    })
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

type TopologySuffixCandidate = (usize, [u32; 4], [u32; 2]);

#[derive(Debug, Clone, Copy)]
struct TopologyPrefix {
    id: u32,
    type_byte: u8,
    feature_id: u32,
    directions: [u8; 2],
    end: usize,
}

fn row_terminator(payload: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    let short = find_in(payload, b"\xe1\xe3", start, end).map(|offset| (offset, 2));
    let long_search_end = short.map_or(end, |(offset, _)| {
        offset
            .saturating_add(b"\xe1\xf5\x05\xf6\xe3".len())
            .min(end)
    });
    let long =
        find_in(payload, b"\xe1\xf5\x05\xf6\xe3", start, long_search_end).map(|offset| (offset, 5));
    match (short, long) {
        (Some(left), Some(right)) => Some(if left.0 < right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

const CURVE_NAMESPACE_BOUNDARIES: [&[u8]; 4] = [
    b"crv_array\0",
    b"lo_array\0",
    b"qlt_array\0",
    b"srf_array\0",
];

fn curve_namespace_end(payload: &[u8], start: usize) -> usize {
    CURVE_NAMESPACE_BOUNDARIES
        .iter()
        .filter_map(|label| find(payload, label, start))
        .min()
        .unwrap_or(payload.len())
}

fn framed_rows_with_face_ids(payload: &[u8], face_ids: Option<&BTreeSet<u32>>) -> Vec<FramedRow> {
    let mut result = Vec::new();
    let mut arrays = Vec::new();
    let mut search = 0;
    while let Some(array) = find(payload, b"crv_array\0", search) {
        arrays.push(array + b"crv_array\0".len());
        search = array + b"crv_array\0".len();
    }
    if arrays.is_empty() {
        arrays.push(0);
    }
    for (index, &namespace_start) in arrays.iter().enumerate() {
        let namespace_end = arrays.get(index + 1).map_or_else(
            || curve_namespace_end(payload, namespace_start),
            |next| next - b"crv_array\0".len(),
        );
        let Some(label) = find_in(payload, b"topol_ref_data\0", namespace_start, namespace_end)
        else {
            continue;
        };
        let mut cursor = label + b"topol_ref_data\0".len();
        let mut boundary_anchored = false;
        let mut segments = Vec::new();
        while let Some((terminator, length)) = row_terminator(payload, cursor, namespace_end) {
            segments.push((cursor, terminator, boundary_anchored));
            cursor = terminator + length;
            boundary_anchored = true;
        }
        if cursor < namespace_end {
            segments.push((cursor, namespace_end, boundary_anchored));
        }
        let known_face_ids = face_ids.map(|face_ids| {
            let mut known = face_ids.clone();
            for &(start, end, _) in &segments {
                let Some((_, suffix, _)) = unique_topology_suffix_in_segment(&payload[start..end])
                else {
                    continue;
                };
                known.extend(suffix[..2].iter().copied().filter(|id| *id != 0));
            }
            known
        });
        for &(start, end, boundary_anchored) in &segments {
            if let Some(row) = framed_segment_with_face_ids(
                payload,
                namespace_start,
                start,
                end,
                boundary_anchored,
                face_ids,
                known_face_ids.as_ref(),
            ) {
                result.push(row);
            }
        }
    }
    result.sort_by_key(|row| row.start);
    result.dedup_by_key(|row| row.start);
    result
}

fn framed_segment_with_face_ids(
    payload: &[u8],
    namespace_start: usize,
    start: usize,
    end: usize,
    boundary_anchored: bool,
    materialized_face_ids: Option<&BTreeSet<u32>>,
    known_face_ids: Option<&BTreeSet<u32>>,
) -> Option<FramedRow> {
    let segment = payload.get(start..end)?;
    let mut prefixes = (0..segment.len())
        .filter_map(|row_start| {
            topology_prefix_fields(segment, row_start).map(|prefix| (row_start, prefix.end))
        })
        .collect::<Vec<_>>();
    prefixes.sort_unstable_by_key(|(_, end)| *end);
    let closes = segment
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == psb::token::COMPOUND_CLOSE)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    for close in closes.into_iter().rev() {
        let row_end = close + 1;
        if !complete_curve_row_linkage(&segment[row_end..]) {
            continue;
        }
        let Some((suffix_start, suffix, reference_geometry)) = topology_suffix_with_face_ids(
            &segment[..row_end],
            materialized_face_ids,
            known_face_ids,
        ) else {
            continue;
        };
        if boundary_anchored
            && topology_prefix_fields(segment, 0).is_some_and(|prefix| prefix.end <= suffix_start)
        {
            return Some(FramedRow {
                namespace_start,
                start,
                end: start + row_end,
                suffix_start,
                suffix,
                reference_geometry,
            });
        }
        let eligible = prefixes.partition_point(|(_, prefix_end)| *prefix_end <= suffix_start);
        if eligible == 1 {
            return Some(FramedRow {
                namespace_start,
                start: start + prefixes[0].0,
                end: start + row_end,
                suffix_start: suffix_start - prefixes[0].0,
                suffix,
                reference_geometry,
            });
        }
    }
    None
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
fn complete_curve_row_linkage(bytes: &[u8]) -> bool {
    let bytes = bytes
        .strip_suffix(&[0xe1, 0xf5, 0x05, 0xf6, 0xe0, 0x00])
        .or_else(|| bytes.strip_suffix(&[0xe1, 0xe0, 0x00]))
        .unwrap_or(bytes);
    let mut cursor = 0;
    if bytes.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let Some((_, next)) = generic_compact_at(bytes, cursor + 1) else {
            return false;
        };
        cursor = next;
    }
    if bytes.get(cursor) == Some(&psb::token::ARRAY_OPEN) {
        let Some((count, next)) = generic_compact_at(bytes, cursor + 1) else {
            return false;
        };
        let Some(count) = bounded_len(count.into(), 1, bytes.len().saturating_sub(next)) else {
            return false;
        };
        cursor = next;
        for _ in 0..count {
            let Some((_, next)) = generic_compact_at(bytes, cursor) else {
                return false;
            };
            cursor = next;
        }
    }
    let mut terminal_count = 0;
    while cursor < bytes.len() {
        let Some((_, next)) = generic_compact_at(bytes, cursor) else {
            return false;
        };
        cursor = next;
        terminal_count += 1;
    }
    terminal_count <= 4
}

fn curve_scalar_lane(
    body: &[u8],
    type_byte: u8,
    cache: &scalar::ScalarCache,
) -> Option<(
    Vec<CurveParameterScalar>,
    Vec<CurveParameterReference>,
    Vec<CurveParameterOpaqueSpan>,
)> {
    let mut scalars = Vec::new();
    let mut references = Vec::new();
    let mut claimed = alloc_filled(body.len(), false, "creo curve scalar claims").ok()?;
    let mut cursor = 0;
    while cursor < body.len() {
        if body[cursor] == psb::token::ENTITY_REF {
            if let Ok((reference, next)) = reference_id(body, cursor + 1) {
                references.push(CurveParameterReference {
                    entity_id: reference,
                    offset: cursor,
                    length: next - cursor,
                });
                claimed[cursor..next].fill(true);
                cursor = next;
                continue;
            }
        }
        if body[cursor] == 0x18
            && cursor + 1 == body.len()
            && matches!(type_byte, 0x00 | 0x01 | 0x06 | 0x08)
            && scalars.len() < 8
        {
            scalars.push(CurveParameterScalar {
                value: 0.0,
                raw: vec![0x18],
                offset: cursor,
                length: 1,
            });
            claimed[cursor] = true;
            cursor += 1;
            continue;
        }
        let decoded = if matches!(type_byte, 0x00 | 0x01 | 0x06 | 0x08) {
            scalar::decode_in_pcurve_lane(body, cursor, cache)
        } else {
            scalar::decode_in_row_lane(body, cursor, cache)
        };
        if let Some((value, next)) = decoded {
            scalars.push(CurveParameterScalar {
                value,
                raw: body[cursor..next].to_vec(),
                offset: cursor,
                length: next - cursor,
            });
            claimed[cursor..next].fill(true);
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    let mut opaque_spans = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if claimed[cursor] {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < body.len() && !claimed[cursor] {
            cursor += 1;
        }
        opaque_spans.push(CurveParameterOpaqueSpan {
            raw: body[start..cursor].to_vec(),
            offset: start,
            length: cursor - start,
        });
    }
    Some((scalars, references, opaque_spans))
}

/// Decode analytic bodies from positional curve rows with one valid terminal
/// topology suffix. Use [`parameter_records_with_face_ids`] when the enclosing
/// `srf_array` identifiers are available.
#[cfg(test)]
pub fn parameter_records(payload: &[u8]) -> Vec<CurveParameterRecord> {
    parameter_records_with_face_ids(payload, None)
}

/// Decode analytic bodies using the enclosing `srf_array` identifier set to
/// resolve variable-width reference boundaries.
pub fn parameter_records_with_face_ids(
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Vec<CurveParameterRecord> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut records = Vec::new();
    for framed in framed_rows_with_face_ids(payload, face_ids) {
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
        let body = row[body_start..suffix_start].to_vec();
        let Some((scalar_tokens, references, opaque_spans)) =
            curve_scalar_lane(&body, type_byte, &cache)
        else {
            continue;
        };
        let scalar_values = scalar_tokens.iter().map(|token| token.value).collect();
        let skipped_references = references
            .iter()
            .map(|reference| reference.entity_id)
            .collect();
        records.push(CurveParameterRecord {
            curve_id,
            type_byte,
            body,
            scalar_values,
            scalar_tokens,
            skipped_references,
            references,
            opaque_spans,
            reference_geometry: framed.reference_geometry,
            suffix: CurveSuffixStatus::Unique,
            offset: framed.start,
            body_offset: framed.start + body_start,
            suffix_offset: framed.start + suffix_start,
        });
    }
    records
}

fn uniquely_bounded_parameter_records(
    records: &[CurveParameterRecord],
) -> Vec<&CurveParameterRecord> {
    let mut counts = BTreeMap::new();
    for record in records {
        *counts.entry(record.curve_id).or_insert(0usize) += 1;
    }
    records
        .iter()
        .filter(|record| counts.get(&record.curve_id) == Some(&1))
        .filter(|record| record.suffix == CurveSuffixStatus::Unique)
        .collect()
}

fn complete_pcurve_values(record: &CurveParameterRecord) -> Option<[f64; 8]> {
    const HELD_SCALAR_OPEN: &[u8] = &[0xd7, 0xe8, 0x03];
    const HELD_SCALAR_CLOSE: u8 = 0x1e;

    record.references.is_empty().then_some(())?;
    let mut tokens = record.scalar_tokens.iter().peekable();
    let mut values = Vec::with_capacity(8);
    let mut cursor = 0;
    while cursor < record.body.len() {
        if record.body.get(cursor..cursor + HELD_SCALAR_OPEN.len()) == Some(HELD_SCALAR_OPEN) {
            cursor += HELD_SCALAR_OPEN.len();
            let token = tokens.next().filter(|token| token.offset == cursor)?;
            (token.length != 0
                && record.body.get(cursor..cursor + token.length) == Some(token.raw.as_slice()))
            .then_some(())?;
            values.push(token.value);
            cursor += token.length;
            (record.body.get(cursor) == Some(&HELD_SCALAR_CLOSE)).then_some(())?;
            cursor += 1;
            continue;
        }
        if let Some(token) = tokens.peek().filter(|token| token.offset == cursor) {
            (token.length != 0
                && record.body.get(cursor..cursor + token.length) == Some(token.raw.as_slice()))
            .then_some(())?;
            values.push(token.value);
            cursor += token.length;
            tokens.next();
        } else if record.body[cursor] == 0x12 {
            values.push(0.0);
            cursor += 1;
        } else {
            return None;
        }
    }
    tokens.next().is_none().then_some(())?;
    values.iter().all(|value| value.is_finite()).then_some(())?;
    values.try_into().ok()
}

/// Interpret complete eight-slot parameter lanes for pcurve-family rows.
pub fn pcurve_endpoints(
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Vec<PcurveEndpoints> {
    let mut result = uniquely_bounded_parameter_records(parameters)
        .into_iter()
        .filter(|record| matches!(record.type_byte, 0x00 | 0x01 | 0x06 | 0x08))
        .filter_map(|record| {
            let values = complete_pcurve_values(record)?;
            let mut matching = topology.iter().filter(|row| row.id == record.curve_id);
            let topology = matching.next()?;
            matching.next().is_none().then_some(())?;
            (topology.type_byte == record.type_byte).then_some(())?;
            Some(PcurveEndpoints {
                curve_id: record.curve_id,
                faces: topology.faces,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: record.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
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
    body: &[u8],
    start: usize,
    count: u32,
    cache: &scalar::ScalarCache,
) -> Option<Vec<[[f64; 2]; 2]>> {
    let sample_count = bounded_len(u64::from(count), 4, body.len().saturating_sub(start))?;
    (sample_count >= 2).then_some(())?;
    let mut cursor = start;
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let mut sample = [[0.0; 2]; 2];
        for (slot, value) in sample.iter_mut().flatten().enumerate() {
            let (decoded, next) = decode_two_chart_scalar(body, cursor, slot % 2 == 0, cache)?;
            (next > cursor && decoded.is_finite()).then_some(())?;
            *value = decoded;
            cursor = next;
        }
        samples.push(sample);
    }
    (cursor == body.len()).then_some(samples)
}

/// Decode byte-complete two-chart sample bodies from one curve namespace.
///
/// A canonical body supplies `fc <count>`. Later rows in the same feature and
/// raw curve family replay the canonical sample extent without the prefix.
/// Every admitted row consumes exactly four finite scalars per sample.
pub fn two_chart_pcurve_samples(
    payload: &[u8],
    face_ids: Option<&BTreeSet<u32>>,
) -> Vec<TwoChartPcurveSamples> {
    let cache = scalar::ScalarCache::from_section(payload);
    let framed = framed_rows_with_face_ids(payload, face_ids);
    let mut canonical_counts = BTreeMap::<(usize, u32, u8), BTreeSet<u32>>::new();
    for row in &framed {
        let bytes = &payload[row.start..row.end];
        let Some(prefix) = topology_prefix(bytes, 0, row.suffix_start) else {
            continue;
        };
        let body = &bytes[prefix.end..row.suffix_start];
        if body.first() != Some(&0xfc) || body.get(1) == Some(&0x05) {
            continue;
        }
        let (count, start) = compact_int(body, 1);
        if prefix.feature_id != 0
            && start > 1
            && complete_two_chart_samples(body, start, count, &cache).is_some()
        {
            canonical_counts
                .entry((row.namespace_start, prefix.feature_id, prefix.type_byte))
                .or_default()
                .insert(count);
        }
    }

    let mut result = Vec::new();
    for row in framed {
        let bytes = &payload[row.start..row.end];
        let Some(prefix) = topology_prefix(bytes, 0, row.suffix_start) else {
            continue;
        };
        let body = &bytes[prefix.end..row.suffix_start];
        let samples = if body.first() == Some(&0xfc) {
            if body.get(1) == Some(&0x05) {
                continue;
            }
            let (count, start) = compact_int(body, 1);
            (start > 1)
                .then(|| complete_two_chart_samples(body, start, count, &cache))
                .flatten()
        } else {
            let Some(counts) =
                canonical_counts.get(&(row.namespace_start, prefix.feature_id, prefix.type_byte))
            else {
                continue;
            };
            let mut candidates = counts
                .iter()
                .filter_map(|count| complete_two_chart_samples(body, 0, *count, &cache));
            let candidate = candidates.next();
            if candidates.next().is_some() {
                None
            } else {
                candidate
            }
        };
        let Some(samples) = samples else {
            continue;
        };
        result.push(TwoChartPcurveSamples {
            curve_id: prefix.id,
            faces: [row.suffix[0], row.suffix[1]],
            samples,
            offset: row.start,
        });
    }
    result.sort_by_key(|record| record.offset);
    let mut counts = BTreeMap::new();
    for record in &result {
        *counts.entry(record.curve_id).or_insert(0usize) += 1;
    }
    result.retain(|record| counts.get(&record.curve_id) == Some(&1));
    result
}

fn complete_fc02_short_pcurve_values(record: &CurveParameterRecord) -> Option<[[f64; 2]; 2]> {
    const ZERO_MARKER: &[u8] = &[0x18];
    const ONE_MARKER: &[u8] = &[0xe4];
    const TWO_MARKER: &[u8] = &[0x29, 0xff, 0xff];

    (record.body.get(..2) == Some(&[0xfc, 0x02])).then_some(())?;
    record.references.is_empty().then_some(())?;
    (record.scalar_values.len() == 7 && record.scalar_tokens.len() == 7).then_some(())?;
    let [prefix, terminal] = record.opaque_spans.as_slice() else {
        return None;
    };
    (prefix.offset == 0
        && prefix.raw == [0xfc, 0x02]
        && terminal.raw.first() == Some(&0x34)
        && terminal.length == 3)
        .then_some(())?;
    let mut cursor = prefix.length;
    for token in &record.scalar_tokens {
        (token.offset == cursor
            && token.length != 0
            && record.body.get(cursor..cursor + token.length) == Some(token.raw.as_slice()))
        .then_some(())?;
        cursor += token.length;
    }
    (terminal.offset == cursor && terminal.offset + terminal.length == record.body.len())
        .then_some(())?;
    let values: [f64; 7] = record
        .scalar_tokens
        .iter()
        .map(|token| token.value)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    (record.scalar_values.as_slice() == values.as_slice()).then_some(())?;
    (values.iter().all(|value| value.is_finite())
        && values[2] == 0.0
        && values[3] == 1.0
        && record.scalar_tokens[2].raw.as_slice() == ZERO_MARKER
        && record.scalar_tokens[3].raw.as_slice() == ONE_MARKER
        && record.scalar_tokens[6].raw.as_slice() == TWO_MARKER)
        .then_some(())?;
    Some([[values[0], values[1]], [values[4], values[5]]])
}

/// Decode complete one-sided endpoint paths from the short fc 02 body.
///
/// A path is admitted only when the body has one unique topology row, a
/// complete seven-scalar lane, and the bounded terminal operand. Other fc 02
/// bodies remain native parameter records until their grammar is settled.
pub fn fc02_short_pcurve_endpoints(
    parameters: &[CurveParameterRecord],
    topology: &[CurveTopologyRow],
) -> Vec<Fc02ShortPcurveEndpoints> {
    let mut result = uniquely_bounded_parameter_records(parameters)
        .into_iter()
        .filter_map(|record| {
            let face_0_endpoints = complete_fc02_short_pcurve_values(record)?;
            let mut matching = topology.iter().filter(|row| row.id == record.curve_id);
            let topology = matching.next()?;
            matching.next().is_none().then_some(())?;
            (topology.type_byte == record.type_byte).then_some(())?;
            Some(Fc02ShortPcurveEndpoints {
                curve_id: record.curve_id,
                faces: topology.faces,
                face_0_endpoints,
                offset: record.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode exact world-coordinate tokens from FC-prefixed dense curve bodies.
pub fn fc_coordinates(parameters: &[CurveParameterRecord]) -> Vec<FcCurveCoordinates> {
    let mut result = Vec::new();
    for record in uniquely_bounded_parameter_records(parameters) {
        let Some((&0xfc, tail)) = record.body.split_first() else {
            continue;
        };
        let Some((&subtype, lane)) = tail.split_first() else {
            continue;
        };
        let mut tokens = Vec::new();
        let mut cursor = 0;
        while cursor < lane.len() {
            if matches!(lane[cursor], 0x46 | 0x2d) {
                if let Some((value, next)) = scalar::decode(lane, cursor) {
                    tokens.push(FcCurveCoordinateToken {
                        value_mm: value,
                        raw: lane[cursor..next].to_vec(),
                        offset: cursor + 2,
                        length: next - cursor,
                    });
                    cursor = next;
                    continue;
                }
            }
            cursor += 1;
        }
        if tokens.len() >= 4 {
            let mut opaque_spans = Vec::new();
            let mut unclaimed = 0;
            for token in &tokens {
                if unclaimed < token.offset {
                    opaque_spans.push(FcCurveOpaqueSpan {
                        raw: record.body[unclaimed..token.offset].to_vec(),
                        offset: unclaimed,
                        length: token.offset - unclaimed,
                    });
                }
                unclaimed = token.offset + token.length;
            }
            if unclaimed < record.body.len() {
                opaque_spans.push(FcCurveOpaqueSpan {
                    raw: record.body[unclaimed..].to_vec(),
                    offset: unclaimed,
                    length: record.body.len() - unclaimed,
                });
            }
            result.push(FcCurveCoordinates {
                curve_id: record.curve_id,
                subtype,
                body: record.body.clone(),
                values_mm: tokens.iter().map(|token| token.value_mm).collect(),
                tokens,
                opaque_spans,
                offset: record.offset,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
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
    let byte_1 = prefix.wrapping_sub(0x8b);
    let mut raw = [0; 8];
    raw[0] = if byte_1 >= 0x80 { 0x3f } else { 0x40 };
    raw[1] = byte_1;
    raw[2..].copy_from_slice(&body[offset + 1..offset + 7]);
    // Computed IEEE bytes 0..1 plus six file bytes; not a contiguous window.
    Some((f64::from_be_bytes(raw), offset + 7))
}

/// Validate FC05 point lanes against their exact circle identity.
pub fn fc05_circles(parameters: &[CurveParameterRecord]) -> Vec<Fc05Circle> {
    let mut circles = Vec::new();
    for record in uniquely_bounded_parameter_records(parameters) {
        if record.body.get(..2) != Some(&[0xfc, 0x05]) {
            continue;
        }
        let mut points = Vec::new();
        let mut cursor = 2;
        while cursor < record.body.len() {
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
        if points
            .iter()
            .any(|point| (point.3 - ordinate).abs() > EPS_ORDINATE_AGREEMENT)
        {
            continue;
        }
        let first = points[0];
        let middle = points[points.len() / 2];
        let last = points[points.len() - 1];
        let a11 = 2.0 * (middle.0 - first.0);
        let a12 = 2.0 * (middle.1 - first.1);
        let a21 = 2.0 * (last.0 - middle.0);
        let a22 = 2.0 * (last.1 - middle.1);
        let determinant = a11.mul_add(a22, -(a12 * a21));
        if determinant.abs() < 1e-15 {
            continue;
        }
        let bx = middle.0.mul_add(middle.0, middle.1 * middle.1)
            - first.0.mul_add(first.0, first.1 * first.1);
        let bz = last.0.mul_add(last.0, last.1 * last.1)
            - middle.0.mul_add(middle.0, middle.1 * middle.1);
        let center_x = bx.mul_add(a22, -(a12 * bz)) / determinant;
        let center_z = a11.mul_add(bz, -(bx * a21)) / determinant;
        let radius = (first.0 - center_x).hypot(first.1 - center_z);
        if radius <= 0.0 {
            continue;
        }
        let residuals = points
            .iter()
            .map(|point| ((point.0 - center_x).hypot(point.1 - center_z) - radius).abs())
            .collect::<Vec<_>>();
        let max_residual = residuals.iter().copied().fold(0.0, f64::max);
        if max_residual > EPS_CIRCLE_RESIDUAL * radius.max(1.0) {
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
            points.iter().all(|point| {
                let (Some(parameter), Some(parameter_0)) = (point.2, parameter_0) else {
                    return false;
                };
                let angle = (point.1 - center_z).atan2(point.0 - center_x);
                let expected = angle_0 + sign * (parameter - parameter_0);
                wrapped_distance(angle, expected) <= EPS_ANGLE_AGREEMENT
            })
        };
        let positive = sign_matches(1.0);
        let negative = sign_matches(-1.0);
        let angle_parameter_consistent = positive ^ negative;
        let parameter_sign = match (positive, negative) {
            (true, false) => Some(1),
            (false, true) => Some(-1),
            _ => None,
        };
        let reference_direction_row_frame =
            parameter_sign.zip(parameter_0).map(|(sign, parameter_0)| {
                let reference_angle = angle_0 - f64::from(sign) * parameter_0;
                [reference_angle.cos(), reference_angle.sin()]
            });
        let sample_direction_row_frame =
            [(first.0 - center_x) / radius, (first.1 - center_z) / radius];
        circles.push(Fc05Circle {
            curve_id: record.curve_id,
            center_row_frame: [center_x, center_z],
            radius_mm: radius,
            sample_direction_row_frame,
            reference_direction_row_frame,
            parameter_sign,
            cap_ordinate_row_frame: Some(ordinate),
            point_count: points.len(),
            max_residual,
            angle_parameter_consistent,
            offset: record.offset,
        });
    }
    circles.sort_by_key(|circle| circle.offset);
    circles
}

/// Bind validated `fc 05` circles to typed cylinder/plane face pairs and retain
/// only groups that agree on radius and center at two distinct cap ordinates.
pub fn fc05_cylinder_cap_pairs(
    circles: &[Fc05Circle],
    topology: &[CurveTopologyRow],
    surfaces: &[crate::surface::SurfaceRow],
) -> Vec<Fc05CylinderCapPair> {
    use std::collections::BTreeMap;

    let faces = crate::topology::uniquely_identified_rows(topology)
        .into_iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();
    let mut circle_counts = BTreeMap::<u32, usize>::new();
    for circle in circles {
        *circle_counts.entry(circle.curve_id).or_default() += 1;
    }
    let mut groups = BTreeMap::<u32, Vec<(&Fc05Circle, u32)>>::new();
    for circle in circles {
        if circle_counts.get(&circle.curve_id) != Some(&1) {
            continue;
        }
        let Some(adjacent) = faces.get(&circle.curve_id) else {
            continue;
        };
        let cylinders = adjacent
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(surfaces, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .copied()
            .collect::<Vec<_>>();
        let planes = adjacent
            .iter()
            .filter(|face| {
                crate::surface::unique_surface_row(surfaces, **face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            })
            .copied()
            .collect::<Vec<_>>();
        if cylinders.len() == 1 && planes.len() == 1 && circle.cap_ordinate_row_frame.is_some() {
            groups
                .entry(cylinders[0])
                .or_default()
                .push((circle, planes[0]));
        }
    }

    let mut result = Vec::new();
    for (surface_id, mut group) in groups {
        group.sort_by_key(|(circle, _)| circle.offset);
        let first = group[0].0;
        let (Some(reference_direction_row_frame), Some(parameter_sign)) =
            (first.reference_direction_row_frame, first.parameter_sign)
        else {
            continue;
        };
        let tolerance = EPS_RADIUS_AGREEMENT * first.radius_mm.max(1.0);
        if !group.iter().all(|(circle, _)| {
            (circle.radius_mm - first.radius_mm).abs() <= tolerance
                && (circle.center_row_frame[0] - first.center_row_frame[0]).abs() <= tolerance
                && (circle.center_row_frame[1] - first.center_row_frame[1]).abs() <= tolerance
                && circle.parameter_sign == first.parameter_sign
                && circle
                    .reference_direction_row_frame
                    .is_some_and(|direction| {
                        (direction[0] - reference_direction_row_frame[0]).abs() <= tolerance
                            && (direction[1] - reference_direction_row_frame[1]).abs() <= tolerance
                    })
                && circle.angle_parameter_consistent
        }) {
            continue;
        }
        let mut ordinates = Vec::new();
        for ordinate in group
            .iter()
            .filter_map(|(circle, _)| circle.cap_ordinate_row_frame)
        {
            if ordinates
                .iter()
                .all(|existing: &f64| (*existing - ordinate).abs() > tolerance)
            {
                ordinates.push(ordinate);
            }
        }
        if ordinates.len() < 2 {
            continue;
        }
        result.push(Fc05CylinderCapPair {
            surface_id,
            curve_ids: group.iter().map(|(circle, _)| circle.curve_id).collect(),
            cap_plane_ids: group.iter().map(|(_, plane)| *plane).collect(),
            curve_cap_ordinates_row_frame: group
                .iter()
                .filter_map(|(circle, _)| circle.cap_ordinate_row_frame)
                .collect(),
            center_row_frame: first.center_row_frame,
            radius_mm: first.radius_mm,
            reference_direction_row_frame,
            parameter_sign,
            cap_ordinates_row_frame: ordinates,
            offset: first.offset,
        });
    }
    result.sort_by_key(|pair| pair.offset);
    result
}

/// Decode labeled `crv_pnt_arr f9 02 04` prototype pcurve endpoints.
pub fn prototype_pcurve_endpoints(payload: &[u8]) -> Vec<PrototypePcurveEndpoints> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(namespace) = find(payload, b"crv_array\0", search) {
        let start = namespace + b"crv_array\0".len();
        let end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        search = start;
        let Some(id_label) = find_in(payload, b"crv_id\0", start, end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let (curve_id, after_id) = compact_int(payload, id_start);
        if after_id == id_start {
            continue;
        }
        let prototype_end = find_in(payload, b"topol_ref_data\0", after_id, end).unwrap_or(end);
        let Some(points_label) = unique_find_in(payload, b"crv_pnt_arr\0", after_id, prototype_end)
        else {
            continue;
        };
        let header = points_label + b"crv_pnt_arr\0".len();
        if payload.get(header..header + 3) != Some(&[psb::token::SCALAR_BODY, 0x02, 0x04]) {
            continue;
        }
        let mut cursor = header + 3;
        let mut values = Vec::with_capacity(8);
        while cursor < prototype_end && values.len() < 8 {
            if payload[cursor] == 0x12
                || (payload[cursor] == 0x18
                    && values.len() == 7
                    && (cursor + 1 == prototype_end || payload.get(cursor + 1) == Some(&0xe0)))
            {
                values.push(0.0);
                cursor += 1;
            } else if let Some((value, next)) = scalar::decode_in_lane(payload, cursor, &cache) {
                values.push(value);
                cursor = next;
            } else {
                break;
            }
        }
        let array_is_bounded = cursor == prototype_end || payload.get(cursor) == Some(&0xe0);
        if values.len() == 8 && values.iter().all(|value| value.is_finite()) && array_is_bounded {
            result.push(PrototypePcurveEndpoints {
                curve_id,
                face_0_endpoints: [[values[0], values[1]], [values[4], values[5]]],
                face_1_endpoints: [[values[2], values[3]], [values[6], values[7]]],
                offset: points_label,
            });
        }
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Decode the four labeled topology pointers of each curve prototype.
pub fn prototype_topology(payload: &[u8]) -> Vec<CurvePrototypeTopology> {
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(namespace) = find(payload, b"crv_array\0", search) {
        let start = namespace + b"crv_array\0".len();
        let end = find(payload, b"crv_array\0", start).unwrap_or(payload.len());
        search = start;
        let Some(id_label) = find_in(payload, b"crv_id\0", start, end) else {
            continue;
        };
        let id_start = id_label + b"crv_id\0".len();
        let Ok((curve_id, _)) = reference_id(payload, id_start) else {
            continue;
        };
        let prototype_end = find_in(payload, b"topol_ref_data\0", id_start, end).unwrap_or(end);
        let reference = |label: &[u8]| {
            let at = unique_find_in(payload, label, id_start, prototype_end)? + label.len();
            reference_id(payload, at).ok().map(|(value, _)| value)
        };
        let Some(face_0) = reference(b"crv_hdr_geom_ptr[0]\0") else {
            continue;
        };
        let Some(face_1) = reference(b"crv_hdr_geom_ptr[1]\0") else {
            continue;
        };
        let Some(next_0) = reference(b"next_crv_hdr_ptr[0]\0") else {
            continue;
        };
        let Some(next_1) = reference(b"next_crv_hdr_ptr[1]\0") else {
            continue;
        };
        result.push(CurvePrototypeTopology {
            curve_id,
            faces: [face_0, face_1],
            next_edges: [next_0, next_1],
            offset: namespace,
        });
    }
    result.sort_by_key(|record| record.offset);
    result
}

/// Bind complete prototype UV endpoints to labeled prototype topology.
pub fn bind_prototype_pcurves(
    pcurves: &[PrototypePcurveEndpoints],
    topology: &[CurvePrototypeTopology],
) -> Vec<BoundPrototypePcurve> {
    let mut pcurve_counts = BTreeMap::new();
    for pcurve in pcurves {
        *pcurve_counts.entry(pcurve.curve_id).or_insert(0usize) += 1;
    }
    let mut topology_counts = BTreeMap::new();
    for row in topology {
        *topology_counts.entry(row.curve_id).or_insert(0usize) += 1;
    }
    let mut result = pcurves
        .iter()
        .filter(|pcurve| pcurve_counts.get(&pcurve.curve_id) == Some(&1))
        .filter(|pcurve| topology_counts.get(&pcurve.curve_id) == Some(&1))
        .filter_map(|pcurve| {
            let topology = topology
                .iter()
                .find(|topology| topology.curve_id == pcurve.curve_id)?;
            Some(BoundPrototypePcurve {
                curve_id: pcurve.curve_id,
                faces: topology.faces,
                face_0_endpoints: pcurve.face_0_endpoints,
                face_1_endpoints: pcurve.face_1_endpoints,
                offset: pcurve.offset,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|record| record.offset);
    result
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
        faces: [f0, f1],
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
    row: &[u8],
    materialized_face_ids: Option<&BTreeSet<u32>>,
    known_face_ids: Option<&BTreeSet<u32>>,
) -> Option<TopologySuffixCandidate> {
    let candidates = topology_suffix_candidates(row)?;
    if candidates.len() == 1 {
        return candidates.first().copied();
    }
    if let Some(ids) = materialized_face_ids.filter(|ids| !ids.is_empty()) {
        let role_matches = candidates
            .iter()
            .filter(|(_, references, _)| {
                references[..2]
                    .iter()
                    .all(|&face_id| face_id == 0 || ids.contains(&face_id))
            })
            .copied()
            .collect::<Vec<_>>();
        match role_matches.as_slice() {
            [candidate] => return Some(*candidate),
            [] => {}
            _ => return None,
        }
    }
    let ids = known_face_ids.filter(|ids| !ids.is_empty())?;
    let mut role_matches = candidates.into_iter().filter(|(_, references, _)| {
        references[..2]
            .iter()
            .all(|&face_id| face_id == 0 || ids.contains(&face_id))
    });
    let candidate = role_matches.next()?;
    role_matches.next().is_none().then_some(candidate)
}

fn unique_topology_suffix_in_segment(segment: &[u8]) -> Option<TopologySuffixCandidate> {
    let closes = segment
        .windows(3)
        .enumerate()
        .filter(|(_, bytes)| *bytes == [0, 0, psb::token::COMPOUND_CLOSE])
        .map(|(offset, _)| offset);
    for close in closes.rev() {
        let row_end = close + 3;
        let Some(candidates) = topology_suffix_candidates(&segment[..row_end]) else {
            continue;
        };
        if let [candidate] = candidates.as_slice() {
            return Some(*candidate);
        }
    }
    None
}

fn topology_suffix_candidates(row: &[u8]) -> Option<Vec<TopologySuffixCandidate>> {
    let close = row.len().checked_sub(1)?;
    (row.get(close) == Some(&psb::token::COMPOUND_CLOSE)).then_some(())?;
    let reference_geometry_candidates = if row.get(close.saturating_sub(2)..close) == Some(&[0, 0])
    {
        vec![(close - 2, [0, 0])]
    } else {
        let mut candidates = Vec::new();
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
                candidates.push((start, [first, second]));
            }
        }
        candidates
    };
    let mut candidates = Vec::new();
    for (reference_geometry_start, reference_geometry) in reference_geometry_candidates {
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
                candidates.push((start, [f0, f1, e0, e1], reference_geometry));
            }
        }
    }
    Some(candidates)
}

fn unique_find_in(data: &[u8], needle: &[u8], from: usize, end: usize) -> Option<usize> {
    let offset = find_in(data, needle, from, end)?;
    find_in(data, needle, offset.saturating_add(1), end)
        .is_none()
        .then_some(offset)
}

#[cfg(test)]
mod tests;
