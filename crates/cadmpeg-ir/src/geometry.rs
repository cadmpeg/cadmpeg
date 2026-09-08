// SPDX-License-Identifier: Apache-2.0
//! Geometry carriers: analytic surfaces, analytic curves, NURBS, and pcurves.
//!
//! Carriers are stored in their own arenas and referenced by id from the
//! topology graph (a face references a [`Surface`], an edge a [`Curve`], a
//! coedge a [`Pcurve`]). One carrier may therefore support several topological
//! entities.

use crate::ids::{CurveId, PcurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
use crate::math::{Point2, Point3, Vector3};
use crate::provenance::SourceObjectAssociation;
use crate::transform::{Transform, Transform2};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{ser::SerializeStruct, Deserialize, Serialize};
use std::num::NonZeroI64;

fn default_true() -> bool {
    true
}

/// Parameter-space continuation used when an offset evaluates beyond its
/// support's active NURBS rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OffsetSupportExtension {
    /// Continue the support's terminal polynomial patch.
    Natural,
    /// Continue boundary tangents as ruled linear strips.
    Linear,
}

/// Admitted conditional flag shapes in the pre-revision offset-surface layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyExtensionFlags {
    /// The compact `offsur` layout has no extension flag.
    Absent,
    /// The extension gate is false, so no dependent flags follow it.
    Disabled,
    /// The extension gate is true and carries its required flag and optional
    /// later-revision flag.
    Enabled {
        /// Required flag following the true extension gate.
        secondary: bool,
        /// Optional flag admitted by the later legacy layout.
        tertiary: Option<bool>,
    },
}

impl LegacyExtensionFlags {
    /// Return the legacy wire sequence.
    #[must_use]
    pub fn wire_values(self) -> Vec<bool> {
        match self {
            Self::Absent => Vec::new(),
            Self::Disabled => vec![false],
            Self::Enabled {
                secondary,
                tertiary,
            } => {
                let mut flags = vec![true, secondary];
                flags.extend(tertiary);
                flags
            }
        }
    }
}

impl TryFrom<Vec<bool>> for LegacyExtensionFlags {
    type Error = Vec<bool>;

    fn try_from(flags: Vec<bool>) -> Result<Self, Self::Error> {
        match flags.as_slice() {
            [] => Ok(Self::Absent),
            [false] => Ok(Self::Disabled),
            [true, secondary] => Ok(Self::Enabled {
                secondary: *secondary,
                tertiary: None,
            }),
            [true, secondary, tertiary] => Ok(Self::Enabled {
                secondary: *secondary,
                tertiary: Some(*tertiary),
            }),
            _ => Err(flags),
        }
    }
}

impl Serialize for LegacyExtensionFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.wire_values().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LegacyExtensionFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::try_from(Vec::<bool>::deserialize(deserializer)?).map_err(|_| {
            serde::de::Error::custom(
                "extension_flags must be [], [false], [true, flag], or [true, flag, flag]",
            )
        })
    }
}

/// Mutually exclusive pre-revision and revision-gated offset layouts.
#[derive(Debug, Clone, PartialEq)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
pub enum OffsetExtension {
    /// Pre-revision conditional flag sequence.
    Legacy(LegacyExtensionFlags),
    /// Revision-gated fields with the required four-boolean carrier run.
    Revision(RevisionSurfaceForm<[bool; 4]>),
}

#[derive(Serialize)]
struct OffsetExtensionWriteWire<'a> {
    extension_flags: Vec<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_form: Option<&'a RevisionSurfaceForm<[bool; 4]>>,
}

#[derive(Deserialize)]
struct OffsetExtensionReadWire {
    extension_flags: Vec<bool>,
    #[serde(default)]
    revision_form: Option<RevisionSurfaceForm>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the offset-extension wire schema")]
struct OffsetExtensionSchemaWire {
    extension_flags: Vec<bool>,
    revision_form: Option<RevisionSurfaceForm<[bool; 4]>>,
}

impl Serialize for OffsetExtension {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (extension_flags, revision_form) = match self {
            Self::Legacy(flags) => (flags.wire_values(), None),
            Self::Revision(form) => (Vec::new(), Some(form)),
        };
        OffsetExtensionWriteWire {
            extension_flags,
            revision_form,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OffsetExtension {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = OffsetExtensionReadWire::deserialize(deserializer)?;
        match wire.revision_form {
            None => LegacyExtensionFlags::try_from(wire.extension_flags)
                .map(Self::Legacy)
                .map_err(|_| {
                    serde::de::Error::custom(
                        "extension_flags must be [], [false], [true, flag], or [true, flag, flag]",
                    )
                }),
            Some(mut form) if wire.extension_flags.is_empty() => {
                let flags: [bool; 4] = std::mem::take(&mut form.flags).try_into().map_err(|_| {
                    serde::de::Error::custom(
                        "revision_form.flags must contain exactly four booleans for an offset surface",
                    )
                })?;
                Ok(Self::Revision(form.with_flags(flags)))
            }
            Some(_) => Err(serde::de::Error::custom(
                "extension_flags must be empty when revision_form is present",
            )),
        }
    }
}

/// A tensor-product NURBS surface.
///
/// Control points use u-major order. `weights == None` denotes a non-rational
/// surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct NurbsSurface {
    /// Degree in the u parametric direction.
    u_degree: u32,
    /// Degree in the v parametric direction.
    v_degree: u32,
    /// Full knot vector in u.
    u_knots: Vec<f64>,
    /// Full knot vector in v.
    v_knots: Vec<f64>,
    /// Number of control points along u (poles per row).
    u_count: u32,
    /// Number of control points along v (poles per column).
    v_count: u32,
    /// Control points, u-major: index `i*v_count + j` is pole `(i, j)`.
    control_points: Vec<Point3>,
    /// Per-pole weights in control-point order; `None` denotes non-rational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weights: Option<Vec<f64>>,
    /// Whether the carrier's oriented normal is opposite `Pu × Pv`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    normal_reversed: bool,
    /// Whether the surface is periodic in u.
    u_periodic: bool,
    /// Whether the surface is periodic in v.
    v_periodic: bool,
}

/// Polynomial tensor-product B-spline surface with a rectangular control grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BsplineSurface {
    u_degree: u32,
    v_degree: u32,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    control_points: Vec<Vec<Point3>>,
}

impl BsplineSurface {
    /// Build a rectangular grid with full knot vectors for both parameters.
    // Both parameter axes and their shared control grid form one NURBS invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        control_points: Vec<Vec<Point3>>,
    ) -> Result<Self, NurbsError> {
        let u_count = control_points.len();
        let v_count = control_points.first().map_or(0, Vec::len);
        for (axis, degree, count, knots) in [
            ("u", u_degree, u_count, &u_knots),
            ("v", v_degree, v_count, &v_knots),
        ] {
            if count <= degree as usize {
                return Err(NurbsError(format!(
                    "control_points {axis} count must exceed degree {degree}, found {count}"
                )));
            }
            require_length(
                &format!("{axis}_knots"),
                knots.len(),
                checked_knot_count(axis, count, degree)?,
            )?;
            require_nondecreasing_knots(knots)?;
        }
        for row in &control_points {
            require_length("control_points row", row.len(), v_count)?;
            require_finite_points_3("control_points", row)?;
        }
        Ok(Self {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            control_points,
        })
    }

    /// Degree in the first parameter.
    pub const fn u_degree(&self) -> u32 {
        self.u_degree
    }

    /// Degree in the second parameter.
    pub const fn v_degree(&self) -> u32 {
        self.v_degree
    }

    /// Full knot vector in the first parameter.
    pub fn u_knots(&self) -> &[f64] {
        &self.u_knots
    }

    /// Full knot vector in the second parameter.
    pub fn v_knots(&self) -> &[f64] {
        &self.v_knots
    }

    /// Rectangular control grid in first-parameter-major order.
    pub fn control_points(&self) -> &[Vec<Point3>] {
        &self.control_points
    }

    /// Atomically edit pole coordinates while preserving the grid and finite values.
    pub fn edit_control_points(
        &mut self,
        mut edit: impl FnMut(&mut Point3),
    ) -> Result<(), NurbsError> {
        let mut points = self.control_points.clone();
        for row in &mut points {
            for point in row.iter_mut() {
                edit(point);
            }
            require_finite_points_3("control_points", row)?;
        }
        self.control_points = points;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for BsplineSurface {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            u_degree: u32,
            v_degree: u32,
            u_knots: Vec<f64>,
            v_knots: Vec<f64>,
            control_points: Vec<Vec<Point3>>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.u_degree,
            wire.v_degree,
            wire.u_knots,
            wire.v_knots,
            wire.control_points,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Structural error in a NURBS knot or pole carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NurbsError(String);

impl std::fmt::Display for NurbsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for NurbsError {}

fn checked_knot_count(field: &str, pole_count: usize, degree: u32) -> Result<usize, NurbsError> {
    pole_count
        .checked_add(degree as usize)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| NurbsError(format!("{field} knot count overflows usize")))
}

fn require_length(field: &str, actual: usize, expected: usize) -> Result<(), NurbsError> {
    if actual == expected {
        Ok(())
    } else {
        Err(NurbsError(format!(
            "{field} must contain {expected} values, found {actual}"
        )))
    }
}

fn require_finite_points_2(field: &str, points: &[Point2]) -> Result<(), NurbsError> {
    if points
        .iter()
        .all(|point| point.u.is_finite() && point.v.is_finite())
    {
        Ok(())
    } else {
        Err(NurbsError(format!("{field} contains a non-finite point")))
    }
}

fn require_finite_points_3(field: &str, points: &[Point3]) -> Result<(), NurbsError> {
    if points
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
    {
        Ok(())
    } else {
        Err(NurbsError(format!("{field} contains a non-finite point")))
    }
}

fn require_finite_scalars(field: &str, values: &[f64]) -> Result<(), NurbsError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(NurbsError(format!("{field} contains a non-finite value")))
    }
}

fn require_nondecreasing_knots(knots: &[f64]) -> Result<(), NurbsError> {
    require_finite_scalars("knots", knots)?;
    if knots_nondecreasing(knots) {
        Ok(())
    } else {
        Err(NurbsError("knots must be non-decreasing".into()))
    }
}

fn require_3d_weights(weights: &[f64]) -> Result<(), NurbsError> {
    if weights
        .iter()
        .all(|weight| weight.is_finite() && *weight != 0.0)
    {
        Ok(())
    } else {
        Err(NurbsError(
            "3D NURBS weights must be finite and non-zero".into(),
        ))
    }
}

fn require_pcurve_weights(weights: &[f64]) -> Result<(), NurbsError> {
    if weights
        .iter()
        .all(|weight| weight.is_finite() && *weight > 0.0)
    {
        Ok(())
    } else {
        Err(NurbsError(
            "pcurve NURBS weights must be finite and positive".into(),
        ))
    }
}

fn require_curve_cardinality(
    degree: u32,
    knot_count: usize,
    pole_count: usize,
    weight_count: Option<usize>,
    point_field: &str,
) -> Result<(), NurbsError> {
    if pole_count <= degree as usize {
        return Err(NurbsError(format!(
            "{point_field} must contain more than degree {degree} poles, found {pole_count}"
        )));
    }
    require_length(
        "knots",
        knot_count,
        checked_knot_count("curve", pole_count, degree)?,
    )?;
    if let Some(weight_count) = weight_count {
        require_length("weights", weight_count, pole_count)?;
    }
    Ok(())
}

impl NurbsSurface {
    /// Build a tensor-product NURBS surface with consistent cardinalities.
    // Both parameter axes and their shared control grid form one NURBS invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        u_count: u32,
        v_count: u32,
        control_points: Vec<Point3>,
        weights: Option<Vec<f64>>,
        normal_reversed: bool,
        u_periodic: bool,
        v_periodic: bool,
    ) -> Result<Self, NurbsError> {
        if u_count <= u_degree {
            return Err(NurbsError(format!(
                "u_count must exceed u_degree {u_degree}, found {u_count}"
            )));
        }
        if v_count <= v_degree {
            return Err(NurbsError(format!(
                "v_count must exceed v_degree {v_degree}, found {v_count}"
            )));
        }
        let pole_count = (u_count as usize)
            .checked_mul(v_count as usize)
            .ok_or_else(|| NurbsError("surface pole count overflows usize".into()))?;
        require_length("control_points", control_points.len(), pole_count)?;
        require_length(
            "u_knots",
            u_knots.len(),
            checked_knot_count("u", u_count as usize, u_degree)?,
        )?;
        require_length(
            "v_knots",
            v_knots.len(),
            checked_knot_count("v", v_count as usize, v_degree)?,
        )?;
        require_finite_points_3("control_points", &control_points)?;
        require_nondecreasing_knots(&u_knots).map_err(|error| NurbsError(format!("u_{error}")))?;
        require_nondecreasing_knots(&v_knots).map_err(|error| NurbsError(format!("v_{error}")))?;
        if let Some(weights) = &weights {
            require_length("weights", weights.len(), pole_count)?;
            require_3d_weights(weights)?;
        }
        Ok(Self {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            u_count,
            v_count,
            control_points,
            weights,
            normal_reversed,
            u_periodic,
            v_periodic,
        })
    }

    /// Degree in the u parametric direction.
    pub const fn u_degree(&self) -> u32 {
        self.u_degree
    }

    /// Degree in the v parametric direction.
    pub const fn v_degree(&self) -> u32 {
        self.v_degree
    }

    /// Full knot vector in u.
    pub fn u_knots(&self) -> &[f64] {
        &self.u_knots
    }

    /// Full knot vector in v.
    pub fn v_knots(&self) -> &[f64] {
        &self.v_knots
    }

    /// Atomically edit the u knot vector and preserve its invariants.
    pub fn edit_u_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.u_knots.clone();
        edit(&mut values);
        require_nondecreasing_knots(&values).map_err(|error| NurbsError(format!("u_{error}")))?;
        self.u_knots = values;
        Ok(())
    }

    /// Atomically edit the v knot vector and preserve its invariants.
    pub fn edit_v_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.v_knots.clone();
        edit(&mut values);
        require_nondecreasing_knots(&values).map_err(|error| NurbsError(format!("v_{error}")))?;
        self.v_knots = values;
        Ok(())
    }

    /// Number of control points along u.
    pub const fn u_count(&self) -> u32 {
        self.u_count
    }

    /// Number of control points along v.
    pub const fn v_count(&self) -> u32 {
        self.v_count
    }

    /// Control points in u-major order.
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }

    /// Atomically edit control points and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnOnce(&mut [Point3]),
    ) -> Result<(), NurbsError> {
        let mut values = self.control_points.clone();
        edit(&mut values);
        require_finite_points_3("control_points", &values)?;
        self.control_points = values;
        Ok(())
    }

    /// Rational weights in control-point order.
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    /// Atomically edit rational weights and preserve their invariants.
    pub fn edit_weights(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let Some(weights) = &self.weights else {
            return Err(NurbsError("surface has no rational weights".into()));
        };
        let mut values = weights.clone();
        edit(&mut values);
        require_3d_weights(&values)?;
        self.weights = Some(values);
        Ok(())
    }

    /// Replace rational weights after checking pole cardinality.
    pub fn set_weights(&mut self, weights: Option<Vec<f64>>) -> Result<(), NurbsError> {
        if let Some(values) = &weights {
            require_length("weights", values.len(), self.control_points.len())?;
            require_3d_weights(values)?;
        }
        self.weights = weights;
        Ok(())
    }

    /// Whether the carrier's oriented normal is reversed.
    pub const fn normal_reversed(&self) -> bool {
        self.normal_reversed
    }

    /// Set whether the carrier's oriented normal is reversed.
    pub fn set_normal_reversed(&mut self, value: bool) {
        self.normal_reversed = value;
    }

    /// Whether the surface is periodic in u.
    pub const fn u_periodic(&self) -> bool {
        self.u_periodic
    }

    /// Set whether the surface is periodic in u.
    pub fn set_u_periodic(&mut self, value: bool) {
        self.u_periodic = value;
    }

    /// Whether the surface is periodic in v.
    pub const fn v_periodic(&self) -> bool {
        self.v_periodic
    }

    /// Set whether the surface is periodic in v.
    pub fn set_v_periodic(&mut self, value: bool) {
        self.v_periodic = value;
    }

    /// Exchange the u and v parameter axes and transpose pole storage.
    /// The natural normal changes sign; `normal_reversed` remains unchanged.
    pub fn transpose_parameter_axes(&mut self) {
        let old_u = self.u_count as usize;
        let old_v = self.v_count as usize;
        let old_points = std::mem::take(&mut self.control_points);
        let old_weights = std::mem::take(&mut self.weights);
        self.control_points = Vec::with_capacity(old_points.len());
        self.weights = old_weights
            .as_ref()
            .map(|_| Vec::with_capacity(old_points.len()));
        for new_u in 0..old_v {
            for new_v in 0..old_u {
                let old_index = new_v * old_v + new_u;
                self.control_points.push(old_points[old_index]);
                if let (Some(source), Some(target)) = (&old_weights, &mut self.weights) {
                    target.push(source[old_index]);
                }
            }
        }
        std::mem::swap(&mut self.u_degree, &mut self.v_degree);
        std::mem::swap(&mut self.u_knots, &mut self.v_knots);
        std::mem::swap(&mut self.u_count, &mut self.v_count);
        std::mem::swap(&mut self.u_periodic, &mut self.v_periodic);
    }
}

impl<'de> Deserialize<'de> for NurbsSurface {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            u_degree: u32,
            v_degree: u32,
            u_knots: Vec<f64>,
            v_knots: Vec<f64>,
            u_count: u32,
            v_count: u32,
            control_points: Vec<Point3>,
            #[serde(default)]
            weights: Option<Vec<f64>>,
            #[serde(default)]
            normal_reversed: bool,
            u_periodic: bool,
            v_periodic: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.u_degree,
            wire.v_degree,
            wire.u_knots,
            wire.v_knots,
            wire.u_count,
            wire.v_count,
            wire.control_points,
            wire.weights,
            wire.normal_reversed,
            wire.u_periodic,
            wire.v_periodic,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Fixed parameter axis used to extract an isoparametric surface curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceParameterAxis {
    /// Hold the surface U parameter constant and vary V.
    U,
    /// Hold the surface V parameter constant and vary U.
    V,
}

/// A NURBS curve knot/pole payload.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct NurbsCurve {
    /// Curve degree.
    degree: u32,
    /// Full knot vector.
    knots: Vec<f64>,
    /// Control points in parameter order.
    control_points: Vec<Point3>,
    /// Per-pole weights; `None` denotes non-rational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weights: Option<Vec<f64>>,
    /// Whether the curve is periodic.
    periodic: bool,
}

impl NurbsCurve {
    /// Build a NURBS curve with consistent knot, pole, and weight cardinalities.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point3>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(
            degree,
            knots.len(),
            control_points.len(),
            weights.as_ref().map(Vec::len),
            "control_points",
        )?;
        require_finite_points_3("control_points", &control_points)?;
        require_nondecreasing_knots(&knots)?;
        if let Some(weights) = &weights {
            require_3d_weights(weights)?;
        }
        Ok(Self {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        })
    }

    /// Curve degree.
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Full knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Atomically edit knot values and preserve their invariants.
    pub fn edit_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.knots.clone();
        edit(&mut values);
        require_nondecreasing_knots(&values)?;
        self.knots = values;
        Ok(())
    }

    /// Control points in parameter order.
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }

    /// Atomically edit control points and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnOnce(&mut [Point3]),
    ) -> Result<(), NurbsError> {
        let mut values = self.control_points.clone();
        edit(&mut values);
        require_finite_points_3("control_points", &values)?;
        self.control_points = values;
        Ok(())
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    /// Atomically edit rational weights and preserve their invariants.
    pub fn edit_weights(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let Some(weights) = &self.weights else {
            return Err(NurbsError("curve has no rational weights".into()));
        };
        let mut values = weights.clone();
        edit(&mut values);
        require_3d_weights(&values)?;
        self.weights = Some(values);
        Ok(())
    }

    /// Replace rational weights after checking pole cardinality.
    pub fn set_weights(&mut self, weights: Option<Vec<f64>>) -> Result<(), NurbsError> {
        if let Some(values) = &weights {
            require_length("weights", values.len(), self.control_points.len())?;
            require_3d_weights(values)?;
        }
        self.weights = weights;
        Ok(())
    }

    /// Whether the curve is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Set whether the curve is periodic.
    pub fn set_periodic(&mut self, value: bool) {
        self.periodic = value;
    }

    /// Reverse poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.control_points.reverse();
        if let Some(weights) = &mut self.weights {
            weights.reverse();
        }
        self.knots.reverse();
        for knot in &mut self.knots {
            *knot = -*knot;
        }
    }
}

impl<'de> Deserialize<'de> for NurbsCurve {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            degree: u32,
            knots: Vec<f64>,
            control_points: Vec<Point3>,
            #[serde(default)]
            weights: Option<Vec<f64>>,
            periodic: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.degree,
            wire.knots,
            wire.control_points,
            wire.weights,
            wire.periodic,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// True when each knot is at least as large as the previous (repeats allowed).
///
/// A NaN pair fails this predicate. Prefer this form when the site already
/// used `windows(2).all(|pair| pair[0] <= pair[1])`.
pub fn knots_nondecreasing(knots: &[f64]) -> bool {
    knots.windows(2).all(|pair| pair[0] <= pair[1])
}

/// True when each knot is strictly larger than the previous.
///
/// A NaN pair fails this predicate. Prefer this form when the site already
/// used `windows(2).all(|pair| pair[0] < pair[1])`.
pub fn knots_strictly_increasing(knots: &[f64]) -> bool {
    knots.windows(2).all(|pair| pair[0] < pair[1])
}

/// Structural error in a sampled polyline or polygonal carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeometryLayoutError(String);

impl std::fmt::Display for GeometryLayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GeometryLayoutError {}

fn geometry_layout_error(message: impl Into<String>) -> GeometryLayoutError {
    GeometryLayoutError(message.into())
}

/// Source-native polygonal surface with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PolygonalSurface {
    vertices: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
    chordal_deflection: f64,
}

impl PolygonalSurface {
    /// Build a polygonal surface whose triangle indices address `vertices`.
    pub fn new(
        vertices: Vec<Point3>,
        triangles: Vec<[u32; 3]>,
        chordal_deflection: f64,
    ) -> Result<Self, GeometryLayoutError> {
        if vertices.len() < 3 {
            return Err(geometry_layout_error(
                "polygonal surface must contain at least three vertices",
            ));
        }
        if triangles.is_empty() {
            return Err(geometry_layout_error(
                "polygonal surface must contain at least one triangle",
            ));
        }
        if triangles
            .iter()
            .flatten()
            .any(|index| *index as usize >= vertices.len())
        {
            return Err(geometry_layout_error(
                "polygonal surface contains an out-of-range triangle index",
            ));
        }
        if vertices
            .iter()
            .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
        {
            return Err(geometry_layout_error("vertices must be finite"));
        }
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        Ok(Self {
            vertices,
            triangles,
            chordal_deflection,
        })
    }

    /// Ordered model-space vertices.
    #[must_use]
    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }

    /// Edit finite vertices transactionally.
    pub fn edit_vertices(
        &mut self,
        edit: impl FnOnce(&mut [Point3]),
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.vertices.clone();
        edit(&mut candidate);
        *self = Self::new(candidate, self.triangles.clone(), self.chordal_deflection)?;
        Ok(())
    }

    /// Zero-based triangle indices into [`Self::vertices`].
    #[must_use]
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// Maximum chordal deviation recorded by the source.
    #[must_use]
    pub const fn chordal_deflection(&self) -> f64 {
        self.chordal_deflection
    }

    /// Set the recorded chordal deviation.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: f64,
    ) -> Result<(), GeometryLayoutError> {
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        self.chordal_deflection = chordal_deflection;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for PolygonalSurface {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            vertices: Vec<Point3>,
            triangles: Vec<[u32; 3]>,
            chordal_deflection: f64,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.vertices, wire.triangles, wire.chordal_deflection)
            .map_err(serde::de::Error::custom)
    }
}

/// Source-native polyline with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PolylineCurve {
    points: Vec<Point3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameters: Option<Vec<f64>>,
    chordal_deflection: f64,
}

impl PolylineCurve {
    /// Build a polyline whose optional parameters match the sample count.
    pub fn new(
        points: Vec<Point3>,
        parameters: Option<Vec<f64>>,
        chordal_deflection: f64,
    ) -> Result<Self, GeometryLayoutError> {
        if points.len() < 2 {
            return Err(geometry_layout_error(
                "polyline must contain at least two points",
            ));
        }
        if let Some(parameters) = &parameters {
            if parameters.len() != points.len() {
                return Err(geometry_layout_error(
                    "polyline parameters do not match the point count",
                ));
            }
        }
        if points
            .iter()
            .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
        {
            return Err(geometry_layout_error("points must be finite"));
        }
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        if parameters.as_ref().is_some_and(|parameters| {
            !parameters.iter().all(|value| value.is_finite())
                || !(parameters.windows(2).all(|pair| pair[0] < pair[1])
                    || parameters.windows(2).all(|pair| pair[0] > pair[1]))
        }) {
            return Err(geometry_layout_error(
                "parameters must be finite and strictly monotonic",
            ));
        }
        Ok(Self {
            points,
            parameters,
            chordal_deflection,
        })
    }

    /// Ordered model-space samples.
    #[must_use]
    pub fn points(&self) -> &[Point3] {
        &self.points
    }

    /// Edit finite points transactionally.
    pub fn edit_points(
        &mut self,
        edit: impl FnOnce(&mut [Point3]),
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.points.clone();
        edit(&mut candidate);
        *self = Self::new(candidate, self.parameters.clone(), self.chordal_deflection)?;
        Ok(())
    }

    /// Optional source parameters parallel to [`Self::points`].
    #[must_use]
    pub fn parameters(&self) -> Option<&[f64]> {
        self.parameters.as_deref()
    }

    /// Edit finite strictly monotonic source parameters transactionally.
    pub fn edit_parameters(
        &mut self,
        edit: impl FnOnce(Option<&mut [f64]>),
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.parameters.clone();
        edit(candidate.as_deref_mut());
        *self = Self::new(self.points.clone(), candidate, self.chordal_deflection)?;
        Ok(())
    }

    /// Maximum chordal deviation recorded by the source.
    #[must_use]
    pub const fn chordal_deflection(&self) -> f64 {
        self.chordal_deflection
    }

    /// Set the recorded chordal deviation.
    pub fn set_chordal_deflection(
        &mut self,
        chordal_deflection: f64,
    ) -> Result<(), GeometryLayoutError> {
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        self.chordal_deflection = chordal_deflection;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for PolylineCurve {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            points: Vec<Point3>,
            #[serde(default)]
            parameters: Option<Vec<f64>>,
            chordal_deflection: f64,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.points, wire.parameters, wire.chordal_deflection)
            .map_err(serde::de::Error::custom)
    }
}

const EPS_ANALYTIC_FRAME: f64 = 1.0e-9;

fn analytic_unit_vector(value: Vector3) -> bool {
    (value.norm() - 1.0).abs() <= EPS_ANALYTIC_FRAME
}

fn analytic_frame(axis: Vector3, reference: Vector3) -> bool {
    analytic_unit_vector(axis)
        && analytic_unit_vector(reference)
        && axis.dot(reference).abs() <= EPS_ANALYTIC_FRAME
}

/// Plane with a finite origin and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PlaneSurfaceWire")]
pub struct PlaneSurface {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PlaneSurfaceWire {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

impl PlaneSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, normal: Vector3, u_axis: Vector3) -> Result<Self, &'static str> {
        if !(origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite()) {
            return Err("PlaneSurface.origin must be finite");
        }
        if !analytic_frame(normal, u_axis) {
            return Err("PlaneSurface.normal/u_axis must form an orthonormal frame");
        }
        Ok(Self {
            origin,
            normal,
            u_axis,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3) {
        (&self.origin, &self.normal, &self.u_axis)
    }
}

impl TryFrom<PlaneSurfaceWire> for PlaneSurface {
    type Error = &'static str;
    fn try_from(wire: PlaneSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.normal, wire.u_axis)
    }
}

/// Circular cylinder with a positive radius and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CylinderSurfaceWire")]
pub struct CylinderSurface {
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CylinderSurfaceWire {
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

impl CylinderSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        if !(origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite()) {
            return Err("CylinderSurface.origin must be finite");
        }
        if !radius.is_finite() {
            return Err("CylinderSurface.radius must be finite");
        }
        if !analytic_frame(axis, ref_direction) {
            return Err("CylinderSurface.axis/ref_direction must form an orthonormal frame");
        }
        if radius <= 0.0 {
            return Err("CylinderSurface.radius must be positive");
        }
        Ok(Self {
            origin,
            axis,
            ref_direction,
            radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64) {
        (&self.origin, &self.axis, &self.ref_direction, &self.radius)
    }
}

impl TryFrom<CylinderSurfaceWire> for CylinderSurface {
    type Error = &'static str;
    fn try_from(wire: CylinderSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.axis, wire.ref_direction, wire.radius)
    }
}

/// Elliptical cone with finite parameters and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ConeSurfaceWire")]
pub struct ConeSurface {
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    ratio: f64,
    half_angle: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ConeSurfaceWire {
    origin: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    ratio: f64,
    half_angle: f64,
}

impl ConeSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        origin: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
        ratio: f64,
        half_angle: f64,
    ) -> Result<Self, &'static str> {
        if !(origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite()) {
            return Err("ConeSurface.origin must be finite");
        }
        if !radius.is_finite() {
            return Err("ConeSurface.radius must be finite");
        }
        if !ratio.is_finite() {
            return Err("ConeSurface.ratio must be finite");
        }
        if !half_angle.is_finite() {
            return Err("ConeSurface.half_angle must be finite");
        }
        if !analytic_frame(axis, ref_direction) {
            return Err("ConeSurface.axis/ref_direction must form an orthonormal frame");
        }
        if radius < 0.0 {
            return Err("ConeSurface.radius must be nonnegative");
        }
        if ratio <= 0.0 {
            return Err("ConeSurface.ratio must be positive");
        }
        Ok(Self {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64, &f64, &f64) {
        (
            &self.origin,
            &self.axis,
            &self.ref_direction,
            &self.radius,
            &self.ratio,
            &self.half_angle,
        )
    }
}

impl TryFrom<ConeSurfaceWire> for ConeSurface {
    type Error = &'static str;
    fn try_from(wire: ConeSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.origin,
            wire.axis,
            wire.ref_direction,
            wire.radius,
            wire.ratio,
            wire.half_angle,
        )
    }
}

/// Sphere with a signed nonzero radius and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SphereSurfaceWire")]
pub struct SphereSurface {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SphereSurfaceWire {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

impl SphereSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.x.is_finite() && center.y.is_finite() && center.z.is_finite()) {
            return Err("SphereSurface.center must be finite");
        }
        if !radius.is_finite() {
            return Err("SphereSurface.radius must be finite");
        }
        if !analytic_frame(axis, ref_direction) {
            return Err("SphereSurface.axis/ref_direction must form an orthonormal frame");
        }
        if radius == 0.0 {
            return Err("SphereSurface.radius must be nonzero");
        }
        Ok(Self {
            center,
            axis,
            ref_direction,
            radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64) {
        (&self.center, &self.axis, &self.ref_direction, &self.radius)
    }
}

impl TryFrom<SphereSurfaceWire> for SphereSurface {
    type Error = &'static str;
    fn try_from(wire: SphereSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.axis, wire.ref_direction, wire.radius)
    }
}

