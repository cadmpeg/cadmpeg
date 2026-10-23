// SPDX-License-Identifier: Apache-2.0
//! The geometry of one edge: its curve, parameter interval, length and box.
//!
//! An edge whose carrier states no range takes its interval from its
//! vertices: a closed circle or periodic curve from the vertex parameter
//! around one period, a closed non-periodic curve whose domain ends coincide
//! from its domain, and an open edge from the parameters of its two
//! vertices, in the increasing-parameter convention of `Edge.param_range`.

use std::rc::Rc;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, EdgeCarrier};

use crate::bbox::{curve_box, point_box, CurveBox};
use crate::boundary::pcurve::{Span, UseFrame};
use crate::canonical::curve::{CurveChart, CurveShape};
use crate::canonical::placement::Placement;
use crate::canonical::Carrier;
use crate::context::{Eval, Work};
use crate::face::box_value;
use crate::failure::{settle, Measure, MeasureFailure, Missing, Outcome};
use crate::geom::{combine, Aabb};
use crate::length::{curve_length, Length};
use crate::owners::Owners;
use crate::quadrature::{extrema, integrate, Target};
use crate::reason::UnavailableReason;
use crate::report::{CurveKind, EdgeMeasures, EdgeParameters, RowStatus, StatusTally};
use crate::tolerance::{allowance, model_tolerance};
use crate::value::{measured, BoxKind, Errors, Method, Point, Quantity, Scalar, Unit};

/// One edge's geometry in model space.
pub(crate) struct EdgeGeometry<'ir> {
    pub(crate) edge: &'ir Edge,
    pub(crate) tau: f64,
    pub(crate) curve: Outcome<Rc<CurveChart<'ir>>>,
    /// The interval from the edge's start to its end.
    pub(crate) interval: Outcome<[f64; 2]>,
    pub(crate) length: Outcome<Length>,
    pub(crate) bbox: Outcome<CurveBox>,
    pub(crate) start: Option<Point3>,
    pub(crate) end: Option<Point3>,
    pub(crate) degenerate: bool,
}

impl EdgeGeometry<'_> {
    /// Whether the edge starts and ends at one vertex.
    pub(crate) fn closed(&self) -> bool {
        self.edge.start == self.edge.end
    }
}

fn missing(reason: UnavailableReason, entity: &str, message: &str) -> MeasureFailure {
    MeasureFailure::unavailable(reason, Some(entity), message.to_owned())
}

