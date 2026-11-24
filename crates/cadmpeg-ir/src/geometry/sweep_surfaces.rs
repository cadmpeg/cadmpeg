// SPDX-License-Identifier: Apache-2.0
//! Sweep surface layouts and constructions.

use super::laws::{LawExpression, LawFormula};
use super::legacy_cache::CacheContract;
use super::revision_forms::{RevisionCacheForm, RevisionSurfaceParameterization};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveI64;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Structurally selected native sweep payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum SweepSurfaceLayout<R = f64, V = Vector3, P = Point3> {
    /// Profile-first modern ASM sweep layout.
    ProfileFirst {
        /// Second native sweep enum.
        secondary_kind: i64,
        /// Five ordered frame directions.
        directions: [V; 5],
        /// Native model-space frame origin.
        origin: P,
        /// Four ordered native frame scalars.
        parameters: [R; 4],
        /// Three ordered parametric laws.
        formulas: Box<[LawFormula<R, V, P>; 3]>,
    },
    /// Explicit sweep layout whose trajectory is controlled by a formula.
    ExplicitFormula {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile_frame"
        )]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [R; 2],
        /// Native trajectory scalar.
        path_parameter: R,
        /// Native formula-side boolean.
        formula_flag: bool,
        /// Parametric trajectory formula.
        formula: LawFormula<R, V, P>,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
    /// Explicit sweep layout controlled by an auxiliary guide curve.
    ExplicitGuide {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile_frame"
        )]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [R; 2],
        /// Native trajectory scalar.
        path_parameter: R,
        /// Two guide-side booleans.
        guide_flags: [bool; 2],
        /// Auxiliary guide curve.
        guide_curve: CurveId,
        /// Guide parameter interval.
        guide_range: [R; 2],
        /// Two native guide integers.
        guide_modes: [i64; 2],
        /// Six ordered guide scalars.
        guide_parameters: [R; 6],
        /// Three trailing guide booleans.
        trailing_flags: [bool; 3],
    },
    /// Explicit sweep layout controlled by a support surface.
    ExplicitSurface {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile_frame"
        )]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [R; 2],
        /// Native trajectory scalar.
        path_parameter: R,
        /// Native singularity enum.
        singularity: i64,
        /// Support surface controlling the sweep.
        support_surface: SurfaceId,
        /// Optional auxiliary curve.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_auxiliary_curve"
        )]
        auxiliary_curve: Option<CurveId>,
        /// Native support-side boolean.
        support_flag: bool,
        /// Legacy pre-219 trailing boolean when present.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_legacy_flag"
        )]
        legacy_flag: Option<bool>,
    },
    /// Explicit-prefix sweep layout controlled by recursive laws.
    LawDriven {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [R; 2],
        /// Optional explicit profile frame.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_profile_frame"
        )]
        profile_frame: Option<(P, V)>,
        /// Sweep frame origin.
        origin: P,
        /// Three ordered sweep frame directions.
        directions: [V; 3],
        /// Leading recursive sweep law.
        first_law: Box<LawExpression<R, V, P>>,
        /// Native integer after the leading law.
        first_mode: i64,
        /// First law parameter interval.
        first_range: [R; 2],
        /// Native law direction.
        law_direction: V,
        /// Native path integer.
        path_mode: i64,
        /// Native path boolean.
        path_flag: bool,
        /// Path parameter interval.
        path_range: [R; 2],
        /// Native path scalar.
        path_parameter: R,
        /// Native second-law boolean.
        second_law_flag: bool,
        /// Trailing recursive sweep law.
        second_law: Box<LawExpression<R, V, P>>,
        /// Native integer before the formula.
        formula_mode: i64,
        /// Parametric trajectory formula.
        formula: LawFormula<R, V, P>,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
}

/// Revision-gated `sweep_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct SweepRevisionForm<R = f64> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Boolean replacing the pre-revision primary enum.
    pub primary_flag: bool,
    /// Optional parameter endpoints following the embedded profile curve.
    pub profile_endpoints: [Option<R>; 2],
    /// Optional parameter endpoints following the embedded path curve.
    pub path_endpoints: [Option<R>; 2],
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
}

/// Complete native `sweep_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct SweepSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Leading native sweep enum.
    pub primary_kind: i64,
    /// Cache contract: the revision-gated form, or the legacy solved-cache
    /// tolerance this construction states instead.
    pub cache: CacheContract<SweepRevisionForm<R>>,
    /// Structurally selected sweep layout.
    pub layout: SweepSurfaceLayout<R, V, P>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

cadmpeg_core::named_optional_field!(deserialize_profile_frame<P, V>, (P, V), "profile_frame");
cadmpeg_core::named_optional_field!(deserialize_auxiliary_curve, CurveId, "auxiliary_curve");
cadmpeg_core::named_optional_field!(deserialize_legacy_flag, bool, "legacy_flag");