/// Torus with a positive major radius, signed tube radius, and orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TorusSurfaceWire")]
pub struct TorusSurface {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TorusSurfaceWire {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

impl TorusSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.x.is_finite() && center.y.is_finite() && center.z.is_finite()) {
            return Err("TorusSurface.center must be finite");
        }
        if !major_radius.is_finite() {
            return Err("TorusSurface.major_radius must be finite");
        }
        if !minor_radius.is_finite() {
            return Err("TorusSurface.minor_radius must be finite");
        }
        if !analytic_frame(axis, ref_direction) {
            return Err("TorusSurface.axis/ref_direction must form an orthonormal frame");
        }
        if major_radius <= 0.0 {
            return Err("TorusSurface.major_radius must be positive");
        }
        if minor_radius == 0.0 {
            return Err("TorusSurface.minor_radius must be nonzero");
        }
        Ok(Self {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64, &f64) {
        (
            &self.center,
            &self.axis,
            &self.ref_direction,
            &self.major_radius,
            &self.minor_radius,
        )
    }
}

impl TryFrom<TorusSurfaceWire> for TorusSurface {
    type Error = &'static str;
    fn try_from(wire: TorusSurfaceWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.axis,
            wire.ref_direction,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Line with a finite origin and a unit direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LineCurveWire")]
pub struct LineCurve {
    origin: Point3,
    direction: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LineCurveWire {
    origin: Point3,
    direction: Vector3,
}

impl LineCurve {
    /// Reverse the curve parameter direction.
    pub fn reverse_parameterization(&mut self) {
        self.direction = Vector3::new(-self.direction.x, -self.direction.y, -self.direction.z);
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, direction: Vector3) -> Result<Self, &'static str> {
        if !(origin.x.is_finite() && origin.y.is_finite() && origin.z.is_finite()) {
            return Err("LineCurve.origin must be finite");
        }
        if !analytic_unit_vector(direction) {
            return Err("LineCurve.direction must have unit length");
        }
        Ok(Self { origin, direction })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3) {
        (&self.origin, &self.direction)
    }
}

impl TryFrom<LineCurveWire> for LineCurve {
    type Error = &'static str;
    fn try_from(wire: LineCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.direction)
    }
}

/// Circle with a positive radius and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CircleCurveWire")]
pub struct CircleCurve {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CircleCurveWire {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

impl CircleCurve {
    /// Reverse the curve parameter direction.
    pub fn reverse_parameterization(&mut self) {
        self.axis = Vector3::new(-self.axis.x, -self.axis.y, -self.axis.z);
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.x.is_finite() && center.y.is_finite() && center.z.is_finite()) {
            return Err("CircleCurve.center must be finite");
        }
        if !radius.is_finite() {
            return Err("CircleCurve.radius must be finite");
        }
        if !analytic_frame(axis, ref_direction) {
            return Err("CircleCurve.axis/ref_direction must form an orthonormal frame");
        }
        if radius <= 0.0 {
            return Err("CircleCurve.radius must be positive");
        }
        Ok(Self {
            center,
            axis,
            ref_direction,
            radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64) {
        (&self.center, &self.axis, &self.ref_direction, &self.radius)
    }
}

impl TryFrom<CircleCurveWire> for CircleCurve {
    type Error = &'static str;
    fn try_from(wire: CircleCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.axis, wire.ref_direction, wire.radius)
    }
}

/// Ellipse with ordered positive radii and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "EllipseCurveWire")]
pub struct EllipseCurve {
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct EllipseCurveWire {
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

impl EllipseCurve {
    /// Reverse the curve parameter direction.
    pub fn reverse_parameterization(&mut self) {
        self.axis = Vector3::new(-self.axis.x, -self.axis.y, -self.axis.z);
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.x.is_finite() && center.y.is_finite() && center.z.is_finite()) {
            return Err("EllipseCurve.center must be finite");
        }
        if !major_radius.is_finite() {
            return Err("EllipseCurve.major_radius must be finite");
        }
        if !minor_radius.is_finite() {
            return Err("EllipseCurve.minor_radius must be finite");
        }
        if !analytic_frame(axis, major_direction) {
            return Err("EllipseCurve.axis/major_direction must form an orthonormal frame");
        }
        if major_radius <= 0.0 {
            return Err("EllipseCurve.major_radius must be positive");
        }
        if minor_radius <= 0.0 {
            return Err("EllipseCurve.minor_radius must be positive");
        }
        if major_radius < minor_radius {
            return Err("EllipseCurve.major_radius must be at least minor_radius");
        }
        Ok(Self {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64, &f64) {
        (
            &self.center,
            &self.axis,
            &self.major_direction,
            &self.major_radius,
            &self.minor_radius,
        )
    }
}

impl TryFrom<EllipseCurveWire> for EllipseCurve {
    type Error = &'static str;
    fn try_from(wire: EllipseCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.axis,
            wire.major_direction,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Parabola with a positive focal distance and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ParabolaCurveWire")]
pub struct ParabolaCurve {
    vertex: Point3,
    axis: Vector3,
    major_direction: Vector3,
    focal_distance: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ParabolaCurveWire {
    vertex: Point3,
    axis: Vector3,
    major_direction: Vector3,
    focal_distance: f64,
}

impl ParabolaCurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        vertex: Point3,
        axis: Vector3,
        major_direction: Vector3,
        focal_distance: f64,
    ) -> Result<Self, &'static str> {
        if !(vertex.x.is_finite() && vertex.y.is_finite() && vertex.z.is_finite()) {
            return Err("ParabolaCurve.vertex must be finite");
        }
        if !focal_distance.is_finite() {
            return Err("ParabolaCurve.focal_distance must be finite");
        }
        if !analytic_frame(axis, major_direction) {
            return Err("ParabolaCurve.axis/major_direction must form an orthonormal frame");
        }
        if focal_distance <= 0.0 {
            return Err("ParabolaCurve.focal_distance must be positive");
        }
        Ok(Self {
            vertex,
            axis,
            major_direction,
            focal_distance,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64) {
        (
            &self.vertex,
            &self.axis,
            &self.major_direction,
            &self.focal_distance,
        )
    }
}

impl TryFrom<ParabolaCurveWire> for ParabolaCurve {
    type Error = &'static str;
    fn try_from(wire: ParabolaCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.vertex,
            wire.axis,
            wire.major_direction,
            wire.focal_distance,
        )
    }
}

/// Hyperbola with positive radii and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HyperbolaCurveWire")]
pub struct HyperbolaCurve {
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HyperbolaCurveWire {
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
}

impl HyperbolaCurve {
    /// Return the opposite branch with unchanged radii.
    #[must_use]
    pub fn opposite_branch(mut self) -> Self {
        self.major_direction = Vector3::new(
            -self.major_direction.x,
            -self.major_direction.y,
            -self.major_direction.z,
        );
        self
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.x.is_finite() && center.y.is_finite() && center.z.is_finite()) {
            return Err("HyperbolaCurve.center must be finite");
        }
        if !major_radius.is_finite() {
            return Err("HyperbolaCurve.major_radius must be finite");
        }
        if !minor_radius.is_finite() {
            return Err("HyperbolaCurve.minor_radius must be finite");
        }
        if !analytic_frame(axis, major_direction) {
            return Err("HyperbolaCurve.axis/major_direction must form an orthonormal frame");
        }
        if major_radius <= 0.0 {
            return Err("HyperbolaCurve.major_radius must be positive");
        }
        if minor_radius <= 0.0 {
            return Err("HyperbolaCurve.minor_radius must be positive");
        }
        Ok(Self {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3, &Vector3, &Vector3, &f64, &f64) {
        (
            &self.center,
            &self.axis,
            &self.major_direction,
            &self.major_radius,
            &self.minor_radius,
        )
    }
}

impl TryFrom<HyperbolaCurveWire> for HyperbolaCurve {
    type Error = &'static str;
    fn try_from(wire: HyperbolaCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.axis,
            wire.major_direction,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Degenerate curve at a finite point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "DegenerateCurveWire")]
pub struct DegenerateCurve {
    point: Point3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DegenerateCurveWire {
    point: Point3,
}

impl DegenerateCurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(point: Point3) -> Result<Self, &'static str> {
        if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
            return Err("DegenerateCurve.point must be finite");
        }
        Ok(Self { point })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point3,) {
        (&self.point,)
    }
}

impl TryFrom<DegenerateCurveWire> for DegenerateCurve {
    type Error = &'static str;
    fn try_from(wire: DegenerateCurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.point)
    }
}

/// Parameter-space line with a finite origin and nonzero direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "LinePcurveWire")]
pub struct LinePcurve {
    origin: Point2,
    direction: Point2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LinePcurveWire {
    origin: Point2,
    direction: Point2,
}

impl LinePcurve {
    /// Unit-u line through the parameter-space origin.
    pub const U_AXIS: Self = Self {
        origin: Point2 { u: 0.0, v: 0.0 },
        direction: Point2 { u: 1.0, v: 0.0 },
    };

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point2, direction: Point2) -> Result<Self, &'static str> {
        if !(origin.u.is_finite() && origin.v.is_finite()) {
            return Err("LinePcurve.origin must be finite");
        }
        if !(direction.u.is_finite() && direction.v.is_finite()) {
            return Err("LinePcurve.direction must be finite");
        }
        if direction.u.hypot(direction.v) <= 0.0 {
            return Err("LinePcurve.direction must be nonzero");
        }
        Ok(Self { origin, direction })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2) {
        (&self.origin, &self.direction)
    }
}

impl TryFrom<LinePcurveWire> for LinePcurve {
    type Error = &'static str;
    fn try_from(wire: LinePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.origin, wire.direction)
    }
}

/// Polar harmonic curve with finite coefficients and nonzero radial variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PolarHarmonicPcurveWire")]
pub struct PolarHarmonicPcurve {
    radial_center: Point2,
    radial_cos: Point2,
    radial_sin: Point2,
    axial_origin: f64,
    axial_cos: f64,
    axial_sin: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PolarHarmonicPcurveWire {
    radial_center: Point2,
    radial_cos: Point2,
    radial_sin: Point2,
    axial_origin: f64,
    axial_cos: f64,
    axial_sin: f64,
}

impl PolarHarmonicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        radial_center: Point2,
        radial_cos: Point2,
        radial_sin: Point2,
        axial_origin: f64,
        axial_cos: f64,
        axial_sin: f64,
    ) -> Result<Self, &'static str> {
        if !(radial_center.u.is_finite() && radial_center.v.is_finite()) {
            return Err("PolarHarmonicPcurve.radial_center must be finite");
        }
        if !(radial_cos.u.is_finite() && radial_cos.v.is_finite()) {
            return Err("PolarHarmonicPcurve.radial_cos must be finite");
        }
        if !(radial_sin.u.is_finite() && radial_sin.v.is_finite()) {
            return Err("PolarHarmonicPcurve.radial_sin must be finite");
        }
        if !axial_origin.is_finite() {
            return Err("PolarHarmonicPcurve.axial_origin must be finite");
        }
        if !axial_cos.is_finite() {
            return Err("PolarHarmonicPcurve.axial_cos must be finite");
        }
        if !axial_sin.is_finite() {
            return Err("PolarHarmonicPcurve.axial_sin must be finite");
        }
        if !(radial_cos.u.hypot(radial_cos.v) > 0.0 || radial_sin.u.hypot(radial_sin.v) > 0.0) {
            return Err("PolarHarmonicPcurve.radial_cos/radial_sin must not both be zero");
        }
        Ok(Self {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2, &f64, &f64, &f64) {
        (
            &self.radial_center,
            &self.radial_cos,
            &self.radial_sin,
            &self.axial_origin,
            &self.axial_cos,
            &self.axial_sin,
        )
    }
}

impl TryFrom<PolarHarmonicPcurveWire> for PolarHarmonicPcurve {
    type Error = &'static str;
    fn try_from(wire: PolarHarmonicPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.radial_center,
            wire.radial_cos,
            wire.radial_sin,
            wire.axial_origin,
            wire.axial_cos,
            wire.axial_sin,
        )
    }
}

/// Spherical great-circle chart with finite coefficients and nonzero azimuth rate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "SphericalGreatCirclePcurveWire")]
pub struct SphericalGreatCirclePcurve {
    azimuth_origin: f64,
    azimuth_rate: f64,
    plane_phase: f64,
    plane_slope: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SphericalGreatCirclePcurveWire {
    azimuth_origin: f64,
    azimuth_rate: f64,
    plane_phase: f64,
    plane_slope: f64,
}

impl SphericalGreatCirclePcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        azimuth_origin: f64,
        azimuth_rate: f64,
        plane_phase: f64,
        plane_slope: f64,
    ) -> Result<Self, &'static str> {
        if !azimuth_origin.is_finite() {
            return Err("SphericalGreatCirclePcurve.azimuth_origin must be finite");
        }
        if !azimuth_rate.is_finite() {
            return Err("SphericalGreatCirclePcurve.azimuth_rate must be finite");
        }
        if !plane_phase.is_finite() {
            return Err("SphericalGreatCirclePcurve.plane_phase must be finite");
        }
        if !plane_slope.is_finite() {
            return Err("SphericalGreatCirclePcurve.plane_slope must be finite");
        }
        if azimuth_rate == 0.0 {
            return Err("SphericalGreatCirclePcurve.azimuth_rate must be nonzero");
        }
        Ok(Self {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&f64, &f64, &f64, &f64) {
        (
            &self.azimuth_origin,
            &self.azimuth_rate,
            &self.plane_phase,
            &self.plane_slope,
        )
    }
}

impl TryFrom<SphericalGreatCirclePcurveWire> for SphericalGreatCirclePcurve {
    type Error = &'static str;
    fn try_from(wire: SphericalGreatCirclePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.azimuth_origin,
            wire.azimuth_rate,
            wire.plane_phase,
            wire.plane_slope,
        )
    }
}

/// Parameter-space circle with a positive radius and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CirclePcurveWire")]
pub struct CirclePcurve {
    center: Point2,
    x_axis: Point2,
    y_axis: Point2,
    radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CirclePcurveWire {
    center: Point2,
    x_axis: Point2,
    y_axis: Point2,
    radius: f64,
}

impl CirclePcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.u.is_finite() && center.v.is_finite()) {
            return Err("CirclePcurve.center must be finite");
        }
        if !(x_axis.u.is_finite() && x_axis.v.is_finite()) {
            return Err("CirclePcurve.x_axis must be finite");
        }
        if !(y_axis.u.is_finite() && y_axis.v.is_finite()) {
            return Err("CirclePcurve.y_axis must be finite");
        }
        if !radius.is_finite() {
            return Err("CirclePcurve.radius must be finite");
        }
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("CirclePcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("CirclePcurve.y_axis must be nonzero");
        }
        if radius <= 0.0 {
            return Err("CirclePcurve.radius must be positive");
        }
        Ok(Self {
            center,
            x_axis,
            y_axis,
            radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2, &f64) {
        (&self.center, &self.x_axis, &self.y_axis, &self.radius)
    }
}

impl TryFrom<CirclePcurveWire> for CirclePcurve {
    type Error = &'static str;
    fn try_from(wire: CirclePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.x_axis, wire.y_axis, wire.radius)
    }
}

/// Parameter-space ellipse with positive radii and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "EllipsePcurveWire")]
pub struct EllipsePcurve {
    center: Point2,
    x_axis: Point2,
    y_axis: Point2,
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct EllipsePcurveWire {
    center: Point2,
    x_axis: Point2,
    y_axis: Point2,
    major_radius: f64,
    minor_radius: f64,
}

impl EllipsePcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.u.is_finite() && center.v.is_finite()) {
            return Err("EllipsePcurve.center must be finite");
        }
        if !(x_axis.u.is_finite() && x_axis.v.is_finite()) {
            return Err("EllipsePcurve.x_axis must be finite");
        }
        if !(y_axis.u.is_finite() && y_axis.v.is_finite()) {
            return Err("EllipsePcurve.y_axis must be finite");
        }
        if !major_radius.is_finite() {
            return Err("EllipsePcurve.major_radius must be finite");
        }
        if !minor_radius.is_finite() {
            return Err("EllipsePcurve.minor_radius must be finite");
        }
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("EllipsePcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("EllipsePcurve.y_axis must be nonzero");
        }
        if major_radius <= 0.0 {
            return Err("EllipsePcurve.major_radius must be positive");
        }
        if minor_radius <= 0.0 {
            return Err("EllipsePcurve.minor_radius must be positive");
        }
        Ok(Self {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2, &f64, &f64) {
        (
            &self.center,
            &self.x_axis,
            &self.y_axis,
            &self.major_radius,
            &self.minor_radius,
        )
    }
}

impl TryFrom<EllipsePcurveWire> for EllipsePcurve {
    type Error = &'static str;
    fn try_from(wire: EllipsePcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.x_axis,
            wire.y_axis,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Harmonic parameter-space curve with finite coefficients and nonzero variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HarmonicPcurveWire")]
pub struct HarmonicPcurve {
    center: Point2,
    cosine: Point2,
    sine: Point2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HarmonicPcurveWire {
    center: Point2,
    cosine: Point2,
    sine: Point2,
}

impl HarmonicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(center: Point2, cosine: Point2, sine: Point2) -> Result<Self, &'static str> {
        if !(center.u.is_finite() && center.v.is_finite()) {
            return Err("HarmonicPcurve.center must be finite");
        }
        if !(cosine.u.is_finite() && cosine.v.is_finite()) {
            return Err("HarmonicPcurve.cosine must be finite");
        }
        if !(sine.u.is_finite() && sine.v.is_finite()) {
            return Err("HarmonicPcurve.sine must be finite");
        }
        if !(cosine.u.hypot(cosine.v) > 0.0 || sine.u.hypot(sine.v) > 0.0) {
            return Err("HarmonicPcurve.cosine/sine must not both be zero");
        }
        Ok(Self {
            center,
            cosine,
            sine,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2) {
        (&self.center, &self.cosine, &self.sine)
    }
}

impl TryFrom<HarmonicPcurveWire> for HarmonicPcurve {
    type Error = &'static str;
    fn try_from(wire: HarmonicPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.cosine, wire.sine)
    }
}

/// Parameter-space parabola with a positive focal distance and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ParabolaPcurveWire")]
pub struct ParabolaPcurve {
    vertex: Point2,
    x_axis: Point2,
    y_axis: Point2,
    focal_distance: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ParabolaPcurveWire {
    vertex: Point2,
    x_axis: Point2,
    y_axis: Point2,
    focal_distance: f64,
}

impl ParabolaPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        vertex: Point2,
        x_axis: Point2,
        y_axis: Point2,
        focal_distance: f64,
    ) -> Result<Self, &'static str> {
        if !(vertex.u.is_finite() && vertex.v.is_finite()) {
            return Err("ParabolaPcurve.vertex must be finite");
        }
        if !(x_axis.u.is_finite() && x_axis.v.is_finite()) {
            return Err("ParabolaPcurve.x_axis must be finite");
        }
        if !(y_axis.u.is_finite() && y_axis.v.is_finite()) {
            return Err("ParabolaPcurve.y_axis must be finite");
        }
        if !focal_distance.is_finite() {
            return Err("ParabolaPcurve.focal_distance must be finite");
        }
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("ParabolaPcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("ParabolaPcurve.y_axis must be nonzero");
        }
        if focal_distance <= 0.0 {
            return Err("ParabolaPcurve.focal_distance must be positive");
        }
        Ok(Self {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2, &f64) {
        (
            &self.vertex,
            &self.x_axis,
            &self.y_axis,
            &self.focal_distance,
        )
    }
}

impl TryFrom<ParabolaPcurveWire> for ParabolaPcurve {
    type Error = &'static str;
    fn try_from(wire: ParabolaPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.vertex, wire.x_axis, wire.y_axis, wire.focal_distance)
    }
}

/// Parameter-space hyperbola with positive radii and finite nonzero axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HyperbolaPcurveWire")]
pub struct HyperbolaPcurve {
    center: Point2,
    x_axis: Point2,
    y_axis: Point2,
    major_radius: f64,
    minor_radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HyperbolaPcurveWire {
    center: Point2,
    x_axis: Point2,
    y_axis: Point2,
    major_radius: f64,
    minor_radius: f64,
}

impl HyperbolaPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point2,
        x_axis: Point2,
        y_axis: Point2,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        if !(center.u.is_finite() && center.v.is_finite()) {
            return Err("HyperbolaPcurve.center must be finite");
        }
        if !(x_axis.u.is_finite() && x_axis.v.is_finite()) {
            return Err("HyperbolaPcurve.x_axis must be finite");
        }
        if !(y_axis.u.is_finite() && y_axis.v.is_finite()) {
            return Err("HyperbolaPcurve.y_axis must be finite");
        }
        if !major_radius.is_finite() {
            return Err("HyperbolaPcurve.major_radius must be finite");
        }
        if !minor_radius.is_finite() {
            return Err("HyperbolaPcurve.minor_radius must be finite");
        }
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("HyperbolaPcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("HyperbolaPcurve.y_axis must be nonzero");
        }
        if major_radius <= 0.0 {
            return Err("HyperbolaPcurve.major_radius must be positive");
        }
        if minor_radius <= 0.0 {
            return Err("HyperbolaPcurve.minor_radius must be positive");
        }
        Ok(Self {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2, &f64, &f64) {
        (
            &self.center,
            &self.x_axis,
            &self.y_axis,
            &self.major_radius,
            &self.minor_radius,
        )
    }
}

impl TryFrom<HyperbolaPcurveWire> for HyperbolaPcurve {
    type Error = &'static str;
    fn try_from(wire: HyperbolaPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.center,
            wire.x_axis,
            wire.y_axis,
            wire.major_radius,
            wire.minor_radius,
        )
    }
}

/// Hyperbolic parameter-space curve with finite coefficients and nonzero variation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HyperbolicPcurveWire")]
pub struct HyperbolicPcurve {
    center: Point2,
    cosine: Point2,
    sine: Point2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HyperbolicPcurveWire {
    center: Point2,
    cosine: Point2,
    sine: Point2,
}

impl HyperbolicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(center: Point2, cosine: Point2, sine: Point2) -> Result<Self, &'static str> {
        if !(center.u.is_finite() && center.v.is_finite()) {
            return Err("HyperbolicPcurve.center must be finite");
        }
        if !(cosine.u.is_finite() && cosine.v.is_finite()) {
            return Err("HyperbolicPcurve.cosine must be finite");
        }
        if !(sine.u.is_finite() && sine.v.is_finite()) {
            return Err("HyperbolicPcurve.sine must be finite");
        }
        if !(cosine.u.hypot(cosine.v) > 0.0 || sine.u.hypot(sine.v) > 0.0) {
            return Err("HyperbolicPcurve.cosine/sine must not both be zero");
        }
        Ok(Self {
            center,
            cosine,
            sine,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&Point2, &Point2, &Point2) {
        (&self.center, &self.cosine, &self.sine)
    }
}

impl TryFrom<HyperbolicPcurveWire> for HyperbolicPcurve {
    type Error = &'static str;
    fn try_from(wire: HyperbolicPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.center, wire.cosine, wire.sine)
    }
}

/// Parameter-space trim with a finite ordered interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TrimmedPcurveWire")]
pub struct TrimmedPcurve {
    parameter_range: [f64; 2],
    #[serde(default = "default_true")]
    same_sense: bool,
    basis: Box<PcurveGeometry>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TrimmedPcurveWire {
    parameter_range: [f64; 2],
    #[serde(default = "default_true")]
    same_sense: bool,
    basis: Box<PcurveGeometry>,
}

impl TrimmedPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        parameter_range: [f64; 2],
        same_sense: bool,
        basis: Box<PcurveGeometry>,
    ) -> Result<Self, &'static str> {
        if !(parameter_range.iter().all(|v| v.is_finite())
            && parameter_range[0] <= parameter_range[1])
        {
            return Err("TrimmedPcurve.parameter_range must be finite and ordered");
        }
        Ok(Self {
            parameter_range,
            same_sense,
            basis,
        })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&[f64; 2], &bool, &PcurveGeometry) {
        (&self.parameter_range, &self.same_sense, &self.basis)
    }
}

impl TryFrom<TrimmedPcurveWire> for TrimmedPcurve {
    type Error = &'static str;
    fn try_from(wire: TrimmedPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.parameter_range, wire.same_sense, wire.basis)
    }
}

/// Parameter-space offset with a finite signed distance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "OffsetPcurveWire")]
pub struct OffsetPcurve {
    distance: f64,
    basis: Box<PcurveGeometry>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct OffsetPcurveWire {
    distance: f64,
    basis: Box<PcurveGeometry>,
}

impl OffsetPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(distance: f64, basis: Box<PcurveGeometry>) -> Result<Self, &'static str> {
        if !distance.is_finite() {
            return Err("OffsetPcurve.distance must be finite");
        }
        Ok(Self { distance, basis })
    }

    /// Borrow the carrier parameters in constructor order.
    #[must_use]
    pub fn parts(&self) -> (&f64, &PcurveGeometry) {
        (&self.distance, &self.basis)
    }
}

impl TryFrom<OffsetPcurveWire> for OffsetPcurve {
    type Error = &'static str;
    fn try_from(wire: OffsetPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.distance, wire.basis)
    }
}

/// Analytic, NURBS, or opaque surface geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceGeometry {
    /// Infinite plane through `origin` with the given `normal`.
    Plane(PlaneSurface),
    /// Right circular cylinder of the given `radius` about the axis line.
    Cylinder(CylinderSurface),
    /// Right elliptical cone. `radius` is the major radius at `origin`;
    /// `ratio` is the minor-to-major radius ratio; `half_angle` is the major
    /// half-angle between the axis and the cone surface, in radians.
    Cone(ConeSurface),
    /// Sphere.
    Sphere(SphereSurface),
    /// Torus. `major_radius` is the distance from `center` to the tube center;
    /// `minor_radius` is the tube radius.
    Torus(TorusSurface),
    /// Free-form NURBS surface.
    Nurbs(NurbsSurface),
    /// Exact surface defined by a procedural construction in the same model.
    Procedural {
        /// Construction that produces this carrier.
        construction: ProceduralSurfaceId,
        /// Solved carrier geometry retained from the source cache.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "schema", schemars(skip))]
        cache: Option<SolvedSurfaceGeometry>,
    },
    /// Source-native polygonal surface with an explicit chordal error bound.
    Polygonal(PolygonalSurface),
    /// Exact affine placement of an inline basis surface.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<SurfaceGeometry>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
    /// Surface geometry that has no typed neutral representation.
    ///
    /// `record` links to retained source bytes when available.
    ///
    /// A [`Surface`] carrying this variant should have entity exactness
    /// [`Exactness::Unknown`](crate::provenance::Exactness::Unknown) in the
    /// document's [`Annotations`](crate::annotations::Annotations): the shape was
    /// not established, so nothing about it is byte-exact or derived.
    Unknown {
        /// Link to the preserved raw record, when the decoder kept the bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// A solved surface cache that cannot recursively contain a procedural carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct SolvedSurfaceGeometry(Box<SurfaceGeometry>);

impl SolvedSurfaceGeometry {
    /// Wrap a non-procedural solved surface geometry.
    // Failed admission returns the complete input geometry to its caller.
    #[allow(clippy::result_large_err)]
    pub fn new(geometry: SurfaceGeometry) -> Result<Self, SurfaceGeometry> {
        let mut basis = &geometry;
        while let SurfaceGeometry::Transformed { basis: inner, .. } = basis {
            basis = inner;
        }
        if matches!(basis, SurfaceGeometry::Procedural { .. }) {
            Err(geometry)
        } else {
            Ok(Self(Box::new(geometry)))
        }
    }

    /// Borrow the solved geometry.
    #[must_use]
    pub fn as_geometry(&self) -> &SurfaceGeometry {
        &self.0
    }
}

impl Serialize for SolvedSurfaceGeometry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_geometry().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SolvedSurfaceGeometry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(SurfaceGeometry::deserialize(deserializer)?).map_err(|_| {
            serde::de::Error::custom("a solved cache cannot contain a procedural carrier")
        })
    }
}

impl AsRef<SurfaceGeometry> for SolvedSurfaceGeometry {
    fn as_ref(&self) -> &SurfaceGeometry {
        self.as_geometry()
    }
}

impl std::ops::Deref for SolvedSurfaceGeometry {
    type Target = SurfaceGeometry;

    fn deref(&self) -> &Self::Target {
        self.as_geometry()
    }
}

impl SurfaceGeometry {
    /// Construction that owns this carrier, when it is procedural.
    #[must_use]
    pub fn procedural_construction(&self) -> Option<&ProceduralSurfaceId> {
        match self {
            Self::Procedural { construction, .. } => Some(construction),
            _ => None,
        }
    }

    /// Geometry used to evaluate this carrier without following a construction.
    #[must_use]
    pub fn solved_cache(&self) -> Option<&SurfaceGeometry> {
        match self {
            Self::Procedural {
                cache: Some(geometry),
                ..
            } => Some(geometry.as_geometry()),
            _ => None,
        }
    }

    pub(crate) fn wire_geometry(&self) -> &SurfaceGeometry {
        self.solved_cache().unwrap_or(self)
    }
}

/// An identified surface carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Surface {
    /// Arena id.
    pub id: SurfaceId,
    /// Surface shape.
    pub geometry: SurfaceGeometry,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
}

/// The analytic or free-form shape of a 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveGeometry {
    /// Infinite line.
    Line(LineCurve),
    /// Full circle.
    Circle(CircleCurve),
    /// Ellipse.
    Ellipse(EllipseCurve),
    /// Parabola in STEP conic form.
    Parabola(ParabolaCurve),
    /// Hyperbola in STEP conic form.
    Hyperbola(HyperbolaCurve),
    /// A curve collapsed to one model-space point at a topological singularity.
    Degenerate(DegenerateCurve),
    /// Ordered child curves joined into one bounded carrier.
    Composite {
        /// Ordered curve uses and their continuity contracts.
        segments: CompositeCurveSegments,
        /// Whether the source classifies the complete curve as self-intersecting.
        self_intersect: Option<bool>,
    },
    /// Free-form NURBS curve.
    Nurbs(NurbsCurve),
    /// Exact curve defined by a procedural construction in the same model.
    Procedural {
        /// Construction that produces this carrier.
        construction: ProceduralCurveId,
        /// Solved carrier geometry retained from the source cache.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "schema", schemars(skip))]
        cache: Option<SolvedCurveGeometry>,
    },
    /// Source-native polyline with an explicit chordal error bound.
    Polyline(PolylineCurve),
    /// Exact affine placement of an inline basis curve.
    Transformed {
        /// Unplaced basis geometry with unchanged parameterization.
        basis: Box<CurveGeometry>,
        /// Affine map from basis coordinates to model coordinates.
        transform: Transform,
    },
    /// Native curve carrier whose shape is not decoded.
    Unknown {
        /// Retained native record containing the curve carrier.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// A solved curve cache that cannot recursively contain a procedural carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct SolvedCurveGeometry(Box<CurveGeometry>);

impl SolvedCurveGeometry {
    /// Wrap a non-procedural solved curve geometry.
    // Failed admission returns the complete input geometry to its caller.
    #[allow(clippy::result_large_err)]
    pub fn new(geometry: CurveGeometry) -> Result<Self, CurveGeometry> {
        let mut basis = &geometry;
        while let CurveGeometry::Transformed { basis: inner, .. } = basis {
            basis = inner;
        }
        if matches!(basis, CurveGeometry::Procedural { .. }) {
            Err(geometry)
        } else {
            Ok(Self(Box::new(geometry)))
        }
    }

    /// Borrow the solved geometry.
    #[must_use]
    pub fn as_geometry(&self) -> &CurveGeometry {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn as_geometry_mut(&mut self) -> &mut CurveGeometry {
        &mut self.0
    }
}

impl Serialize for SolvedCurveGeometry {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_geometry().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SolvedCurveGeometry {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(CurveGeometry::deserialize(deserializer)?).map_err(|_| {
            serde::de::Error::custom("a solved cache cannot contain a procedural carrier")
        })
    }
}

impl AsRef<CurveGeometry> for SolvedCurveGeometry {
    fn as_ref(&self) -> &CurveGeometry {
        self.as_geometry()
    }
}

impl std::ops::Deref for SolvedCurveGeometry {
    type Target = CurveGeometry;

    fn deref(&self) -> &Self::Target {
        self.as_geometry()
    }
}

impl CurveGeometry {
    /// Construction that owns this carrier, when it is procedural.
    #[must_use]
    pub fn procedural_construction(&self) -> Option<&ProceduralCurveId> {
        match self {
            Self::Procedural { construction, .. } => Some(construction),
            _ => None,
        }
    }

    /// Geometry used to evaluate this carrier without following a construction.
    #[must_use]
    pub fn solved_cache(&self) -> Option<&CurveGeometry> {
        match self {
            Self::Procedural {
                cache: Some(geometry),
                ..
            } => Some(geometry.as_geometry()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn solved_cache_mut(&mut self) -> Option<&mut CurveGeometry> {
        match self {
            Self::Procedural {
                cache: Some(geometry),
                ..
            } => Some(geometry.as_geometry_mut()),
            _ => None,
        }
    }

    pub(crate) fn wire_geometry(&self) -> &CurveGeometry {
        self.solved_cache().unwrap_or(self)
    }
}

/// Non-empty ordered child uses of a composite curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<CompositeCurveSegment>")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompositeCurveSegments(Vec<CompositeCurveSegment>);

impl TryFrom<Vec<CompositeCurveSegment>> for CompositeCurveSegments {
    type Error = &'static str;

    fn try_from(segments: Vec<CompositeCurveSegment>) -> Result<Self, Self::Error> {
        if segments.is_empty() {
            return Err("composite curve segments must not be empty");
        }
        Ok(Self(segments))
    }
}

impl std::ops::Deref for CompositeCurveSegments {
    type Target = [CompositeCurveSegment];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for CompositeCurveSegments {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a> IntoIterator for &'a CompositeCurveSegments {
    type Item = &'a CompositeCurveSegment;
    type IntoIter = std::slice::Iter<'a, CompositeCurveSegment>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// One directed child use in a composite curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompositeCurveSegment {
    /// Referenced child curve carrier.
    pub curve: CurveId,
    /// Whether the child parameter direction is retained.
    pub same_sense: bool,
    /// Required continuity from the preceding segment to this segment.
    pub transition: CompositeCurveTransition,
}

/// STEP composite-curve transition continuity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CompositeCurveTransition {
    /// No positional continuity is asserted.
    Discontinuous,
    /// Positional continuity.
    Continuous,
    /// Positional and tangent continuity.
    ContSameGradient,
    /// Positional, tangent, and curvature continuity.
    ContSameGradientSameCurvature,
}

/// Derive a stable in-plane reference direction from an axis.
///
/// The least-aligned global basis axis is projected onto the plane normal to
/// `axis`, then normalized. Degenerate axes fall back to global x.
pub fn derive_reference_direction(axis: Vector3) -> Vector3 {
    let norm = axis.norm();
    if !norm.is_finite() || norm == 0.0 {
        return Vector3::new(1.0, 0.0, 0.0);
    }
    let axis = Vector3::new(axis.x / norm, axis.y / norm, axis.z / norm);
    let basis = if axis.x.abs() <= axis.y.abs() && axis.x.abs() <= axis.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if axis.y.abs() <= axis.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    let dot = axis.x * basis.x + axis.y * basis.y + axis.z * basis.z;
    let projected = Vector3::new(
        basis.x - dot * axis.x,
        basis.y - dot * axis.y,
        basis.z - dot * axis.z,
    );
    let projected_norm = projected.norm();
    Vector3::new(
        projected.x / projected_norm,
        projected.y / projected_norm,
        projected.z / projected_norm,
    )
}

/// A 3D curve carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Curve {
    /// Arena id.
    pub id: CurveId,
    /// Curve shape.
    pub geometry: CurveGeometry,
    /// Native source-object identity and effective display metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<SourceObjectAssociation>,
}

/// A neutral surface construction linked to the carrier it produces.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralSurface {
    /// Stable construction identity.
    pub id: ProceduralSurfaceId,
    /// Neutral construction definition.
    definition: ProceduralSurfaceDefinition,
    /// Fit contract of a legacy solved cache. Revision-gated forms carry the
    /// same value in their [`RevisionCacheForm`].
    legacy_cache_fit_tolerance: Option<FitTolerance>,
    /// Four optional U/V parameter bounds following the record's subtype
    /// scope. For a procedural extrusion or revolution, the first pair is
    /// the neutral surface-carrier interval; its definition retains the
    /// source directrix interval separately. `None` when the record stores no
    /// bound fields.
    pub record_bounds: Option<[Option<f64>; 4]>,
}

/// Parameter fields carried by exact and loft spline-surface constructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SplineSurfaceParameters {
    /// Ordered semantic U and V intervals in the legacy layout.
    OrderedRanges {
        /// Ordered U and V intervals.
        ranges: [[f64; 2]; 2],
    },
    /// Two parameter intervals in a revision-gated layout, each stored as an
    /// ordered `[lo, hi]` pair of optional bounds. For exact and t-spline
    /// surfaces these are the surface's unextended (pre-extension) parameter
    /// ranges; for loft surfaces they are wrap ranges, where a reversed pair
    /// (`lo > hi`) encodes an empty interval (no wrap). `None` is a false
    /// bound-presence flag.
    RevisionRanges {
        /// Two parameter intervals in serialized field order.
        intervals: [[Option<f64>; 2]; 2],
    },
}

/// Mutually exclusive legacy and revision-gated exact-spline layouts.
#[derive(Debug, Clone, PartialEq)]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
pub enum ExactSpline {
    /// Legacy solved-cache layout with ordered U/V ranges.
    Legacy {
        /// Ordered U and V parameter ranges.
        ranges: [[f64; 2]; 2],
        /// Native ASM extension integer following the ranges.
        extension: i64,
    },
    /// Revision-gated layout with optional interval bounds and shared form.
    Revision {
        /// Two optional-bound parameter intervals in wire order.
        intervals: [[Option<f64>; 2]; 2],
        /// Native ASM extension enum following the intervals.
        extension: i64,
        /// Required revision-gated form.
        form: RevisionSurfaceForm,
    },
}

#[derive(Serialize)]
struct ExactSplineWriteWire<'a> {
    parameters: SplineSurfaceParameters,
    extension: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_form: Option<&'a RevisionSurfaceForm>,
}

#[derive(Deserialize)]
struct ExactSplineReadWire {
    parameters: SplineSurfaceParameters,
    extension: i64,
    #[serde(default)]
    revision_form: Option<RevisionSurfaceForm>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the exact-spline wire schema")]
struct ExactSplineSchemaWire {
    parameters: SplineSurfaceParameters,
    extension: i64,
    revision_form: Option<RevisionSurfaceForm>,
}

impl Serialize for ExactSpline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (parameters, extension, revision_form) = match self {
            Self::Legacy { ranges, extension } => (
                SplineSurfaceParameters::OrderedRanges { ranges: *ranges },
                *extension,
                None,
            ),
            Self::Revision {
                intervals,
                extension,
                form,
            } => (
                SplineSurfaceParameters::RevisionRanges {
                    intervals: *intervals,
                },
                *extension,
                Some(form),
            ),
        };
        ExactSplineWriteWire {
            parameters,
            extension,
            revision_form,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExactSpline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ExactSplineReadWire::deserialize(deserializer)?;
        match (wire.parameters, wire.revision_form) {
            (SplineSurfaceParameters::OrderedRanges { ranges }, None) => Ok(Self::Legacy {
                ranges,
                extension: wire.extension,
            }),
            (SplineSurfaceParameters::RevisionRanges { intervals }, Some(form)) => {
                Ok(Self::Revision {
                    intervals,
                    extension: wire.extension,
                    form,
                })
            }
            (SplineSurfaceParameters::OrderedRanges { .. }, Some(_)) => Err(
                serde::de::Error::custom("exact spline ordered ranges cannot carry revision_form"),
            ),
            (SplineSurfaceParameters::RevisionRanges { .. }, None) => Err(
                serde::de::Error::custom("exact spline revision ranges require revision_form"),
            ),
        }
    }
}

/// One component and its native construction scalar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompoundComponent<T> {
    /// Scalar paired with this component.
    pub parameter: f64,
    /// Component geometry or its resolved identity.
    pub component: T,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CompoundSurfaceComponentsWire {
    parameters: Vec<f64>,
    components: Vec<SurfaceId>,
}

mod compound_surface_components_wire {
    use super::{CompoundComponent, CompoundSurfaceComponentsWire, SurfaceId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        components: &[CompoundComponent<SurfaceId>],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CompoundSurfaceComponentsWire {
            parameters: components.iter().map(|item| item.parameter).collect(),
            components: components
                .iter()
                .map(|item| item.component.clone())
                .collect(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Vec<CompoundComponent<SurfaceId>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CompoundSurfaceComponentsWire::deserialize(deserializer)?;
        if wire.parameters.len() != wire.components.len() {
            return Err(serde::de::Error::custom(
                "compound surface parameters must match components",
            ));
        }
        Ok(wire
            .parameters
            .into_iter()
            .zip(wire.components)
            .map(|(parameter, component)| CompoundComponent {
                parameter,
                component,
            })
            .collect())
    }
}

/// A non-empty compound curve with finite construction parameters.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CompoundCurveConstructionWire")]
pub struct CompoundCurveConstruction {
    parameters: Vec<f64>,
    components: Vec<CompoundComponent<CurveId>>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CompoundCurveConstructionWire {
    parameters: Vec<f64>,
    component_parameters: Vec<f64>,
    components: Vec<CurveId>,
}

impl TryFrom<CompoundCurveConstructionWire> for CompoundCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: CompoundCurveConstructionWire) -> Result<Self, Self::Error> {
        if wire.component_parameters.len() != wire.components.len() {
            return Err("compound curve component_parameters must match components");
        }
        Self::try_new(
            wire.parameters,
            wire.component_parameters
                .into_iter()
                .zip(wire.components)
                .map(|(parameter, component)| CompoundComponent {
                    parameter,
                    component,
                })
                .collect(),
        )
    }
}

impl Serialize for CompoundCurveConstruction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CompoundCurveConstructionWire {
            parameters: self.parameters.clone(),
            component_parameters: self.components.iter().map(|item| item.parameter).collect(),
            components: self
                .components
                .iter()
                .map(|item| item.component.clone())
                .collect(),
        }
        .serialize(serializer)
    }
}

impl CompoundCurveConstruction {
    /// Admit at least one component and finite leading and component parameters.
    pub fn try_new(
        parameters: Vec<f64>,
        components: Vec<CompoundComponent<CurveId>>,
    ) -> Result<Self, &'static str> {
        if components.is_empty() {
            return Err("compound curve components must not be empty");
        }
        if parameters
            .iter()
            .chain(components.iter().map(|item| &item.parameter))
            .any(|value| !value.is_finite())
        {
            return Err("compound curve parameters must be finite");
        }
        Ok(Self {
            parameters,
            components,
        })
    }

    /// Leading parameters and ordered parameter-component pairs.
    #[must_use]
    pub fn parts(&self) -> (&[f64], &[CompoundComponent<CurveId>]) {
        (&self.parameters, &self.components)
    }
}