/// Measure one edge placed by `body`.
pub(crate) fn measure_edge<'ir>(
    eval: &Eval<'_, '_>,
    owners: &Owners<'ir>,
    edge: &'ir Edge,
    body: &Outcome<Placement>,
    work: Work,
) -> Result<EdgeGeometry<'ir>, ResourceLimit> {
    let id = edge.id.as_str();
    let placement = body.clone().unwrap_or(Placement::identity());
    let linear = owners.ir.tolerances.linear.get();
    let vertex_tolerance = |vertex: &str| {
        owners
            .vertex(vertex)
            .and_then(|vertex| vertex.tolerance)
            .map(cadmpeg_ir::scalar::PositiveReal::get)
    };
    let local = [
        edge.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        vertex_tolerance(edge.start.as_str()),
        vertex_tolerance(edge.end.as_str()),
    ];
    let alpha = allowance(linear, local.into_iter().flatten());
    let tau = model_tolerance(linear, local.into_iter().flatten());
    let start = owners
        .vertex_position(edge.start.as_str())
        .map(|point| placement.point(point));
    let end = owners
        .vertex_position(edge.end.as_str())
        .map(|point| placement.point(point));
    let curve = settle(
        match (body, edge.carrier.curve()) {
            (Err(missing), _) => Err(missing.failure()),
            (Ok(_), Some(curve)) => match owners.curve(curve.as_str()) {
                Some(record) => CurveChart::new(record, placement).map(Rc::new),
                None => Err(missing(
                    UnavailableReason::CarrierUnresolved,
                    id,
                    "the edge's curve does not resolve",
                )),
            },
            (Ok(_), None) => Err(missing(
                UnavailableReason::BoundaryCoedgeUnresolved,
                id,
                "the edge has no curve",
            )),
        }
        .map_err(|failure| failure.at(id)),
    )?;
    let interval = match &curve {
        Ok(chart) => settle(
            interval(eval, chart, edge, start, end, alpha).map_err(|failure| failure.at(id)),
        )?,
        Err(missing) => Err(missing.clone()),
    };
    let degenerate = match (&curve, &interval) {
        (Ok(chart), _) if matches!(chart.shape, CurveShape::Degenerate { .. }) => true,
        (_, Ok([t0, t1])) => t0 == t1,
        _ => false,
    };
    let scale = match (start, end) {
        (Some(start), Some(end)) => start.distance(end).max(alpha),
        _ => alpha,
    };
    if edge.carrier.curve().is_none() {
        if let (Ok(placement), Some(start), Some(end)) = (body, start, end) {
            let traced = settle(
                pcurve_trace(
                    eval,
                    owners,
                    edge,
                    &PcurveEnds {
                        placement: *placement,
                        alpha,
                        start,
                        end,
                    },
                )
                .map_err(|failure| failure.at(id)),
            )?;
            if let Ok(Some(trace)) = traced {
                return Ok(EdgeGeometry {
                    edge,
                    tau,
                    curve,
                    interval,
                    length: Ok(trace.length),
                    bbox: Ok(trace.bbox),
                    start: Some(start),
                    end: Some(end),
                    degenerate: trace.degenerate,
                });
            }
        }
    }
    let (length, bbox) = match (&curve, &interval) {
        (Ok(chart), Ok(range)) => (
            settle(curve_length(eval, chart, *range, scale).map_err(|failure| failure.at(id)))?,
            match work {
                Work::Row => {
                    settle(curve_box(eval, chart, *range).map_err(|failure| failure.at(id)))?
                }
                Work::Orientation => Err(Missing {
                    reason: UnavailableReason::EvaluationWorkExhausted,
                    entity: Some(id.to_owned()),
                    message: "an orientation pass does not compute this quantity".to_owned(),
                }),
            },
        ),
        (Err(missing), _) | (_, Err(missing)) => (
            Err(missing.clone()),
            match (start, end) {
                (Some(start), Some(end)) if body.is_ok() && (degenerate || start == end) => {
                    Ok(point_box(start))
                }
                _ => Err(missing.clone()),
            },
        ),
    };
    Ok(EdgeGeometry {
        edge,
        tau,
        curve,
        interval,
        length,
        bbox,
        start,
        end,
        degenerate,
    })
}

/// The ends of an edge with no 3D curve.
struct PcurveEnds {
    placement: Placement,
    alpha: f64,
    start: Point3,
    end: Point3,
}

/// The length, box and degeneracy of an edge traced by its pcurves.
struct Trace {
    length: Length,
    bbox: CurveBox,
    degenerate: bool,
}

/// Trace an edge with no 3D curve through the pcurve uses of its first
/// coedge that has any, mapped through that coedge's face surface. `None`
/// when no coedge of the edge has a pcurve.
fn pcurve_trace(
    eval: &Eval<'_, '_>,
    owners: &Owners<'_>,
    edge: &Edge,
    ends: &PcurveEnds,
) -> Measure<Option<Trace>> {
    let Some(coedge) = owners
        .edge_coedges(edge.id.as_str())
        .iter()
        .copied()
        .find(|coedge| !coedge.pcurves.is_empty())
    else {
        return Ok(None);
    };
    let surface = owners
        .coedge_face(coedge)
        .and_then(|face| owners.face(face))
        .and_then(|face| owners.surface(face.surface.as_str()))
        .ok_or_else(|| {
            MeasureFailure::unavailable(
                UnavailableReason::CarrierUnresolved,
                Some(coedge.id.as_str()),
                "the coedge's face surface does not resolve",
            )
        })?;
    let carrier = Carrier::resolve_in(owners, surface, ends.placement)?;
    let chart = carrier.chart()?;
    let (start, end) = match coedge.sense {
        cadmpeg_ir::topology::Sense::Forward => (ends.start, ends.end),
        cadmpeg_ir::topology::Sense::Reversed => (ends.end, ends.start),
    };
    let frame = UseFrame {
        owners,
        chart,
        alpha: ends.alpha,
        start,
        end,
        closed: edge.start == edge.end,
        curve: None,
        edge_range: edge.param_range().map(cadmpeg_ir::units::FiniteVector::get),
    };
    let resolved = frame.resolve(eval, &coedge.pcurves, true)?;
    let scale = start.distance(end).max(ends.alpha);
    let velocity = |span: &Span<'_>, t: f64| -> Measure<(Point3, Vector3)> {
        let uv = span.uv(eval, t)?;
        let duv = span.duv(eval, t)?;
        let partials = chart.partials(eval, uv[0], uv[1])?;
        Ok((
            partials.point,
            combine(&[(duv[0], partials.du), (duv[1], partials.dv)]),
        ))
    };
    let mut length = Length {
        value: 0.0,
        numeric_error: 0.0,
        method: Method::BoundaryQuadrature,
    };
    for span in &resolved.spans {
        let mut charge = |units| eval.charge(units);
        let mut speed = |t: f64| -> Measure<[f64; 1]> { Ok([velocity(span, t)?.1.norm()]) };
        let integral = integrate(
            &mut charge,
            &mut speed,
            span.t0.min(span.t1),
            span.t0.max(span.t1),
            &span.breaks(),
            Target::outer([scale]),
        )?;
        if !integral.converged {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::QuadratureNotConverged,
                None,
                "the length quadrature did not converge",
            ));
        }
        length.value += integral.value[0];
        length.numeric_error += integral.error[0];
    }
    let degenerate = edge.start == edge.end && length.value <= ends.alpha;
    if degenerate {
        return Ok(Some(Trace {
            length: Length {
                value: 0.0,
                numeric_error: length.value,
                method: Method::ClosedForm,
            },
            bbox: point_box(start),
            degenerate,
        }));
    }
    let mut aabb = Aabb::of_point(start);
    aabb.include(end);
    let mut numeric_error: f64 = 0.0;
    for span in &resolved.spans {
        for (axis, direction) in [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut charge = |units| eval.charge(units);
            let mut f = |t: f64| -> Measure<(f64, f64)> {
                let (point, tangent) = velocity(span, t)?;
                Ok((
                    point.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(direction),
                    tangent.dot(direction),
                ))
            };
            let found = extrema(&mut charge, &mut f, span.t0, span.t1, &span.breaks())?;
            aabb.min[axis] = aabb.min[axis].min(found.min);
            aabb.max[axis] = aabb.max[axis].max(found.max);
            numeric_error = numeric_error.max(found.error);
        }
    }
    Ok(Some(Trace {
        length,
        bbox: CurveBox {
            aabb,
            numeric_error,
        },
        degenerate,
    }))
}

