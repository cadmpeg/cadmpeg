// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.
#![allow(clippy::wildcard_imports)]

use std::collections::BTreeSet;

use super::*;
use crate::features::{
    BodySelection, DatumPlaneReference, ExtrudeStart, FaceSelection, FeatureSourceContent,
    PatternKind, PatternSeed, PatternTransform, SplitFaceTool, UnresolvedFamily,
};

fn collect_pattern_paths<'a>(
    pattern: &'a PatternKind,
    paths: &mut Vec<&'a crate::features::PathRef>,
) {
    match pattern.definition() {
        PatternTransform::CurveDriven {
            path: Some(path), ..
        } => paths.push(path),
        PatternTransform::Composite { stages } => {
            for stage in stages {
                collect_pattern_paths(&stage.pattern, paths);
            }
        }
        _ => {}
    }
}
use crate::index::ModelIndex;
use crate::sketches::{SketchConstraintDefinitionInput as Definition, SketchLocus};

pub(super) fn ref_error(findings: &mut Vec<Finding>, owner: &str, target_kind: &str, target: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: format!("references missing {target_kind} `{target}`"),
        entity: Some(owner.to_string()),
    });
}

pub(super) fn check_tolerances(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.tolerances.linear.get() > 1.0e6 || ir.tolerances.angular.get() > std::f64::consts::TAU {
        findings.push(Finding {
            check: Check::Tolerances,
            severity: Severity::Warning,
            message: "document tolerance is outside a sane canonical range".into(),
            entity: None,
        });
    }
}