/// Neutral semantics for a procedural surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralSurfaceDefinition {
    /// Exact native NURBS surface with retained parameter fields.
    Exact {
        /// Complete legacy or revision-gated exact-spline layout.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(with = "ExactSplineSchemaWire"))]
        spline: ExactSpline,
    },
    /// Ordered native compound of a solved surface and component surfaces.
    Compound {
        /// Ordered surfaces paired with their native construction scalars.
        #[serde(flatten, with = "compound_surface_components_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "CompoundSurfaceComponentsWire"))]
        components: Vec<CompoundComponent<SurfaceId>>,
    },
    /// Exact rectangular restriction of an embedded support surface.
    SubSurface {
        /// Embedded support surface whose parameterization is retained.
        support: SurfaceId,
        /// Ordered U and V parameter intervals.
        parameter_ranges: [[f64; 2]; 2],
    },
    /// Taper of a support surface around a reference curve.
    Taper {
        /// Base surface being tapered.
        support: SurfaceId,
        /// Reference curve on the support.
        reference: CurveId,
        /// UV curve on the support, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// Native taper parameter or draft magnitude.
        parameter: f64,
        /// Subtype-specific taper tail.
        taper: TaperSurfaceKind,
        /// Revision-gated form fields; absent from the pre-revision layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Native loft defined by two section graphs and closure contracts.
    Loft {
        /// Two ordered loft sections.
        sections: [LoftSection; 2],
        /// Legacy ordered ranges or revision-native scalar values.
        parameters: SplineSurfaceParameters,
        /// Two ordered native closure enums.
        closures: [i64; 2],
        /// Two ordered native singularity enums.
        singularities: [i64; 2],
        /// Native loft mode integer.
        mode: i64,
        /// Variable native tokens between the mode and solved cache.
        bridge: Vec<LoftBridgeToken>,
        /// Revision-gated form fields; absent from the pre-revision layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<LoftRevisionForm>,
    },
    /// Native compound-loft construction.
    CompoundLoft {
        /// Complete native compound-loft graph.
        construction: Box<CompoundLoftConstruction>,
    },
    /// Revision-gated compound-loft construction.
    RevisionCompoundLoft {
        /// Complete native revision-gated compound-loft graph.
        construction: Box<RevisionCompoundLoftConstruction>,
    },
    /// Native scaled compound-loft construction.
    ScaledCompoundLoft {
        /// Complete native scaled compound-loft graph.
        construction: Box<ScaledCompoundLoftConstruction>,
    },
    /// Native skinned spline surface.
    Skin {
        /// Complete native skin construction graph.
        construction: Box<SkinSurfaceConstruction>,
    },
    /// Native surface defined by recursive law formulas.
    Law {
        /// Complete native law-surface construction graph.
        construction: Box<LawSurfaceConstruction>,
    },
    /// Native curve-network spline surface.
    Net {
        /// Complete native net construction graph.
        construction: Box<NetSurfaceConstruction>,
    },
    /// Native curvature-continuous two-sided blend.
    G2Blend {
        /// Complete native G2 construction graph.
        construction: Box<G2BlendConstruction>,
    },
    /// Revision-gated curvature-continuous blend in the variable-blend side
    /// layout.
    RevisionG2Blend {
        /// Complete native revision-gated G2 construction graph.
        construction: Box<RevisionG2BlendConstruction>,
    },
    /// Native variable-radius two-sided blend.
    VariableBlend {
        /// Complete native variable-blend construction graph.
        construction: Box<VariableBlendConstruction>,
    },
    /// Native vertex-blend patch.
    VertexBlend {
        /// Complete native vertex-blend construction graph.
        construction: Box<VertexBlendConstruction>,
    },
    /// Translation of a directrix along a direction.
    Extrusion {
        /// Curve swept along `direction` to form the surface.
        directrix: CurveId,
        /// Native source directrix parameter interval, when carried by the
        /// source. The neutral surface-carrier interval is in
        /// `ProceduralSurface::record_bounds`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_interval: Option<[f64; 2]>,
        /// Length-bearing sweep direction, in document length units.
        direction: Vector3,
        /// Native model-space position following the sweep direction, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_position: Option<Point3>,
        /// Revision-gated form fields; absent from the pre-revision layout.
        /// The directrix parameter interval is `parameter_interval`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Unbounded linear sweep of a directrix.
    LinearSweep {
        /// Curve swept along `direction`.
        directrix: CurveId,
        /// Length-bearing sweep vector.
        direction: Vector3,
    },
    /// Revolution of a directrix about an axis.
    Revolution {
        /// Curve revolved about the axis to form the surface.
        directrix: CurveId,
        /// A point on the revolution axis.
        axis_origin: Point3,
        /// Unit direction of the revolution axis.
        axis_direction: Vector3,
        /// Angular start and end parameters, in radians.
        angular_interval: [f64; 2],
        /// Surface-parameter interval that maps affinely to
        /// `angular_interval`. Absence means the surface parameter is already
        /// the revolution angle in radians.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angular_parameter_interval: Option<[f64; 2]>,
        /// Native source directrix parameter start and end values, when
        /// carried by the source representation. The neutral surface-carrier
        /// interval is in `ProceduralSurface::record_bounds`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter_interval: Option<[f64; 2]>,
        /// Whether the source parameter directions are transposed.
        transposed: bool,
        /// Revision-gated form fields; absent from the pre-revision layout.
        /// The profile curve's optional endpoints are `reference_endpoints`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Full revolution of a directrix about an axis.
    AxisRevolution {
        /// Curve revolved about the axis.
        directrix: CurveId,
        /// Point on the revolution axis.
        axis_origin: Point3,
        /// Unit revolution-axis direction.
        axis_direction: Vector3,
    },
    /// Sum of two ordered curves from a base point.
    Sum {
        /// First curve, varying in the first surface parameter.
        first: CurveId,
        /// Second curve, varying in the second surface parameter.
        second: CurveId,
        /// Surface base point.
        basepoint: Vector3,
        /// Revision-gated form fields; absent from the pre-revision layout.
        /// The first curve's optional endpoints are `reference_endpoints`
        /// and the second curve's are `second_endpoints`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_form: Option<RevisionSurfaceForm>,
    },
    /// Sweep of a profile along a spine.
    Sweep {
        /// Cross-section curve carried along `spine`.
        profile: CurveId,
        /// Path curve the profile is swept along.
        spine: CurveId,
        /// Complete native sweep graph when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native: Option<Box<SweepSurfaceConstruction>>,
    },
    /// T-spline face with its shared subtransform program.
    TSpline {
        /// Complete native T-spline wrapper construction.
        construction: Box<TSplineSurfaceConstruction>,
    },
    /// Surface generated along an inline circular or linear helix path.
    Helix {
        /// Complete native helix-surface construction.
        construction: Box<HelixSurfaceConstruction>,
    },
    /// Native deformable spline surface.
    Deformable {
        /// Complete decoded deformable construction.
        construction: Box<DeformableSurfaceConstruction>,
    },
    /// Offset from a support surface.
    Offset {
        /// Surface this surface is offset from.
        support: SurfaceId,
        /// Signed offset distance, in document length units.
        distance: f64,
        /// Native U parameter-direction sense enum, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u_sense: Option<i64>,
        /// Native V parameter-direction sense enum, when carried.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        v_sense: Option<i64>,
        /// Support continuation law outside its active NURBS rectangle.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        support_extension: Option<OffsetSupportExtension>,
        /// Legacy conditional extension flags or the revision-gated form.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(with = "OffsetExtensionSchemaWire"))]
        extension: OffsetExtension,
    },
    /// Rectangular parameter sub-range of a support surface.
    Subset {
        /// Surface being restricted.
        support: SurfaceId,
        /// U and V parameter endpoints in the support parameterization.
        ///
        /// The endpoint order is significant for cyclic and reversed
        /// trims. A producer that does not carry direction metadata may
        /// leave the sense fields absent and use increasing endpoints.
        parameter_ranges: [[f64; 2]; 2],
        /// Whether the trimmed surface U direction agrees with the support.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u_sense: Option<bool>,
        /// Whether the trimmed surface V direction agrees with the support.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        v_sense: Option<bool>,
    },
    /// Affine replica of a surface carrier, retaining the parent surface
    /// construction and its parameter domain.
    Replica {
        /// Surface being replicated.
        source: SurfaceId,
        /// Affine map from the parent surface coordinates to this surface.
        transform: Transform,
    },
    /// Parallel offset from a support surface.
    ParallelOffset {
        /// Surface being offset.
        support: SurfaceId,
        /// Signed offset distance.
        distance: f64,
        /// Whether the source classifies the result as self-intersecting.
        self_intersect: Option<bool>,
    },
    /// Self-intersecting torus with an explicitly selected outer or inner sheet.
    DegenerateTorus {
        /// Whether the outer sheet is selected at the self-intersection.
        select_outer: bool,
    },
    /// Surface domain bounded by ordered curves on a supporting surface.
    CurveBounded {
        /// Supporting surface whose parameterization defines the domain.
        support: SurfaceId,
        /// Boundary curves on the support.
        boundaries: Vec<CurveId>,
        /// Parameter-space carriers used by surface-curve boundary segments.
        ///
        /// These are separate from `boundaries`: one STEP surface curve can
        /// carry both a model-space curve and pcurves on the support surface.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        boundary_pcurves: Vec<PcurveId>,
        /// Whether the support's natural outer boundary is implicit.
        implicit_outer: bool,
    },
    /// Ruled surface joining two directrices.
    Ruled {
        /// First bounding curve of the ruled surface.
        first: CurveId,
        /// Second bounding curve of the ruled surface.
        second: CurveId,
    },
    /// Rolling-ball or law-driven blend between two support surfaces.
    Blend {
        /// The two blend support sides, in side order; `None` when a side was
        /// not resolved.
        supports: [Option<BlendSupport>; 2],
        /// Stored center/spine curve, when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spine: Option<CurveId>,
        /// Signed offset-radius law along the spine.
        radius: BlendRadiusLaw,
        /// Cross-section family of the blend.
        cross_section: BlendCrossSection,
        /// Complete byte-backed rolling-ball context when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native: Option<Box<RollingBallConstruction>>,
    },
    /// Rolling-ball surface defined by aligned quintic value/derivative jets.
    RollingBallJet(RollingBallJetStations),
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

impl ProceduralSurfaceDefinition {
    fn revision_cache(&self) -> Option<&RevisionCacheForm> {
        match self {
            Self::Exact {
                spline: ExactSpline::Revision { form, .. },
            } => Some(&form.cache),
            Self::Taper { revision_form, .. }
            | Self::Extrusion { revision_form, .. }
            | Self::Revolution { revision_form, .. }
            | Self::Sum { revision_form, .. } => revision_form.as_ref().map(|form| &form.cache),
            Self::Offset {
                extension: OffsetExtension::Revision(form),
                ..
            } => Some(&form.cache),
            Self::Loft { revision_form, .. } => revision_form.as_ref().map(|form| &form.cache),
            Self::RevisionCompoundLoft { construction } => Some(&construction.cache),
            Self::RevisionG2Blend { construction } => Some(&construction.cache),
            Self::Sweep {
                native: Some(construction),
                ..
            } => construction.revision_form.as_ref().map(|form| &form.cache),
            Self::TSpline { construction } => {
                construction.revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Deformable { construction } => {
                construction.revision_form.as_ref().map(|form| &form.cache)
            }
            Self::Blend {
                native: Some(construction),
                ..
            } => Some(&construction.cache),
            _ => None,
        }
    }

    fn revision_cache_mut(&mut self) -> Option<&mut RevisionCacheForm> {
        match self {
            Self::Exact {
                spline: ExactSpline::Revision { form, .. },
            } => Some(&mut form.cache),
            Self::Taper { revision_form, .. }
            | Self::Extrusion { revision_form, .. }
            | Self::Revolution { revision_form, .. }
            | Self::Sum { revision_form, .. } => revision_form.as_mut().map(|form| &mut form.cache),
            Self::Offset {
                extension: OffsetExtension::Revision(form),
                ..
            } => Some(&mut form.cache),
            Self::Loft { revision_form, .. } => revision_form.as_mut().map(|form| &mut form.cache),
            Self::RevisionCompoundLoft { construction } => Some(&mut construction.cache),
            Self::RevisionG2Blend { construction } => Some(&mut construction.cache),
            Self::Sweep {
                native: Some(construction),
                ..
            } => construction
                .revision_form
                .as_mut()
                .map(|form| &mut form.cache),
            Self::TSpline { construction } => construction
                .revision_form
                .as_mut()
                .map(|form| &mut form.cache),
            Self::Deformable { construction } => construction
                .revision_form
                .as_mut()
                .map(|form| &mut form.cache),
            Self::Blend {
                native: Some(construction),
                ..
            } => Some(&mut construction.cache),
            _ => None,
        }
    }

    fn owns_revision_cache(&self) -> bool {
        self.revision_cache().is_some() || matches!(self, Self::VariableBlend { .. })
    }

    // Outer absence means no revision layout; inner absence means no cache tolerance.
    #[allow(clippy::option_option)]
    fn revision_cache_fit_tolerance(&self) -> Option<Option<f64>> {
        if let Self::VariableBlend { construction } = self {
            return Some(construction.cache.fit_tolerance());
        }
        self.revision_cache().map(RevisionCacheForm::fit_tolerance)
    }
}

/// A finite, non-negative fit tolerance.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "f64", into = "f64")]
pub struct FitTolerance(f64);

impl FitTolerance {
    /// Admit a finite, non-negative fit tolerance.
    pub fn try_new(value: f64) -> Result<Self, CacheFitToleranceError> {
        if value.is_finite() && value >= 0.0 {
            Ok(Self(value))
        } else {
            Err(CacheFitToleranceError::InvalidValue { value })
        }
    }

    /// The fit tolerance in carrier units.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for FitTolerance {
    type Error = CacheFitToleranceError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<FitTolerance> for f64 {
    fn from(value: FitTolerance) -> Self {
        value.get()
    }
}

/// A top-level cache-fit field disagrees with the construction that owns it.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CacheFitToleranceError {
    /// A fit tolerance is negative or non-finite.
    #[error("fit_tolerance must be finite and non-negative, got {value}")]
    InvalidValue {
        /// Rejected tolerance.
        value: f64,
    },
    /// A full law surface requires a solved-cache tolerance.
    #[error("cache_fit_tolerance is required for a full law surface tail")]
    MissingLawFull,
    /// Other law surface tails do not carry a solved-cache tolerance.
    #[error("cache_fit_tolerance must be absent for a non-full law surface tail")]
    NonFullLaw,

    /// A parameterized form cannot carry a solved-cache tolerance.
    #[error("cache_fit_tolerance must be absent for a parameterized revision cache")]
    Parameterized,
    /// A stale variable-blend cache cannot carry an active fit contract.
    #[error("cache_fit_tolerance must be absent for a stale variable-blend cache")]
    StaleVariableBlend,
    /// A solved revision cache cannot lose its required tolerance.
    #[error("cache_fit_tolerance is required for a solved revision cache")]
    MissingSolved,
    /// The compatibility field disagrees with the tolerance in the solved form.
    #[error(
        "cache_fit_tolerance {supplied} does not match the solved revision cache tolerance {stored}"
    )]
    Conflicting {
        /// Value supplied at the outer compatibility boundary.
        supplied: f64,
        /// Value owned by the solved cache form.
        stored: f64,
    },
}

impl ProceduralSurface {
    /// Build a procedural surface without a legacy top-level cache.
    pub fn new(
        id: ProceduralSurfaceId,
        definition: ProceduralSurfaceDefinition,
        record_bounds: Option<[Option<f64>; 4]>,
    ) -> Result<Self, CacheFitToleranceError> {
        Self::try_new(id, definition, None, record_bounds)
    }

    /// Build a procedural surface and reconcile the legacy top-level cache
    /// field with any revision-gated cache form in its definition.
    pub fn try_new(
        id: ProceduralSurfaceId,
        definition: ProceduralSurfaceDefinition,
        cache_fit_tolerance: Option<f64>,
        record_bounds: Option<[Option<f64>; 4]>,
    ) -> Result<Self, CacheFitToleranceError> {
        let legacy_cache_fit_tolerance =
            reconcile_surface_cache_fit_tolerance(&definition, cache_fit_tolerance)?;
        Ok(Self {
            id,
            definition,
            legacy_cache_fit_tolerance,
            record_bounds,
        })
    }

    /// Borrow the neutral construction definition.
    #[must_use]
    pub fn definition(&self) -> &ProceduralSurfaceDefinition {
        &self.definition
    }

    /// Replace the construction definition and discard a legacy cache value
    /// when the new definition owns a revision cache.
    pub fn replace_definition(
        &mut self,
        definition: ProceduralSurfaceDefinition,
    ) -> Result<(), CacheFitToleranceError> {
        let tolerance = if definition.owns_revision_cache() {
            None
        } else {
            self.legacy_cache_fit_tolerance.map(FitTolerance::get)
        };
        self.try_replace_definition(definition, tolerance)
    }

    /// Replace the definition and effective cache-fit tolerance atomically.
    pub fn try_replace_definition(
        &mut self,
        definition: ProceduralSurfaceDefinition,
        cache_fit_tolerance: Option<f64>,
    ) -> Result<(), CacheFitToleranceError> {
        let legacy_cache_fit_tolerance =
            reconcile_surface_cache_fit_tolerance(&definition, cache_fit_tolerance)?;
        self.definition = definition;
        self.legacy_cache_fit_tolerance = legacy_cache_fit_tolerance;
        Ok(())
    }

    /// Edit the definition and normalize legacy cache storage before the edit
    /// can escape this call.
    pub fn edit_definition<R>(
        &mut self,
        edit: impl FnOnce(&mut ProceduralSurfaceDefinition) -> R,
    ) -> Result<R, CacheFitToleranceError> {
        let mut definition = self.definition.clone();
        let result = edit(&mut definition);
        self.replace_definition(definition)?;
        Ok(result)
    }

    /// Effective fit tolerance of the solved cache.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<f64> {
        self.definition
            .revision_cache_fit_tolerance()
            .unwrap_or(self.legacy_cache_fit_tolerance.map(FitTolerance::get))
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved revision cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<f64>,
    ) -> Result<(), CacheFitToleranceError> {
        let value = value.map(FitTolerance::try_new).transpose()?;
        validate_law_cache_fit_tolerance(&self.definition, value)?;
        if let ProceduralSurfaceDefinition::VariableBlend { construction } = &mut self.definition {
            return set_variable_blend_cache_fit_tolerance(&mut construction.cache, value);
        }
        set_cache_fit_tolerance(
            self.definition.revision_cache_mut(),
            &mut self.legacy_cache_fit_tolerance,
            value,
        )
    }

    /// Scale the effective cache-fit tolerance in place.
    pub fn scale_cache_fit_tolerance(&mut self, scale: f64) -> Result<(), CacheFitToleranceError> {
        if let Some(value) = self.cache_fit_tolerance() {
            self.set_cache_fit_tolerance(Some(value * scale))?;
        }
        Ok(())
    }
}

fn reconcile_surface_cache_fit_tolerance(
    definition: &ProceduralSurfaceDefinition,
    supplied: Option<f64>,
) -> Result<Option<FitTolerance>, CacheFitToleranceError> {
    let checked = supplied.map(FitTolerance::try_new).transpose()?;
    validate_law_cache_fit_tolerance(definition, checked)?;
    let ProceduralSurfaceDefinition::VariableBlend { construction } = definition else {
        return reconcile_cache_fit_tolerance(definition.revision_cache(), supplied);
    };
    match (&construction.cache, supplied) {
        (VariableBlendCache::Parameterization { .. }, Some(_)) => {
            Err(CacheFitToleranceError::Parameterized)
        }
        (VariableBlendCache::Stale, Some(_)) => Err(CacheFitToleranceError::StaleVariableBlend),
        (
            VariableBlendCache::Current {
                fit_tolerance: stored,
                ..
            },
            Some(supplied),
        ) if supplied != stored.get() => Err(CacheFitToleranceError::Conflicting {
            supplied,
            stored: stored.get(),
        }),
        _ => Ok(None),
    }
}

fn set_variable_blend_cache_fit_tolerance(
    cache: &mut VariableBlendCache,
    value: Option<FitTolerance>,
) -> Result<(), CacheFitToleranceError> {
    match (cache, value) {
        (VariableBlendCache::Parameterization { .. }, Some(_)) => {
            Err(CacheFitToleranceError::Parameterized)
        }
        (VariableBlendCache::Parameterization { .. }, None) => Ok(()),
        (VariableBlendCache::Current { fit_tolerance, .. }, Some(value)) => {
            *fit_tolerance = value;
            Ok(())
        }
        (VariableBlendCache::Current { .. }, None) => Err(CacheFitToleranceError::MissingSolved),
        (VariableBlendCache::Stale, Some(_)) => Err(CacheFitToleranceError::StaleVariableBlend),
        (VariableBlendCache::Stale, None) => Ok(()),
    }
}

fn reconcile_cache_fit_tolerance<P>(
    cache: Option<&RevisionCacheForm<P>>,
    supplied: Option<f64>,
) -> Result<Option<FitTolerance>, CacheFitToleranceError> {
    supplied.map(FitTolerance::try_new).transpose()?;
    match (cache, supplied) {
        (Some(RevisionCacheForm::Parameterization(_)), Some(_)) => {
            Err(CacheFitToleranceError::Parameterized)
        }
        (
            Some(RevisionCacheForm::SolvedCache {
                fit_tolerance: stored,
            }),
            Some(supplied),
        ) if supplied != stored.get() => Err(CacheFitToleranceError::Conflicting {
            supplied,
            stored: stored.get(),
        }),
        (Some(_), _) => Ok(None),
        (None, supplied) => supplied.map(FitTolerance::try_new).transpose(),
    }
}

fn set_cache_fit_tolerance<P>(
    cache: Option<&mut RevisionCacheForm<P>>,
    legacy: &mut Option<FitTolerance>,
    value: Option<FitTolerance>,
) -> Result<(), CacheFitToleranceError> {
    match (cache, value) {
        (Some(RevisionCacheForm::Parameterization(_)), Some(_)) => {
            Err(CacheFitToleranceError::Parameterized)
        }
        (Some(RevisionCacheForm::Parameterization(_)), None) => Ok(()),
        (Some(RevisionCacheForm::SolvedCache { fit_tolerance }), Some(value)) => {
            *fit_tolerance = value;
            Ok(())
        }
        (Some(RevisionCacheForm::SolvedCache { .. }), None) => {
            Err(CacheFitToleranceError::MissingSolved)
        }
        (None, value) => {
            *legacy = value;
            Ok(())
        }
    }
}

fn validate_law_cache_fit_tolerance(
    definition: &ProceduralSurfaceDefinition,
    value: Option<FitTolerance>,
) -> Result<(), CacheFitToleranceError> {
    if let ProceduralSurfaceDefinition::Law { construction } = definition {
        match (&construction.tail, value) {
            (LawSurfaceTail::Full, None) => return Err(CacheFitToleranceError::MissingLawFull),
            (LawSurfaceTail::Full, Some(_)) | (_, None) => {}
            (_, Some(_)) => return Err(CacheFitToleranceError::NonFullLaw),
        }
    }
    Ok(())
}

/// Structurally selected deformable-surface payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeformableSurfaceData {
    /// Mode-6 full embedded deformation payload.
    Full {
        /// Four leading deformation vectors.
        leading_vectors: [Vector3; 4],
        /// Leading deformation scalar.
        leading_parameter: f64,
        /// Three leading flags.
        leading_flags: [bool; 3],
        /// Native selector before the secondary support.
        selector: i64,
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long after the support.
        native_id: i64,
        /// Native support-side flag.
        flag: bool,
        /// First scalar after the flag.
        first_parameter: f64,
        /// Version-gated ASM long when present.
        version_value: Option<i64>,
        /// Second scalar after the optional long.
        second_parameter: f64,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Two ordered full vector frames.
        frames: Box<[DeformableVectorFrame; 2]>,
        /// Native trailing long.
        trailing_value: i64,
    },
    /// Mode-5 surface-and-curve deformation payload.
    SurfaceCurve {
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long identifier.
        native_id: i64,
        /// Native leading flag.
        flag: bool,
        /// First native scalar.
        first_parameter: f64,
        /// Native selector integer.
        selector: i64,
        /// Second native scalar.
        second_parameter: f64,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Four ordered deformation vectors.
        vectors: [Vector3; 4],
        /// Frame scalar after the vectors.
        frame_parameter: f64,
        /// Three frame flags.
        flags: [bool; 3],
        /// Counted ordered scalar triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// Mode-1 deformation frame with counted parameter triples.
    Plain {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame>,
        /// Ordered native scalar triples.
        parameter_triples: Vec<[f64; 3]>,
    },
    /// Mode-3 deformation frame with a guide scalar.
    Guided {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame>,
        /// Native guide selector.
        selector: i64,
        /// Native guide scalar.
        guide_parameter: f64,
    },
    /// Mode-8 minimal four-vector scaffold.
    Minimal {
        /// Four ordered deformation vectors.
        vectors: [Vector3; 4],
        /// Native trailing selector.
        selector: i64,
    },
    /// Revision-gated mode-3 deformation payload.
    RevisionMode3 {
        /// Four leading deformation vectors.
        leading_vectors: [Vector3; 4],
        /// Scalar following the leading vectors.
        leading_parameter: f64,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position anchoring the trailing frame.
        trailing_point: Point3,
        /// Two vectors following the trailing point.
        trailing_vectors: [Vector3; 2],
        /// Scalar following the trailing vectors.
        frame_parameter: f64,
        /// Two flags following the trailing frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalar parameters following the trailing frame.
        parameters: [f64; 3],
        /// Five flags following the ordered scalar parameters.
        trailing_flags: [bool; 5],
        /// Scalar preceding the payload's final integer.
        trailing_parameter: f64,
        /// Integer closing the revision mode-3 payload.
        trailing_value: i64,
    },
}

/// Four-vector frame used by full deformable surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DeformableVectorFrame {
    /// Four ordered vectors.
    pub vectors: [Vector3; 4],
    /// Frame scalar.
    pub parameter: f64,
    /// Three ordered flags.
    pub flags: [bool; 3],
}

/// Shared frame payload of deformable-surface modes 1 and 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DeformableSurfaceFrame {
    /// Four leading deformation vectors.
    pub leading_vectors: [Vector3; 4],
    /// Leading frame scalar.
    pub leading_parameter: f64,
    /// Three leading frame flags.
    pub leading_flags: [bool; 3],
    /// Three secondary deformation vectors.
    pub secondary_vectors: [Vector3; 3],
    /// Secondary frame scalar.
    pub secondary_parameter: f64,
    /// Two secondary frame flags.
    pub secondary_flags: [bool; 2],
    /// Native model-space frame point.
    pub point: Point3,
    /// Five trailing frame flags.
    pub trailing_flags: [bool; 5],
}

/// Complete native deformable-surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DeformableSurfaceConstruction {
    /// Surface being deformed.
    pub support: SurfaceId,
    /// Discriminator-selected deformation data.
    pub data: DeformableSurfaceData,
    /// Revision-gated fields surrounding the support and shared surface tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_form: Option<RevisionSurfaceForm>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

const EPS_HELIX_SURFACE_RADIUS_RELATIVE: f64 = 1.0e-9;
const EPS_HELIX_CURVE_RADIUS: f64 = 1.0e-9;

/// Finite circular path of a helix surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixPathConstructionWire")]
pub struct HelixPathConstruction {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
    axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HelixPathConstructionWire {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
    axis: Vector3,
}

impl HelixPathConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        center: Point3,
        major: Vector3,
        minor: Vector3,
        pitch: Vector3,
        apex_factor: f64,
        axis: Vector3,
    ) -> Result<Self, &'static str> {
        if !angle_range.iter().all(|value| value.is_finite())
            || ![center.x, center.y, center.z]
                .into_iter()
                .chain(
                    [major, minor, pitch, axis]
                        .into_iter()
                        .flat_map(|vector| [vector.x, vector.y, vector.z]),
                )
                .chain([apex_factor])
                .all(f64::is_finite)
        {
            return Err("helix surface path fields must be finite");
        }
        let major_length = (major.x.powi(2) + major.y.powi(2) + major.z.powi(2)).sqrt();
        let minor_length = (minor.x.powi(2) + minor.y.powi(2) + minor.z.powi(2)).sqrt();
        if !(major_length > 0.0
            && (major_length - minor_length).abs()
                <= EPS_HELIX_SURFACE_RADIUS_RELATIVE * major_length.max(1.0))
        {
            return Err("helix surface path major and minor must define a circular path");
        }

        Ok(Self {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        })
    }
    /// Borrow the payload parameters in constructor order.
    #[must_use]
    pub fn parts(
        &self,
    ) -> (
        &[f64; 2],
        &Point3,
        &Vector3,
        &Vector3,
        &Vector3,
        &f64,
        &Vector3,
    ) {
        (
            &self.angle_range,
            &self.center,
            &self.major,
            &self.minor,
            &self.pitch,
            &self.apex_factor,
            &self.axis,
        )
    }
}

impl TryFrom<HelixPathConstructionWire> for HelixPathConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixPathConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            wire.center,
            wire.major,
            wire.minor,
            wire.pitch,
            wire.apex_factor,
            wire.axis,
        )
    }
}

/// Finite ordered helix curve with a non-degenerate radial frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCurveConstructionWire")]
pub struct HelixCurveConstruction {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
    axis: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HelixCurveConstructionWire {
    angle_range: [f64; 2],
    center: Point3,
    major: Vector3,
    minor: Vector3,
    pitch: Vector3,
    apex_factor: f64,
    axis: Vector3,
}

impl HelixCurveConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        center: Point3,
        major: Vector3,
        minor: Vector3,
        pitch: Vector3,
        apex_factor: f64,
        axis: Vector3,
    ) -> Result<Self, &'static str> {
        if !angle_range.iter().all(|value| value.is_finite())
            || ![center.x, center.y, center.z]
                .into_iter()
                .chain(
                    [major, minor, pitch, axis]
                        .into_iter()
                        .flat_map(|vector| [vector.x, vector.y, vector.z]),
                )
                .chain([apex_factor])
                .all(f64::is_finite)
        {
            return Err("helix curve fields must be finite");
        }
        if angle_range[0] > angle_range[1] {
            return Err("helix curve angle_range must be ordered");
        }
        if [major, minor, axis]
            .iter()
            .any(|vector| vector.norm() <= f64::EPSILON)
        {
            return Err("helix curve major, minor, and axis must be non-degenerate");
        }
        if (major.norm() - minor.norm()).abs() > EPS_HELIX_CURVE_RADIUS {
            return Err("helix curve major and minor radii must agree");
        }

        Ok(Self {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        })
    }
    /// Borrow the payload parameters in constructor order.
    #[must_use]
    pub fn parts(
        &self,
    ) -> (
        &[f64; 2],
        &Point3,
        &Vector3,
        &Vector3,
        &Vector3,
        &f64,
        &Vector3,
    ) {
        (
            &self.angle_range,
            &self.center,
            &self.major,
            &self.minor,
            &self.pitch,
            &self.apex_factor,
            &self.axis,
        )
    }
}

impl TryFrom<HelixCurveConstructionWire> for HelixCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            wire.center,
            wire.major,
            wire.minor,
            wire.pitch,
            wire.apex_factor,
            wire.axis,
        )
    }
}

impl HelixCurveConstruction {
    /// Reverse the native interval and signed path fields.
    pub fn reverse_parameterization(&mut self) {
        self.angle_range = [-self.angle_range[1], -self.angle_range[0]];
        self.minor = Vector3::new(-self.minor.x, -self.minor.y, -self.minor.z);
        self.pitch = Vector3::new(-self.pitch.x, -self.pitch.y, -self.pitch.z);
        self.apex_factor = -self.apex_factor;
    }

    /// Scale lengths atomically and retain the old path when admission fails.
    pub fn try_scale_lengths(&mut self, scale: f64) -> Result<(), &'static str> {
        let vector =
            |value: Vector3| Vector3::new(value.x * scale, value.y * scale, value.z * scale);
        let candidate = Self::try_new(
            self.angle_range,
            Point3::new(
                self.center.x * scale,
                self.center.y * scale,
                self.center.z * scale,
            ),
            vector(self.major),
            vector(self.minor),
            vector(self.pitch),
            self.apex_factor,
            self.axis,
        )?;
        *self = candidate;
        Ok(())
    }
}

/// Finite circular helix profile with a nonzero signed radius.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixCircleProfileWire")]
pub struct HelixCircleProfile {
    length: f64,
    radius: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HelixCircleProfileWire {
    length: f64,
    radius: f64,
}

impl HelixCircleProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(length: f64, radius: f64) -> Result<Self, &'static str> {
        if !length.is_finite() {
            return Err("helix circle profile length must be finite");
        }
        if !radius.is_finite() || radius == 0.0 {
            return Err("helix circle profile radius must be finite and nonzero");
        }
        Ok(Self { length, radius })
    }
    /// Native profile length.
    #[must_use]
    pub const fn length(&self) -> f64 {
        self.length
    }

    /// Signed circular profile radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius
    }
}

impl TryFrom<HelixCircleProfileWire> for HelixCircleProfile {
    type Error = &'static str;
    fn try_from(wire: HelixCircleProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.length, wire.radius)
    }
}

/// Finite non-degenerate linear helix profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixLineProfileWire")]
pub struct HelixLineProfile {
    direction: Vector3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HelixLineProfileWire {
    direction: Vector3,
}

impl HelixLineProfile {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(direction: Vector3) -> Result<Self, &'static str> {
        if ![direction.x, direction.y, direction.z]
            .into_iter()
            .all(f64::is_finite)
            || direction.x * direction.x + direction.y * direction.y + direction.z * direction.z
                <= 0.0
        {
            return Err("helix line profile direction must be finite and non-degenerate");
        }
        Ok(Self { direction })
    }
    /// Finite non-degenerate profile direction.
    #[must_use]
    pub const fn direction(&self) -> Vector3 {
        self.direction
    }
}

impl TryFrom<HelixLineProfileWire> for HelixLineProfile {
    type Error = &'static str;
    fn try_from(wire: HelixLineProfileWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.direction)
    }
}

/// Profile-specific tail of a helix surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HelixSurfaceProfile {
    /// Circular profile swept along the helix.
    Circle(HelixCircleProfile),
    /// Linear profile swept along a direction.
    Line(HelixLineProfile),
}

/// Complete helix-surface construction with finite native intervals.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "HelixSurfaceConstructionWire")]
pub struct HelixSurfaceConstruction {
    angle_range: [f64; 2],
    dimension_range: [f64; 2],
    path: HelixPathConstruction,
    profile: HelixSurfaceProfile,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct HelixSurfaceConstructionWire {
    angle_range: [f64; 2],
    dimension_range: [f64; 2],
    path: HelixPathConstruction,
    profile: HelixSurfaceProfile,
}

impl HelixSurfaceConstruction {
    /// Admit parameters that satisfy the helix payload contract.
    pub fn try_new(
        angle_range: [f64; 2],
        dimension_range: [f64; 2],
        path: HelixPathConstruction,
        profile: HelixSurfaceProfile,
    ) -> Result<Self, &'static str> {
        if !angle_range
            .iter()
            .chain(dimension_range.iter())
            .all(|value| value.is_finite())
        {
            return Err("helix surface angle_range and dimension_range must be finite");
        }
        Ok(Self {
            angle_range,
            dimension_range,
            path,
            profile,
        })
    }
    /// Borrow the payload parameters in constructor order.
    #[must_use]
    pub fn parts(
        &self,
    ) -> (
        &[f64; 2],
        &[f64; 2],
        &HelixPathConstruction,
        &HelixSurfaceProfile,
    ) {
        (
            &self.angle_range,
            &self.dimension_range,
            &self.path,
            &self.profile,
        )
    }
}

impl TryFrom<HelixSurfaceConstructionWire> for HelixSurfaceConstruction {
    type Error = &'static str;
    fn try_from(wire: HelixSurfaceConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.angle_range,
            wire.dimension_range,
            wire.path,
            wire.profile,
        )
    }
}

/// A non-negative native subtype-table index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub struct SubtypeTableIndex(i64);

impl SubtypeTableIndex {
    /// Admit a non-negative native subtype-table index.
    pub fn try_new(index: i64) -> Result<Self, &'static str> {
        if index >= 0 {
            Ok(Self(index))
        } else {
            Err("subtype table index must be non-negative")
        }
    }

    /// Native subtype-table index.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for SubtypeTableIndex {
    type Error = &'static str;
    fn try_from(index: i64) -> Result<Self, Self::Error> {
        Self::try_new(index)
    }
}

impl From<SubtypeTableIndex> for i64 {
    fn from(index: SubtypeTableIndex) -> Self {
        index.get()
    }
}

