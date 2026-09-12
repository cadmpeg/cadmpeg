// SPDX-License-Identifier: Apache-2.0
use super::{default_true, CacheContractError, FitTolerance};
use crate::features::FinitePoint3;
use crate::ids::PcurveId;
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform2;
use crate::scalar::{FiniteReal, NonNegativeReal, NonZeroReal, PositiveReal};
use crate::units::{FinitePoint2, NonzeroPoint2, OrthonormalFrame3, UnitVector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One rational pole in model space: its position and its weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeightedPole3 {
    /// Pole position in model space.
    pub point: Point3,
    /// Rational weight at this pole.
    pub weight: NonZeroReal,
}

/// The poles of a NURBS curve, stating the curve's rational form.
///
/// A rational pole carries its weight in its own row, so a weight list that
/// does not cover the poles has no spelling, and "the curve is polynomial" has
/// exactly one spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum NurbsPoles3 {
    /// A polynomial curve: its poles carry no weight.
    Polynomial {
        /// Poles in parameter order.
        points: Vec<Point3>,
    },
    /// A rational curve: every pole carries its weight.
    Rational {
        /// Pole rows in parameter order.
        points: Vec<WeightedPole3>,
    },
}

impl NurbsPoles3 {
    /// Pair a source's pole lane with its weight lane.
    ///
    /// A source that states poles and weights as two arrays pairs them here,
    /// once, at the decode boundary: the result is absent when the weight lane
    /// does not cover the poles or carries a zero or non-finite weight.
    #[must_use]
    pub fn from_lanes(points: Vec<Point3>, weights: Option<Vec<f64>>) -> Option<Self> {
        let Some(weights) = weights else {
            return Some(Self::Polynomial { points });
        };
        if weights.len() != points.len() {
            return None;
        }
        Some(Self::Rational {
            points: points
                .into_iter()
                .zip(weights)
                .map(|(point, weight)| {
                    Some(WeightedPole3 {
                        point,
                        weight: NonZeroReal::new(weight)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        })
    }

    /// Number of poles.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Polynomial { points } => points.len(),
            Self::Rational { points } => points.len(),
        }
    }

    /// True when the curve states no pole.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pole positions in parameter order.
    #[must_use]
    pub fn points(&self) -> Vec<Point3> {
        match self {
            Self::Polynomial { points } => points.clone(),
            Self::Rational { points } => points.iter().map(|pole| pole.point).collect(),
        }
    }

    /// Rational weights in pole order, absent on a polynomial curve.
    #[must_use]
    pub fn weights(&self) -> Option<Vec<f64>> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { points } => {
                Some(points.iter().map(|pole| pole.weight.get()).collect())
            }
        }
    }

    /// Reverse the pole order.
    pub fn reverse(&mut self) {
        match self {
            Self::Polynomial { points } => points.reverse(),
            Self::Rational { points } => points.reverse(),
        }
    }

    /// Edit every pole position in place.
    pub fn edit_points(&mut self, mut edit: impl FnMut(&mut Point3)) {
        match self {
            Self::Polynomial { points } => points.iter_mut().for_each(&mut edit),
            Self::Rational { points } => {
                points.iter_mut().for_each(|pole| edit(&mut pole.point));
            }
        }
    }

    /// Replace the weights, keeping the pole positions.
    ///
    /// Absent when the weight lane does not cover the poles or carries a zero
    /// or non-finite weight.
    #[must_use]
    pub fn with_weights(&self, weights: Option<Vec<f64>>) -> Option<Self> {
        Self::from_lanes(self.points(), weights)
    }
}

/// The control grid of a NURBS surface, stating the surface's rational form.
///
/// A rational pole carries its weight in its own row, so a weight grid that
/// does not cover the pole grid has no spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum NurbsPoleGrid {
    /// A polynomial surface: its poles carry no weight.
    Polynomial {
        /// Control grid rows: `rows[i][j]` is pole `(i, j)`.
        rows: Vec<Vec<Point3>>,
    },
    /// A rational surface: every pole carries its weight.
    Rational {
        /// Control grid rows: `rows[i][j]` is pole `(i, j)`.
        rows: Vec<Vec<WeightedPole3>>,
    },
}

