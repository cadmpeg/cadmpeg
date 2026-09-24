// SPDX-License-Identifier: Apache-2.0
//! NURBS curves, surfaces, pole layouts, and knot invariants.

/// Homogeneous Bezier extraction and boundary certificates.
pub mod bezier;
/// Rational control-polygon speed bounds.
pub mod bounds;

use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::scalar::NonZeroReal;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Knot values that are finite and non-decreasing.
///
/// A NURBS store admits its knots through [`Self::new`], so a reader holds
/// the guarantee and the raw values through [`Self::as_slice`] or deref.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct KnotVector(Vec<f64>);

impl KnotVector {
    /// Admit finite non-decreasing knot values.
    ///
    /// # Errors
    ///
    /// Refuses a non-finite knot, then a decreasing pair.
    pub(crate) fn new(knots: Vec<f64>) -> Result<Self, NurbsError> {
        require_nondecreasing_knots(&knots)?;
        Ok(Self(knots))
    }

    /// Borrow the knot values.
    #[must_use]
    pub fn as_slice(&self) -> &[f64] {
        &self.0
    }

    /// Reverse the order and negate every value, the knots of the reversed
    /// parameterization. Negation turns a non-decreasing sequence into a
    /// non-increasing one, and the reversal restores the order, so the
    /// result stays admitted.
    pub(super) fn reverse_negated(&mut self) {
        self.0.reverse();
        for knot in &mut self.0 {
            *knot = -*knot;
        }
    }
}

impl std::ops::Deref for KnotVector {
    type Target = [f64];
    fn deref(&self) -> &[f64] {
        &self.0
    }
}

impl<'a> IntoIterator for &'a KnotVector {
    type Item = &'a f64;
    type IntoIter = std::slice::Iter<'a, f64>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// One rational pole in model space: its position and its weight.
// A source states a raw position; a NURBS store holds the admitted row, whose
// position is a `FinitePoint3`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WeightedPole3<P = Point3> {
    /// Pole position in model space.
    pub point: P,
    /// Rational weight at this pole.
    pub weight: NonZeroReal,
}

/// The poles of a NURBS curve, stating the curve's rational form.
///
/// A rational pole carries its weight in its own row, so a weight list that
/// does not cover the poles has no spelling, and "the curve is polynomial" has
/// exactly one spelling.
// A source states raw positions; a `NurbsCurve` holds the admitted poles,
// whose positions are `FinitePoint3` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum NurbsPoles3<P = Point3> {
    /// A polynomial curve: its poles carry no weight.
    Polynomial {
        /// Poles in parameter order.
        points: Vec<P>,
    },
    /// A rational curve: every pole carries its weight.
    Rational {
        /// Pole rows in parameter order.
        points: Vec<WeightedPole3<P>>,
    },
}

/// The refusal of a pole position with a non-finite coordinate.
pub(super) fn non_finite_control_point() -> NurbsError {
    NurbsError::Structure("control_points contains a non-finite point".into())
}

/// A pole value a producer hands a NURBS store: a raw value, which the store
/// admits in its own refusal order, or an admitted value, which it keeps.
pub trait PoleValue<T>: Copy {
    /// The admitted value, absent when a raw value is not finite.
    fn admit(self) -> Option<T>;
}

impl PoleValue<FinitePoint3> for Point3 {
    fn admit(self) -> Option<FinitePoint3> {
        FinitePoint3::new(self)
    }
}

impl PoleValue<FinitePoint3> for FinitePoint3 {
    fn admit(self) -> Option<FinitePoint3> {
        Some(self)
    }
}

/// Pair each pole of a lane with its weight, after the weight lane has been
/// found to cover the poles.
fn weighted_poles<P, W>(
    points: Vec<P>,
    weights: Vec<W>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, NurbsError>,
) -> Result<Vec<WeightedPole3<P>>, NurbsError> {
    points
        .into_iter()
        .zip(weights)
        .enumerate()
        .map(|(index, (point, value))| {
            Ok(WeightedPole3 {
                point,
                weight: weight(index, value)?,
            })
        })
        .collect()
}

impl NurbsPoles3 {
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
}

