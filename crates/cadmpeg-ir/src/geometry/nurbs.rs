// SPDX-License-Identifier: Apache-2.0
//! NURBS curves, surfaces, pole layouts, and knot invariants.

/// Homogeneous Bezier extraction and boundary certificates.
pub mod bezier;
/// Rational control-polygon speed bounds.
pub mod bounds;
/// Immutable temporary rows with scoped storage ownership.
pub mod scoped;
pub(crate) mod scratch;

pub(super) mod admitted;

use crate::features::FinitePoint3;
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonZeroReal};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn copy_decode_grid<T: Copy>(
    rows: &[Vec<T>],
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<Vec<T>>, CodecError> {
    ctx.try_collect_retained_with(rows, operation, |row| {
        super::copy_decode_slice(row, ctx, operation)
    })
}

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
    pub fn new(ctx: &DecodeContext<'_>, knots: Vec<f64>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(build_raw_knots(ctx, knots, ""))
    }

    /// Build a knot vector from finite values. Only their order is checked.
    pub fn from_finite_lanes(ctx: &DecodeContext<'_>, knots: Vec<FiniteReal>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(build_finite_knots(ctx, knots, ""))
    }

    /// Move the admitted knot storage into its owner without copying it.
    pub fn into_values(self) -> Vec<f64> {
        self.0
    }

    /// Borrow the knot values.
    #[must_use]
    pub fn as_slice(&self) -> &[f64] {
        &self.0
    }

    /// Copy admitted knots through the decode collection and retained-byte budgets.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self(ctx.copy_retained_slice(&self.0, operation)?))
    }

    /// Reverse the order and negate every value, the knots of the reversed
    /// parameterization. Negation turns a non-decreasing sequence into a
    /// non-increasing one, and the reversal restores the order, so the
    /// result stays admitted.
    pub(super) fn reverse_negated(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.0.len() / 2), "IR signed knot reversal")?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.0.len()), "IR signed knot negation")?;
        self.0.reverse();
        for knot in &mut self.0 {
            *knot = -*knot;
        }
        Ok(())
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

mod knot_value_sealed {
    pub trait Sealed {}

    impl Sealed for Vec<f64> {}
    impl Sealed for Vec<crate::scalar::FiniteReal> {}
    impl Sealed for super::KnotVector {}
}

/// A raw or admitted knot lane passed into a NURBS constructor.
///
/// Only a raw vector or an admitted [`KnotVector`] can supply this lane.
pub trait KnotValue: knot_value_sealed::Sealed {
    /// Number of knots before cardinality validation.
    fn knot_count(&self) -> usize;
    /// Admit raw knots or keep an admitted knot vector.
    fn admit<E>(self,
        raw: impl FnOnce(Vec<f64>) -> Result<KnotVector, E>,
        finite: impl FnOnce(Vec<FiniteReal>) -> Result<KnotVector, E>,
    ) -> Result<KnotVector, E>;
}

impl KnotValue for Vec<f64> {
    fn knot_count(&self) -> usize {
        Vec::len(self)
    }

    fn admit<E>(self,
        raw: impl FnOnce(Vec<f64>) -> Result<KnotVector, E>,
        _finite: impl FnOnce(Vec<FiniteReal>) -> Result<KnotVector, E>,
    ) -> Result<KnotVector, E> {
        raw(self)
    }
}

impl KnotValue for Vec<FiniteReal> {
    fn knot_count(&self) -> usize {
        Vec::len(self)
    }

    fn admit<E>(self,
        _raw: impl FnOnce(Vec<f64>) -> Result<KnotVector, E>,
        finite: impl FnOnce(Vec<FiniteReal>) -> Result<KnotVector, E>,
    ) -> Result<KnotVector, E> {
        finite(self)
    }
}

impl KnotValue for KnotVector {
    fn knot_count(&self) -> usize {
        self.0.len()
    }

    fn admit<E>(self,
        _raw: impl FnOnce(Vec<f64>) -> Result<KnotVector, E>,
        _finite: impl FnOnce(Vec<FiniteReal>) -> Result<KnotVector, E>,
    ) -> Result<KnotVector, E> {
        Ok(self)
    }
}

fn build_raw_knots<S: NurbsAdmission>(admission: &S, knots: Vec<f64>, prefix: &str) -> Result<KnotVector, S::Error> {
    require_nondecreasing_knots(admission, &knots, prefix)?;
    Ok(KnotVector(knots))
}

fn build_finite_knots<S: NurbsAdmission>(admission: &S, knots: Vec<FiniteReal>, prefix: &str) -> Result<KnotVector, S::Error> {
    let values = admission.collect(knots, "IR finite knot values", |value| Ok(value.get()))?;
    require_knot_order(admission, &values, prefix)?;
    Ok(KnotVector(values))
}

