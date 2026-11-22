// SPDX-License-Identifier: Apache-2.0
//! Law formulas, law expressions, and law surface constructions.

use super::legacy_cache::{FitTolerance, LegacyCache};
use super::lofts::LoftPathCurve;
use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One recursively framed native law formula.
// A source states raw values; a law store holds the admitted formula, whose
// scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum LawFormula<R = f64, V = Vector3, P = Point3> {
    /// The native null law, with no variables.
    Null {},
    /// Named formula and its ordered recursive variables.
    Named {
        /// Native formula name.
        name: cadmpeg_core::text::NonBlankString,
        /// Ordered recursive variables.
        variables: Vec<LawExpression<R, V, P>>,
    },
}

impl<R, V, P> LawFormula<R, V, P> {
    /// Ordered recursive variables; empty for the null variant.
    #[must_use]
    pub fn variables(&self) -> &[LawExpression<R, V, P>] {
        match self {
            Self::Null {} => &[],
            Self::Named { variables, .. } => variables,
        }
    }
}

/// A law formula whose expression tree carries only finite scalars.
///
/// The carrier of the `law_int_cur` formulas. It serializes as the formula it
/// holds, so the curve wire states the formula alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LawFormula")]
pub struct FiniteLawFormula(
    #[cfg_attr(feature = "schema", schemars(with = "LawFormula"))]
    pub(super)  LawFormula<FiniteReal, FiniteVector3, FinitePoint3>,
);

impl TryFrom<LawFormula> for FiniteLawFormula {
    type Error = &'static str;

    fn try_from(formula: LawFormula) -> Result<Self, Self::Error> {
        Self::try_new(formula)
    }
}

impl FiniteLawFormula {
    /// Admit a law formula whose expression tree carries only finite scalars.
    pub fn try_new(formula: LawFormula) -> Result<Self, &'static str> {
        formula.admit().map(Self).ok_or(LAW_FORMULA_NOT_FINITE)
    }

    /// The admitted law formula.
    #[must_use]
    pub const fn formula(&self) -> &LawFormula<FiniteReal, FiniteVector3, FinitePoint3> {
        &self.0
    }
}

/// Refusal of a law formula that carries a non-finite scalar.
pub(super) const LAW_FORMULA_NOT_FINITE: &str = "law formula constants must be finite";

/// Recursion bound of the law-expression walk. An expression nested deeper than
/// this is refused.
pub(super) const LAW_EXPRESSION_DEPTH_LIMIT: usize = 64;

/// Complete recursive construction stored by a native law spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct LawSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Legacy U and V parameter intervals; absent from modern layouts.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_ranges"
    )]
    pub parameter_ranges: Option<[[R; 2]; 2]>,
    /// Primary recursive surface law.
    pub primary: LawFormula<R, V, P>,
    /// Ordered counted auxiliary laws referenced by the primary law.
    pub additional: Vec<LawFormula<R, V, P>>,
    /// Standard surface-tail mode and its mode-specific fields.
    pub tail: LawSurfaceTail<R>,
    /// Six ordered discontinuity arrays from the standard surface tail.
    pub discontinuities: [Vec<R>; 6],
}

/// Mode-specific payload of a native law surface's standard surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LawSurfaceTail<R = f64> {
    /// Selector 0; the surface record carries a solved NURBS cache, whose fit
    /// contract this tail states.
    Full {
        /// Fit contract of the solved cache this tail requires.
        cache: LegacyCache,
    },
    /// Selector 1; compact parameter summaries replace the solved cache.
    Summary {
        /// Ordered U and V parameter summaries.
        parameters: [Vec<R>; 2],
        /// Native model-space fit tolerance.
        fit_tolerance: FitTolerance,
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 2; exact parameter intervals and boundary classifications.
    None {
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[R; 2]; 2],
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 3; no mode-specific payload.
    Historical {},
    /// Selector 4; no mode-specific payload.
    Optimal {},
}

/// One native law-expression node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum LawExpression<R = f64, V = Vector3, P = Point3> {
    /// Zero-payload spelling of the native null law.
    Null {},
    /// Serializer-preserved textual law expression.
    Text {
        /// Exact text stored in the native law slot.
        value: cadmpeg_core::text::NonBlankString,
    },
    /// Tagged integer constant.
    Integer {
        /// Stored integer value.
        value: i64,
    },
    /// Tagged double constant.
    Double {
        /// Stored scalar value.
        value: R,
    },
    /// Tagged model-space point constant.
    Point {
        /// Stored point value.
        value: P,
    },
    /// Tagged direction-vector constant.
    Vector {
        /// Stored vector value.
        value: V,
    },
    /// Inline transform-law payload.
    Transform {
        /// Thirteen ordered transform scalars.
        scalars: [R; 13],
        /// Three ordered transform enums.
        enums: [i64; 3],
    },
    /// Vector-serialized transform-law payload: four ordered vectors, a scale,
    /// and three flags, in place of the thirteen-scalar/three-enum form.
    TransformVec {
        /// Four ordered transform vectors.
        vectors: [V; 4],
        /// Trailing transform scale.
        scale: R,
        /// Three ordered transform flags.
        flags: [bool; 3],
    },
    /// Curve-backed edge law.
    Edge {
        /// Embedded curve carrier and its optional revision-gated endpoints.
        #[serde(flatten)]
        curve: LoftPathCurve<R>,
        /// Two native curve parameters.
        parameters: [R; 2],
    },
    /// Spline-law payload.
    Spline {
        /// Native spline-law integer.
        native_id: i64,
        /// Ordered spline-law knots.
        knots: Vec<R>,
        /// Ordered spline-law controls.
        controls: Vec<R>,
        /// Native model-space point.
        point: P,
    },
    /// Algebraic operator and its recursively framed operands.
    Algebraic {
        /// Native operator token.
        operator: String,
        /// Ordered operands.
        operands: Vec<LawExpression<R, V, P>>,
    },
}

cadmpeg_core::named_optional_field!(
    deserialize_parameter_ranges<R>,
    [[R; 2]; 2],
    "parameter_ranges"
);