impl NurbsPoleGrid {
    /// Pair a source's pole grid with its weight grid.
    ///
    /// The result is absent when the weight grid does not cover the pole grid
    /// or carries a zero or non-finite weight.
    #[must_use]
    pub fn from_lanes(rows: Vec<Vec<Point3>>, weights: Option<Vec<Vec<f64>>>) -> Option<Self> {
        let Some(weights) = weights else {
            return Some(Self::Polynomial { rows });
        };
        if weights.len() != rows.len() {
            return None;
        }
        let paired = rows
            .into_iter()
            .zip(weights)
            .map(|(row, weight_row)| {
                if weight_row.len() != row.len() {
                    return None;
                }
                row.into_iter()
                    .zip(weight_row)
                    .map(|(point, weight)| {
                        Some(WeightedPole3 {
                            point,
                            weight: NonZeroReal::new(weight)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self::Rational { rows: paired })
    }

    /// Number of grid rows, the pole count along u.
    #[must_use]
    pub fn u_count(&self) -> usize {
        match self {
            Self::Polynomial { rows } => rows.len(),
            Self::Rational { rows } => rows.len(),
        }
    }

    /// Length of the first grid row, the pole count along v.
    #[must_use]
    pub fn v_count(&self) -> usize {
        match self {
            Self::Polynomial { rows } => rows.first().map_or(0, Vec::len),
            Self::Rational { rows } => rows.first().map_or(0, Vec::len),
        }
    }

    /// Pole positions as grid rows.
    #[must_use]
    pub fn points(&self) -> Vec<Vec<Point3>> {
        match self {
            Self::Polynomial { rows } => rows.clone(),
            Self::Rational { rows } => rows
                .iter()
                .map(|row| row.iter().map(|pole| pole.point).collect())
                .collect(),
        }
    }

    /// Rational weight rows, absent on a polynomial surface.
    #[must_use]
    pub fn weights(&self) -> Option<Vec<Vec<f64>>> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { rows } => Some(
                rows.iter()
                    .map(|row| row.iter().map(|pole| pole.weight.get()).collect())
                    .collect(),
            ),
        }
    }

    /// Exchange the outer and inner grid index.
    pub fn transpose(&mut self) {
        match self {
            Self::Polynomial { rows } => *rows = transpose_rows(rows),
            Self::Rational { rows } => *rows = transpose_rows(rows),
        }
    }

    /// Edit every pole position in place.
    pub fn edit_points(&mut self, mut edit: impl FnMut(&mut Point3)) {
        match self {
            Self::Polynomial { rows } => rows
                .iter_mut()
                .for_each(|row| row.iter_mut().for_each(&mut edit)),
            Self::Rational { rows } => rows.iter_mut().for_each(|row| {
                row.iter_mut().for_each(|pole| edit(&mut pole.point));
            }),
        }
    }
}

/// One rational pole in parameter space: its position and its weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeightedPole2 {
    /// Pole position in parameter space.
    pub point: Point2,
    /// Rational weight at this pole.
    pub weight: PositiveReal,
}

/// The poles of a parameter-space NURBS curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PcurveNurbsPoles {
    /// A polynomial pcurve: its poles carry no weight.
    Polynomial {
        /// Poles in parameter order.
        points: Vec<Point2>,
    },
    /// A rational pcurve: every pole carries its weight.
    Rational {
        /// Pole rows in parameter order.
        points: Vec<WeightedPole2>,
    },
}

impl PcurveNurbsPoles {
    /// Pair a source's pole lane with its weight lane.
    ///
    /// The result is absent when the weight lane does not cover the poles or
    /// carries a non-positive or non-finite weight.
    #[must_use]
    pub fn from_lanes(points: Vec<Point2>, weights: Option<Vec<f64>>) -> Option<Self> {
        let Some(weights) = weights else {
            return Some(Self::Polynomial { points });
        };
        if weights.len() != points.len() {
            return None;
        }
        Some(Self::Rational {
            points: points
                .into_iter()
                .zip(weights)
                .map(|(point, weight)| {
                    Some(WeightedPole2 {
                        point,
                        weight: PositiveReal::new(weight)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        })
    }

    /// Number of poles.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Polynomial { points } => points.len(),
            Self::Rational { points } => points.len(),
        }
    }

    /// True when the pcurve states no pole.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Pole positions in parameter order.
    #[must_use]
    pub fn points(&self) -> Vec<Point2> {
        match self {
            Self::Polynomial { points } => points.clone(),
            Self::Rational { points } => points.iter().map(|pole| pole.point).collect(),
        }
    }

    /// Rational weights in pole order, absent on a polynomial pcurve.
    #[must_use]
    pub fn weights(&self) -> Option<Vec<f64>> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { points } => {
                Some(points.iter().map(|pole| pole.weight.get()).collect())
            }
        }
    }

