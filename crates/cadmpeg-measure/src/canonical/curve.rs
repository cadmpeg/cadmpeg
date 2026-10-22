// SPDX-License-Identifier: Apache-2.0
//! Model-space charts of 3D curve carriers.
//!
//! Lines, circles and ellipses, bare or under a similarity chain, have closed
//! forms. Every other carrier is evaluated by the IR evaluators in its own
//! coordinates and mapped by the body placement.

use cadmpeg_ir::eval::decode::{curve_point, curve_tangent};
use cadmpeg_ir::eval::model_curve_point_by_id;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use super::placement::Placement;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::geom::{combine, offset};
use crate::reason::UnavailableReason;
use crate::report::CurveKind;

/// A model-space chart of a curve.
#[derive(Debug, Clone)]
pub(crate) enum CurveShape {
    /// `P = origin + t·direction`; `direction` has the curve's speed.
    Line { origin: Point3, direction: Vector3 },
    /// `P = center + a·cos t·e1 + b·sin t·e2` with unit orthogonal `e1`, `e2`.
    Conic {
        center: Point3,
        e1: Vector3,
        e2: Vector3,
        major: f64,
        minor: f64,
    },
    /// One point.
    Degenerate { point: Point3 },
    /// Evaluated by the IR evaluators and mapped by the body placement.
    Evaluated,
    /// No parameterization in this build.
    Unsupported(UnavailableReason),
}

/// A curve carrier in model space.
#[derive(Debug, Clone)]
pub(crate) struct CurveChart<'ir> {
    pub(crate) curve: &'ir Curve,
    pub(crate) kind: CurveKind,
    pub(crate) body: Placement,
    /// The composed placement of the curve's chain and the body.
    pub(crate) placement: Placement,
    pub(crate) shape: CurveShape,
}

/// The leaf of a curve's placement chain and the chain's composed placement.
fn leaf(geometry: &SolvedCurveGeometry) -> Measure<(&SolvedCurveGeometry, Placement)> {
    let mut chain = Placement::identity();
    let mut current = geometry;
    while let SolvedCurveGeometry::Transformed(placed) = current {
        chain = Placement::of(*placed.transform())?.then(chain)?;
        current = placed.basis();
    }
    Ok((current, chain))
}

fn solved_kind(geometry: &SolvedCurveGeometry) -> CurveKind {
    match geometry {
        SolvedCurveGeometry::Line(_) => CurveKind::Line,
        SolvedCurveGeometry::Circle(_) => CurveKind::Circle,
        SolvedCurveGeometry::Ellipse(_) => CurveKind::Ellipse,
        SolvedCurveGeometry::Parabola(_) => CurveKind::Parabola,
        SolvedCurveGeometry::Hyperbola(_) => CurveKind::Hyperbola,
        SolvedCurveGeometry::Degenerate(_) => CurveKind::Degenerate,
        SolvedCurveGeometry::Composite { .. } => CurveKind::Composite,
        SolvedCurveGeometry::Nurbs(_) => CurveKind::Nurbs,
        SolvedCurveGeometry::Polyline(_) => CurveKind::Polyline,
        SolvedCurveGeometry::Transformed(placed) => solved_kind(placed.basis()),
        SolvedCurveGeometry::Unknown { .. } => CurveKind::Unknown,
    }
}

impl<'ir> CurveChart<'ir> {
    /// The chart of `curve` placed by `body`.
    pub(crate) fn new(curve: &'ir Curve, body: Placement) -> Measure<Self> {
        let (kind, placement, shape) = match &curve.geometry {
            CurveGeometry::Procedural { .. } => {
                (CurveKind::Procedural, body, CurveShape::Evaluated)
            }
            CurveGeometry::Solved(geometry) => {
                let (leaf_geometry, chain) = leaf(geometry)?;
                let placement = chain.then(body)?;
                (
                    solved_kind(geometry),
                    placement,
                    Self::closed_form(leaf_geometry, placement),
                )
            }
        };
        Ok(Self {
            curve,
            kind,
            body,
            placement,
            shape,
        })
    }

