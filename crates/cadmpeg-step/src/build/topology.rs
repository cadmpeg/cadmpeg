// SPDX-License-Identifier: Apache-2.0
//! STEP boundary-representation topology and geometry carrier emission.

use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::topology::{LoopBoundaryRole, Sense};

use crate::geometry;
use crate::writer::{real, refs, Ref};

use super::Builder;
use crate::vocab::{
    ADVANCED_FACE, AXIS1_PLACEMENT, CLOSED_SHELL, COMPOSITE_CURVE, COMPOSITE_CURVE_SEGMENT,
    DEFINITIONAL_REPRESENTATION, DEGENERATE_TOROIDAL_SURFACE, EDGE_CURVE, EDGE_LOOP, FACE_BOUND,
    FACE_OUTER_BOUND, GEOMETRIC_REPRESENTATION_CONTEXT, OFFSET_CURVE_3D, OFFSET_SURFACE,
    OPEN_SHELL, ORIENTED_EDGE, PCURVE, SURFACE_CURVE, SURFACE_OF_LINEAR_EXTRUSION,
    SURFACE_OF_REVOLUTION, TRIMMED_CURVE, VECTOR, VERTEX_LOOP, VERTEX_POINT,
};

impl Builder<'_> {
    pub(super) fn emit_shell(&mut self, shell_id: &str, closed: bool) -> Option<Ref> {
        let shell = self.index.shells.get(shell_id).copied()?;
        let face_ids: Vec<String> = shell.faces.iter().map(|f| f.0.clone()).collect();
        let mut face_refs = Vec::new();
        for fid in &face_ids {
            if let Some(r) = self.emit_face(fid) {
                face_refs.push(r);
            }
        }
        if face_refs.is_empty() {
            return None;
        }
        Some(self.emitter.emit(
            if closed { CLOSED_SHELL } else { OPEN_SHELL },
            &format!("'',{}", refs(&face_refs)),
        ))
    }

    fn emit_face(&mut self, face_id: &str) -> Option<Ref> {
        let face = self.index.faces.get(face_id).copied()?;
        let surface_id = face.surface.0.clone();
        // A face resting on an unknown (opaque) surface cannot become an
        // ADVANCED_FACE: STEP requires a real surface. Skip it and aggregate the
        // loss rather than fabricate placeholder geometry.
        if let Some(surf) = self.index.surfaces.get(surface_id.as_str()) {
            if !geometry::surface_is_supported(&surf.geometry) {
                self.skips.unknown_surface_faces.insert(face_id.to_string());
                return None;
            }
        }
        let loop_ids: Vec<String> = face.loops.iter().map(|l| l.0.clone()).collect();
        let same_sense = matches!(face.sense, Sense::Forward);

        let Some(surf_ref) = self.emit_surface(&surface_id) else {
            self.skips.unknown_surface_faces.insert(face_id.to_string());
            return None;
        };

        let mut bound_refs = Vec::new();
        for (i, lid) in loop_ids.iter().enumerate() {
            if let Some(loop_ref) = self.emit_loop(lid) {
                let kind = if matches!(
                    self.index
                        .loops
                        .get(lid.as_str())
                        .map(|loop_| loop_.boundary_role),
                    Some(LoopBoundaryRole::Outer)
                ) || (i == 0
                    && !loop_ids.iter().any(|id| {
                        self.index
                            .loops
                            .get(id.as_str())
                            .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
                    })) {
                    FACE_OUTER_BOUND
                } else {
                    FACE_BOUND
                };
                let b = self.emitter.emit(kind, &format!("'',{loop_ref},.T."));
                bound_refs.push(b);
            }
        }
        if bound_refs.is_empty() {
            return None;
        }
        let flag = if same_sense { ".T." } else { ".F." };
        let advanced_face = self.emitter.emit(
            ADVANCED_FACE,
            &format!("'',{},{surf_ref},{flag}", refs(&bound_refs)),
        );
        self.links
            .face_step_refs
            .insert(face_id.to_string(), advanced_face);
        Some(advanced_face)
    }

    fn emit_loop(&mut self, loop_id: &str) -> Option<Ref> {
        let lp = self.index.loops.get(loop_id).copied()?;
        if lp.coedges.is_empty() && lp.vertex_uses.len() == 1 {
            let vertex = self.emit_vertex(lp.vertex_uses[0].vertex.as_str())?;
            return Some(self.emitter.emit(VERTEX_LOOP, &format!("'',{vertex}")));
        }
        let coedge_ids: Vec<String> = lp.coedges.iter().map(|c| c.0.clone()).collect();
        let mut oe_refs = Vec::new();
        for cid in &coedge_ids {
            let Some(coedge) = self.index.coedges.get(cid.as_str()).copied() else {
                continue;
            };
            let orientation = matches!(coedge.sense, Sense::Forward);
            let Some(edge_ref) = self.emit_edge(coedge.edge.as_str()) else {
                continue;
            };
            let flag = if orientation { ".T." } else { ".F." };
            let oe = self
                .emitter
                .emit(ORIENTED_EDGE, &format!("'',*,*,{edge_ref},{flag}"));
            oe_refs.push(oe);
        }
        if oe_refs.is_empty() {
            return None;
        }
        Some(
            self.emitter
                .emit(EDGE_LOOP, &format!("'',{}", refs(&oe_refs))),
        )
    }

    pub(super) fn emit_edge(&mut self, edge_id: &str) -> Option<Ref> {
        if let Some(r) = self.geom.edge_refs.get(edge_id) {
            return Some(*r);
        }
        let edge = self.index.edges.get(edge_id).copied()?;
        let v1 = self.emit_vertex(edge.start.as_str())?;
        let v2 = self.emit_vertex(edge.end.as_str())?;
        let Some(curve_id) = &edge.curve else {
            self.skips.curveless_edges.insert(edge_id.to_string());
            return None;
        };
        if self
            .index
            .curves
            .get(curve_id.as_str())
            .is_some_and(|curve| !geometry::curve_is_supported(&curve.geometry))
        {
            self.skips.curveless_edges.insert(edge_id.to_string());
            return None;
        }
        let basis_curve = self.emit_curve(curve_id.as_str())?;
        let associated = self
            .index
            .edge_coedges
            .get(edge_id)
            .cloned()
            .unwrap_or_default();
        let mut pcurve_refs = Vec::new();
        for (pcurve_id, surface_id) in associated {
            if let Some(pcurve) = self.emit_pcurve(pcurve_id, surface_id) {
                pcurve_refs.push(pcurve);
            }
        }
        let curve_ref = if pcurve_refs.is_empty() {
            basis_curve
        } else {
            self.emitter.emit(
                SURFACE_CURVE,
                &format!("'',{basis_curve},{},.CURVE_3D.", refs(&pcurve_refs)),
            )
        };
        // same_sense = .T.: the edge runs start→end along the curve's own
        // parameterization, the convention IR curves follow.
        let r = self
            .emitter
            .emit(EDGE_CURVE, &format!("'',{v1},{v2},{curve_ref},.T."));
        self.geom.edge_refs.insert(edge_id.to_string(), r);
        Some(r)
    }

    fn emit_pcurve(&mut self, pcurve_id: &str, surface_id: &str) -> Option<Ref> {
        let pcurve = self.index.pcurves.get(pcurve_id).copied()?;
        let surface = self.emit_surface(surface_id)?;
        let curve = geometry::pcurve(&mut self.emitter, &pcurve.geometry)?;
        let context = if let Some(context) = self.geom.pcurve_context {
            context
        } else {
            let context = self.emitter.emit_raw(
                GEOMETRIC_REPRESENTATION_CONTEXT,
                "( GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('uv','2D') )",
            );
            self.geom.pcurve_context = Some(context);
            context
        };
        let representation = self.emitter.emit(
            DEFINITIONAL_REPRESENTATION,
            &format!("'',({curve}),{context}"),
        );
        Some(
            self.emitter
                .emit(PCURVE, &format!("'',{surface},{representation}")),
        )
    }

    fn emit_vertex(&mut self, vertex_id: &str) -> Option<Ref> {
        if let Some(r) = self.geom.vertex_refs.get(vertex_id) {
            return Some(*r);
        }
        let vertex = self.index.vertices.get(vertex_id).copied()?;
        let pt = self.index.points.get(vertex.point.as_str()).copied()?;
        let cp = geometry::point(&mut self.emitter, pt.position);
        self.geom.point_refs.insert(vertex.point.0.clone(), cp);
        let r = self.emitter.emit(VERTEX_POINT, &format!("'',{cp}"));
        self.geom.vertex_refs.insert(vertex_id.to_string(), r);
        Some(r)
    }

    pub(super) fn emit_surface(&mut self, surface_id: &str) -> Option<Ref> {
        if let Some(r) = self.geom.surface_refs.get(surface_id) {
            return Some(*r);
        }
        if self.geom.geometry_emission_depth >= 256
            || !self.geom.active_surfaces.insert(surface_id.to_string())
        {
            return None;
        }
        self.geom.geometry_emission_depth += 1;
        let result = (|| {
            let surf = self.index.surfaces.get(surface_id).copied()?;
            let procedural = self
                .index
                .procedural_surfaces
                .get(surface_id)
                .map(|procedural| (procedural.id.0.clone(), procedural.definition.clone()));
            let emitted = procedural.and_then(|(id, definition)| {
                self.emit_procedural_surface(&surf.geometry, &definition)
                    .map(|reference| (id, reference))
            });
            let r = if let Some((id, reference)) = emitted {
                self.geom.written_procedural_surfaces.insert(id);
                reference
            } else if !geometry::surface_is_supported(&surf.geometry) {
                return None;
            } else {
                geometry::surface(&mut self.emitter, &surf.geometry)
            };
            Some(r)
        })();
        self.geom.active_surfaces.remove(surface_id);
        self.geom.geometry_emission_depth -= 1;
        if let Some(r) = result {
            self.geom.surface_refs.insert(surface_id.to_string(), r);
        }
        result
    }

    fn emit_procedural_surface(
        &mut self,
        solved: &SurfaceGeometry,
        definition: &ProceduralSurfaceDefinition,
    ) -> Option<Ref> {
        let logical = |value: Option<bool>| match value {
            Some(true) => ".T.",
            Some(false) => ".F.",
            None => ".U.",
        };
        match definition {
            ProceduralSurfaceDefinition::LinearSweep {
                directrix,
                direction,
            } => {
                let directrix = self.emit_curve(directrix.as_str())?;
                let direction_ref = geometry::direction(&mut self.emitter, *direction);
                let vector = self.emitter.emit(
                    VECTOR,
                    &format!("'',{direction_ref},{}", real(direction.norm())),
                );
                Some(self.emitter.emit(
                    SURFACE_OF_LINEAR_EXTRUSION,
                    &format!("'',{directrix},{vector}"),
                ))
            }
            ProceduralSurfaceDefinition::AxisRevolution {
                directrix,
                axis_origin,
                axis_direction,
            } => {
                let directrix = self.emit_curve(directrix.as_str())?;
                let origin = geometry::point(&mut self.emitter, *axis_origin);
                let direction = geometry::direction(&mut self.emitter, *axis_direction);
                let axis = self
                    .emitter
                    .emit(AXIS1_PLACEMENT, &format!("'',{origin},{direction}"));
                Some(
                    self.emitter
                        .emit(SURFACE_OF_REVOLUTION, &format!("'',{directrix},{axis}")),
                )
            }
            ProceduralSurfaceDefinition::ParallelOffset {
                support,
                distance,
                self_intersect,
            } => {
                let support = self.emit_surface(support.as_str())?;
                Some(self.emitter.emit(
                    OFFSET_SURFACE,
                    &format!(
                        "'',{support},{},{}",
                        real(*distance),
                        logical(*self_intersect)
                    ),
                ))
            }
            ProceduralSurfaceDefinition::DegenerateTorus { select_outer } => {
                let SurfaceGeometry::Torus {
                    center,
                    axis,
                    ref_direction,
                    major_radius,
                    minor_radius,
                } = solved
                else {
                    return None;
                };
                let placement =
                    geometry::placement(&mut self.emitter, *center, *axis, *ref_direction);
                Some(self.emitter.emit(
                    DEGENERATE_TOROIDAL_SURFACE,
                    &format!(
                        "'',{placement},{},{},{}",
                        real(major_radius.abs()),
                        real(minor_radius.abs()),
                        if *select_outer { ".T." } else { ".F." }
                    ),
                ))
            }
            _ => None,
        }
    }

    pub(crate) fn emit_curve(&mut self, curve_id: &str) -> Option<Ref> {
        if let Some(r) = self.geom.curve_refs.get(curve_id) {
            return Some(*r);
        }
        if self.geom.geometry_emission_depth >= 256
            || !self.geom.active_curves.insert(curve_id.to_string())
        {
            return None;
        }
        self.geom.geometry_emission_depth += 1;
        let result = (|| {
            let geometry = self.index.curves.get(curve_id)?.geometry.clone();
            let procedural = self
                .index
                .procedural_curves
                .get(curve_id)
                .map(|procedural| (procedural.id.0.clone(), procedural.definition.clone()));
            let emitted = procedural.and_then(|(id, definition)| {
                self.emit_procedural_curve(&definition)
                    .map(|reference| (id, reference))
            });
            let r = if let Some((id, reference)) = emitted {
                self.geom.written_procedural_curves.insert(id);
                reference
            } else if let CurveGeometry::Composite {
                segments,
                self_intersect,
            } = &geometry
            {
                let mut segment_refs = Vec::with_capacity(segments.len());
                for segment in segments {
                    let curve = self.emit_curve(segment.curve.as_str())?;
                    let transition = match segment.transition {
                    cadmpeg_ir::geometry::CompositeCurveTransition::Discontinuous => {
                        ".DISCONTINUOUS."
                    }
                    cadmpeg_ir::geometry::CompositeCurveTransition::Continuous => ".CONTINUOUS.",
                    cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradient => {
                        ".CONTSAMEGRADIENT."
                    }
                    cadmpeg_ir::geometry::CompositeCurveTransition::ContSameGradientSameCurvature => {
                        ".CONTSAMEGRADIENTSAMECURVATURE."
                    }
                };
                    segment_refs.push(self.emitter.emit(
                        COMPOSITE_CURVE_SEGMENT,
                        &format!(
                            "{transition},{},{curve}",
                            if segment.same_sense { ".T." } else { ".F." }
                        ),
                    ));
                }
                self.emitter.emit(
                    COMPOSITE_CURVE,
                    &format!(
                        "'',{},{}",
                        refs(&segment_refs),
                        match self_intersect {
                            Some(true) => ".T.",
                            Some(false) => ".F.",
                            None => ".U.",
                        }
                    ),
                )
            } else if !geometry::curve_is_supported(&geometry) {
                return None;
            } else {
                geometry::curve(&mut self.emitter, &geometry)
            };
            Some(r)
        })();
        self.geom.active_curves.remove(curve_id);
        self.geom.geometry_emission_depth -= 1;
        if let Some(r) = result {
            self.geom.curve_refs.insert(curve_id.to_string(), r);
        }
        result
    }

    fn emit_procedural_curve(&mut self, definition: &ProceduralCurveDefinition) -> Option<Ref> {
        match definition {
            ProceduralCurveDefinition::Subset {
                source,
                parameter_range: [start, end],
            } => {
                let source = self.emit_curve(source.as_str())?;
                Some(self.emitter.emit(
                    TRIMMED_CURVE,
                    &format!(
                        "'',{source},(PARAMETER_VALUE({})),(PARAMETER_VALUE({})),.T.,.PARAMETER.",
                        real(*start),
                        real(*end)
                    ),
                ))
            }
            ProceduralCurveDefinition::SpatialOffset {
                source,
                distance,
                reference_direction,
                self_intersect,
            } => {
                let source = self.emit_curve(source.as_str())?;
                let direction = geometry::direction(&mut self.emitter, *reference_direction);
                let self_intersect = match self_intersect {
                    Some(true) => ".T.",
                    Some(false) => ".F.",
                    None => ".U.",
                };
                Some(self.emitter.emit(
                    OFFSET_CURVE_3D,
                    &format!(
                        "'',{source},{},{self_intersect},{direction}",
                        real(*distance)
                    ),
                ))
            }
            _ => None,
        }
    }
}
