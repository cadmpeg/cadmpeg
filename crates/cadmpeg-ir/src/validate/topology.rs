// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::features::{
    ChamferSpec, FaceMotion, FeatureSourceContent, FlexMode, HoleKind, Length, PatternKind,
    RadiusSpec,
};
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
            ProceduralSurfaceDefinition::Skin { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &IdSets,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !ids.curves.contains(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, ids, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                };
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        check_curve(path, findings);
                        for profile in profiles {
                            check_curve(&profile.curve, findings);
                            if !ids.surfaces.contains(&profile.data.surface.0) {
                                ref_error(
                                    findings,
                                    &procedural.id.0,
                                    "surface",
                                    &profile.data.surface.0,
                                );
                            }
                        }
                    }
                    crate::geometry::SkinSurfaceLayout::Compact {
                        curve,
                        secondary_curve,
                        ..
                    } => {
                        check_curve(curve, findings);
                        check_curve(secondary_curve, findings);
                    }
                }
                check_curve(&construction.parameter_curve, findings);
                for variable in &construction.formula.variables {
                    check_law_curves(variable, ids, procedural, findings);
                }
            }
            ProceduralSurfaceDefinition::Net { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &IdSets,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !ids.curves.contains(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, ids, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                for entry in construction
                    .sections
                    .iter()
                    .flat_map(|section| &section.entries)
                {
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
                for formula in construction.formulas.iter() {
                    for variable in &formula.variables {
                        check_law_curves(variable, ids, procedural, findings);
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
            ProceduralSurfaceDefinition::Sweep {
                profile,
                spine,
                native,
            } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &IdSets,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !ids.curves.contains(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, ids, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                for curve in [profile, spine] {
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
                if let Some(native) = native {
                    let formulas: Vec<_> = match &native.layout {
                        crate::geometry::SweepSurfaceLayout::ProfileFirst { formulas, .. } => {
                            formulas.iter().collect()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitFormula {
                            formula, ..
                        } => {
                            vec![formula]
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitGuide {
                            guide_curve, ..
                        } => {
                            if !ids.curves.contains(&guide_curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &guide_curve.0);
                            }
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            if !ids.surfaces.contains(&support_surface.0) {
                                ref_error(
                                    findings,
                                    &procedural.id.0,
                                    "surface",
                                    &support_surface.0,
                                );
                            }
                            if let Some(curve) = auxiliary_curve {
                                if !ids.curves.contains(&curve.0) {
                                    ref_error(findings, &procedural.id.0, "curve", &curve.0);
                                }
                            }
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::LawDriven {
                            first_law,
                            second_law,
                            formula,
                            ..
                        } => {
                            check_law_curves(first_law, ids, procedural, findings);
                            check_law_curves(second_law, ids, procedural, findings);
                            vec![formula]
                        }
                    };
                    for formula in formulas {
                        for variable in &formula.variables {
                            check_law_curves(variable, ids, procedural, findings);
                        }
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
            ProceduralSurfaceDefinition::Helix { .. }
            | ProceduralSurfaceDefinition::TSpline { .. }
            | ProceduralSurfaceDefinition::Unknown { record: None } => {}
            ProceduralSurfaceDefinition::Deformable { construction } => {
                if !ids.surfaces.contains(&construction.support.0) {
                    ref_error(
                        findings,
                        &procedural.id.0,
                        "surface",
                        &construction.support.0,
                    );
                }
                if let crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    surface, curve, ..
                }
                | crate::geometry::DeformableSurfaceData::Full { surface, curve, .. } =
                    &construction.data
                {
                    if !ids.surfaces.contains(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                    if !ids.curves.contains(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        if !ids.curves.contains(&procedural.curve.0) {
            ref_error(findings, &procedural.curve.0, "curve", &procedural.curve.0);
        }
        match &procedural.definition {
            ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix { .. } => {}
            ProceduralCurveDefinition::Law {
                context,
                primary,
                additional,
                ..
            } => {
                fn check(
                    expression: &crate::geometry::LawExpression,
                    ids: &IdSets,
                    procedural: &crate::geometry::ProceduralCurve,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !ids.curves.contains(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check(operand, ids, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !ids.surfaces.contains(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    for variable in &formula.variables {
                        check(variable, ids, procedural, findings);
                    }
                }
            }
            ProceduralCurveDefinition::Compound { components, .. } => {
                for component in components {
                    if !ids.curves.contains(&component.0) {
                        ref_error(findings, &procedural.id.0, "curve", &component.0);
                    }
                }
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
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
    let feature_ordinals = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<HashMap<_, _>>();
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, (&parameter.owner, parameter.ordinal)))
        .collect::<HashMap<_, _>>();
    let mut parameter_names = HashSet::new();
    let mut parameter_ordinals = HashSet::new();
    for parameter in &ir.model.parameters {
        if !features.contains(parameter.owner.0.as_str()) {
            ref_error(findings, &parameter.id.0, "feature", &parameter.owner.0);
        }
        if !parameter_names.insert((&parameter.owner, parameter.name.as_str())) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!(
                    "feature {} repeats parameter name `{}`",
                    parameter.owner, parameter.name
                ),
                entity: Some(parameter.id.0.clone()),
            });
        }
        if !parameter_ordinals.insert((&parameter.owner, parameter.ordinal)) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!(
                    "feature {} repeats parameter ordinal {}",
                    parameter.owner, parameter.ordinal
                ),
                entity: Some(parameter.id.0.clone()),
            });
        }
        let mut dependencies = HashSet::new();
        for dependency in &parameter.dependencies {
            if !dependencies.insert(dependency) {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: format!(
                        "parameter {} repeats dependency `{}`",
                        parameter.id.0, dependency.0
                    ),
                    entity: Some(parameter.id.0.clone()),
                });
                continue;
            }
            let Some((owner, ordinal)) = parameters.get(dependency) else {
                ref_error(
                    findings,
                    &parameter.id.0,
                    "parameter dependency",
                    &dependency.0,
                );
                continue;
            };
            let precedes = if *owner == &parameter.owner {
                *ordinal < parameter.ordinal
            } else {
                feature_ordinals
                    .get(*owner)
                    .zip(feature_ordinals.get(&parameter.owner))
                    .is_some_and(|(dependency_owner, parameter_owner)| {
                        dependency_owner < parameter_owner
                    })
            };
            if !precedes {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "parameter dependency `{}` does not precede its consumer",
                        dependency.0
                    ),
                    entity: Some(parameter.id.0.clone()),
                });
            }
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
            | Definition::Native {
                entities,
                parameter: None,
                ..
            } => (entities.clone(), None),
            Definition::Native {
                entities,
                parameter: Some(parameter),
                ..
            } => (entities.clone(), Some(parameter.0.as_str())),
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
            Definition::TangentLoci { first, second } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                None,
            ),
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
        BodySelection, EdgeSelection, Extent, FaceSelection, FeatureDefinition, PathRef,
        ProfileRef, ScaleCenter,
    };

    let mut configuration_ordinals = HashSet::new();
    let mut configuration_source_indices = HashSet::new();
    let mut active_configurations = 0;
    for configuration in &ir.model.configurations {
        active_configurations += usize::from(configuration.active);
        if !configuration_ordinals.insert(configuration.ordinal) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!(
                    "design repeats configuration ordinal {}",
                    configuration.ordinal
                ),
                entity: Some(configuration.id.0.clone()),
            });
        }
        if let Some(source_index) = configuration.source_index {
            if !configuration_source_indices.insert(source_index) {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: format!("design repeats configuration source index {source_index}"),
                    entity: Some(configuration.id.0.clone()),
                });
            }
        }
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
    if active_configurations > 1 {
        findings.push(Finding {
            check: Check::Counts,
            severity: Severity::Error,
            message: "design has multiple active configurations".into(),
            entity: None,
        });
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.0.as_str(), feature.ordinal))
        .collect::<HashMap<_, _>>();
    let parameters_by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, &parameter.owner))
        .collect::<HashMap<_, _>>();
    let mut feature_ordinals = HashSet::new();
    for feature in &ir.model.features {
        if !feature_ordinals.insert(feature.ordinal) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!("design repeats feature ordinal {}", feature.ordinal),
                entity: Some(feature.id.0.clone()),
            });
        }
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
        let mut dependencies = HashSet::new();
        for dependency in &feature.dependencies {
            if !dependencies.insert(dependency) {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!("feature repeats dependency `{}`", dependency.0),
                    entity: Some(feature.id.0.clone()),
                });
                continue;
            }
            match features.get(dependency.0.as_str()) {
                None => ref_error(findings, &feature.id.0, "dependency feature", &dependency.0),
                Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "dependency feature `{}` does not precede its consumer",
                        dependency.0
                    ),
                    entity: Some(feature.id.0.clone()),
                }),
                Some(_) => {}
            }
        }
        let mut content_parameters = HashSet::new();
        let mut content_features = HashSet::new();
        for item in &feature.source_content {
            match item {
                FeatureSourceContent::Text(_) => {}
                FeatureSourceContent::Parameter(parameter) => {
                    if !content_parameters.insert(parameter) {
                        findings.push(Finding {
                            check: Check::Counts,
                            severity: Severity::Error,
                            message: format!("feature repeats content parameter `{}`", parameter.0),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
                    match parameters_by_id.get(parameter) {
                        None => {
                            ref_error(findings, &feature.id.0, "content parameter", &parameter.0);
                        }
                        Some(owner) if *owner != &feature.id => findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "content parameter `{}` belongs to another feature",
                                parameter.0
                            ),
                            entity: Some(feature.id.0.clone()),
                        }),
                        Some(_) => {}
                    }
                }
                FeatureSourceContent::Feature(child) => {
                    if !content_features.insert(child) {
                        findings.push(Finding {
                            check: Check::Counts,
                            severity: Severity::Error,
                            message: format!("feature repeats content child `{}`", child.0),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
                    match features.get(child.0.as_str()) {
                        None => ref_error(findings, &feature.id.0, "content child", &child.0),
                        Some(ordinal) if *ordinal <= feature.ordinal => findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "content child `{}` does not follow its parent",
                                child.0
                            ),
                            entity: Some(feature.id.0.clone()),
                        }),
                        Some(_) => {}
                    }
                }
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
            FeatureDefinition::Revolve { construction, .. } => {
                profiles.extend(&construction.profile);
                extents.extend(&construction.extent);
                if construction.axis.as_ref().is_some_and(|axis| {
                    !axis.origin.x.is_finite()
                        || !axis.origin.y.is_finite()
                        || !axis.origin.z.is_finite()
                        || !valid_feature_direction(axis.direction)
                }) {
                    feature_geometry_error(findings, feature, "revolution axis is invalid");
                }
            }
            FeatureDefinition::Sweep {
                profile,
                path,
                twist,
                scale,
                ..
            } => {
                profiles.extend(profile);
                paths.extend(path);
                if twist.is_some_and(|value| !value.0.is_finite())
                    || scale.is_some_and(|value| !value.is_finite() || value <= 0.0)
                {
                    feature_geometry_error(findings, feature, "sweep magnitude is invalid");
                }
            }
            FeatureDefinition::Loft {
                profiles: values,
                guides,
                ..
            } => {
                profiles.extend(values);
                paths.extend(guides);
            }
            FeatureDefinition::Rib { construction, .. } => {
                profiles.extend(&construction.profile);
                if construction
                    .direction
                    .is_some_and(|value| !valid_feature_direction(value))
                    || construction
                        .thickness
                        .is_some_and(|value| !positive_feature_length(value))
                    || matches!(construction.draft, crate::features::RibDraft::Angle(value) if !value.0.is_finite())
                {
                    feature_geometry_error(findings, feature, "rib geometry is invalid");
                }
            }
            FeatureDefinition::Fillet { edges, radius } => {
                edge_selections.push(edges);
                let valid = match radius {
                    RadiusSpec::Unresolved { .. } => true,
                    RadiusSpec::Constant { radius } => positive_feature_length(*radius),
                    RadiusSpec::Variable { points } => {
                        points.len() >= 2
                            && points.iter().all(|point| {
                                point.parameter.is_finite()
                                    && (0.0..=1.0).contains(&point.parameter)
                                    && positive_feature_length(point.radius)
                            })
                            && points
                                .windows(2)
                                .all(|pair| pair[0].parameter < pair[1].parameter)
                    }
                };
                if !valid {
                    feature_geometry_error(findings, feature, "fillet radius is invalid");
                }
            }
            FeatureDefinition::Chamfer { edges, spec } => {
                edge_selections.push(edges);
                let valid = match spec {
                    ChamferSpec::Unresolved { .. } => true,
                    ChamferSpec::Distance { distance } => positive_feature_length(*distance),
                    ChamferSpec::TwoDistances { first, second } => {
                        positive_feature_length(*first) && positive_feature_length(*second)
                    }
                    ChamferSpec::DistanceAngle { distance, angle } => {
                        positive_feature_length(*distance)
                            && angle.0.is_finite()
                            && angle.0 > 0.0
                            && angle.0 < std::f64::consts::PI
                    }
                };
                if !valid {
                    feature_geometry_error(findings, feature, "chamfer dimensions are invalid");
                }
            }
            FeatureDefinition::Shell {
                removed_faces,
                thickness,
                ..
            } => {
                face_selections.push(removed_faces);
                if thickness.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "shell thickness is invalid");
                }
            }
            FeatureDefinition::Thicken {
                faces, thickness, ..
            } => {
                face_selections.push(faces);
                if thickness.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "thicken thickness is invalid");
                }
            }
            FeatureDefinition::OffsetSurface { faces, distance } => {
                face_selections.push(faces);
                if !distance.0.is_finite() {
                    feature_geometry_error(findings, feature, "surface offset is invalid");
                }
            }
            FeatureDefinition::KnitSurface {
                faces,
                gap_tolerance,
                ..
            } => {
                face_selections.push(faces);
                if gap_tolerance.is_some_and(|value| !value.0.is_finite() || value.0 < 0.0) {
                    feature_geometry_error(findings, feature, "knit tolerance is invalid");
                }
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                edge_selections.push(boundary);
                face_selections.push(support_faces);
            }
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                face_selections.push(faces);
                paths.push(tool);
            }
            FeatureDefinition::ExtendSurface {
                faces, distance, ..
            } => {
                face_selections.push(faces);
                if !positive_feature_length(*distance) {
                    feature_geometry_error(findings, feature, "surface extension is invalid");
                }
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                mode,
            } => {
                edge_selections.push(edges);
                face_selections.push(support_faces);
                let valid = match mode {
                    crate::features::RuledSurfaceMode::Normal { distance }
                    | crate::features::RuledSurfaceMode::Tangent { distance } => {
                        positive_feature_length(*distance)
                    }
                    crate::features::RuledSurfaceMode::Direction {
                        direction,
                        distance,
                    } => valid_feature_direction(*direction) && positive_feature_length(*distance),
                };
                if !valid {
                    feature_geometry_error(findings, feature, "ruled surface is invalid");
                }
            }
            FeatureDefinition::Draft {
                faces,
                neutral_plane,
                pull_direction,
                angle,
                ..
            } => {
                face_selections.push(faces);
                face_selections.push(neutral_plane);
                if !valid_feature_direction(*pull_direction) || !angle.0.is_finite() {
                    feature_geometry_error(findings, feature, "draft geometry is invalid");
                }
            }
            FeatureDefinition::DeleteFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => {
                face_selections.push(targets);
                face_selections.push(replacements);
            }
            FeatureDefinition::MoveFace { faces, motion } => {
                face_selections.push(faces);
                let valid = match motion {
                    FaceMotion::Offset { distance } => distance.0.is_finite(),
                    FaceMotion::Translate {
                        direction,
                        distance,
                    } => valid_feature_direction(*direction) && distance.0.is_finite(),
                    FaceMotion::Rotate {
                        axis_origin,
                        axis_dir,
                        angle,
                    } => {
                        axis_origin.x.is_finite()
                            && axis_origin.y.is_finite()
                            && axis_origin.z.is_finite()
                            && valid_feature_direction(*axis_dir)
                            && angle.0.is_finite()
                    }
                };
                if !valid {
                    feature_geometry_error(findings, feature, "face motion is invalid");
                }
            }
            FeatureDefinition::MoveBody {
                bodies,
                translation,
                rotation,
                ..
            } => {
                body_selections.push(bodies);
                let valid_translation = [translation.x, translation.y, translation.z]
                    .into_iter()
                    .all(f64::is_finite);
                let valid_rotation = rotation.as_ref().is_none_or(|rotation| {
                    [
                        rotation.origin.x,
                        rotation.origin.y,
                        rotation.origin.z,
                        rotation.angle.0,
                    ]
                    .into_iter()
                    .all(f64::is_finite)
                        && valid_feature_direction(rotation.direction)
                });
                if !valid_translation || !valid_rotation {
                    feature_geometry_error(findings, feature, "body motion is invalid");
                }
            }
            FeatureDefinition::Dome { faces, height, .. } => {
                face_selections.push(faces);
                if height.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "dome height is invalid");
                }
            }
            FeatureDefinition::Flex { axis, mode } => {
                if axis.is_some_and(|axis| !axis.norm().is_finite() || axis.norm() <= 0.0) {
                    findings.push(Finding {
                        check: Check::GeometricConsistency,
                        severity: Severity::Error,
                        message: "flex axis is degenerate".into(),
                        entity: Some(feature.id.0.clone()),
                    });
                }
                let valid = match mode {
                    FlexMode::Unresolved {
                        angle,
                        factor,
                        distance,
                        ..
                    } => {
                        angle.is_none_or(|value| value.0.is_finite())
                            && factor.is_none_or(|value| value.is_finite() && value > 0.0)
                            && distance.is_none_or(|value| value.0.is_finite())
                    }
                    FlexMode::Bending { angle } | FlexMode::Twisting { angle } => {
                        angle.0.is_finite()
                    }
                    FlexMode::Tapering { factor } => factor.is_finite() && *factor > 0.0,
                    FlexMode::Stretching { distance } => distance.0.is_finite(),
                };
                if !valid {
                    findings.push(Finding {
                        check: Check::GeometricConsistency,
                        severity: Severity::Error,
                        message: "flex magnitude is invalid".into(),
                        entity: Some(feature.id.0.clone()),
                    });
                }
            }
            FeatureDefinition::Scale {
                bodies,
                center,
                factors,
            } => {
                body_selections.push(bodies);
                let center_valid = center.as_ref().is_none_or(|center| match center {
                    ScaleCenter::Point(point) => {
                        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                    }
                    ScaleCenter::Native(reference) => !reference.is_empty(),
                    ScaleCenter::Centroid | ScaleCenter::ModelOrigin => true,
                });
                if !center_valid
                    || ![factors.uniform, factors.x, factors.y, factors.z]
                        .into_iter()
                        .flatten()
                        .all(|factor| factor.is_finite() && factor != 0.0)
                {
                    feature_geometry_error(findings, feature, "scale transform is invalid");
                }
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                body_selections.push(target);
                body_selections.push(tools);
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                body_selections.push(targets);
                face_selections.push(tools);
            }
            FeatureDefinition::DeleteBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureDefinition::Hole {
                face,
                kind,
                diameter,
                extent,
                direction,
                position,
            } => {
                face_selections.extend(face);
                extents.extend(extent);
                let kind_valid = match kind {
                    HoleKind::Unresolved {
                        counterbore_diameter,
                        counterbore_depth,
                        countersink_diameter,
                        countersink_angle,
                        ..
                    } => {
                        counterbore_diameter.is_none_or(positive_feature_length)
                            && counterbore_depth.is_none_or(positive_feature_length)
                            && countersink_diameter.is_none_or(positive_feature_length)
                            && countersink_angle.is_none_or(|value| {
                                value.0.is_finite()
                                    && value.0 > 0.0
                                    && value.0 < std::f64::consts::PI
                            })
                    }
                    HoleKind::Simple => true,
                    HoleKind::Counterbore { diameter, depth } => {
                        positive_feature_length(*diameter) && positive_feature_length(*depth)
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        positive_feature_length(*diameter)
                            && angle.0.is_finite()
                            && angle.0 > 0.0
                            && angle.0 < std::f64::consts::PI
                    }
                };
                let position_valid = position.is_none_or(|point| {
                    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
                });
                if diameter.is_some_and(|value| !positive_feature_length(value))
                    || !kind_valid
                    || !position_valid
                    || direction.is_some_and(|value| !valid_feature_direction(value))
                {
                    feature_geometry_error(findings, feature, "hole geometry is invalid");
                }
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                if let PatternKind::CurveDriven {
                    path: Some(path), ..
                } = pattern
                {
                    paths.push(path);
                }
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
                let valid = match pattern {
                    PatternKind::Unresolved { .. } => true,
                    PatternKind::Linear {
                        direction,
                        spacing,
                        count,
                    } => {
                        direction.is_none_or(valid_feature_direction)
                            && positive_feature_length(*spacing)
                            && *count > 0
                    }
                    PatternKind::Circular {
                        axis_origin,
                        axis_dir,
                        angle,
                        count,
                    } => {
                        axis_origin.x.is_finite()
                            && axis_origin.y.is_finite()
                            && axis_origin.z.is_finite()
                            && valid_feature_direction(*axis_dir)
                            && angle.0.is_finite()
                            && angle.0 > 0.0
                            && *count > 0
                    }
                    PatternKind::CurveDriven { spacing, count, .. } => {
                        positive_feature_length(*spacing) && *count > 0
                    }
                    PatternKind::Mirror {
                        plane_origin,
                        plane_normal,
                    } => {
                        plane_origin.x.is_finite()
                            && plane_origin.y.is_finite()
                            && plane_origin.z.is_finite()
                            && valid_feature_direction(*plane_normal)
                    }
                };
                if !valid {
                    feature_geometry_error(findings, feature, "pattern geometry is invalid");
                }
            }
            FeatureDefinition::Sketch { space, sketch } => {
                if matches!(space, crate::features::SketchSpace::Spatial) && sketch.is_some() {
                    feature_geometry_error(
                        findings,
                        feature,
                        "spatial sketch owns planar sketch geometry",
                    );
                }
                if let Some(sketch) = sketch {
                    if !ir.model.sketches.iter().any(|value| value.id == *sketch) {
                        ref_error(findings, &feature.id.0, "owned sketch", &sketch.0);
                    }
                }
            }
            FeatureDefinition::DatumCoordinateSystem {
                origin,
                x_axis,
                y_axis,
                z_axis,
            } => {
                let dot = |left: crate::math::Vector3, right: crate::math::Vector3| {
                    left.x * right.x + left.y * right.y + left.z * right.z
                };
                let cross = crate::math::Vector3::new(
                    x_axis.y * y_axis.z - x_axis.z * y_axis.y,
                    x_axis.z * y_axis.x - x_axis.x * y_axis.z,
                    x_axis.x * y_axis.y - x_axis.y * y_axis.x,
                );
                let valid = [origin.x, origin.y, origin.z]
                    .into_iter()
                    .all(f64::is_finite)
                    && [x_axis, y_axis, z_axis]
                        .into_iter()
                        .all(|axis| (axis.norm() - 1.0).abs() <= 1.0e-9)
                    && dot(*x_axis, *y_axis).abs() <= 1.0e-9
                    && dot(*x_axis, *z_axis).abs() <= 1.0e-9
                    && dot(*y_axis, *z_axis).abs() <= 1.0e-9
                    && dot(cross, *z_axis) >= 1.0 - 1.0e-9;
                if !valid {
                    feature_geometry_error(findings, feature, "coordinate-system frame is invalid");
                }
            }
            FeatureDefinition::EquationCurve {
                parameter,
                x_expression,
                y_expression,
                z_expression,
                start,
                end,
            } => {
                if parameter.trim().is_empty()
                    || x_expression.trim().is_empty()
                    || y_expression.trim().is_empty()
                    || z_expression.trim().is_empty()
                    || !start.is_finite()
                    || !end.is_finite()
                    || start >= end
                {
                    feature_geometry_error(findings, feature, "equation curve is invalid");
                }
            }
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                ..
            } => {
                paths.push(source);
                face_selections.push(target_faces);
                if direction.is_some_and(|value| !valid_feature_direction(value)) {
                    feature_geometry_error(findings, feature, "projection direction is invalid");
                }
            }
            FeatureDefinition::CompositeCurve { segments, .. } => {
                paths.extend(segments);
                if segments.is_empty() {
                    feature_geometry_error(findings, feature, "composite curve is empty");
                }
            }
            FeatureDefinition::Helix {
                axis_origin,
                axis_direction,
                radius,
                pitch,
                revolutions,
                ..
            } => {
                let valid = [axis_origin.x, axis_origin.y, axis_origin.z, pitch.0]
                    .into_iter()
                    .all(f64::is_finite)
                    && valid_feature_direction(*axis_direction)
                    && radius.0.is_finite()
                    && radius.0 > 0.0
                    && revolutions.is_finite()
                    && *revolutions > 0.0;
                if !valid {
                    feature_geometry_error(findings, feature, "helix geometry is invalid");
                }
            }
            FeatureDefinition::HelixNativeAxis {
                axis_native_ref,
                radius,
                height,
                revolutions,
                start_angle,
                ..
            } => {
                let valid = !axis_native_ref.is_empty()
                    && radius.0.is_finite()
                    && radius.0 > 0.0
                    && height.0.is_finite()
                    && revolutions.is_finite()
                    && *revolutions > 0.0
                    && start_angle.0.is_finite();
                if !valid {
                    feature_geometry_error(findings, feature, "native-axis helix is invalid");
                }
            }
            FeatureDefinition::Wrap {
                profile,
                face,
                mode,
                depth,
            } => {
                profiles.push(profile);
                face_selections.push(face);
                let valid = match mode {
                    crate::features::WrapMode::Emboss | crate::features::WrapMode::Deboss => {
                        depth.is_some_and(positive_feature_length)
                    }
                    crate::features::WrapMode::Scribe => depth.is_none(),
                };
                if !valid {
                    feature_geometry_error(findings, feature, "wrap depth is invalid");
                }
            }
            FeatureDefinition::TreeNode { .. }
            | FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlane { .. }
            | FeatureDefinition::DatumAxis { .. }
            | FeatureDefinition::DatumPoint { .. }
            | FeatureDefinition::Native { .. } => {}
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } => {
                if let Some(reference) = reference {
                    match features.get(reference.0.as_str()) {
                        None => ref_error(findings, &feature.id.0, "reference plane", &reference.0),
                        Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "reference plane `{}` does not precede its offset plane",
                                reference.0
                            ),
                            entity: Some(feature.id.0.clone()),
                        }),
                        Some(_) => {}
                    }
                }
                if !distance.0.is_finite() {
                    feature_geometry_error(findings, feature, "datum-plane offset is invalid");
                }
            }
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
            let valid_magnitude = match extent {
                Extent::Blind { length } | Extent::Symmetric { length } => {
                    length.0.is_finite() && length.0 > 0.0
                }
                Extent::TwoSided { first, second } => {
                    first.0.is_finite() && first.0 > 0.0 && second.0.is_finite() && second.0 > 0.0
                }
                Extent::Angle { angle } | Extent::SymmetricAngle { angle } => {
                    angle.0.is_finite() && angle.0 > 0.0
                }
                Extent::TwoSidedAngles { first, second } => {
                    first.0.is_finite() && first.0 > 0.0 && second.0.is_finite() && second.0 > 0.0
                }
                Extent::ThroughAll | Extent::ToFace { .. } => true,
            };
            if !valid_magnitude {
                findings.push(Finding {
                    check: Check::GeometricConsistency,
                    severity: Severity::Error,
                    message: "feature extent magnitude is invalid".into(),
                    entity: Some(feature.id.0.clone()),
                });
            }
            if let Extent::ToFace {
                face: FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. },
            } = extent
            {
                check_ids(
                    findings,
                    &feature.id.0,
                    "termination face",
                    faces.iter().map(|id| id.0.as_str()),
                    &ids.faces,
                );
            }
        }
        for selection in edge_selections {
            if let EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } = selection {
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
            if let FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } = selection {
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
            if let BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } =
                selection
            {
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

fn positive_feature_length(value: Length) -> bool {
    value.0.is_finite() && value.0 > 0.0
}

fn valid_feature_direction(value: Vector3) -> bool {
    value.norm().is_finite() && value.norm() > 0.0
}

fn feature_geometry_error(findings: &mut Vec<Finding>, feature: &Feature, message: &str) {
    findings.push(Finding {
        check: Check::GeometricConsistency,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(feature.id.0.clone()),
    });
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

    let mut owners = HashMap::new();
    for feature in &ir.model.features {
        if let FeatureDefinition::Sketch {
            sketch: Some(sketch),
            ..
        } = &feature.definition
        {
            if owners
                .insert(sketch.0.as_str(), (feature.id.0.as_str(), feature.ordinal))
                .is_some()
            {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!("sketch `{}` has multiple owning features", sketch.0),
                    entity: Some(feature.id.0.clone()),
                });
            }
        }
    }

    for feature in &ir.model.features {
        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        match &feature.definition {
            FeatureDefinition::Extrude { profile, .. } => {
                profiles.push(profile);
            }
            FeatureDefinition::Rib { construction, .. } => {
                profiles.extend(&construction.profile);
            }
            FeatureDefinition::Revolve { construction, .. } => {
                profiles.extend(&construction.profile);
            }
            FeatureDefinition::Sweep { profile, path, .. } => {
                profiles.extend(profile);
                paths.extend(path);
            }
            FeatureDefinition::Loft {
                profiles: sections,
                guides,
                ..
            } => {
                profiles.extend(sections);
                paths.extend(guides);
            }
            FeatureDefinition::Pattern {
                pattern:
                    PatternKind::CurveDriven {
                        path: Some(path), ..
                    },
                ..
            } => paths.push(path),
            _ => {}
        }
        for profile in profiles {
            if let ProfileRef::Sketch(sketch) = profile {
                if !sketches.contains(sketch.0.as_str()) {
                    ref_error(findings, &feature.id.0, "sketch profile", &sketch.0);
                } else if let Some((owner, ordinal)) = owners.get(sketch.0.as_str()) {
                    if *ordinal >= feature.ordinal {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "sketch owner `{owner}` does not precede its profile consumer"
                            ),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
                }
            }
        }
        for path in paths {
            if let PathRef::Sketch(sketch) = path {
                if !sketches.contains(sketch.0.as_str()) {
                    ref_error(findings, &feature.id.0, "sketch path", &sketch.0);
                } else if let Some((owner, ordinal)) = owners.get(sketch.0.as_str()) {
                    if *ordinal >= feature.ordinal {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "sketch owner `{owner}` does not precede its path consumer"
                            ),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
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