pub(super) fn check_references(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    for b in &ir.model.bodies {
        for l in &b.regions {
            if ids.regions(l.as_str()).is_none() {
                ref_error(findings, b.id.as_str(), "region", l.as_str());
            }
        }
    }
    for l in &ir.model.regions {
        if ids.bodies(l.body.as_str()).is_none() {
            ref_error(findings, l.id.as_str(), "body", l.body.as_str());
        }
        for s in &l.shells {
            if ids.shells(s.as_str()).is_none() {
                ref_error(findings, l.id.as_str(), "shell", s.as_str());
            }
        }
    }
    for s in &ir.model.shells {
        if ids.regions(s.region.as_str()).is_none() {
            ref_error(findings, s.id.as_str(), "region", s.region.as_str());
        }
        for f in s.faces() {
            if ids.faces(f.as_str()).is_none() {
                ref_error(findings, s.id.as_str(), "face", f.as_str());
            }
        }
        for e in s.wire_edges() {
            if ids.edges(e.as_str()).is_none() {
                ref_error(findings, s.id.as_str(), "wire edge", e.as_str());
            }
        }
        for v in s.free_vertices() {
            if ids.vertices(v.as_str()).is_none() {
                ref_error(findings, s.id.as_str(), "free vertex", v.as_str());
            }
        }
    }
    for f in &ir.model.faces {
        if ids.shells(f.shell.as_str()).is_none() {
            ref_error(findings, f.id.as_str(), "shell", f.shell.as_str());
        }
        if ids.surfaces(f.surface.as_str()).is_none() {
            ref_error(findings, f.id.as_str(), "surface", f.surface.as_str());
        }
        for lp in &f.loops {
            if ids.loops(lp.as_str()).is_none() {
                ref_error(findings, f.id.as_str(), "loop", lp.as_str());
            }
        }
    }
    for lp in &ir.model.loops {
        if ids.faces(lp.face.as_str()).is_none() {
            ref_error(findings, lp.id.as_str(), "face", lp.face.as_str());
        }
        match &lp.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, pcurves } => {
                if ids.vertices(vertex.as_str()).is_none() {
                    ref_error(findings, lp.id.as_str(), "vertex", vertex.as_str());
                }
                for pcurve in pcurves {
                    if ids.pcurves(pcurve.pcurve.as_str()).is_none() {
                        ref_error(
                            findings,
                            lp.id.as_str(),
                            "pcurve(vertex use)",
                            pcurve.pcurve.as_str(),
                        );
                    }
                }
            }
            crate::topology::LoopBoundary::Ring(ring) => {
                for ce in ring.coedges() {
                    if ids.coedges(ce.as_str()).is_none() {
                        ref_error(findings, lp.id.as_str(), "coedge", ce.as_str());
                    }
                }
                for use_ in ring.vertex_uses() {
                    if ids.vertices(use_.vertex.as_str()).is_none() {
                        ref_error(findings, lp.id.as_str(), "vertex", use_.vertex.as_str());
                    }
                    let after = &use_.after;
                    if ids.coedges(after.as_str()).is_none() {
                        ref_error(
                            findings,
                            lp.id.as_str(),
                            "coedge(vertex-use after)",
                            after.as_str(),
                        );
                    }
                    for pcurve in &use_.pcurves {
                        if ids.pcurves(pcurve.pcurve.as_str()).is_none() {
                            ref_error(
                                findings,
                                lp.id.as_str(),
                                "pcurve(vertex use)",
                                pcurve.pcurve.as_str(),
                            );
                        }
                    }
                }
            }
        }
    }
    for ce in &ir.model.coedges {
        if ids.loops(ce.owner_loop.as_str()).is_none() {
            ref_error(findings, ce.id.as_str(), "loop", ce.owner_loop.as_str());
        }
        if ids.edges(ce.edge.as_str()).is_none() {
            ref_error(findings, ce.id.as_str(), "edge", ce.edge.as_str());
        }
        if ids.coedges(ce.radial_next.as_str()).is_none() {
            ref_error(
                findings,
                ce.id.as_str(),
                "coedge(radial_next)",
                ce.radial_next.as_str(),
            );
        }
        for use_ in &ce.pcurves {
            if ids.pcurves(use_.pcurve.as_str()).is_none() {
                ref_error(findings, ce.id.as_str(), "pcurve", use_.pcurve.as_str());
            }
        }
        if let Some(curve) = &ce.use_curve {
            if ids.curves(curve.curve.as_str()).is_none() {
                ref_error(
                    findings,
                    ce.id.as_str(),
                    "coedge use curve",
                    curve.curve.as_str(),
                );
            }
        }
    }
    for e in &ir.model.edges {
        if let Some(c) = e.curve() {
            if ids.curves(c.as_str()).is_none() {
                ref_error(findings, e.id.as_str(), "curve", c.as_str());
            }
        }
        if ids.vertices(e.start.as_str()).is_none() {
            ref_error(findings, e.id.as_str(), "vertex(start)", e.start.as_str());
        }
        if ids.vertices(e.end.as_str()).is_none() {
            ref_error(findings, e.id.as_str(), "vertex(end)", e.end.as_str());
        }
    }
    for v in &ir.model.vertices {
        if ids.points(v.point.as_str()).is_none() {
            ref_error(findings, v.id.as_str(), "point", v.point.as_str());
        }
    }
    for binding in &ir.model.appearance_bindings {
        use crate::appearance::AppearanceTarget;
        let owner = format!("appearance-binding:{}", binding.appearance.as_str());
        if ids.appearances(binding.appearance.as_str()).is_none() {
            ref_error(findings, &owner, "appearance", binding.appearance.as_str());
        }
        match &binding.target {
            AppearanceTarget::Body(body) if ids.bodies(body.as_str()).is_none() => {
                ref_error(findings, &owner, "body", body.as_str());
            }
            AppearanceTarget::Face(face) if ids.faces(face.as_str()).is_none() => {
                ref_error(findings, &owner, "face", face.as_str());
            }
            AppearanceTarget::Edge(edge) if ids.edges(edge.as_str()).is_none() => {
                ref_error(findings, &owner, "edge", edge.as_str());
            }
            AppearanceTarget::Vertex(vertex) if ids.vertices(vertex.as_str()).is_none() => {
                ref_error(findings, &owner, "vertex", vertex.as_str());
            }
            AppearanceTarget::Surface(surface) if ids.surfaces(surface.as_str()).is_none() => {
                ref_error(findings, &owner, "surface", surface.as_str());
            }
            AppearanceTarget::Curve(curve) if ids.curves(curve.as_str()).is_none() => {
                ref_error(findings, &owner, "curve", curve.as_str());
            }
            AppearanceTarget::Point(point) if ids.points(point.as_str()).is_none() => {
                ref_error(findings, &owner, "point", point.as_str());
            }
            AppearanceTarget::Tessellation(tessellation)
                if ids.tessellations(tessellation).is_none() =>
            {
                ref_error(findings, &owner, "tessellation", tessellation);
            }
            AppearanceTarget::Source { .. } => {}
            _ => {}
        }
    }
    for attribute in &ir.model.attributes {
        use crate::attributes::AttributeTarget;
        let owner = attribute.id.as_str();
        match &attribute.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if ids.bodies(id.as_str()).is_none() => {
                ref_error(findings, owner, "body", id.as_str());
            }
            AttributeTarget::Face(id) if ids.faces(id.as_str()).is_none() => {
                ref_error(findings, owner, "face", id.as_str());
            }
            AttributeTarget::Coedge(id) if ids.coedges(id.as_str()).is_none() => {
                ref_error(findings, owner, "coedge", id.as_str());
            }
            AttributeTarget::Edge(id) if ids.edges(id.as_str()).is_none() => {
                ref_error(findings, owner, "edge", id.as_str());
            }
            AttributeTarget::Vertex(id) if ids.vertices(id.as_str()).is_none() => {
                ref_error(findings, owner, "vertex", id.as_str());
            }
            _ => {}
        }
    }
    for s in &ir.model.surfaces {
        match &s.geometry {
            SurfaceGeometry::Procedural { construction, .. } => {
                if ids.procedural_surfaces(construction.as_str()).is_none() {
                    ref_error(
                        findings,
                        s.id.as_str(),
                        "procedural surface construction",
                        construction.as_str(),
                    );
                }
            }
            SurfaceGeometry::Unknown { record: Some(u) } if !ids.contains(u.as_str()) => {
                ref_error(findings, s.id.as_str(), "unknown record", u.as_str());
            }
            _ => {}
        }
    }
    for curve in &ir.model.curves {
        match &curve.geometry {
            CurveGeometry::Procedural { construction, .. } => {
                if ids.procedural_curves(construction.as_str()).is_none() {
                    ref_error(
                        findings,
                        curve.id.as_str(),
                        "procedural curve construction",
                        construction.as_str(),
                    );
                }
            }
            CurveGeometry::Unknown {
                record: Some(unknown),
            } => {
                if !ids.contains(unknown.as_str()) {
                    ref_error(
                        findings,
                        curve.id.as_str(),
                        "unknown record",
                        unknown.as_str(),
                    );
                }
            }
            CurveGeometry::Composite { segments, .. } => {
                for segment in segments {
                    if ids.curves(segment.curve.as_str()).is_none() {
                        ref_error(findings, curve.id.as_str(), "curve", segment.curve.as_str());
                    }
                }
            }
            _ => {}
        }
    }
    let composite_segments = ir
        .model
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            CurveGeometry::Composite { segments, .. } => Some((
                curve.id.as_str(),
                segments
                    .iter()
                    .map(|segment| segment.curve.as_str())
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut complete = HashSet::new();
    let mut active = HashSet::new();
    for curve in composite_segments.keys().copied() {
        check_composite_cycle(
            curve,
            &composite_segments,
            &mut active,
            &mut complete,
            findings,
        );
    }
    for procedural in &ir.model.procedural_surfaces {
        match procedural.definition() {
            ProceduralSurfaceDefinition::Exact { .. } => {}
            ProceduralSurfaceDefinition::Compound { components, .. } => {
                for component in components {
                    if ids.surfaces(component.component.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            component.component.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::SubSurface { support, .. }
            | ProceduralSurfaceDefinition::Replica {
                source: support, ..
            } => {
                if ids.surfaces(support.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                if ids.surfaces(support.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
                if ids.curves(reference.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        reference.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Loft { sections, .. } => {
                for entry in sections.iter().flat_map(|section| &section.entries) {
                    for curve in entry
                        .path
                        .curve
                        .iter()
                        .map(|curve| &curve.id)
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.curve.id))
                    {
                        if ids.curves(curve.as_str()).is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = member.form.surface() {
                            if ids.surfaces(surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                );
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                };
                let mut scales = construction.scales.as_slice().iter().collect::<Vec<_>>();
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
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
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
                        let surface = &member.data.surface;
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                };
                let mut scales = construction.scales.as_slice().iter().collect::<Vec<_>>();
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
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
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
                        let surface = &member.data.surface;
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Skin { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(curve.id.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.id.as_str(),
                                );
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
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                };
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        check_curve(path, findings);
                        for profile in profiles {
                            check_curve(&profile.curve, findings);
                            let surface = &profile.data.surface;
                            if ids.surfaces(surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
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
                for variable in construction.formula.variables() {
                    check_law_curves(variable, ids, procedural, findings);
                }
            }
            ProceduralSurfaceDefinition::Law { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(curve.id.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.id.as_str(),
                                );
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
                for formula in
                    std::iter::once(&construction.primary).chain(&construction.additional)
                {
                    for variable in formula.variables() {
                        check_law_curves(variable, ids, procedural, findings);
                    }
                }
            }
            ProceduralSurfaceDefinition::Net { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(curve.id.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.id.as_str(),
                                );
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
                    for curve in entry
                        .path
                        .curve
                        .iter()
                        .map(|curve| &curve.id)
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.curve.id))
                    {
                        if ids.curves(curve.as_str()).is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = member.form.surface() {
                            if ids.surfaces(surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                );
                            }
                        }
                    }
                }
                for formula in construction.formulas.iter() {
                    for variable in formula.variables() {
                        check_law_curves(variable, ids, procedural, findings);
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend { construction } => {
                for surface in [&construction.first.surface, &construction.second.surface]
                    .into_iter()
                    .chain(std::iter::once(&construction.second_exact_surface))
                {
                    if ids.surfaces(surface.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        );
                    }
                }
                if let crate::geometry::G2BlendFirstShape::Full {
                    support: Some(support),
                } = &construction.first_shape
                {
                    if ids.surfaces(support.surface.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.surface.as_str(),
                        );
                    }
                }
                for curve in [
                    &construction.first.curve,
                    &construction.second.curve,
                    &construction.center_curve,
                ] {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::VariableBlend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.surface.as_str(),
                            );
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(curve.curve.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.curve.as_str(),
                            );
                        }
                    }
                }
                for curve in [
                    Some(&construction.slice),
                    construction
                        .secondary_curve
                        .as_ref()
                        .map(|curve| &curve.curve),
                    construction.post_curve.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                for member in construction
                    .base_profile
                    .iter()
                    .chain(construction.entries.iter().flat_map(|entry| &entry.profile))
                {
                    if ids.curves(member.curve.id.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            member.curve.id.as_str(),
                        );
                    }
                    if let Some(surface) = member.form.surface() {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
                for curve in std::iter::once(&construction.base_path)
                    .chain(construction.entries.iter().map(|entry| &entry.path))
                    .flat_map(|path| {
                        path.curve
                            .iter()
                            .map(|curve| &curve.id)
                            .chain(path.auxiliaries.iter())
                    })
                    .chain(match &construction.direction {
                        crate::geometry::CompoundLoftDirection::Vector { .. } => None,
                        crate::geometry::CompoundLoftDirection::Curve { curve, .. } => Some(curve),
                    })
                    .chain(construction.tail.curve())
                {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.surface.as_str(),
                            );
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(curve.curve.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.curve.as_str(),
                            );
                        }
                    }
                }
                if ids.curves(construction.center.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        construction.center.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::VertexBlend { construction } => {
                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            if ids.curves(curve.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.as_str(),
                                );
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            if ids.surfaces(surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                );
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Degenerate { .. } => {}
                    }
                }
            }
            ProceduralSurfaceDefinition::Extrusion { directrix, .. }
            | ProceduralSurfaceDefinition::LinearSweep { directrix, .. }
            | ProceduralSurfaceDefinition::Revolution { directrix, .. }
            | ProceduralSurfaceDefinition::AxisRevolution { directrix, .. } => {
                if ids.curves(directrix.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        directrix.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Sweep {
                profile,
                spine,
                native,
            } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(curve.id.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.id.as_str(),
                                );
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
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
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
                            if ids.curves(guide_curve.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    guide_curve.as_str(),
                                );
                            }
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            if ids.surfaces(support_surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    support_surface.as_str(),
                                );
                            }
                            if let Some(curve) = auxiliary_curve {
                                if ids.curves(curve.as_str()).is_none() {
                                    ref_error(
                                        findings,
                                        procedural.id.as_str(),
                                        "curve",
                                        curve.as_str(),
                                    );
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
                        for variable in formula.variables() {
                            check_law_curves(variable, ids, procedural, findings);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset { support, .. } => {
                if ids.surfaces(support.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Subset { support, .. }
            | ProceduralSurfaceDefinition::ParallelOffset { support, .. } => {
                if ids.surfaces(support.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                for curve in [first, second] {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                for curve in [first, second] {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
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
                    if ids.surfaces(support.surface.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.surface.as_str(),
                        );
                    }
                }
                if let Some(spine) = spine {
                    if ids.curves(spine.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", spine.as_str());
                    }
                }
                if let Some(native) = native {
                    let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                        if ids.curves(curve.as_str()).is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                    };
                    let check_surface =
                        |surface: &crate::ids::SurfaceId, findings: &mut Vec<Finding>| {
                            if ids.surfaces(surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                );
                            }
                        };
                    check_curve(&native.slice, findings);
                    for side in native.sides.iter() {
                        if let Some(curve) = &side.curve {
                            check_curve(&curve.curve, findings);
                        }
                        if let Some(surface) = &side.surface {
                            check_surface(&surface.surface, findings);
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
                if !ids.contains(record.as_str()) {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "unknown record",
                        record.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::RollingBallJet(_)
            | ProceduralSurfaceDefinition::Helix { .. }
            | ProceduralSurfaceDefinition::TSpline { .. }
            | ProceduralSurfaceDefinition::DegenerateTorus { .. }
            | ProceduralSurfaceDefinition::Unknown { record: None } => {}
            ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                boundary_pcurves,
                ..
            } => {
                if ids.surfaces(support.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
                for boundary in boundaries {
                    if ids.curves(boundary.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", boundary.as_str());
                    }
                }
                for pcurve in boundary_pcurves {
                    if ids.pcurves(pcurve.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "pcurve boundary",
                            pcurve.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::Deformable { construction } => {
                if ids.surfaces(construction.support.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        construction.support.as_str(),
                    );
                }
                if let crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    surface, curve, ..
                }
                | crate::geometry::DeformableSurfaceData::Full { surface, curve, .. } =
                    &construction.data
                {
                    if ids.surfaces(surface.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        );
                    }
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        match procedural.definition() {
            ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix(_) => {}
            ProceduralCurveDefinition::Law {
                context,
                primary,
                additional,
                ..
            } => {
                fn check(
                    expression: &crate::geometry::LawExpression,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralCurve,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(curve.id.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.id.as_str(),
                                );
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
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    for variable in formula.variables() {
                        check(variable, ids, procedural, findings);
                    }
                }
            }
            ProceduralCurveDefinition::Compound(compound) => {
                let (_, components) = compound.parts();

                for component in components {
                    if ids.curves(component.component.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            component.component.as_str(),
                        );
                    }
                }
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::TolerantIntersection {
                construction: intersection,
                ..
            } => {
                let (supports, _, _) = intersection.parts();

                for surface in supports {
                    if ids.surfaces(surface.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        );
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides().iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { family } => {
                for side in family.context().sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette {
                context,
                cast_surface,
                ..
            } => {
                if ids.surfaces(cast_surface.as_str()).is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        cast_surface.as_str(),
                    );
                }
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                if ids.curves(base.as_str()).is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", base.as_str());
                }
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Spring { layout, .. } => match layout.support_context() {
                Ok(context) => {
                    for side in context.sides() {
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str()).is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                );
                            }
                        }
                    }
                }
                Err(error) => ref_error(
                    findings,
                    procedural.id.as_str(),
                    "spring support context",
                    error,
                ),
            },
            ProceduralCurveDefinition::Deformable {
                context, source, ..
            } => {
                if let crate::geometry::DeformableCurveSource::Curve { curve } = source {
                    if ids.curves(curve.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                if ids.curves(source.as_str()).is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Offset {
                source,
                side,
                range,
                ..
            } => {
                if ids.curves(source.as_str()).is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
                if let crate::geometry::OffsetSide::Direction {
                    support: Some(support),
                    ..
                } = side
                {
                    if ids.surfaces(support.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        );
                    }
                }
                if let Some(crate::geometry::CurveOffsetRange::Variable {
                    distance_law:
                        crate::geometry::CurveOffsetDistanceLaw::Coordinate { function, .. },
                    ..
                }) = range
                {
                    if ids.curves(function.as_str()).is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", function.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::SpatialOffset { source, .. } => {
                if ids.curves(source.as_str()).is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str()).is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                if ids.curves(source.as_str()).is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
            }
            ProceduralCurveDefinition::Replica { source, .. }
            | ProceduralCurveDefinition::Subset { source, .. } => {
                if ids.curves(source.as_str()).is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    if ids.surfaces(surface.as_str()).is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        );
                    }
                }
            }
            ProceduralCurveDefinition::Unknown {
                native_kind: _,
                record: Some(record),
            } => {
                if !ids.contains(record.as_str()) {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "unknown record",
                        record.as_str(),
                    );
                }
            }
            ProceduralCurveDefinition::Unknown {
                native_kind: _,
                record: None,
            } => {}
        }
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
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
        .map(|parameter| (&parameter.id, (parameter.owner.as_ref(), parameter.ordinal)))
        .collect::<HashMap<_, _>>();
    let mut parameter_names = HashSet::new();
    let mut parameter_ordinals = HashSet::new();
    for parameter in &ir.model.parameters {
        if let Some(owner) = &parameter.owner {
            if !features.contains(owner.as_str()) {
                ref_error(findings, parameter.id.as_str(), "feature", owner.as_str());
            }
        }
        if !parameter_names.insert((&parameter.owner, parameter.name.as_str())) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!(
                    "parameter scope {:?} repeats parameter name `{}`",
                    parameter.owner, parameter.name
                ),
                entity: Some(parameter.id.as_str().to_owned()),
            });
        }
        if !parameter_ordinals.insert((&parameter.owner, parameter.ordinal)) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!(
                    "parameter scope {:?} repeats parameter ordinal {}",
                    parameter.owner, parameter.ordinal
                ),
                entity: Some(parameter.id.as_str().to_owned()),
            });
        }
        for dependency in &parameter.dependencies {
            let Some((owner, ordinal)) = parameters.get(dependency) else {
                ref_error(
                    findings,
                    parameter.id.as_str(),
                    "parameter dependency",
                    dependency.as_str(),
                );
                continue;
            };
            let precedes = if *owner == parameter.owner.as_ref() {
                *ordinal < parameter.ordinal
            } else {
                match (*owner, parameter.owner.as_ref()) {
                    (None, Some(_)) => true,
                    (Some(dependency_owner), Some(parameter_owner)) => feature_ordinals
                        .get(dependency_owner)
                        .zip(feature_ordinals.get(parameter_owner))
                        .is_some_and(|(dependency_owner, parameter_owner)| {
                            dependency_owner < parameter_owner
                        }),
                    (Some(_) | None, None) => false,
                }
            };
            if !precedes {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "parameter dependency `{}` does not precede its consumer",
                        dependency.as_str()
                    ),
                    entity: Some(parameter.id.as_str().to_owned()),
                });
            }
        }
    }
    let sketches = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect::<HashSet<_>>();
    let sketch_entities = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.id().as_str())
        .collect::<HashSet<_>>();
    let sketch_entity_owners = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (entity.id().as_str(), entity.sketch.as_str()))
        .collect::<HashMap<_, _>>();
    let parameters = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<HashSet<_>>();
    for sketch in &ir.model.sketches {
        for entity_use in sketch.profiles.iter().flatten() {
            if !sketch_entities.contains(entity_use.entity.as_str()) {
                ref_error(
                    findings,
                    sketch.id.as_str(),
                    "sketch entity",
                    entity_use.entity.as_str(),
                );
            }
        }
    }
    for entity in &ir.model.sketch_entities {
        if !sketches.contains(entity.sketch.as_str()) {
            ref_error(
                findings,
                entity.id().as_str(),
                "sketch",
                entity.sketch.as_str(),
            );
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if !sketches.contains(constraint.sketch.as_str()) {
            ref_error(
                findings,
                constraint.id.as_str(),
                "sketch",
                constraint.sketch.as_str(),
            );
        }
        let (entities, parameter) = match constraint.definition.kind() {
            Definition::Disabled => (Vec::new(), None),
            Definition::Polygon { polygon } => (polygon.entities().to_vec(), None),
            Definition::Coincident { entities }
            | Definition::SplineGroup { entities }
            | Definition::Distance {
                entities,
                parameter: _,
            }
            | Definition::Native {
                entities,
                parameter: None,
                ..
            } => (entities.clone(), None),
            Definition::RectangularPattern { pattern } => (
                pattern
                    .rows()
                    .iter()
                    .flatten()
                    .flat_map(|instance| instance.entities.iter().cloned())
                    .collect(),
                None,
            ),
            Definition::CircularPattern { pattern } => (
                std::iter::once(pattern.center().clone())
                    .chain(
                        pattern
                            .instances()
                            .iter()
                            .flat_map(|instance| instance.entities.iter().cloned()),
                    )
                    .collect(),
                None,
            ),
            Definition::TextFrame { text, frame } => (
                std::iter::once(text.clone())
                    .chain(frame.iter().cloned())
                    .collect(),
                None,
            ),
            Definition::TextPath { text, path, .. } => (vec![text.clone(), path.clone()], None),
            Definition::Native {
                entities,
                parameter: Some(parameter),
                ..
            } => (entities.clone(), Some(parameter.as_str())),
            Definition::Horizontal { entity }
            | Definition::Vertical { entity }
            | Definition::Fixed { entity }
            | Definition::ArcAngle { entity, .. }
            | Definition::EllipseAngle { entity, .. } => (vec![entity.clone()], None),
            Definition::Parallel { first, second }
            | Definition::Perpendicular { first, second }
            | Definition::Tangent { first, second }
            | Definition::Curvature { first, second }
            | Definition::Equal { first, second }
            | Definition::Concentric { first, second }
            | Definition::Coradial { first, second }
            | Definition::Collinear { first, second }
            | Definition::ProjectedCopy {
                source: first,
                result: second,
            } => (vec![first.clone(), second.clone()], None),
            Definition::InternalAlignment { helper, parent, .. } => {
                (vec![helper.clone(), parent.clone()], None)
            }
            Definition::Group { elements } | Definition::Text { elements, .. } => {
                (elements.iter().map(locus_entity).cloned().collect(), None)
            }
            Definition::CoincidentLoci { loci } => {
                (loci.iter().map(locus_entity).cloned().collect(), None)
            }
            Definition::SameCoordinate { relation } => (
                vec![
                    locus_entity(relation.first()).clone(),
                    locus_entity(relation.second()).clone(),
                ],
                None,
            ),
            Definition::TangentLoci { first, second } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                None,
            ),
            Definition::PointSymmetric {
                first,
                second,
                center,
            } => (
                vec![
                    locus_entity(first).clone(),
                    locus_entity(second).clone(),
                    locus_entity(center).clone(),
                ],
                None,
            ),
            Definition::Midpoint { point, entity } => {
                (vec![locus_entity(point).clone(), entity.clone()], None)
            }
            Definition::PointCoordinateValues { point, .. } => {
                (vec![locus_entity(point).clone()], None)
            }
            Definition::MidpointCoordinate { first, second, .. } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                None,
            ),
            Definition::AtIntersection {
                point,
                first,
                second,
            } => (
                vec![locus_entity(point).clone(), first.clone(), second.clone()],
                None,
            ),
            Definition::Offset {
                pairs, parameter, ..
            } => (
                pairs
                    .iter()
                    .flat_map(|pair| [pair.source.clone(), pair.result.clone()])
                    .collect(),
                parameter.as_ref().map(|parameter| parameter.id.as_str()),
            ),
            Definition::PointOnObject { point, entity } => {
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
            | Definition::PolarDistance {
                first,
                second,
                distance_parameter: Some(parameter),
                ..
            }
            | Definition::DistanceLociValue {
                first,
                second,
                parameter: Some(parameter),
                ..
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
                Some(parameter.as_str()),
            ),
            Definition::PolarDistance {
                first,
                second,
                distance_parameter: None,
                ..
            } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                None,
            ),
            Definition::DistanceLociValue {
                first,
                second,
                parameter: None,
                ..
            } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                None,
            ),
            Definition::AngleDifference { .. } => (Vec::new(), None),
            Definition::ScalarEquality { .. } => (Vec::new(), None),
            Definition::EqualDistance { first, second } => (
                vec![
                    locus_entity(&first.first).clone(),
                    locus_entity(&first.second).clone(),
                    locus_entity(&second.first).clone(),
                    locus_entity(&second.second).clone(),
                ],
                None,
            ),
            Definition::RepeatedDistance {
                measurements,
                parameter,
            } => (
                measurements
                    .iter()
                    .flat_map(|measurement| {
                        use crate::sketches::SketchDistanceMeasurement as Measurement;
                        let (first, second) = match measurement {
                            Measurement::Distance { first, second }
                            | Measurement::Horizontal { first, second }
                            | Measurement::Vertical { first, second } => (first, second),
                        };
                        [locus_entity(first).clone(), locus_entity(second).clone()]
                    })
                    .collect(),
                Some(parameter.as_str()),
            ),
            Definition::RepeatedLength {
                entities,
                parameter,
            } => (entities.clone(), Some(parameter.as_str())),
            Definition::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => (
                first.iter().chain(second).cloned().collect(),
                Some(parameter.as_str()),
            ),
            Definition::Angle {
                first,
                second,
                parameter,
            } => (
                vec![first.clone(), second.clone()],
                Some(parameter.as_str()),
            ),
            Definition::AngleToAxis {
                entity, parameter, ..
            } => (vec![entity.clone()], Some(parameter.as_str())),
            Definition::RepeatedRadius {
                entities,
                parameter,
            }
            | Definition::RepeatedDiameter {
                entities,
                parameter,
            } => (entities.clone(), Some(parameter.as_str())),
            Definition::Radius { entity, parameter }
            | Definition::Diameter { entity, parameter }
            | Definition::Weight { entity, parameter } => {
                (vec![entity.clone()], Some(parameter.as_str()))
            }
            Definition::SnellsLaw {
                incident,
                refracted,
                interface,
                parameter,
            } => (
                vec![
                    locus_entity(incident).clone(),
                    locus_entity(refracted).clone(),
                    interface.clone(),
                ],
                Some(parameter.as_str()),
            ),
        };
        let parameter = parameter.or(match constraint.definition.kind() {
            Definition::Distance { parameter, .. } => Some(parameter.as_str()),
            _ => None,
        });
        for entity in entities {
            if !sketch_entities.contains(entity.as_str()) {
                ref_error(
                    findings,
                    constraint.id.as_str(),
                    "sketch entity",
                    entity.as_str(),
                );
            } else if sketch_entity_owners.get(entity.as_str()).copied()
                != Some(constraint.sketch.as_str())
            {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "sketch entity `{}` belongs to a different sketch",
                        entity.as_str()
                    ),
                    entity: Some(constraint.id.as_str().to_owned()),
                });
            }
        }
        if let Some(parameter) = parameter {
            if !parameters.contains(parameter) {
                ref_error(findings, constraint.id.as_str(), "parameter", parameter);
            }
        }
        if let Definition::RectangularPattern { pattern } = constraint.definition.kind() {
            for parameter in pattern.directions().iter().flat_map(|direction| {
                [
                    direction
                        .distance
                        .as_ref()
                        .map(crate::sketches::SketchPatternDistance::parameter),
                    direction.count_parameter.as_ref(),
                ]
                .into_iter()
                .flatten()
            }) {
                if !parameters.contains(parameter.as_str()) {
                    ref_error(
                        findings,
                        constraint.id.as_str(),
                        "parameter",
                        parameter.as_str(),
                    );
                }
            }
        }
        if let Definition::CircularPattern { pattern } = constraint.definition.kind() {
            for parameter in [pattern.angle_parameter(), pattern.count_parameter()]
                .into_iter()
                .flatten()
            {
                if !parameters.contains(parameter.as_str()) {
                    ref_error(
                        findings,
                        constraint.id.as_str(),
                        "parameter",
                        parameter.as_str(),
                    );
                }
            }
        }
    }
    check_feature_sketch_references(ir, &sketches, findings);
    check_feature_references(ir, ids, findings);
}