/// An inline T-spline program and its non-empty companion values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename = "inline",
    try_from = "InlineTSplineSubtransformWire"
)]
pub struct InlineTSplineSubtransform {
    /// Line-oriented topology and geometry program.
    pub program: crate::products::NonEmptyString,
    /// Optional native separator boolean.
    pub separator: Option<bool>,
    /// Companion values program.
    pub values: crate::products::NonEmptyString,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InlineTSplineSubtransformWire {
    Inline {
        program: String,
        separator: Option<bool>,
        values: String,
    },
}

impl TryFrom<InlineTSplineSubtransformWire> for InlineTSplineSubtransform {
    type Error = &'static str;
    fn try_from(wire: InlineTSplineSubtransformWire) -> Result<Self, Self::Error> {
        let InlineTSplineSubtransformWire::Inline {
            program,
            separator,
            values,
        } = wire;
        Self::try_new(program, separator, values)
    }
}

impl InlineTSplineSubtransform {
    /// Admit a non-empty T-spline program and companion values.
    pub fn try_new(
        program: impl Into<String>,
        separator: Option<bool>,
        values: impl Into<String>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            program: crate::products::NonEmptyString::new(program)
                .ok_or("T-spline program must not be empty")?,
            separator,
            values: crate::products::NonEmptyString::new(values)
                .ok_or("T-spline values must not be empty")?,
        })
    }
}

/// Native T-spline subtransform storage form.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TSplineSubtransformWire")]
pub enum TSplineSubtransform {
    /// Inline line-oriented T-spline program and companion values.
    Inline(InlineTSplineSubtransform),
    /// Reference to an earlier subtype-table entry.
    Reference {
        /// Native subtype-table index.
        index: SubtypeTableIndex,
        /// Resolved shared program when the table target is available.
        resolved: Option<Box<InlineTSplineSubtransform>>,
    },
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TSplineSubtransformWire {
    Inline {
        program: String,
        separator: Option<bool>,
        values: String,
    },
    Reference {
        index: SubtypeTableIndex,
        #[serde(default)]
        resolved: Option<Box<InlineTSplineSubtransform>>,
    },
}

impl TryFrom<TSplineSubtransformWire> for TSplineSubtransform {
    type Error = &'static str;
    fn try_from(wire: TSplineSubtransformWire) -> Result<Self, Self::Error> {
        match wire {
            TSplineSubtransformWire::Inline {
                program,
                separator,
                values,
            } => InlineTSplineSubtransform::try_new(program, separator, values).map(Self::Inline),
            TSplineSubtransformWire::Reference { index, resolved } => {
                Ok(Self::Reference { index, resolved })
            }
        }
    }
}

impl Serialize for TSplineSubtransform {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Wire<'a> {
            Reference {
                index: SubtypeTableIndex,
                #[serde(skip_serializing_if = "Option::is_none")]
                resolved: Option<&'a InlineTSplineSubtransform>,
            },
        }
        match self {
            Self::Inline(inline) => inline.serialize(serializer),
            Self::Reference { index, resolved } => Wire::Reference {
                index: *index,
                resolved: resolved.as_deref(),
            }
            .serialize(serializer),
        }
    }
}

impl TSplineSubtransform {
    /// Effective inline program when present or resolved.
    #[must_use]
    pub fn inline(&self) -> Option<&InlineTSplineSubtransform> {
        match self {
            Self::Inline(inline) => Some(inline),
            Self::Reference { resolved, .. } => resolved.as_deref(),
        }
    }
}

/// Complete native `t_spl_sur` wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct TSplineSurfaceConstruction {
    /// Ordered U and V native parameter intervals.
    pub parameter_ranges: [[f64; 2]; 2],
    /// Native T-spline type integer.
    pub type_code: i64,
    /// Inline or referenced shared subtransform object.
    pub subtransform: TSplineSubtransform,
    /// Native trailing integer.
    pub trailing_value: i64,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Revision-gated form fields; absent from the pre-revision layout. The
    /// revision layout stores the shared tail first, then four optional
    /// parameter values (`support_bounds`), the type code as an enum, the
    /// nested subtransform scope, and the trailing integer.
    pub revision_form: Option<RevisionSurfaceForm>,
}

impl TSplineSurfaceConstruction {
    /// Parse the semantic index of the effective topology program.
    #[must_use]
    pub fn program_graph(&self) -> Option<TSplineProgram> {
        self.subtransform
            .inline()
            .map(|inline| TSplineProgram::parse(inline.program.as_str()))
    }

    /// Parse the semantic index of the effective values program.
    #[must_use]
    pub fn values_graph(&self) -> Option<TSplineProgram> {
        self.subtransform
            .inline()
            .map(|inline| TSplineProgram::parse(inline.values.as_str()))
    }
}

#[derive(Serialize)]
struct TSplineSurfaceConstructionWriteWire<'a> {
    parameter_ranges: &'a [[f64; 2]; 2],
    type_code: i64,
    subtransform: &'a TSplineSubtransform,
    #[serde(skip_serializing_if = "Option::is_none")]
    program_graph: Option<&'a TSplineProgram>,
    #[serde(skip_serializing_if = "Option::is_none")]
    values_graph: Option<&'a TSplineProgram>,
    trailing_value: i64,
    discontinuities: &'a [Vec<f64>; 6],
    discontinuity_flag: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision_form: Option<&'a RevisionSurfaceForm>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TSplineSurfaceConstructionReadWire {
    parameter_ranges: [[f64; 2]; 2],
    type_code: i64,
    subtransform: TSplineSubtransform,
    #[serde(default)]
    program_graph: Option<TSplineProgram>,
    #[serde(default)]
    values_graph: Option<TSplineProgram>,
    trailing_value: i64,
    discontinuities: [Vec<f64>; 6],
    discontinuity_flag: bool,
    #[serde(default)]
    revision_form: Option<RevisionSurfaceForm>,
}

impl Serialize for TSplineSurfaceConstruction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let program_graph = self.program_graph();
        let values_graph = self.values_graph();
        TSplineSurfaceConstructionWriteWire {
            parameter_ranges: &self.parameter_ranges,
            type_code: self.type_code,
            subtransform: &self.subtransform,
            program_graph: program_graph.as_ref(),
            values_graph: values_graph.as_ref(),
            trailing_value: self.trailing_value,
            discontinuities: &self.discontinuities,
            discontinuity_flag: self.discontinuity_flag,
            revision_form: self.revision_form.as_ref(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TSplineSurfaceConstruction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TSplineSurfaceConstructionReadWire::deserialize(deserializer)?;
        let construction = Self {
            parameter_ranges: wire.parameter_ranges,
            type_code: wire.type_code,
            subtransform: wire.subtransform,
            trailing_value: wire.trailing_value,
            discontinuities: wire.discontinuities,
            discontinuity_flag: wire.discontinuity_flag,
            revision_form: wire.revision_form,
        };
        if wire
            .program_graph
            .as_ref()
            .is_some_and(|graph| Some(graph) != construction.program_graph().as_ref())
        {
            return Err(serde::de::Error::custom(
                "program_graph does not match the T-spline program",
            ));
        }
        if wire
            .values_graph
            .as_ref()
            .is_some_and(|graph| Some(graph) != construction.values_graph().as_ref())
        {
            return Err(serde::de::Error::custom(
                "values_graph does not match the T-spline values program",
            ));
        }
        Ok(construction)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for TSplineSurfaceConstruction {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "TSplineSurfaceConstruction".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        TSplineSurfaceConstructionReadWire::json_schema(generator)
    }
}

/// Parsed line-oriented T-spline subtransform program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TSplineProgram {
    /// Ordered recognized header declarations.
    pub headers: Vec<TSplineProgramLine>,
    /// Ordered recognized topology, geometry, and constraint records.
    pub records: Vec<TSplineProgramLine>,
    /// Non-comment lines outside the defined vocabulary.
    pub unparsed_lines: Vec<String>,
}

/// One tokenized T-spline program line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct TSplineProgramLine {
    /// Leading record or header token.
    pub kind: String,
    /// Ordered remaining fields without interpretation loss.
    pub fields: Vec<String>,
}

impl TSplineProgram {
    /// Parse the defined line vocabulary while retaining every other line.
    #[must_use]
    pub fn parse(program: &str) -> Self {
        const HEADERS: &[&str] = &[
            "degree",
            "cap_type",
            "units",
            "end_conditions",
            "star_knot_rule",
            "star_smoothness",
            "tol",
            "ver",
            "behavior_version",
            "geom_tol",
            "compat_version",
        ];
        const RECORDS: &[&str] = &[
            "f",
            "e",
            "v",
            "l",
            "ec",
            "0m",
            "0g",
            "100edges",
            "100verts",
            "105sym",
            "105plane",
            "105a",
            "106ek",
            "50000grip",
        ];
        let mut parsed = Self {
            headers: Vec::new(),
            records: Vec::new(),
            unparsed_lines: Vec::new(),
        };
        for line in program.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split_whitespace();
            let Some(kind) = fields.next() else { continue };
            let parsed_line = TSplineProgramLine {
                kind: kind.into(),
                fields: fields.map(String::from).collect(),
            };
            if HEADERS.contains(&kind) {
                parsed.headers.push(parsed_line);
            } else if RECORDS.contains(&kind) {
                parsed.records.push(parsed_line);
            } else {
                parsed.unparsed_lines.push(line.into());
            }
        }
        parsed
    }
}

/// One oriented support of a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BlendSupport {
    /// The support surface.
    pub surface: SurfaceId,
    /// Selects the opposite surface-normal side when true.
    #[serde(default)]
    pub reversed: bool,
}

/// One parameter station of a rolling-ball jet, with its complete value rows.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetStation {
    /// Native spine parameter.
    pub knot: f64,
    /// Multiplicity of this parameter in the native knot vector.
    pub multiplicity: u32,
    /// Values and derivatives at this parameter.
    pub site: RollingBallJetSite,
}

const EPS_ROLLING_BALL_RADIUS: f64 = 1.0e-9;

/// Degree and finite clamped station data of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "RollingBallJetReadWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetStations {
    degree: u32,
    stations: Vec<RollingBallJetStation>,
}

impl RollingBallJetStations {
    /// Admit clamped increasing knots and finite equal-radius station data.
    pub fn try_new(
        degree: u32,
        stations: Vec<RollingBallJetStation>,
    ) -> Result<Self, &'static str> {
        let maximum_multiplicity = degree.checked_add(1).filter(|_| degree != 0).ok_or(
            "rolling-ball jet degree must be positive with a representable end multiplicity",
        )?;
        if stations.len() < 2 {
            return Err("rolling-ball jet stations must contain at least two rows");
        }
        if stations[0].multiplicity != maximum_multiplicity
            || stations[stations.len() - 1].multiplicity != maximum_multiplicity
            || stations.iter().any(|station| {
                station.multiplicity == 0 || station.multiplicity > maximum_multiplicity
            })
        {
            return Err(
                "rolling-ball jet multiplicities must be in 1..=degree+1 with clamped ends",
            );
        }
        if stations.iter().any(|station| !station.knot.is_finite())
            || stations.windows(2).any(|pair| pair[0].knot >= pair[1].knot)
        {
            return Err("rolling-ball jet knots must be finite and strictly increasing");
        }
        for station in &stations {
            let site = &station.site;
            if [site.first_limit, site.second_limit, site.center]
                .iter()
                .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
                || !site.angle.is_finite()
            {
                return Err("rolling-ball jet site coordinates and angle must be finite");
            }
            for derivative in [&site.first_derivative, &site.second_derivative] {
                if [
                    derivative.first_limit,
                    derivative.second_limit,
                    derivative.center,
                ]
                .iter()
                .any(|vector| {
                    ![vector.x, vector.y, vector.z]
                        .into_iter()
                        .all(f64::is_finite)
                }) || !derivative.angle.is_finite()
                {
                    return Err("rolling-ball jet site derivatives must be finite");
                }
            }
            let first_radius = site.first_limit.distance(site.center);
            let second_radius = site.second_limit.distance(site.center);
            if !first_radius.is_finite()
                || first_radius <= 0.0
                || !second_radius.is_finite()
                || (first_radius - second_radius).abs()
                    > EPS_ROLLING_BALL_RADIUS * first_radius.max(second_radius).max(1.0)
            {
                return Err("rolling-ball jet site radii must be finite and agree within tolerance, with a positive first radius");
            }
        }
        Ok(Self { degree, stations })
    }

    /// Return the polynomial degree of each scalar channel.
    #[must_use]
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Return the ordered station data.
    #[must_use]
    pub fn stations(&self) -> &[RollingBallJetStation] {
        &self.stations
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct RollingBallJetReadWire {
    degree: u32,
    knots: Vec<f64>,
    multiplicities: Vec<u32>,
    sites: Vec<RollingBallJetSite>,
}

impl TryFrom<RollingBallJetReadWire> for RollingBallJetStations {
    type Error = &'static str;

    fn try_from(wire: RollingBallJetReadWire) -> Result<Self, Self::Error> {
        if wire.knots.len() != wire.multiplicities.len() || wire.knots.len() != wire.sites.len() {
            return Err(
                "rolling-ball jet knots, multiplicities, and sites must have equal lengths",
            );
        }
        let stations = wire
            .knots
            .into_iter()
            .zip(wire.multiplicities)
            .zip(wire.sites)
            .map(|((knot, multiplicity), site)| RollingBallJetStation {
                knot,
                multiplicity,
                site,
            })
            .collect();
        Self::try_new(wire.degree, stations)
    }
}

#[derive(Serialize)]
struct RollingBallJetWriteWire<'a> {
    degree: u32,
    knots: Vec<f64>,
    multiplicities: Vec<u32>,
    sites: Vec<&'a RollingBallJetSite>,
}

impl Serialize for RollingBallJetStations {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollingBallJetWriteWire {
            degree: self.degree,
            knots: self.stations.iter().map(|station| station.knot).collect(),
            multiplicities: self
                .stations
                .iter()
                .map(|station| station.multiplicity)
                .collect(),
            sites: self.stations.iter().map(|station| &station.site).collect(),
        }
        .serialize(serializer)
    }
}

/// One aligned knot site of an exact rolling-ball surface jet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetSite {
    /// First limiting point at the knot.
    pub first_limit: Point3,
    /// Second limiting point at the knot.
    pub second_limit: Point3,
    /// Rolling-ball center at the knot.
    pub center: Point3,
    /// Signed opening angle at the knot, in radians.
    pub angle: f64,
    /// First parameter derivative of all four value channels.
    pub first_derivative: RollingBallJetDerivative,
    /// Second parameter derivative of all four value channels.
    pub second_derivative: RollingBallJetDerivative,
}

/// One derivative row for the four channels of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetDerivative {
    /// Derivative of the first limiting point.
    pub first_limit: Vector3,
    /// Derivative of the second limiting point.
    pub second_limit: Vector3,
    /// Derivative of the rolling-ball center.
    pub center: Vector3,
    /// Derivative of the signed opening angle.
    pub angle: f64,
}

/// Cross-section family of a procedural blend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BlendCrossSection {
    /// Constant-radius circular cross-section.
    Circular,
    /// Conic (non-circular quadric) cross-section.
    Conic,
    /// Free-form polynomial cross-section.
    Polynomial,
}

/// Shared fields of a revision-gated spline-surface form: the revision
/// integer, optional support bounds and reference-curve endpoints, a
/// carrier-specific boolean run, and the shared tail enum, discontinuity
/// arrays, tail boolean, and post-tail boolean run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RevisionSurfaceForm<F: Default = Vec<bool>> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Optional U/V bound fields following the support surface.
    #[serde(default)]
    pub support_bounds: [Option<f64>; 4],
    /// Optional parameter endpoints following the embedded reference curve.
    #[serde(default)]
    pub reference_endpoints: [Option<f64>; 2],
    /// Optional parameter endpoints following a second embedded curve, used
    /// by two-curve carriers such as `sum_spl_sur`.
    #[serde(default)]
    pub second_endpoints: [Option<f64>; 2],
    /// Carrier-specific boolean run preceding the shared tail.
    #[serde(default)]
    pub flags: F,
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "revision_surface_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "RevisionSurfaceCacheSchemaWire"))]
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Boolean run following the shared tail.
    #[serde(default)]
    pub trailing_flags: Vec<bool>,
}

impl<F: Default> RevisionSurfaceForm<F> {
    fn with_flags<G: Default>(self, flags: G) -> RevisionSurfaceForm<G> {
        RevisionSurfaceForm {
            revision: self.revision,
            support_bounds: self.support_bounds,
            reference_endpoints: self.reference_endpoints,
            second_endpoints: self.second_endpoints,
            flags,
            cache: self.cache,
            discontinuities: self.discontinuities,
            tail_flag: self.tail_flag,
            trailing_flags: self.trailing_flags,
        }
    }
}

/// Mutually exclusive payloads of a revision-gated approximation cache.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum RevisionCacheForm<P = RevisionSurfaceParameterization> {
    /// A solved cache followed by its carrier-specific cache contract.
    SolvedCache {
        /// Carrier-specific solved-cache contract.
        fit_tolerance: FitTolerance,
    },
    /// Parameterization stored in place of a solved cache.
    Parameterization(P),
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(
    dead_code,
    reason = "fields define the revision-surface cache wire schema"
)]
struct RevisionSurfaceCacheSchemaWire {
    tail_enum: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tail_parameterization: Option<RevisionSurfaceParameterization>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the cache-first curve wire schema")]
struct CacheFirstCurveCacheSchemaWire {
    cache_enum: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameterization: Option<CacheFirstCurveParameterization>,
}

impl<P> RevisionCacheForm<P> {
    /// Native selector emitted for this cache form.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::SolvedCache { .. } => 0,
            Self::Parameterization(_) => 2,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&P> {
        match self {
            Self::SolvedCache { .. } => None,
            Self::Parameterization(parameterization) => Some(parameterization),
        }
    }

    /// Fit tolerance carried by a solved cache.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::SolvedCache { fit_tolerance } => Some(fit_tolerance.get()),
            Self::Parameterization(_) => None,
        }
    }
}

/// Approximation state and its dependent fit contract for a variable blend.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum VariableBlendCache {
    /// A nonzero approximation-current flag with an active fit contract.
    Current {
        /// Native approximation-current flag.
        shape_prefix: NonZeroI64,
        /// Fit tolerance in document length units.
        fit_tolerance: FitTolerance,
    },
    /// A zero approximation-current flag without an active fit contract.
    Stale,
    /// Parameterization in place of a solved cache.
    Parameterization {
        /// Native approximation-current flag, independent of the parameterization.
        shape_prefix: i64,
        /// Surface parameterization.
        parameterization: RevisionSurfaceParameterization,
    },
}

impl VariableBlendCache {
    /// Native approximation-current flag.
    #[must_use]
    pub const fn shape_prefix(&self) -> i64 {
        match self {
            Self::Current { shape_prefix, .. } => shape_prefix.get(),
            Self::Stale => 0,
            Self::Parameterization { shape_prefix, .. } => *shape_prefix,
        }
    }

    /// Native tail selector.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::Current { .. } | Self::Stale => 0,
            Self::Parameterization { .. } => 2,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&RevisionSurfaceParameterization> {
        match self {
            Self::Parameterization {
                parameterization, ..
            } => Some(parameterization),
            _ => None,
        }
    }

    /// Active fit tolerance, absent for a stale or parameterized approximation.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::Current { fit_tolerance, .. } => Some(fit_tolerance.get()),
            _ => None,
        }
    }
}

mod revision_surface_cache_wire {
    use super::{FitTolerance, RevisionCacheForm, RevisionSurfaceParameterization};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    struct SolvedWire {
        tail_enum: i64,
    }

    #[derive(Serialize)]
    struct ParameterizationWriteWire<'a> {
        tail_enum: i64,
        tail_parameterization: &'a RevisionSurfaceParameterization,
    }

    #[derive(Deserialize)]
    struct ReadWire {
        #[serde(alias = "cache_selector")]
        tail_enum: i64,
        #[serde(default)]
        tail_parameterization: Option<RevisionSurfaceParameterization>,
        #[serde(default)]
        cache_fit_tolerance: Option<FitTolerance>,
    }

    pub fn serialize<S>(value: &RevisionCacheForm, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            RevisionCacheForm::SolvedCache { .. } => {
                SolvedWire { tail_enum: 0 }.serialize(serializer)
            }
            RevisionCacheForm::Parameterization(parameterization) => ParameterizationWriteWire {
                tail_enum: 2,
                tail_parameterization: parameterization,
            }
            .serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<RevisionCacheForm, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReadWire::deserialize(deserializer)?;
        match (
            wire.tail_enum,
            wire.tail_parameterization,
            wire.cache_fit_tolerance,
        ) {
            (0, None, Some(fit_tolerance)) => {
                Ok(RevisionCacheForm::SolvedCache { fit_tolerance })
            }
            (2, Some(parameterization), None) => {
                Ok(RevisionCacheForm::Parameterization(parameterization))
            }
            (selector, _, _) => Err(serde::de::Error::custom(format_args!(
                "tail_enum must be 0 with cache_fit_tolerance or 2 with tail_parameterization, got {selector}"
            ))),
        }
    }
}

mod variable_blend_cache_wire {
    use super::{FitTolerance, RevisionSurfaceParameterization, VariableBlendCache};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::num::NonZeroI64;

    #[derive(Serialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    pub(super) struct WriteWire<'a> {
        shape_prefix: i64,
        tail_enum: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        tail_parameterization: Option<&'a RevisionSurfaceParameterization>,
    }

    #[derive(Deserialize)]
    struct ReadWire {
        shape_prefix: i64,
        #[serde(alias = "cache_selector")]
        tail_enum: i64,
        #[serde(default)]
        tail_parameterization: Option<RevisionSurfaceParameterization>,
        #[serde(default)]
        cache_fit_tolerance: Option<FitTolerance>,
    }

    pub fn serialize<S>(value: &VariableBlendCache, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        WriteWire {
            shape_prefix: value.shape_prefix(),
            tail_enum: value.selector(),
            tail_parameterization: value.parameterization(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VariableBlendCache, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReadWire::deserialize(deserializer)?;
        match (wire.tail_enum, wire.tail_parameterization, wire.cache_fit_tolerance, NonZeroI64::new(wire.shape_prefix)) {
            (0, None, Some(fit_tolerance), Some(shape_prefix)) => Ok(VariableBlendCache::Current { shape_prefix, fit_tolerance }),
            (0, None, None, None) => Ok(VariableBlendCache::Stale),
            (2, Some(parameterization), None, _) => Ok(VariableBlendCache::Parameterization { shape_prefix: wire.shape_prefix, parameterization }),
            _ => Err(serde::de::Error::custom("variable-blend cache requires a nonzero prefix with a fit tolerance, a zero prefix without a fit tolerance, or tail_enum 2 with parameterization")),
        }
    }
}

mod cache_first_curve_cache_wire {
    use super::{CacheFirstCurveParameterization, FitTolerance, RevisionCacheForm};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    struct SolvedWire {
        cache_enum: i64,
    }

    #[derive(Serialize)]
    struct ParameterizationWriteWire<'a> {
        cache_enum: i64,
        parameterization: &'a CacheFirstCurveParameterization,
    }

    #[derive(Deserialize)]
    struct ReadWire {
        cache_enum: i64,
        #[serde(default)]
        parameterization: Option<CacheFirstCurveParameterization>,
        #[serde(default)]
        cache_fit_tolerance: Option<FitTolerance>,
    }

    pub fn serialize<S>(
        value: &RevisionCacheForm<CacheFirstCurveParameterization>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            RevisionCacheForm::SolvedCache { .. } => {
                SolvedWire { cache_enum: 0 }.serialize(serializer)
            }
            RevisionCacheForm::Parameterization(parameterization) => ParameterizationWriteWire {
                cache_enum: 2,
                parameterization,
            }
            .serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<RevisionCacheForm<CacheFirstCurveParameterization>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReadWire::deserialize(deserializer)?;
        match (
            wire.cache_enum,
            wire.parameterization,
            wire.cache_fit_tolerance,
        ) {
            (0, None, Some(fit_tolerance)) => {
                Ok(RevisionCacheForm::SolvedCache { fit_tolerance })
            }
            (2, Some(parameterization), None) => {
                Ok(RevisionCacheForm::Parameterization(parameterization))
            }
            (selector, _, _) => Err(serde::de::Error::custom(format_args!(
                "cache_enum must be 0 with cache_fit_tolerance or 2 with parameterization, got {selector}"
            ))),
        }
    }
}

/// Parameterization carried by tail-enum form `2` of the shared revision-gated
/// spline-surface tail. This form stores no approximation cache and no fit
/// tolerance; it stores the two parameter intervals followed by four enums, in
/// the order the fields appear below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RevisionSurfaceParameterization {
    /// U parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    #[serde(default)]
    pub u_interval: [Option<f64>; 2],
    /// V parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    #[serde(default)]
    pub v_interval: [Option<f64>; 2],
    /// U closure enum.
    pub u_closure: i64,
    /// V closure enum.
    pub v_closure: i64,
    /// U singularity enum.
    pub u_singularity: i64,
    /// V singularity enum.
    pub v_singularity: i64,
}

/// Subtype-specific tail of a native taper spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaperSurfaceKind {
    /// Standard taper without a subtype-specific tail.
    Standard,
    /// Orthogonal taper with a native sense flag.
    Orthogonal {
        /// Native orientation sense.
        sense: bool,
    },
    /// Edge taper with a model-space draft vector.
    Edge {
        /// Native draft vector.
        draft: Vector3,
    },
    /// Shadow taper with a pre-factored draft angle.
    Shadow {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
    },
    /// Ruled taper with a pre-factored angle and factor.
    Ruled {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
        /// Native ruled-taper factor.
        factor: f64,
    },
    /// Swept taper with a pre-factored draft angle.
    Swept {
        /// Native draft vector.
        draft: Vector3,
        /// Stored draft-angle sine.
        sine: f64,
        /// Stored draft-angle cosine.
        cosine: f64,
    },
}

/// One scalar row in native loft subdata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LoftSubdataRow {
    /// Leading ordered scalar pair.
    pub parameters: [f64; 2],
    /// Ordered per-column scalar pairs; empty for subdata type 211.
    pub columns: Vec<[f64; 2]>,
    /// Trailing scalar pair stored by the revision-gated row encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<[f64; 2]>,
}

/// Native loft constraint table with structurally consistent dimensions.
#[derive(Debug, Clone, PartialEq)]
pub enum LoftSubdata {
    /// Type 211 stores exactly one leading pair and no column pairs.
    Type211 {
        /// Native row/column header values. They do not count this form's payload.
        dimensions: [i64; 2],
        /// The sole leading scalar pair.
        row: [f64; 2],
    },
    /// All other table types store rows of one shared column width.
    Table(LoftSubdataTable),
}

/// Checked non-211 loft table payload.
#[derive(Debug, Clone, PartialEq)]
pub struct LoftSubdataTable {
    type_code: i64,
    rows: Vec<LoftSubdataRow>,
}

impl LoftSubdata {
    /// Construct the fixed type-211 form.
    #[must_use]
    pub fn type_211(dimensions: [i64; 2], row: [f64; 2]) -> Self {
        Self::Type211 { dimensions, row }
    }

    /// Construct a non-211 table whose rows have one shared column width.
    #[must_use]
    pub fn table(type_code: i64, rows: Vec<LoftSubdataRow>) -> Option<Self> {
        if type_code == 211
            || rows.len() > i64::MAX as usize
            || rows
                .first()
                .is_some_and(|row| row.columns.len() > i64::MAX as usize)
        {
            return None;
        }
        let column_count = rows.first().map_or(0, |row| row.columns.len());
        rows.iter()
            .all(|row| row.columns.len() == column_count)
            .then_some(Self::Table(LoftSubdataTable { type_code, rows }))
    }

    /// Native table type discriminator.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Type211 { .. } => 211,
            Self::Table(table) => table.type_code,
        }
    }

    /// Native row header: a stored value for type 211, otherwise the row count.
    #[must_use]
    pub fn row_count(&self) -> i64 {
        match self {
            Self::Type211 { dimensions, .. } => dimensions[0],
            Self::Table(table) => table.rows.len() as i64,
        }
    }

    /// Native column header: stored for type 211, otherwise the shared row width.
    #[must_use]
    pub fn column_count(&self) -> i64 {
        match self {
            Self::Type211 { dimensions, .. } => dimensions[1],
            Self::Table(table) => table.rows.first().map_or(0, |row| row.columns.len() as i64),
        }
    }

    /// Visit the leading and column pairs in each row.
    pub fn visit_rows(&self, mut visit: impl FnMut(&[f64; 2], &[[f64; 2]], Option<&[f64; 2]>)) {
        match self {
            Self::Type211 { row, .. } => visit(row, &[], None),
            Self::Table(table) => {
                for row in &table.rows {
                    visit(&row.parameters, &row.columns, row.extra.as_ref());
                }
            }
        }
    }

    /// Whether every leading and per-column scalar is finite.
    #[must_use]
    pub(crate) fn row_values_are_finite(&self) -> bool {
        let mut valid = true;
        self.visit_rows(|parameters, columns, _extra| {
            valid &= parameters.iter().all(|value| value.is_finite())
                && columns.iter().flatten().all(|value| value.is_finite());
        });
        valid
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LoftSubdataWire {
    type_code: i64,
    row_count: i64,
    column_count: i64,
    rows: Vec<LoftSubdataRow>,
}

impl Serialize for LoftSubdata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("LoftSubdata", 4)?;
        state.serialize_field("type_code", &self.type_code())?;
        state.serialize_field("row_count", &self.row_count())?;
        state.serialize_field("column_count", &self.column_count())?;
        match self {
            Self::Type211 { row, .. } => {
                let row = LoftSubdataRow {
                    parameters: *row,
                    columns: Vec::new(),
                    extra: None,
                };
                state.serialize_field("rows", std::slice::from_ref(&row))?;
            }
            Self::Table(table) => state.serialize_field("rows", &table.rows)?,
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for LoftSubdata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = LoftSubdataWire::deserialize(deserializer)?;
        if wire.type_code == 211 {
            let [row] = wire.rows.as_slice() else {
                return Err(serde::de::Error::custom(
                    "loft subdata type 211 requires exactly one row",
                ));
            };
            if !row.columns.is_empty() || row.extra.is_some() {
                return Err(serde::de::Error::custom(
                    "loft subdata type 211 forbids columns and a trailing pair",
                ));
            }
            return Ok(Self::type_211(
                [wire.row_count, wire.column_count],
                row.parameters,
            ));
        }
        let row_count = usize::try_from(wire.row_count)
            .map_err(|_| serde::de::Error::custom("loft subdata row_count is negative"))?;
        let column_count = usize::try_from(wire.column_count)
            .map_err(|_| serde::de::Error::custom("loft subdata column_count is negative"))?;
        if wire.rows.len() != row_count {
            return Err(serde::de::Error::custom(
                "loft subdata row_count does not match rows",
            ));
        }
        if wire
            .rows
            .iter()
            .any(|row| row.columns.len() != column_count)
        {
            return Err(serde::de::Error::custom(
                "loft subdata column_count does not match every row",
            ));
        }
        Self::table(wire.type_code, wire.rows).ok_or_else(|| {
            serde::de::Error::custom("loft subdata dimensions exceed the wire representation")
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for LoftSubdata {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LoftSubdata".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        LoftSubdataWire::json_schema(generator)
    }
}

/// Surface-side constraint attached to one loft profile curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "LoftProfileData"))]
struct LoftProfileDataWire {
    /// Constraint support surface, absent for the native `null_surface`
    /// sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surface: Option<SurfaceId>,
    /// Optional U/V bound fields following the support surface in the
    /// revision-gated encoding.
    #[serde(default)]
    support_bounds: [Option<f64>; 4],
    /// UV curve on the support, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pcurve: Option<PcurveGeometry>,
    /// Second UV curve slot, carried only by the type-zero member form and
    /// absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_pcurve: Option<PcurveGeometry>,
    /// First native constraint flag, absent from the type-zero member form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_flag: Option<bool>,
    /// ASM extension integer following the first flag, absent from member
    /// forms that omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asm_extension: Option<i64>,
    /// Native constraint table.
    subdata: LoftSubdata,
    /// Optional direction selected by the second native flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direction: Option<Vector3>,
}

/// The complete constraint payload of a classic loft or skin profile.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "LoftProfileDataWire")]
pub struct ClassicLoftProfileData {
    /// Required support surface.
    pub surface: SurfaceId,
    /// Nullable parameter curve on the support.
    pub pcurve: Option<PcurveGeometry>,
    /// First native constraint flag.
    pub first_flag: bool,
    /// ASM extension integer preceding the subdata.
    pub asm_extension: i64,
    /// Native constraint table.
    pub subdata: LoftSubdata,
    /// Optional direction selected by the second native flag.
    pub direction: Option<Vector3>,
}

impl TryFrom<LoftProfileDataWire> for ClassicLoftProfileData {
    type Error = String;

    fn try_from(wire: LoftProfileDataWire) -> Result<Self, Self::Error> {
        if wire.support_bounds.iter().any(Option::is_some) {
            return Err("classic loft data.support_bounds must be unbounded".into());
        }
        if wire.secondary_pcurve.is_some() {
            return Err("classic loft data.secondary_pcurve must be absent".into());
        }
        Ok(Self {
            surface: wire
                .surface
                .ok_or("classic loft data.surface is required")?,
            pcurve: wire.pcurve,
            first_flag: wire
                .first_flag
                .ok_or("classic loft data.first_flag is required")?,
            asm_extension: wire
                .asm_extension
                .ok_or("classic loft data.asm_extension is required")?,
            subdata: wire.subdata,
            direction: wire.direction,
        })
    }
}

impl Serialize for ClassicLoftProfileData {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let field_count =
            5 + usize::from(self.pcurve.is_some()) + usize::from(self.direction.is_some());
        let mut wire = serializer.serialize_struct("LoftProfileData", field_count)?;
        wire.serialize_field("surface", &self.surface)?;
        wire.serialize_field("support_bounds", &[None::<f64>; 4])?;
        if let Some(pcurve) = &self.pcurve {
            wire.serialize_field("pcurve", pcurve)?;
        }
        wire.serialize_field("first_flag", &self.first_flag)?;
        wire.serialize_field("asm_extension", &self.asm_extension)?;
        wire.serialize_field("subdata", &self.subdata)?;
        if let Some(direction) = &self.direction {
            wire.serialize_field("direction", direction)?;
        }
        wire.end()
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ClassicLoftProfileData {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LoftProfileData".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        LoftProfileDataWire::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        LoftProfileDataWire::json_schema(generator)
    }
}

/// Type-selected fields of one loft profile member.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum LoftMemberForm {
    /// Support-surface form. Legacy layouts can use type zero; revision-gated
    /// layouts select this form with a nonzero type code.
    Support {
        /// Native member type discriminator.
        type_code: i64,
        /// Constraint support surface, absent for the native `null_surface`
        /// sentinel.
        surface: Option<SurfaceId>,
        /// Optional U/V bound fields following the support surface in the
        /// revision-gated encoding.
        support_bounds: [Option<f64>; 4],
        /// UV curve on the support, absent for `nullbs`.
        pcurve: Option<PcurveGeometry>,
        /// First native constraint flag.
        first_flag: bool,
        /// ASM extension integer when the stream version carries it.
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata,
        /// Optional direction selected by the second native flag.
        direction: Option<Vector3>,
    },
    /// Revision-gated type-zero form with two nullable UV curve slots.
    PcurvePair {
        /// First UV curve slot, absent for `nullbs`.
        pcurve: Option<PcurveGeometry>,
        /// Second UV curve slot, absent for `nullbs`.
        secondary_pcurve: Option<PcurveGeometry>,
        /// ASM extension integer when the stream version carries it.
        asm_extension: Option<i64>,
        /// Native constraint table.
        subdata: LoftSubdata,
        /// Optional direction selected by the second native flag.
        direction: Option<Vector3>,
    },
}

impl LoftMemberForm {
    /// Return the native type code selected by this form.
    #[must_use]
    pub fn type_code(&self) -> i64 {
        match self {
            Self::Support { type_code, .. } => *type_code,
            Self::PcurvePair { .. } => 0,
        }
    }

    /// Return the support surface when this is the support form.
    #[must_use]
    pub fn surface(&self) -> Option<&SurfaceId> {
        match self {
            Self::Support { surface, .. } => surface.as_ref(),
            Self::PcurvePair { .. } => None,
        }
    }

    /// Return the first pcurve slot.
    #[must_use]
    pub fn pcurve(&self) -> Option<&PcurveGeometry> {
        match self {
            Self::Support { pcurve, .. } | Self::PcurvePair { pcurve, .. } => pcurve.as_ref(),
        }
    }

    /// Return the constraint subdata.
    #[must_use]
    pub fn subdata(&self) -> &LoftSubdata {
        match self {
            Self::Support { subdata, .. } | Self::PcurvePair { subdata, .. } => subdata,
        }
    }

    /// Return the optional direction selected by the second native flag.
    #[must_use]
    pub fn direction(&self) -> Option<&Vector3> {
        match self {
            Self::Support { direction, .. } | Self::PcurvePair { direction, .. } => {
                direction.as_ref()
            }
        }
    }
}

/// One referenced curve together with its optional native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LoftPathCurve {
    /// Referenced curve carrier.
    #[serde(rename = "curve")]
    pub id: CurveId,
    /// Optional parameter endpoints following the curve in a revision-gated encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<[Option<f64>; 2]>,
}

/// One curve member of a loft profile.
#[derive(Debug, Clone, PartialEq)]
pub struct LoftProfileMember {
    /// Profile curve and its revision-gated parameter endpoints.
    pub curve: LoftPathCurve,
    /// Structurally selected surface-side constraint form.
    pub form: LoftMemberForm,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LoftProfileMemberWire {
    type_code: i64,
    curve: CurveId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoints: Option<[Option<f64>; 2]>,
    data: LoftProfileDataWire,
}

impl Serialize for LoftProfileMember {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let data = match &self.form {
            LoftMemberForm::Support {
                surface,
                support_bounds,
                pcurve,
                first_flag,
                asm_extension,
                subdata,
                direction,
                ..
            } => LoftProfileDataWire {
                surface: surface.clone(),
                support_bounds: *support_bounds,
                pcurve: pcurve.clone(),
                secondary_pcurve: None,
                first_flag: Some(*first_flag),
                asm_extension: *asm_extension,
                subdata: subdata.clone(),
                direction: *direction,
            },
            LoftMemberForm::PcurvePair {
                pcurve,
                secondary_pcurve,
                asm_extension,
                subdata,
                direction,
            } => LoftProfileDataWire {
                surface: None,
                support_bounds: [None; 4],
                pcurve: pcurve.clone(),
                secondary_pcurve: secondary_pcurve.clone(),
                first_flag: None,
                asm_extension: *asm_extension,
                subdata: subdata.clone(),
                direction: *direction,
            },
        };
        LoftProfileMemberWire {
            type_code: self.form.type_code(),
            curve: self.curve.id.clone(),
            endpoints: self.curve.endpoints,
            data,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LoftProfileMember {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LoftProfileMemberWire::deserialize(deserializer)?;
        let LoftProfileDataWire {
            surface,
            support_bounds,
            pcurve,
            secondary_pcurve,
            first_flag,
            asm_extension,
            subdata,
            direction,
        } = wire.data;
        let form = match first_flag {
            Some(first_flag) if secondary_pcurve.is_none() => LoftMemberForm::Support {
                type_code: wire.type_code,
                surface,
                support_bounds,
                pcurve,
                first_flag,
                asm_extension,
                subdata,
                direction,
            },
            Some(_) => {
                return Err(serde::de::Error::custom(
                    "loft support form cannot carry secondary_pcurve",
                ));
            }
            None if wire.type_code == 0
                && surface.is_none()
                && support_bounds.iter().all(Option::is_none) =>
            {
                LoftMemberForm::PcurvePair {
                    pcurve,
                    secondary_pcurve,
                    asm_extension,
                    subdata,
                    direction,
                }
            }
            None if wire.type_code == 0 => {
                return Err(serde::de::Error::custom(
                    "loft pcurve-pair form cannot carry a support surface or support_bounds",
                ));
            }
            None => {
                return Err(serde::de::Error::custom(
                    "nonzero loft type_code requires data.first_flag",
                ));
            }
        };
        Ok(Self {
            curve: LoftPathCurve {
                id: wire.curve,
                endpoints: wire.endpoints,
            },
            form,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for LoftProfileMember {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LoftProfileMember".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        LoftProfileMemberWire::json_schema(generator)
    }
}

/// Native path data attached to one loft section entry.
#[derive(Debug, Clone, PartialEq)]
pub struct LoftPath {
    /// Primary path curve and its optional endpoints, absent for `null_curve`.
    pub curve: Option<LoftPathCurve>,
    /// Ordered auxiliary BS3 curves.
    pub auxiliaries: Vec<CurveId>,
    /// Native path tail integer.
    pub flag: i64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LoftPathWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CurveId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoints: Option<[Option<f64>; 2]>,
    auxiliaries: Vec<CurveId>,
    flag: i64,
}

impl Serialize for LoftPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        LoftPathWire {
            curve: self.curve.as_ref().map(|curve| curve.id.clone()),
            endpoints: self.curve.as_ref().and_then(|curve| curve.endpoints),
            auxiliaries: self.auxiliaries.clone(),
            flag: self.flag,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LoftPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = LoftPathWire::deserialize(deserializer)?;
        let curve = match (wire.curve, wire.endpoints) {
            (Some(id), endpoints) => Some(LoftPathCurve { id, endpoints }),
            (None, None) => None,
            (None, Some(_)) => {
                return Err(serde::de::Error::custom(
                    "loft path endpoints require a curve",
                ));
            }
        };
        Ok(Self {
            curve,
            auxiliaries: wire.auxiliaries,
            flag: wire.flag,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for LoftPath {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LoftPath".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        LoftPathWire::json_schema(generator)
    }
}

/// One parameterized entry in a native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LoftSectionEntry {
    /// Native section parameter.
    pub parameter: f64,
    /// Ordered profile members.
    pub profile: Vec<LoftProfileMember>,
    /// Native path data.
    pub path: LoftPath,
}

/// Revision-gated `loft_spl_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LoftRevisionForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Four booleans following the parameter intervals.
    #[serde(default)]
    pub flags: [bool; 4],
    /// Two integers preceding the shared tail.
    #[serde(default)]
    pub ints: [i64; 2],
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "revision_surface_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "RevisionSurfaceCacheSchemaWire"))]
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
}

/// Ordered native loft section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LoftSection {
    /// Ordered entries in the section.
    pub entries: Vec<LoftSectionEntry>,
}

/// Token retained from the variable bridge preceding a loft solved cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LoftBridgeToken {
    /// Native boolean token.
    Boolean(bool),
    /// Native integer token.
    Integer(i64),
    /// Native double token.
    Double(f64),
    /// Native string token.
    Text(String),
    /// Native enum token.
    Enum(i64),
}

/// Common carrier fields of one G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct G2BlendSide {
    /// Native side label.
    pub label: String,
    /// Primary support surface.
    pub surface: SurfaceId,
    /// Primary side curve.
    pub curve: CurveId,
    /// First and second ordered BS2 pcurves; each may be `nullbs`.
    pub pcurves: [Option<PcurveGeometry>; 2],
    /// Native side direction.
    pub direction: Vector3,
}

/// Singularity-specific payload of the first G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum G2BlendFirstShape {
    /// Full singularity with an optional BS3 support surface.
    Full {
        /// Exact BS3 support and fit tolerance, when serialized.
        #[serde(flatten, with = "g2_blend_full_support_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "G2BlendFullSupportSchemaWire"))]
        support: Option<G2BlendFullSupport>,
    },
    /// Non-singular nine-scalar frame and tertiary pcurve.
    None {
        /// Ordered native frame scalars.
        coefficients: [f64; 9],
        /// Native fit tolerance.
        tolerance: FitTolerance,
        /// Optional intervening native token.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extension: Option<LoftBridgeToken>,
        /// Tertiary BS2 pcurve, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
    },
}

/// Exact support surface and fit tolerance of a full G2 first-side shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct G2BlendFullSupport {
    /// Exact BS3 support surface.
    pub surface: SurfaceId,
    /// Fit tolerance of the support, in document length units.
    pub tolerance: FitTolerance,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the G2 full-support wire schema")]
struct G2BlendFullSupportSchemaWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surface: Option<SurfaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tolerance: Option<FitTolerance>,
}

mod g2_blend_full_support_wire {
    use super::{FitTolerance, G2BlendFullSupport, SurfaceId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<SurfaceId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tolerance: Option<FitTolerance>,
    }

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(
        value: &Option<G2BlendFullSupport>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            Some(value) => Wire {
                surface: Some(value.surface.clone()),
                tolerance: Some(value.tolerance),
            },
            None => Wire {
                surface: None,
                tolerance: None,
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<G2BlendFullSupport>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.surface, wire.tolerance) {
            (Some(surface), Some(tolerance)) => Ok(Some(G2BlendFullSupport { surface, tolerance })),
            (None, None) => Ok(None),
            _ => Err(serde::de::Error::custom(
                "G2 full surface and tolerance must occur together",
            )),
        }
    }
}

/// Full native G2 blend construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct G2BlendConstruction {
    /// First side common fields.
    pub first: G2BlendSide,
    /// Native first-side singularity enum.
    pub singularity: i64,
    /// First-side singularity payload.
    pub first_shape: G2BlendFirstShape,
    /// Second side common fields.
    pub second: G2BlendSide,
    /// Exact second-side spline support.
    pub second_exact_surface: SurfaceId,
    /// Center or transition curve.
    pub center_curve: CurveId,
    /// Ordered center-curve scalars.
    pub center_parameters: [f64; 2],
    /// Native center tail integer.
    pub center_flag: i64,
    /// Native U and V intervals.
    pub parameter_ranges: [[f64; 2]; 2],
    /// Four ordered trailing scalars.
    pub trailing_parameters: [f64; 4],
    /// Three ordered ASM discontinuity arrays.
    pub discontinuities: [Vec<f64>; 3],
}