impl<P: PoleValue<FinitePoint3>> NurbsPoles3<P> {
    /// The poles with admitted positions.
    ///
    /// # Errors
    ///
    /// Refuses a pole position with a non-finite coordinate.
    fn admit(self) -> Result<NurbsPoles3<FinitePoint3>, NurbsError> {
        self.try_map_points(|point| point.admit().ok_or_else(non_finite_control_point))
    }
}

impl NurbsPoles3<FinitePoint3> {
    /// The poles with raw positions, for a reader that edits or writes them.
    #[must_use]
    pub fn to_raw(&self) -> NurbsPoles3 {
        let Ok(raw) = self
            .clone()
            .try_map_points(|point| Ok::<_, std::convert::Infallible>(point.get()));
        raw
    }

    /// Raw pole positions in parameter order, for a reader that computes with
    /// or writes them.
    #[must_use]
    pub fn raw_points(&self) -> Vec<Point3> {
        self.points().into_iter().map(FinitePoint3::get).collect()
    }
}

impl<P> NurbsPoles3<P> {
    /// Pair a source's pole lane with its weight lane. A store admits the
    /// positions it is handed.
    ///
    /// A source that states poles and weights as two arrays pairs them here,
    /// once, at the decode boundary.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, naming both counts,
    /// and a weight that is zero or non-finite, naming its index.
    pub fn from_lanes(points: Vec<P>, weights: Option<Vec<f64>>) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { points });
        };
        require_weight_lane("poles", points.len(), weights.len())?;
        Ok(Self::Rational {
            points: weighted_poles(points, weights, |index, weight| {
                admit_weight("poles", index, weight)
            })?,
        })
    }

    /// Pair a pole lane with an admitted weight lane. The weight type states
    /// the weight contract; a store admits the positions it is handed.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, naming both
    /// counts.
    pub fn from_checked_lanes(
        points: Vec<P>,
        weights: Option<Vec<NonZeroReal>>,
    ) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { points });
        };
        require_weight_lane("poles", points.len(), weights.len())?;
        Ok(Self::Rational {
            points: weighted_poles(points, weights, |_, weight| Ok(weight))?,
        })
    }

    /// Count poles.
    #[must_use]
    pub fn count(&self) -> usize {
        match self {
            Self::Polynomial { points } => points.len(),
            Self::Rational { points } => points.len(),
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

    /// Map every pole position in parameter order, keeping the weights.
    fn try_map_points<Q, E>(
        self,
        mut point: impl FnMut(P) -> Result<Q, E>,
    ) -> Result<NurbsPoles3<Q>, E> {
        Ok(match self {
            Self::Polynomial { points } => NurbsPoles3::Polynomial {
                points: points.into_iter().map(point).collect::<Result<_, E>>()?,
            },
            Self::Rational { points } => NurbsPoles3::Rational {
                points: points
                    .into_iter()
                    .map(|pole| {
                        Ok(WeightedPole3 {
                            point: point(pole.point)?,
                            weight: pole.weight,
                        })
                    })
                    .collect::<Result<_, E>>()?,
            },
        })
    }
}

impl<P: Copy> NurbsPoles3<P> {
    /// Pole positions in parameter order.
    #[must_use]
    pub fn points(&self) -> Vec<P> {
        match self {
            Self::Polynomial { points } => points.clone(),
            Self::Rational { points } => points.iter().map(|pole| pole.point).collect(),
        }
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
// A source states raw positions; a `NurbsSurface` holds the admitted grid,
// whose positions are `FinitePoint3` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "form", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum NurbsPoleGrid<P = Point3> {
    /// A polynomial surface: its poles carry no weight.
    Polynomial {
        /// Control grid rows: `rows[i][j]` is pole `(i, j)`.
        rows: Vec<Vec<P>>,
    },
    /// A rational surface: every pole carries its weight.
    Rational {
        /// Control grid rows: `rows[i][j]` is pole `(i, j)`.
        rows: Vec<Vec<WeightedPole3<P>>>,
    },
}

/// Pair each row of a pole grid with its weight row, refusing a weight grid
/// that does not cover the pole grid, row count or row width.
fn weighted_rows<P, W>(
    rows: Vec<Vec<P>>,
    weights: Vec<Vec<W>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, NurbsError>,
) -> Result<Vec<Vec<WeightedPole3<P>>>, NurbsError> {
    require_weight_lane("pole grid", rows.len(), weights.len())?;
    rows.into_iter()
        .zip(weights)
        .map(|(row, weight_row)| {
            require_weight_lane("pole grid row", row.len(), weight_row.len())?;
            weighted_poles(row, weight_row, &mut weight)
        })
        .collect()
}

impl NurbsPoleGrid {
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

impl<P: PoleValue<FinitePoint3>> NurbsPoleGrid<P> {
    /// The grid with admitted positions.
    ///
    /// # Errors
    ///
    /// Refuses a pole position with a non-finite coordinate.
    fn admit(self) -> Result<NurbsPoleGrid<FinitePoint3>, NurbsError> {
        self.try_map_points(|point| point.admit().ok_or_else(non_finite_control_point))
    }
}

impl NurbsPoleGrid<FinitePoint3> {
    /// The grid with raw positions, for a reader that edits or writes them.
    #[must_use]
    pub fn to_raw(&self) -> NurbsPoleGrid {
        let Ok(raw) = self
            .clone()
            .try_map_points(|point| Ok::<_, std::convert::Infallible>(point.get()));
        raw
    }

    /// Raw pole positions as grid rows, for a reader that computes with or
    /// writes them.
    #[must_use]
    pub fn raw_points(&self) -> Vec<Vec<Point3>> {
        self.points()
            .into_iter()
            .map(|row| row.into_iter().map(FinitePoint3::get).collect())
            .collect()
    }
}

impl<P> NurbsPoleGrid<P> {
    /// Pair a source's pole grid with its weight grid. A store admits the
    /// positions it is handed.
    ///
    /// # Errors
    ///
    /// Refuses a weight grid that does not cover the pole grid, row count or
    /// row width, naming both counts, and a weight that is zero or non-finite,
    /// naming its index within its row.
    pub fn from_lanes(
        rows: Vec<Vec<P>>,
        weights: Option<Vec<Vec<f64>>>,
    ) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { rows });
        };
        Ok(Self::Rational {
            rows: weighted_rows(rows, weights, |index, weight| {
                admit_weight("pole grid row", index, weight)
            })?,
        })
    }

    /// Pair a pole grid with an admitted weight grid. The weight type states
    /// the weight contract; a store admits the positions it is handed.
    ///
    /// # Errors
    ///
    /// Refuses a weight grid that does not cover the pole grid, row count or
    /// row width, naming both counts.
    pub fn from_checked_lanes(
        rows: Vec<Vec<P>>,
        weights: Option<Vec<Vec<NonZeroReal>>>,
    ) -> Result<Self, NurbsError> {
        let Some(weights) = weights else {
            return Ok(Self::Polynomial { rows });
        };
        Ok(Self::Rational {
            rows: weighted_rows(rows, weights, |_, weight| Ok(weight))?,
        })
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

    /// Map every pole position in grid order, keeping the weights.
    fn try_map_points<Q, E>(
        self,
        mut point: impl FnMut(P) -> Result<Q, E>,
    ) -> Result<NurbsPoleGrid<Q>, E> {
        Ok(match self {
            Self::Polynomial { rows } => NurbsPoleGrid::Polynomial {
                rows: rows
                    .into_iter()
                    .map(|row| row.into_iter().map(&mut point).collect())
                    .collect::<Result<_, E>>()?,
            },
            Self::Rational { rows } => NurbsPoleGrid::Rational {
                rows: rows
                    .into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|pole| {
                                Ok(WeightedPole3 {
                                    point: point(pole.point)?,
                                    weight: pole.weight,
                                })
                            })
                            .collect()
                    })
                    .collect::<Result<_, E>>()?,
            },
        })
    }
}