fn check_feature_references(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    use crate::features::{EdgeSelection, FeatureDefinition, PathRef, ProfileRef, ScaleCenter};

    let mut configuration_ordinals = HashSet::new();
    let mut configuration_source_indices = HashSet::new();
    let mut active_configurations = 0;
    let parameter_ids = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<HashSet<_>>();
    let asset_ids = ir
        .model
        .assets
        .iter()
        .map(|asset| asset.id.as_str())
        .collect::<HashSet<_>>();
    let parameter_values = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.as_str(), parameter.value.as_ref()))
        .collect::<HashMap<_, _>>();
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.as_str(), feature.ordinal))
        .collect::<HashMap<_, _>>();
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
                entity: Some(configuration.id.as_str().to_owned()),
            });
        }
        if let Some(source_index) = configuration.source_index {
            if !configuration_source_indices.insert(source_index) {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: format!("design repeats configuration source index {source_index}"),
                    entity: Some(configuration.id.as_str().to_owned()),
                });
            }
        }
        for body in &configuration.bodies {
            if ids.bodies(body.as_str()).is_none() {
                ref_error(
                    findings,
                    configuration.id.as_str(),
                    "configuration body",
                    body.as_str(),
                );
            }
        }
        for parameter in configuration.parameter_overrides.keys() {
            if !parameter_ids.contains(parameter.as_str()) {
                ref_error(
                    findings,
                    configuration.id.as_str(),
                    "configuration parameter override",
                    parameter.as_str(),
                );
            }
        }
        let suppressed_features = configuration.suppressed_features().collect::<HashSet<_>>();
        if configuration.active {
            for feature in &ir.model.features {
                if feature.suppressed.is_some_and(|suppressed| {
                    suppressed_features.contains(&feature.id) != suppressed
                }) {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message:
                            "active configuration suppression disagrees with current feature state"
                                .into(),
                        entity: Some(configuration.id.as_str().to_owned()),
                    });
                }
            }
        }
        for (parameter, value) in &configuration.parameter_values {
            match parameter_values.get(parameter.as_str()) {
                None => ref_error(
                    findings,
                    configuration.id.as_str(),
                    "configuration parameter value",
                    parameter.as_str(),
                ),
                Some(baseline)
                    if baseline.is_some_and(|baseline| {
                        std::mem::discriminant(baseline) != std::mem::discriminant(value)
                    }) =>
                {
                    geometry_error(
                        findings,
                        configuration.id.as_str(),
                        "configuration parameter value is invalid",
                    );
                }
                Some(_) => {}
            }
        }
        for (feature, state) in &configuration.feature_states {
            let feature_ordinal = features.get(feature.as_str()).copied();
            if feature_ordinal.is_none() {
                ref_error(
                    findings,
                    configuration.id.as_str(),
                    "configuration feature state",
                    feature.as_str(),
                );
            }
            for dependency in &state.dependencies {
                match features.get(dependency.as_str()) {
                    None => ref_error(
                        findings,
                        configuration.id.as_str(),
                        "configuration feature dependency",
                        dependency.as_str(),
                    ),
                    Some(dependency_ordinal)
                        if feature_ordinal.is_some_and(|feature_ordinal| {
                            *dependency_ordinal >= feature_ordinal
                        }) =>
                    {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "configuration feature dependency `{}` does not precede `{}`",
                                dependency.as_str(),
                                feature.as_str()
                            ),
                            entity: Some(configuration.id.as_str().to_owned()),
                        });
                    }
                    Some(_) => {}
                }
            }
            for reference in regeneration_references(&state.definition) {
                match features.get(reference.as_str()) {
                    None => ref_error(
                        findings,
                        configuration.id.as_str(),
                        "configuration definition feature",
                        reference.as_str(),
                    ),
                    Some(reference_ordinal)
                        if feature_ordinal.is_some_and(|feature_ordinal| {
                            *reference_ordinal >= feature_ordinal
                        }) =>
                    {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "configuration definition feature `{}` does not precede `{}`",
                                reference.as_str(),
                                feature.as_str()
                            ),
                            entity: Some(configuration.id.as_str().to_owned()),
                        });
                    }
                    Some(_) if !state.dependencies.contains(reference) => {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
                                feature.as_str(), reference.as_str()
                            ),
                            entity: Some(configuration.id.as_str().to_owned()),
                        });
                    }
                    Some(_) => {}
                }
            }
            for output in state.evaluation.outputs() {
                if ids.bodies(output.as_str()).is_none() {
                    ref_error(
                        findings,
                        configuration.id.as_str(),
                        "configuration feature output",
                        output.as_str(),
                    );
                }
            }
        }
        check_configuration_state_closure(configuration, findings);
    }
    if active_configurations > 1 {
        findings.push(Finding {
            check: Check::Counts,
            severity: Severity::Error,
            message: "design has multiple active configurations".into(),
            entity: None,
        });
    }
    let feature_records = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let sketch_entities = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.id().as_str().to_owned())
        .collect::<HashSet<_>>();
    let spatial_sketch_entity_owners = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (entity.id().as_str(), entity.sketch.as_str()))
        .collect::<HashMap<_, _>>();
    let mut reported_plane_cycles = HashSet::new();
    for feature in &ir.model.features {
        let mut path = Vec::new();
        let mut positions = HashMap::new();
        let mut cursor = feature.id.as_str();
        loop {
            if let Some(&cycle_start) = positions.get(cursor) {
                let mut cycle = path[cycle_start..].to_vec();
                cycle.sort_unstable();
                if reported_plane_cycles.insert(cycle.clone()) {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "datum-plane reference cycle contains {}",
                            cycle
                                .iter()
                                .map(|id| format!("`{id}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        entity: Some(feature.id.as_str().to_owned()),
                    });
                }
                break;
            }
            positions.insert(cursor, path.len());
            path.push(cursor);
            let Some(next) = feature_records.get(cursor).and_then(|feature| {
                let FeatureDefinition::DatumOffsetPlane {
                    reference: Some(DatumPlaneReference::Feature(reference)),
                    ..
                } = feature.evaluation.definition()
                else {
                    return None;
                };
                Some(reference.as_str())
            }) else {
                break;
            };
            cursor = next;
        }
    }
    let parameters_by_id = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter.owner.as_ref()))
        .collect::<HashMap<_, _>>();
    let input_topologies = ir
        .model
        .feature_input_topologies
        .iter()
        .map(|state| (state.id.as_str(), state))
        .collect::<HashMap<_, _>>();
    let mut topology_owners = HashSet::new();
    for state in &ir.model.feature_input_topologies {
        if !features.contains_key(state.input_of.as_str()) {
            ref_error(
                findings,
                state.id.as_str(),
                "input feature",
                state.input_of.as_str(),
            );
        }
        if !topology_owners.insert(state.input_of.as_str()) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: "feature has multiple input topology states".into(),
                entity: Some(state.input_of.as_str().to_owned()),
            });
        }
    }
    let mut result_owners = HashSet::new();
    for state in &ir.model.feature_result_topologies {
        if !features.contains_key(state.output_of.as_str()) {
            ref_error(
                findings,
                state.id.as_str(),
                "result feature",
                state.output_of.as_str(),
            );
        }
        if !result_owners.insert(state.output_of.as_str()) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: "feature has multiple result topology states".into(),
                entity: Some(state.output_of.as_str().to_owned()),
            });
        }
    }
    let result_topologies_by_feature = ir
        .model
        .feature_result_topologies
        .iter()
        .map(|state| (state.output_of.as_str(), state))
        .collect::<HashMap<_, _>>();
    let mut feature_ordinals = HashSet::new();
    for feature in &ir.model.features {
        if !feature_ordinals.insert(feature.ordinal) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!("design repeats feature ordinal {}", feature.ordinal),
                entity: Some(feature.id.as_str().to_owned()),
            });
        }
        for dependency in &feature.dependencies {
            match features.get(dependency.as_str()) {
                None => ref_error(
                    findings,
                    feature.id.as_str(),
                    "dependency feature",
                    dependency.as_str(),
                ),
                Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "dependency feature `{}` does not precede its consumer",
                        dependency.as_str()
                    ),
                    entity: Some(feature.id.as_str().to_owned()),
                }),
                Some(_) => {}
            }
        }
        for item in &feature.source_content {
            match item {
                FeatureSourceContent::Text(_) => {}
                FeatureSourceContent::Parameter(parameter) => {
                    match parameters_by_id.get(parameter) {
                        None => {
                            ref_error(
                                findings,
                                feature.id.as_str(),
                                "content parameter",
                                parameter.as_str(),
                            );
                        }
                        Some(owner) if *owner != Some(&feature.id) => findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "content parameter `{}` belongs to another feature",
                                parameter.as_str()
                            ),
                            entity: Some(feature.id.as_str().to_owned()),
                        }),
                        Some(_) => {}
                    }
                }
                FeatureSourceContent::Feature(child) => match features.get(child.as_str()) {
                    None => ref_error(
                        findings,
                        feature.id.as_str(),
                        "content child",
                        child.as_str(),
                    ),
                    Some(ordinal) if *ordinal <= feature.ordinal => findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "content child `{}` does not follow its parent",
                            child.as_str()
                        ),
                        entity: Some(feature.id.as_str().to_owned()),
                    }),
                    Some(_) => {}
                },
            }
        }
        for body in feature.evaluation.outputs() {
            if ids.bodies(body.as_str()).is_none() {
                ref_error(findings, feature.id.as_str(), "output body", body.as_str());
            }
        }

        let mut paths = Vec::new();
        let mut edge_selections = Vec::new();
        let mut face_selections = Vec::new();
        let mut vertex_selections = Vec::new();
        let mut body_selections = Vec::new();
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. } => operation.as_ref(),
            definition => definition,
        };
        match definition {
            FeatureDefinition::Unresolved { .. }
            | FeatureDefinition::Primitive { .. }
            | FeatureDefinition::SheetMetalBaseFlange { .. }
            | FeatureDefinition::PlanarPatch { .. } => {}
            FeatureDefinition::ReferenceImage { asset, .. } => {
                if !asset_ids.contains(asset.as_str()) {
                    ref_error(
                        findings,
                        feature.id.as_str(),
                        "reference-image asset",
                        asset.as_str(),
                    );
                }
            }
            FeatureDefinition::Decal { asset, faces, .. } => {
                if !asset_ids.contains(asset.as_str()) {
                    ref_error(findings, feature.id.as_str(), "decal asset", asset.as_str());
                }
                face_selections.push(faces);
            }
            FeatureDefinition::Block { .. } => {}

            FeatureDefinition::ExtractBody { source } => body_selections.push(source),
            FeatureDefinition::FaceBlend { operands, .. } => {
                face_selections.push(operands.first_faces());
                face_selections.push(operands.second_faces());
            }
            FeatureDefinition::FullRoundFillet { groups } => {
                for group in groups {
                    face_selections.push(group.center_faces());
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        if let crate::features::FullRoundSideSelection::Explicit(selection) = side {
                            face_selections.push(selection);
                        }
                    }
                }
            }
            FeatureDefinition::SewBodies { bodies, .. } => body_selections.push(bodies),
            FeatureDefinition::BaseFeature { bodies } => body_selections.push(bodies),
            FeatureDefinition::MeshImport { tessellations } => {
                for tessellation in tessellations {
                    if ids.tessellations(tessellation).is_none() {
                        ref_error(
                            findings,
                            feature.id.as_str(),
                            "mesh import tessellation",
                            tessellation,
                        );
                    }
                }
            }
            FeatureDefinition::InsertBodies { bodies } => body_selections.push(bodies),
            FeatureDefinition::InsertComponent { occurrence } => {
                if !ir
                    .model
                    .occurrences
                    .iter()
                    .any(|candidate| candidate.id == *occurrence)
                {
                    ref_error(
                        findings,
                        feature.id.as_str(),
                        "inserted component occurrence",
                        occurrence.as_str(),
                    );
                }
            }
            FeatureDefinition::AssemblyJoint { joint } => {
                if !ir
                    .model
                    .assembly_joints
                    .iter()
                    .any(|candidate| candidate.id == *joint)
                {
                    ref_error(
                        findings,
                        feature.id.as_str(),
                        "assembly joint",
                        joint.as_str(),
                    );
                }
            }
            FeatureDefinition::Form { cages } => {
                check_ids(
                    findings,
                    feature.id.as_str(),
                    "Form control cage",
                    cages.iter().map(super::super::ids::SubdId::as_str),
                    |identity| ids.subds(identity).is_some(),
                );
            }
            FeatureDefinition::CosmeticThread { face, .. } => face_selections.push(face),
            FeatureDefinition::Extrude {
                direction, start, ..
            } => {
                if let crate::features::ExtrudeDirection::Explicit {
                    source: Some(crate::features::ExtrusionDirectionSource::Edge { reference }),
                    ..
                } = direction
                {
                    paths.push(reference);
                }
                if let ExtrudeStart::FromFace { face, .. } = start {
                    face_selections.push(face);
                }
            }
            FeatureDefinition::SheetMetalEdgeFlange { edges, height, .. } => {
                edge_selections.push(edges);
                if matches!(height, crate::features::SheetMetalFlangeHeight::ToObject {
                    target: crate::features::SheetMetalFlangeHeightTarget::Native(native), ..
                } if native.is_empty())
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal edge-flange height is invalid",
                    );
                }
            }
            FeatureDefinition::SheetMetalHem { edges, .. } => edge_selections.push(edges),
            FeatureDefinition::Revolve { construction, .. } => {
                paths.extend(construction.axis().and_then(|axis| axis.reference.as_ref()));
            }
            FeatureDefinition::Sweep {
                path,
                orientation,
                guide_rail,
                ..
            } => {
                paths.extend(path);
                if let Some(guide_rail) = guide_rail {
                    paths.push(&guide_rail.path);
                }
                if let Some(crate::features::SweepOrientation::Auxiliary { path, .. }) = orientation
                {
                    paths.push(path);
                }
                if let Some(crate::features::SweepOrientation::GuideSurface { faces }) = orientation
                {
                    face_selections.push(faces);
                }
            }
            FeatureDefinition::Loft {
                sections, guidance, ..
            } => {
                for section in sections {
                    match section {
                        crate::features::LoftSection::Profile(_) => {}
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Native(_),
                        ) => {}
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Point(_),
                        ) => {}
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Vertex(vertex),
                        ) => check_ids(
                            findings,
                            feature.id.as_str(),
                            "loft section vertex",
                            std::iter::once(vertex.as_str()),
                            |identity| ids.vertices(identity).is_some(),
                        ),
                    }
                }
                match guidance {
                    crate::features::LoftGuidance::Guides(guides) => paths.extend(guides),
                    crate::features::LoftGuidance::Centerline(centerline) => paths.push(centerline),
                }
            }
            FeatureDefinition::Rib { .. } => {}
            FeatureDefinition::Fillet { groups } => {
                edge_selections.extend(groups.iter().map(|group| &group.edges));
            }
            FeatureDefinition::Chamfer { groups, .. } => {
                edge_selections.extend(groups.iter().map(|group| &group.edges));
            }
            FeatureDefinition::Shell {
                bodies,
                removed_faces,
                ..
            } => {
                if let Some(bodies) = bodies {
                    body_selections.push(bodies);
                }
                face_selections.push(removed_faces);
            }
            FeatureDefinition::OffsetShape { source, .. } => body_selections.push(source),
            FeatureDefinition::Compound { members } => body_selections.push(members),
            FeatureDefinition::RefineShape { source }
            | FeatureDefinition::ReverseShape { source } => body_selections.push(source),
            FeatureDefinition::RuledBetweenCurves { first, second, .. } => {
                paths.push(first);
                paths.push(second);
            }
            FeatureDefinition::SectionShape { operands, .. } => {
                body_selections.push(operands.first());
                body_selections.push(operands.second());
            }
            FeatureDefinition::MirrorShape {
                source,
                plane_reference,
                ..
            } => {
                body_selections.push(source);
                face_selections.extend(plane_reference);
            }
            FeatureDefinition::Thicken { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::OffsetSurface { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::KnitSurface { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                match boundary {
                    crate::features::SurfaceBoundary::Edges(edges) => {
                        edge_selections.push(edges);
                    }
                    crate::features::SurfaceBoundary::Path(path) => paths.push(path),
                }
                face_selections.push(support_faces);
            }
            FeatureDefinition::TrimSurface { faces, tool, .. } => {
                face_selections.push(faces);
                paths.push(tool);
            }
            FeatureDefinition::ExtendSurface { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                ..
            } => {
                edge_selections.push(edges);
                face_selections.push(support_faces);
            }
            FeatureDefinition::Draft { faces, anchor, .. } => {
                face_selections.push(faces);
                match anchor {
                    crate::features::DraftAnchor::NeutralPlane { plane, .. } => {
                        face_selections.push(plane);
                    }
                    crate::features::DraftAnchor::PartingLine { tool, .. } => {
                        face_selections.push(tool);
                    }
                }
                if let Some(pull_plane) = anchor.pull().and_then(|pull| pull.plane.as_ref()) {
                    check_plane_feature_reference(
                        findings,
                        feature,
                        pull_plane,
                        &feature_records,
                        "draft pull plane",
                    );
                }
            }
            FeatureDefinition::BoundaryFill { tools, cells } => {
                body_selections.push(tools);
                body_selections.extend(cells);
            }
            FeatureDefinition::SplitBody { targets, tools } => {
                body_selections.push(targets);
                face_selections.push(tools);
            }
            FeatureDefinition::SplitFace { targets, tool } => {
                face_selections.push(targets);
                match tool {
                    SplitFaceTool::Path(path) => paths.push(path),
                    SplitFaceTool::Plane { plane } => check_plane_feature_reference(
                        findings,
                        feature,
                        plane,
                        &feature_records,
                        "split-face tool plane",
                    ),
                    SplitFaceTool::Planes { planes } => {
                        for plane in planes {
                            check_plane_feature_reference(
                                findings,
                                feature,
                                plane,
                                &feature_records,
                                "split-face tool plane",
                            );
                        }
                    }
                }
            }
            FeatureDefinition::DeleteFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::ReplaceFace { operands } => {
                face_selections.push(operands.targets());
                face_selections.push(operands.replacements());
            }
            FeatureDefinition::MoveFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::MoveBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureDefinition::Dome { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureDefinition::Flex { .. } => {}
            FeatureDefinition::Scale { bodies, center, .. } => {
                body_selections.push(bodies);
                let center_valid = center.as_ref().is_none_or(|center| match center {
                    ScaleCenter::Point(_) => true,
                    ScaleCenter::Native(reference) => !reference.is_empty(),
                    ScaleCenter::Centroid | ScaleCenter::ModelOrigin => true,
                });
                if !center_valid {
                    feature_geometry_error(findings, feature, "scale transform is invalid");
                }
            }
            FeatureDefinition::Combine { operands, .. } => {
                body_selections.push(operands.target());
                body_selections.push(operands.tools());
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                body_selections.push(targets);
                face_selections.push(tools);
            }
            FeatureDefinition::TrimBodies { operands, .. } => {
                body_selections.push(operands.targets());
                body_selections.push(operands.tools());
            }
            FeatureDefinition::DeleteBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureDefinition::Hole { face, .. } => face_selections.extend(face),
            FeatureDefinition::Pattern { seeds, pattern } => {
                collect_pattern_paths(pattern, &mut paths);
                for seed in seeds {
                    match seed {
                        PatternSeed::Feature(seed) => match features.get(seed.as_str()) {
                            None => ref_error(
                                findings,
                                feature.id.as_str(),
                                "seed feature",
                                seed.as_str(),
                            ),
                            Some(ordinal) if *ordinal >= feature.ordinal => {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "seed feature `{}` does not precede its pattern",
                                        seed.as_str()
                                    ),
                                    entity: Some(feature.id.as_str().to_owned()),
                                });
                            }
                            Some(_) if !feature.dependencies.contains(seed) => {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "pattern omits seed feature `{}` from its dependencies",
                                        seed.as_str()
                                    ),
                                    entity: Some(feature.id.as_str().to_owned()),
                                });
                            }
                            Some(_) => {}
                        },
                        PatternSeed::Faces(selection) => face_selections.push(selection),
                        PatternSeed::Bodies(selection) => body_selections.push(selection),
                        PatternSeed::Occurrences(occurrences) => {
                            for occurrence in occurrences {
                                if !ir
                                    .model
                                    .occurrences
                                    .iter()
                                    .any(|candidate| candidate.id == *occurrence)
                                {
                                    ref_error(
                                        findings,
                                        feature.id.as_str(),
                                        "seed occurrence",
                                        occurrence.as_str(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            FeatureDefinition::Sketch { sketch, .. } => {
                if let Some(sketch) = sketch.id() {
                    if !ir.model.sketches.iter().any(|value| value.id == *sketch) {
                        ref_error(
                            findings,
                            feature.id.as_str(),
                            "owned sketch",
                            sketch.as_str(),
                        );
                    }
                }
            }
            FeatureDefinition::SpatialSketch { sketch } => {
                if let Some(sketch) = sketch {
                    if !ir
                        .model
                        .spatial_sketches
                        .iter()
                        .any(|value| value.id == *sketch)
                    {
                        ref_error(
                            findings,
                            feature.id.as_str(),
                            "owned spatial sketch",
                            sketch.as_str(),
                        );
                    }
                }
            }
            FeatureDefinition::DatumCoordinateSystem { .. } => {}
            FeatureDefinition::EquationCurve { .. } => {}

            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                ..
            } => {
                paths.push(source);
                face_selections.push(target_faces);
            }
            FeatureDefinition::ProjectOnSurface {
                sources,
                support_face,
                ..
            } => {
                paths.push(sources);
                face_selections.push(support_face);
            }
            FeatureDefinition::CompositeCurve { segments, .. } => {
                paths.extend(segments);
            }
            FeatureDefinition::Helix { .. } => {}
            FeatureDefinition::HelixNativeAxis { .. } => {}
            FeatureDefinition::Coil { result, .. } => {
                use crate::features::CoilResult;
                if let CoilResult::Boolean { targets, .. } = result {
                    body_selections.push(targets);
                }
            }
            FeatureDefinition::HelicalSweep { .. } => {}

            FeatureDefinition::Binder {
                sources,
                construction,
            } => {
                for target in
                    sources
                        .iter()
                        .map(|source| &source.target)
                        .chain(match construction {
                            crate::features::BinderConstruction::SubShape { context, .. } => {
                                context.as_ref()
                            }
                            crate::features::BinderConstruction::Shape { .. } => None,
                        })
                {
                    if let crate::features::BinderTarget::Feature { feature: target } = target {
                        match features.get(target.as_str()) {
                            None => ref_error(
                                findings,
                                feature.id.as_str(),
                                "binder target feature",
                                target.as_str(),
                            ),
                            Some(ordinal) if *ordinal >= feature.ordinal => {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "binder target feature `{}` does not precede its binder",
                                        target.as_str()
                                    ),
                                    entity: Some(feature.id.as_str().to_owned()),
                                });
                            }
                            Some(_) => {}
                        }
                    }
                }
            }
            FeatureDefinition::Wrap { face, .. } => {
                face_selections.push(face);
            }
            FeatureDefinition::Sphere { .. } => {}
            FeatureDefinition::Torus { .. } => {}
            FeatureDefinition::PointGeometry { .. } => {}
            FeatureDefinition::LineSegment { .. } => {}

            FeatureDefinition::CircularArc { .. } => {}

            FeatureDefinition::EllipticArc { .. } => {}

            FeatureDefinition::Polyline { .. } => {}

            FeatureDefinition::RegularPolygonCurve { .. } => {}
            FeatureDefinition::FaceFromShapes { sources, .. } => {
                body_selections.push(sources);
            }
            FeatureDefinition::TreeNode { children, .. } => {
                for child in children {
                    if !ir
                        .model
                        .features
                        .iter()
                        .any(|candidate| candidate.id == *child)
                    {
                        ref_error(findings, feature.id.as_str(), "tree child", child.as_str());
                    }
                }
            }
            FeatureDefinition::DatumPlane { .. } => {}
            FeatureDefinition::DatumThreePointPlane { points, .. } => {
                for point in points.iter() {
                    vertex_selections.push((point, "three-point datum-plane"));
                }
            }
            FeatureDefinition::DatumAxis { .. } => {}
            FeatureDefinition::DatumPoint { construction, .. } => {
                let mut plane_references = Vec::new();
                if let Some(construction) = construction.as_deref() {
                    match construction {
                        crate::features::DatumPointConstruction::CircleCenter { edge }
                        | crate::features::DatumPointConstruction::DistanceOnEdge {
                            edge, ..
                        } => edge_selections.push(edge),
                        crate::features::DatumPointConstruction::TwoEdgeIntersection { edges } => {
                            edge_selections.extend(edges);
                        }
                        crate::features::DatumPointConstruction::ThreePlaneIntersection {
                            planes,
                        } => plane_references.extend(planes.iter()),
                        crate::features::DatumPointConstruction::Vertex { vertex } => {
                            vertex_selections.push((vertex, "datum-point"));
                        }
                        crate::features::DatumPointConstruction::SketchPoint { .. } => {}
                        crate::features::DatumPointConstruction::EdgePlaneIntersection {
                            edge,
                            plane,
                        } => {
                            edge_selections.push(edge);
                            plane_references.push(plane);
                        }
                    }
                }
                for plane in plane_references {
                    match plane {
                        DatumPlaneReference::Feature(reference) => {
                            match feature_records.get(reference.as_str()) {
                                None => ref_error(
                                    findings,
                                    feature.id.as_str(),
                                    "datum-point plane",
                                    reference.as_str(),
                                ),
                                Some(record)
                                    if !matches!(
                                        record.evaluation.definition(),
                                        FeatureDefinition::DatumPrincipalPlane { .. }
                                            | FeatureDefinition::DatumPlane { .. }
                                            | FeatureDefinition::Unresolved {
                                                family: UnresolvedFamily::DatumPlane
                                            }
                                            | FeatureDefinition::DatumOffsetPlane { .. }
                                    ) =>
                                {
                                    feature_geometry_error(
                                        findings,
                                        feature,
                                        "datum-point plane reference does not name a plane",
                                    );
                                }
                                Some(record) if record.ordinal >= feature.ordinal => {
                                    feature_geometry_error(
                                        findings,
                                        feature,
                                        "datum-point plane does not precede the point",
                                    );
                                }
                                Some(_) if !feature.dependencies.contains(reference) => {
                                    findings.push(Finding {
                                        check: Check::ReferentialIntegrity,
                                        severity: Severity::Error,
                                        message: format!(
                                            "datum point omits plane feature `{}` from its dependencies",
                                            reference.as_str()
                                        ),
                                        entity: Some(feature.id.as_str().to_owned()),
                                    });
                                }
                                Some(_) => {}
                            }
                        }
                        DatumPlaneReference::Face(face) => face_selections.push(face),
                        DatumPlaneReference::ResolvedPlane { .. } => {}
                    }
                }
            }
            FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::SketchBlockDefinition { .. }
            | FeatureDefinition::StoredGeometry
            | FeatureDefinition::Native { .. } => {}
            FeatureDefinition::SketchBlockInstance {
                block,
                placement: _,
            } => {
                if let Some(block) = block {
                    match features.get(block.as_str()) {
                        None => ref_error(
                            findings,
                            feature.id.as_str(),
                            "sketch block",
                            block.as_str(),
                        ),
                        Some(ordinal) if *ordinal >= feature.ordinal => feature_geometry_error(
                            findings,
                            feature,
                            "sketch block does not precede its instance",
                        ),
                        Some(_)
                            if !ir.model.features.iter().any(|candidate| {
                                candidate.id == *block
                                    && matches!(
                                        candidate.evaluation.definition(),
                                        FeatureDefinition::SketchBlockDefinition { .. }
                                    )
                            }) =>
                        {
                            feature_geometry_error(
                                findings,
                                feature,
                                "sketch block target is not a block definition",
                            );
                        }
                        Some(_) if !feature.dependencies.contains(block) => {
                            findings.push(Finding {
                                check: Check::ReferentialIntegrity,
                                severity: Severity::Error,
                                message: format!(
                                    "sketch block instance omits block feature `{}` from its dependencies",
                                    block.as_str()
                                ),
                                entity: Some(feature.id.as_str().to_owned()),
                            });
                        }
                        Some(_) => {}
                    }
                }
            }
            FeatureDefinition::DerivedGeometry { source } => match features.get(source.as_str()) {
                None => ref_error(
                    findings,
                    feature.id.as_str(),
                    "source feature",
                    source.as_str(),
                ),
                Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "source feature `{}` does not precede its derived geometry",
                        source.as_str()
                    ),
                    entity: Some(feature.id.as_str().to_owned()),
                }),
                Some(_) if !feature.dependencies.contains(source) => findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "derived geometry omits source feature `{}` from its dependencies",
                        source.as_str()
                    ),
                    entity: Some(feature.id.as_str().to_owned()),
                }),
                Some(_) => {}
            },
            FeatureDefinition::ImportedGeometry { .. } | FeatureDefinition::PostProcess { .. } => {}
            FeatureDefinition::DatumOffsetPlane { reference, .. } => {
                if let Some(reference) = reference {
                    match reference {
                        DatumPlaneReference::Feature(reference) => {
                            match feature_records.get(reference.as_str()) {
                                None => {
                                    ref_error(
                                        findings,
                                        feature.id.as_str(),
                                        "reference plane",
                                        reference.as_str(),
                                    );
                                }
                                Some(record)
                                    if !matches!(
                                        record.evaluation.definition(),
                                        FeatureDefinition::DatumPrincipalPlane { .. }
                                            | FeatureDefinition::DatumPlane { .. }
                                            | FeatureDefinition::Unresolved {
                                                family: UnresolvedFamily::DatumPlane
                                            }
                                            | FeatureDefinition::DatumOffsetPlane { .. }
                                    ) =>
                                {
                                    feature_geometry_error(
                                        findings,
                                        feature,
                                        "datum-plane feature reference does not name a plane",
                                    );
                                }
                                Some(record) if record.ordinal >= feature.ordinal => {
                                    findings.push(Finding {
                                        check: Check::ReferentialIntegrity,
                                        severity: Severity::Error,
                                        message: format!(
                                            "reference plane `{}` does not precede its offset plane",
                                            reference.as_str()
                                        ),
                                        entity: Some(feature.id.as_str().to_owned()),
                                    });
                                }
                                Some(_) if !feature.dependencies.contains(reference) => {
                                    findings.push(Finding {
                                        check: Check::ReferentialIntegrity,
                                        severity: Severity::Error,
                                        message: format!(
                                            "offset plane omits reference feature `{}` from its dependencies",
                                            reference.as_str()
                                        ),
                                        entity: Some(feature.id.as_str().to_owned()),
                                    });
                                }
                                Some(_) => {}
                            }
                        }
                        DatumPlaneReference::Face(face) => face_selections.push(face),
                        DatumPlaneReference::ResolvedPlane { .. } => {}
                    }
                }
            }
        }
        for profile in definition_profiles(definition) {
            match profile {
                ProfileRef::Faces(faces) => check_ids(
                    findings,
                    feature.id.as_str(),
                    "profile face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| ids.faces(identity).is_some(),
                ),
                ProfileRef::HistoricalFaces { state, faces, .. } => {
                    check_historical_members(
                        findings,
                        &feature.id,
                        (
                            state,
                            faces.iter().map(crate::ids::HistoricalFaceId::as_str),
                        ),
                        "profile face",
                        &input_topologies,
                        |topology| {
                            topology
                                .faces
                                .iter()
                                .map(crate::ids::HistoricalFaceId::as_str)
                                .collect()
                        },
                    );
                }
                ProfileRef::Feature(producer) => match features.get(producer.as_str()) {
                    None => ref_error(
                        findings,
                        feature.id.as_str(),
                        "profile feature",
                        producer.as_str(),
                    ),
                    Some(ordinal)
                        if *ordinal >= feature.ordinal
                            || !feature.dependencies.contains(producer) =>
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "profile feature is not a preceding dependency",
                        );
                    }
                    Some(_) => {}
                },
                ProfileRef::Generated { curves, .. }
                    if curves.iter().any(|curve| {
                        features
                            .get(curve.feature.as_str())
                            .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                            || !feature.dependencies.contains(&curve.feature)
                    }) =>
                {
                    feature_geometry_error(findings, feature, "generated profile curve is invalid");
                }
                _ => {}
            }
        }
        for path in paths {
            match path {
                PathRef::Edges(edges) => check_ids(
                    findings,
                    feature.id.as_str(),
                    "path edge",
                    edges.iter().map(super::super::ids::EdgeId::as_str),
                    |identity| ids.edges(identity).is_some(),
                ),
                PathRef::Curves(curves) => check_ids(
                    findings,
                    feature.id.as_str(),
                    "path curve",
                    curves.iter().map(super::super::ids::CurveId::as_str),
                    |identity| ids.curves(identity).is_some(),
                ),
                PathRef::SketchCurves { curves, .. } => check_ids(
                    findings,
                    feature.id.as_str(),
                    "sketch path curve",
                    curves.iter().map(crate::sketches::SketchEntityId::as_str),
                    |identity| sketch_entities.contains(identity),
                ),
                PathRef::SpatialSketchCurves { curves, .. } => check_ids(
                    findings,
                    feature.id.as_str(),
                    "spatial sketch path curve",
                    curves
                        .iter()
                        .map(crate::sketches::SpatialSketchEntityId::as_str),
                    |identity| spatial_sketch_entity_owners.contains_key(identity),
                ),
                PathRef::HistoricalEdges { state, edges, .. } => check_historical_members(
                    findings,
                    &feature.id,
                    (
                        state,
                        edges.iter().map(crate::ids::HistoricalEdgeId::as_str),
                    ),
                    "path edge",
                    &input_topologies,
                    |topology| {
                        topology
                            .edges
                            .iter()
                            .map(crate::ids::HistoricalEdgeId::as_str)
                            .collect()
                    },
                ),
                PathRef::Unresolved(_)
                | PathRef::Native(_)
                | PathRef::Sketch(_)
                | PathRef::SpatialSketchSelection { .. } => {}
            }
        }
        for termination in definition_terminations(definition) {
            if let Some(FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. }) =
                termination.face()
            {
                check_ids(
                    findings,
                    feature.id.as_str(),
                    "termination face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| ids.faces(identity).is_some(),
                );
            }
            if let Some(FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. }) =
                termination.shape()
            {
                check_ids(
                    findings,
                    feature.id.as_str(),
                    "termination shape face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| ids.faces(identity).is_some(),
                );
            }
            if let Some(vertex) = termination.vertex() {
                vertex_selections.push((vertex, "termination"));
            }
        }
        for (selection, consumer) in vertex_selections {
            match selection {
                crate::features::VertexSelection::Generated { vertex, .. } => {
                    if features
                        .get(vertex.feature.as_str())
                        .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                        || !feature.dependencies.contains(&vertex.feature)
                        || result_topologies_by_feature
                            .get(vertex.feature.as_str())
                            .is_some_and(|state| {
                                !state
                                    .vertices()
                                    .iter()
                                    .any(|id| id == vertex.local_id.as_str())
                            })
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            &format!("generated {consumer} vertex is invalid"),
                        );
                    }
                }
                crate::features::VertexSelection::Historical {
                    state,
                    vertex,
                    native: _,
                } => check_historical_members(
                    findings,
                    &feature.id,
                    (state, std::iter::once(vertex.as_str())),
                    "vertex",
                    &input_topologies,
                    |topology| {
                        topology
                            .vertices
                            .iter()
                            .map(crate::ids::HistoricalVertexId::as_str)
                            .collect()
                    },
                ),
                crate::features::VertexSelection::Unresolved
                | crate::features::VertexSelection::Native(_) => {}
            }
        }
        for selection in edge_selections {
            let historical = match selection {
                EdgeSelection::Historical { state, edges, .. } => Some((state, edges.as_slice())),
                EdgeSelection::HistoricalPartial { state, edges, .. } => {
                    Some((state, edges.as_slice()))
                }
                _ => None,
            };
            if let Some((state, selected)) = historical {
                check_historical_members(
                    findings,
                    &feature.id,
                    (
                        state,
                        selected.iter().map(crate::ids::HistoricalEdgeId::as_str),
                    ),
                    "edge",
                    &input_topologies,
                    |topology| {
                        topology
                            .edges
                            .iter()
                            .map(crate::ids::HistoricalEdgeId::as_str)
                            .collect()
                    },
                );
            }
            match selection {
                EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => check_ids(
                    findings,
                    feature.id.as_str(),
                    "selected edge",
                    edges.iter().map(super::super::ids::EdgeId::as_str),
                    |identity| ids.edges(identity).is_some(),
                ),
                EdgeSelection::Historical { .. } | EdgeSelection::HistoricalPartial { .. } => {}
                EdgeSelection::Generated { edges, .. } => {
                    if edges.iter().any(|edge| {
                        !feature.dependencies.contains(&edge.feature)
                            || result_topologies_by_feature
                                .get(edge.feature.as_str())
                                .is_some_and(|state| {
                                    !state.edges().iter().any(|id| id == edge.local_id.as_str())
                                })
                    }) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "generated edge selection is invalid",
                        );
                    }
                }
                EdgeSelection::All | EdgeSelection::Unresolved | EdgeSelection::Native(_) => {}
            }
        }
        for selection in face_selections {
            let historical = match selection {
                FaceSelection::Historical { state, faces, .. } => Some((state, faces.as_slice())),
                FaceSelection::HistoricalPartial { state, faces, .. } => {
                    Some((state, faces.as_slice()))
                }
                _ => None,
            };
            if let Some((state, selected)) = historical {
                check_historical_members(
                    findings,
                    &feature.id,
                    (
                        state,
                        selected.iter().map(crate::ids::HistoricalFaceId::as_str),
                    ),
                    "face",
                    &input_topologies,
                    |topology| {
                        topology
                            .faces
                            .iter()
                            .map(crate::ids::HistoricalFaceId::as_str)
                            .collect()
                    },
                );
            }
            match selection {
                FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => check_ids(
                    findings,
                    feature.id.as_str(),
                    "selected face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| ids.faces(identity).is_some(),
                ),
                FaceSelection::Historical { .. } | FaceSelection::HistoricalPartial { .. } => {}
                FaceSelection::Generated { faces, .. } => {
                    if faces.iter().any(|face| {
                        !feature.dependencies.contains(&face.feature)
                            || result_topologies_by_feature
                                .get(face.feature.as_str())
                                .is_some_and(|state| {
                                    !state.faces().iter().any(|id| id == face.local_id.as_str())
                                })
                    }) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "generated face selection is invalid",
                        );
                    }
                }
                FaceSelection::Unresolved | FaceSelection::Native(_) => {}
            }
        }
        for selection in body_selections {
            match selection {
                BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
                    check_ids(
                        findings,
                        feature.id.as_str(),
                        "selected body",
                        bodies.iter().map(super::super::ids::BodyId::as_str),
                        |identity| ids.bodies(identity).is_some(),
                    );
                }
                BodySelection::ResolvedSet { members } => {
                    check_ids(
                        findings,
                        feature.id.as_str(),
                        "selected body",
                        members.bodies().map(super::super::ids::BodyId::as_str),
                        |identity| ids.bodies(identity).is_some(),
                    );
                }
                BodySelection::Historical {
                    state,
                    bodies,
                    native: _,
                } => {
                    check_historical_members(
                        findings,
                        &feature.id,
                        (
                            state,
                            bodies.iter().map(crate::ids::HistoricalBodyId::as_str),
                        ),
                        "body",
                        &input_topologies,
                        |topology| {
                            topology
                                .bodies
                                .iter()
                                .map(crate::ids::HistoricalBodyId::as_str)
                                .collect()
                        },
                    );
                }
                BodySelection::HistoricalSet { state, members } => {
                    check_historical_members(
                        findings,
                        &feature.id,
                        (
                            state,
                            members.bodies().map(crate::ids::HistoricalBodyId::as_str),
                        ),
                        "body",
                        &input_topologies,
                        |topology| {
                            topology
                                .bodies
                                .iter()
                                .map(crate::ids::HistoricalBodyId::as_str)
                                .collect()
                        },
                    );
                }
                BodySelection::HistoricalUnorderedSet { state, selection } => {
                    check_historical_members(
                        findings,
                        &feature.id,
                        (
                            state,
                            selection
                                .bodies()
                                .iter()
                                .map(crate::ids::HistoricalBodyId::as_str),
                        ),
                        "body",
                        &input_topologies,
                        |topology| {
                            topology
                                .bodies
                                .iter()
                                .map(crate::ids::HistoricalBodyId::as_str)
                                .collect()
                        },
                    );
                }
                BodySelection::Generated { bodies, .. } => {
                    if bodies.iter().any(|body| {
                        !feature.dependencies.contains(&body.feature)
                            || result_topologies_by_feature
                                .get(body.feature.as_str())
                                .is_some_and(|state| {
                                    !state.bodies().iter().any(|id| id == body.local_id.as_str())
                                })
                    }) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "generated body selection is invalid",
                        );
                    }
                }
                BodySelection::Local { .. }
                | BodySelection::NativeSet(_)
                | BodySelection::Unresolved
                | BodySelection::Native(_) => {}
            }
        }
    }
}