fn interval(
    eval: &Eval<'_, '_>,
    chart: &CurveChart<'_>,
    edge: &Edge,
    start: Option<Point3>,
    end: Option<Point3>,
    alpha: f64,
) -> Measure<[f64; 2]> {
    if let EdgeCarrier::Bounded(_, range) = &edge.carrier {
        return Ok(range.endpoints());
    }
    let unresolved = |message: &str| {
        MeasureFailure::unavailable(
            UnavailableReason::BoundaryEdgeRangeUnresolved,
            None,
            message.to_owned(),
        )
    };
    let start = start.ok_or_else(|| unresolved("the edge's start vertex has no point"))?;
    let end = end.ok_or_else(|| unresolved("the edge's end vertex has no point"))?;
    let domain = chart.domain();
    let seed = domain.map_or(0.0, |[lower, _]| lower);
    let parameter = |point: Point3, seed: f64| -> Measure<f64> {
        chart
            .parameter(eval, point, seed, alpha)?
            .ok_or_else(|| unresolved("a vertex does not invert onto the edge's curve"))
    };
    if edge.start == edge.end {
        if let Some(period) = chart.period() {
            let t = parameter(start, seed)?;
            return Ok([t, t + period]);
        }
        return match domain {
            Some([lower, upper]) if upper > lower => {
                let a = chart.point(eval, lower)?;
                let b = chart.point(eval, upper)?;
                if a.distance(b) <= alpha {
                    Ok([lower, upper])
                } else {
                    Err(unresolved("a closed edge's curve is not closed"))
                }
            }
            _ => Err(unresolved("a closed edge's curve states no domain")),
        };
    }
    let t_start = parameter(start, seed)?;
    let t_end = parameter(end, domain.map_or(t_start, |[_, upper]| upper))?;
    if let Some(period) = chart.period() {
        let span = (t_end - t_start).rem_euclid(period);
        if span == 0.0 {
            return Err(unresolved(
                "the vertices of an open edge invert to one parameter",
            ));
        }
        return Ok([t_start, t_start + span]);
    }
    if t_start == t_end {
        return Err(unresolved(
            "the vertices of an open edge invert to one parameter",
        ));
    }
    Ok([t_start, t_end])
}

