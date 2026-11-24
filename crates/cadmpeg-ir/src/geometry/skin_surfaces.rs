// SPDX-License-Identifier: Apache-2.0
//! Skin and net surface constructions.

use super::laws::LawFormula;
use super::lofts::{ClassicLoftProfileData, LoftSection, LoftSubdata};
use crate::ids::CurveId;
use crate::math::{Point3, Vector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One profile entry in the expanded skin layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub struct SkinSurfaceProfile<R = f64, V = Vector3> {
    /// Native profile type integer.
    pub type_code: i64,
    /// Profile curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData<R, V>,
}

/// Structurally selected native skin payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
pub enum SkinSurfaceLayout<R = f64, V = Vector3> {
    /// Expanded sequence of profile curves and loft constraints.
    Profiles {
        /// Ordered profile entries.
        profiles: Vec<SkinSurfaceProfile<R, V>>,
        /// Trailing path curve.
        path: CurveId,
        /// Two native trailing integers.
        tail: [i64; 2],
    },
    /// Compact curve/subdata form.
    Compact {
        /// Native compact-layout inner integer.
        inner_count: i64,
        /// Primary curve.
        curve: CurveId,
        /// Native loft subdata.
        subdata: LoftSubdata<R>,
        /// Integer after the subdata.
        first_tail: i64,
        /// Secondary curve.
        secondary_curve: CurveId,
        /// Final compact-layout integer.
        second_tail: i64,
    },
}

impl<R, V> SkinSurfaceLayout<R, V> {
    /// Native inner count, derived from the profile list in the expanded form.
    pub fn inner_count(&self) -> Result<i64, cadmpeg_core::CodecError> {
        match self {
            Self::Profiles { profiles, .. } => i64::try_from(profiles.len()).map_err(|_| {
                cadmpeg_core::CodecError::malformed(
                    "skin surface profile count exceeds the native integer range",
                )
            }),
            Self::Compact { inner_count, .. } => Ok(*inner_count),
        }
    }
}

/// Complete native `skin_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct SkinSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Native `SURF_BOOL` enum.
    pub surface_boolean: i64,
    /// Native `SURF_NORM` enum.
    pub surface_normal: i64,
    /// Native `SURF_DIR` enum.
    pub surface_direction: i64,
    /// Native leading count.
    pub count: i64,
    /// Native leading scalar.
    pub parameter: R,
    /// Structurally selected skin payload.
    pub layout: SkinSurfaceLayout<R, V>,
    /// Stored direction vector.
    pub direction: V,
    /// Native scalar before the formula.
    pub trailing_parameter: R,
    /// Recursive parametric law.
    pub formula: LawFormula<R, V, P>,
    /// Trailing curve after the formula.
    pub parameter_curve: CurveId,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Complete native `net_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct NetSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Two ordered loft-section graphs.
    pub sections: Box<[LoftSection<R, V>; 2]>,
    /// Twelve ordered frame scalars.
    pub frame_parameters: [R; 12],
    /// Native frame integer.
    pub flag: i64,
    /// Four ordered frame directions.
    pub directions: [V; 4],
    /// Four ordered parameter laws.
    pub formulas: Box<[LawFormula<R, V, P>; 4]>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}
