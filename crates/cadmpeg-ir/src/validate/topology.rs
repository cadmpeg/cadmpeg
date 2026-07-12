// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::sketches::{SketchConstraintDefinition as Definition, SketchLocus};

/// Presence sets for every arena, keyed by the string id.
pub(super) struct IdSets {
    bodies: HashSet<String>,
    regions: HashSet<String>,
    shells: HashSet<String>,
    faces: HashSet<String>,
    loops: HashSet<String>,
    coedges: HashSet<String>,
    edges: HashSet<String>,
    vertices: HashSet<String>,
    points: HashSet<String>,
    surfaces: HashSet<String>,
    curves: HashSet<String>,
    pcurves: HashSet<String>,
    appearances: HashSet<String>,
    unknowns: HashSet<String>,
}

impl IdSets {
    pub(super) fn build(ir: &CadIr) -> Self {
        IdSets {
            bodies: ir.model.bodies.iter().map(|e| e.id.0.clone()).collect(),
            regions: ir.model.regions.iter().map(|e| e.id.0.clone()).collect(),
            shells: ir.model.shells.iter().map(|e| e.id.0.clone()).collect(),
            faces: ir.model.faces.iter().map(|e| e.id.0.clone()).collect(),
            loops: ir.model.loops.iter().map(|e| e.id.0.clone()).collect(),
            coedges: ir.model.coedges.iter().map(|e| e.id.0.clone()).collect(),
            edges: ir.model.edges.iter().map(|e| e.id.0.clone()).collect(),
            vertices: ir.model.vertices.iter().map(|e| e.id.0.clone()).collect(),
            points: ir.model.points.iter().map(|e| e.id.0.clone()).collect(),
            surfaces: ir.model.surfaces.iter().map(|e| e.id.0.clone()).collect(),
            curves: ir.model.curves.iter().map(|e| e.id.0.clone()).collect(),
            pcurves: ir.model.pcurves.iter().map(|e| e.id.0.clone()).collect(),
            appearances: ir
                .model
                .appearances
                .iter()
                .map(|e| e.id.0.clone())
                .collect(),
            unknowns: ir
                .all_native_unknowns()
                .unwrap_or_default()
                .into_iter()
                .map(|record| record.id.0)
                .collect(),
        }
    }
}

pub(super) fn ref_error(findings: &mut Vec<Finding>, owner: &str, target_kind: &str, target: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: format!("references missing {target_kind} `{target}`"),
        entity: Some(owner.to_string()),
    });
}

pub(super) fn check_units(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.units.length != LengthUnit::Millimeter {
        findings.push(Finding {
            check: Check::Units,
            severity: Severity::Warning,
            message: format!(
                "document length unit is {:?}, not the canonical millimeter",
                ir.units.length
            ),
            entity: None,
        });
    }
    if nonpositive(ir.tolerances.linear) {
        findings.push(Finding {
            check: Check::Tolerances,
            severity: Severity::Warning,
            message: "document linear tolerance is not positive and finite".into(),
            entity: None,
        });
    }
    if nonpositive(ir.tolerances.angular) {
        findings.push(Finding {
            check: Check::Tolerances,
            severity: Severity::Warning,
            message: "document angular tolerance is not positive and finite".into(),
            entity: None,
        });
    }
    if ir.tolerances.linear > 1.0e6 || ir.tolerances.angular > std::f64::consts::TAU {
        findings.push(Finding {
            check: Check::Tolerances,
            severity: Severity::Warning,
            message: "document tolerance is outside a sane canonical range".into(),
            entity: None,
        });
    }
}