    /// Reverse the pole order.
    pub fn reverse(&mut self) {
        match self {
            Self::Polynomial { points } => points.reverse(),
            Self::Rational { points } => points.reverse(),
        }
    }

    /// Edit every pole position in place.
    pub fn edit_points(&mut self, mut edit: impl FnMut(&mut Point2)) {
        match self {
            Self::Polynomial { points } => points.iter_mut().for_each(&mut edit),
            Self::Rational { points } => {
                points.iter_mut().for_each(|pole| edit(&mut pole.point));
            }
        }
    }
}

/// A tensor-product NURBS surface.
///
/// The control grid is stored as rows: the outer index is u, the inner index
/// is v, so the grid states both pole counts. `weights == None` denotes a
/// non-rational surface.
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
    /// Control grid rows, with the surface's rational form.
    poles: NurbsPoleGrid,
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
        require_rectangular_grid("control_points", &control_points)?;
        for row in &control_points {
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
        require_rectangular_grid("control_points", &points)?;
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
        #[serde(deny_unknown_fields)]
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

/// Exchange the outer and inner index of a rectangular row grid.
fn transpose_rows<T: Clone>(rows: &[Vec<T>]) -> Vec<Vec<T>> {
    let inner = rows.first().map_or(0, Vec::len);
    (0..inner)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column).cloned())
                .collect()
        })
        .collect()
}

fn checked_knot_count(field: &str, pole_count: usize, degree: u32) -> Result<usize, NurbsError> {
    pole_count
        .checked_add(degree as usize)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| NurbsError(format!("{field} knot count overflows usize")))
}

/// Every row of a control grid states the same pole count.
///
/// The grid is one object, so its rectangularity is one shape mint over that
/// object, not a comparison between two independently stated lists.
fn require_rectangular_grid(field: &str, rows: &[Vec<Point3>]) -> Result<(), NurbsError> {
    let width = rows.first().map_or(0, Vec::len);
    for row in rows {
        require_length(&format!("{field} row"), row.len(), width)?;
    }
    Ok(())
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

fn require_curve_cardinality(
    degree: u32,
    knot_count: usize,
    pole_count: usize,
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
    )
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
        poles: NurbsPoleGrid,
        normal_reversed: bool,
        u_periodic: bool,
        v_periodic: bool,
    ) -> Result<Self, NurbsError> {
        let control_points = poles.points();
        let u_count = poles.u_count();
        let v_count = poles.v_count();
        if u_count <= u_degree as usize {
            return Err(NurbsError(format!(
                "u_count must exceed u_degree {u_degree}, found {u_count}"
            )));
        }
        if v_count <= v_degree as usize {
            return Err(NurbsError(format!(
                "v_count must exceed v_degree {v_degree}, found {v_count}"
            )));
        }
        require_length(
            "u_knots",
            u_knots.len(),
            checked_knot_count("u", u_count, u_degree)?,
        )?;
        require_length(
            "v_knots",
            v_knots.len(),
            checked_knot_count("v", v_count, v_degree)?,
        )?;
        require_rectangular_grid("control_points", &control_points)?;
        for row in &control_points {
            require_finite_points_3("control_points", row)?;
        }
        require_nondecreasing_knots(&u_knots).map_err(|error| NurbsError(format!("u_{error}")))?;
        require_nondecreasing_knots(&v_knots).map_err(|error| NurbsError(format!("v_{error}")))?;
        Ok(Self {
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            poles,
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

    /// Number of control points along u, the number of grid rows.
    pub fn u_count(&self) -> u32 {
        self.poles.u_count() as u32
    }

    /// Number of control points along v, the length of every grid row.
    pub fn v_count(&self) -> u32 {
        self.poles.v_count() as u32
    }

    /// Build a NURBS surface from a source's pole grid and weight grid.
    ///
    /// A source that states poles and weights as two grids pairs them here,
    /// once, at the decode boundary; the surface itself carries pole rows.
    // Both parameter axes and their shared control grid form one NURBS invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn from_lanes(
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        control_points: Vec<Vec<Point3>>,
        weights: Option<Vec<Vec<f64>>>,
        normal_reversed: bool,
        u_periodic: bool,
        v_periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = NurbsPoleGrid::from_lanes(control_points, weights).ok_or_else(|| {
            NurbsError("3D NURBS weights must cover the pole grid and be finite and non-zero".into())
        })?;
        Self::new(
            u_degree,
            v_degree,
            u_knots,
            v_knots,
            poles,
            normal_reversed,
            u_periodic,
            v_periodic,
        )
    }

    /// Control grid rows, with the surface's rational form.
    pub const fn pole_grid(&self) -> &NurbsPoleGrid {
        &self.poles
    }

    /// Control-point rows, outer index u and inner index v.
    pub fn control_grid(&self) -> Vec<Vec<Point3>> {
        self.poles.points()
    }

    /// Control points in u-major order.
    pub fn poles(&self) -> Vec<Point3> {
        self.poles.points().into_iter().flatten().collect()
    }

    /// Pole at grid position `(u, v)`.
    pub fn pole(&self, u: usize, v: usize) -> Option<Point3> {
        self.poles.points().get(u)?.get(v).copied()
    }

    /// Rational weight at grid position `(u, v)`, absent when non-rational.
    pub fn weight(&self, u: usize, v: usize) -> Option<f64> {
        self.poles.weights()?.get(u)?.get(v).copied()
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point3),
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.edit_points(edit);
        for row in &poles.points() {
            require_finite_points_3("control_points", row)?;
        }
        self.poles = poles;
        Ok(())
    }

    /// Rational weight rows in control-grid order.
    pub fn weights(&self) -> Option<Vec<Vec<f64>>> {
        self.poles.weights()
    }

    /// Rational weights in control-point order.
    pub fn pole_weights(&self) -> Option<Vec<f64>> {
        Some(self.poles.weights()?.into_iter().flatten().collect())
    }

    /// Replace the control grid, keeping the knot cardinalities.
    pub fn set_poles(&mut self, poles: NurbsPoleGrid) -> Result<(), NurbsError> {
        *self = Self::new(
            self.u_degree,
            self.v_degree,
            self.u_knots.clone(),
            self.v_knots.clone(),
            poles,
            self.normal_reversed,
            self.u_periodic,
            self.v_periodic,
        )?;
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
        self.poles.transpose();
        std::mem::swap(&mut self.u_degree, &mut self.v_degree);
        std::mem::swap(&mut self.u_knots, &mut self.v_knots);
        std::mem::swap(&mut self.u_periodic, &mut self.v_periodic);
    }
}