fn check_historical_members<'a, I, F>(
    findings: &mut Vec<Finding>,
    feature: &crate::features::FeatureId,
    selection: (&crate::ids::FeatureInputTopologyId, I),
    kind: &str,
    states: &HashMap<&str, &crate::features::FeatureInputTopology>,
    members: F,
) where
    I: IntoIterator<Item = &'a str>,
    F: FnOnce(&crate::features::FeatureInputTopology) -> Vec<&str>,
{
    let (state_id, selected) = selection;
    let Some(state) = states.get(state_id.as_str()) else {
        ref_error(
            findings,
            feature.as_str(),
            "feature input topology",
            state_id.as_str(),
        );
        return;
    };
    if state.input_of != *feature {
        findings.push(Finding {
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: format!("historical {kind} selection uses another feature's input topology"),
            entity: Some(feature.as_str().to_owned()),
        });
    }
    let available = members(state).into_iter().collect::<HashSet<_>>();
    let selected = selected.into_iter().collect::<Vec<_>>();
    for id in selected {
        if !available.contains(id) {
            ref_error(
                findings,
                feature.as_str(),
                &format!("historical {kind}"),
                id,
            );
        }
    }
}

fn regeneration_references(
    definition: &crate::features::FeatureDefinition,
) -> impl Iterator<Item = &crate::features::FeatureId> {
    let mut references = BTreeSet::new();
    match definition {
        // A datum offset plane regenerates from its reference only when that
        // reference names a feature; a face-supported plane carries its frame
        // inline and is checked through the face selection instead.
        crate::features::FeatureDefinition::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature(reference)),
            ..
        } => {
            references.insert(reference);
        }
        crate::features::FeatureDefinition::DatumPoint {
            construction: Some(construction),
            ..
        } => references.extend(construction.feature_references()),
        crate::features::FeatureDefinition::DatumThreePointPlane { points, .. } => {
            references.extend(points.iter().filter_map(|point| match point {
                crate::features::VertexSelection::Generated { vertex, .. } => Some(&vertex.feature),
                crate::features::VertexSelection::Historical { .. }
                | crate::features::VertexSelection::Native(_)
                | crate::features::VertexSelection::Unresolved => None,
            }));
        }
        crate::features::FeatureDefinition::DerivedGeometry { source: reference }
        | crate::features::FeatureDefinition::SketchBlockInstance {
            block: Some(reference),
            ..
        } => {
            references.insert(reference);
        }
        crate::features::FeatureDefinition::Pattern { seeds, .. } => {
            references.extend(seeds.iter().filter_map(|seed| match seed {
                crate::features::PatternSeed::Feature(feature) => Some(feature),
                crate::features::PatternSeed::Faces(_)
                | crate::features::PatternSeed::Bodies(_)
                | crate::features::PatternSeed::Occurrences(_) => None,
            }));
        }
        _ => {}
    }
    references.extend(
        definition_terminations(definition).filter_map(|termination| match termination.vertex() {
            Some(crate::features::VertexSelection::Generated { vertex, .. }) => {
                Some(&vertex.feature)
            }
            _ => None,
        }),
    );
    for profile in definition_profiles(definition) {
        match profile {
            crate::features::ProfileRef::Feature(feature) => {
                references.insert(feature);
            }
            crate::features::ProfileRef::Generated { curves, .. } => {
                references.extend(curves.iter().map(|curve| &curve.feature));
            }
            _ => {}
        }
    }
    references.into_iter()
}

