// SPDX-License-Identifier: Apache-2.0
//! NURBS curves, surfaces, pole layouts, and knot invariants.

use crate::math::Point3;
use crate::scalar::NonZeroReal;
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
    /// once, at the decode boundary.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, naming both counts,
    /// and a weight that is zero or non-finite, naming its index.
    pub fn from_lanes(points: Vec<Point3>, weights: Option<Vec<f64>>) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { points });
        };
        if weights.len() != points.len() {
            return Err(NurbsError::WeightLaneLength {
                field: "poles".to_owned(),
                poles: points.len(),
                weights: weights.len(),
            });
        }
        Ok(Self::Rational {
            points: points
                .into_iter()
                .zip(weights)
                .enumerate()
                .map(|(index, (point, weight))| {
                    Ok(WeightedPole3 {
                        point,
                        weight: NonZeroReal::new(weight).ok_or(NurbsError::UnusableWeight {
                            field: "poles".to_owned(),
                            index,
                            weight,
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, NurbsError>>()?,
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

    fn require_finite_points(&self) -> Result<(), NurbsError> {
        match self {
            Self::Polynomial { points } => require_finite_points_3("control_points", points),
            Self::Rational { points } => {
                require_finite_points_3("control_points", points.iter().map(|pole| &pole.point))
            }
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

    /// Edit every pole position, keeping the prior positions on a refusal.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_points(
        &mut self,
        edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut candidate = self.clone();
        candidate.apply_points(edit)?;
        *self = candidate;
        Ok(())
    }

    /// Edit every pole position in place, keeping every accepted edit.
    ///
    /// A refusal leaves the lane partly edited, so the caller owns the copy
    /// that states the prior positions.
    fn apply_points(
        &mut self,
        mut edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        match self {
            Self::Polynomial { points } => {
                for point in points.iter_mut() {
                    edit(point)?;
                }
            }
            Self::Rational { points } => {
                for pole in points.iter_mut() {
                    edit(&mut pole.point)?;
                }
            }
        }
        Ok(())
    }

    /// Replace the weights, keeping the pole positions.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles or carries a zero or
    /// non-finite weight, as [`Self::from_lanes`] does.
    pub fn with_weights(&self, weights: Option<Vec<f64>>) -> Result<Self, NurbsError> {
        Self::from_lanes(self.points(), weights)
    }
}

/// The control grid of a NURBS surface, stating the surface's rational form.
///
/// A rational pole carries its weight in its own row, so a weight grid that
/// does not cover the pole grid has no spelling.
///
/// Transposition requires admission as a [`NurbsSurface`].
///
/// ```compile_fail
/// use cadmpeg_ir::geometry::nurbs::NurbsPoleGrid;
/// let mut grid = NurbsPoleGrid::Polynomial { rows: Vec::new() };
/// grid.transpose();
/// ```
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
    /// # Errors
    ///
    /// Refuses a weight grid that does not cover the pole grid, row count or
    /// row width, naming both counts, and a weight that is zero or non-finite,
    /// naming its index within its row.
    pub fn from_lanes(
        rows: Vec<Vec<Point3>>,
        weights: Option<Vec<Vec<f64>>>,
    ) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { rows });
        };
        if weights.len() != rows.len() {
            return Err(NurbsError::WeightLaneLength {
                field: "pole grid".to_owned(),
                poles: rows.len(),
                weights: weights.len(),
            });
        }
        let paired = rows
            .into_iter()
            .zip(weights)
            .map(|(row, weight_row)| {
                if weight_row.len() != row.len() {
                    return Err(NurbsError::WeightLaneLength {
                        field: "pole grid row".to_owned(),
                        poles: row.len(),
                        weights: weight_row.len(),
                    });
                }
                row.into_iter()
                    .zip(weight_row)
                    .enumerate()
                    .map(|(index, (point, weight))| {
                        Ok(WeightedPole3 {
                            point,
                            weight: NonZeroReal::new(weight).ok_or(NurbsError::UnusableWeight {
                                field: "pole grid row".to_owned(),
                                index,
                                weight,
                            })?,
                        })
                    })
                    .collect::<Result<Vec<_>, NurbsError>>()
            })
            .collect::<Result<Vec<_>, NurbsError>>()?;
        Ok(Self::Rational { rows: paired })
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

    fn require_finite_points(&self) -> Result<(), NurbsError> {
        match self {
            Self::Polynomial { rows } => {
                require_finite_points_3("control_points", rows.iter().flatten())
            }
            Self::Rational { rows } => require_finite_points_3(
                "control_points",
                rows.iter().flatten().map(|pole| &pole.point),
            ),
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

    /// Edit every pole position, keeping the prior positions on a refusal.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_points(
        &mut self,
        edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut candidate = self.clone();
        candidate.apply_points(edit)?;
        *self = candidate;
        Ok(())
    }

    /// Edit every pole position in place, keeping every accepted edit.
    ///
    /// A refusal leaves the grid partly edited, so the caller owns the copy
    /// that states the prior positions.
    fn apply_points(
        &mut self,
        mut edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        match self {
            Self::Polynomial { rows } => {
                for point in rows.iter_mut().flatten() {
                    edit(point)?;
                }
            }
            Self::Rational { rows } => {
                for pole in rows.iter_mut().flatten() {
                    edit(&mut pole.point)?;
                }
            }
        }
        Ok(())
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
                return Err(NurbsError::Structure(format!(
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

    /// Endpoints of the full U knot vector.
    #[must_use]
    pub fn full_u_knot_endpoints(&self) -> [f64; 2] {
        [self.u_knots[0], self.u_knots[self.u_knots.len() - 1]]
    }

    /// Endpoints of the full V knot vector.
    #[must_use]
    pub fn full_v_knot_endpoints(&self) -> [f64; 2] {
        [self.v_knots[0], self.v_knots[self.v_knots.len() - 1]]
    }

    /// Rectangular control grid in first-parameter-major order.
    pub fn control_points(&self) -> &[Vec<Point3>] {
        &self.control_points
    }

    /// Atomically edit pole coordinates while preserving the grid and finite values.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        mut edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut points = self.control_points.clone();
        for row in &mut points {
            for point in row.iter_mut() {
                edit(point)?;
            }
            require_finite_points_3("control_points", row.iter())?;
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
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NurbsError {
    /// A source stated a weight lane that does not cover its pole lane.
    #[error("{field}: {poles} pole(s) against {weights} weight(s)")]
    WeightLaneLength {
        /// The carrier field the lanes belong to.
        field: String,
        /// Poles the source stated.
        poles: usize,
        /// Weights the source stated.
        weights: usize,
    },
    /// A source stated a weight no pole can carry.
    #[error("{field}: weight {weight} at index {index} is not a usable weight")]
    UnusableWeight {
        /// The carrier field the weight belongs to.
        field: String,
        /// Position of the weight in the lane the source stated.
        index: usize,
        /// The refused weight.
        weight: f64,
    },
    /// A knot vector, degree, grid or coordinate the carrier cannot state.
    #[error("{0}")]
    Structure(String),
    /// An edit closure refused the value it was given, stating its own reason.
    #[error("{0}")]
    EditRefused(String),
}

impl From<NurbsError> for cadmpeg_core::CodecError {
    fn from(error: NurbsError) -> Self {
        Self::Malformed(error.to_string())
    }
}

fn checked_knot_count(field: &str, pole_count: usize, degree: u32) -> Result<usize, NurbsError> {
    pole_count
        .checked_add(degree as usize)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| NurbsError::Structure(format!("{field} knot count overflows usize")))
}

/// Every row of a control grid states the same pole count.
///
/// The grid is one object, so its rectangularity is one shape mint over that
/// object, not a comparison between two independently stated lists.
fn require_rectangular_grid<T>(field: &str, rows: &[Vec<T>]) -> Result<(), NurbsError> {
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
        Err(NurbsError::Structure(format!(
            "{field} must contain {expected} values, found {actual}"
        )))
    }
}

fn require_finite_points_3<'a>(
    field: &str,
    points: impl IntoIterator<Item = &'a Point3>,
) -> Result<(), NurbsError> {
    if points.into_iter().all(Point3::is_finite) {
        Ok(())
    } else {
        Err(NurbsError::Structure(format!(
            "{field} contains a non-finite point"
        )))
    }
}

fn require_finite_scalars(field: &str, values: &[f64]) -> Result<(), NurbsError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(NurbsError::Structure(format!(
            "{field} contains a non-finite value"
        )))
    }
}

pub(super) fn require_nondecreasing_knots(knots: &[f64]) -> Result<(), NurbsError> {
    require_finite_scalars("knots", knots)?;
    if knots_nondecreasing(knots) {
        Ok(())
    } else {
        Err(NurbsError::Structure("knots must be non-decreasing".into()))
    }
}

pub(super) fn require_curve_cardinality(
    degree: u32,
    knot_count: usize,
    pole_count: usize,
    point_field: &str,
) -> Result<(), NurbsError> {
    if pole_count <= degree as usize {
        return Err(NurbsError::Structure(format!(
            "{point_field} must contain more than degree {degree} poles, found {pole_count}"
        )));
    }
    require_length(
        "knots",
        knot_count,
        checked_knot_count("curve", pole_count, degree)?,
    )
}

/// One parameter axis of a tensor-product NURBS surface.
///
/// A degree, its knot vector and its periodicity are one statement about one
/// axis: the knot count a source may state depends on the degree, and the
/// periodicity describes that same knot vector. They travel together.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurfaceAxis {
    degree: u32,
    knots: Vec<f64>,
    periodic: bool,
}

impl NurbsSurfaceAxis {
    /// One axis of a surface: its degree, its knot vector and its periodicity.
    #[must_use]
    pub const fn new(degree: u32, knots: Vec<f64>, periodic: bool) -> Self {
        Self {
            degree,
            knots,
            periodic,
        }
    }
}

/// The pole grid and weight grid a source states for one NURBS surface.
///
/// A weight grid covers the pole grid it belongs to, so the two are one
/// statement and are paired once, at the decode boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurfaceLanes {
    control_points: Vec<Vec<Point3>>,
    weights: Option<Vec<Vec<f64>>>,
}

impl NurbsSurfaceLanes {
    /// The pole grid a source states, with its weight grid when it is
    /// rational.
    #[must_use]
    pub const fn new(control_points: Vec<Vec<Point3>>, weights: Option<Vec<Vec<f64>>>) -> Self {
        Self {
            control_points,
            weights,
        }
    }
}

impl NurbsSurface {
    /// Build a tensor-product NURBS surface with consistent cardinalities.
    pub fn new(
        u: NurbsSurfaceAxis,
        v: NurbsSurfaceAxis,
        poles: NurbsPoleGrid,
        normal_reversed: bool,
    ) -> Result<Self, NurbsError> {
        let NurbsSurfaceAxis {
            degree: u_degree,
            knots: u_knots,
            periodic: u_periodic,
        } = u;
        let NurbsSurfaceAxis {
            degree: v_degree,
            knots: v_knots,
            periodic: v_periodic,
        } = v;
        let u_count = poles.u_count();
        let v_count = poles.v_count();
        if u_count <= u_degree as usize {
            return Err(NurbsError::Structure(format!(
                "u_count must exceed u_degree {u_degree}, found {u_count}"
            )));
        }
        if v_count <= v_degree as usize {
            return Err(NurbsError::Structure(format!(
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
        match &poles {
            NurbsPoleGrid::Polynomial { rows } => require_rectangular_grid("control_points", rows)?,
            NurbsPoleGrid::Rational { rows } => require_rectangular_grid("control_points", rows)?,
        }
        poles.require_finite_points()?;
        require_nondecreasing_knots(&u_knots)
            .map_err(|error| NurbsError::Structure(format!("u_{error}")))?;
        require_nondecreasing_knots(&v_knots)
            .map_err(|error| NurbsError::Structure(format!("v_{error}")))?;
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
        require_nondecreasing_knots(&values)
            .map_err(|error| NurbsError::Structure(format!("u_{error}")))?;
        self.u_knots = values;
        Ok(())
    }

    /// Atomically edit the v knot vector and preserve its invariants.
    pub fn edit_v_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.v_knots.clone();
        edit(&mut values);
        require_nondecreasing_knots(&values)
            .map_err(|error| NurbsError::Structure(format!("v_{error}")))?;
        self.v_knots = values;
        Ok(())
    }

    /// Number of control points along u, the number of grid rows.
    pub fn u_count(&self) -> usize {
        self.poles.u_count()
    }

    /// Number of control points along v, the length of every grid row.
    pub fn v_count(&self) -> usize {
        self.poles.v_count()
    }

    /// Build a NURBS surface from a source's pole grid and weight grid.
    ///
    /// A source that states poles and weights as two grids pairs them here,
    /// once, at the decode boundary; the surface itself carries pole rows.
    ///
    /// # Errors
    ///
    /// Refuses lanes the carrier cannot state: a weight grid that does not
    /// cover its pole grid, an unusable weight, a knot count that does not
    /// follow from the degree and the pole count, or a non-finite coordinate.
    pub fn from_lanes(
        u: NurbsSurfaceAxis,
        v: NurbsSurfaceAxis,
        lanes: NurbsSurfaceLanes,
        normal_reversed: bool,
    ) -> Result<Self, NurbsError> {
        let NurbsSurfaceLanes {
            control_points,
            weights,
        } = lanes;
        let poles = NurbsPoleGrid::from_lanes(control_points, weights)?;
        Self::new(u, v, poles, normal_reversed)
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
        match &self.poles {
            NurbsPoleGrid::Polynomial { rows } => rows.get(u)?.get(v).copied(),
            NurbsPoleGrid::Rational { rows } => rows.get(u)?.get(v).map(|pole| pole.point),
        }
    }

    /// Rational weight at grid position `(u, v)`, absent when non-rational.
    pub fn weight(&self, u: usize, v: usize) -> Option<f64> {
        match &self.poles {
            NurbsPoleGrid::Polynomial { .. } => None,
            NurbsPoleGrid::Rational { rows } => rows.get(u)?.get(v).map(|pole| pole.weight.get()),
        }
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.apply_points(edit)?;
        poles.require_finite_points()?;
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
            NurbsSurfaceAxis::new(self.u_degree, self.u_knots.clone(), self.u_periodic),
            NurbsSurfaceAxis::new(self.v_degree, self.v_knots.clone(), self.v_periodic),
            poles,
            self.normal_reversed,
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
        // Surface admission and every grid mutation preserve nonempty,
        // rectangular rows. Raw pole grids do not expose this operation.
        fn transpose<T: Copy>(rows: &[Vec<T>], width: usize) -> Vec<Vec<T>> {
            (0..width)
                .map(|column| rows.iter().map(|row| row[column]).collect())
                .collect()
        }

        let width = self.v_count();
        match &mut self.poles {
            NurbsPoleGrid::Polynomial { rows } => *rows = transpose(rows, width),
            NurbsPoleGrid::Rational { rows } => *rows = transpose(rows, width),
        }
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
            NurbsSurfaceAxis::new(wire.u_degree, wire.u_knots, wire.u_periodic),
            NurbsSurfaceAxis::new(wire.v_degree, wire.v_knots, wire.v_periodic),
            wire.poles,
            wire.normal_reversed,
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
        poles.require_finite_points()?;
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

    /// Endpoints of the full knot vector.
    #[must_use]
    pub fn full_knot_endpoints(&self) -> [f64; 2] {
        [self.knots[0], self.knots[self.knots.len() - 1]]
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
        let poles = NurbsPoles3::from_lanes(control_points, weights)?;
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
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.clone();
        poles.apply_points(edit)?;
        poles.require_finite_points()?;
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

#[cfg(test)]
mod tests;