impl<'de> Deserialize<'de> for NurbsSurface {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            u_degree: u32,
            v_degree: u32,
            u_knots: Vec<f64>,
            v_knots: Vec<f64>,
            poles: NurbsPoleGrid,
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
            wire.poles,
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
#[serde(deny_unknown_fields)]
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
    /// Poles in parameter order, with the curve's rational form.
    poles: NurbsPoles3,
    /// Whether the curve is periodic.
    periodic: bool,
}

impl NurbsCurve {
    /// Build a NURBS curve with consistent knot, pole, and weight cardinalities.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        poles: NurbsPoles3,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(degree, knots.len(), poles.len(), "control_points")?;
        require_finite_points_3("control_points", &poles.points())?;
        require_nondecreasing_knots(&knots)?;
        Ok(Self {
            degree,
            knots,
            poles,
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

    /// Build a NURBS curve from a source's pole lane and weight lane.
    ///
    /// A source that states poles and weights as two arrays pairs them here,
    /// once, at the decode boundary; the curve itself carries pole rows.
    pub fn from_lanes(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point3>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = NurbsPoles3::from_lanes(control_points, weights).ok_or_else(|| {
            NurbsError("3D NURBS weights must cover the poles and be finite and non-zero".into())
        })?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Poles in parameter order, with the curve's rational form.
    pub const fn pole_rows(&self) -> &NurbsPoles3 {
        &self.poles
    }

    /// Control points in parameter order.
    pub fn control_points(&self) -> Vec<Point3> {
        self.poles.points()
    }

    /// Number of poles.
    pub fn pole_count(&self) -> usize {
        self.poles.len()
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point3),
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.edit_points(edit);
        require_finite_points_3("control_points", &poles.points())?;
        self.poles = poles;
        Ok(())
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<Vec<f64>> {
        self.poles.weights()
    }

    /// Replace the poles, keeping the knot cardinality.
    pub fn set_poles(&mut self, poles: NurbsPoles3) -> Result<(), NurbsError> {
        *self = Self::new(self.degree, self.knots.clone(), poles, self.periodic)?;
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
        self.poles.reverse();
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
        #[serde(deny_unknown_fields)]
        struct Wire {
            degree: u32,
            knots: Vec<f64>,
            poles: NurbsPoles3,
            periodic: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.degree, wire.knots, wire.poles, wire.periodic)
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
        #[serde(deny_unknown_fields)]
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

/// One polyline sample with the source parameter recorded at it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PolylineVertex {
    /// Source parameter at this sample.
    pub parameter: f64,
    /// Model-space sample.
    pub point: Point3,
}

/// The samples of a polyline, with or without source parameters.
///
/// A parameterized polyline carries one parameter per sample in the sample
/// row, so a parameter list that does not match the sample count has no
/// spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PolylineSamples {
    /// Samples the source did not parameterize.
    Unparameterized {
        /// Ordered model-space samples.
        points: Vec<Point3>,
    },
    /// Samples the source parameterized.
    Parameterized {
        /// Ordered samples, each with its source parameter.
        vertices: Vec<PolylineVertex>,
    },
}

/// Source-native polyline with an explicit chordal error bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "PolylineCurveWire"))]
#[serde(try_from = "PolylineCurveWire", into = "PolylineCurveWire")]
pub struct PolylineCurve {
    samples: PolylineSamples,
    chordal_deflection: f64,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolylineCurveWire {
    samples: PolylineSamples,
    chordal_deflection: f64,
}

impl From<PolylineCurve> for PolylineCurveWire {
    fn from(curve: PolylineCurve) -> Self {
        Self {
            samples: curve.samples,
            chordal_deflection: curve.chordal_deflection,
        }
    }
}

impl TryFrom<PolylineCurveWire> for PolylineCurve {
    type Error = GeometryLayoutError;

    fn try_from(wire: PolylineCurveWire) -> Result<Self, Self::Error> {
        Self::new(wire.samples, wire.chordal_deflection)
    }
}

impl PolylineSamples {
    /// Number of samples.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Unparameterized { points } => points.len(),
            Self::Parameterized { vertices } => vertices.len(),
        }
    }

    /// True when the polyline states no sample.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ordered model-space samples.
    pub fn points(&self) -> impl Iterator<Item = Point3> + '_ {
        let (points, vertices) = match self {
            Self::Unparameterized { points } => (Some(points), None),
            Self::Parameterized { vertices } => (None, Some(vertices)),
        };
        points
            .into_iter()
            .flatten()
            .copied()
            .chain(vertices.into_iter().flatten().map(|vertex| vertex.point))
    }

    /// Source parameters, absent when the source stated none.
    pub fn parameters(&self) -> Option<impl Iterator<Item = f64> + '_> {
        match self {
            Self::Unparameterized { .. } => None,
            Self::Parameterized { vertices } => {
                Some(vertices.iter().map(|vertex| vertex.parameter))
            }
        }
    }