fn definition_profiles(
    definition: &crate::features::FeatureDefinition,
) -> impl Iterator<Item = &crate::features::ProfileRef> {
    let mut profiles: Vec<&crate::features::ProfileRef> = Vec::new();
    match definition {
        crate::features::FeatureDefinition::Extrude { profile, .. } => profiles.push(profile),
        crate::features::FeatureDefinition::SheetMetalBaseFlange { profile, .. }
        | crate::features::FeatureDefinition::Wrap { profile, .. } => profiles.push(profile),
        crate::features::FeatureDefinition::Revolve { construction, .. } => {
            profiles.extend(construction.profile().map(|profile| &**profile));
        }
        crate::features::FeatureDefinition::Rib { construction, .. } => {
            profiles.extend(construction.profile.as_deref());
        }
        crate::features::FeatureDefinition::Sweep { shape, .. } => {
            profiles.extend(shape.section().referenced_profile());
            profiles.extend(
                shape
                    .sections()
                    .iter()
                    .filter_map(crate::features::SweepSection::referenced_profile),
            );
        }
        crate::features::FeatureDefinition::HelicalSweep { construction, .. } => {
            profiles.push(&construction.profile);
        }
        crate::features::FeatureDefinition::Loft { sections, .. } => {
            profiles.extend(sections.iter().filter_map(|section| match section {
                crate::features::LoftSection::Profile(profile) => Some(profile),
                crate::features::LoftSection::Point(_) => None,
            }));
        }
        crate::features::FeatureDefinition::Hole {
            profile: Some(profile),
            ..
        } => profiles.push(profile),
        _ => {}
    }
    profiles.into_iter()
}

