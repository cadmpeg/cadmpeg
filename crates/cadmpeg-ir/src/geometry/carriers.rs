// SPDX-License-Identifier: Apache-2.0
use super::{default_true, CacheFitToleranceError, FitTolerance};
use crate::features::FinitePoint3;
use crate::ids::PcurveId;
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform2;
use crate::units::{
    FinitePoint2, FiniteScalar, NonNegativeScalar, NonzeroPoint2, OrthonormalFrame3,
    PositiveScalar, UnitVector3,
};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// Plane with a finite origin and an orthonormal frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "PlaneSurfaceWire", into = "PlaneSurfaceWire")]
pub struct PlaneSurface {
    origin: FinitePoint3,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct PlaneSurfaceWire {
    origin: Point3,
    normal: Vector3,
    u_axis: Vector3,
}

impl PlaneSurface {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, normal: Vector3, u_axis: Vector3) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(normal, u_axis)
            .ok_or("PlaneSurface.normal/u_axis must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("PlaneSurface.origin must be finite")?;
        Ok(Self { origin, frame })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the normal.
    #[must_use]
    pub const fn normal(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the u axis.
    #[must_use]
    pub const fn u_axis(&self) -> &Vector3 {
        self.frame.reference()
    }
}

impl From<PlaneSurface> for PlaneSurfaceWire {
    fn from(value: PlaneSurface) -> Self {
        Self {
            origin: *value.origin(),
            normal: *value.normal(),
            u_axis: *value.u_axis(),
        }
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
#[serde(try_from = "CylinderSurfaceWire", into = "CylinderSurfaceWire")]
pub struct CylinderSurface {
    origin: FinitePoint3,
    radius: PositiveScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("CylinderSurface.axis/ref_direction must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("CylinderSurface.origin must be finite")?;
        let radius = PositiveScalar::new(radius)
            .ok_or("CylinderSurface.radius must be positive and finite")?;
        Ok(Self {
            origin,
            radius,
            frame,
        })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl From<CylinderSurface> for CylinderSurfaceWire {
    fn from(value: CylinderSurface) -> Self {
        Self {
            origin: *value.origin(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
        }
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
#[serde(try_from = "ConeSurfaceWire", into = "ConeSurfaceWire")]
pub struct ConeSurface {
    origin: FinitePoint3,
    radius: NonNegativeScalar,
    ratio: PositiveScalar,
    half_angle: FiniteScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("ConeSurface.axis/ref_direction must form an orthonormal frame")?;
        let origin = FinitePoint3::new(origin).ok_or("ConeSurface.origin must be finite")?;
        let radius = NonNegativeScalar::new(radius)
            .ok_or("ConeSurface.radius must be nonnegative and finite")?;
        let ratio =
            PositiveScalar::new(ratio).ok_or("ConeSurface.ratio must be positive and finite")?;
        let half_angle =
            FiniteScalar::new(half_angle).ok_or("ConeSurface.half_angle must be finite")?;
        Ok(Self {
            origin,
            radius,
            ratio,
            half_angle,
            frame,
        })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }

    /// Return the ratio.
    #[must_use]
    pub const fn ratio(&self) -> f64 {
        self.ratio.get()
    }

    /// Return the half angle.
    #[must_use]
    pub const fn half_angle(&self) -> f64 {
        self.half_angle.get()
    }
}

impl From<ConeSurface> for ConeSurfaceWire {
    fn from(value: ConeSurface) -> Self {
        Self {
            origin: *value.origin(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
            ratio: value.ratio(),
            half_angle: value.half_angle(),
        }
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
#[serde(try_from = "SphereSurfaceWire", into = "SphereSurfaceWire")]
pub struct SphereSurface {
    center: FinitePoint3,
    radius: FiniteScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("SphereSurface.axis/ref_direction must form an orthonormal frame")?;
        if radius == 0.0 {
            return Err("SphereSurface.radius must be nonzero");
        }
        let center = FinitePoint3::new(center).ok_or("SphereSurface.center must be finite")?;
        let radius = FiniteScalar::new(radius).ok_or("SphereSurface.radius must be finite")?;
        Ok(Self {
            center,
            radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl From<SphereSurface> for SphereSurfaceWire {
    fn from(value: SphereSurface) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
        }
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
#[serde(try_from = "TorusSurfaceWire", into = "TorusSurfaceWire")]
pub struct TorusSurface {
    center: FinitePoint3,
    major_radius: PositiveScalar,
    minor_radius: FiniteScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("TorusSurface.axis/ref_direction must form an orthonormal frame")?;
        if minor_radius == 0.0 {
            return Err("TorusSurface.minor_radius must be nonzero");
        }
        let center = FinitePoint3::new(center).ok_or("TorusSurface.center must be finite")?;
        let major_radius = PositiveScalar::new(major_radius)
            .ok_or("TorusSurface.major_radius must be positive and finite")?;
        let minor_radius =
            FiniteScalar::new(minor_radius).ok_or("TorusSurface.minor_radius must be finite")?;
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }
}

impl From<TorusSurface> for TorusSurfaceWire {
    fn from(value: TorusSurface) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            major_radius: value.major_radius(),
            minor_radius: value.minor_radius(),
        }
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
    origin: FinitePoint3,
    direction: UnitVector3,
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
        self.direction = self.direction.reversed();
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point3, direction: Vector3) -> Result<Self, &'static str> {
        let origin = FinitePoint3::new(origin).ok_or("LineCurve.origin must be finite")?;
        let direction =
            UnitVector3::new(direction).ok_or("LineCurve.direction must have unit length")?;
        Ok(Self { origin, direction })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point3 {
        self.origin.as_raw()
    }

    /// Return the direction.
    #[must_use]
    pub const fn direction(&self) -> &Vector3 {
        self.direction.as_raw()
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
#[serde(try_from = "CircleCurveWire", into = "CircleCurveWire")]
pub struct CircleCurve {
    center: FinitePoint3,
    radius: PositiveScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        self.frame.reverse_axis();
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        ref_direction: Vector3,
        radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, ref_direction)
            .ok_or("CircleCurve.axis/ref_direction must form an orthonormal frame")?;
        let center = FinitePoint3::new(center).ok_or("CircleCurve.center must be finite")?;
        let radius =
            PositiveScalar::new(radius).ok_or("CircleCurve.radius must be positive and finite")?;
        Ok(Self {
            center,
            radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the ref direction.
    #[must_use]
    pub const fn ref_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
    }
}

impl From<CircleCurve> for CircleCurveWire {
    fn from(value: CircleCurve) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            ref_direction: *value.ref_direction(),
            radius: value.radius(),
        }
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
#[serde(try_from = "EllipseCurveWire", into = "EllipseCurveWire")]
pub struct EllipseCurve {
    center: FinitePoint3,
    major_radius: PositiveScalar,
    minor_radius: PositiveScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        self.frame.reverse_axis();
    }

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(
        center: Point3,
        axis: Vector3,
        major_direction: Vector3,
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Self, &'static str> {
        let frame = OrthonormalFrame3::new(axis, major_direction)
            .ok_or("EllipseCurve.axis/major_direction must form an orthonormal frame")?;
        if major_radius < minor_radius {
            return Err("EllipseCurve.major_radius must be at least minor_radius");
        }
        let center = FinitePoint3::new(center).ok_or("EllipseCurve.center must be finite")?;
        let major_radius = PositiveScalar::new(major_radius)
            .ok_or("EllipseCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveScalar::new(minor_radius)
            .ok_or("EllipseCurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the major direction.
    #[must_use]
    pub const fn major_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }
}

impl From<EllipseCurve> for EllipseCurveWire {
    fn from(value: EllipseCurve) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            major_radius: value.major_radius(),
            minor_radius: value.minor_radius(),
        }
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
#[serde(try_from = "ParabolaCurveWire", into = "ParabolaCurveWire")]
pub struct ParabolaCurve {
    vertex: FinitePoint3,
    focal_distance: PositiveScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        let frame = OrthonormalFrame3::new(axis, major_direction)
            .ok_or("ParabolaCurve.axis/major_direction must form an orthonormal frame")?;
        let vertex = FinitePoint3::new(vertex).ok_or("ParabolaCurve.vertex must be finite")?;
        let focal_distance = PositiveScalar::new(focal_distance)
            .ok_or("ParabolaCurve.focal_distance must be positive and finite")?;
        Ok(Self {
            vertex,
            focal_distance,
            frame,
        })
    }

    /// Return the vertex.
    #[must_use]
    pub const fn vertex(&self) -> &Point3 {
        self.vertex.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the major direction.
    #[must_use]
    pub const fn major_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the focal distance.
    #[must_use]
    pub const fn focal_distance(&self) -> f64 {
        self.focal_distance.get()
    }
}

impl From<ParabolaCurve> for ParabolaCurveWire {
    fn from(value: ParabolaCurve) -> Self {
        Self {
            vertex: *value.vertex(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            focal_distance: value.focal_distance(),
        }
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
#[serde(try_from = "HyperbolaCurveWire", into = "HyperbolaCurveWire")]
pub struct HyperbolaCurve {
    center: FinitePoint3,
    major_radius: PositiveScalar,
    minor_radius: PositiveScalar,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
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
        self.frame.reverse_reference();
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
        let frame = OrthonormalFrame3::new(axis, major_direction)
            .ok_or("HyperbolaCurve.axis/major_direction must form an orthonormal frame")?;
        let center = FinitePoint3::new(center).ok_or("HyperbolaCurve.center must be finite")?;
        let major_radius = PositiveScalar::new(major_radius)
            .ok_or("HyperbolaCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveScalar::new(minor_radius)
            .ok_or("HyperbolaCurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            major_radius,
            minor_radius,
            frame,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point3 {
        self.center.as_raw()
    }

    /// Return the axis.
    #[must_use]
    pub const fn axis(&self) -> &Vector3 {
        self.frame.axis()
    }

    /// Return the major direction.
    #[must_use]
    pub const fn major_direction(&self) -> &Vector3 {
        self.frame.reference()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
    }
}

impl From<HyperbolaCurve> for HyperbolaCurveWire {
    fn from(value: HyperbolaCurve) -> Self {
        Self {
            center: *value.center(),
            axis: *value.axis(),
            major_direction: *value.major_direction(),
            major_radius: value.major_radius(),
            minor_radius: value.minor_radius(),
        }
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
    point: FinitePoint3,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct DegenerateCurveWire {
    point: Point3,
}

impl DegenerateCurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(point: Point3) -> Result<Self, &'static str> {
        let point = FinitePoint3::new(point).ok_or("DegenerateCurve.point must be finite")?;
        Ok(Self { point })
    }

    /// Return the point.
    #[must_use]
    pub const fn point(&self) -> &Point3 {
        self.point.as_raw()
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
    origin: FinitePoint2,
    direction: NonzeroPoint2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct LinePcurveWire {
    origin: Point2,
    direction: Point2,
}

impl LinePcurve {
    /// Unit-u line through the parameter-space origin.
    pub const U_AXIS: Self = Self {
        origin: FinitePoint2::ZERO,
        direction: NonzeroPoint2::U_AXIS,
    };

    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(origin: Point2, direction: Point2) -> Result<Self, &'static str> {
        let origin = FinitePoint2::new(origin).ok_or("LinePcurve.origin must be finite")?;
        let direction = NonzeroPoint2::new(direction)
            .ok_or("LinePcurve.direction must be finite with squared norm greater than epsilon")?;
        Ok(Self { origin, direction })
    }

    /// Return the origin.
    #[must_use]
    pub const fn origin(&self) -> &Point2 {
        self.origin.as_raw()
    }

    /// Return the direction.
    #[must_use]
    pub const fn direction(&self) -> &Point2 {
        self.direction.as_raw()
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
    radial_center: FinitePoint2,
    radial_cos: FinitePoint2,
    radial_sin: FinitePoint2,
    axial_origin: FiniteScalar,
    axial_cos: FiniteScalar,
    axial_sin: FiniteScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        if !(radial_cos.u.hypot(radial_cos.v) > 0.0 || radial_sin.u.hypot(radial_sin.v) > 0.0) {
            return Err("PolarHarmonicPcurve.radial_cos/radial_sin must not both be zero");
        }
        let radial_center = FinitePoint2::new(radial_center)
            .ok_or("PolarHarmonicPcurve.radial_center must be finite")?;
        let radial_cos =
            FinitePoint2::new(radial_cos).ok_or("PolarHarmonicPcurve.radial_cos must be finite")?;
        let radial_sin =
            FinitePoint2::new(radial_sin).ok_or("PolarHarmonicPcurve.radial_sin must be finite")?;
        let axial_origin = FiniteScalar::new(axial_origin)
            .ok_or("PolarHarmonicPcurve.axial_origin must be finite")?;
        let axial_cos =
            FiniteScalar::new(axial_cos).ok_or("PolarHarmonicPcurve.axial_cos must be finite")?;
        let axial_sin =
            FiniteScalar::new(axial_sin).ok_or("PolarHarmonicPcurve.axial_sin must be finite")?;
        Ok(Self {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        })
    }

    /// Return the radial center.
    #[must_use]
    pub const fn radial_center(&self) -> &Point2 {
        self.radial_center.as_raw()
    }

    /// Return the radial cos.
    #[must_use]
    pub const fn radial_cos(&self) -> &Point2 {
        self.radial_cos.as_raw()
    }

    /// Return the radial sin.
    #[must_use]
    pub const fn radial_sin(&self) -> &Point2 {
        self.radial_sin.as_raw()
    }

    /// Return the axial origin.
    #[must_use]
    pub const fn axial_origin(&self) -> f64 {
        self.axial_origin.get()
    }

    /// Return the axial cos.
    #[must_use]
    pub const fn axial_cos(&self) -> f64 {
        self.axial_cos.get()
    }

    /// Return the axial sin.
    #[must_use]
    pub const fn axial_sin(&self) -> f64 {
        self.axial_sin.get()
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
    azimuth_origin: FiniteScalar,
    azimuth_rate: FiniteScalar,
    plane_phase: FiniteScalar,
    plane_slope: FiniteScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        if azimuth_rate == 0.0 {
            return Err("SphericalGreatCirclePcurve.azimuth_rate must be nonzero");
        }
        let azimuth_origin = FiniteScalar::new(azimuth_origin)
            .ok_or("SphericalGreatCirclePcurve.azimuth_origin must be finite")?;
        let azimuth_rate = FiniteScalar::new(azimuth_rate)
            .ok_or("SphericalGreatCirclePcurve.azimuth_rate must be finite")?;
        let plane_phase = FiniteScalar::new(plane_phase)
            .ok_or("SphericalGreatCirclePcurve.plane_phase must be finite")?;
        let plane_slope = FiniteScalar::new(plane_slope)
            .ok_or("SphericalGreatCirclePcurve.plane_slope must be finite")?;
        Ok(Self {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        })
    }

    /// Return the azimuth origin.
    #[must_use]
    pub const fn azimuth_origin(&self) -> f64 {
        self.azimuth_origin.get()
    }

    /// Return the azimuth rate.
    #[must_use]
    pub const fn azimuth_rate(&self) -> f64 {
        self.azimuth_rate.get()
    }

    /// Return the plane phase.
    #[must_use]
    pub const fn plane_phase(&self) -> f64 {
        self.plane_phase.get()
    }

    /// Return the plane slope.
    #[must_use]
    pub const fn plane_slope(&self) -> f64 {
        self.plane_slope.get()
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
    center: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    radius: PositiveScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("CirclePcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("CirclePcurve.y_axis must be nonzero");
        }
        let center = FinitePoint2::new(center).ok_or("CirclePcurve.center must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("CirclePcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("CirclePcurve.y_axis must be finite")?;
        let radius =
            PositiveScalar::new(radius).ok_or("CirclePcurve.radius must be positive and finite")?;
        Ok(Self {
            center,
            x_axis,
            y_axis,
            radius,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius.get()
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
    center: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    major_radius: PositiveScalar,
    minor_radius: PositiveScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("EllipsePcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("EllipsePcurve.y_axis must be nonzero");
        }
        let center = FinitePoint2::new(center).ok_or("EllipsePcurve.center must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("EllipsePcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("EllipsePcurve.y_axis must be finite")?;
        let major_radius = PositiveScalar::new(major_radius)
            .ok_or("EllipsePcurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveScalar::new(minor_radius)
            .ok_or("EllipsePcurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
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
    center: FinitePoint2,
    cosine: FinitePoint2,
    sine: FinitePoint2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HarmonicPcurveWire {
    center: Point2,
    cosine: Point2,
    sine: Point2,
}

impl HarmonicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(center: Point2, cosine: Point2, sine: Point2) -> Result<Self, &'static str> {
        if !(cosine.u.hypot(cosine.v) > 0.0 || sine.u.hypot(sine.v) > 0.0) {
            return Err("HarmonicPcurve.cosine/sine must not both be zero");
        }
        let center = FinitePoint2::new(center).ok_or("HarmonicPcurve.center must be finite")?;
        let cosine = FinitePoint2::new(cosine).ok_or("HarmonicPcurve.cosine must be finite")?;
        let sine = FinitePoint2::new(sine).ok_or("HarmonicPcurve.sine must be finite")?;
        Ok(Self {
            center,
            cosine,
            sine,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the cosine.
    #[must_use]
    pub const fn cosine(&self) -> &Point2 {
        self.cosine.as_raw()
    }

    /// Return the sine.
    #[must_use]
    pub const fn sine(&self) -> &Point2 {
        self.sine.as_raw()
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
    vertex: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    focal_distance: PositiveScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("ParabolaPcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("ParabolaPcurve.y_axis must be nonzero");
        }
        let vertex = FinitePoint2::new(vertex).ok_or("ParabolaPcurve.vertex must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("ParabolaPcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("ParabolaPcurve.y_axis must be finite")?;
        let focal_distance = PositiveScalar::new(focal_distance)
            .ok_or("ParabolaPcurve.focal_distance must be positive and finite")?;
        Ok(Self {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        })
    }

    /// Return the vertex.
    #[must_use]
    pub const fn vertex(&self) -> &Point2 {
        self.vertex.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the focal distance.
    #[must_use]
    pub const fn focal_distance(&self) -> f64 {
        self.focal_distance.get()
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
    center: FinitePoint2,
    x_axis: FinitePoint2,
    y_axis: FinitePoint2,
    major_radius: PositiveScalar,
    minor_radius: PositiveScalar,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        if x_axis.u.hypot(x_axis.v) <= 0.0 {
            return Err("HyperbolaPcurve.x_axis must be nonzero");
        }
        if y_axis.u.hypot(y_axis.v) <= 0.0 {
            return Err("HyperbolaPcurve.y_axis must be nonzero");
        }
        let center = FinitePoint2::new(center).ok_or("HyperbolaPcurve.center must be finite")?;
        let x_axis = FinitePoint2::new(x_axis).ok_or("HyperbolaPcurve.x_axis must be finite")?;
        let y_axis = FinitePoint2::new(y_axis).ok_or("HyperbolaPcurve.y_axis must be finite")?;
        let major_radius = PositiveScalar::new(major_radius)
            .ok_or("HyperbolaPcurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveScalar::new(minor_radius)
            .ok_or("HyperbolaPcurve.minor_radius must be positive and finite")?;
        Ok(Self {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the x axis.
    #[must_use]
    pub const fn x_axis(&self) -> &Point2 {
        self.x_axis.as_raw()
    }

    /// Return the y axis.
    #[must_use]
    pub const fn y_axis(&self) -> &Point2 {
        self.y_axis.as_raw()
    }

    /// Return the major radius.
    #[must_use]
    pub const fn major_radius(&self) -> f64 {
        self.major_radius.get()
    }

    /// Return the minor radius.
    #[must_use]
    pub const fn minor_radius(&self) -> f64 {
        self.minor_radius.get()
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
    center: FinitePoint2,
    cosine: FinitePoint2,
    sine: FinitePoint2,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct HyperbolicPcurveWire {
    center: Point2,
    cosine: Point2,
    sine: Point2,
}

impl HyperbolicPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(center: Point2, cosine: Point2, sine: Point2) -> Result<Self, &'static str> {
        if !(cosine.u.hypot(cosine.v) > 0.0 || sine.u.hypot(sine.v) > 0.0) {
            return Err("HyperbolicPcurve.cosine/sine must not both be zero");
        }
        let center = FinitePoint2::new(center).ok_or("HyperbolicPcurve.center must be finite")?;
        let cosine = FinitePoint2::new(cosine).ok_or("HyperbolicPcurve.cosine must be finite")?;
        let sine = FinitePoint2::new(sine).ok_or("HyperbolicPcurve.sine must be finite")?;
        Ok(Self {
            center,
            cosine,
            sine,
        })
    }

    /// Return the center.
    #[must_use]
    pub const fn center(&self) -> &Point2 {
        self.center.as_raw()
    }

    /// Return the cosine.
    #[must_use]
    pub const fn cosine(&self) -> &Point2 {
        self.cosine.as_raw()
    }

    /// Return the sine.
    #[must_use]
    pub const fn sine(&self) -> &Point2 {
        self.sine.as_raw()
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
#[serde(deny_unknown_fields)]
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

    /// Return the parameter range.
    #[must_use]
    pub const fn parameter_range(&self) -> &[f64; 2] {
        &self.parameter_range
    }

    /// Return the same sense.
    #[must_use]
    pub const fn same_sense(&self) -> bool {
        self.same_sense
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &PcurveGeometry {
        &self.basis
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
    distance: FiniteScalar,
    basis: Box<PcurveGeometry>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct OffsetPcurveWire {
    distance: f64,
    basis: Box<PcurveGeometry>,
}

impl OffsetPcurve {
    /// Admit finite parameters that satisfy the carrier's numeric contract.
    pub fn try_new(distance: f64, basis: Box<PcurveGeometry>) -> Result<Self, &'static str> {
        let distance = FiniteScalar::new(distance).ok_or("OffsetPcurve.distance must be finite")?;
        Ok(Self { distance, basis })
    }

    /// Return the distance.
    #[must_use]
    pub const fn distance(&self) -> f64 {
        self.distance.get()
    }

    /// Return the basis.
    #[must_use]
    pub const fn basis(&self) -> &PcurveGeometry {
        &self.basis
    }
}

impl TryFrom<OffsetPcurveWire> for OffsetPcurve {
    type Error = &'static str;
    fn try_from(wire: OffsetPcurveWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.distance, wire.basis)
    }
}

/// The shape of a parameter-space (u, v) curve on a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
                scale(*line.origin()),
                scale(*line.direction()),
            )?),
            Self::Circle(circle) if isotropic => Self::Circle(CirclePcurve::try_new(
                scale(*circle.center()),
                *circle.x_axis(),
                *circle.y_axis(),
                circle.radius() * u_scale,
            )?),
            Self::Circle(circle) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(*circle.center()),
                scale(Point2::new(
                    circle.radius() * circle.x_axis().u,
                    circle.radius() * circle.x_axis().v,
                )),
                scale(Point2::new(
                    circle.radius() * circle.y_axis().u,
                    circle.radius() * circle.y_axis().v,
                )),
            )?),
            Self::Ellipse(ellipse) if isotropic => Self::Ellipse(EllipsePcurve::try_new(
                scale(*ellipse.center()),
                *ellipse.x_axis(),
                *ellipse.y_axis(),
                ellipse.major_radius() * u_scale,
                ellipse.minor_radius() * u_scale,
            )?),
            Self::Ellipse(ellipse) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(*ellipse.center()),
                scale(Point2::new(
                    ellipse.major_radius() * ellipse.x_axis().u,
                    ellipse.major_radius() * ellipse.x_axis().v,
                )),
                scale(Point2::new(
                    ellipse.minor_radius() * ellipse.y_axis().u,
                    ellipse.minor_radius() * ellipse.y_axis().v,
                )),
            )?),
            Self::Parabola(parabola) => {
                if !isotropic {
                    return Err("parabola coordinate scaling must be isotropic".into());
                }
                Self::Parabola(ParabolaPcurve::try_new(
                    scale(*parabola.vertex()),
                    *parabola.x_axis(),
                    *parabola.y_axis(),
                    parabola.focal_distance() * u_scale,
                )?)
            }
            Self::Hyperbola(hyperbola) if isotropic => Self::Hyperbola(HyperbolaPcurve::try_new(
                scale(*hyperbola.center()),
                *hyperbola.x_axis(),
                *hyperbola.y_axis(),
                hyperbola.major_radius() * u_scale,
                hyperbola.minor_radius() * u_scale,
            )?),
            Self::Hyperbola(hyperbola) => Self::Hyperbolic(HyperbolicPcurve::try_new(
                scale(*hyperbola.center()),
                scale(Point2::new(
                    hyperbola.major_radius() * hyperbola.x_axis().u,
                    hyperbola.major_radius() * hyperbola.x_axis().v,
                )),
                scale(Point2::new(
                    hyperbola.minor_radius() * hyperbola.y_axis().u,
                    hyperbola.minor_radius() * hyperbola.y_axis().v,
                )),
            )?),
            Self::Harmonic(harmonic) => Self::Harmonic(HarmonicPcurve::try_new(
                scale(*harmonic.center()),
                scale(*harmonic.cosine()),
                scale(*harmonic.sine()),
            )?),
            Self::Hyperbolic(hyperbolic) => Self::Hyperbolic(HyperbolicPcurve::try_new(
                scale(*hyperbolic.center()),
                scale(*hyperbolic.cosine()),
                scale(*hyperbolic.sine()),
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
                Self::Offset(OffsetPcurve::try_new(offset.distance() * u_scale, basis)?)
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
                let origin = line_pcurve.origin();
                let direction = line_pcurve.direction();
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
                let basis = trimmed_pcurve.basis();
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
#[serde(deny_unknown_fields)]
pub struct Pcurve {
    /// Arena id.
    pub id: PcurveId,
    /// Parameter-space shape.
    pub geometry: PcurveGeometry,
    /// Source parameterization metadata.
    #[serde(default)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum PcurveMetadata {
    /// An ASM inline `exp_par_cur` record and its complete native tail.
    AsmInline {
        /// Complete inline record fields.
        form: PcurveInlineForm,
    },
    /// Metadata that does not assert the ASM inline-record contract.
    General {
        /// Optional fields carried without an inline-record claim.
        #[serde(default)]
        form: PcurveGeneralForm,
    },
}

impl Default for PcurveMetadata {
    fn default() -> Self {
        Self::General {
            form: PcurveGeneralForm::default(),
        }
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
            .map(|form| Self::General { form })
    }

    /// Native wrapper reversal, when the source stores one.
    pub fn wrapper_reversed(&self) -> Option<bool> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.wrapper_reversed),
            Self::General { form: general } => general.wrapper_reversed,
        }
    }

    /// Four ASM booleans following an inline subtype scope.
    pub fn native_tail_flags(&self) -> Option<[bool; 4]> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.native_tail_flags),
            Self::General { .. } => None,
        }
    }

    /// Directed native parameter interval on which this pcurve is evaluated.
    pub fn parameter_range(&self) -> Option<[f64; 2]> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.parameter_range),
            Self::General { form: general } => general.parameter_range,
        }
    }

    /// Parameter-space fit tolerance following a solved UV cache.
    pub fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::AsmInline { form: inline } => Some(inline.fit_tolerance()),
            Self::General { form: general } => general.fit_tolerance(),
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
    fit_tolerance: FitTolerance,
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
    /// Admit inline metadata with finite parameter endpoints and a finite non-negative tolerance.
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
            fit_tolerance: FitTolerance::try_new(fit_tolerance)
                .map_err(|_| "pcurve fit_tolerance must be finite and non-negative")?,
        })
    }

    /// Parameter-space fit tolerance.
    #[must_use]
    pub const fn fit_tolerance(&self) -> f64 {
        self.fit_tolerance.get()
    }

    /// Replace the fit tolerance while retaining its previous value on rejection.
    pub fn set_fit_tolerance(&mut self, value: f64) -> Result<(), CacheFitToleranceError> {
        self.fit_tolerance = FitTolerance::try_new(value)?;
        Ok(())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fit_tolerance: Option<FitTolerance>,
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
    /// Admit general metadata with finite parameter endpoints and a finite non-negative tolerance.
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
            fit_tolerance: fit_tolerance
                .map(FitTolerance::try_new)
                .transpose()
                .map_err(|_| "pcurve fit_tolerance must be finite and non-negative")?,
        })
    }

    /// Parameter-space fit tolerance.
    #[must_use]
    pub fn fit_tolerance(&self) -> Option<f64> {
        self.fit_tolerance.map(FitTolerance::get)
    }

    /// Replace the fit tolerance while retaining its previous value on rejection.
    pub fn set_fit_tolerance(&mut self, value: Option<f64>) -> Result<(), CacheFitToleranceError> {
        self.fit_tolerance = value.map(FitTolerance::try_new).transpose()?;
        Ok(())
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