impl EdgeGeometry<'_> {
    /// The edge row.
    pub(crate) fn row(&self, owners: &Owners<'_>) -> EdgeMeasures {
        let id = self.edge.id.as_str();
        let chart = self.curve.as_ref().ok();
        let kind = match (&self.curve, self.edge.carrier.curve()) {
            (Ok(chart), _) => chart.kind,
            (Err(_), None) => CurveKind::None,
            (Err(_), Some(_)) => CurveKind::Unknown,
        };
        let parameters = chart.map_or(EdgeParameters::None {}, |chart| curve_parameters(chart));
        let length = match &self.length {
            Ok(length) => measured(
                Scalar::new(length.value),
                Unit::Millimeter,
                length.method,
                Errors::new(length.numeric_error, 2.0 * self.tau),
                id,
            ),
            Err(missing) => missing.quantity(),
        };
        let span = self.interval.as_ref().ok().map(|[t0, t1]| (t1 - t0).abs());
        let (arc_angle, parameter_span) = match (chart.map(|chart| &chart.shape), span) {
            (Some(CurveShape::Conic { major, minor, .. }), Some(span)) if major == minor => (
                Some(measured(
                    Scalar::new(span),
                    Unit::Radian,
                    Method::ClosedForm,
                    Errors::ZERO,
                    id,
                )),
                None,
            ),
            (Some(CurveShape::Conic { .. }), Some(span)) => (
                None,
                Some(measured(
                    Scalar::new(span),
                    Unit::One,
                    Method::ClosedForm,
                    Errors::ZERO,
                    id,
                )),
            ),
            _ => (None, None),
        };
        let endpoint = |point: Option<Point3>, vertex: &str| match point {
            Some(point) => measured(
                Point::new(point),
                Unit::Millimeter,
                Method::Source,
                Errors::ZERO,
                vertex,
            ),
            None => Quantity::missing(
                UnavailableReason::BoundaryCoedgeUnresolved,
                Some(vertex),
                "the vertex has no point",
            ),
        };
        let bbox = match &self.bbox {
            Ok(curve_box) => measured(
                box_value(curve_box.aabb, BoxKind::Tight, None),
                Unit::Millimeter,
                Method::BoundaryExtremum,
                Errors::new(curve_box.numeric_error, self.tau),
                id,
            ),
            Err(missing) => missing.quantity(),
        };
        let mut row = EdgeMeasures {
            edge: id.to_owned(),
            placement: crate::report::RowPlacement::Model {},
            curve: self
                .edge
                .carrier
                .curve()
                .map(|curve| curve.as_str().to_owned()),
            kind,
            parameters,
            length,
            arc_angle,
            parameter_span,
            start: endpoint(self.start, self.edge.start.as_str()),
            end: endpoint(self.end, self.edge.end.as_str()),
            closed: self.closed(),
            degenerate: self.degenerate,
            faces: owners.edge_faces(id),
            bbox,
            status: RowStatus::Measured,
        };
        let mut tally = StatusTally::default();
        row.visit(&mut tally);
        row.status = tally.status();
        row
    }
}

/// Kind-specific curve parameters in model space.
fn curve_parameters(chart: &CurveChart<'_>) -> EdgeParameters {
    let id = chart.id();
    let method = if chart.placement.class == crate::report::PlacementClass::Identity {
        Method::Source
    } else {
        Method::ClosedForm
    };
    let direction = |vector: cadmpeg_ir::math::Vector3| {
        measured(
            vector
                .unit()
                .and_then(|unit| Point::new(Point3::new(unit.x, unit.y, unit.z))),
            Unit::One,
            method,
            Errors::ZERO,
            id,
        )
    };
    let point = |point: Point3| {
        measured(
            Point::new(point),
            Unit::Millimeter,
            method,
            Errors::ZERO,
            id,
        )
    };
    let length = |value: f64| {
        measured(
            Scalar::new(value),
            Unit::Millimeter,
            method,
            Errors::new(0.0, chart.placement.model_term(1, value)),
            id,
        )
    };
    match &chart.shape {
        CurveShape::Line { direction: d, .. } => EdgeParameters::Line {
            direction: direction(*d),
        },
        CurveShape::Conic {
            center,
            e1,
            e2,
            major,
            minor,
        } => {
            if major == minor {
                EdgeParameters::Circle {
                    radius: length(*major),
                    center: point(*center),
                    normal: direction(e1.cross(*e2)),
                }
            } else {
                EdgeParameters::Ellipse {
                    major_radius: length(*major),
                    minor_radius: length(*minor),
                    center: point(*center),
                    normal: direction(e1.cross(*e2)),
                }
            }
        }
        CurveShape::Degenerate { .. } | CurveShape::Evaluated | CurveShape::Unsupported(_) => {
            EdgeParameters::None {}
        }
    }
}

#[cfg(test)]
mod tests;