#[derive(Clone, Copy)]
enum TerminationRef<'a> {
    Linear(&'a crate::features::LinearTermination),
    Angular(&'a crate::features::AngularTermination),
}

impl<'a> TerminationRef<'a> {
    fn face(self) -> Option<&'a crate::features::FaceSelection> {
        match self {
            Self::Linear(crate::features::LinearTermination::ToFace { face, .. })
            | Self::Angular(crate::features::AngularTermination::ToFace { face, .. }) => Some(face),
            _ => None,
        }
    }

    fn shape(self) -> Option<&'a crate::features::FaceSelection> {
        match self {
            Self::Linear(crate::features::LinearTermination::ToShape { target })
            | Self::Angular(crate::features::AngularTermination::ToShape { target }) => {
                Some(target)
            }
            _ => None,
        }
    }

    fn vertex(self) -> Option<&'a crate::features::VertexSelection> {
        match self {
            Self::Linear(crate::features::LinearTermination::ToVertex { vertex })
            | Self::Angular(crate::features::AngularTermination::ToVertex { vertex }) => {
                Some(vertex)
            }
            _ => None,
        }
    }
}

fn definition_terminations(
    definition: &crate::features::FeatureDefinition,
) -> impl Iterator<Item = TerminationRef<'_>> {
    let mut terminations = Vec::new();
    match definition {
        crate::features::FeatureDefinition::Extrude { extent, .. } => match extent {
            crate::features::ExtrudeExtent::OneSided { side }
            | crate::features::ExtrudeExtent::Symmetric { side } => {
                terminations.push(TerminationRef::Linear(&side.termination));
            }
            crate::features::ExtrudeExtent::TwoSided { first, second } => {
                terminations.extend([
                    TerminationRef::Linear(&first.termination),
                    TerminationRef::Linear(&second.termination),
                ]);
            }
        },
        crate::features::FeatureDefinition::Revolve { construction, .. } => {
            match construction.extent() {
                Some(
                    crate::features::RevolveExtent::OneSided { termination }
                    | crate::features::RevolveExtent::Symmetric { termination },
                ) => terminations.push(TerminationRef::Angular(termination)),
                Some(crate::features::RevolveExtent::TwoSided { first, second }) => {
                    terminations.extend([
                        TerminationRef::Angular(first),
                        TerminationRef::Angular(second),
                    ]);
                }
                None => {}
            }
        }
        crate::features::FeatureDefinition::Hole {
            extent: Some(extent),
            ..
        } => terminations.push(TerminationRef::Linear(extent)),
        _ => {}
    }
    terminations.into_iter()
}

