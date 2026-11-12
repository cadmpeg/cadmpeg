// SPDX-License-Identifier: Apache-2.0
//! Pole-bounded sheets of a spindle torus.

use std::f64::consts::TAU;

use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
use cadmpeg_ir::geometry::{
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::{BodyKind, Face, LoopBoundary, Sense};

use super::{Builder, ClosedBound};
use crate::export::WrittenSurface;
use crate::geometry;
use crate::writer::{refs, Ref};

#[derive(Clone)]
pub(in crate::export) struct Spindle {
    outer: bool,
    interval: [f64; 2],
    vertices: [String; 2],
    meridian: CircleCurve,
    forward: bool,
}

impl Builder<'_> {
    pub(super) fn spindle_bound(&self, face: &Face) -> Option<ClosedBound> {
        let SolvedSurfaceGeometry::Torus(torus) = self
            .surfaces
            .get(face.surface.as_str())?
            .geometry
            .solved()?
        else {
            return None;
        };
        let major = torus.major_radius().get();
        let minor = torus.minor_radius().get();
        if minor <= major
            || face.loops.len() != 2
            || self
                .faces
                .values()
                .filter(|other| other.surface == face.surface)
                .count()
                != 1
        {
            return None;
        }
        let shell = self.shells.get(face.shell.as_str())?;
        let region = self
            .ir
            .model
            .regions
            .iter()
            .find(|region| region.id == shell.region)?;
        let body = self.bodies.get(region.body.as_str())?;
        if shell.faces().len() != 1
            || body.kind != BodyKind::Solid
            || region.shells.first() != Some(&shell.id)
        {
            return None;
        }
        let outer = match self.procedural_surfaces.get(face.surface.as_str()) {
            Some(surface) => match surface.definition() {
                ProceduralSurfaceDefinition::DegenerateTorus { select_outer } => *select_outer,
                _ => return None,
            },
            None => face.sense == Sense::Forward,
        };
        let angle = (-major / minor).acos();
        let interval = if outer {
            [-angle, angle]
        } else {
            [angle, TAU - angle]
        };
        let center = torus.center().get();
        let axis = *torus.frame().axis().as_raw();
        let reference = *torus.frame().reference().as_raw();
        let height = ((minor - major) * (minor + major)).sqrt();
        if !height.is_finite() || height <= self.ir.tolerances.linear.get() {
            return None;
        }
        let mut vertices = Vec::new();
        for sign in if outer { [-1.0, 1.0] } else { [1.0, -1.0] } {
            let expected = center.translated(axis, sign * height);
            let vertex = face.loops.iter().find_map(|id| {
                let LoopBoundary::Vertex { vertex, .. } = &self.loops.get(id.as_str())?.boundary
                else {
                    return None;
                };
                let record = self.vertices.get(vertex.as_str())?;
                let point = self.points.get(record.point.as_str())?.position().get();
                (point.distance(expected) <= self.ir.tolerances.linear.get())
                    .then(|| vertex.to_string())
            })?;
            vertices.push(vertex);
        }
        let meridian = CircleCurve::try_new(
            center.translated(reference, major),
            reference.cross(axis),
            reference,
            minor,
        )
        .ok()?;
        Some(ClosedBound::Spindle(Box::new(Spindle {
            outer,
            interval,
            vertices: vertices.try_into().ok()?,
            meridian,
            forward: face.sense == Sense::Forward,
        })))
    }

    pub(in crate::export) fn emit_spindle_surface(
        &mut self,
        surface_id: &str,
        spindle: &Spindle,
    ) -> Option<Ref> {
        if let Some(surface) = self.surface_refs.get(surface_id) {
            return Some(surface.reference);
        }
        let solved = self.surfaces.get(surface_id).copied()?.geometry.solved()?;
        let reference = self.emit_procedural_surface(
            Some(solved),
            &ProceduralSurfaceDefinition::DegenerateTorus {
                select_outer: spindle.outer,
            },
        )?;
        self.surface_refs.insert(
            surface_id.to_owned(),
            WrittenSurface {
                reference,
                chart: None,
                reversed_normal: false,
            },
        );
        if let Some(procedural) = self.procedural_surfaces.get(surface_id) {
            self.written_procedural_surfaces
                .insert(procedural.id.to_string());
        }
        Some(reference)
    }

    pub(super) fn emit_spindle_bound(
        &mut self,
        spindle: &Spindle,
        surface: Ref,
        flag: &str,
    ) -> Option<Vec<Ref>> {
        let start = self.emit_vertex(&spindle.vertices[0])?;
        let end = self.emit_vertex(&spindle.vertices[1])?;
        let curve = geometry::curve(
            &mut self.emitter,
            &SolvedCurveGeometry::Circle(spindle.meridian),
        )?;
        let context = self.pcurve_context();
        let [low, high] = spindle.interval;
        let mut pcurves = Vec::new();
        for u in [0.0, TAU] {
            let trace = LinePcurve::try_new(Point2::new(u, 0.0), Point2::new(0.0, 1.0)).ok()?;
            let trace = geometry::pcurve(&mut self.emitter, &PcurveGeometry::Line(trace))?;
            let trace = self.emitter.emit(
                "TRIMMED_CURVE",
                &format!(
                    "'',{trace},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),.T.,.PARAMETER.",
                    self.emitter.real(low),
                    self.emitter.real(high)
                ),
            );
            let representation = self.emitter.emit(
                "DEFINITIONAL_REPRESENTATION",
                &format!("'',({trace}),{context}"),
            );
            pcurves.push(
                self.emitter
                    .emit("PCURVE", &format!("'',{surface},{representation}")),
            );
        }
        let trimmed = self.emitter.emit(
            "TRIMMED_CURVE",
            &format!(
                "'',{curve},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),.T.,.PARAMETER.",
                self.emitter.real(low),
                self.emitter.real(high)
            ),
        );
        let seam = self.emitter.emit(
            "SEAM_CURVE",
            &format!("'',{trimmed},{},.CURVE_3D.", refs(&pcurves)),
        );
        let edge = self
            .emitter
            .emit("EDGE_CURVE", &format!("'',{start},{end},{seam},.T."));
        let order = if spindle.forward {
            [(1, true), (0, false)]
        } else {
            [(0, true), (1, false)]
        };
        let uses = order.map(|(trace, forward)| {
            self.emitter.emit(
                "SEAM_EDGE",
                &format!(
                    "'',*,*,{edge},{},{}",
                    if forward { ".T." } else { ".F." },
                    pcurves[trace]
                ),
            )
        });
        let lp = self
            .emitter
            .emit("EDGE_LOOP", &format!("'',{}", refs(&uses)));
        Some(vec![self
            .emitter
            .emit("FACE_OUTER_BOUND", &format!("'',{lp},{flag}"))])
    }
}