    fn closed_form(geometry: &SolvedCurveGeometry, placement: Placement) -> CurveShape {
        match geometry {
            SolvedCurveGeometry::Composite { .. } => {
                return CurveShape::Unsupported(UnavailableReason::CarrierCompositeCurve)
            }
            SolvedCurveGeometry::Unknown { .. } => {
                return CurveShape::Unsupported(UnavailableReason::CarrierUnknown)
            }
            SolvedCurveGeometry::Degenerate(curve) => {
                return CurveShape::Degenerate {
                    point: placement.point(curve.point().get()),
                }
            }
            _ => {}
        }
        if !placement.is_similarity() {
            return CurveShape::Evaluated;
        }
        let identity = placement.class == crate::report::PlacementClass::Identity;
        let point = |point: Point3| {
            if identity {
                point
            } else {
                placement.point(point)
            }
        };
        let rotate = |vector: Vector3| {
            if identity {
                vector
            } else {
                placement.rotate(vector)
            }
        };
        let vector = |vector: Vector3| {
            if identity {
                vector
            } else {
                placement.vector(vector)
            }
        };
        match geometry {
            SolvedCurveGeometry::Line(line) => CurveShape::Line {
                origin: point(line.origin().get()),
                direction: vector(*line.direction().as_raw()),
            },
            SolvedCurveGeometry::Circle(circle) => {
                let axis = *circle.frame().axis().as_raw();
                let reference = *circle.frame().reference().as_raw();
                let radius = circle.radius().get() * placement.scale;
                CurveShape::Conic {
                    center: point(circle.center().get()),
                    e1: rotate(reference),
                    e2: rotate(axis.cross(reference)),
                    major: radius,
                    minor: radius,
                }
            }
            SolvedCurveGeometry::Ellipse(ellipse) => {
                let axis = *ellipse.frame().axis().as_raw();
                let reference = *ellipse.frame().reference().as_raw();
                CurveShape::Conic {
                    center: point(ellipse.center().get()),
                    e1: rotate(reference),
                    e2: rotate(axis.cross(reference)),
                    major: ellipse.major_radius().get() * placement.scale,
                    minor: ellipse.minor_radius().get() * placement.scale,
                }
            }
            _ => CurveShape::Evaluated,
        }
    }