impl<P: Copy> NurbsPoleGrid<P> {
    /// Pole positions as grid rows.
    #[must_use]
    pub fn points(&self) -> Vec<Vec<P>> {
        match self {
            Self::Polynomial { rows } => rows.clone(),
            Self::Rational { rows } => rows
                .iter()
                .map(|row| row.iter().map(|pole| pole.point).collect())
                .collect(),
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
    #[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))]
    u_knots: KnotVector,
    /// Full knot vector in v.
    #[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))]
    v_knots: KnotVector,
    /// Control grid rows, with the surface's rational form.
    #[cfg_attr(feature = "schema", schemars(with = "NurbsPoleGrid"))]
    poles: NurbsPoleGrid<FinitePoint3>,
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
    #[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))]
    u_knots: KnotVector,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))]
    v_knots: KnotVector,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<Vec<Point3>>"))]
    control_points: Vec<Vec<FinitePoint3>>,
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
        let u_knots = bspline_axis_knots("u", u_degree, u_count, u_knots)?;
        let v_knots = bspline_axis_knots("v", v_degree, v_count, v_knots)?;
        require_rectangular_grid("control_points", &control_points)?;
        let control_points = control_points
            .into_iter()
            .map(admit_finite_row_3)
            .collect::<Result<Vec<_>, NurbsError>>()?;
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

    /// Atomically edit pole coordinates while preserving the grid and finite values.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        mut edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut points = Vec::with_capacity(self.control_points.len());
        for row in &self.control_points {
            let mut row: Vec<Point3> = row.iter().map(|point| point.get()).collect();
            for point in &mut row {
                edit(point)?;
            }
            points.push(admit_finite_row_3(row)?);
        }
        self.control_points = points;
        Ok(())
    }
}