pub(super) fn admit_knots<S: NurbsAdmission, K: KnotValue>(admission: &S, knots: K, prefix: &str) -> Result<KnotVector, S::Error> {
    knots.admit(|values| build_raw_knots(admission, values, prefix),
        |values| build_finite_knots(admission, values, prefix))
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

/// A pole value a producer hands a NURBS store: a raw value, which the store
/// admits in its own refusal order, or an admitted value, which it keeps.
pub trait PoleValue<T>: Copy {
    /// Whether pole admission retains the input vector storage.
    const RETAINS_POLE_STORAGE: bool = false;
    /// The admitted value, absent when a raw value is not finite.
    fn admit(self) -> Option<T>;

    /// Admit a curve lane through its explicit conversion policy.
    /// An implementation may keep storage whose positions are already admitted.
    fn admit_curve_poles<E>(
        poles: NurbsPoles3<Self>,
        convert: impl FnOnce(NurbsPoles3<Self>) -> Result<NurbsPoles3<T>, E>,
    ) -> Result<NurbsPoles3<T>, E> {
        convert(poles)
    }

    /// Admit a parameter-space lane through its explicit conversion policy.
    /// An implementation may keep storage whose positions are already admitted.
    fn admit_pcurve_poles<E>(
        poles: super::pcurve::PcurveNurbsPoles<Self>,
        convert: impl FnOnce(super::pcurve::PcurveNurbsPoles<Self>) -> Result<super::pcurve::PcurveNurbsPoles<T>, E>,
    ) -> Result<super::pcurve::PcurveNurbsPoles<T>, E> {
        convert(poles)
    }

    /// Admit a surface grid through its explicit conversion policy.
    /// An implementation may keep storage whose positions are already admitted.
    fn admit_surface_poles<E>(
        grid: NurbsPoleGrid<Self>,
        convert: impl FnOnce(NurbsPoleGrid<Self>) -> Result<NurbsPoleGrid<T>, E>,
    ) -> Result<NurbsPoleGrid<T>, E> {
        convert(grid)
    }
}

impl PoleValue<FinitePoint3> for Point3 {
    fn admit(self) -> Option<FinitePoint3> {
        FinitePoint3::new(self)
    }
}

impl PoleValue<FinitePoint3> for FinitePoint3 {
    const RETAINS_POLE_STORAGE: bool = true;
    fn admit(self) -> Option<FinitePoint3> {
        Some(self)
    }

    fn admit_curve_poles<E>(
        poles: NurbsPoles3<Self>,
        _convert: impl FnOnce(NurbsPoles3<Self>) -> Result<NurbsPoles3<FinitePoint3>, E>,
    ) -> Result<NurbsPoles3<FinitePoint3>, E> {
        Ok(poles)
    }

    fn admit_surface_poles<E>(
        grid: NurbsPoleGrid<Self>,
        _convert: impl FnOnce(NurbsPoleGrid<Self>) -> Result<NurbsPoleGrid<FinitePoint3>, E>,
    ) -> Result<NurbsPoleGrid<FinitePoint3>, E> {
        Ok(grid)
    }
}

/// Explicit storage, work and diagnostic policy for NURBS admission.
pub(crate) trait NurbsAdmission {
    type Error: From<NurbsError>;

    fn collect<I, T>(
        &self,
        values: Vec<I>,
        operation: &'static str,
        convert: impl FnMut(I) -> Result<T, Self::Error>,
    ) -> Result<Vec<T>, Self::Error>;

    fn reserve<T>(&self, values: &mut Vec<T>, storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'_>>, operation: &'static str) -> Result<(), Self::Error>;

    fn copy_field(&self, field: &str) -> Result<String, Self::Error>;

    fn work(&self, count: u64, operation: &'static str) -> Result<(), Self::Error>;

    fn structure(&self, message: std::fmt::Arguments<'_>) -> Result<Self::Error, Self::Error>;

}

pub(crate) struct StandardNurbsAdmission;

impl NurbsAdmission for StandardNurbsAdmission {
    type Error = NurbsError;

    fn collect<I, T>(
        &self,
        values: Vec<I>,
        _operation: &'static str,
        mut convert: impl FnMut(I) -> Result<T, Self::Error>,
    ) -> Result<Vec<T>, Self::Error> {
        let mut output = Vec::new();
        scratch::reserve_exact(&mut output, values.len(), "reconstruct NURBS poles")?;
        for value in values {
            output.push(convert(value)?);
        }
        Ok(output)
    }

    fn reserve<T>(&self, values: &mut Vec<T>, _storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'_>>, operation: &'static str) -> Result<(), Self::Error> {
        values.try_reserve(1).map_err(|_| scratch::allocation_refusal(1, operation).into())
    }

    fn copy_field(&self, field: &str) -> Result<String, Self::Error> {
        Ok(field.to_owned())
    }

    fn work(&self, _count: u64, _operation: &'static str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn structure(&self, message: std::fmt::Arguments<'_>) -> Result<Self::Error, Self::Error> {
        Ok(NurbsError::Structure(message.to_string()))
    }
}

fn map_pole<P: PoleValue<T>, T, S: NurbsAdmission>(
    storage: &S,
    point: P,
) -> Result<T, S::Error> {
    match point.admit() {
        Some(point) => Ok(point),
        None => Err(storage.structure(format_args!("control_points contains a non-finite point"))?),
    }
}

fn map_curve_poles<P: PoleValue<T>, T, S: NurbsAdmission>(
    storage: &S,
    poles: NurbsPoles3<P>,
) -> Result<NurbsPoles3<T>, S::Error> {
    Ok(match poles {
        NurbsPoles3::Polynomial { points } => NurbsPoles3::Polynomial {
            points: storage.collect(points, "IR NURBS admitted poles", |point| map_pole(storage, point))?,
        },
        NurbsPoles3::Rational { points } => NurbsPoles3::Rational {
            points: storage.collect(points, "IR NURBS admitted poles", |pole| Ok(WeightedPole3 {
                point: map_pole(storage, pole.point)?,
                weight: pole.weight,
            }))?,
        },
    })
}

fn map_surface_poles<P: PoleValue<T>, T, S: NurbsAdmission>(
    storage: &S,
    grid: NurbsPoleGrid<P>,
) -> Result<NurbsPoleGrid<T>, S::Error> {
    Ok(match grid {
        NurbsPoleGrid::Polynomial { rows } => NurbsPoleGrid::Polynomial {
            rows: storage.collect(rows, "IR NURBS admitted grid rows", |row| {
                storage.collect(row, "IR NURBS admitted poles", |point| map_pole(storage, point))
            })?,
        },
        NurbsPoleGrid::Rational { rows } => NurbsPoleGrid::Rational {
            rows: storage.collect(rows, "IR NURBS admitted grid rows", |row| {
                storage.collect(row, "IR NURBS admitted poles", |pole| Ok(WeightedPole3 {
                    point: map_pole(storage, pole.point)?,
                    weight: pole.weight,
                }))
            })?,
        },
    })
}

/// Pair each pole with its weight through the caller's storage and work policy.
fn weighted_poles<P, W, E>(
    points: Vec<P>,
    weights: Vec<W>,
    mut reserve: impl FnMut(&mut Vec<WeightedPole3<P>>) -> Result<(), E>,
    mut work: impl FnMut() -> Result<(), E>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, E>,
) -> Result<Vec<WeightedPole3<P>>, E> {
    let mut output = Vec::new();
    for (index, (point, value)) in points.into_iter().zip(weights).enumerate() {
        reserve(&mut output)?;
        work()?;
        output.push(WeightedPole3 {
            point,
            weight: weight(index, value)?,
        });
    }
    Ok(output)
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
    pub fn from_lanes(ctx: &DecodeContext<'_>, points: Vec<P>, weights: Option<Vec<f64>>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(pair_curve_lanes(ctx, points, weights, &mut None, |index, weight| admit_weight(ctx, "poles", index, weight)))
    }

    /// Pair poles with finite weights, checking only lane length and nonzero weights.
    pub fn from_finite_lanes(ctx: &DecodeContext<'_>, points: Vec<P>, weights: Option<Vec<FiniteReal>>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(pair_curve_lanes(ctx, points, weights, &mut None, |index, weight| admit_finite_weight(ctx, "poles", index, weight)))
    }

    /// Pair a pole lane with an admitted weight lane. The weight type states
    /// the weight contract; a store admits the positions it is handed.
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, naming both
    /// counts.
    pub fn from_checked_lanes(ctx: &DecodeContext<'_>, points: Vec<P>, weights: Option<Vec<NonZeroReal>>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(pair_curve_lanes(ctx, points, weights, &mut None, |_, weight| Ok(weight)))
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
    /// One borrowed pole position in parameter order.
    #[must_use]
    pub fn point_at(&self, index: usize) -> Option<P> {
        match self {
            Self::Polynomial { points } => points.get(index).copied(),
            Self::Rational { points } => points.get(index).map(|pole| pole.point),
        }
    }

    /// One rational weight, absent for a polynomial curve or invalid index.
    #[must_use]
    pub fn weight_at(&self, index: usize) -> Option<f64> {
        match self {
            Self::Polynomial { .. } => None,
            Self::Rational { points } => points.get(index).map(|pole| pole.weight.get()),
        }
    }

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

pub(crate) fn pair_curve_lanes<P, W, S: NurbsAdmission>(
    admission: &S,
    points: Vec<P>,
    weights: Option<Vec<W>>,
    storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'_>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, S::Error>,
) -> Result<NurbsPoles3<P>, S::Error> {
    let Some(weights) = weights else { return Ok(NurbsPoles3::Polynomial { points }); };
    require_weight_lane(admission, "poles", points.len(), weights.len())?;
    Ok(NurbsPoles3::Rational { points: weighted_poles(points, weights,
        |values| admission.reserve(values, storage, "IR NURBS paired poles"),
        || admission.work(1, "IR NURBS paired poles"), &mut weight)? })
}

fn pair_grid_lanes<P, W, S: NurbsAdmission>(
    admission: &S,
    rows: Vec<Vec<P>>,
    weights: Option<Vec<Vec<W>>>,
    storage: &mut Option<cadmpeg_core::decode::ScopedReservation<'_>>,
    mut weight: impl FnMut(usize, W) -> Result<NonZeroReal, S::Error>,
) -> Result<NurbsPoleGrid<P>, S::Error> {
    let Some(weights) = weights else { return Ok(NurbsPoleGrid::Polynomial { rows }); };
    require_weight_lane(admission, "pole grid", rows.len(), weights.len())?;
    let mut output = Vec::new();
    for (row, weights) in rows.into_iter().zip(weights) {
        admission.work(1, "IR NURBS paired grid rows")?;
        admission.reserve(&mut output, storage, "IR NURBS paired grid rows")?;
        require_weight_lane(admission, "pole grid row", row.len(), weights.len())?;
        output.push(weighted_poles(row, weights,
            |values| admission.reserve(values, storage, "IR NURBS paired poles"),
            || admission.work(1, "IR NURBS paired poles"), &mut weight)?);
    }
    Ok(NurbsPoleGrid::Rational { rows: output })
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
    pub fn from_lanes(ctx: &DecodeContext<'_>, rows: Vec<Vec<P>>, weights: Option<Vec<Vec<f64>>>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(pair_grid_lanes(ctx, rows, weights, &mut None, |index, weight| admit_weight(ctx, "pole grid row", index, weight)))
    }

    /// Pair a pole grid with finite weights, checking grid shape and nonzero weights.
    pub fn from_finite_lanes(ctx: &DecodeContext<'_>, rows: Vec<Vec<P>>, weights: Option<Vec<Vec<FiniteReal>>>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(pair_grid_lanes(ctx, rows, weights, &mut None, |index, weight| admit_finite_weight(ctx, "pole grid row", index, weight)))
    }

    /// Pair a pole grid with an admitted weight grid. The weight type states
    /// the weight contract; a store admits the positions it is handed.
    ///
    /// # Errors
    ///
    /// Refuses a weight grid that does not cover the pole grid, row count or
    /// row width, naming both counts.
    pub fn from_checked_lanes(ctx: &DecodeContext<'_>, rows: Vec<Vec<P>>, weights: Option<Vec<Vec<NonZeroReal>>>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(pair_grid_lanes(ctx, rows, weights, &mut None, |_, weight| Ok(weight)))
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
    /// Copy the retained knot vectors and control grid through the decode budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(1, operation)?;
        Ok(Self {
            u_degree: self.u_degree,
            v_degree: self.v_degree,
            u_knots: self.u_knots.try_clone_for_decode(ctx, operation)?,
            v_knots: self.v_knots.try_clone_for_decode(ctx, operation)?,
            control_points: copy_decode_grid(&self.control_points, ctx, operation)?,
        })
    }

    /// Build a rectangular grid with full knot vectors for both parameters.
    pub fn new(
        ctx: &DecodeContext<'_>,
        u_degree: u32,
        v_degree: u32,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
        control_points: Vec<Vec<Point3>>,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(build_bspline_surface(ctx, u_degree, v_degree, u_knots, v_knots, control_points))
    }

    /// Degree in the first parameter.
    pub const fn u_degree(&self) -> u32 {
        self.u_degree
    }

    /// Degree in the second parameter.
    pub const fn v_degree(&self) -> u32 {
        self.v_degree
    }

    /// Map every pole position in row-major order, or change nothing.
    ///
    /// `map` receives each pole's row-major index and position. The first
    /// refusal returns before any pole changes; otherwise `map` runs again for
    /// every pole and the results are written in place. Nothing is allocated.
    pub fn try_map_control_points<E>(
        &mut self,
        map: impl Fn(usize, FinitePoint3) -> Result<FinitePoint3, E>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), E>, CodecError> {
        let count = cadmpeg_core::decode::u64_from_index(self.control_points.len()).checked_mul(cadmpeg_core::decode::u64_from_index(self.control_points.first().map_or(0, Vec::len))).ok_or_else(|| ctx.refuse_codec_limit("IR pole edit work", u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(count, "IR pole edit validation")?;
        ctx.charge_work(count, "IR pole edit mutation")?;
        Ok((|| {
        for (index, point) in self.control_points.iter().flatten().copied().enumerate() {
            map(index, point)?;
        }
        for (index, point) in self.control_points.iter_mut().flatten().enumerate() {
            *point = map(index, *point)?;
        }
        Ok(())
            })())
    }
}

fn build_bspline_surface<S: NurbsAdmission>(
    admission: &S,
    u_degree: u32,
    v_degree: u32,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    control_points: Vec<Vec<Point3>>,
) -> Result<BsplineSurface, S::Error> {
    let u_count = control_points.len();
    let v_count = control_points.first().map_or(0, Vec::len);
    let u_knots = bspline_axis_knots(admission, "u", "u_knots", u_degree, u_count, u_knots)?;
    let v_knots = bspline_axis_knots(admission, "v", "v_knots", v_degree, v_count, v_knots)?;
    require_rectangular_grid(admission, "control_points", &control_points)?;
    let control_points = admission.collect(control_points, "IR admitted B-spline grid rows", |row| {
        admission.collect(row, "IR admitted B-spline grid poles", |point| map_pole(admission, point))
    })?;
    Ok(BsplineSurface { u_degree, v_degree, u_knots, v_knots, control_points })
}

/// Admit one B-spline axis: more poles than its degree, the full knot count
/// for them, and finite non-decreasing knots, refused in that order.
fn bspline_axis_knots<S: NurbsAdmission>(
    admission: &S,
    axis: &str,
    knot_field: &str,
    degree: u32,
    count: usize,
    knots: Vec<f64>,
) -> Result<KnotVector, S::Error> {
    if count <= cadmpeg_core::decode::index_from_u32(degree) {
        return Err(admission.structure(format_args!(
            "control_points {axis} count must exceed degree {degree}, found {count}"
        ))?);
    }
    require_length(admission, knot_field, knots.len(),
        checked_knot_count(admission, axis, count, degree)?)?;
    build_raw_knots(admission, knots, "")
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
        build_bspline_surface(
            &StandardNurbsAdmission,
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
    /// Storage for a knot or pole carrier was refused.
    #[error("resource limit: {0:?}")]
    ResourceLimit(cadmpeg_core::decode::ResourceLimit),
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

impl From<cadmpeg_core::decode::ResourceLimit> for NurbsError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}

impl From<NurbsError> for cadmpeg_core::CodecError {
    fn from(error: NurbsError) -> Self {
        match error {
            NurbsError::ResourceLimit(limit) => Self::ResourceLimit(limit),
            error => Self::Malformed(error.to_string()),
        }
    }
}

fn checked_knot_count<S: NurbsAdmission>(
    admission: &S,
    field: &str,
    pole_count: usize,
    degree: u32,
) -> Result<usize, S::Error> {
    let Ok(degree) = usize::try_from(degree) else {
        return Err(admission.structure(format_args!("{field} knot count overflows usize"))?);
    };
    match pole_count.checked_add(degree).and_then(|count| count.checked_add(1)) {
        Some(count) => Ok(count),
        None => Err(admission.structure(format_args!("{field} knot count overflows usize"))?),
    }
}

/// Check every row width in source order through the caller's work policy.
fn require_rectangular_grid<T, S: NurbsAdmission>(
    admission: &S,
    field: &str,
    rows: &[Vec<T>],
) -> Result<(), S::Error> {
    let width = rows.first().map_or(0, Vec::len);
    for row in rows {
        admission.work(1, "IR NURBS grid row shape")?;
        if row.len() != width {
            return Err(admission.structure(format_args!(
                "{field} row must contain {width} values, found {}", row.len(),
            ))?);
        }
    }
    Ok(())
}

fn require_length<S: NurbsAdmission>(
    admission: &S,
    field: &str,
    actual: usize,
    expected: usize,
) -> Result<(), S::Error> {
    if actual == expected {
        Ok(())
    } else {
        Err(admission.structure(format_args!(
            "{field} must contain {expected} values, found {actual}"
        ))?)
    }
}

/// A weight lane covers the pole lane it belongs to.
pub(super) fn require_weight_lane<S: NurbsAdmission>(
    admission: &S,
    field: &str,
    poles: usize,
    weights: usize,
) -> Result<(), S::Error> {
    if poles == weights {
        Ok(())
    } else {
        Err(NurbsError::WeightLaneLength {
            field: admission.copy_field(field)?,
            poles,
            weights,
        }.into())
    }
}

/// Admit one weight a source states, naming its index within its lane.
pub(crate) fn admit_weight<S: NurbsAdmission>(
    admission: &S,
    field: &str,
    index: usize,
    weight: f64,
) -> Result<NonZeroReal, S::Error> {
    if let Some(admitted) = NonZeroReal::new(weight) { return Ok(admitted); }
    Err(NurbsError::UnusableWeight { field: admission.copy_field(field)?, index, weight }.into())
}

/// Check the nonzero condition of a weight whose finiteness is already admitted.
pub(super) fn admit_finite_weight<S: NurbsAdmission>(
    admission: &S,
    field: &str,
    index: usize,
    weight: FiniteReal,
) -> Result<NonZeroReal, S::Error> {
    if let Some(admitted) = NonZeroReal::from_finite(weight) { return Ok(admitted); }
    Err(NurbsError::UnusableWeight { field: admission.copy_field(field)?, index, weight: weight.get() }.into())
}

fn require_finite_scalars<S: NurbsAdmission>(
    admission: &S,
    prefix: &str,
    values: &[f64],
) -> Result<(), S::Error> {
    admission.work(cadmpeg_core::decode::u64_from_index(values.len()), "IR NURBS knot finiteness")?;
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(admission.structure(format_args!("{prefix}knots contains a non-finite value"))?)
    }
}

fn require_knot_order<S: NurbsAdmission>(
    admission: &S,
    knots: &[f64],
    prefix: &str,
) -> Result<(), S::Error> {
    admission.work(cadmpeg_core::decode::u64_from_index(knots.len()), "IR NURBS knot order")?;
    if knots_nondecreasing(knots) {
        Ok(())
    } else {
        Err(admission.structure(format_args!("{prefix}knots must be non-decreasing"))?)
    }
}

fn require_nondecreasing_knots<S: NurbsAdmission>(
    admission: &S,
    knots: &[f64],
    prefix: &str,
) -> Result<(), S::Error> {
    require_finite_scalars(admission, prefix, knots)?;
    require_knot_order(admission, knots, prefix)
}

pub(super) fn require_curve_cardinality<S: NurbsAdmission>(
    admission: &S,
    degree: u32,
    knot_count: usize,
    pole_count: usize,
    point_field: &str,
) -> Result<(), S::Error> {
    if u64::try_from(pole_count).is_ok_and(|count| count <= u64::from(degree)) {
        return Err(admission.structure(format_args!(
            "{point_field} must contain more than degree {degree} poles, found {pole_count}"
        ))?);
    }
    require_length(admission, "knots", knot_count,
        checked_knot_count(admission, "curve", pole_count, degree)?)
}

/// One parameter axis of a tensor-product NURBS surface.
///
/// A degree, its knot vector and its periodicity are one statement about one
/// axis: the knot count a source may state depends on the degree, and the
/// periodicity describes that same knot vector. They travel together.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurfaceAxis<K = Vec<f64>> {
    degree: u32,
    knots: K,
    periodic: bool,
}

impl<K> NurbsSurfaceAxis<K> {
    /// One axis of a surface: its degree, its knot vector and its periodicity.
    #[must_use]
    pub const fn new(degree: u32, knots: K, periodic: bool) -> Self {
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

pub(crate) fn build_curve<P: PoleValue<FinitePoint3>, K: KnotValue, S: NurbsAdmission>(
    admission: &S,
    degree: u32,
    knots: K,
    poles: NurbsPoles3<P>,
    periodic: bool,
) -> Result<NurbsCurve, S::Error> {
    require_curve_cardinality(admission, degree, knots.knot_count(), poles.count(), "control_points")?;
    let poles = P::admit_curve_poles(poles, |poles| map_curve_poles(admission, poles))?;
    let knots = admit_knots(admission, knots, "")?;
    Ok(NurbsCurve { degree, knots, poles, periodic })
}

fn build_surface<P: PoleValue<FinitePoint3>, U: KnotValue, V: KnotValue, S: NurbsAdmission>(
    admission: &S,
    u: NurbsSurfaceAxis<U>,
    v: NurbsSurfaceAxis<V>,
    poles: NurbsPoleGrid<P>,
    normal_reversed: bool,
) -> Result<NurbsSurface, S::Error> {
    let NurbsSurfaceAxis { degree: u_degree, knots: u_knots, periodic: u_periodic } = u;
    let NurbsSurfaceAxis { degree: v_degree, knots: v_knots, periodic: v_periodic } = v;
    require_surface_shape(admission, u_degree, u_knots.knot_count(), v_degree, v_knots.knot_count(), &poles)?;
    let poles = P::admit_surface_poles(poles, |poles| map_surface_poles(admission, poles))?;
    let u_knots = admit_knots(admission, u_knots, "u_")?;
    let v_knots = admit_knots(admission, v_knots, "v_")?;
    Ok(NurbsSurface { u_degree, v_degree, u_knots, v_knots, poles, normal_reversed, u_periodic, v_periodic })
}

fn require_surface_shape<P, S: NurbsAdmission>(
    admission: &S,
    u_degree: u32,
    u_knots: usize,
    v_degree: u32,
    v_knots: usize,
    poles: &NurbsPoleGrid<P>,
) -> Result<(), S::Error> {
    let u_count = poles.u_count();
    let v_count = poles.v_count();
    if u64::try_from(u_count).is_ok_and(|count| count <= u64::from(u_degree)) {
        return Err(admission.structure(format_args!(
            "u_count must exceed u_degree {u_degree}, found {u_count}"
        ))?);
    }
    if u64::try_from(v_count).is_ok_and(|count| count <= u64::from(v_degree)) {
        return Err(admission.structure(format_args!(
            "v_count must exceed v_degree {v_degree}, found {v_count}"
        ))?);
    }
    require_length(admission, "u_knots", u_knots,
        checked_knot_count(admission, "u", u_count, u_degree)?)?;
    require_length(admission, "v_knots", v_knots,
        checked_knot_count(admission, "v", v_count, v_degree)?)?;
    match poles {
        NurbsPoleGrid::Polynomial { rows } => require_rectangular_grid(admission, "control_points", rows)?,
        NurbsPoleGrid::Rational { rows } => require_rectangular_grid(admission, "control_points", rows)?,
    }
    Ok(())
}

impl NurbsSurface {
    /// Copy the admitted lanes through the decode collection budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let u_knots = self.u_knots.try_clone_for_decode(ctx, operation)?;
        let v_knots = self.v_knots.try_clone_for_decode(ctx, operation)?;
        let poles = match &self.poles {
            NurbsPoleGrid::Polynomial { rows } => NurbsPoleGrid::Polynomial {
                rows: copy_decode_grid(rows, ctx, operation)?,
            },
            NurbsPoleGrid::Rational { rows } => NurbsPoleGrid::Rational {
                rows: copy_decode_grid(rows, ctx, operation)?,
            },
        };
        Ok(Self {
            u_degree: self.u_degree,
            v_degree: self.v_degree,
            u_knots,
            v_knots,
            poles,
            normal_reversed: self.normal_reversed,
            u_periodic: self.u_periodic,
            v_periodic: self.v_periodic,
        })
    }

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
    pub fn new<P: PoleValue<FinitePoint3>, U: KnotValue, V: KnotValue>(
        ctx: &DecodeContext<'_>,
        u: NurbsSurfaceAxis<U>,
        v: NurbsSurfaceAxis<V>,
        poles: NurbsPoleGrid<P>,
        normal_reversed: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(build_surface(ctx, u, v, poles, normal_reversed))
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

    /// Build from finite knots, poles, and weights. Only relationships and
    /// the nonzero weight condition are checked.
    pub fn from_finite_lanes(
        ctx: &DecodeContext<'_>,
        u: NurbsSurfaceAxis<Vec<FiniteReal>>,
        v: NurbsSurfaceAxis<Vec<FiniteReal>>,
        lanes: NurbsSurfaceLanes<FinitePoint3, FiniteReal>,
        normal_reversed: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish((|| {
        let mut storage = None;
        let NurbsSurfaceLanes {
            control_points,
            weights,
        } = lanes;
        let poles = pair_grid_lanes(ctx, control_points, weights, &mut storage, |index, weight| admit_finite_weight(ctx, "pole grid row", index, weight))?;
        build_surface(ctx, u, v, poles, normal_reversed)
        })())
    }

    /// Build a NURBS surface from knot axes, a pole grid and an admitted weight grid.
    /// Admitted knot vectors are kept; raw knots are admitted by [`Self::new`].
    ///
    /// # Errors
    ///
    /// Refuses a weight grid that does not cover its pole grid, invalid
    /// cardinalities, a non-finite raw pole coordinate, or a non-finite or
    /// decreasing raw knot.
    pub fn from_checked_lanes<P: PoleValue<FinitePoint3>, U: KnotValue, V: KnotValue>(
        ctx: &DecodeContext<'_>,
        u: NurbsSurfaceAxis<U>,
        v: NurbsSurfaceAxis<V>,
        lanes: NurbsSurfaceLanes<P, NonZeroReal>,
        normal_reversed: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish((|| {
        let mut storage = if lanes.weights.is_some() && !P::RETAINS_POLE_STORAGE { Some(ctx.reserve_scoped(0, "IR NURBS paired grid rows")?) } else { None };
        let NurbsSurfaceLanes {
            control_points,
            weights,
        } = lanes;
        let poles = pair_grid_lanes(ctx, control_points, weights, &mut storage, |_, weight| Ok(weight))?;
        build_surface(ctx, u, v, poles, normal_reversed)
        })())
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

    /// Map pole positions in row-major order after every result passes admission.
    /// The map must return the same result for the same index and position.
    /// Pole weights and knots stay in place.
    pub fn try_map_control_points<E>(
        &mut self,
        map: impl Fn(usize, FinitePoint3) -> Result<FinitePoint3, E>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), E>, CodecError> {
        let count = cadmpeg_core::decode::u64_from_index(self.u_count()).checked_mul(cadmpeg_core::decode::u64_from_index(self.v_count())).ok_or_else(|| ctx.refuse_codec_limit("IR pole edit work", u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(count, "IR pole edit validation")?;
        ctx.charge_work(count, "IR pole edit mutation")?;
        Ok((|| {
        match &self.poles {
            NurbsPoleGrid::Polynomial { rows } => {
                for (index, point) in rows.iter().flatten().copied().enumerate() {
                    map(index, point)?;
                }
            }
            NurbsPoleGrid::Rational { rows } => {
                for (index, pole) in rows.iter().flatten().enumerate() {
                    map(index, pole.point)?;
                }
            }
        }
        match &mut self.poles {
            NurbsPoleGrid::Polynomial { rows } => {
                for (index, point) in rows.iter_mut().flatten().enumerate() {
                    *point = map(index, *point)?;
                }
            }
            NurbsPoleGrid::Rational { rows } => {
                for (index, pole) in rows.iter_mut().flatten().enumerate() {
                    pole.point = map(index, pole.point)?;
                }
            }
        }
        Ok(())
            })())
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
    pub fn transpose_parameter_axes(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        // Surface admission and every grid mutation preserve nonempty,
        // rectangular rows. Raw pole grids do not expose this operation.
        fn transpose<T: Copy>(ctx: &DecodeContext<'_>, rows: &[Vec<T>], width: usize) -> Result<Vec<Vec<T>>, CodecError> {
            ctx.try_collect_retained_with(0..width, "IR NURBS transposed grid rows", |column| {
                ctx.try_collect_retained_with(rows, "IR NURBS transposed grid poles", |row| Ok(row[column]))
            })
        }

        let width = self.v_count();
        match &mut self.poles {
            NurbsPoleGrid::Polynomial { rows } => *rows = transpose(ctx, rows, width)?,
            NurbsPoleGrid::Rational { rows } => *rows = transpose(ctx, rows, width)?,
        }
        std::mem::swap(&mut self.u_degree, &mut self.v_degree);
        std::mem::swap(&mut self.u_knots, &mut self.v_knots);
        std::mem::swap(&mut self.u_periodic, &mut self.v_periodic);
        Ok(())
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
        build_surface(&StandardNurbsAdmission,
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

    /// Copy the admitted lanes through the decode collection budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let knots = self.knots.try_clone_for_decode(ctx, operation)?;
        let poles = match &self.poles {
            NurbsPoles3::Polynomial { points } => NurbsPoles3::Polynomial {
                points: super::copy_decode_slice(points, ctx, operation)?,
            },
            NurbsPoles3::Rational { points } => NurbsPoles3::Rational {
                points: super::copy_decode_slice(points, ctx, operation)?,
            },
        };
        Ok(Self {
            degree: self.degree,
            knots,
            poles,
            periodic: self.periodic,
        })
    }

    /// Copy a curve after charging its knot and pole lanes, then map the
    /// copied positions without another allocation. A non-finite result leaves
    /// the source untouched and returns no curve.
    pub fn map_control_points(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
        mut map: impl FnMut(Point3) -> Point3,
    ) -> Result<Option<Self>, CodecError> {
        let mut mapped = self.try_clone_for_decode(ctx, operation)?;
        match &mut mapped.poles {
            NurbsPoles3::Polynomial { points } => {
                for point in points {
                    ctx.charge_work(1, operation)?;
                    let Some(next) = FinitePoint3::new(map(point.get())) else {
                        return Ok(None);
                    };
                    *point = next;
                }
            }
            NurbsPoles3::Rational { points } => {
                for pole in points {
                    ctx.charge_work(1, operation)?;
                    let Some(next) = FinitePoint3::new(map(pole.point.get())) else {
                        return Ok(None);
                    };
                    pole.point = next;
                }
            }
        }
        Ok(Some(mapped))
    }

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
    pub fn new<P: PoleValue<FinitePoint3>, K: KnotValue>(
        ctx: &DecodeContext<'_>,
        degree: u32,
        knots: K,
        poles: NurbsPoles3<P>,
        periodic: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish(build_curve(ctx, degree, knots, poles, periodic))
    }

    /// Curve degree.
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Full knot vector.
    pub fn knots(&self) -> &KnotVector {
        &self.knots
    }

    /// Move the curve lanes to a caller that will rebuild a checked curve.
    pub fn into_parts(self) -> (u32, KnotVector, NurbsPoles3<FinitePoint3>, bool) {
        (self.degree, self.knots, self.poles, self.periodic)
    }

    /// Atomically edit knot values and preserve their invariants.
    pub fn edit_knots(&mut self, ctx: &DecodeContext<'_>, edit: impl FnOnce(&mut [f64])) -> Result<Result<(), NurbsError>, CodecError> {
        admitted::finish((|| {
            let (mut values, storage) = ctx.copy_temporary_slice(self.knots.as_slice(), "IR NURBS edited knots").map_err(CodecError::from)?;
            ctx.charge_work(cadmpeg_core::decode::u64_from_index(values.len()), "IR NURBS knot edit")?;
            edit(&mut values);
            let knots = build_raw_knots(ctx, values, "")?;
            storage.commit()?;
            self.knots = knots;
            Ok(())
        })())
    }

    /// Replace the knot vector of an owned curve without copying its poles.
    ///
    /// # Errors
    ///
    /// Refuses a knot count inconsistent with the degree and pole count, a
    /// non-finite knot, or a decreasing pair.
    pub fn with_knots(mut self, ctx: &DecodeContext<'_>, knots: Vec<f64>) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish((|| {
            require_curve_cardinality(
                ctx,
                self.degree,
                knots.len(),
                self.poles.count(),
                "control_points",
            )?;
            self.knots = build_raw_knots(ctx, knots, "")?;
            Ok(self)
        })())
    }

    /// Build from finite knots, poles, and weights. Only relationships and
    /// the nonzero weight condition are checked.
    pub fn from_finite_lanes(
        ctx: &DecodeContext<'_>,
        degree: u32,
        knots: Vec<FiniteReal>,
        control_points: Vec<FinitePoint3>,
        weights: Option<Vec<FiniteReal>>,
        periodic: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish((|| {
        let mut storage = None;
        let poles = pair_curve_lanes(ctx, control_points, weights, &mut storage, |index, weight| admit_finite_weight(ctx, "poles", index, weight))?;
        build_curve(ctx, degree, knots, poles, periodic)
        })())
    }

    /// Build a NURBS curve from knots, a pole lane and an admitted weight lane.
    /// An admitted knot vector is kept; raw knots are admitted by [`Self::new`].
    ///
    /// # Errors
    ///
    /// Refuses a weight lane that does not cover the poles, a pole count
    /// inconsistent with the degree, or a non-finite or decreasing raw knot.
    pub fn from_checked_lanes<P: PoleValue<FinitePoint3>, K: KnotValue>(
        ctx: &DecodeContext<'_>,
        degree: u32,
        knots: K,
        control_points: Vec<P>,
        weights: Option<Vec<NonZeroReal>>,
        periodic: bool,
    ) -> Result<Result<Self, NurbsError>, CodecError> {
        admitted::finish((|| {
        let mut storage = if weights.is_some() && !P::RETAINS_POLE_STORAGE { Some(ctx.reserve_scoped(0, "IR NURBS paired poles")?) } else { None };
        let poles = pair_curve_lanes(ctx, control_points, weights, &mut storage, |_, weight| Ok(weight))?;
        build_curve(ctx, degree, knots, poles, periodic)
        })())
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

    /// Map pole positions in order after every result passes admission.
    /// The map must return the same result for the same index and position.
    /// Pole weights and knots stay in place.
    pub fn try_map_control_points<E>(
        &mut self,
        map: impl Fn(usize, FinitePoint3) -> Result<FinitePoint3, E>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), E>, CodecError> {
        let count = cadmpeg_core::decode::u64_from_index(self.poles.count());
        ctx.charge_work(count, "IR pole edit validation")?;
        ctx.charge_work(count, "IR pole edit mutation")?;
        Ok((|| {
        match &self.poles {
            NurbsPoles3::Polynomial { points } => {
                for (index, point) in points.iter().copied().enumerate() {
                    map(index, point)?;
                }
            }
            NurbsPoles3::Rational { points } => {
                for (index, pole) in points.iter().enumerate() {
                    map(index, pole.point)?;
                }
            }
        }
        match &mut self.poles {
            NurbsPoles3::Polynomial { points } => {
                for (index, point) in points.iter_mut().enumerate() {
                    *point = map(index, *point)?;
                }
            }
            NurbsPoles3::Rational { points } => {
                for (index, pole) in points.iter_mut().enumerate() {
                    pole.point = map(index, pole.point)?;
                }
            }
        }
        Ok(())
            })())
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
    pub fn reverse_parameterization(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.poles.count() / 2), "IR signed pole reversal")?;
        self.knots.reverse_negated(ctx)?;
        self.poles.reverse();
        Ok(())
    }

    /// Reverse poles and reflect knots within an admitted parameter range.
    /// The curve stays unchanged when a reflected knot is not finite or the
    /// resulting knot lane is decreasing.
    pub fn reverse_parameterization_in_range(
        &mut self,
        ctx: &DecodeContext<'_>,
        start: FiniteReal,
        end: FiniteReal,
    ) -> Result<Option<()>, CodecError> {
        let mut previous = None;
        for knot in self.knots.0.iter().rev() {
            ctx.charge_work(1, "IR NURBS reflected knot validation")?;
            let Some(reflected) = FiniteReal::new(*knot)
                .and_then(|knot| crate::math::reflect_parameter(knot, start, end)) else {
                return Ok(None);
            };
            if previous.is_some_and(|previous| previous > reflected) {
                return Ok(None);
            }
            previous = Some(reflected);
        }
        // Admit every mutation pass before changing any carrier lane.
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.poles.count() / 2), "IR NURBS reflected pole reversal")?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.knots.0.len() / 2), "IR NURBS reflected knot reversal")?;
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(self.knots.0.len()), "IR NURBS reflected knot edit")?;
        self.poles.reverse();
        self.knots.0.reverse();
        for knot in &mut self.knots.0 {
            // The validation pass reached the same original knot before mutation.
            let Some(reflected) = FiniteReal::new(*knot)
                .and_then(|knot| crate::math::reflect_parameter(knot, start, end)) else {
                return Ok(None);
            };
            *knot = reflected.get();
        }
        Ok(Some(()))
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
        build_curve(&StandardNurbsAdmission, wire.degree, wire.knots, wire.poles, wire.periodic)
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

impl From<CodecError> for NurbsError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(limit) => Self::ResourceLimit(limit),
            error => Self::Structure(error.to_string()),
        }
    }
}

mod identity_rewrite;