    /// The curve's ID.
    pub(crate) fn id(&self) -> &'ir str {
        self.curve.id.as_str()
    }

    /// The parameter period of a closed curve.
    pub(crate) fn period(&self) -> Option<f64> {
        match (&self.shape, &self.curve.geometry) {
            (CurveShape::Conic { .. }, _) => Some(std::f64::consts::TAU),
            (_, geometry) => match geometry.solved().map(leaf) {
                Some(Ok((SolvedCurveGeometry::Nurbs(nurbs), _))) if nurbs.periodic() => {
                    cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)
                        .map(|domain| domain.upper() - domain.lower())
                }
                Some(Ok((SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_), _))) => {
                    Some(std::f64::consts::TAU)
                }
                _ => None,
            },
        }
    }

    /// The finite parameter domain the carrier states, when it states one.
    pub(crate) fn domain(&self) -> Option<[f64; 2]> {
        if let Some(range) = self.curve.parameter_range {
            return Some(range.endpoints());
        }
        match self.curve.geometry.solved().map(leaf) {
            Some(Ok((SolvedCurveGeometry::Nurbs(nurbs), _))) => {
                cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
            }
            _ => None,
        }
    }

    /// Parameters where the curve's smoothness may break: the knots of a
    /// NURBS leaf.
    pub(crate) fn breaks(&self) -> Vec<f64> {
        match self.curve.geometry.solved().map(leaf) {
            Some(Ok((SolvedCurveGeometry::Nurbs(nurbs), _))) => nurbs.knots().to_vec(),
            _ => Vec::new(),
        }
    }

    /// The model-space point at `t`.
    pub(crate) fn point(&self, eval: &Eval<'_, '_>, t: f64) -> Measure<Point3> {
        match &self.shape {
            CurveShape::Line { origin, direction } => Ok(offset(*origin, direction.scale(t))),
            CurveShape::Conic {
                center,
                e1,
                e2,
                major,
                minor,
            } => {
                let (sine, cosine) = t.sin_cos();
                Ok(offset(
                    *center,
                    combine(&[(major * cosine, *e1), (minor * sine, *e2)]),
                ))
            }
            CurveShape::Degenerate { point } => Ok(*point),
            CurveShape::Evaluated => {
                eval.charge(1)?;
                let local = match &self.curve.geometry {
                    CurveGeometry::Procedural { cache: None, .. } => {
                        eval.work.evaluate(eval.ctx, |admission| {
                            model_curve_point_by_id(admission, eval.index, &self.curve.id, t)
                        })?
                    }
                    geometry => eval
                        .work
                        .evaluate(eval.ctx, |admission| curve_point(admission, geometry, t))?,
                };
                Ok(self.body.point(local.get()))
            }
            CurveShape::Unsupported(reason) => Err(MeasureFailure::unavailable(
                *reason,
                Some(self.id()),
                "the curve has no parameterization",
            )),
        }
    }

    /// The model-space derivative at `t`.
    pub(crate) fn tangent(&self, eval: &Eval<'_, '_>, t: f64) -> Measure<Vector3> {
        match &self.shape {
            CurveShape::Line { direction, .. } => Ok(*direction),
            CurveShape::Conic {
                e1,
                e2,
                major,
                minor,
                ..
            } => {
                let (sine, cosine) = t.sin_cos();
                Ok(combine(&[(-major * sine, *e1), (minor * cosine, *e2)]))
            }
            CurveShape::Degenerate { .. } => Ok(Vector3::new(0.0, 0.0, 0.0)),
            CurveShape::Evaluated => {
                if matches!(
                    self.curve.geometry,
                    CurveGeometry::Procedural { cache: None, .. }
                ) {
                    return Err(MeasureFailure::unavailable(
                        UnavailableReason::LengthDerivativeUnavailable,
                        Some(self.id()),
                        "a procedural curve without a solved cache has no derivative evaluator",
                    ));
                }
                eval.charge(1)?;
                let local = eval.work.evaluate(eval.ctx, |admission| {
                    curve_tangent(admission, &self.curve.geometry, t)
                })?;
                Ok(self.body.vector(local.get()))
            }
            CurveShape::Unsupported(reason) => Err(MeasureFailure::unavailable(
                *reason,
                Some(self.id()),
                "the curve has no parameterization",
            )),
        }
    }

    /// The parameter of `point` on a closed-form chart, or `None` for an
    /// evaluated chart.
    pub(crate) fn closed_form_parameter(&self, point: Point3) -> Option<f64> {
        match &self.shape {
            CurveShape::Line { origin, direction } => {
                let speed = direction.dot(*direction);
                (speed > 0.0).then(|| point.vector_from(*origin).dot(*direction) / speed)
            }
            CurveShape::Conic {
                center,
                e1,
                e2,
                major,
                minor,
            } => {
                let d = point.vector_from(*center);
                Some((d.dot(*e2) / minor).atan2(d.dot(*e1) / major))
            }
            CurveShape::Degenerate { .. } | CurveShape::Evaluated | CurveShape::Unsupported(_) => {
                None
            }
        }
    }

    /// The parameter of a model point on the curve within `tolerance`, near
    /// `seed`.
    pub(crate) fn parameter(
        &self,
        eval: &Eval<'_, '_>,
        point: Point3,
        seed: f64,
        tolerance: f64,
    ) -> Measure<Option<f64>> {
        if let Some(parameter) = self.closed_form_parameter(point) {
            let reached = self.point(eval, parameter)?;
            return Ok((reached.distance(point) <= tolerance).then_some(parameter));
        }
        match self.shape {
            CurveShape::Evaluated => {
                let Some(local) = self.body.inverse_point(point) else {
                    return Ok(None);
                };
                eval.charge(1)?;
                let found =
                    cadmpeg_ir::eval::model_curve_parameter_near_point_in_index_with_tolerance(
                        eval.ctx,
                        eval.index,
                        &self.curve.id,
                        local,
                        seed,
                        tolerance / self.body.scale,
                    )
                    .map_err(MeasureFailure::from_codec)?;
                Ok(found.map(cadmpeg_ir::scalar::FiniteReal::get))
            }
            _ => Ok(None),
        }
    }
}