pub(super) fn check_references(ir: &CadIr, ids: &IdSets, findings: &mut Vec<Finding>) {
    for b in &ir.model.bodies {
        for l in &b.regions {
            if !ids.regions.contains(&l.0) {
                ref_error(findings, &b.id.0, "region", &l.0);
            }
        }
    }
    for l in &ir.model.regions {
        if !ids.bodies.contains(&l.body.0) {
            ref_error(findings, &l.id.0, "body", &l.body.0);
        }
        for s in &l.shells {
            if !ids.shells.contains(&s.0) {
                ref_error(findings, &l.id.0, "shell", &s.0);
            }
        }
    }
    for s in &ir.model.shells {
        if !ids.regions.contains(&s.region.0) {
            ref_error(findings, &s.id.0, "region", &s.region.0);
        }
        for f in &s.faces {
            if !ids.faces.contains(&f.0) {
                ref_error(findings, &s.id.0, "face", &f.0);
            }
        }
        for e in &s.wire_edges {
            if !ids.edges.contains(&e.0) {
                ref_error(findings, &s.id.0, "wire edge", &e.0);
            }
        }
        for v in &s.free_vertices {
            if !ids.vertices.contains(&v.0) {
                ref_error(findings, &s.id.0, "free vertex", &v.0);
            }
        }
    }
    for f in &ir.model.faces {
        if !ids.shells.contains(&f.shell.0) {
            ref_error(findings, &f.id.0, "shell", &f.shell.0);
        }
        if !ids.surfaces.contains(&f.surface.0) {
            ref_error(findings, &f.id.0, "surface", &f.surface.0);
        }
        for lp in &f.loops {
            if !ids.loops.contains(&lp.0) {
                ref_error(findings, &f.id.0, "loop", &lp.0);
            }
        }
    }
    for lp in &ir.model.loops {
        if !ids.faces.contains(&lp.face.0) {
            ref_error(findings, &lp.id.0, "face", &lp.face.0);
        }
        for ce in &lp.coedges {
            if !ids.coedges.contains(&ce.0) {
                ref_error(findings, &lp.id.0, "coedge", &ce.0);
            }
        }
    }
    for ce in &ir.model.coedges {
        if !ids.loops.contains(&ce.owner_loop.0) {
            ref_error(findings, &ce.id.0, "loop", &ce.owner_loop.0);
        }
        if !ids.edges.contains(&ce.edge.0) {
            ref_error(findings, &ce.id.0, "edge", &ce.edge.0);
        }
        if !ids.coedges.contains(&ce.next.0) {
            ref_error(findings, &ce.id.0, "coedge(next)", &ce.next.0);
        }
        if !ids.coedges.contains(&ce.previous.0) {
            ref_error(findings, &ce.id.0, "coedge(previous)", &ce.previous.0);
        }
        if !ids.coedges.contains(&ce.radial_next.0) {
            ref_error(findings, &ce.id.0, "coedge(radial_next)", &ce.radial_next.0);
        }
        if let Some(pc) = &ce.pcurve {
            if !ids.pcurves.contains(&pc.0) {
                ref_error(findings, &ce.id.0, "pcurve", &pc.0);
            }
        }
    }
    for e in &ir.model.edges {
        if let Some(c) = &e.curve {
            if !ids.curves.contains(&c.0) {
                ref_error(findings, &e.id.0, "curve", &c.0);
            }
        }
        if !ids.vertices.contains(&e.start.0) {
            ref_error(findings, &e.id.0, "vertex(start)", &e.start.0);
        }
        if !ids.vertices.contains(&e.end.0) {
            ref_error(findings, &e.id.0, "vertex(end)", &e.end.0);
        }
    }
    for v in &ir.model.vertices {
        if !ids.points.contains(&v.point.0) {
            ref_error(findings, &v.id.0, "point", &v.point.0);
        }
    }
    for binding in &ir.model.appearance_bindings {
        use crate::appearance::AppearanceTarget;
        let owner = format!("appearance-binding:{}", binding.appearance.0);
        if !ids.appearances.contains(&binding.appearance.0) {
            ref_error(findings, &owner, "appearance", &binding.appearance.0);
        }
        match &binding.target {
            AppearanceTarget::Body(body) if !ids.bodies.contains(&body.0) => {
                ref_error(findings, &owner, "body", &body.0);
            }
            AppearanceTarget::Face(face) if !ids.faces.contains(&face.0) => {
                ref_error(findings, &owner, "face", &face.0);
            }
            _ => {}
        }
    }
    for attribute in &ir.model.attributes {
        use crate::attributes::AttributeTarget;
        let owner = &attribute.id.0;
        match &attribute.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if !ids.bodies.contains(&id.0) => {
                ref_error(findings, owner, "body", &id.0);
            }
            AttributeTarget::Face(id) if !ids.faces.contains(&id.0) => {
                ref_error(findings, owner, "face", &id.0);
            }
            AttributeTarget::Coedge(id) if !ids.coedges.contains(&id.0) => {
                ref_error(findings, owner, "coedge", &id.0);
            }
            AttributeTarget::Edge(id) if !ids.edges.contains(&id.0) => {
                ref_error(findings, owner, "edge", &id.0);
            }
            AttributeTarget::Vertex(id) if !ids.vertices.contains(&id.0) => {
                ref_error(findings, owner, "vertex", &id.0);
            }
            _ => {}
        }
    }
    for s in &ir.model.surfaces {
        if let SurfaceGeometry::Unknown { record: Some(u) } = &s.geometry {
            if !ids.unknowns.contains(&u.0) {
                ref_error(findings, &s.id.0, "unknown record", &u.0);
            }
        }
    }
    for curve in &ir.model.curves {
        if let CurveGeometry::Unknown {
            record: Some(unknown),
        } = &curve.geometry
        {
            if !ids.unknowns.contains(&unknown.0) {
                ref_error(findings, &curve.id.0, "unknown record", &unknown.0);
            }
        }
    }
    for procedural in &ir.model.procedural_surfaces {
        if !ids.surfaces.contains(&procedural.surface.0) {
            ref_error(
                findings,
                &procedural.surface.0,
                "surface",
                &procedural.surface.0,
            );
        }
        match &procedural.definition {
            ProceduralSurfaceDefinition::Exact { .. } => {}
            ProceduralSurfaceDefinition::Compound { components, .. } => {
                for component in components {
                    if !ids.surfaces.contains(&component.0) {
                        ref_error(findings, &procedural.id.0, "surface", &component.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                if !ids.surfaces.contains(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
                if !ids.curves.contains(&reference.0) {
                    ref_error(findings, &procedural.id.0, "curve", &reference.0);
                }
            }
            ProceduralSurfaceDefinition::Loft { sections, .. } => {
                for entry in sections.iter().flat_map(|section| &section.entries) {
                    for curve in std::iter::once(&entry.path.curve)
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.curve))
                    {
                        if !ids.curves.contains(&curve.0) {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                    for member in &entry.profile {
                        if !ids.surfaces.contains(&member.data.surface.0) {
                            ref_error(
                                findings,
                                &procedural.id.0,
                                "surface",
                                &member.data.surface.0,
                            );
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                };
                let mut scales = construction.scales.iter().flatten().collect::<Vec<_>>();
                scales.extend(construction.fifth_scale.iter().map(Box::as_ref));
                match &construction.tail {
                    crate::geometry::CompoundLoftTail::Six { scale, curve, .. } => {
                        scales.push(scale.as_ref());
                        check_curve(curve, findings);
                    }
                    crate::geometry::CompoundLoftTail::Seven {
                        first_scale,
                        second_scale,
                        ..
                    } => {
                        scales.extend(first_scale.iter().map(Box::as_ref));
                        scales.push(second_scale.as_ref());
                    }
                    crate::geometry::CompoundLoftTail::Zero { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve } = direction {
                            check_curve(curve, findings);
                        }
                    }
                }
                for scale in scales {
                    check_curve(&scale.path, findings);
                    for curve in &scale.auxiliaries {
                        check_curve(curve, findings);
                    }
                    for member in &scale.members {
                        check_curve(&member.curve, findings);
                        if !ids.surfaces.contains(&member.data.surface.0) {
                            ref_error(
                                findings,
                                &procedural.id.0,
                                "surface",
                                &member.data.surface.0,
                            );
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                };
                let mut scales = construction.scales.iter().flatten().collect::<Vec<_>>();
                match &construction.branch {
                    crate::geometry::ScaledCompoundLoftBranch::ExtendedVector {
                        first_scale,
                        second_scale,
                        ..
                    } => {
                        scales.extend(first_scale.iter().map(Box::as_ref));
                        scales.push(second_scale.as_ref());
                    }
                    crate::geometry::ScaledCompoundLoftBranch::ExtendedCurve {
                        scale,
                        curve,
                        ..
                    } => {
                        scales.extend(scale.iter().map(Box::as_ref));
                        check_curve(curve, findings);
                    }
                    crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve } = direction {
                            check_curve(curve, findings);
                        }
                    }
                }
                check_curve(&construction.tail_curve, findings);
                for scale in scales {
                    check_curve(&scale.path, findings);
                    for curve in &scale.auxiliaries {
                        check_curve(curve, findings);
                    }
                    for member in &scale.members {
                        check_curve(&member.curve, findings);
                        if !ids.surfaces.contains(&member.data.surface.0) {
                            ref_error(
                                findings,
                                &procedural.id.0,
                                "surface",
                                &member.data.surface.0,
                            );
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend { construction } => {
                for surface in [&construction.first.surface, &construction.second.surface]
                    .into_iter()
                    .chain(std::iter::once(&construction.second_exact_surface))
                {
                    if !ids.surfaces.contains(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
                if let crate::geometry::G2BlendFirstShape::Full {
                    surface: Some(surface),
                    ..
                } = &construction.first_shape
                {
                    if !ids.surfaces.contains(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
                for curve in [
                    &construction.first.curve,
                    &construction.second.curve,
                    &construction.center_curve,
                ] {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::VariableBlend { construction } => {
                for side in construction.sides.iter() {
                    if !ids.surfaces.contains(&side.surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &side.surface.0);
                    }
                    if !ids.curves.contains(&side.curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &side.curve.0);
                    }
                }
                for curve in [
                    &construction.primary_curve,
                    &construction.secondary_curve,
                    &construction.post_curve,
                ] {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::VertexBlend { construction } => {
                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            if !ids.curves.contains(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            if !ids.surfaces.contains(&surface.0) {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Degenerate { .. } => {}
                    }
                }
            }
            ProceduralSurfaceDefinition::Extrusion { directrix, .. }
            | ProceduralSurfaceDefinition::Revolution { directrix, .. } => {
                if !ids.curves.contains(&directrix.0) {
                    ref_error(findings, &procedural.id.0, "curve", &directrix.0);
                }
            }
            ProceduralSurfaceDefinition::Sweep { profile, spine } => {
                for curve in [profile, spine] {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset { support, .. } => {
                if !ids.surfaces.contains(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                for curve in [first, second] {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                for curve in [first, second] {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Blend {
                supports,
                spine,
                native,
                ..
            } => {
                for support in supports.iter().flatten() {
                    if !ids.surfaces.contains(&support.surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &support.surface.0);
                    }
                }
                if let Some(spine) = spine {
                    if !ids.curves.contains(&spine.0) {
                        ref_error(findings, &procedural.id.0, "curve", &spine.0);
                    }
                }
                if let Some(native) = native {
                    let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                        if !ids.curves.contains(&curve.0) {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    };
                    let check_surface =
                        |surface: &crate::ids::SurfaceId, findings: &mut Vec<Finding>| {
                            if !ids.surfaces.contains(&surface.0) {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        };
                    check_curve(&native.slice, findings);
                    for side in native.sides.iter() {
                        check_curve(&side.curve, findings);
                        if let Some(surface) = &side.surface {
                            check_surface(surface, findings);
                        }
                        if let Some(surface) = &side.exact_support {
                            check_surface(surface, findings);
                        }
                    }
                    if let Some(side) = &native.third {
                        check_curve(&side.curve, findings);
                        check_surface(&side.surface, findings);
                    }
                }
            }
            ProceduralSurfaceDefinition::Unknown {
                record: Some(record),
            } => {
                if !ids.unknowns.contains(&record.0) {
                    ref_error(findings, &procedural.id.0, "unknown record", &record.0);
                }
            }
            ProceduralSurfaceDefinition::Unknown { record: None } => {}
        }
    }
    for procedural in &ir.model.procedural_curves {
        if !ids.curves.contains(&procedural.curve.0) {
            ref_error(findings, &procedural.curve.0, "curve", &procedural.curve.0);
        }
        match &procedural.definition {
            ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix { .. } => {}
            ProceduralCurveDefinition::Compound { components, .. } => {
                for component in components {
                    if !ids.curves.contains(&component.0) {
                        ref_error(findings, &procedural.id.0, "curve", &component.0);
                    }
                }
            }
            ProceduralCurveDefinition::Intersection { context } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides.iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette {
                context,
                cast_surface,
                ..
            } => {
                if !ids.surfaces.contains(&cast_surface.0) {
                    ref_error(findings, &procedural.id.0, "surface", &cast_surface.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                if !ids.curves.contains(&base.0) {
                    ref_error(findings, &procedural.id.0, "curve", &base.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Spring { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Deformable { bend, data, .. } => {
                if !ids.curves.contains(&bend.0) {
                    ref_error(findings, &procedural.id.0, "curve", &bend.0);
                }
                if let crate::geometry::DeformableCurveData::Surface { surface } = data {
                    if !ids.surfaces.contains(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                if !ids.curves.contains(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Offset {
                source, support, ..
            } => {
                if !ids.curves.contains(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
                if let Some(support) = support {
                    if !ids.surfaces.contains(&support.0) {
                        ref_error(findings, &procedural.id.0, "surface", &support.0);
                    }
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                if !ids.curves.contains(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::Subset { source, .. } => {
                if !ids.curves.contains(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    if !ids.surfaces.contains(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Unknown {
                record: Some(record),
            } => {
                if !ids.unknowns.contains(&record.0) {
                    ref_error(findings, &procedural.id.0, "unknown record", &record.0);
                }
            }
            ProceduralCurveDefinition::Unknown { record: None } => {}
        }
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.0.as_str())
        .collect::<HashSet<_>>();
    for parameter in &ir.model.parameters {
        if !features.contains(parameter.owner.0.as_str()) {
            ref_error(findings, &parameter.id.0, "feature", &parameter.owner.0);
        }
    }
    let sketches = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.id.0.as_str())
        .collect::<HashSet<_>>();
    let sketch_entities = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.id.0.as_str())
        .collect::<HashSet<_>>();
    let sketch_entity_owners = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (entity.id.0.as_str(), entity.sketch.0.as_str()))
        .collect::<HashMap<_, _>>();
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.0.as_str())
        .collect::<HashSet<_>>();
    for sketch in &ir.model.sketches {
        for entity_use in sketch.profiles.iter().flatten() {
            if !sketch_entities.contains(entity_use.entity.0.as_str()) {
                ref_error(
                    findings,
                    &sketch.id.0,
                    "sketch entity",
                    &entity_use.entity.0,
                );
            }
        }
    }
    for entity in &ir.model.sketch_entities {
        if !sketches.contains(entity.sketch.0.as_str()) {
            ref_error(findings, &entity.id.0, "sketch", &entity.sketch.0);
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if !sketches.contains(constraint.sketch.0.as_str()) {
            ref_error(findings, &constraint.id.0, "sketch", &constraint.sketch.0);
        }
        let (entities, parameter) = match &constraint.definition {
            Definition::Coincident { entities }
            | Definition::Distance {
                entities,
                parameter: _,
            }
            | Definition::Native { entities, .. } => (entities.clone(), None),
            Definition::Horizontal { entity }
            | Definition::Vertical { entity }
            | Definition::Fixed { entity } => (vec![entity.clone()], None),
            Definition::Parallel { first, second }
            | Definition::Perpendicular { first, second }
            | Definition::Tangent { first, second }
            | Definition::Equal { first, second }
            | Definition::Concentric { first, second }
            | Definition::Collinear { first, second } => {
                (vec![first.clone(), second.clone()], None)
            }
            Definition::CoincidentLoci { loci } => {
                (loci.iter().map(locus_entity).cloned().collect(), None)
            }
            Definition::Midpoint { point, entity } => {
                (vec![locus_entity(point).clone(), entity.clone()], None)
            }
            Definition::Symmetric {
                first,
                second,
                axis,
            } => (
                vec![
                    locus_entity(first).clone(),
                    locus_entity(second).clone(),
                    axis.clone(),
                ],
                None,
            ),
            Definition::DistanceLoci {
                first,
                second,
                parameter,
            }
            | Definition::HorizontalDistance {
                first,
                second,
                parameter,
            }
            | Definition::VerticalDistance {
                first,
                second,
                parameter,
            } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                Some(parameter.0.as_str()),
            ),
            Definition::Angle {
                first,
                second,
                parameter,
            } => (
                vec![first.clone(), second.clone()],
                Some(parameter.0.as_str()),
            ),
            Definition::Radius { entity, parameter }
            | Definition::Diameter { entity, parameter } => {
                (vec![entity.clone()], Some(parameter.0.as_str()))
            }
        };
        let parameter = parameter.or(match &constraint.definition {
            Definition::Distance { parameter, .. } => Some(parameter.0.as_str()),
            _ => None,
        });
        for entity in entities {
            if !sketch_entities.contains(entity.0.as_str()) {
                ref_error(findings, &constraint.id.0, "sketch entity", &entity.0);
            } else if sketch_entity_owners.get(entity.0.as_str()).copied()
                != Some(constraint.sketch.0.as_str())
            {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!("sketch entity `{}` belongs to a different sketch", entity.0),
                    entity: Some(constraint.id.0.clone()),
                });
            }
        }
        if let Some(parameter) = parameter {
            if !parameters.contains(parameter) {
                ref_error(findings, &constraint.id.0, "parameter", parameter);
            }
        }
    }
    check_feature_sketch_references(ir, &sketches, findings);
    check_feature_references(ir, ids, findings);
}

fn check_feature_references(ir: &CadIr, ids: &IdSets, findings: &mut Vec<Finding>) {
    use crate::features::{
        BodySelection, EdgeSelection, Extent, FaceSelection, FeatureDefinition, PathRef, ProfileRef,
    };

    for configuration in &ir.model.configurations {
        let mut seen = HashSet::new();
        for body in &configuration.bodies {
            if !ids.bodies.contains(&body.0) {
                ref_error(findings, &configuration.id.0, "configuration body", &body.0);
            }
            if !seen.insert(body) {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: format!("configuration repeats body `{}`", body.0),
                    entity: Some(configuration.id.0.clone()),
                });
            }
        }
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.0.as_str(), feature.ordinal))
        .collect::<HashMap<_, _>>();
    for feature in &ir.model.features {
        if let Some(parent) = &feature.parent {
            match features.get(parent.0.as_str()) {
                None => ref_error(findings, &feature.id.0, "parent feature", &parent.0),
                Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!("parent feature `{}` does not precede its child", parent.0),
                    entity: Some(feature.id.0.clone()),
                }),
                Some(_) => {}
            }
        }
        for body in &feature.outputs {
            if !ids.bodies.contains(&body.0) {
                ref_error(findings, &feature.id.0, "output body", &body.0);
            }
        }

        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        let mut extents = Vec::new();
        let mut edge_selections = Vec::new();
        let mut face_selections = Vec::new();
        let mut body_selections = Vec::new();
        match &feature.definition {
            FeatureDefinition::Extrude {
                profile, extent, ..
            } => {
                profiles.push(profile);
                extents.push(extent);
            }
            FeatureDefinition::Revolve { profile, angle, .. } => {
                profiles.push(profile);
                extents.push(angle);
            }
            FeatureDefinition::Sweep { profile, path, .. } => {
                profiles.push(profile);
                paths.push(path);
            }
            FeatureDefinition::Loft {
                profiles: values,
                guides,
                ..
            } => {
                profiles.extend(values);
                paths.extend(guides);
            }
            FeatureDefinition::Rib { profile, .. } => {
                profiles.push(profile);
            }
            FeatureDefinition::Fillet { edges, .. } | FeatureDefinition::Chamfer { edges, .. } => {
                edge_selections.push(edges);
            }
            FeatureDefinition::Shell { removed_faces, .. } => {
                face_selections.push(removed_faces);
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                ..
            } => {
                face_selections.push(faces);
                face_selections.push(neutral_plane);
            }
            FeatureDefinition::DeleteFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::MoveFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                body_selections.push(target);
                body_selections.push(tools);
            }
            FeatureDefinition::Hole { face, extent, .. } => {
                face_selections.extend(face);
                extents.push(extent);
            }
            FeatureDefinition::Pattern { seeds, .. } => {
                for seed in seeds {
                    match features.get(seed.0.as_str()) {
                        None => ref_error(findings, &feature.id.0, "seed feature", &seed.0),
                        Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "seed feature `{}` does not precede its pattern",
                                seed.0
                            ),
                            entity: Some(feature.id.0.clone()),
                        }),
                        Some(_) => {}
                    }
                }
            }
            FeatureDefinition::Sketch { sketch } => {
                if let Some(sketch) = sketch {
                    if !ir.model.sketches.iter().any(|value| value.id == *sketch) {
                        ref_error(findings, &feature.id.0, "owned sketch", &sketch.0);
                    }
                }
            }
            FeatureDefinition::Native { .. } => {}
        }
        for profile in profiles {
            if let ProfileRef::Faces(faces) = profile {
                check_ids(
                    findings,
                    &feature.id.0,
                    "profile face",
                    faces.iter().map(|id| id.0.as_str()),
                    &ids.faces,
                );
            }
        }
        for path in paths {
            match path {
                PathRef::Edges(edges) => check_ids(
                    findings,
                    &feature.id.0,
                    "path edge",
                    edges.iter().map(|id| id.0.as_str()),
                    &ids.edges,
                ),
                PathRef::Curves(curves) => check_ids(
                    findings,
                    &feature.id.0,
                    "path curve",
                    curves.iter().map(|id| id.0.as_str()),
                    &ids.curves,
                ),
                PathRef::Native(_) | PathRef::Sketch(_) => {}
            }
        }
        for extent in extents {
            if let Extent::ToFace { face } = extent {
                if !ids.faces.contains(&face.0) {
                    ref_error(findings, &feature.id.0, "termination face", &face.0);
                }
            }
        }
        for selection in edge_selections {
            if let EdgeSelection::Edges(edges) = selection {
                check_ids(
                    findings,
                    &feature.id.0,
                    "selected edge",
                    edges.iter().map(|id| id.0.as_str()),
                    &ids.edges,
                );
            }
        }
        for selection in face_selections {
            if let FaceSelection::Faces(faces) = selection {
                check_ids(
                    findings,
                    &feature.id.0,
                    "selected face",
                    faces.iter().map(|id| id.0.as_str()),
                    &ids.faces,
                );
            }
        }
        for selection in body_selections {
            if let BodySelection::Bodies(bodies) = selection {
                check_ids(
                    findings,
                    &feature.id.0,
                    "selected body",
                    bodies.iter().map(|id| id.0.as_str()),
                    &ids.bodies,
                );
            }
        }
    }
}

fn check_ids<'a>(
    findings: &mut Vec<Finding>,
    owner: &str,
    kind: &str,
    values: impl Iterator<Item = &'a str>,
    valid: &HashSet<String>,
) {
    for value in values {
        if !valid.contains(value) {
            ref_error(findings, owner, kind, value);
        }
    }
}

fn check_feature_sketch_references(
    ir: &CadIr,
    sketches: &HashSet<&str>,
    findings: &mut Vec<Finding>,
) {
    use crate::features::{FeatureDefinition, PathRef, ProfileRef};

    for feature in &ir.model.features {
        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        match &feature.definition {
            FeatureDefinition::Extrude { profile, .. }
            | FeatureDefinition::Revolve { profile, .. }
            | FeatureDefinition::Rib { profile, .. } => profiles.push(profile),
            FeatureDefinition::Sweep { profile, path, .. } => {
                profiles.push(profile);
                paths.push(path);
            }
            FeatureDefinition::Loft {
                profiles: sections,
                guides,
                ..
            } => {
                profiles.extend(sections);
                paths.extend(guides);
            }
            _ => {}
        }
        for profile in profiles {
            if let ProfileRef::Sketch(sketch) = profile {
                if !sketches.contains(sketch.0.as_str()) {
                    ref_error(findings, &feature.id.0, "sketch profile", &sketch.0);
                }
            }
        }
        for path in paths {
            if let PathRef::Sketch(sketch) = path {
                if !sketches.contains(sketch.0.as_str()) {
                    ref_error(findings, &feature.id.0, "sketch path", &sketch.0);
                }
            }
        }
    }
}

fn locus_entity(locus: &SketchLocus) -> &crate::sketches::SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

pub(super) fn check_loops(ir: &CadIr, findings: &mut Vec<Finding>) {
    let by_id: HashMap<&str, &Coedge> = ir
        .model
        .coedges
        .iter()
        .map(|c| (c.id.0.as_str(), c))
        .collect();

    for lp in &ir.model.loops {
        if lp.coedges.is_empty() {
            findings.push(Finding {
                check: Check::LoopClosure,
                severity: Severity::Error,
                message: "loop has no coedges".into(),
                entity: Some(lp.id.0.clone()),
            });
            continue;
        }
        // Walk the `next` chain from the first listed coedge and confirm it is a
        // simple cycle whose members are exactly the loop's coedge set.
        let expected: HashSet<&str> = lp.coedges.iter().map(|c| c.0.as_str()).collect();
        let start = lp.coedges[0].0.as_str();
        let mut visited: HashSet<&str> = HashSet::new();
        let mut cur = start;
        let mut broke = false;
        for _ in 0..lp.coedges.len() {
            if !visited.insert(cur) {
                break; // returned early to an already-seen node
            }
            match by_id.get(cur) {
                Some(ce) => cur = ce.next.0.as_str(),
                None => {
                    broke = true; // dangling next; referential check already flags it
                    break;
                }
            }
        }
        if broke {
            continue;
        }
        if visited != expected || cur != start {
            findings.push(Finding {
                check: Check::LoopClosure,
                severity: Severity::Error,
                message: format!(
                    "coedge `next` ring does not close over the loop's {} coedges",
                    lp.coedges.len()
                ),
                entity: Some(lp.id.0.clone()),
            });
        }
    }
}

pub(super) fn check_coedge_pairing(ir: &CadIr, findings: &mut Vec<Finding>) {
    let by_id: HashMap<&str, &Coedge> = ir
        .model
        .coedges
        .iter()
        .map(|c| (c.id.0.as_str(), c))
        .collect();
    for coedge in &ir.model.coedges {
        let mut current = coedge;
        let mut closed = false;
        let mut members = 1usize;
        for _ in 0..=ir.model.coedges.len() {
            let Some(next) = by_id.get(current.radial_next.0.as_str()) else {
                break;
            };
            if next.edge != coedge.edge {
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring crosses edges".into(),
                    entity: Some(coedge.id.0.clone()),
                });
                break;
            }
            if next.id == coedge.id {
                closed = true;
                break;
            }
            members += 1;
            current = next;
        }
        if !closed {
            findings.push(Finding {
                check: Check::CoedgePairing,
                severity: Severity::Error,
                message: "radial ring does not close".into(),
                entity: Some(coedge.id.0.clone()),
            });
        } else if members == 2 {
            if let Some(other) = by_id.get(coedge.radial_next.0.as_str()) {
                if other.sense == coedge.sense {
                    findings.push(Finding {
                        check: Check::CoedgePairing,
                        severity: Severity::Warning,
                        message: "two-member radial ring has equal coedge senses".into(),
                        entity: Some(coedge.id.0.clone()),
                    });
                }
            }
        }
    }
}

pub(super) fn check_wire_topology(ir: &CadIr, findings: &mut Vec<Finding>) {
    let coedge_edges = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.0.as_str())
        .collect::<HashSet<_>>();
    let edge_vertices = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.0.as_str(), edge.end.0.as_str()])
        .collect::<HashSet<_>>();
    let mut wire_owners = HashMap::<&str, usize>::new();
    let mut free_owners = HashMap::<&str, usize>::new();

    for shell in &ir.model.shells {
        if shell.faces.is_empty() && shell.wire_edges.is_empty() && shell.free_vertices.is_empty() {
            wire_error(findings, &shell.id.0, "shell owns no topology");
        }
        for edge in &shell.wire_edges {
            *wire_owners.entry(&edge.0).or_default() += 1;
            if coedge_edges.contains(edge.0.as_str()) {
                wire_error(
                    findings,
                    &shell.id.0,
                    "wire edge is also referenced by a coedge",
                );
            }
        }
        for vertex in &shell.free_vertices {
            *free_owners.entry(&vertex.0).or_default() += 1;
            if edge_vertices.contains(vertex.0.as_str()) {
                wire_error(
                    findings,
                    &shell.id.0,
                    "free vertex is also referenced by an edge",
                );
            }
        }
    }
    for edge in &ir.model.edges {
        if !coedge_edges.contains(edge.id.0.as_str())
            && wire_owners.get(edge.id.0.as_str()).copied().unwrap_or(0) != 1
        {
            wire_error(
                findings,
                &edge.id.0,
                "wire edge must belong to exactly one shell",
            );
        }
    }
    for vertex in &ir.model.vertices {
        if !edge_vertices.contains(vertex.id.0.as_str())
            && free_owners.get(vertex.id.0.as_str()).copied().unwrap_or(0) != 1
        {
            wire_error(
                findings,
                &vertex.id.0,
                "free vertex must belong to exactly one shell",
            );
        }
    }

    let regions = ir
        .model
        .regions
        .iter()
        .map(|region| (region.id.0.as_str(), region))
        .collect::<HashMap<_, _>>();
    let shells = ir
        .model
        .shells
        .iter()
        .map(|shell| (shell.id.0.as_str(), shell))
        .collect::<HashMap<_, _>>();
    for body in &ir.model.bodies {
        if body.kind == crate::topology::BodyKind::Wire
            && body.regions.iter().any(|region_id| {
                regions.get(region_id.0.as_str()).is_some_and(|region| {
                    region.shells.iter().any(|shell_id| {
                        shells
                            .get(shell_id.0.as_str())
                            .is_some_and(|shell| !shell.faces.is_empty())
                    })
                })
            })
        {
            wire_error(findings, &body.id.0, "wire body contains faces");
        }
    }
}

pub(super) fn wire_error(findings: &mut Vec<Finding>, id: &str, message: &str) {
    findings.push(Finding {
        check: Check::WireTopology,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(id.into()),
    });
}