fn check_configuration_state_closure(
    configuration: &crate::features::DesignConfiguration,
    findings: &mut Vec<Finding>,
) {
    if configuration.feature_states.is_empty() {
        return;
    }
    let mut closure = configuration
        .feature_states
        .iter()
        .filter(|(_, state)| !state.evaluation.is_suppressed())
        .map(|(feature, _)| feature.clone())
        .collect::<HashSet<_>>();
    let mut pending = closure.iter().cloned().collect::<Vec<_>>();
    while let Some(feature) = pending.pop() {
        let state = &configuration.feature_states[&feature];
        for dependency in &state.dependencies {
            match configuration.feature_states.get(dependency) {
                None => {}
                Some(dependency_state) if dependency_state.evaluation.is_suppressed() => {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "configuration state closure uses suppressed dependency state `{}`",
                            dependency.as_str()
                        ),
                        entity: Some(configuration.id.as_str().to_owned()),
                    });
                }
                Some(_) if closure.insert(dependency.clone()) => pending.push(dependency.clone()),
                Some(_) => {}
            }
        }
    }
}

fn feature_geometry_error(findings: &mut Vec<Finding>, feature: &Feature, message: &str) {
    geometry_error(findings, feature.id.as_str(), message);
}

fn check_plane_feature_reference(
    findings: &mut Vec<Finding>,
    feature: &Feature,
    reference: &crate::features::FeatureId,
    feature_records: &HashMap<&str, &Feature>,
    reference_kind: &str,
) {
    match feature_records.get(reference.as_str()) {
        None => ref_error(
            findings,
            feature.id.as_str(),
            reference_kind,
            reference.as_str(),
        ),
        Some(record)
            if !matches!(
                record.evaluation.definition(),
                crate::features::FeatureDefinition::DatumPrincipalPlane { .. }
                    | crate::features::FeatureDefinition::DatumPlane { .. }
                    | crate::features::FeatureDefinition::Unresolved {
                        family: crate::features::UnresolvedFamily::DatumPlane
                    }
                    | crate::features::FeatureDefinition::DatumOffsetPlane { .. }
            ) =>
        {
            feature_geometry_error(
                findings,
                feature,
                "feature reference does not name a datum plane",
            );
        }
        Some(record) if record.ordinal >= feature.ordinal => findings.push(Finding {
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: format!(
                "{reference_kind} `{}` does not precede its consuming feature",
                reference.as_str()
            ),
            entity: Some(feature.id.as_str().to_owned()),
        }),
        Some(_) if !feature.dependencies.contains(reference) => findings.push(Finding {
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: format!(
                "feature omits {reference_kind} dependency `{}`",
                reference.as_str()
            ),
            entity: Some(feature.id.as_str().to_owned()),
        }),
        Some(_) => {}
    }
}

fn geometry_error(findings: &mut Vec<Finding>, entity: &str, message: &str) {
    findings.push(Finding {
        check: Check::GeometricConsistency,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(entity.into()),
    });
}

fn check_ids<'a>(
    findings: &mut Vec<Finding>,
    owner: &str,
    kind: &str,
    values: impl Iterator<Item = &'a str>,
    valid: impl Fn(&str) -> bool,
) {
    for value in values {
        if !valid(value) {
            ref_error(findings, owner, kind, value);
        }
    }
}

fn check_feature_sketch_references(
    ir: &CadIr,
    sketches: &HashSet<&str>,
    findings: &mut Vec<Finding>,
) {
    use crate::features::{FeatureDefinition, PathRef, ProfileRef, SketchPointSelection};

    let spatial_sketches = ir
        .model
        .spatial_sketches
        .iter()
        .map(|sketch| sketch.id.as_str())
        .collect::<HashSet<_>>();
    let sketch_entity_owners = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (entity.id().as_str(), entity.sketch.as_str()))
        .collect::<HashMap<_, _>>();
    let spatial_sketch_entity_owners = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (entity.id().as_str(), entity.sketch.as_str()))
        .collect::<HashMap<_, _>>();
    let mut owners = HashMap::new();
    for feature in &ir.model.features {
        let sketch = match feature.evaluation.definition() {
            FeatureDefinition::Sketch {
                sketch: crate::features::SketchFeatureBinding::Planar(Some(sketch)),
                ..
            } => sketch.as_str(),
            FeatureDefinition::SpatialSketch {
                sketch: Some(sketch),
            } => sketch.as_str(),
            _ => continue,
        };
        if owners
            .insert(sketch, (feature.id.as_str(), feature.ordinal))
            .is_some()
        {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: format!("sketch `{sketch}` has multiple owning features"),
                entity: Some(feature.id.as_str().to_owned()),
            });
        }
    }

    for feature in &ir.model.features {
        let FeatureDefinition::DatumPoint {
            construction: Some(construction),
            ..
        } = feature.evaluation.definition()
        else {
            continue;
        };
        let crate::features::DatumPointConstruction::SketchPoint { point } = construction.as_ref()
        else {
            continue;
        };
        let valid = match point {
            SketchPointSelection::Planar {
                sketch,
                point,
                native,
            } => {
                !native.trim().is_empty()
                    && sketches.contains(sketch.as_str())
                    && sketch_entity_owners
                        .get(point.as_str())
                        .is_some_and(|owner| *owner == sketch.as_str())
                    && ir.model.sketch_entities.iter().any(|entity| {
                        entity.id() == point
                            && entity.sketch == *sketch
                            && matches!(
                                entity.geometry.definition(),
                                crate::sketches::SketchGeometryDefinition::Point { .. }
                            )
                    })
            }
            SketchPointSelection::Spatial {
                sketch,
                point,
                native,
            } => {
                !native.trim().is_empty()
                    && spatial_sketches.contains(sketch.as_str())
                    && spatial_sketch_entity_owners
                        .get(point.as_str())
                        .is_some_and(|owner| *owner == sketch.as_str())
                    && ir.model.spatial_sketch_entities.iter().any(|entity| {
                        entity.id() == point
                            && entity.sketch == *sketch
                            && matches!(
                                entity.geometry.definition(),
                                crate::sketches::SpatialSketchGeometryDefinition::Point { .. }
                            )
                    })
            }
            SketchPointSelection::Native(native) => !native.trim().is_empty(),
            SketchPointSelection::Unresolved => true,
        };
        if !valid {
            feature_geometry_error(
                findings,
                feature,
                "datum-point sketch-point selection is invalid",
            );
        }
        let sketch_id = match point {
            SketchPointSelection::Planar { sketch, .. } => Some(sketch.as_str()),
            SketchPointSelection::Spatial { sketch, .. } => Some(sketch.as_str()),
            SketchPointSelection::Native(_) | SketchPointSelection::Unresolved => None,
        };
        if let Some(sketch_id) = sketch_id {
            if let Some((owner, ordinal)) = owners.get(sketch_id) {
                if *ordinal >= feature.ordinal {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "sketch owner `{owner}` does not precede its datum-point consumer"
                        ),
                        entity: Some(feature.id.as_str().to_owned()),
                    });
                }
            }
        }
    }

    for feature in &ir.model.features {
        let mut profiles: Vec<&crate::features::ProfileRef> = Vec::new();
        let mut paths = Vec::new();
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. } => operation.as_ref(),
            definition => definition,
        };
        match definition {
            FeatureDefinition::Extrude { profile, .. } => {
                profiles.push(profile);
            }
            FeatureDefinition::SheetMetalBaseFlange { profile, .. } => {
                profiles.push(profile);
            }
            FeatureDefinition::Rib { construction, .. } => {
                profiles.extend(construction.profile.as_deref());
            }
            FeatureDefinition::Revolve { construction, .. } => {
                profiles.extend(construction.profile().map(|profile| &**profile));
                paths.extend(construction.axis().and_then(|axis| axis.reference.as_ref()));
            }
            FeatureDefinition::Sweep {
                shape,
                path,
                guide_rail,
                ..
            } => {
                profiles.extend(shape.section().referenced_profile());
                profiles.extend(
                    shape
                        .sections()
                        .iter()
                        .filter_map(crate::features::SweepSection::referenced_profile),
                );
                paths.extend(path);
                if let Some(guide_rail) = guide_rail {
                    paths.push(&guide_rail.path);
                }
            }
            FeatureDefinition::HelicalSweep { construction, .. } => {
                profiles.push(&construction.profile);
            }
            FeatureDefinition::Loft {
                sections, guidance, ..
            } => {
                profiles.extend(sections.iter().filter_map(|section| match section {
                    crate::features::LoftSection::Profile(profile) => Some(profile),
                    crate::features::LoftSection::Point(_) => None,
                }));
                match guidance {
                    crate::features::LoftGuidance::Guides(guides) => paths.extend(guides),
                    crate::features::LoftGuidance::Centerline(centerline) => paths.push(centerline),
                }
            }
            FeatureDefinition::Pattern { pattern, .. } => {
                collect_pattern_paths(pattern, &mut paths);
            }
            _ => {}
        }
        for profile in profiles {
            let (sketch, sketch_kind, defined_sketches) = match profile {
                ProfileRef::SpatialSketchProfiles { sketch, .. }
                | ProfileRef::SpatialSketchSelection { sketch, .. } => {
                    (sketch.as_str(), "spatial sketch", &spatial_sketches)
                }
                ProfileRef::Sketch(sketch)
                | ProfileRef::SketchProfiles { sketch, .. }
                | ProfileRef::SketchRegions { sketch, .. }
                | ProfileRef::SketchEntities { sketch, .. }
                | ProfileRef::SketchSelection { sketch, .. } => {
                    (sketch.as_str(), "sketch", sketches)
                }
                _ => continue,
            };
            if !defined_sketches.contains(sketch) {
                ref_error(
                    findings,
                    feature.id.as_str(),
                    &format!("{sketch_kind} profile"),
                    sketch,
                );
            } else if let Some((owner, ordinal)) = owners.get(sketch) {
                if *ordinal >= feature.ordinal {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "{sketch_kind} owner `{owner}` does not precede its profile consumer"
                        ),
                        entity: Some(feature.id.as_str().to_owned()),
                    });
                }
            }
            match profile {
                ProfileRef::SpatialSketchProfiles { sketch, profiles } => {
                    let profile_count = ir
                        .model
                        .spatial_sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .map_or(0, |sketch| sketch.profiles.len());
                    if profiles
                        .iter()
                        .any(|index| *index as usize >= profile_count)
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "spatial sketch profile indices are empty, repeated, or out of range",
                        );
                    }
                }
                ProfileRef::SketchProfiles { sketch, profiles } => {
                    let sketch_profile_count = ir
                        .model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .map_or(0, |sketch| sketch.profiles.len());
                    if profiles
                        .iter()
                        .any(|index| *index as usize >= sketch_profile_count)
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "sketch profile indices are empty, repeated, or out of range",
                        );
                    }
                }
                ProfileRef::SketchRegions { sketch, regions } => {
                    let selected_sketch = ir
                        .model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch);
                    let sketch_profile_count =
                        selected_sketch.map_or(0, |sketch| sketch.profiles.len());
                    let invalid = regions.iter().any(|region| match region {
                        crate::features::SketchProfileRegion::Loops(loops) => {
                            loops.outer() as usize >= sketch_profile_count
                                || loops
                                    .holes()
                                    .iter()
                                    .any(|index| *index as usize >= sketch_profile_count)
                        }
                        crate::features::SketchProfileRegion::Trimmed {
                            outer_boundary,
                            hole_boundaries,
                        } => {
                            let valid_ring =
                                |ring: &[crate::features::SketchProfileBoundaryUse]| {
                                    ring.iter().all(|use_| {
                                        ir.model.sketch_entities.iter().any(|entity| {
                                            entity.id() == &use_.entity && entity.sketch == *sketch
                                        })
                                    })
                                };
                            !valid_ring(outer_boundary)
                                || hole_boundaries.iter().any(|ring| !valid_ring(ring))
                        }
                    });
                    if invalid {
                        feature_geometry_error(
                            findings,
                            feature,
                            "sketch regions have empty, repeated, invalid, or out-of-range boundaries",
                        );
                    }
                }
                ProfileRef::SketchEntities { sketch, entities } => {
                    if entities.iter().any(|entity| {
                        sketch_entity_owners
                            .get(entity.as_str())
                            .is_none_or(|owner| *owner != sketch.as_str())
                    }) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "sketch profile entities are empty, repeated, missing, or owned by another sketch",
                        );
                    }
                }
                ProfileRef::Native(_)
                | ProfileRef::Unresolved(_)
                | ProfileRef::Feature(_)
                | ProfileRef::Generated { .. }
                | ProfileRef::Sketch(_)
                | ProfileRef::SketchSelection { .. }
                | ProfileRef::SpatialSketchSelection { .. }
                | ProfileRef::HistoricalFaces { .. }
                | ProfileRef::Faces(_) => {}
            }
        }
        for path in paths {
            if let PathRef::SketchCurves { sketch, curves } = path {
                let invalid = curves.iter().any(|curve| {
                    sketch_entity_owners
                        .get(curve.as_str())
                        .is_none_or(|owner| *owner != sketch.as_str())
                });
                if invalid {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sketch path curves are empty, repeated, or owned by another sketch",
                    );
                }
            }
            let (sketch, known_sketches, description) = match path {
                PathRef::Sketch(sketch) => (sketch.as_str(), sketches, "sketch path"),
                PathRef::SketchCurves { sketch, .. } => {
                    (sketch.as_str(), sketches, "sketch curve path")
                }
                PathRef::SpatialSketchCurves { sketch, curves } => {
                    let invalid = curves.iter().any(|curve| {
                        spatial_sketch_entity_owners
                            .get(curve.as_str())
                            .is_none_or(|owner| *owner != sketch.as_str())
                    });
                    if invalid {
                        feature_geometry_error(
                            findings,
                            feature,
                            "spatial sketch path curves are empty, repeated, missing, or owned by another sketch",
                        );
                    }
                    (
                        sketch.as_str(),
                        &spatial_sketches,
                        "spatial sketch curve path",
                    )
                }
                PathRef::SpatialSketchSelection { sketch, .. } => {
                    (sketch.as_str(), &spatial_sketches, "spatial sketch path")
                }
                _ => continue,
            };
            if !known_sketches.contains(sketch) {
                ref_error(findings, feature.id.as_str(), description, sketch);
            } else if let Some((owner, ordinal)) = owners.get(sketch) {
                if *ordinal >= feature.ordinal {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "sketch owner `{owner}` does not precede its path consumer"
                        ),
                        entity: Some(feature.id.as_str().to_owned()),
                    });
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

