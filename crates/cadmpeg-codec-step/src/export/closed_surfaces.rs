// SPDX-License-Identifier: Apache-2.0
//! Exact bounds of whole closed carriers.

use std::f64::consts::TAU;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::nurbs_surface_point;
use cadmpeg_ir::eval::{finite_or_refusal, nurbs_surface_isoline, IsolineDirection};
use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::{Face, LoopBoundary, Sense};

use super::Builder;
use crate::geometry;
use crate::writer::{refs, Ref};

#[derive(Clone)]
pub(super) enum ClosedBound {
    Poles(Vec<Point3>),
    Seams(Box<Seams>),
    Stated,
}

#[derive(Clone)]
pub(super) struct Seams {
    point: Point3,
    curves: [SolvedCurveGeometry; 2],
    domain: [[f64; 2]; 2],
    forward: bool,
}

impl Builder<'_> {
    pub(super) fn whole_closed_bound(
        &self,
        face: &Face,
    ) -> Result<Option<ClosedBound>, ResourceLimit> {
        for id in &face.loops {
            if !self
                .loops
                .get(id.as_str())
                .is_some_and(|lp| matches!(lp.boundary, LoopBoundary::Vertex { .. }))
            {
                return Ok(None);
            }
        }
        let Some(surface) = self
            .surfaces
            .get(face.surface.as_str())
            .and_then(|s| s.geometry.solved())
        else {
            return Ok(None);
        };
        let forward = face.sense == Sense::Forward;
        let bound = match surface {
            SolvedSurfaceGeometry::Sphere(s) => ClosedBound::Poles(vec![s
                .center()
                .get()
                .translated(*s.frame().axis().as_raw(), s.radius().get())]),
            SolvedSurfaceGeometry::Torus(torus)
                if torus.major_radius().get() > torus.minor_radius().get().abs() =>
            {
                let center = torus.center().get();
                let axis = *torus.frame().axis().as_raw();
                let reference = *torus.frame().reference().as_raw();
                let meridian_normal = reference.cross(axis);
                let major = torus.major_radius().get();
                let minor = torus.minor_radius().get().abs();
                let Ok(latitude) = CircleCurve::try_new(center, axis, reference, major + minor)
                else {
                    return Ok(None);
                };
                let Ok(meridian) = CircleCurve::try_new(
                    center.translated(reference, major),
                    meridian_normal,
                    reference,
                    minor,
                ) else {
                    return Ok(None);
                };
                ClosedBound::Seams(Box::new(Seams {
                    point: center.translated(reference, major + minor),
                    curves: [
                        SolvedCurveGeometry::Circle(latitude),
                        SolvedCurveGeometry::Circle(meridian),
                    ],
                    domain: [[0.0, TAU], [0.0, TAU]],
                    forward,
                }))
            }
            SolvedSurfaceGeometry::Nurbs(n) => {
                let tolerance = if face.loops.is_empty() {
                    0.0
                } else {
                    self.ir.tolerances.linear.get()
                };
                let Some(bound) = nurbs_bound(n, forward, tolerance)? else {
                    return Ok(None);
                };
                bound
            }
            _ => return Ok(None),
        };
        if face.loops.is_empty() {
            Ok(Some(bound))
        } else if matches!(bound, ClosedBound::Poles(_)) {
            Ok(Some(ClosedBound::Stated))
        } else {
            Ok(None)
        }
    }

    pub(super) fn emit_closed_bound(
        &mut self,
        bound: ClosedBound,
        surface: Ref,
        flag: &str,
    ) -> Option<Vec<Ref>> {
        match bound {
            ClosedBound::Stated => Some(Vec::new()),
            ClosedBound::Poles(poles) => Some(
                poles
                    .into_iter()
                    .map(|point| {
                        let point = geometry::point(&mut self.emitter, point);
                        let vertex = self.emitter.emit("VERTEX_POINT", &format!("'',{point}"));
                        let lp = self.emitter.emit("VERTEX_LOOP", &format!("'',{vertex}"));
                        self.emitter.emit("FACE_BOUND", &format!("'',{lp},{flag}"))
                    })
                    .collect(),
            ),
            ClosedBound::Seams(seams) => self
                .emit_closed_seams(&seams, surface, flag)
                .map(|bound| vec![bound]),
        }
    }

    fn emit_closed_seams(&mut self, seams: &Seams, surface: Ref, flag: &str) -> Option<Ref> {
        let point = geometry::point(&mut self.emitter, seams.point);
        let vertex = self.emitter.emit("VERTEX_POINT", &format!("'',{point}"));
        let [[u0, u1], [v0, v1]] = seams.domain;
        let traces = [
            [
                (Point2::new(0.0, v0), Point2::new(1.0, 0.0)),
                (Point2::new(0.0, v1), Point2::new(1.0, 0.0)),
            ],
            [
                (Point2::new(u1, 0.0), Point2::new(0.0, 1.0)),
                (Point2::new(u0, 0.0), Point2::new(0.0, 1.0)),
            ],
        ];
        let context = self.pcurve_context();
        let mut edges = Vec::new();
        for (curve, traces) in seams.curves.iter().zip(traces) {
            let curve = geometry::curve(&mut self.emitter, curve)?;
            let mut pcurves = Vec::new();
            for (origin, direction) in traces {
                let trace = LinePcurve::try_new(origin, direction).ok()?;
                let trace = geometry::pcurve(&mut self.emitter, &PcurveGeometry::Line(trace))?;
                let representation = self.emitter.emit(
                    "DEFINITIONAL_REPRESENTATION",
                    &format!("'',({trace}),{context}"),
                );
                pcurves.push(
                    self.emitter
                        .emit("PCURVE", &format!("'',{surface},{representation}")),
                );
            }
            let seam = self.emitter.emit(
                "SEAM_CURVE",
                &format!("'',{curve},{},.CURVE_3D.", refs(&pcurves)),
            );
            let edge = self
                .emitter
                .emit("EDGE_CURVE", &format!("'',{vertex},{vertex},{seam},.T."));
            edges.push((edge, pcurves));
        }
        let order = if seams.forward {
            [(0, 0, true), (1, 0, true), (0, 1, false), (1, 1, false)]
        } else {
            [(1, 1, true), (0, 1, true), (1, 0, false), (0, 0, false)]
        };
        let uses = order
            .into_iter()
            .map(|(edge, trace, forward)| {
                let sense = if forward { ".T." } else { ".F." };
                self.emitter.emit(
                    "SEAM_EDGE",
                    &format!("'',*,*,{},{sense},{}", edges[edge].0, edges[edge].1[trace]),
                )
            })
            .collect::<Vec<_>>();
        let lp = self
            .emitter
            .emit("EDGE_LOOP", &format!("'',{}", refs(&uses)));
        Some(
            self.emitter
                .emit("FACE_OUTER_BOUND", &format!("'',{lp},{flag}")),
        )
    }
}

