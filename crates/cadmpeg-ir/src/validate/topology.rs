// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;

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
            unknowns: ir.unknowns.iter().map(|e| e.id.0.clone()).collect(),
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

pub(super) fn native_ref_error(
    findings: &mut Vec<Finding>,
    owner: &str,
    target_kind: &str,
    target: &str,
) {
    findings.push(Finding {
        check: Check::NativeLinks,
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
    for link in ir
        .native
        .f3d
        .iter()
        .flat_map(|native| &native.persistent_design_links)
    {
        use crate::attributes::AttributeTarget;
        let owner = format!("persistent-design-link:{}", link.design_id);
        match &link.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if !ids.bodies.contains(&id.0) => {
                native_ref_error(findings, &owner, "body", &id.0);
            }
            AttributeTarget::Face(id) if !ids.faces.contains(&id.0) => {
                native_ref_error(findings, &owner, "face", &id.0);
            }
            AttributeTarget::Coedge(id) if !ids.coedges.contains(&id.0) => {
                native_ref_error(findings, &owner, "coedge", &id.0);
            }
            AttributeTarget::Edge(id) if !ids.edges.contains(&id.0) => {
                native_ref_error(findings, &owner, "edge", &id.0);
            }
            AttributeTarget::Vertex(id) if !ids.vertices.contains(&id.0) => {
                native_ref_error(findings, &owner, "vertex", &id.0);
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
    for link in ir
        .native
        .f3d
        .iter()
        .flat_map(|native| &native.sketch_curve_links)
    {
        if !ids.coedges.contains(&link.coedge.0) {
            native_ref_error(findings, &link.id, "coedge", &link.coedge.0);
        }
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