#[derive(Clone, Copy)]
enum RadialStatus {
    Closed(usize),
    DoesNotClose,
    CrossesEdge,
}

pub(super) fn check_coedge_pairing(ir: &CadIr, findings: &mut Vec<Finding>) {
    let by_id: HashMap<&str, &Coedge> = ir
        .model
        .coedges
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();
    let mut statuses = HashMap::<&str, RadialStatus>::new();
    for coedge in &ir.model.coedges {
        let start = coedge.id.as_str();
        if statuses.contains_key(start) {
            continue;
        }
        let expected_edge = &coedge.edge;
        let mut path = Vec::<&str>::new();
        let mut positions = HashMap::<&str, usize>::new();
        let mut current = start;
        loop {
            if let Some(status) = statuses.get(current).copied() {
                let status = match status {
                    RadialStatus::Closed(_) => RadialStatus::DoesNotClose,
                    status => status,
                };
                for member in path {
                    statuses.insert(member, status);
                }
                break;
            }
            if let Some(&cycle_start) = positions.get(current) {
                let cycle_len = path.len() - cycle_start;
                for &member in &path[cycle_start..] {
                    statuses.insert(member, RadialStatus::Closed(cycle_len));
                }
                for &member in &path[..cycle_start] {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            }
            positions.insert(current, path.len());
            path.push(current);
            let Some(current_coedge) = by_id.get(current) else {
                for member in path {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            };
            let Some(next) = by_id.get(current_coedge.radial_next.as_str()) else {
                for member in path {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            };
            if next.edge != *expected_edge {
                for member in path {
                    statuses.insert(member, RadialStatus::CrossesEdge);
                }
                break;
            }
            current = next.id.as_str();
        }
    }
    for coedge in &ir.model.coedges {
        match statuses[coedge.id.as_str()] {
            RadialStatus::CrossesEdge => {
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring crosses edges".into(),
                    entity: Some(coedge.id.as_str().to_owned()),
                });
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring does not close".into(),
                    entity: Some(coedge.id.as_str().to_owned()),
                });
            }
            RadialStatus::DoesNotClose => {
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring does not close".into(),
                    entity: Some(coedge.id.as_str().to_owned()),
                });
            }
            RadialStatus::Closed(2) => {
                if let Some(other) = by_id.get(coedge.radial_next.as_str()) {
                    if other.sense == coedge.sense {
                        findings.push(Finding {
                            check: Check::CoedgePairing,
                            severity: Severity::Warning,
                            message: "two-member radial ring has equal coedge senses".into(),
                            entity: Some(coedge.id.as_str().to_owned()),
                        });
                    }
                }
            }
            RadialStatus::Closed(_) => {}
        }
    }
}

pub(super) fn check_wire_topology(ir: &CadIr, findings: &mut Vec<Finding>) {
    let coedge_edges = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str())
        .collect::<HashSet<_>>();
    let edge_vertices = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.as_str(), edge.end.as_str()])
        .collect::<HashSet<_>>();
    let loop_vertices = ir
        .model
        .loops
        .iter()
        .flat_map(crate::topology::Loop::vertices)
        .map(super::super::ids::VertexId::as_str)
        .collect::<HashSet<_>>();
    let mut wire_owners = HashMap::<&str, usize>::new();
    let mut free_owners = HashMap::<&str, usize>::new();

    for shell in &ir.model.shells {
        for edge in shell.wire_edges() {
            *wire_owners.entry(edge.as_str()).or_default() += 1;
            if coedge_edges.contains(edge.as_str()) {
                wire_error(
                    findings,
                    shell.id.as_str(),
                    "wire edge is also referenced by a coedge",
                );
            }
        }
        for vertex in shell.free_vertices() {
            *free_owners.entry(vertex.as_str()).or_default() += 1;
            if edge_vertices.contains(vertex.as_str()) {
                wire_error(
                    findings,
                    shell.id.as_str(),
                    "free vertex is also referenced by an edge",
                );
            }
        }
    }
    for edge in &ir.model.edges {
        if !coedge_edges.contains(edge.id.as_str())
            && wire_owners.get(edge.id.as_str()).copied().unwrap_or(0) != 1
        {
            wire_error(
                findings,
                edge.id.as_str(),
                "wire edge must belong to exactly one shell",
            );
        }
    }
    for vertex in &ir.model.vertices {
        let owner_count = free_owners.get(vertex.id.as_str()).copied().unwrap_or(0);
        if owner_count > 1
            || (!edge_vertices.contains(vertex.id.as_str())
                && !loop_vertices.contains(vertex.id.as_str())
                && owner_count != 1)
        {
            wire_error(
                findings,
                vertex.id.as_str(),
                "free vertex must belong to exactly one shell",
            );
        }
    }

    let regions = ir
        .model
        .regions
        .iter()
        .map(|region| (region.id.as_str(), region))
        .collect::<HashMap<_, _>>();
    let shells = ir
        .model
        .shells
        .iter()
        .map(|shell| (shell.id.as_str(), shell))
        .collect::<HashMap<_, _>>();
    for body in &ir.model.bodies {
        if body.kind == crate::topology::BodyKind::Wire
            && body.regions.iter().any(|region_id| {
                regions.get(region_id.as_str()).is_some_and(|region| {
                    region.shells.iter().any(|shell_id| {
                        shells
                            .get(shell_id.as_str())
                            .is_some_and(|shell| !shell.faces().is_empty())
                    })
                })
            })
        {
            wire_error(findings, body.id.as_str(), "wire body contains faces");
        }
    }
}

pub(super) fn check_shell_connectivity(ir: &CadIr, findings: &mut Vec<Finding>) {
    let faces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<HashMap<_, _>>();
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.as_str(), loop_.face.as_str()))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let mut faces_by_edge = HashMap::<&str, HashSet<&str>>::new();
    let mut faces_by_vertex = HashMap::<&str, HashSet<&str>>::new();
    for coedge in &ir.model.coedges {
        let Some(face) = loop_faces.get(coedge.owner_loop.as_str()) else {
            continue;
        };
        faces_by_edge
            .entry(coedge.edge.as_str())
            .or_default()
            .insert(*face);
        if let Some(edge) = edges.get(coedge.edge.as_str()) {
            faces_by_vertex
                .entry(edge.start.as_str())
                .or_default()
                .insert(*face);
            faces_by_vertex
                .entry(edge.end.as_str())
                .or_default()
                .insert(*face);
        }
    }
    for loop_ in &ir.model.loops {
        let Some(face) = loop_faces.get(loop_.id.as_str()) else {
            continue;
        };
        match &loop_.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, .. } => {
                faces_by_vertex
                    .entry(vertex.as_str())
                    .or_default()
                    .insert(*face);
            }
            crate::topology::LoopBoundary::Ring(ring) => {
                for vertex_use in ring.vertex_uses() {
                    faces_by_vertex
                        .entry(vertex_use.vertex.as_str())
                        .or_default()
                        .insert(*face);
                }
            }
        }
    }
    let mut neighbors = HashMap::<&str, HashSet<&str>>::new();
    for incident_faces in faces_by_edge.values().chain(faces_by_vertex.values()) {
        for &face in incident_faces {
            neighbors.entry(face).or_default().extend(
                incident_faces
                    .iter()
                    .copied()
                    .filter(|other| *other != face),
            );
        }
    }

    for shell in &ir.model.shells {
        if shell.faces().len() < 2
            || shell.faces().iter().any(|face| {
                faces
                    .get(face.as_str())
                    .is_none_or(|face| face.loops.is_empty())
            })
        {
            continue;
        }
        let owned = shell
            .faces()
            .iter()
            .map(super::super::ids::FaceId::as_str)
            .collect::<HashSet<_>>();
        let mut reached = HashSet::from([shell.faces()[0].as_str()]);
        let mut pending = vec![shell.faces()[0].as_str()];
        while let Some(face) = pending.pop() {
            for &neighbor in neighbors.get(face).into_iter().flatten() {
                if owned.contains(neighbor) && reached.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
        if reached.len() != owned.len() {
            findings.push(Finding {
                check: Check::ShellTopology,
                severity: Severity::Error,
                message: "shell faces are disconnected through shared edges or vertices".into(),
                entity: Some(shell.id.as_str().to_owned()),
            });
        }
    }
}

fn check_composite_cycle<'a>(
    curve: &'a str,
    segments: &BTreeMap<&'a str, Vec<&'a str>>,
    active: &mut HashSet<&'a str>,
    complete: &mut HashSet<&'a str>,
    findings: &mut Vec<Finding>,
) {
    if complete.contains(curve) {
        return;
    }
    active.insert(curve);
    let mut stack = vec![(curve, 0usize)];
    while let Some((node, child_index)) = stack.last_mut() {
        let children = &segments[*node];
        if *child_index >= children.len() {
            let (node, _) = stack.pop().expect("nonempty composite traversal stack");
            active.remove(node);
            complete.insert(node);
            continue;
        }
        let child = children[*child_index];
        *child_index += 1;
        if !segments.contains_key(child) || complete.contains(child) {
            continue;
        }
        if !active.insert(child) {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: "composite curve graph contains a cycle".into(),
                entity: Some(child.into()),
            });
            continue;
        }
        stack.push((child, 0));
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

#[cfg(test)]
mod tests;