/// Admit one B-spline axis: more poles than its degree, the full knot count
/// for them, and finite non-decreasing knots, refused in that order.
fn bspline_axis_knots(
    axis: &str,
    degree: u32,
    count: usize,
    knots: Vec<f64>,
) -> Result<KnotVector, NurbsError> {
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
    KnotVector::new(knots)
}

/// Admit one control-point row whose every point is finite.
fn admit_finite_row_3(row: Vec<Point3>) -> Result<Vec<FinitePoint3>, NurbsError> {
    row.into_iter()
        .map(FinitePoint3::new)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| NurbsError::Structure("control_points contains a non-finite point".into()))
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

/// A weight lane covers the pole lane it belongs to.
pub(super) fn require_weight_lane(
    field: &str,
    poles: usize,
    weights: usize,
) -> Result<(), NurbsError> {
    if poles == weights {
        Ok(())
    } else {
        Err(NurbsError::WeightLaneLength {
            field: field.to_owned(),
            poles,
            weights,
        })
    }
}

/// Admit one weight a source states, naming its index within its lane.
pub(super) fn admit_weight(
    field: &str,
    index: usize,
    weight: f64,
) -> Result<NonZeroReal, NurbsError> {
    NonZeroReal::new(weight).ok_or_else(|| NurbsError::UnusableWeight {
        field: field.to_owned(),
        index,
        weight,
    })
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
/// statement and are paired once, at the decode boundary. A source states raw
/// lanes; a producer that holds admitted positions and weights states the
/// `FinitePoint3` and `NonZeroReal` lanes.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurfaceLanes<P = Point3, W = f64> {
    control_points: Vec<Vec<P>>,
    weights: Option<Vec<Vec<W>>>,
}

impl<P, W> NurbsSurfaceLanes<P, W> {
    /// The pole grid a source states, with its weight grid when it is
    /// rational.
    #[must_use]
    pub const fn new(control_points: Vec<Vec<P>>, weights: Option<Vec<Vec<W>>>) -> Self {
        Self {
            control_points,
            weights,
        }
    }
}