/// A present rolling-ball support surface and its native UV bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallSupportSurface<S = SurfaceId> {
    /// Support surface or embedded geometry.
    pub surface: S,
    /// Optional native U and V endpoints.
    pub parameter_ranges: [[Option<f64>; 2]; 2],
}

/// A present rolling-ball side curve and its native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallSupportCurve<C = CurveId> {
    /// Side curve or embedded geometry.
    pub curve: C,
    /// Optional native parameter endpoints.
    pub parameter_range: [Option<f64>; 2],
}

/// The optional rolling-ball extension clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallSideExtension<P = PcurveGeometry> {
    /// Native integer introducing the clause.
    pub value: i64,
    /// Tertiary BS2 pcurve, absent for `nullbs`.
    pub pcurve: Option<P>,
}

/// One complete native rolling-ball support side.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "RollingBallSideWire<S, C, P>"))]
pub struct RollingBallSide<S = SurfaceId, C = CurveId, P = PcurveGeometry> {
    /// Geometry role selected by the support-side discriminator.
    pub support_kind: VariableBlendSupportKind,
    /// Primary support surface and bounds, absent for `null_surface`.
    pub surface: Option<RollingBallSupportSurface<S>>,
    /// Side curve and bounds, absent for `null_curve`.
    pub curve: Option<RollingBallSupportCurve<C>>,
    /// Primary BS2 pcurve, absent for `nullbs`.
    pub pcurve: Option<P>,
    /// Native model-space side location.
    pub location: Point3,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    pub secondary_pcurve: Option<P>,
    /// Native extension integer and nullable tertiary pcurve.
    pub extension: Option<RollingBallSideExtension<P>>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(bound(deserialize = "S: Deserialize<'de>, C: Deserialize<'de>, P: Deserialize<'de>"))]
struct RollingBallSideWire<S, C, P> {
    support_kind: VariableBlendSupportKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surface: Option<S>,
    #[serde(default)]
    surface_ranges: [[Option<f64>; 2]; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<C>,
    #[serde(default)]
    curve_range: [Option<f64>; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pcurve: Option<P>,
    location: Point3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_pcurve: Option<P>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extension: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tertiary_pcurve: Option<P>,
}

impl<S: Serialize, C: Serialize, P: Serialize> Serialize for RollingBallSide<S, C, P> {
    fn serialize<W: serde::Serializer>(&self, serializer: W) -> Result<W::Ok, W::Error> {
        RollingBallSideWire {
            support_kind: self.support_kind,
            surface: self.surface.as_ref().map(|support| &support.surface),
            surface_ranges: self
                .surface
                .as_ref()
                .map_or([[None; 2]; 2], |support| support.parameter_ranges),
            curve: self.curve.as_ref().map(|support| &support.curve),
            curve_range: self
                .curve
                .as_ref()
                .map_or([None; 2], |support| support.parameter_range),
            pcurve: self.pcurve.as_ref(),
            location: self.location,
            secondary_pcurve: self.secondary_pcurve.as_ref(),
            extension: self.extension.as_ref().map(|extension| extension.value),
            tertiary_pcurve: self
                .extension
                .as_ref()
                .and_then(|extension| extension.pcurve.as_ref()),
        }
        .serialize(serializer)
    }
}

impl<'de, S: Deserialize<'de>, C: Deserialize<'de>, P: Deserialize<'de>> Deserialize<'de>
    for RollingBallSide<S, C, P>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RollingBallSideWire::<S, C, P>::deserialize(deserializer)?;
        let surface = match wire.surface {
            Some(surface) => Some(RollingBallSupportSurface {
                surface,
                parameter_ranges: wire.surface_ranges,
            }),
            None if wire.surface_ranges.iter().flatten().any(Option::is_some) => {
                return Err(serde::de::Error::custom(
                    "rolling-ball surface_ranges require surface",
                ))
            }
            None => None,
        };
        let curve = match wire.curve {
            Some(curve) => Some(RollingBallSupportCurve {
                curve,
                parameter_range: wire.curve_range,
            }),
            None if wire.curve_range.iter().any(Option::is_some) => {
                return Err(serde::de::Error::custom(
                    "rolling-ball curve_range requires curve",
                ))
            }
            None => None,
        };
        let extension = match (wire.extension, wire.tertiary_pcurve) {
            (Some(value), pcurve) => Some(RollingBallSideExtension { value, pcurve }),
            (None, Some(_)) => {
                return Err(serde::de::Error::custom(
                    "rolling-ball tertiary_pcurve requires extension",
                ))
            }
            (None, None) => None,
        };
        Ok(Self {
            support_kind: wire.support_kind,
            surface,
            curve,
            pcurve: wire.pcurve,
            location: wire.location,
            secondary_pcurve: wire.secondary_pcurve,
            extension,
        })
    }
}

/// Third support graph appended by `sss_blend_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallThirdSide {
    /// Native side label.
    pub label: String,
    /// Third support surface.
    pub surface: SurfaceId,
    /// Third side curve.
    pub curve: CurveId,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<PcurveGeometry>,
    /// Native side vector.
    pub direction: Vector3,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_pcurve: Option<PcurveGeometry>,
    /// Native ASM integer following the secondary pcurve.
    pub extension: i64,
    /// ASM tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tertiary_pcurve: Option<PcurveGeometry>,
    /// Final ASM flag.
    pub flag: bool,
}

/// Native optional-radius selector in a rolling-ball construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RollingBallRadiusSelector<T = f64> {
    /// Native `-1` no-radius sentinel.
    None,
    /// Explicit native selector scalar.
    Value {
        /// Stored scalar value.
        value: T,
    },
}

/// Integer radius-selector value in a revision G2 blend.
///
/// The native `-1` value denotes the absence variant and is not a value of
/// this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(transparent))]
pub struct RevisionG2RadiusValue(i64);

impl RevisionG2RadiusValue {
    /// Construct an explicit selector value.
    #[must_use]
    pub const fn new(value: i64) -> Option<Self> {
        if value == -1 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Return the native integer value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl Serialize for RevisionG2RadiusValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RevisionG2RadiusValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| {
            serde::de::Error::custom("revision G2 radius selector value cannot be -1")
        })
    }
}

mod revision_g2_radius_selector_wire {
    use super::{RevisionG2RadiusValue, RollingBallRadiusSelector};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        selector: &RollingBallRadiusSelector<RevisionG2RadiusValue>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match selector {
            RollingBallRadiusSelector::None => (-1_i64).serialize(serializer),
            RollingBallRadiusSelector::Value { value } => value.get().serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<RollingBallRadiusSelector<RevisionG2RadiusValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match i64::deserialize(deserializer)? {
            -1 => RollingBallRadiusSelector::None,
            value => RollingBallRadiusSelector::Value {
                value: RevisionG2RadiusValue(value),
            },
        })
    }
}

/// Complete byte-backed rolling-ball or three-surface blend context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallConstruction {
    /// Native subtype definition-table index.
    pub definition_index: i64,
    /// Two ordered primary support sides.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored slice or center curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<f64>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [f64; 2],
    /// Optional-radius selector field.
    pub radius_selector: RollingBallRadiusSelector,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer preceding the trailing scalars.
    pub shape_prefix: i64,
    /// Two ordered trailing scalars.
    pub parameters: [f64; 2],
    /// Native long following the trailing scalars.
    pub tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "revision_surface_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "RevisionSurfaceCacheSchemaWire"))]
    pub cache: RevisionCacheForm,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// Native Boolean closing the shared tail.
    #[serde(default)]
    pub tail_flag: bool,
    /// Third side present only for `sss_blend_spl_sur`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub third: Option<Box<RollingBallThirdSide>>,
    /// Three ASM integers preceding the subtype close.
    #[serde(default)]
    pub tail_extensions: [i64; 3],
}

/// Geometry role selected by a variable-blend support-side discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendSupportKind {
    /// Support defined by a cosine curve.
    CosineCurve,
    /// Support defined by a general curve.
    Curve,
    /// Support defined by a point curve.
    PointCurve,
    /// Support defined by a surface.
    Surface,
    /// Support defined by a zero curve.
    ZeroCurve,
}

/// Convexity selected for a variable-radius blend surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendConvexity {
    /// The blend bends toward the support intersection.
    Convex,
    /// The blend bends away from the support intersection.
    Concave,
}

/// Solved-surface representation selected for a variable-radius blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendRenderMode {
    /// The solved surface is the rolling-ball envelope.
    RollingBallEnvelope,
    /// The solved surface is a rolling-ball snapshot.
    RollingBallSnapshot,
}

/// One interpolation control point in a variable blend-value law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VariableBlendInterpolationPoint {
    /// Law parameter.
    pub parameter: f64,
    /// Radius in document length units.
    pub radius: f64,
    /// Optional first and second derivative scalars.
    #[serde(with = "variable_blend_tangents_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "[Option<f64>; 2]"))]
    pub tangents: [Option<f64>; 2],
    /// Model-space control location.
    pub location: Point3,
    /// Control normal.
    pub normal: Vector3,
}

mod variable_blend_tangents_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    const LEGACY_UNSET_TANGENT: f64 = 1.0e37;

    pub fn serialize<S>(tangents: &[Option<f64>; 2], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        tangents.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[Option<f64>; 2], D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(
            <[Option<f64>; 2]>::deserialize(deserializer)?.map(|value| match value {
                Some(LEGACY_UNSET_TANGENT) => None,
                value => value,
            }),
        )
    }
}

/// Native edge-offset blend-value sub-discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum EdgeOffsetDiscriminator {
    /// Explicit zero sub-discriminator.
    Zero,
    /// One sub-discriminator, including the elided default.
    One,
}

impl EdgeOffsetDiscriminator {
    /// Admit the two native edge-offset codes.
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            _ => None,
        }
    }

    /// Native integer code.
    pub const fn code(self) -> i64 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }
}

/// Numeric or symbolic terminal of a functional blend-value law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum VariableBlendTerminal {
    /// Native double token.
    Double(f64),
    /// Native string token.
    Text(String),
}

/// Complete recursive native `getBlendValues` payload.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "VariableBlendValueWire"))]
pub struct VariableBlendValue {
    /// Native Boolean following the calibrated enum.
    pub modern_flag: bool,
    /// Native calibrated enum.
    pub calibrated: i64,
    /// Type-specific payload with its native sub-discriminator.
    pub payload: VariableBlendValuePayload,
}

/// Type-specific payload of a variable blend value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum VariableBlendValuePayload {
    /// Law-domain parameter range and two endpoint radii.
    TwoEnds {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Law-domain parameter range (lower, upper).
        parameters: [f64; 2],
        /// Endpoint radii in document length units.
        radii: [f64; 2],
    },
    /// Fixed-width branch: the parameter-range bounds and the chamfer width
    /// scalar, stored unscaled.
    FixedWidth {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Parameter-range lower and upper bounds.
        parameters: [f64; 2],
        /// Chamfer width.
        width: f64,
    },
    /// Edge-offset branch.
    EdgeOffset {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: EdgeOffsetDiscriminator,
        /// Ordered native scalar payload.
        scalars: [f64; 2],
        /// Ordered length payload in document units.
        lengths: [f64; 1],
    },
    /// Functional radius law carried by a BS2 pcurve.
    Functional {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: f64,
        /// Leading length in document units.
        radius: f64,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Numeric or symbolic terminal value.
        terminal: VariableBlendTerminal,
    },
    /// Constant law followed by a recursive chamfer value.
    Constant {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Ordered native scalars.
        parameters: [f64; 2],
        /// Radius in document length units.
        radius: f64,
        /// Native variable-chamfer enum.
        variable_chamfer: i64,
        /// Native chamfer-type enum.
        chamfer_type: i64,
        /// Recursively nested blend value.
        nested: Box<VariableBlendValue>,
    },
    /// Interpolated radius law.
    Interpolated {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: f64,
        /// Leading radius in document length units.
        radius: f64,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Native extension enum, stored ahead of the radius-point count. It
        /// gates nothing; the payload ends at the last radius point.
        enum_count: i64,
        /// Whether the extension enum is stored as a `0x15` enum token
        /// (revision-gated streams) rather than a `0x04` integer.
        enum_tagged: bool,
        /// Counted radius-point array: each control carries a parameter,
        /// radius, two derivative scalars, a position, and a vector.
        points: Vec<VariableBlendInterpolationPoint>,
    },
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum VariableBlendPayloadWire<F, T, N, P> {
    TwoEnds {
        parameters: [f64; 2],
        radii: [f64; 2],
    },
    FixedWidth {
        parameters: [f64; 2],
        width: f64,
    },
    EdgeOffset {
        scalars: [f64; 2],
        lengths: [f64; 1],
    },
    Functional {
        parameter: f64,
        radius: f64,
        function: F,
        terminal: T,
    },
    Constant {
        parameters: [f64; 2],
        radius: f64,
        variable_chamfer: i64,
        chamfer_type: i64,
        nested: N,
    },
    Interpolated {
        parameter: f64,
        radius: f64,
        function: F,
        enum_count: i64,
        #[serde(default)]
        enum_tagged: bool,
        points: P,
    },
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct VariableBlendValueWire {
    name: String,
    modern_flag: bool,
    discriminator: i64,
    calibrated: i64,
    payload: VariableBlendPayloadWire<
        PcurveGeometry,
        VariableBlendTerminal,
        Box<VariableBlendValue>,
        Vec<VariableBlendInterpolationPoint>,
    >,
}

impl Serialize for VariableBlendValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let payload = match &self.payload {
            VariableBlendValuePayload::TwoEnds {
                parameters, radii, ..
            } => VariableBlendPayloadWire::TwoEnds {
                parameters: *parameters,
                radii: *radii,
            },
            VariableBlendValuePayload::FixedWidth {
                parameters, width, ..
            } => VariableBlendPayloadWire::FixedWidth {
                parameters: *parameters,
                width: *width,
            },
            VariableBlendValuePayload::EdgeOffset {
                scalars, lengths, ..
            } => VariableBlendPayloadWire::EdgeOffset {
                scalars: *scalars,
                lengths: *lengths,
            },
            VariableBlendValuePayload::Functional {
                parameter,
                radius,
                function,
                terminal,
                ..
            } => VariableBlendPayloadWire::Functional {
                parameter: *parameter,
                radius: *radius,
                function,
                terminal,
            },
            VariableBlendValuePayload::Constant {
                parameters,
                radius,
                variable_chamfer,
                chamfer_type,
                nested,
                ..
            } => VariableBlendPayloadWire::Constant {
                parameters: *parameters,
                radius: *radius,
                variable_chamfer: *variable_chamfer,
                chamfer_type: *chamfer_type,
                nested: nested.as_ref(),
            },
            VariableBlendValuePayload::Interpolated {
                parameter,
                radius,
                function,
                enum_count,
                enum_tagged,
                points,
                ..
            } => VariableBlendPayloadWire::Interpolated {
                parameter: *parameter,
                radius: *radius,
                function,
                enum_count: *enum_count,
                enum_tagged: *enum_tagged,
                points: points.as_slice(),
            },
        };
        let mut wire = serializer.serialize_struct("VariableBlendValue", 5)?;
        wire.serialize_field("name", self.payload.native_name())?;
        wire.serialize_field("modern_flag", &self.modern_flag)?;
        wire.serialize_field("discriminator", &self.payload.discriminator())?;
        wire.serialize_field("calibrated", &self.calibrated)?;
        wire.serialize_field("payload", &payload)?;
        wire.end()
    }
}

impl<'de> Deserialize<'de> for VariableBlendValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = VariableBlendValueWire::deserialize(deserializer)?;
        let payload = match wire.payload {
            VariableBlendPayloadWire::TwoEnds { parameters, radii } => {
                VariableBlendValuePayload::TwoEnds {
                    discriminator: wire.discriminator,
                    parameters,
                    radii,
                }
            }
            VariableBlendPayloadWire::FixedWidth { parameters, width } => {
                VariableBlendValuePayload::FixedWidth {
                    discriminator: wire.discriminator,
                    parameters,
                    width,
                }
            }
            VariableBlendPayloadWire::EdgeOffset { scalars, lengths } => {
                VariableBlendValuePayload::EdgeOffset {
                    discriminator: EdgeOffsetDiscriminator::from_code(wire.discriminator)
                        .ok_or_else(|| {
                            serde::de::Error::custom("edge-offset discriminator must be 0 or 1")
                        })?,
                    scalars,
                    lengths,
                }
            }
            VariableBlendPayloadWire::Functional {
                parameter,
                radius,
                function,
                terminal,
            } => VariableBlendValuePayload::Functional {
                discriminator: wire.discriminator,
                parameter,
                radius,
                function,
                terminal,
            },
            VariableBlendPayloadWire::Constant {
                parameters,
                radius,
                variable_chamfer,
                chamfer_type,
                nested,
            } => VariableBlendValuePayload::Constant {
                discriminator: wire.discriminator,
                parameters,
                radius,
                variable_chamfer,
                chamfer_type,
                nested,
            },
            VariableBlendPayloadWire::Interpolated {
                parameter,
                radius,
                function,
                enum_count,
                enum_tagged,
                points,
            } => VariableBlendValuePayload::Interpolated {
                discriminator: wire.discriminator,
                parameter,
                radius,
                function,
                enum_count,
                enum_tagged,
                points,
            },
        };
        if wire.name != payload.native_name() {
            return Err(serde::de::Error::custom(
                "variable-blend name must match payload",
            ));
        }
        Ok(Self {
            modern_flag: wire.modern_flag,
            calibrated: wire.calibrated,
            payload,
        })
    }
}

impl VariableBlendValuePayload {
    /// Native sub-discriminator preceding the calibrated enum.
    pub const fn discriminator(&self) -> i64 {
        match self {
            Self::EdgeOffset { discriminator, .. } => discriminator.code(),
            Self::TwoEnds { discriminator, .. }
            | Self::FixedWidth { discriminator, .. }
            | Self::Functional { discriminator, .. }
            | Self::Constant { discriminator, .. }
            | Self::Interpolated { discriminator, .. } => *discriminator,
        }
    }

    /// Native type name introducing this payload.
    pub const fn native_name(&self) -> &'static str {
        match self {
            Self::TwoEnds { .. } => "two_ends",
            Self::FixedWidth { .. } => "fixed_width",
            Self::EdgeOffset { .. } => "edge_offset",
            Self::Functional { .. } => "functional",
            Self::Constant { .. } => "const",
            Self::Interpolated { .. } => "interp",
        }
    }
}

/// Radius-law payloads of a variable blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VariableBlendRadii {
    /// One radius law controls both support sides.
    Single {
        /// Shared radius law.
        value: VariableBlendValue,
    },
    /// Each support side has an independent radius law.
    Two {
        /// First support-side radius law.
        first: VariableBlendValue,
        /// Second support-side radius law.
        second: VariableBlendValue,
    },
}

impl VariableBlendRadii {
    /// First radius law in native order.
    #[must_use]
    pub const fn first(&self) -> &VariableBlendValue {
        match self {
            Self::Single { value } | Self::Two { first: value, .. } => value,
        }
    }

    /// Second radius law when both sides are controlled independently.
    #[must_use]
    pub const fn second(&self) -> Option<&VariableBlendValue> {
        match self {
            Self::Single { .. } => None,
            Self::Two { second, .. } => Some(second),
        }
    }

    /// Whether one radius law controls both sides.
    #[must_use]
    pub const fn is_single(&self) -> bool {
        matches!(self, Self::Single { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum VariableBlendRadiusKindWire {
    SingleRadius,
    TwoRadii,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(
    dead_code,
    reason = "fields define the variable-blend radii wire schema"
)]
struct VariableBlendRadiiSchemaWire {
    radius_kind: VariableBlendRadiusKindWire,
    first_value: VariableBlendValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_value: Option<VariableBlendValue>,
}

mod variable_blend_radii_wire {
    use super::{VariableBlendRadii, VariableBlendRadiusKindWire, VariableBlendValue};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        radius_kind: VariableBlendRadiusKindWire,
        first_value: VariableBlendValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second_value: Option<VariableBlendValue>,
    }

    pub fn serialize<S>(value: &VariableBlendRadii, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            VariableBlendRadii::Single { value } => Wire {
                radius_kind: VariableBlendRadiusKindWire::SingleRadius,
                first_value: value.clone(),
                second_value: None,
            },
            VariableBlendRadii::Two { first, second } => Wire {
                radius_kind: VariableBlendRadiusKindWire::TwoRadii,
                first_value: first.clone(),
                second_value: Some(second.clone()),
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VariableBlendRadii, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.radius_kind, wire.second_value) {
            (VariableBlendRadiusKindWire::SingleRadius, None) => Ok(VariableBlendRadii::Single {
                value: wire.first_value,
            }),
            (VariableBlendRadiusKindWire::TwoRadii, Some(second)) => Ok(VariableBlendRadii::Two {
                first: wire.first_value,
                second,
            }),
            _ => Err(serde::de::Error::custom(
                "variable-blend radius kind conflicts with its payload count",
            )),
        }
    }
}

mod variable_blend_u_range_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &[f64; 2], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        [Some(value[0]), Some(value[1])].serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[f64; 2], D::Error>
    where
        D: Deserializer<'de>,
    {
        match <[Option<f64>; 2]>::deserialize(deserializer)? {
            [Some(lower), Some(upper)] => Ok([lower, upper]),
            _ => Err(serde::de::Error::custom(
                "variable-blend u_range requires both bounds",
            )),
        }
    }
}

mod variable_blend_v_range_wire {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        [*value, None].serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match <[Option<f64>; 2]>::deserialize(deserializer)? {
            [lower, None] => Ok(lower),
            _ => Err(serde::de::Error::custom(
                "variable-blend v_range requires an absent upper bound",
            )),
        }
    }
}

/// Cross-section clause following the variable-radius laws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VariableBlendCrossSection {
    /// Circular section with no additional parameters.
    Circular,
    /// Thumbweight-controlled section with two ordered shape parameters.
    Thumbweights {
        /// Ordered native shape parameters.
        parameters: [f64; 2],
    },
    /// Rounded chamfer with an optional independent rounding-radius law.
    RoundedChamfer {
        /// Rounding-radius law; absent when the clause stores `no_radius`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<Box<VariableBlendValue>>,
    },
    /// Curvature-continuous round with two ordered shape parameters.
    G2Round {
        /// Ordered native shape parameters.
        parameters: [f64; 2],
    },
    /// A zero-width native selector whose record framing is known but whose
    /// geometric cross-section law is not classified.
    UnclassifiedBare {
        /// Exact numeric selector retained from the native record.
        selector: VariableBlendBareCrossSection,
    },
}

/// Native zero-width variable-blend cross-section selectors whose framing is
/// established while their geometric laws remain unclassified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[repr(i64)]
pub enum VariableBlendBareCrossSection {
    /// Native selector `2`.
    Selector2 = 2,
    /// Native selector `4`.
    Selector4 = 4,
    /// Native selector `5`.
    Selector5 = 5,
    /// Native selector `6`.
    Selector6 = 6,
}

impl VariableBlendBareCrossSection {
    /// Numeric selector stored in the native variable-blend record.
    pub const fn native_selector(self) -> i64 {
        self as i64
    }
}

impl TryFrom<i64> for VariableBlendBareCrossSection {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            2 => Ok(Self::Selector2),
            4 => Ok(Self::Selector4),
            5 => Ok(Self::Selector5),
            6 => Ok(Self::Selector6),
            _ => Err(()),
        }
    }
}

/// Native variable-radius blend surface subtype.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VariableBlendSurfaceSubtype {
    /// General variable-blend surface.
    #[default]
    VariableBlend,
    /// Surface-to-surface variable blend.
    SurfaceSurface,
    /// Curve-to-curve variable blend.
    CurveCurve,
    /// Curve-to-surface variable blend.
    CurveSurface,
    /// Free surface-curve variable blend.
    SurfaceCurveFree,
}

mod variable_blend_secondary_curve_wire {
    use super::{CurveId, RollingBallSupportCurve};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(bound(deserialize = "C: Deserialize<'de>"))]
    pub(super) struct Wire<C> {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secondary_curve: Option<C>,
        #[serde(default)]
        secondary_range: [Option<f64>; 2],
    }

    // Serde's field adapter passes a reference to the complete optional field.
    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(
        curve: &Option<RollingBallSupportCurve>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            secondary_curve: curve.as_ref().map(|curve| &curve.curve),
            secondary_range: curve
                .as_ref()
                .map_or([None; 2], |curve| curve.parameter_range),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<RollingBallSupportCurve>, D::Error> {
        let wire = Wire::<CurveId>::deserialize(deserializer)?;
        match wire.secondary_curve {
            Some(curve) => Ok(Some(RollingBallSupportCurve {
                curve,
                parameter_range: wire.secondary_range,
            })),
            None if wire.secondary_range.iter().any(Option::is_some) => Err(
                serde::de::Error::custom("variable-blend secondary_range requires secondary_curve"),
            ),
            None => Ok(None),
        }
    }
}

/// Complete native variable-radius blend construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VariableBlendConstruction {
    /// Native surface subtype selecting the variable-blend behavior class.
    #[serde(default)]
    pub subtype: VariableBlendSurfaceSubtype,
    /// Native serializer-revision integer following the subtype name.
    #[serde(alias = "definition_index")]
    pub revision: i64,
    /// Two ordered support-side graphs in the rolling-ball side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored slice curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    #[serde(default)]
    pub slice_range: [Option<f64>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [f64; 2],
    /// Structurally selected radius-control payloads.
    #[serde(flatten, with = "variable_blend_radii_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "VariableBlendRadiiSchemaWire"))]
    pub radii: VariableBlendRadii,
    /// Cross-section clause following the complete radius-law sequence.
    /// Absence denotes an elided default circular section; an explicit
    /// circular clause remains distinct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_section: Option<VariableBlendCrossSection>,
    /// Support-side parameter interval `(T0, T1)`; both bounds present in
    /// every instance.
    #[serde(with = "variable_blend_u_range_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "[Option<f64>; 2]"))]
    pub u_range: [f64; 2],
    /// Second interval: a lower bound with an unbounded-above marker,
    /// encoded as `(T lo, F)` and decoding to `[Some(lo), None]`. The `F`
    /// upper-bound marker is an interval bound, not a standalone Boolean.
    #[serde(rename = "v_range", with = "variable_blend_v_range_wire")]
    #[cfg_attr(
        feature = "schema",
        schemars(rename = "v_range", with = "[Option<f64>; 2]")
    )]
    pub v_lower: Option<f64>,
    /// Requested fit tolerance for the surface cache.
    pub shape_parameter: f64,
    /// Achieved fit tolerance for the surface cache, at or below
    /// `shape_parameter`, in document units.
    pub shape_length: f64,
    /// Non-negative integer immediately before the shared tail's enum.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "variable_blend_cache_wire")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "variable_blend_cache_wire::WriteWire<'_>")
    )]
    pub cache: VariableBlendCache,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<f64>; 6],
    /// Native Boolean following the discontinuity arrays.
    pub tail_flag: bool,
    /// Three ASM integers following the tail Boolean.
    pub tail_extensions: [i64; 3],
    /// Secondary curve and its bounds, absent for `null_curve`.
    #[serde(flatten, with = "variable_blend_secondary_curve_wire")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "variable_blend_secondary_curve_wire::Wire<CurveId>")
    )]
    pub secondary_curve: Option<RollingBallSupportCurve>,
    /// Blend convexity.
    pub convexity: VariableBlendConvexity,
    /// Solved-surface representation.
    pub render_mode: VariableBlendRenderMode,
    /// Native optional post-shape interval endpoints.
    pub post_range: [Option<f64>; 2],
    /// Native post-shape BS3 curve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_curve: Option<CurveId>,
    /// Native post-shape BS2 pcurve, absent for `nullbs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_pcurve: Option<PcurveGeometry>,
}

/// Complete native revision-gated `g2_blend_spl_sur` construction. The
/// revision layout stores the two support sides in the variable-blend side
/// layout and ends with the shared revision-gated surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RevisionG2BlendConstruction {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Two native scalars following the revision integer.
    pub leading_parameters: [f64; 2],
    /// Two ordered support-side graphs in the variable-blend side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored center curve.
    pub center: CurveId,
    /// Optional native center-curve parameter endpoints.
    #[serde(default)]
    pub center_range: [Option<f64>; 2],
    /// Two signed blend radii in document length units.
    pub radii: [f64; 2],
    /// Integer-valued optional-radius selector following the radii.
    #[serde(with = "revision_g2_radius_selector_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "i64"))]
    pub radius_selector: RollingBallRadiusSelector<RevisionG2RadiusValue>,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer before the solved shape.
    pub shape_prefix: i64,
    /// Native scalar before the solved shape.
    pub shape_parameter: f64,
    /// Native length before the solved shape, in document units.
    pub shape_length: f64,
    /// Native integer immediately before the shared tail.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "revision_surface_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "RevisionSurfaceCacheSchemaWire"))]
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Three ASM integers following the shared tail.
    pub tail_extensions: [i64; 3],
}

