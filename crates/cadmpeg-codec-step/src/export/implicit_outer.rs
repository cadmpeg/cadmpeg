// SPDX-License-Identifier: Apache-2.0
//! The outer bound of a face whose curve-bounded surface leaves the outer
//! boundary implicit.
//!
//! A STEP face is the finite region to the left of each of its bounds, so the
//! natural boundary of the support must be written as a bound or the face
//! changes its domain.

use std::collections::HashMap;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::{
    finite_or_refusal, model_surface_point, nurbs_surface_isoline, IsolineDirection,
};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
use cadmpeg_ir::geometry::{
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::{Face, LoopBoundaryRole, Sense};

use super::Builder;
use crate::geometry;
use crate::writer::{refs, Ref};

/// What a face's implicit outer boundary needs in STEP.
enum ImplicitOuter<'a> {
    /// The face's loops state its whole boundary.
    Stated,
    /// The natural boundary is the parameter rectangle of a NURBS support.
    Rectangle(&'a NurbsSurface),
    /// STEP has no bound for the natural boundary, for the stated reason.
    Unwritable(&'static str),
}

/// The STEP outer bound of a face whose outer boundary is implicit.
#[derive(Clone)]
pub(super) enum OuterBound {
    /// The parameter rectangle of the support.
    Rectangle(Box<ParameterRectangle>),
    /// STEP has no bound for the natural boundary, for the stated reason.
    Unwritable(&'static str),
}

/// The four sides of a NURBS parameter rectangle, ready to emit.
#[derive(Clone)]
pub(super) struct ParameterRectangle {
    /// The corners at `(u0, v0)`, `(u1, v0)`, `(u1, v1)` and `(u0, v1)`.
    corners: [Point3; 4],
    /// `[[u0, u1], [v0, v1]]`.
    domain: [[f64; 2]; 2],
    /// The isolines at `v0`, `u1`, `v1` and `u0`, each on the free parameter.
    sides: [NurbsCurve; 4],
    /// Whether the loop runs counterclockwise in `(u, v)`.
    counterclockwise: bool,
}

/// Corners of each side, as indices into [`ParameterRectangle::corners`], in
/// the direction of the side's free parameter.
const SIDE_CORNERS: [[usize; 2]; 4] = [[0, 1], [1, 2], [3, 2], [0, 3]];

impl<'a> Builder<'a> {
    /// The outer bound of every face whose outer boundary is implicit.
    pub(super) fn implicit_outer_bounds(
        &self,
    ) -> Result<HashMap<&'a str, OuterBound>, ResourceLimit> {
        let mut bounds = HashMap::new();
        for (id, face) in &self.faces {
            let bound = match self.implicit_outer(face) {
                ImplicitOuter::Stated => continue,
                ImplicitOuter::Rectangle(nurbs) => match self.parameter_rectangle(face, nurbs)? {
                    Ok(rectangle) => OuterBound::Rectangle(Box::new(rectangle)),
                    Err(reason) => OuterBound::Unwritable(reason),
                },
                ImplicitOuter::Unwritable(reason) => OuterBound::Unwritable(reason),
            };
            bounds.insert(*id, bound);
        }
        Ok(bounds)
    }

    /// The implicit outer boundary of `face`, from the curve-bounded
    /// construction of its surface.
    fn implicit_outer(&self, face: &Face) -> ImplicitOuter<'a> {
        let Some(procedural) = self.procedural_surfaces.get(face.surface.as_str()) else {
            return ImplicitOuter::Stated;
        };
        let ProceduralSurfaceDefinition::CurveBounded {
            support,
            implicit_outer: true,
            ..
        } = procedural.definition()
        else {
            return ImplicitOuter::Stated;
        };
        if face
            .loops
            .iter()
            .any(|id| face.loop_role(id) == LoopBoundaryRole::Outer)
        {
            return ImplicitOuter::Stated;
        }
        let support = self
            .surfaces
            .get(support.as_str())
            .and_then(|surface| surface.geometry.solved());
        let carrier = self
            .surfaces
            .get(face.surface.as_str())
            .and_then(|surface| surface.geometry.solved());
        let Some(support) = support else {
            return ImplicitOuter::Unwritable("the support has no solved carrier");
        };
        match geometry::emitted_basis(support) {
            SolvedSurfaceGeometry::Plane(_)
            | SolvedSurfaceGeometry::Cylinder(_)
            | SolvedSurfaceGeometry::Cone(_) => {
                ImplicitOuter::Unwritable("the support is unbounded")
            }
            SolvedSurfaceGeometry::Sphere(_) | SolvedSurfaceGeometry::Torus(_) => {
                ImplicitOuter::Stated
            }
            SolvedSurfaceGeometry::Nurbs(nurbs) if nurbs.u_periodic() && nurbs.v_periodic() => {
                ImplicitOuter::Stated
            }
            SolvedSurfaceGeometry::Nurbs(nurbs) if nurbs.u_periodic() || nurbs.v_periodic() => {
                ImplicitOuter::Unwritable("the support's natural boundary is two closed isolines")
            }
            SolvedSurfaceGeometry::Nurbs(nurbs) => match support {
                SolvedSurfaceGeometry::Nurbs(_) if carrier == Some(support) => {
                    ImplicitOuter::Rectangle(nurbs)
                }
                SolvedSurfaceGeometry::Nurbs(_) => {
                    ImplicitOuter::Unwritable("the face carrier is not the support")
                }
                _ => ImplicitOuter::Unwritable("the support is a placed NURBS surface"),
            },
            SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed(_)
            | SolvedSurfaceGeometry::Unknown { .. } => {
                ImplicitOuter::Unwritable("the support has no stated natural boundary")
            }
        }
    }

    /// The sides of the parameter rectangle of `nurbs`, wound so the face
    /// lies on their left.
    fn parameter_rectangle(
        &self,
        face: &Face,
        nurbs: &NurbsSurface,
    ) -> Result<Result<ParameterRectangle, &'static str>, ResourceLimit> {
        let (Some(u), Some(v)) = (
            knot_domain(nurbs.u_knots(), nurbs.u_degree(), nurbs.u_count()),
            knot_domain(nurbs.v_knots(), nurbs.v_degree(), nurbs.v_count()),
        ) else {
            return Ok(Err("the support has no knot domain"));
        };
        let domain = [u, v];
        let [[u0, u1], [v0, v1]] = domain;
        let Some(surface) = self.surfaces.get(face.surface.as_str()) else {
            return Ok(Err("the face carrier is missing"));
        };
        let mut corners = [Point3::new(0.0, 0.0, 0.0); 4];
        for (corner, (u, v)) in corners
            .iter_mut()
            .zip([(u0, v0), (u1, v0), (u1, v1), (u0, v1)])
        {
            let Some(point) = finite_or_refusal(model_surface_point(
                EvaluationAdmission::Standard,
                self.ir,
                &surface.geometry,
                u,
                v,
            ))?
            else {
                return Ok(Err("a parameter rectangle corner does not evaluate"));
            };
            *corner = point.get();
        }
        let tolerance = self.ir.tolerances.linear.get();
        for (index, corner) in corners.iter().enumerate() {
            if corners[index + 1..]
                .iter()
                .any(|other| other.distance(*corner) <= tolerance)
            {
                return Ok(Err("two corners of the parameter rectangle coincide"));
            }
        }
        let mut sides = Vec::with_capacity(4);
        for (direction, at) in [
            (IsolineDirection::ConstantV, v0),
            (IsolineDirection::ConstantU, u1),
            (IsolineDirection::ConstantV, v1),
            (IsolineDirection::ConstantU, u0),
        ] {
            let Some(side) =
                nurbs_surface_isoline(EvaluationAdmission::Standard, nurbs, direction, at)?
            else {
                return Ok(Err("a side of the parameter rectangle does not evaluate"));
            };
            sides.push(side);
        }
        let Ok(sides) = <[NurbsCurve; 4]>::try_from(sides) else {
            return Ok(Err("a side of the parameter rectangle does not evaluate"));
        };
        Ok(Ok(ParameterRectangle {
            corners,
            domain,
            sides,
            counterclockwise: matches!(face.sense, Sense::Forward) != nurbs.normal_reversed(),
        }))
    }

    /// Write the rectangle as a `FACE_OUTER_BOUND` on `surface`.
    pub(super) fn emit_parameter_rectangle(
        &mut self,
        rectangle: ParameterRectangle,
        surface: Ref,
        bound_flag: &str,
    ) -> Option<Ref> {
        let ParameterRectangle {
            corners,
            domain: [[u0, u1], [v0, v1]],
            sides,
            counterclockwise,
        } = rectangle;
        let vertices = corners.map(|corner| {
            let point = geometry::point(&mut self.emitter, corner);
            self.emitter.emit("VERTEX_POINT", &format!("'',{point}"))
        });
        let traces = [
            ([0.0, v0], [1.0, 0.0]),
            ([u1, 0.0], [0.0, 1.0]),
            ([0.0, v1], [1.0, 0.0]),
            ([u0, 0.0], [0.0, 1.0]),
        ];
        let context = self.pcurve_context();
        let mut edges = Vec::with_capacity(sides.len());
        for ((side, [start, end]), ([ou, ov], [du, dv])) in
            sides.into_iter().zip(SIDE_CORNERS).zip(traces)
        {
            let curve = geometry::curve(&mut self.emitter, &SolvedCurveGeometry::Nurbs(side))?;
            let trace = LinePcurve::try_new(Point2::new(ou, ov), Point2::new(du, dv)).ok()?;
            let trace = geometry::pcurve(&mut self.emitter, &PcurveGeometry::Line(trace))?;
            let representation = self.emitter.emit(
                "DEFINITIONAL_REPRESENTATION",
                &format!("'',({trace}),{context}"),
            );
            let pcurve = self
                .emitter
                .emit("PCURVE", &format!("'',{surface},{representation}"));
            let surface_curve = self.emitter.emit(
                "SURFACE_CURVE",
                &format!("'',{curve},({pcurve}),.CURVE_3D."),
            );
            edges.push(self.emitter.emit(
                "EDGE_CURVE",
                &format!(
                    "'',{},{},{surface_curve},.T.",
                    vertices[start], vertices[end]
                ),
            ));
        }
        let order: [(usize, bool); 4] = if counterclockwise {
            [(0, true), (1, true), (2, false), (3, false)]
        } else {
            [(3, true), (2, true), (1, false), (0, false)]
        };
        let oriented = order
            .into_iter()
            .map(|(side, forward)| {
                let flag = if forward { ".T." } else { ".F." };
                self.emitter
                    .emit("ORIENTED_EDGE", &format!("'',*,*,{},{flag}", edges[side]))
            })
            .collect::<Vec<_>>();
        let edge_loop = self
            .emitter
            .emit("EDGE_LOOP", &format!("'',{}", refs(&oriented)));
        Some(
            self.emitter
                .emit("FACE_OUTER_BOUND", &format!("'',{edge_loop},{bound_flag}")),
        )
    }
}

/// The parameter interval `[knots[degree], knots[count]]` of a B-spline axis.
fn knot_domain(knots: &[f64], degree: u32, count: usize) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    let start = *knots.get(degree)?;
    let end = *knots.get(count)?;
    (start < end).then_some([start, end])
}

#[cfg(test)]
mod tests;