    /// Edit each sample point in place.
    pub fn edit_points(&mut self, mut edit: impl FnMut(&mut Point3)) {
        match self {
            Self::Unparameterized { points } => points.iter_mut().for_each(&mut edit),
            Self::Parameterized { vertices } => vertices
                .iter_mut()
                .for_each(|vertex| edit(&mut vertex.point)),
        }
    }
}

impl PolylineCurve {
    /// Build a polyline from its sample rows.
    ///
    /// A parameterized sample carries its parameter in the row, so there is no
    /// second list to pair up and no count to compare.
    pub fn new(
        samples: PolylineSamples,
        chordal_deflection: f64,
    ) -> Result<Self, GeometryLayoutError> {
        if samples.len() < 2 {
            return Err(geometry_layout_error(
                "polyline must contain at least two points",
            ));
        }
        if samples
            .points()
            .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite))
        {
            return Err(geometry_layout_error("points must be finite"));
        }
        if !chordal_deflection.is_finite() || chordal_deflection < 0.0 {
            return Err(geometry_layout_error(
                "chordal_deflection must be finite and non-negative",
            ));
        }
        if let Some(parameters) = samples.parameters() {
            let parameters: Vec<f64> = parameters.collect();
            if !parameters.iter().all(|value| value.is_finite())
                || !(parameters.windows(2).all(|pair| pair[0] < pair[1])
                    || parameters.windows(2).all(|pair| pair[0] > pair[1]))
            {
                return Err(geometry_layout_error(
                    "parameters must be finite and strictly monotonic",
                ));
            }
        }
        Ok(Self {
            samples,
            chordal_deflection,
        })
    }

    /// The polyline's sample rows.
    #[must_use]
    pub const fn samples(&self) -> &PolylineSamples {
        &self.samples
    }

    /// Ordered model-space samples.
    pub fn points(&self) -> impl Iterator<Item = Point3> + '_ {
        self.samples.points()
    }

    /// Number of samples.
    #[must_use]
    pub fn point_count(&self) -> usize {
        self.samples.len()
    }

    /// Source parameters, absent when the source stated none.
    pub fn parameters(&self) -> Option<impl Iterator<Item = f64> + '_> {
        self.samples.parameters()
    }

    /// Edit the sample rows transactionally.
    pub fn edit_samples(
        &mut self,
        edit: impl FnOnce(&mut PolylineSamples),
    ) -> Result<(), GeometryLayoutError> {
        let mut candidate = self.samples.clone();
        edit(&mut candidate);
        *self = Self::new(candidate, self.chordal_deflection)?;
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
#[serde(deny_unknown_fields)]
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
    radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let radius = PositiveReal::new(radius)
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
    radius: NonNegativeReal,
    ratio: PositiveReal,
    half_angle: FiniteReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let radius = NonNegativeReal::new(radius)
            .ok_or("ConeSurface.radius must be nonnegative and finite")?;
        let ratio =
            PositiveReal::new(ratio).ok_or("ConeSurface.ratio must be positive and finite")?;
        let half_angle =
            FiniteReal::new(half_angle).ok_or("ConeSurface.half_angle must be finite")?;
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
    radius: FiniteReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let radius = FiniteReal::new(radius).ok_or("SphereSurface.radius must be finite")?;
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
    major_radius: PositiveReal,
    minor_radius: FiniteReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("TorusSurface.major_radius must be positive and finite")?;
        let minor_radius =
            FiniteReal::new(minor_radius).ok_or("TorusSurface.minor_radius must be finite")?;
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
#[serde(deny_unknown_fields)]
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
    radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
            PositiveReal::new(radius).ok_or("CircleCurve.radius must be positive and finite")?;
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
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("EllipseCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
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
    focal_distance: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let focal_distance = PositiveReal::new(focal_distance)
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
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
    frame: OrthonormalFrame3,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("HyperbolaCurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
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
#[serde(deny_unknown_fields)]
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
    axial_origin: FiniteReal,
    axial_cos: FiniteReal,
    axial_sin: FiniteReal,
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
        let axial_origin = FiniteReal::new(axial_origin)
            .ok_or("PolarHarmonicPcurve.axial_origin must be finite")?;
        let axial_cos =
            FiniteReal::new(axial_cos).ok_or("PolarHarmonicPcurve.axial_cos must be finite")?;
        let axial_sin =
            FiniteReal::new(axial_sin).ok_or("PolarHarmonicPcurve.axial_sin must be finite")?;
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
    azimuth_origin: FiniteReal,
    azimuth_rate: FiniteReal,
    plane_phase: FiniteReal,
    plane_slope: FiniteReal,
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
        let azimuth_origin = FiniteReal::new(azimuth_origin)
            .ok_or("SphericalGreatCirclePcurve.azimuth_origin must be finite")?;
        let azimuth_rate = FiniteReal::new(azimuth_rate)
            .ok_or("SphericalGreatCirclePcurve.azimuth_rate must be finite")?;
        let plane_phase = FiniteReal::new(plane_phase)
            .ok_or("SphericalGreatCirclePcurve.plane_phase must be finite")?;
        let plane_slope = FiniteReal::new(plane_slope)
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
    radius: PositiveReal,
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
            PositiveReal::new(radius).ok_or("CirclePcurve.radius must be positive and finite")?;
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
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
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
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("EllipsePcurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
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
    focal_distance: PositiveReal,
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
        let focal_distance = PositiveReal::new(focal_distance)
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
    major_radius: PositiveReal,
    minor_radius: PositiveReal,
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
        let major_radius = PositiveReal::new(major_radius)
            .ok_or("HyperbolaPcurve.major_radius must be positive and finite")?;
        let minor_radius = PositiveReal::new(minor_radius)
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
    distance: FiniteReal,
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
        let distance = FiniteReal::new(distance).ok_or("OffsetPcurve.distance must be finite")?;
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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
    poles: PolarNurbsPoles,
    periodic: bool,
}

/// One rational polar pole: its radial and axial halves and its weight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeightedPolarNurbsPole {
    /// Euclidean radial-plane pole.
    pub radial: Point2,
    /// Axial pole value.
    pub axial: f64,
    /// Rational weight at this pole.
    pub weight: PositiveReal,
}

/// The poles of a polar parameter-space NURBS curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum PolarNurbsPoles {
    /// A polynomial polar curve: its poles carry no weight.
    Polynomial {
        /// Poles in parameter order.
        poles: Vec<PolarNurbsPole>,
    },
    /// A rational polar curve: every pole carries its weight.
    Rational {
        /// Pole rows in parameter order.
        poles: Vec<WeightedPolarNurbsPole>,
    },
}

impl PolarNurbsPoles {
    /// Pair a source's pole lane with its weight lane.
    ///
    /// The result is absent when the weight lane does not cover the poles or
    /// carries a non-positive or non-finite weight.
    #[must_use]
    pub fn from_lanes(poles: Vec<PolarNurbsPole>, weights: Option<Vec<f64>>) -> Option<Self> {
        let Some(weights) = weights else {
            return Some(Self::Polynomial { poles });
        };
        if weights.len() != poles.len() {
            return None;
        }
        Some(Self::Rational {
            poles: poles
                .into_iter()
                .zip(weights)
                .map(|(pole, weight)| {
                    Some(WeightedPolarNurbsPole {
                        radial: pole.radial,
                        axial: pole.axial,
                        weight: PositiveReal::new(weight)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        })
    }

    /// Number of poles.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Polynomial { poles } => poles.len(),
            Self::Rational { poles } => poles.len(),
        }
    }

    /// True when the curve states no pole.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Paired radial and axial poles in parameter order.
    #[must_use]
    pub fn poles(&self) -> Vec<PolarNurbsPole> {
        match self {
            Self::Polynomial { poles } => poles.clone(),
            Self::Rational { poles } => poles
                .iter()
                .map(|pole| PolarNurbsPole {
                    radial: pole.radial,
                    axial: pole.axial,
                })
                .collect(),
        }
    }

    /// Rational weights in pole order, absent on a polynomial curve.
    #[must_use]
    pub fn weights(&self) -> Option<Vec<f64>> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { poles } => Some(poles.iter().map(|pole| pole.weight.get()).collect()),
        }
    }

    /// Reverse the pole order.
    pub fn reverse(&mut self) {
        match self {
            Self::Polynomial { poles } => poles.reverse(),
            Self::Rational { poles } => poles.reverse(),
        }
    }

    /// Edit every pole in place.
    pub fn edit_poles(&mut self, mut edit: impl FnMut(&mut Point2, &mut f64)) {
        match self {
            Self::Polynomial { poles } => poles
                .iter_mut()
                .for_each(|pole| edit(&mut pole.radial, &mut pole.axial)),
            Self::Rational { poles } => poles
                .iter_mut()
                .for_each(|pole| edit(&mut pole.radial, &mut pole.axial)),
        }
    }
}

impl PolarPcurveNurbs {
    /// Build a polar NURBS from its pole rows.
    ///
    /// A pole is one row carrying its radial and axial halves together, so
    /// there are no two lists to pair up and no length to compare.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        poles: PolarNurbsPoles,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(degree, knots.len(), poles.len(), "poles")?;
        if degree == 0 {
            return Err(NurbsError("polar NURBS degree must be positive".into()));
        }
        if !poles.poles().iter().all(|pole| {
            pole.radial.u.is_finite() && pole.radial.v.is_finite() && pole.axial.is_finite()
        }) {
            return Err(NurbsError("poles contain a non-finite value".into()));
        }
        require_nondecreasing_knots(&knots)?;
        Ok(Self {
            degree,
            knots,
            poles,
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

    /// Build a polar NURBS from a source's pole and weight lanes.
    pub fn from_lanes(
        degree: u32,
        knots: Vec<f64>,
        poles: Vec<PolarNurbsPole>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = PolarNurbsPoles::from_lanes(poles, weights).ok_or_else(|| {
            NurbsError("polar NURBS weights must cover the poles and be finite and positive".into())
        })?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Paired radial and axial poles.
    pub fn poles(&self) -> Vec<PolarNurbsPole> {
        self.poles.poles()
    }

    /// Poles in parameter order, with the curve's rational form.
    pub const fn pole_rows(&self) -> &PolarNurbsPoles {
        &self.poles
    }

    /// Atomically edit paired poles and preserve finite coordinates.
    pub fn edit_poles(
        &mut self,
        edit: impl FnMut(&mut Point2, &mut f64),
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.edit_poles(edit);
        if poles.poles().iter().all(|pole| {
            pole.radial.u.is_finite() && pole.radial.v.is_finite() && pole.axial.is_finite()
        }) {
            self.poles = poles;
            Ok(())
        } else {
            Err(NurbsError("poles contain a non-finite value".into()))
        }
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<Vec<f64>> {
        self.poles.weights()
    }

    /// Replace the poles, keeping the knot cardinality.
    pub fn set_poles(&mut self, poles: PolarNurbsPoles) -> Result<(), NurbsError> {
        *self = Self::new(self.degree, self.knots.clone(), poles, self.periodic)?;
        Ok(())
    }

    /// Whether the NURBS parameterization is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Reverse paired poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.poles.reverse();
        self.knots.reverse();
        for knot in &mut self.knots {
            *knot = -*knot;
        }
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct PolarPcurveNurbsWire {
    degree: u32,
    knots: Vec<f64>,
    poles: PolarNurbsPoles,
    #[serde(default)]
    periodic: bool,
}

impl Serialize for PolarPcurveNurbs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        PolarPcurveNurbsWire {
            degree: self.degree,
            knots: self.knots.clone(),
            poles: self.poles.clone(),
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
        Self::new(wire.degree, wire.knots, wire.poles, wire.periodic)
            .map_err(serde::de::Error::custom)
    }
}

/// Checked two-dimensional NURBS payload for pcurves and sketch curves.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveNurbs {
    degree: u32,
    knots: Vec<f64>,
    poles: PcurveNurbsPoles,
    #[serde(default)]
    periodic: bool,
}

impl PcurveNurbs {
    /// Build a parameter-space NURBS with consistent cardinalities.
    pub fn new(
        degree: u32,
        knots: Vec<f64>,
        poles: PcurveNurbsPoles,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(degree, knots.len(), poles.len(), "control_points")?;
        if degree == 0 {
            return Err(NurbsError("pcurve NURBS degree must be positive".into()));
        }
        require_finite_points_2("control_points", &poles.points())?;
        require_nondecreasing_knots(&knots)?;
        Ok(Self {
            degree,
            knots,
            poles,
            periodic,
        })
    }

    /// Lift each two-dimensional pole into model space, keeping its weight.
    pub fn lift(&self, lift: impl FnMut(Point2) -> Point3) -> Result<NurbsCurve, NurbsError> {
        let points: Vec<Point3> = self.poles.points().into_iter().map(lift).collect();
        let poles = NurbsPoles3::from_lanes(points, self.poles.weights())
            .ok_or_else(|| NurbsError("lifted pcurve weights are not admissible".into()))?;
        NurbsCurve::new(self.degree, self.knots.clone(), poles, self.periodic)
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

    /// Build a parameter-space NURBS from a source's pole and weight lanes.
    pub fn from_lanes(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point2>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = PcurveNurbsPoles::from_lanes(control_points, weights).ok_or_else(|| {
            NurbsError("pcurve NURBS weights must cover the poles and be finite and positive".into())
        })?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Poles in parameter order, with the pcurve's rational form.
    pub const fn pole_rows(&self) -> &PcurveNurbsPoles {
        &self.poles
    }

    /// Control points in parameter order.
    pub fn control_points(&self) -> Vec<Point2> {
        self.poles.points()
    }

    /// Number of poles.
    pub fn pole_count(&self) -> usize {
        self.poles.len()
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point2),
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.edit_points(edit);
        require_finite_points_2("control_points", &poles.points())?;
        self.poles = poles;
        Ok(())
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<Vec<f64>> {
        self.poles.weights()
    }

    /// Replace the poles, keeping the knot cardinality.
    pub fn set_poles(&mut self, poles: PcurveNurbsPoles) -> Result<(), NurbsError> {
        *self = Self::new(self.degree, self.knots.clone(), poles, self.periodic)?;
        Ok(())
    }

    /// Whether the parameter-space curve is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Reverse poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.poles.reverse();
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
        #[serde(deny_unknown_fields)]
        struct Wire {
            degree: u32,
            knots: Vec<f64>,
            poles: PcurveNurbsPoles,
            #[serde(default)]
            periodic: bool,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.degree, wire.knots, wire.poles, wire.periodic)
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
                    .edit_control_points(|point| *point = scale(*point))
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
                let mut rows = transform.affine_rows();
                rows[0][1] *= u_scale / v_scale;
                rows[0][2] *= u_scale;
                rows[1][0] *= v_scale / u_scale;
                rows[1][2] *= v_scale;
                let transform =
                    Transform2::affine(rows).ok_or("scaled pcurve transform is invalid")?;
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
#[serde(try_from = "PcurveInlineFormWire")]
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
    pub fn set_fit_tolerance(&mut self, value: f64) -> Result<(), CacheContractError> {
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
#[serde(try_from = "PcurveGeneralFormWire")]
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
    pub fn set_fit_tolerance(&mut self, value: Option<f64>) -> Result<(), CacheContractError> {
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