/// Complete native revision-gated `cl_loft_spl_sur` construction. The
/// revision layout is cache-first: the revision integer and shared
/// revision-gated surface tail precede the construction fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RevisionCompoundLoftConstruction {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "revision_surface_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "RevisionSurfaceCacheSchemaWire"))]
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    #[serde(default)]
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Leading unparameterized scale block: ordered profile members and path.
    pub base_profile: Vec<LoftProfileMember>,
    /// Path data of the leading scale block.
    pub base_path: LoftPath,
    /// Counted parameterized entries; the native parameter trails each
    /// entry's fields.
    pub entries: Vec<LoftSectionEntry>,
    /// Two flags following the entries.
    pub flags: [bool; 2],
    /// Two flags opening the kind-zero payload.
    pub kind_flags: [bool; 2],
    /// Direction carrier selected by the kind-zero direction tag.
    #[serde(flatten, with = "revision_compound_loft_direction_wire")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "RevisionCompoundLoftDirectionSchemaWire")
    )]
    pub direction: CompoundLoftDirection,
    /// Trailing bounds and their dependent BS3 curve.
    #[serde(flatten, with = "revision_compound_loft_tail_wire")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "revision_compound_loft_tail_wire::WriteWire<'_, CurveId>")
    )]
    pub tail: RevisionCompoundLoftTail<CurveId>,
}

/// Trailing parameter bounds of a revision compound loft.
/// Both bounds select a trailing curve; all other forms contain no curve.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum RevisionCompoundLoftTail<T> {
    /// Neither parameter bound is present.
    Unbounded,
    /// Only the lower parameter bound is present.
    LowerBound(f64),
    /// Only the upper parameter bound is present.
    UpperBound(f64),
    /// Both parameter bounds and their selected curve.
    Curve {
        /// Ordered lower and upper parameter bounds.
        interval: [f64; 2],
        /// Curve selected by the complete parameter pair.
        curve: T,
    },
}

impl<T> RevisionCompoundLoftTail<T> {
    /// Optional bounds in native order.
    #[must_use]
    pub const fn interval(&self) -> [Option<f64>; 2] {
        match self {
            Self::Unbounded => [None, None],
            Self::LowerBound(value) => [Some(*value), None],
            Self::UpperBound(value) => [None, Some(*value)],
            Self::Curve { interval, .. } => [Some(interval[0]), Some(interval[1])],
        }
    }

    /// Curve selected by a complete parameter pair.
    #[must_use]
    pub const fn curve(&self) -> Option<&T> {
        match self {
            Self::Curve { curve, .. } => Some(curve),
            _ => None,
        }
    }

    /// Transform the curve payload while retaining its parameter bounds.
    #[must_use]
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> RevisionCompoundLoftTail<U> {
        match self {
            Self::Unbounded => RevisionCompoundLoftTail::Unbounded,
            Self::LowerBound(value) => RevisionCompoundLoftTail::LowerBound(value),
            Self::UpperBound(value) => RevisionCompoundLoftTail::UpperBound(value),
            Self::Curve { interval, curve } => RevisionCompoundLoftTail::Curve {
                interval,
                curve: map(curve),
            },
        }
    }
}

mod revision_compound_loft_tail_wire {
    use super::RevisionCompoundLoftTail;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    pub(super) struct WriteWire<'a, T> {
        interval: [Option<f64>; 2],
        #[serde(skip_serializing_if = "Option::is_none")]
        trailing_curve: Option<&'a T>,
    }

    #[derive(Deserialize)]
    #[serde(bound(deserialize = "T: Deserialize<'de>"))]
    struct ReadWire<T> {
        #[serde(default)]
        interval: [Option<f64>; 2],
        #[serde(default)]
        trailing_curve: Option<T>,
    }

    pub fn serialize<T: Serialize, S: Serializer>(
        value: &RevisionCompoundLoftTail<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        WriteWire {
            interval: value.interval(),
            trailing_curve: value.curve(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<RevisionCompoundLoftTail<T>, D::Error> {
        let wire = ReadWire::deserialize(deserializer)?;
        match (wire.interval, wire.trailing_curve) {
            ([None, None], None) => Ok(RevisionCompoundLoftTail::Unbounded),
            ([Some(value), None], None) => Ok(RevisionCompoundLoftTail::LowerBound(value)),
            ([None, Some(value)], None) => Ok(RevisionCompoundLoftTail::UpperBound(value)),
            ([Some(lower), Some(upper)], Some(curve)) => Ok(RevisionCompoundLoftTail::Curve {
                interval: [lower, upper],
                curve,
            }),
            _ => Err(serde::de::Error::custom(
                "revision compound loft pairs its trailing curve with both parameter values",
            )),
        }
    }
}

/// One boundary record in a native vertex-blend patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VertexBlendBoundary {
    /// Native cross flag. The wire form is a logical, so the value is the
    /// tag itself and no payload follows.
    pub boundary_type: bool,
    /// Native magic direction. A unit direction or the zero vector, never a
    /// length, so it carries no unit scale.
    pub magic: Vector3,
    /// Native U-smoothing flag, a logical on the wire.
    pub u_smoothing: bool,
    /// Native V-smoothing flag, a logical on the wire.
    pub v_smoothing: bool,
    /// Native fullness scalar.
    pub fullness: f64,
    /// Structurally selected boundary geometry.
    pub geometry: VertexBlendBoundaryGeometry,
}

/// Twist payload selected by a vertex-blend circle form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "VertexBlendTwistsWire", into = "VertexBlendTwistsWire")]
pub enum VertexBlendTwists {
    /// Form zero has no twist entries.
    None,
    /// Form one has one twist entry.
    One(Point3),
    /// Form three has two ordered twist entries.
    Two([Point3; 2]),
}

impl VertexBlendTwists {
    /// Native form selected by the twist payload.
    #[must_use]
    pub const fn form(&self) -> i64 {
        match self {
            Self::None => 0,
            Self::One(_) => 1,
            Self::Two(_) => 3,
        }
    }

    /// Ordered twist entries. Their coordinate semantics depend on the layout revision.
    #[must_use]
    pub fn entries(&self) -> &[Point3] {
        match self {
            Self::None => &[],
            Self::One(point) => std::slice::from_ref(point),
            Self::Two(points) => points,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct VertexBlendTwistsWire {
    form: i64,
    twists: Vec<Point3>,
}

impl TryFrom<VertexBlendTwistsWire> for VertexBlendTwists {
    type Error = &'static str;
    fn try_from(wire: VertexBlendTwistsWire) -> Result<Self, Self::Error> {
        match (wire.form, wire.twists.as_slice()) {
            (0, []) => Ok(Self::None),
            (1, [point]) => Ok(Self::One(*point)),
            (3, [first, second]) => Ok(Self::Two([*first, *second])),
            _ => Err("vertex-blend circle forms 0, 1, and 3 require zero, one, and two twists"),
        }
    }
}

impl From<VertexBlendTwists> for VertexBlendTwistsWire {
    fn from(value: VertexBlendTwists) -> Self {
        Self {
            form: value.form(),
            twists: value.entries().to_vec(),
        }
    }
}

/// Type-specific geometry of a vertex-blend boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VertexBlendBoundaryGeometry {
    /// Curve boundary with a circle/ellipse/unknown twist form.
    Circle {
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<f64>; 2],
        /// Twist payload. Pre-revision layouts store model-space locations;
        /// the revision-gated layout stores unscaled twist vectors.
        #[serde(flatten)]
        twists: VertexBlendTwists,
        /// Two ordered curve parameters.
        parameters: [f64; 2],
        /// Native sense flag, a logical on the wire.
        sense: bool,
    },
    /// Degenerate boundary at a model-space location.
    Degenerate {
        /// Degenerate location.
        location: Point3,
        /// Two ordered boundary normals.
        normals: [Vector3; 2],
    },
    /// Surface pcurve boundary.
    Pcurve {
        /// Support surface.
        surface: SurfaceId,
        /// Optional U/V bound fields stored after the support by the
        /// revision-gated layout.
        #[serde(default)]
        support_bounds: [Option<f64>; 4],
        /// Native BS2 pcurve, absent for `nullbs`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pcurve: Option<PcurveGeometry>,
        /// Native sense flag, a logical on the wire.
        sense: bool,
        /// Parameter-space fit tolerance.
        fit_tolerance: FitTolerance,
    },
    /// Planar boundary described by a normal and curve.
    Plane {
        /// Plane normal.
        normal: Vector3,
        /// Two ordered plane parameters.
        parameters: [f64; 2],
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        #[serde(default)]
        curve_endpoints: [Option<f64>; 2],
    },
}

/// Complete native vertex-blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct VertexBlendConstruction {
    /// Positive serializer-revision integer selecting the revision-gated
    /// layout; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    /// Ordered boundary records.
    pub boundaries: Vec<VertexBlendBoundary>,
    /// Native grid-size integer.
    pub grid_size: i64,
    /// Native model-space fit tolerance.
    pub fit_tolerance: FitTolerance,
}

/// One member of a compound-loft scale block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompoundLoftScaleMember {
    /// Native member integer.
    pub type_code: i64,
    /// Member curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData,
}

/// Complete `_readScaleClLoft` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompoundLoftScale {
    /// Ordered scale members.
    pub members: Vec<CompoundLoftScaleMember>,
    /// Scale path curve.
    pub path: CurveId,
    /// Ordered BS3 auxiliary curves.
    pub auxiliaries: Vec<CurveId>,
    /// Two native trailing integers.
    pub tail: [i64; 2],
}

/// Direction carrier in the zero-kind compound-loft tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompoundLoftDirection {
    /// Inline direction vector when the selector is zero.
    Vector {
        /// Stored direction.
        value: Vector3,
    },
    /// BS3 direction curve when the selector is nonzero.
    Curve {
        /// Stored curve.
        curve: CurveId,
        /// Exact nonzero native selector retained for byte-faithful export.
        #[serde(skip, default = "default_compound_loft_curve_selector")]
        #[cfg_attr(feature = "schema", schemars(skip))]
        selector: NonZeroI64,
    },
}

const fn default_compound_loft_curve_selector() -> NonZeroI64 {
    NonZeroI64::new(1).expect("one is nonzero")
}

impl CompoundLoftDirection {
    /// Native selector for this direction form.
    #[must_use]
    pub const fn selector(&self) -> i64 {
        match self {
            Self::Vector { .. } => 0,
            Self::Curve { selector, .. } => selector.get(),
        }
    }
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(
    dead_code,
    reason = "fields define the revision compound-loft direction wire schema"
)]
struct RevisionCompoundLoftDirectionSchemaWire {
    kind: i64,
    selector: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direction: Option<Vector3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direction_curve: Option<CurveId>,
}

mod revision_compound_loft_direction_wire {
    use super::{CompoundLoftDirection, CurveId, Vector3};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::num::NonZeroI64;

    #[derive(Serialize, Deserialize)]
    struct Wire {
        kind: i64,
        selector: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction: Option<Vector3>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        direction_curve: Option<CurveId>,
    }

    pub fn serialize<S>(value: &CompoundLoftDirection, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            CompoundLoftDirection::Vector { value } => Wire {
                kind: 0,
                selector: 0,
                direction: Some(*value),
                direction_curve: None,
            },
            CompoundLoftDirection::Curve { curve, selector } => Wire {
                kind: 0,
                selector: selector.get(),
                direction: None,
                direction_curve: Some(curve.clone()),
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<CompoundLoftDirection, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        if wire.kind != 0 {
            return Err(serde::de::Error::custom(
                "revision compound loft requires kind zero",
            ));
        }
        match (wire.selector, wire.direction, wire.direction_curve) {
            (0, Some(value), None) => Ok(CompoundLoftDirection::Vector { value }),
            (selector, None, Some(curve)) => NonZeroI64::new(selector)
                .map(|selector| CompoundLoftDirection::Curve { curve, selector })
                .ok_or_else(|| {
                    serde::de::Error::custom("compound-loft direction conflicts with its selector")
                }),
            _ => Err(serde::de::Error::custom(
                "compound-loft direction conflicts with its selector",
            )),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CompoundLoftDirectionWire {
    selector: i64,
    direction: CompoundLoftDirection,
}

mod compound_loft_direction_wire {
    use super::{CompoundLoftDirection, CompoundLoftDirectionWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::num::NonZeroI64;

    pub fn serialize<S>(direction: &CompoundLoftDirection, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CompoundLoftDirectionWire {
            selector: direction.selector(),
            direction: direction.clone(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<CompoundLoftDirection, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CompoundLoftDirectionWire::deserialize(deserializer)?;
        match (wire.selector, wire.direction) {
            (0, direction @ CompoundLoftDirection::Vector { .. }) => Ok(direction),
            (selector, CompoundLoftDirection::Curve { curve, .. }) => NonZeroI64::new(selector)
                .map(|selector| CompoundLoftDirection::Curve { curve, selector })
                .ok_or_else(|| {
                    serde::de::Error::custom("compound-loft curve selector must be nonzero")
                }),
            _ => Err(serde::de::Error::custom(
                "compound-loft vector selector must be zero",
            )),
        }
    }
}

/// Structurally selected tail of `cl_loft_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompoundLoftTail {
    /// Native kind `6` tail.
    Six {
        /// Two leading flags.
        flags: [bool; 2],
        /// Required scale block.
        scale: Box<CompoundLoftScale>,
        /// Native integer following the scale.
        selector: i64,
        /// Stored direction.
        direction: Vector3,
        /// Native parameter interval.
        parameter_range: [f64; 2],
        /// BS3 tail curve.
        curve: CurveId,
    },
    /// Native kind `7` tail.
    Seven {
        /// First flag.
        first_flag: bool,
        /// First optional scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_scale: Option<Box<CompoundLoftScale>>,
        /// Second flag.
        second_flag: bool,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction.
        direction: Vector3,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
    /// Native kind `0` tail.
    Zero {
        /// Two leading flags.
        flags: [bool; 2],
        /// Vector or BS3 curve with its derived native selector.
        #[serde(flatten, with = "compound_loft_direction_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "CompoundLoftDirectionWire"))]
        direction: CompoundLoftDirection,
        /// Two trailing flags.
        trailing_flags: [bool; 2],
    },
}

/// A bounded leading prefix of compound-loft scales.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Vec<Option<CompoundLoftScale>>")]
pub struct CompoundLoftScales<const CAPACITY: usize>(Vec<CompoundLoftScale>);

#[cfg(feature = "schema")]
impl<const CAPACITY: usize> JsonSchema for CompoundLoftScales<CAPACITY> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("CompoundLoftScales_{CAPACITY}").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = Vec::<Option<CompoundLoftScale>>::json_schema(generator);
        schema.insert("minItems".into(), CAPACITY.into());
        schema.insert("maxItems".into(), CAPACITY.into());
        schema
    }
}

impl<const CAPACITY: usize> CompoundLoftScales<CAPACITY> {
    /// Admit a leading scale list within the native slot capacity.
    pub fn try_new(scales: Vec<CompoundLoftScale>) -> Result<Self, &'static str> {
        if scales.len() > CAPACITY {
            return Err("compound loft scales exceed slot capacity");
        }
        Ok(Self(scales))
    }

    /// Admit native optional slots whose present values form a leading prefix.
    pub fn try_from_slots(
        slots: impl IntoIterator<Item = Option<CompoundLoftScale>>,
    ) -> Result<Self, &'static str> {
        let mut scales = Vec::new();
        let mut absent = false;
        for (index, slot) in slots.into_iter().enumerate() {
            if index >= CAPACITY {
                return Err("compound loft scales exceed slot capacity");
            }
            match slot {
                Some(scale) if !absent => scales.push(scale),
                Some(_) => return Err("compound loft scales must form a leading prefix"),
                None => absent = true,
            }
        }
        Ok(Self(scales))
    }

    /// Present scales in native order.
    #[must_use]
    pub fn as_slice(&self) -> &[CompoundLoftScale] {
        &self.0
    }
}

impl<const CAPACITY: usize> TryFrom<Vec<Option<CompoundLoftScale>>>
    for CompoundLoftScales<CAPACITY>
{
    type Error = &'static str;
    fn try_from(slots: Vec<Option<CompoundLoftScale>>) -> Result<Self, Self::Error> {
        if slots.len() != CAPACITY {
            return Err("compound loft scales have the wrong slot count");
        }
        Self::try_from_slots(slots)
    }
}

impl<const CAPACITY: usize> Serialize for CompoundLoftScales<CAPACITY> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq((0..CAPACITY).map(|index| self.0.get(index)))
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CompoundLoftScalesWire {
    scales: [Option<CompoundLoftScale>; 4],
    #[serde(default)]
    fifth_scale: Option<CompoundLoftScale>,
}

mod compound_loft_scales_wire {
    use super::{CompoundLoftScale, CompoundLoftScales, CompoundLoftScalesWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        scales: &CompoundLoftScales<5>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            scales: [Option<&'a CompoundLoftScale>; 4],
            #[serde(skip_serializing_if = "Option::is_none")]
            fifth_scale: Option<&'a CompoundLoftScale>,
        }
        Wire {
            scales: std::array::from_fn(|index| scales.as_slice().get(index)),
            fifth_scale: scales.as_slice().get(4),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<CompoundLoftScales<5>, D::Error> {
        let wire = CompoundLoftScalesWire::deserialize(deserializer)?;
        CompoundLoftScales::try_from_slots(wire.scales.into_iter().chain([wire.fifth_scale]))
            .map_err(serde::de::Error::custom)
    }
}

/// Complete native compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompoundLoftConstruction {
    /// Present leading scales, up to the optional fifth native slot.
    #[serde(flatten, with = "compound_loft_scales_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "CompoundLoftScalesWire"))]
    pub scales: CompoundLoftScales<5>,
    /// Two flags before the tail kind.
    pub flags: [bool; 2],
    /// Kind-specific trailing graph.
    pub tail: CompoundLoftTail,
}

/// Initial solved-shape branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScaledCompoundLoftShape {
    /// A solved NURBS cache follows the singularity enum.
    Full,
    /// The cache is replaced by two intervals and two scalar arrays.
    None {
        /// Two ordered native intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Two ordered native scalar arrays.
        parameters: [Vec<f64>; 2],
    },
}

/// Structurally selected middle branch of a scaled compound loft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScaledCompoundLoftBranch {
    /// Extended branch ending in a direction vector.
    ExtendedVector {
        /// Optional first scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_scale: Option<Box<CompoundLoftScale>>,
        /// Required second scale block.
        second_scale: Box<CompoundLoftScale>,
        /// Native selector integer.
        selector: i64,
        /// Stored direction vector.
        direction: Vector3,
    },
    /// Extended branch ending in a singularity and curve.
    ExtendedCurve {
        /// Optional scale block.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<Box<CompoundLoftScale>>,
        /// Native branch flag.
        flag: bool,
        /// Native singularity enum.
        singularity: i64,
        /// Stored BS3 curve.
        curve: CurveId,
    },
    /// Direct vector-or-curve branch.
    Direct {
        /// Native branch flag.
        flag: bool,
        /// Vector or BS3 curve with its derived native selector.
        #[serde(flatten, with = "compound_loft_direction_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "CompoundLoftDirectionWire"))]
        direction: CompoundLoftDirection,
    },
}

/// Complete native scaled compound-loft construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ScaledCompoundLoftConstruction {
    /// Native leading singularity enum.
    pub singularity: i64,
    /// Singularity-selected solved-shape payload.
    pub shape: ScaledCompoundLoftShape,
    /// Six ordered discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
    /// Present leading scales within the three native slots.
    pub scales: CompoundLoftScales<3>,
    /// Two native flags preceding the selector.
    pub flags: [bool; 2],
    /// Native integer preceding the middle branch.
    pub selector: i64,
    /// Structurally selected middle branch.
    pub branch: ScaledCompoundLoftBranch,
    /// Two trailing branch flags.
    pub trailing_flags: [bool; 2],
    /// Native trailing kind integer.
    pub tail_kind: i64,
    /// Two native trailing vectors.
    pub tail_directions: [Vector3; 2],
    /// Native trailing singularity enum.
    pub tail_singularity: i64,
    /// Native trailing BS3 curve.
    pub tail_curve: CurveId,
}

/// A native law formula name that cannot be the `null_law` sentinel.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct LawFormulaName(String);

impl LawFormulaName {
    /// Construct a non-sentinel law formula name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Option<Self> {
        let name = name.into();
        (name != "null_law").then_some(Self(name))
    }

    /// Borrow the native formula name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for LawFormulaName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Self::new(name)
            .ok_or_else(|| serde::de::Error::custom("law formula name cannot be null_law"))
    }
}

impl std::fmt::Display for LawFormulaName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One recursively framed native law formula.
#[derive(Debug, Clone, PartialEq)]
pub enum LawFormula {
    /// Native `null_law` with no variables.
    Null,
    /// Named formula and its ordered recursive variables.
    Named {
        /// Non-sentinel native formula name.
        name: LawFormulaName,
        /// Ordered recursive variables.
        variables: Vec<LawExpression>,
    },
}

impl LawFormula {
    /// Native formula name, including `null_law` for the null variant.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Null => "null_law",
            Self::Named { name, .. } => name.as_str(),
        }
    }

    /// Ordered recursive variables; empty for the null variant.
    #[must_use]
    pub fn variables(&self) -> &[LawExpression] {
        match self {
            Self::Null => &[],
            Self::Named { variables, .. } => variables,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct LawFormulaWire {
    name: String,
    variables: Vec<LawExpression>,
}

impl Serialize for LawFormula {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("LawFormula", 2)?;
        state.serialize_field("name", self.name())?;
        state.serialize_field("variables", self.variables())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for LawFormula {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = LawFormulaWire::deserialize(deserializer)?;
        match LawFormulaName::new(wire.name) {
            Some(name) => Ok(Self::Named {
                name,
                variables: wire.variables,
            }),
            None if wire.variables.is_empty() => Ok(Self::Null),
            None => Err(serde::de::Error::custom(
                "null_law formula cannot carry variables",
            )),
        }
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for LawFormula {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "LawFormula".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        LawFormulaWire::json_schema(generator)
    }
}

/// Complete recursive construction stored by a native law spline surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LawSurfaceConstruction {
    /// Legacy U and V parameter intervals; absent from modern layouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_ranges: Option<[[f64; 2]; 2]>,
    /// Primary recursive surface law.
    pub primary: LawFormula,
    /// Ordered counted auxiliary laws referenced by the primary law.
    pub additional: Vec<LawFormula>,
    /// Standard surface-tail mode and its mode-specific fields.
    pub tail: LawSurfaceTail,
    /// Six ordered discontinuity arrays from the standard surface tail.
    pub discontinuities: [Vec<f64>; 6],
}

/// Mode-specific payload of a native law surface's standard surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LawSurfaceTail {
    /// Selector 0; the surface record carries a solved NURBS cache.
    Full,
    /// Selector 1; compact parameter summaries replace the solved cache.
    Summary {
        /// Ordered U and V parameter summaries.
        parameters: [Vec<f64>; 2],
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
        parameter_ranges: [[f64; 2]; 2],
        /// Ordered U and V closure enums.
        closures: [i64; 2],
        /// Ordered U and V singularity enums.
        singularities: [i64; 2],
    },
    /// Selector 3; no mode-specific payload.
    Historical,
    /// Selector 4; no mode-specific payload.
    Optimal,
}

/// One native law-expression node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LawExpression {
    /// Zero-payload `null_law` sentinel.
    Null,
    /// Serializer-preserved textual law expression.
    Text {
        /// Exact text stored in the native law slot.
        value: String,
    },
    /// Tagged integer constant.
    Integer {
        /// Stored integer value.
        value: i64,
    },
    /// Tagged double constant.
    Double {
        /// Stored scalar value.
        value: f64,
    },
    /// Tagged model-space point constant.
    Point {
        /// Stored point value.
        value: Point3,
    },
    /// Tagged direction-vector constant.
    Vector {
        /// Stored vector value.
        value: Vector3,
    },
    /// Inline transform-law payload.
    Transform {
        /// Thirteen ordered transform scalars.
        scalars: [f64; 13],
        /// Three ordered transform enums.
        enums: [i64; 3],
    },
    /// Vector-serialized transform-law payload: four ordered vectors, a scale,
    /// and three flags, in place of the thirteen-scalar/three-enum form.
    TransformVec {
        /// Four ordered transform vectors.
        vectors: [Vector3; 4],
        /// Trailing transform scale.
        scale: f64,
        /// Three ordered transform flags.
        flags: [bool; 3],
    },
    /// Curve-backed edge law.
    Edge {
        /// Embedded curve carrier and its optional revision-gated endpoints.
        #[serde(flatten)]
        curve: LoftPathCurve,
        /// Two native curve parameters.
        parameters: [f64; 2],
    },
    /// Spline-law payload.
    Spline {
        /// Native spline-law integer.
        native_id: i64,
        /// Ordered spline-law knots.
        knots: Vec<f64>,
        /// Ordered spline-law controls.
        controls: Vec<f64>,
        /// Native model-space point.
        point: Point3,
    },
    /// Algebraic operator and its recursively framed operands.
    Algebraic {
        /// Native operator token.
        operator: String,
        /// Ordered operands.
        operands: Vec<LawExpression>,
    },
}

/// One profile entry in the expanded skin layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkinSurfaceProfile {
    /// Native profile type integer.
    pub type_code: i64,
    /// Profile curve.
    pub curve: CurveId,
    /// Native loft constraint data.
    pub data: ClassicLoftProfileData,
}

/// Structurally selected native skin payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkinSurfaceLayout {
    /// Expanded sequence of profile curves and loft constraints.
    Profiles {
        /// Ordered profile entries.
        profiles: Vec<SkinSurfaceProfile>,
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
        subdata: LoftSubdata,
        /// Integer after the subdata.
        first_tail: i64,
        /// Secondary curve.
        secondary_curve: CurveId,
        /// Final compact-layout integer.
        second_tail: i64,
    },
}

impl SkinSurfaceLayout {
    /// Native inner count, derived from the profile list in the expanded form.
    pub fn inner_count(&self) -> i64 {
        match self {
            Self::Profiles { profiles, .. } => profiles.len() as i64,
            Self::Compact { inner_count, .. } => *inner_count,
        }
    }
}

mod skin_surface_layout_wire {
    use super::{CurveId, LoftSubdata, SkinSurfaceLayout, SkinSurfaceProfile};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Layout<P, C, S> {
        Profiles {
            profiles: P,
            path: C,
            tail: [i64; 2],
        },
        Compact {
            curve: C,
            subdata: S,
            first_tail: i64,
            secondary_curve: C,
            second_tail: i64,
        },
    }

    #[derive(Deserialize)]
    #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
    pub(super) struct ReadWire {
        inner_count: i64,
        layout: Layout<Vec<SkinSurfaceProfile>, CurveId, LoftSubdata>,
    }