impl NurbsSurface {
    /// Build a tensor-product NURBS surface with consistent cardinalities.
    ///
    /// Raw pole positions are admitted; admitted positions are kept, so a
    /// producer that holds them tests only the grid's cross-field conditions.
    ///
    /// # Errors
    ///
    /// Refuses a pole count that does not exceed its degree, a knot count that
    /// does not follow from the degree and the pole count, a ragged grid, a
    /// non-finite raw pole coordinate and then a non-finite or decreasing
    /// knot.
    pub fn new<P: PoleValue<FinitePoint3>>(
        u: NurbsSurfaceAxis,
        v: NurbsSurfaceAxis,
        poles: NurbsPoleGrid<P>,
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
        let poles = poles.admit()?;
        let u_knots = KnotVector::new(u_knots)
            .map_err(|error| NurbsError::Structure(format!("u_{error}")))?;
        let v_knots = KnotVector::new(v_knots)
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
    pub fn u_knots(&self) -> &KnotVector {
        &self.u_knots
    }

    /// Full knot vector in v.
    pub fn v_knots(&self) -> &KnotVector {
        &self.v_knots
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
    pub fn from_lanes<P: PoleValue<FinitePoint3>>(
        u: NurbsSurfaceAxis,
        v: NurbsSurfaceAxis,
        lanes: NurbsSurfaceLanes<P>,
        normal_reversed: bool,
    ) -> Result<Self, NurbsError> {
        let NurbsSurfaceLanes {
            control_points,
            weights,
        } = lanes;
        let poles = NurbsPoleGrid::from_lanes(control_points, weights)?;
        Self::new(u, v, poles, normal_reversed)
    }

    /// Build a NURBS surface from a pole grid and an admitted weight grid.
    ///
    /// # Errors
    ///
    /// Refuses a weight grid that does not cover its pole grid and what
    /// [`Self::new`] refuses.
    pub fn from_checked_lanes<P: PoleValue<FinitePoint3>>(
        u: NurbsSurfaceAxis,
        v: NurbsSurfaceAxis,
        lanes: NurbsSurfaceLanes<P, NonZeroReal>,
        normal_reversed: bool,
    ) -> Result<Self, NurbsError> {
        let NurbsSurfaceLanes {
            control_points,
            weights,
        } = lanes;
        let poles = NurbsPoleGrid::from_checked_lanes(control_points, weights)?;
        Self::new(u, v, poles, normal_reversed)
    }

    /// Control grid rows, with the surface's rational form and admitted
    /// positions.
    pub const fn pole_grid(&self) -> &NurbsPoleGrid<FinitePoint3> {
        &self.poles
    }

    /// Control-point rows, outer index u and inner index v.
    #[must_use]
    pub fn control_grid(&self) -> Vec<Vec<FinitePoint3>> {
        self.poles.points()
    }

    /// Control points in u-major order.
    #[must_use]
    pub fn poles(&self) -> Vec<FinitePoint3> {
        self.control_grid().into_iter().flatten().collect()
    }

    /// Pole at grid position `(u, v)`.
    #[must_use]
    pub fn pole(&self, u: usize, v: usize) -> Option<FinitePoint3> {
        match &self.poles {
            NurbsPoleGrid::Polynomial { rows } => rows.get(u)?.get(v).copied(),
            NurbsPoleGrid::Rational { rows } => rows.get(u)?.get(v).map(|pole| pole.point),
        }
    }

    /// Rational weight at grid position `(u, v)`, absent when non-rational.
    pub fn weight(&self, u: usize, v: usize) -> Option<NonZeroReal> {
        match &self.poles {
            NurbsPoleGrid::Polynomial { .. } => None,
            NurbsPoleGrid::Rational { rows } => rows.get(u)?.get(v).map(|pole| pole.weight),
        }
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.to_raw();
        poles.apply_points(edit)?;
        self.poles = poles.admit()?;
        Ok(())
    }

    /// Atomically map every admitted pole position. The closure states its
    /// own refusal, which discards the whole map; the positions it returns are
    /// admitted, so nothing is checked.
    pub fn map_control_points(
        &mut self,
        map: impl FnMut(FinitePoint3) -> Result<FinitePoint3, NurbsError>,
    ) -> Result<(), NurbsError> {
        self.poles = self.poles.clone().try_map_points(map)?;
        Ok(())
    }

    /// Rational weight rows in control-grid order.
    pub fn weights(&self) -> Option<Vec<Vec<NonZeroReal>>> {
        match &self.poles {
            NurbsPoleGrid::Polynomial { .. } => None,
            NurbsPoleGrid::Rational { rows } => Some(
                rows.iter()
                    .map(|row| row.iter().map(|pole| pole.weight).collect())
                    .collect(),
            ),
        }
    }

    /// Rational weights in control-point order.
    pub fn pole_weights(&self) -> Option<Vec<NonZeroReal>> {
        Some(self.weights()?.into_iter().flatten().collect())
    }

    /// Whether the carrier's oriented normal is reversed.
    pub const fn normal_reversed(&self) -> bool {
        self.normal_reversed
    }

    /// Whether the surface is periodic in u.
    pub const fn u_periodic(&self) -> bool {
        self.u_periodic
    }

    /// Whether the surface is periodic in v.
    pub const fn v_periodic(&self) -> bool {
        self.v_periodic
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
    #[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))]
    knots: KnotVector,
    /// Poles in parameter order, with the curve's rational form.
    #[cfg_attr(feature = "schema", schemars(with = "NurbsPoles3"))]
    poles: NurbsPoles3<FinitePoint3>,
    /// Whether the curve is periodic.
    periodic: bool,
}

impl NurbsCurve {
    /// Build a NURBS curve with consistent knot, pole, and weight cardinalities.
    ///
    /// Raw pole positions are admitted; admitted positions are kept, so a
    /// producer that holds them tests only the cardinalities and the knots.
    ///
    /// # Errors
    ///
    /// Refuses a pole or knot count that does not follow from the degree, a
    /// non-finite raw pole coordinate and then a non-finite or decreasing
    /// knot.
    pub fn new<P: PoleValue<FinitePoint3>>(
        degree: u32,
        knots: Vec<f64>,
        poles: NurbsPoles3<P>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        require_curve_cardinality(degree, knots.len(), poles.count(), "control_points")?;
        let poles = poles.admit()?;
        let knots = KnotVector::new(knots)?;
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
    pub fn knots(&self) -> &KnotVector {
        &self.knots
    }

    /// Atomically edit knot values and preserve their invariants.
    pub fn edit_knots(&mut self, edit: impl FnOnce(&mut [f64])) -> Result<(), NurbsError> {
        let mut values = self.knots.to_vec();
        edit(&mut values);
        self.knots = KnotVector::new(values)?;
        Ok(())
    }

    /// Build a NURBS curve from a source's pole lane and weight lane.
    ///
    /// A source that states poles and weights as two arrays pairs them here,
    /// once, at the decode boundary; the curve itself carries pole rows.
    pub fn from_lanes<P: PoleValue<FinitePoint3>>(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<P>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = NurbsPoles3::from_lanes(control_points, weights)?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Build a NURBS curve from a pole lane and an admitted weight lane.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles and what
    /// [`Self::new`] refuses.
    pub fn from_checked_lanes<P: PoleValue<FinitePoint3>>(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<P>,
        weights: Option<Vec<NonZeroReal>>,
        periodic: bool,
    ) -> Result<Self, NurbsError> {
        let poles = NurbsPoles3::from_checked_lanes(control_points, weights)?;
        Self::new(degree, knots, poles, periodic)
    }

    /// Poles in parameter order, with the curve's rational form and admitted
    /// positions.
    pub const fn pole_rows(&self) -> &NurbsPoles3<FinitePoint3> {
        &self.poles
    }

    /// Control points in parameter order.
    #[must_use]
    pub fn control_points(&self) -> Vec<FinitePoint3> {
        self.poles.points()
    }

    /// Number of poles.
    pub fn pole_count(&self) -> usize {
        self.poles.count()
    }

    /// Atomically edit pole positions and preserve finite coordinates.
    ///
    /// The closure states its own refusal, which discards the whole edit.
    pub fn edit_control_points(
        &mut self,
        edit: impl FnMut(&mut Point3) -> Result<(), NurbsError>,
    ) -> Result<(), NurbsError> {
        let mut poles = self.poles.to_raw();
        poles.apply_points(edit)?;
        self.poles = poles.admit()?;
        Ok(())
    }

    /// Atomically map every admitted pole position. The closure states its
    /// own refusal, which discards the whole map; the positions it returns are
    /// admitted, so nothing is checked.
    pub fn map_control_points(
        &mut self,
        map: impl FnMut(FinitePoint3) -> Result<FinitePoint3, NurbsError>,
    ) -> Result<(), NurbsError> {
        self.poles = self.poles.clone().try_map_points(map)?;
        Ok(())
    }

    /// Rational weights in pole order.
    pub fn weights(&self) -> Option<Vec<NonZeroReal>> {
        match &self.poles {
            NurbsPoles3::Polynomial { .. } => None,
            NurbsPoles3::Rational { points } => {
                Some(points.iter().map(|pole| pole.weight).collect())
            }
        }
    }

    /// Whether the curve is periodic.
    pub const fn periodic(&self) -> bool {
        self.periodic
    }

    /// Reverse poles, weights, and the signed knot parameterization together.
    pub fn reverse_parameterization(&mut self) {
        self.poles.reverse();
        self.knots.reverse_negated();
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