fn domain(knots: &[f64], degree: u32, count: usize) -> Option<[f64; 2]> {
    let start = *knots.get(usize::try_from(degree).ok()?)?;
    let end = *knots.get(count)?;
    (start < end).then_some([start, end])
}

fn same_curve(a: &NurbsCurve, b: &NurbsCurve) -> bool {
    a.degree() == b.degree()
        && a.knots() == b.knots()
        && a.control_points() == b.control_points()
        && a.weights() == b.weights()
}

fn pole(curve: &NurbsCurve, tolerance: f64) -> Option<Point3> {
    let points = curve.control_points();
    let first = points.first()?.get();
    points
        .iter()
        .all(|p| p.get().distance(first) <= tolerance)
        .then_some(first)
}

fn nurbs_bound(
    n: &NurbsSurface,
    forward: bool,
    tolerance: f64,
) -> Result<Option<ClosedBound>, ResourceLimit> {
    let (Some([u0, u1]), Some([v0, v1])) = (
        domain(n.u_knots(), n.u_degree(), n.u_count()),
        domain(n.v_knots(), n.v_degree(), n.v_count()),
    ) else {
        return Ok(None);
    };
    let mut sides = Vec::new();
    for (direction, at) in [
        (IsolineDirection::ConstantV, v0),
        (IsolineDirection::ConstantU, u1),
        (IsolineDirection::ConstantV, v1),
        (IsolineDirection::ConstantU, u0),
    ] {
        let Some(curve) = nurbs_surface_isoline(EvaluationAdmission::Standard, n, direction, at)?
        else {
            return Ok(None);
        };
        sides.push(curve);
    }
    let u_seam = n.u_periodic() || same_curve(&sides[1], &sides[3]);
    let v_seam = n.v_periodic() || same_curve(&sides[0], &sides[2]);
    if u_seam && v_seam {
        let Some(point) = finite_or_refusal(nurbs_surface_point(
            EvaluationAdmission::Standard,
            n,
            u0,
            v0,
        ))?
        else {
            return Ok(None);
        };
        Ok(Some(ClosedBound::Seams(Box::new(Seams {
            point: point.get(),
            curves: [
                SolvedCurveGeometry::Nurbs(sides[0].clone()),
                SolvedCurveGeometry::Nurbs(sides[1].clone()),
            ],
            domain: [[u0, u1], [v0, v1]],
            forward: forward != n.normal_reversed(),
        }))))
    } else if u_seam {
        Ok(pole(&sides[0], tolerance)
            .zip(pole(&sides[2], tolerance))
            .map(|(a, b)| ClosedBound::Poles(vec![a, b])))
    } else if v_seam {
        Ok(pole(&sides[1], tolerance)
            .zip(pole(&sides[3], tolerance))
            .map(|(a, b)| ClosedBound::Poles(vec![a, b])))
    } else {
        Ok(None)
    }
}