    pub fn serialize<S: Serializer>(
        value: &SkinSurfaceLayout,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct WriteWire<'a> {
            inner_count: i64,
            layout: Layout<&'a [SkinSurfaceProfile], &'a CurveId, &'a LoftSubdata>,
        }
        let layout = match value {
            SkinSurfaceLayout::Profiles {
                profiles,
                path,
                tail,
            } => Layout::Profiles {
                profiles: profiles.as_slice(),
                path,
                tail: *tail,
            },
            SkinSurfaceLayout::Compact {
                curve,
                subdata,
                first_tail,
                secondary_curve,
                second_tail,
                ..
            } => Layout::Compact {
                curve,
                subdata,
                first_tail: *first_tail,
                secondary_curve,
                second_tail: *second_tail,
            },
        };
        WriteWire {
            inner_count: value.inner_count(),
            layout,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SkinSurfaceLayout, D::Error> {
        let wire = ReadWire::deserialize(deserializer)?;
        Ok(match wire.layout {
            Layout::Profiles {
                profiles,
                path,
                tail,
            } => {
                if usize::try_from(wire.inner_count).ok() != Some(profiles.len()) {
                    return Err(serde::de::Error::custom(
                        "skin inner_count must match profiles",
                    ));
                }
                SkinSurfaceLayout::Profiles {
                    profiles,
                    path,
                    tail,
                }
            }
            Layout::Compact {
                curve,
                subdata,
                first_tail,
                secondary_curve,
                second_tail,
            } => SkinSurfaceLayout::Compact {
                inner_count: wire.inner_count,
                curve,
                subdata,
                first_tail,
                secondary_curve,
                second_tail,
            },
        })
    }
}

/// Complete native `skin_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SkinSurfaceConstruction {
    /// Native `SURF_BOOL` enum.
    pub surface_boolean: i64,
    /// Native `SURF_NORM` enum.
    pub surface_normal: i64,
    /// Native `SURF_DIR` enum.
    pub surface_direction: i64,
    /// Native leading count.
    pub count: i64,
    /// Native leading scalar.
    pub parameter: f64,
    /// Structurally selected skin payload.
    #[serde(flatten, with = "skin_surface_layout_wire")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "skin_surface_layout_wire::ReadWire")
    )]
    pub layout: SkinSurfaceLayout,
    /// Stored direction vector.
    pub direction: Vector3,
    /// Native scalar before the formula.
    pub trailing_parameter: f64,
    /// Recursive parametric law.
    pub formula: LawFormula,
    /// Trailing curve after the formula.
    pub parameter_curve: CurveId,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Complete native `net_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct NetSurfaceConstruction {
    /// Two ordered loft-section graphs.
    pub sections: Box<[LoftSection; 2]>,
    /// Twelve ordered frame scalars.
    pub frame_parameters: [f64; 12],
    /// Native frame integer.
    pub flag: i64,
    /// Four ordered frame directions.
    pub directions: [Vector3; 4],
    /// Four ordered parameter laws.
    pub formulas: Box<[LawFormula; 4]>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Structurally selected native sweep payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SweepSurfaceLayout {
    /// Profile-first modern ASM sweep layout.
    ProfileFirst {
        /// Second native sweep enum.
        secondary_kind: i64,
        /// Five ordered frame directions.
        directions: [Vector3; 5],
        /// Native model-space frame origin.
        origin: Point3,
        /// Four ordered native frame scalars.
        parameters: [f64; 4],
        /// Three ordered parametric laws.
        formulas: Box<[LawFormula; 3]>,
    },
    /// Explicit sweep layout whose trajectory is controlled by a formula.
    ExplicitFormula {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Native formula-side boolean.
        formula_flag: bool,
        /// Parametric trajectory formula.
        formula: LawFormula,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
    /// Explicit sweep layout controlled by an auxiliary guide curve.
    ExplicitGuide {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Two guide-side booleans.
        guide_flags: [bool; 2],
        /// Auxiliary guide curve.
        guide_curve: CurveId,
        /// Guide parameter interval.
        guide_range: [f64; 2],
        /// Two native guide integers.
        guide_modes: [i64; 2],
        /// Six ordered guide scalars.
        guide_parameters: [f64; 6],
        /// Three trailing guide booleans.
        trailing_flags: [bool; 3],
    },
    /// Explicit sweep layout controlled by a support surface.
    ExplicitSurface {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Native trajectory boolean.
        trajectory_flag: bool,
        /// Path parameter interval in model length units.
        path_range: [f64; 2],
        /// Native trajectory scalar.
        path_parameter: f64,
        /// Native singularity enum.
        singularity: i64,
        /// Support surface controlling the sweep.
        support_surface: SurfaceId,
        /// Optional auxiliary curve.
        auxiliary_curve: Option<CurveId>,
        /// Native support-side boolean.
        support_flag: bool,
        /// Legacy pre-219 trailing boolean when present.
        legacy_flag: Option<bool>,
    },
    /// Explicit-prefix sweep layout controlled by recursive laws.
    LawDriven {
        /// Native explicit-layout integer.
        mode: i64,
        /// Profile parameter interval.
        profile_range: [f64; 2],
        /// Optional explicit profile frame.
        profile_frame: Option<(Point3, Vector3)>,
        /// Sweep frame origin.
        origin: Point3,
        /// Three ordered sweep frame directions.
        directions: [Vector3; 3],
        /// Leading recursive sweep law.
        first_law: Box<LawExpression>,
        /// Native integer after the leading law.
        first_mode: i64,
        /// First law parameter interval.
        first_range: [f64; 2],
        /// Native law direction.
        law_direction: Vector3,
        /// Native path integer.
        path_mode: i64,
        /// Native path boolean.
        path_flag: bool,
        /// Path parameter interval.
        path_range: [f64; 2],
        /// Native path scalar.
        path_parameter: f64,
        /// Native second-law boolean.
        second_law_flag: bool,
        /// Trailing recursive sweep law.
        second_law: Box<LawExpression>,
        /// Native integer before the formula.
        formula_mode: i64,
        /// Parametric trajectory formula.
        formula: LawFormula,
        /// Native trailing boolean.
        trailing_flag: bool,
    },
}

/// Revision-gated `sweep_sur` form fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepRevisionForm {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: i64,
    /// Boolean replacing the pre-revision primary enum.
    pub primary_flag: bool,
    /// Optional parameter endpoints following the embedded profile curve.
    #[serde(default)]
    pub profile_endpoints: [Option<f64>; 2],
    /// Optional parameter endpoints following the embedded path curve.
    #[serde(default)]
    pub path_endpoints: [Option<f64>; 2],
    /// Approximation-cache form selected by the shared tail enum.
    #[serde(flatten, with = "revision_surface_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "RevisionSurfaceCacheSchemaWire"))]
    pub cache: RevisionCacheForm,
}

/// Complete native `sweep_spl_sur` construction graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct SweepSurfaceConstruction {
    /// Leading native sweep enum.
    pub primary_kind: i64,
    /// Revision-gated form fields; absent from the pre-revision layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_form: Option<SweepRevisionForm>,
    /// Structurally selected sweep layout.
    pub layout: SweepSurfaceLayout,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<f64>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

/// Radius law for a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlendRadiusLaw {
    /// Constant blend radius along the whole spine.
    Constant {
        /// Signed radius, in document length units; sign selects the support offset side.
        signed_radius: f64,
    },
    /// Radius varying linearly from `start` to `end` along the spine.
    Linear {
        /// Signed radius at the spine start, in document length units.
        start: f64,
        /// Signed radius at the spine end, in document length units.
        end: f64,
    },
    /// Radius varying along the spine per an explicit law curve.
    Law {
        /// Curve whose parameterization gives the signed radius along the spine.
        curve: NurbsCurve,
    },
}

/// A neutral curve construction linked to its solved carrier.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralCurve {
    /// Stable construction identity.
    pub id: ProceduralCurveId,
    /// Neutral construction definition.
    definition: ProceduralCurveDefinition,
    /// Fit contract of a legacy solved cache. Revision-gated forms carry the
    /// same value in their [`RevisionCacheForm`].
    legacy_cache_fit_tolerance: Option<FitTolerance>,
}

/// A parameter-space support curve and its optional affine parameter map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SupportPcurve {
    /// UV curve carried by the support side.
    pub geometry: PcurveGeometry,
    /// Directed pcurve interval corresponding to the solved interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_range: Option<DirectedParameterRange>,
}

impl SupportPcurve {
    /// Pair a pcurve geometry with an optional checked parameter interval.
    #[must_use]
    pub const fn new(
        geometry: PcurveGeometry,
        parameter_range: Option<DirectedParameterRange>,
    ) -> Self {
        Self {
            geometry,
            parameter_range,
        }
    }

    /// Return the mapped pcurve endpoints as an array.
    #[must_use]
    pub const fn parameter_range_array(&self) -> Option<[f64; 2]> {
        match self.parameter_range {
            Some(range) => Some(range.endpoints()),
            None => None,
        }
    }
}

impl From<PcurveGeometry> for SupportPcurve {
    fn from(geometry: PcurveGeometry) -> Self {
        Self::new(geometry, None)
    }
}

/// A finite, non-zero-width directed parameter interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct DirectedParameterRange([f64; 2]);

/// Error returned when a directed parameter range cannot be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRangeError;

impl std::fmt::Display for ParameterRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("parameter range endpoints must be finite and distinct")
    }
}

impl std::error::Error for ParameterRangeError {}

impl DirectedParameterRange {
    /// Construct an interval. Either ascending or descending direction is valid.
    pub fn new(values: [f64; 2]) -> Result<Self, ParameterRangeError> {
        if values.iter().all(|value| value.is_finite()) && values[0] != values[1] {
            Ok(Self(values))
        } else {
            Err(ParameterRangeError)
        }
    }

    /// Return the directed endpoints.
    #[must_use]
    pub const fn endpoints(self) -> [f64; 2] {
        self.0
    }
}

impl<'de> Deserialize<'de> for DirectedParameterRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(<[f64; 2]>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One paired surface and parameter-space curve in an intcurve construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IntcurveSupportSide {
    /// Supporting surface, absent for the native `null_surface` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<SurfaceId>,
    /// UV curve on `surface`, absent for the native `nullbs` sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<SupportPcurve>,
}

impl IntcurveSupportSide {
    /// Return the mapped pcurve endpoints as an array.
    #[must_use]
    pub fn pcurve_parameter_range(&self) -> Option<[f64; 2]> {
        self.pcurve
            .as_ref()
            .and_then(SupportPcurve::parameter_range_array)
    }

    /// Map one solved-curve parameter into this side's pcurve parameter.
    ///
    /// Returns `None` when this side has no pcurve, an explicit map has an invalid
    /// solved interval, or the mapped parameter cannot be represented as finite.
    #[must_use]
    pub fn pcurve_parameter(
        &self,
        solved_parameter_range: [f64; 2],
        parameter: f64,
    ) -> Option<f64> {
        if !parameter.is_finite() {
            return None;
        }
        let pcurve_range = self
            .pcurve
            .as_ref()?
            .parameter_range
            .map(DirectedParameterRange::endpoints);
        let Some(pcurve_range) = pcurve_range else {
            return Some(parameter);
        };
        let solved_span = solved_parameter_range[1] - solved_parameter_range[0];
        if solved_span == 0.0
            || solved_parameter_range
                .iter()
                .any(|value| !value.is_finite())
        {
            return None;
        }
        if parameter == solved_parameter_range[0] {
            return Some(pcurve_range[0]);
        }
        if parameter == solved_parameter_range[1] {
            return Some(pcurve_range[1]);
        }
        let offset = parameter - solved_parameter_range[0];
        let fraction = if solved_span.is_finite() && offset.is_finite() {
            offset / solved_span
        } else {
            // Halving before subtraction retains finite opposite-sign endpoints.
            (parameter * 0.5 - solved_parameter_range[0] * 0.5)
                / (solved_parameter_range[1] * 0.5 - solved_parameter_range[0] * 0.5)
        };
        if !fraction.is_finite() {
            return None;
        }
        let mapped = pcurve_range[0] + fraction * (pcurve_range[1] - pcurve_range[0]);
        if mapped.is_finite() {
            Some(mapped)
        } else {
            let mapped = (1.0 - fraction) * pcurve_range[0] + fraction * pcurve_range[1];
            mapped.is_finite().then_some(mapped)
        }
    }
}

/// Version-stamp prefix and unbounded interval carried by the stamped
/// `law_int_cur` serializer form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LawCurveVersionForm {
    /// Serializer version stamp emitted after the subtype name.
    pub stamp: i64,
    /// Native enum following the version stamp.
    pub post_enum: i64,
    /// Solved-curve interval endpoints; `None` records an unbounded bound.
    pub parameter_range: [Option<f64>; 2],
}

/// Shared support surfaces, UV curves, interval, and discontinuity arrays of a native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "IntcurveSupportContextWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct IntcurveSupportContext {
    sides: [IntcurveSupportSide; 2],
    parameter_range: [f64; 2],
    discontinuities: [Vec<f64>; 3],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct IntcurveSupportContextWire {
    sides: [IntcurveSupportSide; 2],
    parameter_range: [f64; 2],
    discontinuities: [Vec<f64>; 3],
}

impl TryFrom<IntcurveSupportContextWire> for IntcurveSupportContext {
    type Error = &'static str;

    fn try_from(wire: IntcurveSupportContextWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.sides, wire.parameter_range, wire.discontinuities)
    }
}

impl IntcurveSupportContext {
    /// Construct a support context with a finite ordered interval and finite discontinuities.
    pub fn try_new(
        sides: [IntcurveSupportSide; 2],
        parameter_range: [f64; 2],
        discontinuities: [Vec<f64>; 3],
    ) -> Result<Self, &'static str> {
        if !parameter_range.iter().all(|value| value.is_finite())
            || parameter_range[0] > parameter_range[1]
        {
            return Err("support context parameter_range must be finite and ordered");
        }
        if parameter_range[0] == parameter_range[1]
            && sides.iter().any(|side| {
                side.pcurve
                    .as_ref()
                    .is_some_and(|pcurve| pcurve.parameter_range.is_some())
            })
        {
            return Err(
                "support context parameter_range must be nonzero for an explicit pcurve mapping",
            );
        }
        if !discontinuities
            .iter()
            .flatten()
            .all(|value| value.is_finite())
        {
            return Err("support context discontinuities must be finite");
        }
        Ok(Self {
            sides,
            parameter_range,
            discontinuities,
        })
    }

    /// Edit the context transactionally and retain its previous value on rejection.
    pub fn edit<R>(
        &mut self,
        edit: impl FnOnce(&mut [IntcurveSupportSide; 2], &mut [f64; 2], &mut [Vec<f64>; 3]) -> R,
    ) -> Result<R, &'static str> {
        let mut candidate = self.clone();
        let result = edit(
            &mut candidate.sides,
            &mut candidate.parameter_range,
            &mut candidate.discontinuities,
        );
        *self = Self::try_new(
            candidate.sides,
            candidate.parameter_range,
            candidate.discontinuities,
        )?;
        Ok(result)
    }

    /// Set a support surface without changing its pcurve mapping.
    pub fn set_surface(&mut self, side: usize, surface: Option<SurfaceId>) {
        self.sides[side].surface = surface;
    }

    /// Set a support pcurve with the solved-curve parameterization.
    pub fn set_unmapped_pcurve(&mut self, side: usize, geometry: Option<PcurveGeometry>) {
        self.sides[side].pcurve = geometry.map(SupportPcurve::from);
    }

    /// Copy a pcurve mapping between support sides of this context.
    pub fn copy_pcurve(&mut self, source: usize, target: usize) {
        self.sides[target].pcurve = self.sides[source].pcurve.clone();
    }

    /// Return the ordered support sides.
    #[must_use]
    pub const fn sides(&self) -> &[IntcurveSupportSide; 2] {
        &self.sides
    }

    /// Return the solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> [f64; 2] {
        self.parameter_range
    }

    /// Return the ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<f64>; 3] {
        &self.discontinuities
    }
}

/// Finite endpoint witnesses and distinct supports of a tolerant intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionConstructionWire")]
pub struct TolerantIntersectionConstruction {
    supports: [SurfaceId; 2],
    endpoints: [Point3; 2],
    tolerance: f64,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TolerantIntersectionConstructionWire {
    supports: [SurfaceId; 2],
    endpoints: [Point3; 2],
    tolerance: f64,
}

impl TryFrom<TolerantIntersectionConstructionWire> for TolerantIntersectionConstruction {
    type Error = &'static str;
    fn try_from(wire: TolerantIntersectionConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.supports, wire.endpoints, wire.tolerance)
    }
}

impl TolerantIntersectionConstruction {
    /// Admit distinct supports, finite endpoints, and a finite non-negative tolerance.
    pub fn try_new(
        supports: [SurfaceId; 2],
        endpoints: [Point3; 2],
        tolerance: f64,
    ) -> Result<Self, &'static str> {
        if supports[0] == supports[1] {
            return Err("tolerant intersection supports must be distinct");
        }
        if !endpoints
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        {
            return Err("tolerant intersection endpoints must be finite");
        }
        FitTolerance::try_new(tolerance)
            .map_err(|_| "tolerant intersection tolerance must be finite and non-negative")?;
        Ok(Self {
            supports,
            endpoints,
            tolerance,
        })
    }

    /// Support surfaces, endpoint witnesses, and maximum admitted deviation.
    #[must_use]
    pub const fn parts(&self) -> (&[SurfaceId; 2], &[Point3; 2], &f64) {
        (&self.supports, &self.endpoints, &self.tolerance)
    }
}

/// Complete neutral parameterization of one topology-bounded intersection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "TolerantIntersectionParameterizationWire")]
pub struct TolerantIntersectionParameterization {
    /// Coincident support charts in support order.
    pub pcurves: [PcurveGeometry; 2],
    parameter_range: [f64; 2],
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct TolerantIntersectionParameterizationWire {
    pcurves: [PcurveGeometry; 2],
    parameter_range: [f64; 2],
}

impl TryFrom<TolerantIntersectionParameterizationWire> for TolerantIntersectionParameterization {
    type Error = &'static str;
    fn try_from(wire: TolerantIntersectionParameterizationWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.pcurves, wire.parameter_range)
    }
}

impl TolerantIntersectionParameterization {
    /// Admit a finite strictly increasing solved-curve interval.
    pub fn try_new(
        pcurves: [PcurveGeometry; 2],
        parameter_range: [f64; 2],
    ) -> Result<Self, &'static str> {
        if !parameter_range.iter().all(|value| value.is_finite())
            || parameter_range[0] >= parameter_range[1]
        {
            return Err(
                "tolerant intersection parameter_range must be finite and strictly increasing",
            );
        }
        Ok(Self {
            pcurves,
            parameter_range,
        })
    }

    /// Common finite solved-curve interval.
    #[must_use]
    pub const fn parameter_range(&self) -> [f64; 2] {
        self.parameter_range
    }
}

/// Cache-first shared-context fields absent from the context-first layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CacheFirstCurveForm {
    /// Positive serializer-revision integer selecting the cache-first layout.
    pub revision: i64,
    /// Approximation-cache form selected by the shared context enum.
    #[serde(flatten, with = "cache_first_curve_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "CacheFirstCurveCacheSchemaWire"))]
    pub cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    /// Optional U/V bound fields following each ordered support surface.
    #[serde(default)]
    pub support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    #[serde(default)]
    pub solved_range: [Option<f64>; 2],
    /// Native integer ASM extension following the discontinuity arrays.
    pub extension: i64,
}

/// One support slot in a context-first spring construction.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SpringSupport {
    /// Resolved support surface.
    Surface(SurfaceId),
    /// Native U/V ranges stored in place of `null_surface`.
    Ranges([[f64; 2]; 2]),
}

/// First pcurve slot in a context-first spring construction.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum SpringPcurve {
    /// Resolved parameter-space curve.
    Pcurve(PcurveGeometry),
    /// Native interval stored in place of `nullbs`.
    Range([f64; 2]),
}

/// Mutually exclusive spring construction layouts.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
// Variant payloads retain the native layout as one value without separate heap ownership.
#[allow(clippy::large_enum_variant)]
pub enum SpringLayout {
    /// Support-first layout with inline null-carrier replacement ranges.
    ContextFirst {
        /// Two ordered support slots.
        supports: [SpringSupport; 2],
        /// First pcurve or its null replacement range.
        first_pcurve: SpringPcurve,
        /// Nullable second pcurve slot.
        second_pcurve: Option<PcurveGeometry>,
        /// Native solved-curve parameter interval.
        parameter_range: [f64; 2],
        /// Three ordered discontinuity arrays.
        discontinuities: [Vec<f64>; 3],
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
    },
    /// Cache-first layout, which carries no inline replacement ranges or
    /// context-first discontinuity flag.
    CacheFirst {
        /// Shared support context following the solved cache.
        context: IntcurveSupportContext,
        /// Cache-first serializer fields.
        form: CacheFirstCurveForm,
    },
}

impl SpringLayout {
    /// Return the support context, deriving it for the context-first layout.
    pub fn support_context(
        &self,
    ) -> Result<std::borrow::Cow<'_, IntcurveSupportContext>, &'static str> {
        match self {
            Self::CacheFirst { context, .. } => Ok(std::borrow::Cow::Borrowed(context)),
            Self::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                ..
            } => Ok(std::borrow::Cow::Owned(IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: match &supports[0] {
                            SpringSupport::Surface(surface) => Some(surface.clone()),
                            SpringSupport::Ranges(_) => None,
                        },
                        pcurve: match first_pcurve {
                            SpringPcurve::Pcurve(pcurve) => {
                                Some(SupportPcurve::new(pcurve.clone(), None))
                            }
                            SpringPcurve::Range(_) => None,
                        },
                    },
                    IntcurveSupportSide {
                        surface: match &supports[1] {
                            SpringSupport::Surface(surface) => Some(surface.clone()),
                            SpringSupport::Ranges(_) => None,
                        },
                        pcurve: second_pcurve
                            .clone()
                            .map(|pcurve| SupportPcurve::new(pcurve, None)),
                    },
                ],
                *parameter_range,
                discontinuities.clone(),
            )?)),
        }
    }

    fn cache_first(&self) -> Option<&CacheFirstCurveForm> {
        match self {
            Self::CacheFirst { form, .. } => Some(form),
            Self::ContextFirst { .. } => None,
        }
    }

    fn cache_first_mut(&mut self) -> Option<&mut CacheFirstCurveForm> {
        match self {
            Self::CacheFirst { form, .. } => Some(form),
            Self::ContextFirst { .. } => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SpringLayoutWire {
    context: IntcurveSupportContext,
    surface_parameter_ranges: [Option<[[f64; 2]; 2]>; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_pcurve_parameter_range: Option<[f64; 2]>,
    discontinuity_flag: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_first: Option<CacheFirstCurveForm>,
}

mod spring_layout_wire {
    use super::{SpringLayout, SpringLayoutWire, SpringPcurve, SpringSupport};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &SpringLayout, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            SpringLayout::ContextFirst {
                supports,
                first_pcurve,
                discontinuity_flag,
                ..
            } => SpringLayoutWire {
                context: value
                    .support_context()
                    .map_err(serde::ser::Error::custom)?
                    .into_owned(),
                surface_parameter_ranges: std::array::from_fn(|side| match &supports[side] {
                    SpringSupport::Surface(_) => None,
                    SpringSupport::Ranges(ranges) => Some(*ranges),
                }),
                first_pcurve_parameter_range: match first_pcurve {
                    SpringPcurve::Pcurve(_) => None,
                    SpringPcurve::Range(range) => Some(*range),
                },
                discontinuity_flag: *discontinuity_flag,
                cache_first: None,
            },
            SpringLayout::CacheFirst { context, form } => SpringLayoutWire {
                context: context.clone(),
                surface_parameter_ranges: [None, None],
                first_pcurve_parameter_range: None,
                discontinuity_flag: false,
                cache_first: Some(form.clone()),
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SpringLayout, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SpringLayoutWire::deserialize(deserializer)?;
        if let Some(form) = wire.cache_first {
            if wire.surface_parameter_ranges.iter().any(Option::is_some)
                || wire.first_pcurve_parameter_range.is_some()
                || wire.discontinuity_flag
            {
                return Err(serde::de::Error::custom(
                    "cache_first spring cannot carry inline ranges or discontinuity_flag",
                ));
            }
            return Ok(SpringLayout::CacheFirst {
                context: wire.context,
                form,
            });
        }
        let [first_side, second_side] = wire.context.sides;
        if first_side
            .pcurve
            .as_ref()
            .is_some_and(|pcurve| pcurve.parameter_range.is_some())
            || second_side
                .pcurve
                .as_ref()
                .is_some_and(|pcurve| pcurve.parameter_range.is_some())
        {
            return Err(serde::de::Error::custom(
                "spring context sides cannot carry pcurve parameter ranges",
            ));
        }
        let [first_ranges, second_ranges] = wire.surface_parameter_ranges;
        let support = |surface, ranges, side| {
            match (surface, ranges) {
            (Some(surface), None) => Ok(SpringSupport::Surface(surface)),
            (None, Some(ranges)) => Ok(SpringSupport::Ranges(ranges)),
            _ => Err(serde::de::Error::custom(format_args!(
                "spring support side {side} requires exactly one of surface or surface_parameter_ranges"
            ))),
        }
        };
        let first_pcurve = match (first_side.pcurve, wire.first_pcurve_parameter_range) {
            (Some(pcurve), None) => SpringPcurve::Pcurve(pcurve.geometry),
            (None, Some(range)) => SpringPcurve::Range(range),
            _ => {
                return Err(serde::de::Error::custom(
                    "spring first pcurve requires exactly one of pcurve or first_pcurve_parameter_range",
                ));
            }
        };
        Ok(SpringLayout::ContextFirst {
            supports: [
                support(first_side.surface, first_ranges, 0)?,
                support(second_side.surface, second_ranges, 1)?,
            ],
            first_pcurve,
            second_pcurve: second_side.pcurve.map(|pcurve| pcurve.geometry),
            parameter_range: wire.context.parameter_range,
            discontinuities: wire.context.discontinuities,
            discontinuity_flag: wire.discontinuity_flag,
        })
    }
}

/// Parameterization carried by cache form `2` of the shared cache-first
/// intcurve context. This form stores no solved curve cache and no fit
/// tolerance; it stores the curve interval followed by the closed-form enum, in
/// the order the fields appear below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CacheFirstCurveParameterization {
    /// Curve interval, an ordered `[lo, hi]` pair of optional bounds. `None` is
    /// a false bound-presence flag.
    #[serde(default)]
    pub interval: [Option<f64>; 2],
    /// Closed-form enum following the interval.
    pub closed_form: i64,
}

/// Family-independent tail fields carried by a cache-first surface curve.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceCurveTail {
    /// Native integer following the discontinuity arrays.
    pub extension: i64,
    /// Positive serializer-revision integer opening the cache-first layout.
    pub revision: i64,
    /// Approximation-cache form selected by the shared context enum.
    pub cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    /// Optional U/V bound fields following each ordered support surface.
    pub support_bounds: [[Option<f64>; 4]; 2],
    /// Optional solved-curve interval endpoints; absent endpoints inherit the
    /// solved NURBS domain.
    pub solved_range: [Option<f64>; 2],
}

/// Cache-first surface-curve tail paired with its family-specific flags.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceCurveCacheFirst<F> {
    /// Family-independent cache-first fields.
    pub tail: SurfaceCurveTail,
    /// Flags admitted by the selected surface-curve family.
    pub flags: F,
}

/// Two terminating flags carried only by a parametric surface curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParametricSurfaceCurveFlags {
    /// Support-slot selector.
    pub flag: bool,
    /// Optional later-revision terminating flag.
    pub second_flag: Option<bool>,
}

/// Mutually exclusive tail forms of a native projected intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectionTail {
    /// The ASM flag is followed immediately by the subtype close.
    EarlyClose {
        /// Native ASM projection flag.
        flag: bool,
    },
    /// The ASM flag is followed by a retained source interval and role text.
    Ranged {
        /// Native ASM projection flag.
        flag: bool,
        /// Native parameter interval on the projected source curve.
        parameter_range: [f64; 2],
        /// Projection support role.
        role: ProjectionRole,
    },
}

/// Support selected by a ranged projection tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ProjectionRole {
    /// First support surface.
    #[serde(rename = "surf1")]
    Surf1,
    /// Second support surface.
    #[serde(rename = "surf2")]
    Surf2,
}

impl ProjectionRole {
    /// Parse the native ASM identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "surf1" => Some(Self::Surf1),
            "surf2" => Some(Self::Surf2),
            _ => None,
        }
    }

    /// Native ASM identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Surf1 => "surf1",
            Self::Surf2 => "surf2",
        }
    }
}

impl<'de> Deserialize<'de> for ProjectionRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid projection role field value `{value}`; expected `surf1` or `surf2`"
            ))
        })
    }
}

/// Native surface-curve family with its support context and optional
/// cache-first form.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceCurveFamily {
    /// Blend edge curve whose construction details live on its blend support.
    Blend {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Curve constrained to a support surface.
    SurfaceConstrained {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
    /// Parametric curve on a support surface.
    Parametric {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields with the parametric-only second flag.
        tail: Option<SurfaceCurveCacheFirst<ParametricSurfaceCurveFlags>>,
    },
    /// Skin curve on a support surface.
    Skin {
        /// Shared support context.
        context: IntcurveSupportContext,
        /// Cache-first fields, when this is not the prefix-first layout.
        tail: Option<SurfaceCurveCacheFirst<bool>>,
    },
}

/// Discriminant of a native surface-curve family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SurfaceCurveFamilyKind {
    /// Blend edge curve whose construction details live on its blend support.
    Blend,
    /// Curve constrained to a support surface.
    SurfaceConstrained,
    /// Parametric curve on a support surface.
    Parametric,
    /// Skin curve on a support surface.
    Skin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SurfaceCurveTailWire {
    extension: i64,
    flag: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second_flag: Option<bool>,
    #[serde(default)]
    revision: i64,
    #[serde(flatten, with = "cache_first_curve_cache_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "CacheFirstCurveCacheSchemaWire"))]
    cache: RevisionCacheForm<CacheFirstCurveParameterization>,
    #[serde(default)]
    support_bounds: [[Option<f64>; 4]; 2],
    #[serde(default)]
    solved_range: [Option<f64>; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SurfaceCurveFamilyWire {
    family: SurfaceCurveFamilyKind,
    context: IntcurveSupportContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tail: Option<SurfaceCurveTailWire>,
}

impl SurfaceCurveTail {
    fn into_wire(self, flag: bool, second_flag: Option<bool>) -> SurfaceCurveTailWire {
        SurfaceCurveTailWire {
            extension: self.extension,
            flag,
            second_flag,
            revision: self.revision,
            cache: self.cache,
            support_bounds: self.support_bounds,
            solved_range: self.solved_range,
        }
    }
}

impl SurfaceCurveTailWire {
    fn into_tail(self) -> SurfaceCurveTail {
        SurfaceCurveTail {
            extension: self.extension,
            revision: self.revision,
            cache: self.cache,
            support_bounds: self.support_bounds,
            solved_range: self.solved_range,
        }
    }
}

impl SurfaceCurveFamily {
    /// Return the family discriminant.
    #[must_use]
    pub const fn kind(&self) -> SurfaceCurveFamilyKind {
        match self {
            Self::Blend { .. } => SurfaceCurveFamilyKind::Blend,
            Self::SurfaceConstrained { .. } => SurfaceCurveFamilyKind::SurfaceConstrained,
            Self::Parametric { .. } => SurfaceCurveFamilyKind::Parametric,
            Self::Skin { .. } => SurfaceCurveFamilyKind::Skin,
        }
    }

    /// Borrow the shared support context.
    #[must_use]
    pub const fn context(&self) -> &IntcurveSupportContext {
        match self {
            Self::Blend { context, .. }
            | Self::SurfaceConstrained { context, .. }
            | Self::Parametric { context, .. }
            | Self::Skin { context, .. } => context,
        }
    }

    /// Mutably borrow the shared support context.
    #[must_use]
    pub fn context_mut(&mut self) -> &mut IntcurveSupportContext {
        match self {
            Self::Blend { context, .. }
            | Self::SurfaceConstrained { context, .. }
            | Self::Parametric { context, .. }
            | Self::Skin { context, .. } => context,
        }
    }

    fn revision_cache(&self) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_ref().map(|form| &form.tail.cache),
            Self::Parametric { tail, .. } => tail.as_ref().map(|form| &form.tail.cache),
        }
    }

    fn revision_cache_mut(
        &mut self,
    ) -> Option<&mut RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::Blend { tail, .. }
            | Self::SurfaceConstrained { tail, .. }
            | Self::Skin { tail, .. } => tail.as_mut().map(|form| &mut form.tail.cache),
            Self::Parametric { tail, .. } => tail.as_mut().map(|form| &mut form.tail.cache),
        }
    }

    /// Return whether two families have the same discriminant and cache-first
    /// tail, excluding the editable support context.
    #[must_use]
    pub fn has_same_form(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Blend { tail: first, .. }, Self::Blend { tail: second, .. })
            | (
                Self::SurfaceConstrained { tail: first, .. },
                Self::SurfaceConstrained { tail: second, .. },
            )
            | (Self::Skin { tail: first, .. }, Self::Skin { tail: second, .. }) => first == second,
            (Self::Parametric { tail: first, .. }, Self::Parametric { tail: second, .. }) => {
                first == second
            }
            _ => false,
        }
    }
}

impl Serialize for SurfaceCurveFamily {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let wire = match self.clone() {
            Self::Blend { context, tail } => SurfaceCurveFamilyWire {
                family: SurfaceCurveFamilyKind::Blend,
                context,
                tail: tail.map(|value| value.tail.into_wire(value.flags, None)),
            },
            Self::SurfaceConstrained { context, tail } => SurfaceCurveFamilyWire {
                family: SurfaceCurveFamilyKind::SurfaceConstrained,
                context,
                tail: tail.map(|value| value.tail.into_wire(value.flags, None)),
            },
            Self::Parametric { context, tail } => SurfaceCurveFamilyWire {
                family: SurfaceCurveFamilyKind::Parametric,
                context,
                tail: tail.map(|value| {
                    value
                        .tail
                        .into_wire(value.flags.flag, value.flags.second_flag)
                }),
            },
            Self::Skin { context, tail } => SurfaceCurveFamilyWire {
                family: SurfaceCurveFamilyKind::Skin,
                context,
                tail: tail.map(|value| value.tail.into_wire(value.flags, None)),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SurfaceCurveFamily {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = SurfaceCurveFamilyWire::deserialize(deserializer)?;
        let single_tail = |tail: Option<SurfaceCurveTailWire>| {
            tail.map(|wire| {
                if wire.second_flag.is_some() {
                    return Err(serde::de::Error::custom(
                        "second_flag is valid only for a parametric surface curve",
                    ));
                }
                let flag = wire.flag;
                Ok(SurfaceCurveCacheFirst {
                    tail: wire.into_tail(),
                    flags: flag,
                })
            })
            .transpose()
        };
        match wire.family {
            SurfaceCurveFamilyKind::Blend => Ok(Self::Blend {
                context: wire.context,
                tail: single_tail(wire.tail)?,
            }),
            SurfaceCurveFamilyKind::SurfaceConstrained => Ok(Self::SurfaceConstrained {
                context: wire.context,
                tail: single_tail(wire.tail)?,
            }),
            SurfaceCurveFamilyKind::Parametric => Ok(Self::Parametric {
                context: wire.context,
                tail: wire.tail.map(|tail| {
                    let flags = ParametricSurfaceCurveFlags {
                        flag: tail.flag,
                        second_flag: tail.second_flag,
                    };
                    SurfaceCurveCacheFirst {
                        tail: tail.into_tail(),
                        flags,
                    }
                }),
            }),
            SurfaceCurveFamilyKind::Skin => Ok(Self::Skin {
                context: wire.context,
                tail: single_tail(wire.tail)?,
            }),
        }
    }
}

/// Native silhouette construction family and its exclusive tail fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SilhouetteKind {
    /// Standard implicit silhouette.
    Standard,
    /// Parametric silhouette.
    Parametric,
    /// Draft/taper silhouette with an explicit factor.
    Taper {
        /// Native unscaled draft factor.
        draft_factor: f64,
    },
}

/// Discriminator-specific payload of a deformable native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeformableCurveData {
    /// Mode 8 vector field followed by ordered scalar pairs.
    VectorField {
        /// Four ordered native vectors.
        vectors: [Vector3; 4],
        /// Ordered pairs from the mode-8 scalar table.
        parameter_pairs: Vec<[f64; 2]>,
    },
    /// Mode 3 fixed deformation payload.
    Mode3 {
        /// Four vectors at the start of the payload.
        leading_vectors: [Vector3; 4],
        /// Scalar following the leading vectors.
        leading_parameter: f64,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position following the leading flags.
        trailing_point: Point3,
        /// Two vectors following the position.
        trailing_vectors: [Vector3; 2],
        /// Scalar following the trailing frame.
        frame_parameter: f64,
        /// Two flags following the frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalars following the frame flags.
        parameters: [f64; 3],
        /// Five flags following the ordered scalars.
        trailing_flags: [bool; 5],
        /// Final scalar before the trailing integer.
        trailing_parameter: f64,
        /// Integer closing the mode-3 payload.
        trailing_value: i64,
    },
}

/// Source slot of a deformable native intcurve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeformableCurveSource {
    /// Source geometry resolved to a neutral curve carrier.
    Curve {
        /// Curve being deformed.
        curve: CurveId,
    },
    /// Native intcurve reference whose target is absent from the active subtype table.
    NativeReference {
        /// Boolean stored before the reference scope.
        flag: bool,
        /// Integer stored by the native `ref` subtype.
        index: i64,
    },
}

/// Orientation carrier of a planar curve offset.
#[derive(Debug, Clone, PartialEq)]
pub enum OffsetSide {
    /// Unit plane normal defining the positive offset side.
    PlaneNormal(Vector3),
    /// Explicit offset direction, optionally constrained to a support surface.
    Direction {
        /// Nonzero offset direction.
        direction: Vector3,
        /// Support surface within which the offset is measured.
        support: Option<SurfaceId>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct OffsetSideWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direction: Option<Vector3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    support: Option<SurfaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    normal: Option<Vector3>,
}

impl Serialize for OffsetSide {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let wire = match self {
            Self::PlaneNormal(normal) => OffsetSideWire {
                direction: None,
                support: None,
                normal: Some(*normal),
            },
            Self::Direction { direction, support } => OffsetSideWire {
                direction: Some(*direction),
                support: support.clone(),
                normal: None,
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OffsetSide {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = OffsetSideWire::deserialize(deserializer)?;
        match (wire.direction, wire.support, wire.normal) {
            (Some(direction), support, None) => Ok(Self::Direction { direction, support }),
            (None, None, Some(normal)) => Ok(Self::PlaneNormal(normal)),
            _ => Err(serde::de::Error::custom(
                "offset direction and normal are exclusive, and support requires direction",
            )),
        }
    }
}

/// Parameter interval and optional variable-distance law of a curve offset.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveOffsetRange {
    /// Constant-distance offset over a retained source interval.
    Uniform {
        /// Parameter interval on the source curve.
        parameter_range: [f64; 2],
    },
    /// Variable-distance offset over the interval used by its law.
    Variable {
        /// Parameter interval on the source curve.
        parameter_range: [f64; 2],
        /// Variable signed-distance law.
        distance_law: CurveOffsetDistanceLaw,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CurveOffsetRangeWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_range: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    distance_law: Option<CurveOffsetDistanceLaw>,
}

mod curve_offset_range_wire {
    use super::{CurveOffsetRange, CurveOffsetRangeWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // Serde passes the borrowed field to this adapter.
    #[allow(clippy::ref_option)]
    pub fn serialize<S>(range: &Option<CurveOffsetRange>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match range {
            None => CurveOffsetRangeWire {
                parameter_range: None,
                distance_law: None,
            },
            Some(CurveOffsetRange::Uniform { parameter_range }) => CurveOffsetRangeWire {
                parameter_range: Some(*parameter_range),
                distance_law: None,
            },
            Some(CurveOffsetRange::Variable {
                parameter_range,
                distance_law,
            }) => CurveOffsetRangeWire {
                parameter_range: Some(*parameter_range),
                distance_law: Some(distance_law.clone()),
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<CurveOffsetRange>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CurveOffsetRangeWire::deserialize(deserializer)?;
        match (wire.parameter_range, wire.distance_law) {
            (None, None) => Ok(None),
            (Some(parameter_range), None) => {
                Ok(Some(CurveOffsetRange::Uniform { parameter_range }))
            }
            (Some(parameter_range), Some(distance_law)) => Ok(Some(CurveOffsetRange::Variable {
                parameter_range,
                distance_law,
            })),
            (None, Some(_)) => Err(serde::de::Error::custom(
                "offset distance_law requires parameter_range",
            )),
        }
    }
}

/// Neutral semantics for a procedural curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralCurveDefinition {
    /// An exact native intcurve whose solved NURBS cache is authoritative.
    Exact,
    /// Curve defined by recursive native law formulas.
    Law {
        /// Shared support surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Version-stamped serializer form, absent for the legacy layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<LawCurveVersionForm>,
        /// Native ASM extension integer.
        extension: i64,
        /// Primary recursive law formula.
        primary: LawFormula,
        /// Counted additional recursive law formulas.
        additional: Vec<LawFormula>,
    },
    /// Ordered compound of native child curves with construction parameters.
    Compound(CompoundCurveConstruction),
    /// Circular or conical helix around an axis.
    Helix(HelixCurveConstruction),
    /// Intersection of two support surfaces.
    Intersection {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
    },
    /// Tolerance-bounded intersection relation selected by topology endpoints.
    TolerantIntersection {
        /// Distinct supports and finite endpoint bounds.
        #[serde(flatten)]
        construction: TolerantIntersectionConstruction,
        /// Atomic neutral parameterization established by validated support charts.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameterization: Option<TolerantIntersectionParameterization>,
    },
    /// Intersection constrained by a third ordered support surface.
    ThreeSurfaceIntersection {
        /// Shared first two surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Native selector preceding the third support pair.
        selector: i64,
        /// Third `(surface, pcurve)` support pair.
        third: IntcurveSupportSide,
    },
    /// Surface-related curve whose native subtype has no tail beyond the shared prefix.
    SurfaceCurve {
        /// Native family, support context, and optional cache-first tail.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(with = "SurfaceCurveFamilyWire"))]
        family: SurfaceCurveFamily,
    },
    /// Silhouette of a cast surface in a light direction.
    Silhouette {
        /// Shared first two support pairs.
        context: IntcurveSupportContext,
        /// Standard, parametric, or taper silhouette semantics.
        silhouette: SilhouetteKind,
        /// Surface whose silhouette is constructed.
        cast_surface: SurfaceId,
        /// Native model-space light direction.
        light_direction: Vector3,
    },
    /// Curve offset relative to a surface parameterization.
    SurfaceOffset {
        /// Shared first two support pairs.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Native U interval on the base surface.
        base_u_range: [f64; 2],
        /// Native V interval on the base surface.
        base_v_range: [f64; 2],
        /// Embedded base curve.
        base: CurveId,
        /// Native interval on `base`.
        base_range: [f64; 2],
        /// Optional parameter endpoints following the embedded base curve in
        /// the cache-first layout.
        #[serde(default)]
        base_endpoints: [Option<f64>; 2],
        /// Cache-first shared-context fields; absent from the context-first
        /// layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_first: Option<CacheFirstCurveForm>,
        /// Signed model-space offset distance.
        distance: f64,
        /// Native unscaled parameter shift.
        shift: f64,
        /// Native unscaled parameter scale.
        scale: f64,
    },
    /// Blend spring guide between two support sides.
    Spring {
        /// Structurally selected context-first or cache-first representation.
        #[serde(flatten, with = "spring_layout_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "SpringLayoutWire"))]
        layout: SpringLayout,
        /// Native `CURV_DIR` enum value.
        direction: i64,
    },
    /// Deformation of an embedded source curve.
    Deformable {
        /// Shared cache-first support context.
        context: IntcurveSupportContext,
        /// Cache-first serializer fields surrounding the solved curve cache.
        cache_first: CacheFirstCurveForm,
        /// Curve being deformed or its unresolved native reference.
        source: DeformableCurveSource,
        /// Optional native bounds following the source curve.
        source_parameter_range: [Option<f64>; 2],
        /// Discriminator-specific deformation payload.
        data: DeformableCurveData,
    },
    /// Projection of a source curve onto a support surface.
    Projection {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Curve being projected.
        source: CurveId,
        /// Native post-source tail form.
        tail: ProjectionTail,
    },
    /// Offset from a source curve.
    Offset {
        /// Curve this curve is offset from.
        source: CurveId,
        /// Signed offset distance, in document length units.
        distance: f64,
        /// Exclusive plane-normal or explicit-direction carrier.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(with = "OffsetSideWire"))]
        side: OffsetSide,
        /// Retained parameter range, with its distance law when variable.
        #[serde(flatten, with = "curve_offset_range_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "CurveOffsetRangeWire"))]
        range: Option<CurveOffsetRange>,
    },
    /// Free-space 3D offset using a reference direction.
    SpatialOffset {
        /// Curve being offset.
        source: CurveId,
        /// Signed offset distance.
        distance: f64,
        /// Reference direction controlling the offset frame.
        reference_direction: Vector3,
        /// Whether the source classifies the result as self-intersecting.
        self_intersect: Option<bool>,
    },
    /// Intersection of two surfaces after applying independent signed offsets.
    TwoSidedOffset {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Signed offset distance for each support side, in document length units.
        offsets: [f64; 2],
    },
    /// Free-space vector offset of a source curve over a parameter interval.
    VectorOffset {
        /// Curve being offset.
        source: CurveId,
        /// Native parameter interval on the source curve.
        parameter_range: [f64; 2],
        /// Model-space offset vector.
        offset: Vector3,
        /// Integer codes attached to the fixed `source` and `offset` roles.
        #[serde(flatten, with = "vector_offset_roles_wire")]
        #[cfg_attr(feature = "schema", schemars(with = "VectorOffsetRolesWire"))]
        roles: VectorOffsetRoles,
    },
    /// A parameter sub-range of a parent curve.
    Subset {
        /// Parent curve being restricted.
        source: CurveId,
        /// Native parameter interval retained from the parent.
        parameter_range: [f64; 2],
        /// Whether the subset follows increasing parent parameters.
        #[serde(default = "default_true")]
        sense: bool,
    },
    /// Affine replica of a curve carrier, retaining the parent curve's
    /// parameter range and parameterization.
    Replica {
        /// Curve being replicated.
        source: CurveId,
        /// Affine map from the parent curve coordinates to this curve.
        transform: Transform,
    },
    /// Spine or center curve of a blend surface.
    BlendSpine {
        /// The blend surface this curve is the spine of, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blend_surface: Option<SurfaceId>,
    },
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Source construction-family discriminator, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_kind: Option<String>,
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
    },
}

/// Codes attached to the fixed native vector-offset role labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorOffsetRoles {
    /// Code following the `source` label.
    pub source_code: i64,
    /// Code following the `offset` label.
    pub offset_code: i64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct VectorOffsetRolesWire {
    labels: [String; 2],
    codes: [i64; 2],
}

mod vector_offset_roles_wire {
    use super::{VectorOffsetRoles, VectorOffsetRolesWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(roles: &VectorOffsetRoles, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        VectorOffsetRolesWire {
            labels: ["source".into(), "offset".into()],
            codes: [roles.source_code, roles.offset_code],
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<VectorOffsetRoles, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = VectorOffsetRolesWire::deserialize(deserializer)?;
        if wire.labels != ["source", "offset"] {
            return Err(serde::de::Error::custom(
                "vector-offset labels must be [\"source\", \"offset\"]",
            ));
        }
        Ok(VectorOffsetRoles {
            source_code: wire.codes[0],
            offset_code: wire.codes[1],
        })
    }
}

impl ProceduralCurveDefinition {
    fn revision_cache(&self) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::SurfaceCurve { family } => family.revision_cache(),
            Self::SurfaceOffset {
                cache_first: Some(form),
                ..
            } => Some(&form.cache),
            Self::Spring { layout, .. } => layout.cache_first().map(|form| &form.cache),
            Self::Deformable { cache_first, .. } => Some(&cache_first.cache),
            _ => None,
        }
    }

    fn revision_cache_mut(
        &mut self,
    ) -> Option<&mut RevisionCacheForm<CacheFirstCurveParameterization>> {
        match self {
            Self::SurfaceCurve { family } => family.revision_cache_mut(),
            Self::SurfaceOffset {
                cache_first: Some(form),
                ..
            } => Some(&mut form.cache),
            Self::Spring { layout, .. } => layout.cache_first_mut().map(|form| &mut form.cache),
            Self::Deformable { cache_first, .. } => Some(&mut cache_first.cache),
            _ => None,
        }
    }
}

impl ProceduralCurve {
    /// Build a procedural curve without a legacy top-level cache.
    #[must_use]
    pub fn new(id: ProceduralCurveId, definition: ProceduralCurveDefinition) -> Self {
        Self {
            id,
            definition,
            legacy_cache_fit_tolerance: None,
        }
    }

    /// Build a procedural curve and reconcile the legacy top-level cache
    /// field with any revision-gated cache form in its definition.
    pub fn try_new(
        id: ProceduralCurveId,
        definition: ProceduralCurveDefinition,
        cache_fit_tolerance: Option<f64>,
    ) -> Result<Self, CacheFitToleranceError> {
        let legacy_cache_fit_tolerance =
            reconcile_cache_fit_tolerance(definition.revision_cache(), cache_fit_tolerance)?;
        Ok(Self {
            id,
            definition,
            legacy_cache_fit_tolerance,
        })
    }

    /// Borrow the neutral construction definition.
    #[must_use]
    pub fn definition(&self) -> &ProceduralCurveDefinition {
        &self.definition
    }

    /// Replace the construction definition and discard a legacy cache value
    /// when the new definition owns a revision cache.
    pub fn replace_definition(&mut self, definition: ProceduralCurveDefinition) {
        if definition.revision_cache().is_some() {
            self.legacy_cache_fit_tolerance = None;
        }
        self.definition = definition;
    }

    /// Replace the definition and effective cache-fit tolerance atomically.
    pub fn try_replace_definition(
        &mut self,
        definition: ProceduralCurveDefinition,
        cache_fit_tolerance: Option<f64>,
    ) -> Result<(), CacheFitToleranceError> {
        let legacy_cache_fit_tolerance =
            reconcile_cache_fit_tolerance(definition.revision_cache(), cache_fit_tolerance)?;
        self.definition = definition;
        self.legacy_cache_fit_tolerance = legacy_cache_fit_tolerance;
        Ok(())
    }

    /// Edit the definition and normalize legacy cache storage before the edit
    /// can escape this call.
    pub fn edit_definition<R>(
        &mut self,
        edit: impl FnOnce(&mut ProceduralCurveDefinition) -> R,
    ) -> R {
        let result = edit(&mut self.definition);
        if self.definition.revision_cache().is_some() {
            self.legacy_cache_fit_tolerance = None;
        }
        result
    }

    /// Effective fit tolerance of the solved cache.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<f64> {
        self.definition.revision_cache().map_or(
            self.legacy_cache_fit_tolerance.map(FitTolerance::get),
            RevisionCacheForm::fit_tolerance,
        )
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved revision cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<f64>,
    ) -> Result<(), CacheFitToleranceError> {
        set_cache_fit_tolerance(
            self.definition.revision_cache_mut(),
            &mut self.legacy_cache_fit_tolerance,
            value.map(FitTolerance::try_new).transpose()?,
        )
    }

    /// Raise the fit tolerance of an existing solved cache. Parameterized
    /// forms have no solved cache and remain unchanged.
    pub fn raise_cache_fit_tolerance(&mut self, value: FitTolerance) {
        match self.definition.revision_cache_mut() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => {
                *fit_tolerance = FitTolerance(fit_tolerance.get().max(value.get()));
            }
            Some(RevisionCacheForm::Parameterization(_)) => {}
            None => {
                self.legacy_cache_fit_tolerance = Some(FitTolerance(
                    self.legacy_cache_fit_tolerance
                        .map_or(0.0, FitTolerance::get)
                        .max(value.get()),
                ));
            }
        }
    }

    /// Scale the effective cache-fit tolerance in place.
    pub fn scale_cache_fit_tolerance(&mut self, scale: f64) -> Result<(), CacheFitToleranceError> {
        if let Some(value) = self.cache_fit_tolerance() {
            self.set_cache_fit_tolerance(Some(value * scale))?;
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ProceduralSurfaceWriteWire<'a> {
    id: &'a ProceduralSurfaceId,
    definition: &'a ProceduralSurfaceDefinition,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_fit_tolerance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_bounds: Option<[Option<f64>; 4]>,
}

#[derive(Deserialize)]
pub(crate) struct ProceduralSurfaceReadWire {
    id: ProceduralSurfaceId,
    #[serde(default)]
    surface: Option<SurfaceId>,
    definition: serde_json::Value,
    #[serde(default)]
    cache_fit_tolerance: Option<f64>,
    #[serde(default)]
    record_bounds: Option<[Option<f64>; 4]>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the procedural-surface wire schema")]
struct ProceduralSurfaceSchemaWire {
    id: ProceduralSurfaceId,
    surface: SurfaceId,
    definition: ProceduralSurfaceDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_fit_tolerance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    record_bounds: Option<[Option<f64>; 4]>,
}

#[derive(Serialize)]
struct ProceduralCurveWriteWire<'a> {
    id: &'a ProceduralCurveId,
    definition: &'a ProceduralCurveDefinition,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_fit_tolerance: Option<f64>,
}

#[derive(Deserialize)]
pub(crate) struct ProceduralCurveReadWire {
    id: ProceduralCurveId,
    #[serde(default)]
    curve: Option<CurveId>,
    definition: serde_json::Value,
    #[serde(default)]
    cache_fit_tolerance: Option<f64>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the procedural-curve wire schema")]
struct ProceduralCurveSchemaWire {
    id: ProceduralCurveId,
    curve: CurveId,
    definition: ProceduralCurveDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_fit_tolerance: Option<f64>,
}

fn inject_revision_cache(
    value: &mut serde_json::Value,
    selector_field: &str,
    fit_tolerance: Option<f64>,
    stale_solved: bool,
) -> Result<bool, String> {
    fn visit(
        value: &mut serde_json::Value,
        selector_field: &str,
        fit_tolerance: Option<f64>,
        stale_solved: bool,
        found: &mut bool,
    ) -> Result<(), String> {
        match value {
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value, selector_field, fit_tolerance, stale_solved, found)?;
                }
            }
            serde_json::Value::Object(fields) => {
                if let Some(selector) = fields.get(selector_field).or_else(|| {
                    (selector_field == "tail_enum")
                        .then(|| fields.get("cache_selector"))
                        .flatten()
                }) {
                    if *found {
                        return Err(format!(
                            "definition contains more than one {selector_field} cache selector"
                        ));
                    }
                    *found = true;
                    let selector = selector
                        .as_i64()
                        .ok_or_else(|| format!("{selector_field} must be an integer"))?;
                    match (selector, fit_tolerance, stale_solved) {
                        (0, Some(fit_tolerance), false) => {
                            fields.insert(
                                "cache_fit_tolerance".into(),
                                serde_json::to_value(fit_tolerance)
                                    .map_err(|error| error.to_string())?,
                            );
                        }
                        (0, None, true) | (2, None, _) => {}
                        (0, None, false) => {
                            return Err(format!("{selector_field} 0 requires cache_fit_tolerance"))
                        }
                        (0, Some(_), true) => {
                            return Err(format!(
                            "stale variable-blend {selector_field} 0 forbids cache_fit_tolerance"
                        ))
                        }
                        (2, Some(_), _) => {
                            return Err(format!("{selector_field} 2 forbids cache_fit_tolerance"))
                        }
                        (selector, _, _) => {
                            return Err(format!("{selector_field} must be 0 or 2, got {selector}"))
                        }
                    }
                }
                for value in fields.values_mut() {
                    visit(value, selector_field, fit_tolerance, stale_solved, found)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    let mut found = false;
    visit(
        value,
        selector_field,
        fit_tolerance,
        stale_solved,
        &mut found,
    )?;
    Ok(found)
}

fn stale_variable_blend_cache(value: &serde_json::Value) -> bool {
    value.get("kind").and_then(serde_json::Value::as_str) == Some("variable_blend")
        && value
            .get("construction")
            .and_then(|construction| construction.get("shape_prefix"))
            .and_then(serde_json::Value::as_i64)
            == Some(0)
}

impl ProceduralSurfaceReadWire {
    pub(crate) fn into_parts(mut self) -> Result<(Option<SurfaceId>, ProceduralSurface), String> {
        let stale_solved = stale_variable_blend_cache(&self.definition);
        let revision = inject_revision_cache(
            &mut self.definition,
            "tail_enum",
            self.cache_fit_tolerance,
            stale_solved,
        )?;
        let definition =
            serde_json::from_value(self.definition).map_err(|error| error.to_string())?;
        let legacy_cache_fit_tolerance = (!revision).then_some(self.cache_fit_tolerance).flatten();
        let procedural = ProceduralSurface::try_new(
            self.id,
            definition,
            legacy_cache_fit_tolerance,
            self.record_bounds,
        )
        .map_err(|error| error.to_string())?;
        Ok((self.surface, procedural))
    }
}

impl ProceduralCurveReadWire {
    pub(crate) fn into_parts(mut self) -> Result<(Option<CurveId>, ProceduralCurve), String> {
        let revision = inject_revision_cache(
            &mut self.definition,
            "cache_enum",
            self.cache_fit_tolerance,
            false,
        )?;
        let definition =
            serde_json::from_value(self.definition).map_err(|error| error.to_string())?;
        let legacy_cache_fit_tolerance = (!revision).then_some(self.cache_fit_tolerance).flatten();
        let procedural = ProceduralCurve::try_new(self.id, definition, legacy_cache_fit_tolerance)
            .map_err(|error| error.to_string())?;
        Ok((self.curve, procedural))
    }
}

impl Serialize for ProceduralSurface {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ProceduralSurfaceWriteWire {
            id: &self.id,
            definition: &self.definition,
            cache_fit_tolerance: self.cache_fit_tolerance(),
            record_bounds: self.record_bounds,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProceduralSurface {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ProceduralSurfaceReadWire::deserialize(deserializer)?
            .into_parts()
            .map(|(_, procedural)| procedural)
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for ProceduralCurve {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ProceduralCurveWriteWire {
            id: &self.id,
            definition: &self.definition,
            cache_fit_tolerance: self.cache_fit_tolerance(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProceduralCurve {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ProceduralCurveReadWire::deserialize(deserializer)?
            .into_parts()
            .map(|(_, procedural)| procedural)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ProceduralSurface {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProceduralSurface".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ProceduralSurfaceSchemaWire::json_schema(generator)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for ProceduralCurve {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProceduralCurve".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ProceduralCurveSchemaWire::json_schema(generator)
    }
}

/// Independent variable used by a curve-offset distance law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CurveOffsetLawBasis {
    /// Distance measured along the source curve from the offset interval start.
    ArcLength,
    /// Native source-curve parameter.
    Parameter,
}

/// A one-based coordinate of a curve-offset distance function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "u8", into = "u8")]
pub struct CurveOffsetCoordinate(u8);

impl CurveOffsetCoordinate {
    /// Admit coordinate one, two, or three.
    pub fn try_new(coordinate: u8) -> Result<Self, &'static str> {
        match coordinate {
            1..=3 => Ok(Self(coordinate)),
            _ => Err("curve offset coordinate must be 1, 2, or 3"),
        }
    }

    /// One-based coordinate number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for CurveOffsetCoordinate {
    type Error = &'static str;
    fn try_from(coordinate: u8) -> Result<Self, Self::Error> {
        Self::try_new(coordinate)
    }
}

impl From<CurveOffsetCoordinate> for u8 {
    fn from(coordinate: CurveOffsetCoordinate) -> Self {
        coordinate.get()
    }
}

/// Variable signed distance law for a planar curve offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveOffsetDistanceLaw {
    /// Linear interpolation between two distance controls.
    Linear {
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Ordered signed distances in document length units.
        distances: [f64; 2],
        /// Ordered arc-length or neutral carrier-parameter controls.
        control_range: [f64; 2],
    },
    /// One coordinate of another curve defines the signed distance.
    Coordinate {
        /// Curve carrying the distance function.
        function: CurveId,
        /// One-based coordinate number on `function`.
        coordinate: CurveOffsetCoordinate,
        /// Independent-variable interpretation.
        basis: CurveOffsetLawBasis,
        /// Function parameter at zero source parameter or arc length.
        function_parameter_offset: f64,
        /// Function-parameter change per neutral source parameter or length unit.
        function_parameter_scale: f64,
    },
}

/// The shape of a parameter-space (u, v) curve on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PcurveGeometry {
    /// A straight line in parameter space.
    Line(LinePcurve),
    /// Polar angle and axial coordinate of a first-order harmonic spatial curve.
    PolarHarmonic(PolarHarmonicPcurve),
    /// Polar angle and axial coordinate obtained from a rational NURBS vector.
    PolarNurbs {
        /// Checked polar NURBS payload.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(flatten, with = "PolarPcurveNurbsWire"))]
        nurbs: PolarPcurveNurbs,
    },
    /// Great-circle locus in a sphere's azimuth/latitude parameter chart.
    SphericalGreatCircle(SphericalGreatCirclePcurve),
    /// Full circle in parameter space.
    Circle(CirclePcurve),
    /// Full ellipse in parameter space.
    Ellipse(EllipsePcurve),
    /// General first-order harmonic curve in parameter space.
    Harmonic(HarmonicPcurve),
    /// Parabola in parameter space.
    Parabola(ParabolaPcurve),
    /// Hyperbola in parameter space.
    Hyperbola(HyperbolaPcurve),
    /// General first-order hyperbolic curve in parameter space.
    Hyperbolic(HyperbolicPcurve),
    /// A free-form NURBS curve in parameter space (control points are (u, v)).
    Nurbs {
        /// Checked parameter-space NURBS payload.
        #[serde(flatten)]
        #[cfg_attr(feature = "schema", schemars(flatten))]
        nurbs: PcurveNurbs,
    },
    /// Affine replica of a parent pcurve in the same parameter space.
    Transformed {
        /// Exact parent pcurve and its parameterization.
        basis: Box<PcurveGeometry>,
        /// Two-dimensional affine map from parent coordinates to replica coordinates.
        transform: Transform2,
    },
    /// Parameter restriction of an exact basis pcurve.
    Trimmed(TrimmedPcurve),
    /// Signed planar offset of an exact basis pcurve.
    Offset(OffsetPcurve),
}

/// One paired radial and axial pole of a polar parameter-space NURBS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolarNurbsPole {
    /// Euclidean radial-plane pole.
    pub radial: Point2,
    /// Axial pole value.
    pub axial: f64,
}

/// Checked polar parameter-space NURBS payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PolarPcurveNurbs {
    degree: u32,
    knots: Vec<f64>,
    poles: Vec<PolarNurbsPole>,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PolarPcurveNurbsWire {
    degree: u32,
    knots: Vec<f64>,
    radial_control_points: Vec<Point2>,
    axial_control_points: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weights: Option<Vec<f64>>,
    #[serde(default)]
    periodic: bool,
}

impl PolarPcurveNurbs {
    /// Build a polar NURBS with paired radial and axial poles.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        radial_control_points: Vec<Point2>,
        axial_control_points: Vec<f64>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_length(
            "axial_control_points",
            axial_control_points.len(),
            radial_control_points.len(),
        )?;
        require_curve_cardinality(
            degree,
            knots.len(),
            radial_control_points.len(),
            weights.as_ref().map(Vec::len),
            "radial_control_points",
        )?;
        if degree == 0 {
            return Err(NurbsError("polar NURBS degree must be positive".into()));
        }
        require_finite_points_2("radial_control_points", &radial_control_points)?;
        require_finite_scalars("axial_control_points", &axial_control_points)?;
        require_nondecreasing_knots(&knots)?;
        if let Some(weights) = &weights {
            require_pcurve_weights(weights)?;
        }
        Ok(Self {
            degree,
            knots,
            poles: radial_control_points
                .into_iter()
                .zip(axial_control_points)
                .map(|(radial, axial)| PolarNurbsPole { radial, axial })
                .collect(),
            weights,
            periodic,
        })
    }

    /// Polynomial degree shared by every component.
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Expanded knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Atomically edit knot values and preserve their invariants.
    pub fn edit_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.knots.clone();
        edit(&mut values);
        require_nondecreasing_knots(&values)?;
        self.knots = values;
        Ok(())
    }

    /// Paired radial and axial poles.
    pub fn poles(&self) -> &[PolarNurbsPole] {
        &self.poles
    }

    /// Atomically edit paired poles and preserve finite coordinates.
    pub fn edit_poles(
        &mut self,
        edit: impl FnOnce(&mut [PolarNurbsPole]),
    ) -> Result<(), NurbsError> {
        let mut values = self.poles.clone();
        edit(&mut values);
        if values.iter().all(|pole| {
            pole.radial.u.is_finite() && pole.radial.v.is_finite() && pole.axial.is_finite()
        }) {
            self.poles = values;
            Ok(())
        } else {
            Err(NurbsError("poles contain a non-finite value".into()))
        }
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    /// Atomically edit rational weights and preserve their invariants.
    pub fn edit_weights(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let Some(weights) = &self.weights else {
            return Err(NurbsError("polar pcurve has no rational weights".into()));
        };
        let mut values = weights.clone();
        edit(&mut values);
        require_pcurve_weights(&values)?;
        self.weights = Some(values);
        Ok(())
    }

    /// Replace rational weights after checking pole cardinality.
    pub fn set_weights(&mut self, weights: Option<Vec<f64>>) -> Result<(), NurbsError> {
        if let Some(values) = &weights {
            require_length("weights", values.len(), self.poles.len())?;
            require_pcurve_weights(values)?;
        }
        self.weights = weights;
        Ok(())
    }

    /// Whether the NURBS parameterization is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Reverse paired poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.poles.reverse();
        if let Some(weights) = &mut self.weights {
            weights.reverse();
        }
        self.knots.reverse();
        for knot in &mut self.knots {
            *knot = -*knot;
        }
    }
}

impl Serialize for PolarPcurveNurbs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PolarPcurveNurbsWire {
            degree: self.degree,
            knots: self.knots.clone(),
            radial_control_points: self.poles.iter().map(|pole| pole.radial).collect(),
            axial_control_points: self.poles.iter().map(|pole| pole.axial).collect(),
            weights: self.weights.clone(),
            periodic: self.periodic,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PolarPcurveNurbs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PolarPcurveNurbsWire::deserialize(deserializer)?;
        Self::new(
            wire.degree,
            wire.knots,
            wire.radial_control_points,
            wire.axial_control_points,
            wire.weights,
            wire.periodic,
        )
        .map_err(serde::de::Error::custom)
    }
}

/// Checked two-dimensional NURBS payload for pcurves and sketch curves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveNurbs {
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<Point2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weights: Option<Vec<f64>>,
    #[serde(default)]
    periodic: bool,
}

impl PcurveNurbs {
    /// Build a parameter-space NURBS with consistent cardinalities.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point2>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(
            degree,
            knots.len(),
            control_points.len(),
            weights.as_ref().map(Vec::len),
            "control_points",
        )?;
        if degree == 0 {
            return Err(NurbsError("pcurve NURBS degree must be positive".into()));
        }
        require_finite_points_2("control_points", &control_points)?;
        require_nondecreasing_knots(&knots)?;
        if let Some(weights) = &weights {
            require_pcurve_weights(weights)?;
        }
        Ok(Self {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        })
    }

    /// Lift each two-dimensional pole into model space without changing knot or weight cardinalities.
    pub fn lift(&self, lift: impl FnMut(Point2) -> Point3) -> Result<NurbsCurve, NurbsError> {
        NurbsCurve::new(
            self.degree,
            self.knots.clone(),
            self.control_points.iter().copied().map(lift).collect(),
            self.weights.clone(),
            self.periodic,
        )
    }

    /// Curve degree.
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Full knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// Atomically edit knot values and preserve their invariants.
    pub fn edit_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.knots.clone();
        edit(&mut values);
        require_nondecreasing_knots(&values)?;
        self.knots = values;
        Ok(())
    }

    /// Control points in parameter order.
    pub fn control_points(&self) -> &[Point2] {
        &self.control_points
    }

    /// Atomically edit control points and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnOnce(&mut [Point2]),
    ) -> Result<(), NurbsError> {
        let mut values = self.control_points.clone();
        edit(&mut values);
        require_finite_points_2("control_points", &values)?;
        self.control_points = values;
        Ok(())
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    /// Atomically edit rational weights and preserve their invariants.
    pub fn edit_weights(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let Some(weights) = &self.weights else {
            return Err(NurbsError("pcurve has no rational weights".into()));
        };
        let mut values = weights.clone();
        edit(&mut values);
        require_pcurve_weights(&values)?;
        self.weights = Some(values);
        Ok(())
    }

    /// Replace rational weights after checking pole cardinality.
    pub fn set_weights(&mut self, weights: Option<Vec<f64>>) -> Result<(), NurbsError> {
        if let Some(values) = &weights {
            require_length("weights", values.len(), self.control_points.len())?;
            require_pcurve_weights(values)?;
        }
        self.weights = weights;
        Ok(())
    }

    /// Whether the parameter-space curve is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Reverse poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.control_points.reverse();
        if let Some(weights) = &mut self.weights {
            weights.reverse();
        }
        self.knots.reverse();
        for knot in &mut self.knots {
            *knot = -*knot;
        }
    }
}

impl<'de> Deserialize<'de> for PcurveNurbs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            degree: u32,
            knots: Vec<f64>,
            control_points: Vec<Point2>,
            #[serde(default)]
            weights: Option<Vec<f64>>,
            #[serde(default)]
            periodic: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.degree,
            wire.knots,
            wire.control_points,
            wire.weights,
            wire.periodic,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl PcurveGeometry {
    /// Scale chart coordinates atomically without changing the curve parameterization.
    pub fn try_scale_coordinates(&mut self, scales: [f64; 2]) -> Result<(), String> {
        let [u_scale, v_scale] = scales;
        let scale = |point: Point2| Point2::new(point.u * u_scale, point.v * v_scale);
        let isotropic = u_scale == v_scale;
        let scaled = match self {
            Self::Line(line) => Self::Line(LinePcurve::try_new(
                scale(line.origin),
                scale(line.direction),
            )?),
            Self::Circle(circle) if isotropic => Self::Circle(CirclePcurve::try_new(
                scale(circle.center),
                circle.x_axis,
                circle.y_axis,
                circle.radius * u_scale,
            )?),
            Self::Circle(circle) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(circle.center),
                scale(Point2::new(
                    circle.radius * circle.x_axis.u,
                    circle.radius * circle.x_axis.v,
                )),
                scale(Point2::new(
                    circle.radius * circle.y_axis.u,
                    circle.radius * circle.y_axis.v,
                )),
            )?),
            Self::Ellipse(ellipse) if isotropic => Self::Ellipse(EllipsePcurve::try_new(
                scale(ellipse.center),
                ellipse.x_axis,
                ellipse.y_axis,
                ellipse.major_radius * u_scale,
                ellipse.minor_radius * u_scale,
            )?),
            Self::Ellipse(ellipse) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(ellipse.center),
                scale(Point2::new(
                    ellipse.major_radius * ellipse.x_axis.u,
                    ellipse.major_radius * ellipse.x_axis.v,
                )),
                scale(Point2::new(
                    ellipse.minor_radius * ellipse.y_axis.u,
                    ellipse.minor_radius * ellipse.y_axis.v,
                )),
            )?),
            Self::Parabola(parabola) => {
                if !isotropic {
                    return Err("parabola coordinate scaling must be isotropic".into());
                }
                Self::Parabola(ParabolaPcurve::try_new(
                    scale(parabola.vertex),
                    parabola.x_axis,
                    parabola.y_axis,
                    parabola.focal_distance * u_scale,
                )?)
            }
            Self::Hyperbola(hyperbola) if isotropic => Self::Hyperbola(HyperbolaPcurve::try_new(
                scale(hyperbola.center),
                hyperbola.x_axis,
                hyperbola.y_axis,
                hyperbola.major_radius * u_scale,
                hyperbola.minor_radius * u_scale,
            )?),
            Self::Hyperbola(hyperbola) => Self::Hyperbolic(HyperbolicPcurve::try_new(
                scale(hyperbola.center),
                scale(Point2::new(
                    hyperbola.major_radius * hyperbola.x_axis.u,
                    hyperbola.major_radius * hyperbola.x_axis.v,
                )),
                scale(Point2::new(
                    hyperbola.minor_radius * hyperbola.y_axis.u,
                    hyperbola.minor_radius * hyperbola.y_axis.v,
                )),
            )?),
            Self::Harmonic(harmonic) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(harmonic.center),
                scale(harmonic.cosine),
                scale(harmonic.sine),
            )?),
            Self::Hyperbolic(hyperbolic) => Self::Hyperbolic(HyperbolicPcurve::try_new(
                scale(hyperbolic.center),
                scale(hyperbolic.cosine),
                scale(hyperbolic.sine),
            )?),
            Self::Nurbs { nurbs } => {
                let mut nurbs = nurbs.clone();
                nurbs
                    .edit_control_points(|points| {
                        for point in points {
                            *point = scale(*point);
                        }
                    })
                    .map_err(|error| error.to_string())?;
                Self::Nurbs { nurbs }
            }
            Self::Trimmed(trimmed) => {
                let mut basis = trimmed.basis.clone();
                basis.try_scale_coordinates(scales)?;
                Self::Trimmed(TrimmedPcurve::try_new(
                    trimmed.parameter_range,
                    trimmed.same_sense,
                    basis,
                )?)
            }
            Self::Offset(offset) => {
                if !isotropic {
                    return Err("offset coordinate scaling must be isotropic".into());
                }
                let mut basis = offset.basis.clone();
                basis.try_scale_coordinates(scales)?;
                Self::Offset(OffsetPcurve::try_new(offset.distance * u_scale, basis)?)
            }
            Self::Transformed { basis, transform } => {
                if !u_scale.is_finite() || !v_scale.is_finite() || u_scale == 0.0 || v_scale == 0.0
                {
                    return Err(
                        "transformed pcurve coordinate scales must be finite and nonzero".into(),
                    );
                }
                let mut rows = transform.rows();
                rows[0][1] *= u_scale / v_scale;
                rows[0][2] *= u_scale;
                rows[1][0] *= v_scale / u_scale;
                rows[1][2] *= v_scale;
                let transform =
                    Transform2::from_rows(rows).ok_or("scaled pcurve transform is invalid")?;
                let mut basis = basis.clone();
                basis.try_scale_coordinates(scales)?;
                Self::Transformed { basis, transform }
            }
            Self::PolarHarmonic(_) | Self::PolarNurbs { .. } | Self::SphericalGreatCircle(_) => {
                return if isotropic && u_scale == 1.0 {
                    Ok(())
                } else {
                    Err("polar pcurve coordinate scaling must be identity".into())
                };
            }
        };
        *self = scaled;
        Ok(())
    }

    /// Returns the origin and direction of a line-valued pcurve.
    ///
    /// Trimming and affine replicas preserve a line's parameterization. An
    /// offset does not preserve it because the offset is evaluated from the
    /// basis tangent, so it is deliberately excluded.
    pub fn line_parameters(&self) -> Option<(Point2, Point2)> {
        match self {
            Self::Line(line_pcurve) => {
                let (origin, direction) = line_pcurve.parts();
                Some((*origin, *direction))
            }
            Self::Transformed { basis, transform } => {
                let (origin, direction) = basis.line_parameters()?;
                Some((
                    transform.apply_point(origin),
                    transform.apply_vector(direction),
                ))
            }
            Self::Trimmed(trimmed_pcurve) => {
                let (_, _, basis) = trimmed_pcurve.parts();
                basis.line_parameters()
            }
            Self::PolarHarmonic(_) => None,
            Self::PolarNurbs { .. } => None,
            Self::SphericalGreatCircle(_) => None,
            Self::Circle(_) => None,
            Self::Ellipse(_) => None,
            Self::Harmonic(_) => None,
            Self::Parabola(_) => None,
            Self::Hyperbola(_) => None,
            Self::Hyperbolic(_) => None,
            Self::Nurbs { .. } => None,
            Self::Offset(_) => None,
        }
    }
}

/// A pcurve carrier: the 2D image of a coedge in its face's surface parameter
/// space. Referenced by a coedge; the owning surface establishes whether a
/// parameter dimension is a length (relevant to unit scaling, see [F3D spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#5-topology-records)).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Pcurve {
    /// Arena id.
    pub id: PcurveId,
    /// Parameter-space shape.
    pub geometry: PcurveGeometry,
    /// Source parameterization metadata.
    #[serde(flatten)]
    pub metadata: PcurveMetadata,
}

impl Pcurve {
    /// Native wrapper reversal, when the source stores one.
    pub fn wrapper_reversed(&self) -> Option<bool> {
        self.metadata.wrapper_reversed()
    }

    /// Four ASM booleans following an inline subtype scope.
    pub fn native_tail_flags(&self) -> Option<[bool; 4]> {
        self.metadata.native_tail_flags()
    }

    /// Directed native parameter interval on which this pcurve is evaluated.
    pub fn parameter_range(&self) -> Option<[f64; 2]> {
        self.metadata.parameter_range()
    }

    /// Parameter-space fit tolerance following a solved UV cache.
    pub fn fit_tolerance(&self) -> Option<f64> {
        self.metadata.fit_tolerance()
    }
}

/// Source-specific pcurve parameterization metadata.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PcurveMetadata {
    /// An ASM inline `exp_par_cur` record and its complete native tail.
    AsmInline(PcurveInlineForm),
    /// Metadata that does not assert the ASM inline-record contract.
    General(PcurveGeneralForm),
}

impl Default for PcurveMetadata {
    fn default() -> Self {
        Self::General(PcurveGeneralForm::default())
    }
}

impl PcurveMetadata {
    /// Admit metadata without an ASM inline-record claim.
    pub fn try_general(
        wrapper_reversed: Option<bool>,
        parameter_range: Option<[f64; 2]>,
        fit_tolerance: Option<f64>,
    ) -> Result<Self, &'static str> {
        PcurveGeneralForm::try_new(wrapper_reversed, parameter_range, fit_tolerance)
            .map(Self::General)
    }

    /// Native wrapper reversal, when the source stores one.
    pub fn wrapper_reversed(&self) -> Option<bool> {
        match self {
            Self::AsmInline(inline) => Some(inline.wrapper_reversed),
            Self::General(general) => general.wrapper_reversed,
        }
    }

    /// Four ASM booleans following an inline subtype scope.
    pub fn native_tail_flags(&self) -> Option<[bool; 4]> {
        match self {
            Self::AsmInline(inline) => Some(inline.native_tail_flags),
            Self::General(_) => None,
        }
    }

    /// Directed native parameter interval on which this pcurve is evaluated.
    pub fn parameter_range(&self) -> Option<[f64; 2]> {
        match self {
            Self::AsmInline(inline) => Some(inline.parameter_range),
            Self::General(general) => general.parameter_range,
        }
    }

    /// Parameter-space fit tolerance following a solved UV cache.
    pub fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::AsmInline(inline) => Some(inline.fit_tolerance),
            Self::General(general) => general.fit_tolerance,
        }
    }
}

impl<'de> Deserialize<'de> for PcurveMetadata {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn present_flags<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<[bool; 4]>, D::Error> {
            <[bool; 4]>::deserialize(deserializer).map(Some)
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(default)]
            wrapper_reversed: Option<bool>,
            #[serde(default, deserialize_with = "present_flags")]
            native_tail_flags: Option<[bool; 4]>,
            #[serde(default)]
            parameter_range: Option<[f64; 2]>,
            #[serde(default)]
            fit_tolerance: Option<f64>,
        }
        let wire = Wire::deserialize(deserializer)?;
        if let Some(flags) = wire.native_tail_flags {
            let wrapper = wire
                .wrapper_reversed
                .ok_or_else(|| serde::de::Error::missing_field("wrapper_reversed"))?;
            let range = wire
                .parameter_range
                .ok_or_else(|| serde::de::Error::missing_field("parameter_range"))?;
            let tolerance = wire
                .fit_tolerance
                .ok_or_else(|| serde::de::Error::missing_field("fit_tolerance"))?;
            PcurveInlineForm::try_new(wrapper, flags, range, tolerance)
                .map(Self::AsmInline)
                .map_err(serde::de::Error::custom)
        } else {
            Self::try_general(
                wire.wrapper_reversed,
                wire.parameter_range,
                wire.fit_tolerance,
            )
            .map_err(serde::de::Error::custom)
        }
    }
}

fn admit_pcurve_parameter_range(range: [f64; 2]) -> Result<[f64; 2], &'static str> {
    if range.iter().all(|value| value.is_finite()) {
        Ok(range)
    } else {
        Err("pcurve parameter_range endpoints must be finite")
    }
}

/// The fields carried together by an ASM inline `exp_par_cur` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "PcurveInlineFormWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveInlineForm {
    /// Parameterization wrapper reversal.
    pub wrapper_reversed: bool,
    /// Four native booleans following the inline subtype scope.
    pub native_tail_flags: [bool; 4],
    parameter_range: [f64; 2],
    /// Parameter-space fit tolerance following the solved UV cache.
    pub fit_tolerance: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PcurveInlineFormWire {
    wrapper_reversed: bool,
    native_tail_flags: [bool; 4],
    parameter_range: [f64; 2],
    fit_tolerance: f64,
}

impl TryFrom<PcurveInlineFormWire> for PcurveInlineForm {
    type Error = &'static str;
    fn try_from(wire: PcurveInlineFormWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.wrapper_reversed,
            wire.native_tail_flags,
            wire.parameter_range,
            wire.fit_tolerance,
        )
    }
}

impl PcurveInlineForm {
    /// Admit inline metadata with finite parameter endpoints.
    pub fn try_new(
        wrapper_reversed: bool,
        native_tail_flags: [bool; 4],
        parameter_range: [f64; 2],
        fit_tolerance: f64,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            wrapper_reversed,
            native_tail_flags,
            parameter_range: admit_pcurve_parameter_range(parameter_range)?,
            fit_tolerance,
        })
    }

    /// Directed native parameter interval.
    #[must_use]
    pub const fn parameter_range(&self) -> [f64; 2] {
        self.parameter_range
    }

    /// Replace the parameter range while preserving the previous range on rejection.
    pub fn set_parameter_range(&mut self, range: [f64; 2]) -> Result<(), &'static str> {
        self.parameter_range = admit_pcurve_parameter_range(range)?;
        Ok(())
    }
}

/// Pcurve metadata with no ASM inline-record contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "PcurveGeneralFormWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveGeneralForm {
    /// Source wrapper reversal, when stored independently of an ASM tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper_reversed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_range: Option<[f64; 2]>,
    /// Parameter-space fit tolerance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_tolerance: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PcurveGeneralFormWire {
    #[serde(default)]
    wrapper_reversed: Option<bool>,
    #[serde(default)]
    parameter_range: Option<[f64; 2]>,
    #[serde(default)]
    fit_tolerance: Option<f64>,
}

impl TryFrom<PcurveGeneralFormWire> for PcurveGeneralForm {
    type Error = &'static str;
    fn try_from(wire: PcurveGeneralFormWire) -> Result<Self, Self::Error> {
        Self::try_new(
            wire.wrapper_reversed,
            wire.parameter_range,
            wire.fit_tolerance,
        )
    }
}

impl PcurveGeneralForm {
    /// Admit general metadata with finite parameter endpoints.
    pub fn try_new(
        wrapper_reversed: Option<bool>,
        parameter_range: Option<[f64; 2]>,
        fit_tolerance: Option<f64>,
    ) -> Result<Self, &'static str> {
        Ok(Self {
            wrapper_reversed,
            parameter_range: parameter_range
                .map(admit_pcurve_parameter_range)
                .transpose()?,
            fit_tolerance,
        })
    }

    /// Directed native parameter interval.
    #[must_use]
    pub const fn parameter_range(&self) -> Option<[f64; 2]> {
        self.parameter_range
    }

    /// Replace the parameter range while preserving the previous range on rejection.
    pub fn set_parameter_range(&mut self, range: Option<[f64; 2]>) -> Result<(), &'static str> {
        self.parameter_range = range.map(admit_pcurve_parameter_range).transpose()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
