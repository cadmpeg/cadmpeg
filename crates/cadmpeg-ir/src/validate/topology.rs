// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::features::{
    patterns::{PatternKind, PatternSeed, PatternTransform},
    BodySelection, DatumPlaneReference, ExtrudeStart, FaceSelection, Feature, FeatureSourceContent,
    SplitFaceTool, UnresolvedFamily,
};
use crate::geometry::{
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry,
};
use crate::report::{
    check::{Check, Finding},
    Severity,
};
pub(super) mod graphs;
mod composite;

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
                collect_stage_pattern_paths(&stage.pattern, paths);
            }
        }
        _ => {}
    }
}

/// A composite stage applies one transform, so its paths do not recurse.
fn collect_stage_pattern_paths<'a>(
    pattern: &'a crate::features::patterns::StagePatternKind,
    paths: &mut Vec<&'a crate::features::PathRef>,
) {
    if let PatternTransform::CurveDriven {
        path: Some(path), ..
    } = pattern.definition()
    {
        paths.push(path);
    }
}
use super::sketches::locus_entity;
use crate::index::ModelIndex;
use crate::sketches::SketchConstraintDefinitionInput as Definition;

fn ref_error(findings: &mut Vec<Finding>, owner: &str, target_kind: &str, target: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: format!("references missing {target_kind} `{target}`"),
        entity: Some(owner.to_string()),
    });
}

fn check_law_curves<R, V, P>(ctx: &DecodeContext<'_>, 
    expression: &crate::geometry::LawExpression<R, V, P>,
    ids: &ModelIndex<'_>,
    procedural: &crate::geometry::ProceduralSurface,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("law reference depth")?;
    match expression {
        crate::geometry::LawExpression::Edge { curve, .. } => {
            if ids.curves(curve.id.as_str(), ctx)?.is_none() {
                ref_error(findings, procedural.id.as_str(), "curve", curve.id.as_str());
            }
        }
        crate::geometry::LawExpression::Algebraic { operands, .. } => {
            for operand in operands {
                check_law_curves(ctx, operand, ids, procedural, findings)?;
            }
        }
        _ => {}
    }

    Ok(())
}

pub(super) fn check_tolerances(ctx: &DecodeContext<'_>, ir: &CadIr, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    ctx.charge_work(1, "document tolerance check")?;
    if ir.tolerances.linear.get() > 1.0e6 || ir.tolerances.angular.get() > std::f64::consts::TAU {
        super::record_finding(ctx, findings, Check::Tolerances, Severity::Warning, None,
            format_args!("document tolerance is outside a sane canonical range"))?;
    }
    Ok(())
}

pub(super) fn check_topology_tolerances(ctx: &DecodeContext<'_>, ir: &CadIr, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    for (id, tolerance) in ir
        .model
        .vertices
        .iter()
        .map(|entity| (entity.id.as_str(), entity.tolerance))
        .chain(
            ir.model
                .edges
                .iter()
                .map(|entity| (entity.id.as_str(), entity.tolerance)),
        )
        .chain(
            ir.model
                .faces
                .iter()
                .map(|entity| (entity.id.as_str(), entity.tolerance)),
        )
    {
        ctx.charge_work(1, "topology tolerance row")?;
        if tolerance.is_some_and(|value| value.get() > 1.0e6) {
            super::record_finding(ctx, findings, Check::Tolerances, Severity::Warning, Some(id),
                format_args!("topology tolerance is outside a sane canonical range"))?;
        }
    }
    Ok(())
}

pub(super) fn check_references(ctx: &DecodeContext<'_>, ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    for b in &ir.model.bodies {
        for l in &b.regions {
            if ids.regions(l.as_str(), ctx)?.is_none() {
                ref_error(findings, b.id.as_str(), "region", l.as_str());
            }
        }
    }
    for l in &ir.model.regions {
        if ids.bodies(l.body.as_str(), ctx)?.is_none() {
            ref_error(findings, l.id.as_str(), "body", l.body.as_str());
        }
        for s in &l.shells {
            if ids.shells(s.as_str(), ctx)?.is_none() {
                ref_error(findings, l.id.as_str(), "shell", s.as_str());
            }
        }
    }
    for s in &ir.model.shells {
        if ids.regions(s.region.as_str(), ctx)?.is_none() {
            ref_error(findings, s.id.as_str(), "region", s.region.as_str());
        }
        for f in s.faces() {
            if ids.faces(f.as_str(), ctx)?.is_none() {
                ref_error(findings, s.id.as_str(), "face", f.as_str());
            }
        }
        for e in s.wire_edges() {
            if ids.edges(e.as_str(), ctx)?.is_none() {
                ref_error(findings, s.id.as_str(), "wire edge", e.as_str());
            }
        }
        for v in s.free_vertices() {
            if ids.vertices(v.as_str(), ctx)?.is_none() {
                ref_error(findings, s.id.as_str(), "free vertex", v.as_str());
            }
        }
    }
    for f in &ir.model.faces {
        if ids.shells(f.shell.as_str(), ctx)?.is_none() {
            ref_error(findings, f.id.as_str(), "shell", f.shell.as_str());
        }
        if ids.surfaces(f.surface.as_str(), ctx)?.is_none() {
            ref_error(findings, f.id.as_str(), "surface", f.surface.as_str());
        }
        for lp in &f.loops {
            if ids.loops(lp.as_str(), ctx)?.is_none() {
                ref_error(findings, f.id.as_str(), "loop", lp.as_str());
            }
        }
    }
    for lp in &ir.model.loops {
        if ids.faces(lp.face.as_str(), ctx)?.is_none() {
            ref_error(findings, lp.id.as_str(), "face", lp.face.as_str());
        }
        match &lp.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, pcurves } => {
                if ids.vertices(vertex.as_str(), ctx)?.is_none() {
                    ref_error(findings, lp.id.as_str(), "vertex", vertex.as_str());
                }
                for pcurve in pcurves {
                    if ids.pcurves(pcurve.pcurve.as_str(), ctx)?.is_none() {
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
                    if ids.coedges(ce.as_str(), ctx)?.is_none() {
                        ref_error(findings, lp.id.as_str(), "coedge", ce.as_str());
                    }
                }
                for use_ in ring.vertex_uses() {
                    if ids.vertices(use_.vertex.as_str(), ctx)?.is_none() {
                        ref_error(findings, lp.id.as_str(), "vertex", use_.vertex.as_str());
                    }
                    let after = &use_.after;
                    if ids.coedges(after.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            lp.id.as_str(),
                            "coedge(vertex-use after)",
                            after.as_str(),
                        );
                    }
                    for pcurve in &use_.pcurves {
                        if ids.pcurves(pcurve.pcurve.as_str(), ctx)?.is_none() {
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
        if ids.loops(ce.owner_loop.as_str(), ctx)?.is_none() {
            ref_error(findings, ce.id.as_str(), "loop", ce.owner_loop.as_str());
        }
        if ids.edges(ce.edge.as_str(), ctx)?.is_none() {
            ref_error(findings, ce.id.as_str(), "edge", ce.edge.as_str());
        }
        if ids.coedges(ce.radial_next.as_str(), ctx)?.is_none() {
            ref_error(
                findings,
                ce.id.as_str(),
                "coedge(radial_next)",
                ce.radial_next.as_str(),
            );
        }
        for use_ in &ce.pcurves {
            if ids.pcurves(use_.pcurve.as_str(), ctx)?.is_none() {
                ref_error(findings, ce.id.as_str(), "pcurve", use_.pcurve.as_str());
            }
        }
        if let Some(curve) = &ce.use_curve {
            if ids.curves(curve.curve.as_str(), ctx)?.is_none() {
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
            if ids.curves(c.as_str(), ctx)?.is_none() {
                ref_error(findings, e.id.as_str(), "curve", c.as_str());
            }
        }
        if ids.vertices(e.start.as_str(), ctx)?.is_none() {
            ref_error(findings, e.id.as_str(), "vertex(start)", e.start.as_str());
        }
        if ids.vertices(e.end.as_str(), ctx)?.is_none() {
            ref_error(findings, e.id.as_str(), "vertex(end)", e.end.as_str());
        }
    }
    for v in &ir.model.vertices {
        if ids.points(v.point.as_str(), ctx)?.is_none() {
            ref_error(findings, v.id.as_str(), "point", v.point.as_str());
        }
    }
    for binding in &ir.model.appearance_bindings {
        use crate::appearance::AppearanceTarget;
        let owner = format!("appearance-binding:{}", binding.appearance.as_str());
        if ids.appearances(binding.appearance.as_str(), ctx)?.is_none() {
            ref_error(findings, &owner, "appearance", binding.appearance.as_str());
        }
        match &binding.target {
            AppearanceTarget::Body(body) if ids.bodies(body.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "body", body.as_str());
            }
            AppearanceTarget::Face(face) if ids.faces(face.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "face", face.as_str());
            }
            AppearanceTarget::Edge(edge) if ids.edges(edge.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "edge", edge.as_str());
            }
            AppearanceTarget::Vertex(vertex) if ids.vertices(vertex.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "vertex", vertex.as_str());
            }
            AppearanceTarget::Surface(surface) if ids.surfaces(surface.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "surface", surface.as_str());
            }
            AppearanceTarget::Curve(curve) if ids.curves(curve.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "curve", curve.as_str());
            }
            AppearanceTarget::Point(point) if ids.points(point.as_str(), ctx)?.is_none() => {
                ref_error(findings, &owner, "point", point.as_str());
            }
            AppearanceTarget::Tessellation(tessellation)
                if ids.tessellations(tessellation, ctx)?.is_none() =>
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
            AttributeTarget::Body(id) if ids.bodies(id.as_str(), ctx)?.is_none() => {
                ref_error(findings, owner, "body", id.as_str());
            }
            AttributeTarget::Face(id) if ids.faces(id.as_str(), ctx)?.is_none() => {
                ref_error(findings, owner, "face", id.as_str());
            }
            AttributeTarget::Coedge(id) if ids.coedges(id.as_str(), ctx)?.is_none() => {
                ref_error(findings, owner, "coedge", id.as_str());
            }
            AttributeTarget::Edge(id) if ids.edges(id.as_str(), ctx)?.is_none() => {
                ref_error(findings, owner, "edge", id.as_str());
            }
            AttributeTarget::Vertex(id) if ids.vertices(id.as_str(), ctx)?.is_none() => {
                ref_error(findings, owner, "vertex", id.as_str());
            }
            _ => {}
        }
    }
    for s in &ir.model.surfaces {
        match &s.geometry {
            SurfaceGeometry::Procedural { construction, .. } => {
                if ids.procedural_surfaces(construction.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        s.id.as_str(),
                        "procedural surface construction",
                        construction.as_str(),
                    );
                }
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: Some(u) })
                if !ids.contains(u.as_str()) =>
            {
                ref_error(findings, s.id.as_str(), "unknown record", u.as_str());
            }
            SurfaceGeometry::Solved(_) => {}
        }
    }
    for curve in &ir.model.curves {
        match &curve.geometry {
            CurveGeometry::Procedural { construction, .. } => {
                if ids.procedural_curves(construction.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        curve.id.as_str(),
                        "procedural curve construction",
                        construction.as_str(),
                    );
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(unknown),
            }) => {
                if !ids.contains(unknown.as_str()) {
                    ref_error(
                        findings,
                        curve.id.as_str(),
                        "unknown record",
                        unknown.as_str(),
                    );
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) => {
                for segment in segments {
                    if ids.curves(segment.curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, curve.id.as_str(), "curve", segment.curve.as_str());
                    }
                }
            }
            CurveGeometry::Solved(_) => {}
        }
    }
    composite::check(ctx, ir, findings)?;
    for procedural in &ir.model.procedural_surfaces {
        match procedural.definition() {
            ProceduralSurfaceDefinition::Exact(..) => {}
            ProceduralSurfaceDefinition::Compound(definition_payload) => {
                let components = definition_payload.components();

                for component in components {
                    if ids.surfaces(component.component.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            component.component.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::SubSurface(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::Replica {
                source: support, ..
            } => {
                if ids.surfaces(support.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Taper(definition_payload) => {
                let support = definition_payload.support();
                let reference = definition_payload.reference();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        );
                    }
                    if ids.curves(reference.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            reference.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::Loft(definition_payload) => {
                let sections = definition_payload.sections();

                for entry in sections.iter().flat_map(|section| &section.entries) {
                    for curve in entry
                        .path
                        .path
                        .iter()
                        .map(|curve| &curve.id)
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.profile.id))
                    {
                        if ids.curves(curve.as_str(), ctx)?.is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = member.form.surface() {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralSurfaceDefinition::CompoundLoft(definition_payload) => {
                let construction = definition_payload.construction();

                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| -> Result<(), CodecError> {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                        Ok(())
                };
                let mut scales = construction.scales.as_slice().iter().collect::<Vec<_>>();
                match &construction.tail {
                    crate::geometry::CompoundLoftTail::Six { scale, curve, .. } => {
                        scales.push(scale.as_ref());
                        check_curve(curve, findings)?;
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
                            check_curve(curve, findings)?;
                        }
                    }
                }
                for scale in scales {
                    check_curve(&scale.path, findings)?;
                    for curve in &scale.auxiliaries {
                        check_curve(curve, findings)?;
                    }
                    for member in &scale.members {
                        check_curve(&member.curve, findings)?;
                        let surface = &member.data.surface;
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) => {
                let construction = definition_payload.construction();

                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| -> Result<(), CodecError> {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                        Ok(())
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
                        check_curve(curve, findings)?;
                    }
                    crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
                            check_curve(curve, findings)?;
                        }
                    }
                }
                check_curve(&construction.tail_curve, findings)?;
                for scale in scales {
                    check_curve(&scale.path, findings)?;
                    for curve in &scale.auxiliaries {
                        check_curve(curve, findings)?;
                    }
                    for member in &scale.members {
                        check_curve(&member.curve, findings)?;
                        let surface = &member.data.surface;
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralSurfaceDefinition::Skin(definition_payload) => {
                let construction = definition_payload.construction();
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| -> Result<(), CodecError> {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                        Ok(())
                };
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        check_curve(path, findings)?;
                        for profile in profiles {
                            check_curve(&profile.curve, findings)?;
                            let surface = &profile.data.surface;
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                        check_curve(curve, findings)?;
                        check_curve(secondary_curve, findings)?;
                    }
                }
                check_curve(&construction.parameter_curve, findings)?;
                for variable in construction.formula.variables() {
                    check_law_curves(ctx, variable, ids, procedural, findings)?;
                }
            }
            ProceduralSurfaceDefinition::Law(definition_payload) => {
                let construction = definition_payload.construction();
                for formula in
                    std::iter::once(&construction.primary).chain(&construction.additional)
                {
                    for variable in formula.variables() {
                        check_law_curves(ctx, variable, ids, procedural, findings)?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Net(definition_payload) => {
                let construction = definition_payload.construction();
                for entry in construction
                    .sections
                    .iter()
                    .flat_map(|section| &section.entries)
                {
                    for curve in entry
                        .path
                        .path
                        .iter()
                        .map(|curve| &curve.id)
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.profile.id))
                    {
                        if ids.curves(curve.as_str(), ctx)?.is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = member.form.surface() {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                        check_law_curves(ctx, variable, ids, procedural, findings)?;
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend(definition_payload) => {
                let construction = definition_payload.construction();

                for surface in [&construction.first.surface, &construction.second.surface]
                    .into_iter()
                    .chain(std::iter::once(&construction.second_exact_surface))
                {
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                    if ids.surfaces(support.surface.as_str(), ctx)?.is_none() {
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
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
                let construction = definition_payload.construction();

                for side in &construction.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.surface.as_str(),
                            );
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(curve.curve.as_str(), ctx)?.is_none() {
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
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                for member in construction.base_profile().iter().chain(
                    construction
                        .entries()
                        .iter()
                        .flat_map(|entry| &entry.profile),
                ) {
                    if ids.curves(member.profile.id.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            member.profile.id.as_str(),
                        );
                    }
                    if let Some(surface) = member.form.surface() {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            );
                        }
                    }
                }
                for curve in std::iter::once(construction.base_path())
                    .chain(construction.entries().iter().map(|entry| &entry.path))
                    .flat_map(|path| {
                        path.path
                            .iter()
                            .map(|curve| &curve.id)
                            .chain(path.auxiliaries.iter())
                    })
                    .chain(match construction.direction() {
                        crate::geometry::CompoundLoftDirection::Vector { .. } => None,
                        crate::geometry::CompoundLoftDirection::Curve { curve, .. } => Some(curve),
                    })
                    .chain(construction.tail().curve())
                {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.surface.as_str(),
                            );
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(curve.curve.as_str(), ctx)?.is_none() {
                            ref_error(
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.curve.as_str(),
                            );
                        }
                    }
                }
                if ids.curves(construction.center().as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        construction.center().as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::VertexBlend(definition_payload) => {
                let construction = definition_payload.construction();

                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            if ids.curves(curve.as_str(), ctx)?.is_none() {
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
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
                let directrix = definition_payload.directrix();
                {
                    if ids.curves(directrix.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            directrix.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
                let directrix = definition_payload.directrix();
                if ids.curves(directrix.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        directrix.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                let directrix = definition_payload.directrix();
                {
                    if ids.curves(directrix.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            directrix.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
                let directrix = definition_payload.directrix();
                if ids.curves(directrix.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        directrix.as_str(),
                    );
                }
            }
            ProceduralSurfaceDefinition::Sweep(definition_payload) => {
                let profile = definition_payload.profile();
                let spine = definition_payload.spine();
                let native = definition_payload.native();
                for curve in [profile, spine] {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
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
                            if ids.curves(guide_curve.as_str(), ctx)?.is_none() {
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
                            if ids.surfaces(support_surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    support_surface.as_str(),
                                );
                            }
                            if let Some(curve) = auxiliary_curve {
                                if ids.curves(curve.as_str(), ctx)?.is_none() {
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
                            check_law_curves(ctx, first_law, ids, procedural, findings)?;
                            check_law_curves(ctx, second_law, ids, procedural, findings)?;
                            vec![formula]
                        }
                    };
                    for formula in formulas {
                        for variable in formula.variables() {
                            check_law_curves(ctx, variable, ids, procedural, findings)?;
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::Subset(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::ParallelOffset(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
                for curve in [first, second] {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::Sum(definition_payload) => {
                for curve in [definition_payload.first(), definition_payload.second()] {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::Blend(definition_payload) => {
                let supports = definition_payload.supports();
                let spine = definition_payload.spine();
                let native = definition_payload.native();

                for support in supports.iter().flatten() {
                    if ids.surfaces(support.surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.surface.as_str(),
                        );
                    }
                }
                if let Some(spine) = spine {
                    if ids.curves(spine.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", spine.as_str());
                    }
                }
                if let Some(native) = native {
                    let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| -> Result<(), CodecError> {
                        if ids.curves(curve.as_str(), ctx)?.is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                        Ok(())
                    };
                    let check_surface =
                        |surface: &crate::ids::SurfaceId, findings: &mut Vec<Finding>| -> Result<(), CodecError> {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                );
                            }
                        Ok(())
                        };
                    check_curve(&native.slice, findings)?;
                    for side in &native.sides {
                        if let Some(curve) = &side.curve {
                            check_curve(&curve.curve, findings)?;
                        }
                        if let Some(surface) = &side.surface {
                            check_surface(&surface.surface, findings)?;
                        }
                    }
                    if let Some(side) = &native.third {
                        check_curve(&side.curve, findings)?;
                        check_surface(&side.surface, findings)?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Unknown {
                record: Some(record),
                ..
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
            | ProceduralSurfaceDefinition::Unknown { record: None, .. } => {}
            ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                boundary_pcurves,
                ..
            } => {
                if ids.surfaces(support.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    );
                }
                for boundary in boundaries {
                    if ids.curves(boundary.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", boundary.as_str());
                    }
                }
                for pcurve in boundary_pcurves {
                    if ids.pcurves(pcurve.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "pcurve boundary",
                            pcurve.as_str(),
                        );
                    }
                }
            }
            ProceduralSurfaceDefinition::Deformable(definition_payload) => {
                let construction = definition_payload.construction();

                if ids.surfaces(construction.support.as_str(), ctx)?.is_none() {
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
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        );
                    }
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                    }
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        match procedural.definition() {
            ProceduralCurveDefinition::Exact { .. } | ProceduralCurveDefinition::Helix(_) => {}
            ProceduralCurveDefinition::Law {
                context,
                primary,
                additional,
                ..
            } => {
                fn check<R, V, P>(ctx: &DecodeContext<'_>, 
                    expression: &crate::geometry::LawExpression<R, V, P>,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralCurve,
                    findings: &mut Vec<Finding>,
                ) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("law reference depth")?;
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(curve.id.as_str(), ctx)?.is_none() {
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
                                check(ctx, operand, ids, procedural, findings)?;
                            }
                        }
                        _ => {}
                    }
                
    Ok(())
}
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                    for variable in formula.formula().variables() {
                        check(ctx, variable, ids, procedural, findings)?;
                    }
                }
            }
            ProceduralCurveDefinition::Compound(compound) => {
                let components = compound.components();

                for component in components {
                    if ids.curves(component.component.as_str(), ctx)?.is_none() {
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
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                let supports = intersection.supports();

                for surface in supports {
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        );
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) => {
                let context = definition_payload.context();
                let third = definition_payload.third();

                for side in context.sides().iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::Silhouette(definition_payload) => {
                let context = definition_payload.context();
                let cast_surface = definition_payload.cast_surface();
                if ids.surfaces(cast_surface.as_str(), ctx)?.is_none() {
                    ref_error(
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        cast_surface.as_str(),
                    );
                }
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::SurfaceOffset(definition_payload) => {
                let context = definition_payload.context();
                let base = definition_payload.base();
                {
                    if ids.curves(base.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", base.as_str());
                    }
                    for side in context.sides() {
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::Spring(definition_payload) => {
                for side in definition_payload.support_context().sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::Deformable(definition_payload) => {
                let context = definition_payload.context();
                let source = definition_payload.source();
                {
                    if let crate::geometry::DeformableCurveSource::Curve { curve } = source {
                        if ids.curves(curve.as_str(), ctx)?.is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", curve.as_str());
                        }
                    }
                    for side in context.sides() {
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::Projection(definition_payload) => {
                let context = definition_payload.context();
                let source = definition_payload.source();

                if ids.curves(source.as_str(), ctx)?.is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
                for side in context.sides() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::Offset(definition_payload) => {
                let source = definition_payload.source();
                let side = definition_payload.side();
                let range = definition_payload.range();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                    }
                    if let crate::geometry::OffsetSide::Direction {
                        support: Some(support),
                        ..
                    } = side
                    {
                        if ids.surfaces(support.as_str(), ctx)?.is_none() {
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
                        if ids.curves(function.as_str(), ctx)?.is_none() {
                            ref_error(findings, procedural.id.as_str(), "curve", function.as_str());
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SpatialOffset(definition_payload) => {
                let source = definition_payload.source();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset(definition_payload) => {
                let context = definition_payload.context();
                {
                    for side in context.sides() {
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
            ProceduralCurveDefinition::VectorOffset(definition_payload) => {
                let source = definition_payload.source();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::Replica { source, .. } => {
                if ids.curves(source.as_str(), ctx)?.is_none() {
                    ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                }
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(findings, procedural.id.as_str(), "curve", source.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
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
                ..
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
                ..
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
            Definition::Disabled {} => (Vec::new(), None),
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
    check_feature_references(ctx, ir, ids, findings)?;
    Ok(())
}

fn check_feature_references(ctx: &DecodeContext<'_>, ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    use crate::features::{
        EdgeSelection, FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef, ScaleCenter,
    };

    if let Err(error) = crate::document::feature_parents::validate(Some(ctx), &[&ir.model])? {
        super::record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(error.owner().as_str()), format_args!("{error}"))?;
    }

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
        for body in configuration.bodies.iter().flatten() {
            if ids.bodies(body.as_str(), ctx)?.is_none() {
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
            for reference in regeneration_references(state.definition.operation()) {
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
                if ids.bodies(output.as_str(), ctx)?.is_none() {
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
    let feature_records_storage = ctx.with_scoped_storage("feature reference index", || {
        let mut records = HashMap::new();
        let mut longest = 0;
        for feature in &ir.model.features {
            ctx.charge_work(1, "feature reference index scan")?;
            longest = longest.max(feature.id.as_str().len());
            plane_lookup_work(ctx, records.len(), feature.id.as_str().len(), longest)?;
            ctx.insert_hash_map(&mut records, feature.id.as_str(), feature, "feature reference index")?;
        }
        Ok::<_, CodecError>((records, longest))
    })?;
    let (feature_records, longest_feature_id) = &feature_records_storage.0;
    let longest_feature_id = *longest_feature_id;
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
    let mut reported_storage = ctx.reserve_scoped(0, "datum-plane reported cycles")?;
    let mut reported_plane_cycles = HashSet::new();
    for feature in &ir.model.features {
        let mut path_storage = ctx.reserve_scoped(0, "datum-plane traversal storage")?;
            let mut path: Vec<(&str, cadmpeg_core::decode::DepthGuard<'_>)> = Vec::new();
            let mut positions = HashMap::new();
            let mut cursor = feature.id.as_str();
            let mut longest_path_id = 0;
            loop {
                ctx.charge_work(1, "datum-plane traversal")?;
                longest_path_id = longest_path_id.max(cursor.len());
                plane_lookup_work(ctx, positions.len(), cursor.len(), longest_path_id)?;
                if let Some(&cycle_start) = positions.get(cursor) {
                    let mut cycle_storage = ctx.with_scoped_storage("datum-plane cycle identities", || {
                        ctx.collect_vec(path[cycle_start..].iter().map(|(id, _)| *id), "datum-plane cycle identities")
                    })?;
                    let cycle = &mut cycle_storage.0;
                    ctx.sort_unstable_by(cycle, Ord::cmp, |id| id.len(), "sort datum-plane cycle identities")?;
                    ctx.charge_work(u64_from_index(cycle.len()), "datum-plane cycle byte scan")?;
                    let cycle_bytes = cycle.iter().try_fold(0u64, |bytes, id| {
                        bytes.checked_add(u64_from_index(id.len()))
                    }).ok_or_else(|| ctx.refuse_codec_limit("datum-plane cycle hash", u64::MAX - 1, u64::MAX))?;
                    ctx.charge_work(cycle_bytes, "datum-plane cycle hash")?;
                    let comparisons = u64_from_index(reported_plane_cycles.len()).checked_add(1)
                        .and_then(|count| count.checked_mul(cycle_bytes.checked_mul(2)?.checked_add(1)?))
                        .ok_or_else(|| ctx.refuse_codec_limit("datum-plane cycle comparisons", u64::MAX - 1, u64::MAX))?;
                    ctx.charge_work(comparisons, "datum-plane cycle comparisons")?;
                    if !reported_plane_cycles.contains(&*cycle) {
                        let finding = Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: ctx.format_retained(format_args!("datum-plane reference cycle contains {}", PlaneCyclePath(cycle)), "datum-plane cycle finding")?,
                            entity: Some(ctx.copy_retained_text(feature.id.as_str(), "datum-plane cycle finding identity")?),
                        };
                        ctx.push_vec(findings, finding, "datum-plane cycle findings")?;
                        reported_storage.with_storage(|| {
                            let copy = ctx.collect_vec(cycle.iter().copied(), "datum-plane reported cycle identities")?;
                            ctx.charge_work(cycle_bytes, "datum-plane reported cycle hash")?;
                            ctx.charge_work(comparisons, "datum-plane reported cycle comparisons")?;
                            ctx.insert_hash_set(&mut reported_plane_cycles, copy, "datum-plane reported cycles")
                        })?;
                    }
                    break;
                }
                plane_lookup_work(ctx, positions.len(), cursor.len(), longest_path_id)?;
                path_storage.with_storage(|| ctx.insert_hash_map(&mut positions, cursor, path.len(), "datum-plane traversal positions"))?;
                let depth = ctx.enter_nested("datum-plane traversal depth")?;
                ctx.push_scoped_vec(&mut path_storage, &mut path, (cursor, depth), "datum-plane traversal path")?;
                plane_lookup_work(ctx, feature_records.len(), cursor.len(), longest_feature_id)?;
                let Some(next) = feature_records.get(cursor).and_then(|feature| {
                    let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                        reference: Some(DatumPlaneReference::Feature { feature: reference }),
                        ..
                    }) = feature.evaluation.definition()
                    else { return None; };
                    Some(reference.as_str())
                }) else { break; };
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
            if ids.bodies(body.as_str(), ctx)?.is_none() {
                ref_error(findings, feature.id.as_str(), "output body", body.as_str());
            }
        }

        let mut paths = Vec::new();
        let mut edge_selections = Vec::new();
        let mut face_selections = Vec::new();
        let mut vertex_selections = Vec::new();
        let mut body_selections = Vec::new();
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. }
            | FeatureDefinition::Operation(operation) => operation,
        };
        match definition {
            FeatureOperation::Unresolved { .. }
            | FeatureOperation::Primitive { .. }
            | FeatureOperation::SheetMetalBaseFlange { .. }
            | FeatureOperation::PlanarPatch { .. } => {}
            FeatureOperation::ReferenceImage { asset, .. } => {
                if !asset_ids.contains(asset.as_str()) {
                    ref_error(
                        findings,
                        feature.id.as_str(),
                        "reference-image asset",
                        asset.as_str(),
                    );
                }
            }
            FeatureOperation::Decal { asset, faces, .. } => {
                if !asset_ids.contains(asset.as_str()) {
                    ref_error(findings, feature.id.as_str(), "decal asset", asset.as_str());
                }
                face_selections.push(faces);
            }
            FeatureOperation::Block { .. } => {}

            FeatureOperation::ExtractBody { source } => body_selections.push(source),
            FeatureOperation::FaceBlend { operands, .. } => {
                face_selections.push(operands.first_faces());
                face_selections.push(operands.second_faces());
            }
            FeatureOperation::FullRoundFillet { groups } => {
                for group in groups {
                    face_selections.push(group.center_faces());
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        if let crate::features::edge_treatments::FullRoundSideSelection::Explicit(
                            selection,
                        ) = side
                        {
                            face_selections.push(selection);
                        }
                    }
                }
            }
            FeatureOperation::SewBodies { bodies, .. } => body_selections.push(bodies),
            FeatureOperation::BaseFeature { bodies } => body_selections.push(bodies),
            FeatureOperation::MeshImport { tessellations } => {
                for tessellation in tessellations {
                    if ids.tessellations(tessellation, ctx)?.is_none() {
                        ref_error(
                            findings,
                            feature.id.as_str(),
                            "mesh import tessellation",
                            tessellation,
                        );
                    }
                }
            }
            FeatureOperation::InsertBodies { .. } => {}
            FeatureOperation::InsertComponent { occurrence } => {
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
            FeatureOperation::AssemblyJoint { joint } => {
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
            FeatureOperation::Form { cages } => {
                check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "Form control cage",
                    cages.iter().map(super::super::ids::SubdId::as_str),
                    |identity| Ok(ids.subds(identity, ctx)?.is_some()),
                )?;
            }
            FeatureOperation::CosmeticThread { face, .. } => face_selections.push(face),
            FeatureOperation::Extrude {
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
            FeatureOperation::SheetMetalEdgeFlange { edges, height, .. } => {
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
            FeatureOperation::SheetMetalHem { edges, .. } => edge_selections.push(edges),
            FeatureOperation::Revolve { construction, .. } => {
                paths.extend(construction.axis().and_then(|axis| axis.reference.as_ref()));
            }
            FeatureOperation::Sweep {
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
            FeatureOperation::Loft {
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
                        ) => check_ids(ctx, 
                            findings,
                            feature.id.as_str(),
                            "loft section vertex",
                            std::iter::once(vertex.as_str()),
                            |identity| Ok(ids.vertices(identity, ctx)?.is_some()),
                        )?,
                    }
                }
                match guidance {
                    crate::features::LoftGuidance::Guides(guides) => paths.extend(guides),
                    crate::features::LoftGuidance::Centerline(centerline) => paths.push(centerline),
                }
            }
            FeatureOperation::Rib { .. } => {}
            FeatureOperation::Fillet { groups } => {
                edge_selections.extend(groups.iter().map(|group| &group.edges));
            }
            FeatureOperation::Chamfer { groups, .. } => {
                edge_selections.extend(groups.iter().map(|group| &group.edges));
            }
            FeatureOperation::Shell {
                bodies,
                removed_faces,
                ..
            } => {
                if let Some(bodies) = bodies {
                    body_selections.push(bodies);
                }
                face_selections.push(removed_faces);
            }
            FeatureOperation::OffsetShape { source, .. } => body_selections.push(source),
            FeatureOperation::Compound { members } => body_selections.push(members),
            FeatureOperation::RefineShape { source }
            | FeatureOperation::ReverseShape { source } => body_selections.push(source),
            FeatureOperation::RuledBetweenCurves { first, second, .. } => {
                paths.push(first);
                paths.push(second);
            }
            FeatureOperation::SectionShape { operands, .. } => {
                body_selections.push(operands.first());
                body_selections.push(operands.second());
            }
            FeatureOperation::MirrorShape {
                source,
                plane_reference,
                ..
            } => {
                body_selections.push(source);
                face_selections.extend(plane_reference);
            }
            FeatureOperation::Thicken { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::OffsetSurface { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::KnitSurface { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::FilledSurface {
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
            FeatureOperation::TrimSurface { faces, tool, .. } => {
                face_selections.push(faces);
                paths.push(tool);
            }
            FeatureOperation::ExtendSurface { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::RuledSurface {
                edges,
                support_faces,
                ..
            } => {
                edge_selections.push(edges);
                face_selections.push(support_faces);
            }
            FeatureOperation::Draft { faces, anchor, .. } => {
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
            FeatureOperation::BoundaryFill { tools, cells } => {
                body_selections.push(tools);
                body_selections.extend(cells);
            }
            FeatureOperation::SplitBody { targets, tools } => {
                body_selections.push(targets);
                face_selections.push(tools);
            }
            FeatureOperation::SplitFace { targets, tool } => {
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
            FeatureOperation::DeleteFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::ReplaceFace { operands } => {
                face_selections.push(operands.targets());
                face_selections.push(operands.replacements());
            }
            FeatureOperation::MoveFace { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::MoveBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureOperation::Dome { faces, .. } => {
                face_selections.push(faces);
            }
            FeatureOperation::Flex { .. } => {}
            FeatureOperation::Scale { bodies, center, .. } => {
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
            FeatureOperation::Combine { operands, .. } => {
                body_selections.push(operands.target());
                body_selections.push(operands.tools());
            }
            FeatureOperation::CutWithSurface { targets, tools, .. } => {
                body_selections.push(targets);
                face_selections.push(tools);
            }
            FeatureOperation::TrimBodies { operands, .. } => {
                body_selections.push(operands.targets());
                body_selections.push(operands.tools());
            }
            FeatureOperation::DeleteBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureOperation::Hole { face, .. } => face_selections.extend(face),
            FeatureOperation::Pattern { seeds, pattern } => {
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
            FeatureOperation::Sketch { sketch, .. } => {
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
            FeatureOperation::SpatialSketch { sketch } => {
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
            FeatureOperation::DatumCoordinateSystem { .. } => {}
            FeatureOperation::EquationCurve { .. } => {}

            FeatureOperation::ProjectedCurve {
                source,
                target_faces,
                ..
            } => {
                paths.push(source);
                face_selections.push(target_faces);
            }
            FeatureOperation::ProjectOnSurface {
                sources,
                support_face,
                ..
            } => {
                paths.push(sources);
                face_selections.push(support_face);
            }
            FeatureOperation::CompositeCurve { segments, .. } => {
                paths.extend(segments);
            }
            FeatureOperation::Helix { .. } => {}
            FeatureOperation::HelixNativeAxis { .. } => {}
            FeatureOperation::Coil { result, .. } => {
                use crate::features::CoilResult;
                if let CoilResult::Boolean { targets, .. } = result {
                    body_selections.push(targets);
                }
            }
            FeatureOperation::HelicalSweep { .. } => {}

            FeatureOperation::Binder {
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
            FeatureOperation::Wrap { face, .. } => {
                face_selections.push(face);
            }
            FeatureOperation::Sphere { .. } => {}
            FeatureOperation::Torus { .. } => {}
            FeatureOperation::PointGeometry { .. } => {}
            FeatureOperation::LineSegment { .. } => {}

            FeatureOperation::CircularArc { .. } => {}

            FeatureOperation::EllipticArc { .. } => {}

            FeatureOperation::Polyline { .. } => {}

            FeatureOperation::RegularPolygonCurve { .. } => {}
            FeatureOperation::FaceFromShapes { sources, .. } => {
                body_selections.push(sources);
            }
            FeatureOperation::TreeNode { children, .. } => {
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
            FeatureOperation::DatumPlane { .. } => {}
            FeatureOperation::DatumThreePointPlane { points, .. } => {
                for point in points.iter() {
                    vertex_selections.push((point, "three-point datum-plane"));
                }
            }
            FeatureOperation::DatumAxis { .. } => {}
            FeatureOperation::DatumPoint { construction, .. } => {
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
                        DatumPlaneReference::Feature { feature: reference } => {
                            match feature_records.get(reference.as_str()) {
                                None => ref_error(
                                    findings,
                                    feature.id.as_str(),
                                    "datum-point plane",
                                    reference.as_str(),
                                ),
                                Some(record)
                                    if !matches!(
                                        record.evaluation.definition().operation(),
                                        FeatureOperation::DatumPrincipalPlane { .. }
                                            | FeatureOperation::DatumPlane { .. }
                                            | FeatureOperation::Unresolved {
                                                family: UnresolvedFamily::DatumPlane
                                            }
                                            | FeatureOperation::DatumOffsetPlane { .. }
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
                        DatumPlaneReference::Face { face } => face_selections.push(face),
                        DatumPlaneReference::ResolvedPlane { .. } => {}
                    }
                }
            }
            FeatureOperation::DatumPrincipalPlane { .. }
            | FeatureOperation::SketchBlockDefinition { .. }
            | FeatureOperation::StoredGeometry {}
            | FeatureOperation::Native { .. } => {}
            FeatureOperation::SketchBlockInstance {
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
                                        candidate.evaluation.definition().operation(),
                                        FeatureOperation::SketchBlockDefinition { .. }
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
            FeatureOperation::DerivedGeometry { source } => match features.get(source.as_str()) {
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
            FeatureOperation::ImportedGeometry { .. } => {}
            FeatureOperation::DatumOffsetPlane { reference, .. } => {
                if let Some(reference) = reference {
                    match reference {
                        DatumPlaneReference::Feature { feature: reference } => {
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
                                        record.evaluation.definition().operation(),
                                        FeatureOperation::DatumPrincipalPlane { .. }
                                            | FeatureOperation::DatumPlane { .. }
                                            | FeatureOperation::Unresolved {
                                                family: UnresolvedFamily::DatumPlane
                                            }
                                            | FeatureOperation::DatumOffsetPlane { .. }
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
                        DatumPlaneReference::Face { face } => face_selections.push(face),
                        DatumPlaneReference::ResolvedPlane { .. } => {}
                    }
                }
            }
        }
        for profile in definition_profiles(definition) {
            match profile {
                PlanarProfileRef::Faces(faces) => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "profile face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?,
                PlanarProfileRef::HistoricalFaces { state, faces, .. } => {
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
                PlanarProfileRef::Feature(producer) => match features.get(producer.as_str()) {
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
                PlanarProfileRef::Generated { curves, .. }
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
                PathRef::Edges(edges) => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "path edge",
                    edges.iter().map(super::super::ids::EdgeId::as_str),
                    |identity| Ok(ids.edges(identity, ctx)?.is_some()),
                )?,
                PathRef::Curves(curves) => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "path curve",
                    curves.iter().map(super::super::ids::CurveId::as_str),
                    |identity| Ok(ids.curves(identity, ctx)?.is_some()),
                )?,
                PathRef::SketchCurves { curves, .. } => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "sketch path curve",
                    curves.iter().map(crate::sketches::SketchEntityId::as_str),
                    |identity| Ok(sketch_entities.contains(identity)),
                )?,
                PathRef::SpatialSketchCurves { curves, .. } => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "spatial sketch path curve",
                    curves
                        .iter()
                        .map(crate::sketches::SpatialSketchEntityId::as_str),
                    |identity| Ok(spatial_sketch_entity_owners.contains_key(identity)),
                )?,
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
                check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "termination face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?;
            }
            if let Some(FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. }) =
                termination.shape()
            {
                check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "termination shape face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?;
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
                EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "selected edge",
                    edges.iter().map(super::super::ids::EdgeId::as_str),
                    |identity| Ok(ids.edges(identity, ctx)?.is_some()),
                )?,
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
                FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => check_ids(ctx, 
                    findings,
                    feature.id.as_str(),
                    "selected face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?,
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
                    check_ids(ctx, 
                        findings,
                        feature.id.as_str(),
                        "selected body",
                        bodies.iter().map(super::super::ids::BodyId::as_str),
                        |identity| Ok(ids.bodies(identity, ctx)?.is_some()),
                    )?;
                }
                BodySelection::ResolvedSet { members } => {
                    check_ids(ctx, 
                        findings,
                        feature.id.as_str(),
                        "selected body",
                        members.bodies().map(super::super::ids::BodyId::as_str),
                        |identity| Ok(ids.bodies(identity, ctx)?.is_some()),
                    )?;
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
    Ok(())
}

struct PlaneCyclePath<'a, 'id>(&'a [&'id str]);

impl fmt::Display for PlaneCyclePath<'_, '_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, id) in self.0.iter().enumerate() {
            if index != 0 { output.write_str(", ")?; }
            write!(output, "`{id}`")?;
        }
        Ok(())
    }
}

fn plane_lookup_work(ctx: &DecodeContext<'_>, count: usize, key_bytes: usize, longest: usize) -> Result<(), CodecError> {
    let work = u64_from_index(count).checked_add(1)
        .and_then(|count| count.checked_mul(u64_from_index(key_bytes).checked_add(u64_from_index(longest))?.checked_add(1)?))
        .ok_or_else(|| ctx.refuse_codec_limit("datum-plane identity lookup", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, "datum-plane identity lookup")
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
    definition: &crate::features::FeatureOperation,
) -> impl Iterator<Item = &crate::features::FeatureId> {
    let mut references = BTreeSet::new();
    match definition {
        // A datum offset plane regenerates from its reference only when that
        // reference names a feature; a face-supported plane carries its frame
        // inline and is checked through the face selection instead.
        crate::features::FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            ..
        } => {
            references.insert(reference);
        }
        crate::features::FeatureOperation::DatumPoint {
            construction: Some(construction),
            ..
        } => references.extend(construction.feature_references()),
        crate::features::FeatureOperation::DatumThreePointPlane { points, .. } => {
            references.extend(points.iter().filter_map(|point| match point {
                crate::features::VertexSelection::Generated { vertex, .. } => Some(&vertex.feature),
                crate::features::VertexSelection::Historical { .. }
                | crate::features::VertexSelection::Native(_)
                | crate::features::VertexSelection::Unresolved => None,
            }));
        }
        crate::features::FeatureOperation::DerivedGeometry { source: reference }
        | crate::features::FeatureOperation::SketchBlockInstance {
            block: Some(reference),
            ..
        } => {
            references.insert(reference);
        }
        crate::features::FeatureOperation::Pattern { seeds, .. } => {
            references.extend(seeds.iter().filter_map(|seed| match seed {
                crate::features::patterns::PatternSeed::Feature(feature) => Some(feature),
                crate::features::patterns::PatternSeed::Faces(_)
                | crate::features::patterns::PatternSeed::Bodies(_)
                | crate::features::patterns::PatternSeed::Occurrences(_) => None,
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
            crate::features::PlanarProfileRef::Feature(feature) => {
                references.insert(feature);
            }
            crate::features::PlanarProfileRef::Generated { curves, .. } => {
                references.extend(curves.iter().map(|curve| &curve.feature));
            }
            _ => {}
        }
    }
    references.into_iter()
}

fn definition_profiles(
    definition: &crate::features::FeatureOperation,
) -> impl Iterator<Item = &crate::features::PlanarProfileRef> {
    let mut profiles: Vec<&crate::features::PlanarProfileRef> = Vec::new();
    match definition {
        crate::features::FeatureOperation::Extrude { profile, .. } => {
            profiles.extend(profile.planar());
        }
        crate::features::FeatureOperation::SheetMetalBaseFlange { profile, .. }
        | crate::features::FeatureOperation::Wrap { profile, .. } => profiles.push(profile),
        crate::features::FeatureOperation::Revolve { construction, .. } => {
            profiles.extend(construction.profile());
        }
        crate::features::FeatureOperation::Rib { construction, .. } => {
            profiles.extend(construction.profile.as_ref());
        }
        crate::features::FeatureOperation::Sweep { shape, .. } => {
            profiles.extend(shape.referenced_profiles());
        }
        crate::features::FeatureOperation::HelicalSweep { construction, .. } => {
            profiles.push(&construction.profile);
        }
        crate::features::FeatureOperation::Loft { sections, .. } => {
            profiles.extend(sections.iter().filter_map(|section| match section {
                crate::features::LoftSection::Profile(profile) => profile.planar(),
                crate::features::LoftSection::Point(_) => None,
            }));
        }
        crate::features::FeatureOperation::Hole {
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
    definition: &crate::features::FeatureOperation,
) -> impl Iterator<Item = TerminationRef<'_>> {
    let mut terminations = Vec::new();
    match definition {
        crate::features::FeatureOperation::Extrude { extent, .. } => match extent {
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
        crate::features::FeatureOperation::Revolve { construction, .. } => {
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
        crate::features::FeatureOperation::Hole {
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
                record.evaluation.definition().operation(),
                crate::features::FeatureOperation::DatumPrincipalPlane { .. }
                    | crate::features::FeatureOperation::DatumPlane { .. }
                    | crate::features::FeatureOperation::Unresolved {
                        family: crate::features::UnresolvedFamily::DatumPlane
                    }
                    | crate::features::FeatureOperation::DatumOffsetPlane { .. }
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

fn check_ids<'a>(ctx: &DecodeContext<'_>, 
    findings: &mut Vec<Finding>,
    owner: &str,
    kind: &str,
    values: impl Iterator<Item = &'a str>,
    valid: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    for value in values {
        ctx.charge_work(1, "selected identity validation")?;
        if !valid(value)? {
            ref_error(findings, owner, kind, value);
        }
    }

    Ok(())
}

fn check_feature_sketch_references(
    ir: &CadIr,
    sketches: &HashSet<&str>,
    findings: &mut Vec<Finding>,
) {
    use crate::features::{
        FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef, ProfileRef,
        SketchPointSelection,
    };

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
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: crate::features::SketchFeatureBinding::Planar(Some(sketch)),
                ..
            }) => sketch.as_str(),
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                sketch: Some(sketch),
            }) => sketch.as_str(),
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
        let FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            construction: Some(construction),
            ..
        }) = feature.evaluation.definition()
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
        let mut profiles: Vec<crate::features::ProfileRef> = Vec::new();
        let mut paths = Vec::new();
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. }
            | FeatureDefinition::Operation(operation) => operation,
        };
        match definition {
            FeatureOperation::Extrude { profile, .. } => {
                profiles.push(profile.clone());
            }
            FeatureOperation::SheetMetalBaseFlange { profile, .. } => {
                profiles.push(ProfileRef::Planar(profile.clone()));
            }
            FeatureOperation::Rib { construction, .. } => {
                profiles.extend(
                    construction
                        .profile
                        .as_ref()
                        .map(|profile| ProfileRef::Planar(profile.clone())),
                );
            }
            FeatureOperation::Revolve { construction, .. } => {
                profiles.extend(
                    construction
                        .profile()
                        .map(|profile| ProfileRef::Planar(profile.clone())),
                );
                paths.extend(construction.axis().and_then(|axis| axis.reference.as_ref()));
            }
            FeatureOperation::Sweep {
                shape,
                path,
                guide_rail,
                ..
            } => {
                profiles.extend(
                    shape
                        .referenced_profiles()
                        .into_iter()
                        .map(|profile| ProfileRef::Planar(profile.clone())),
                );
                paths.extend(path);
                if let Some(guide_rail) = guide_rail {
                    paths.push(&guide_rail.path);
                }
            }
            FeatureOperation::HelicalSweep { construction, .. } => {
                profiles.push(ProfileRef::Planar(construction.profile.clone()));
            }
            FeatureOperation::Loft {
                sections, guidance, ..
            } => {
                profiles.extend(sections.iter().filter_map(|section| match section {
                    crate::features::LoftSection::Profile(profile) => Some(profile.clone()),
                    crate::features::LoftSection::Point(_) => None,
                }));
                match guidance {
                    crate::features::LoftGuidance::Guides(guides) => paths.extend(guides),
                    crate::features::LoftGuidance::Centerline(centerline) => paths.push(centerline),
                }
            }
            FeatureOperation::Pattern { pattern, .. } => {
                collect_pattern_paths(pattern, &mut paths);
            }
            _ => {}
        }
        for profile in &profiles {
            let (sketch, sketch_kind, defined_sketches) = match profile {
                ProfileRef::SpatialSketchProfiles { sketch, .. }
                | ProfileRef::SpatialSketchSelection { sketch, .. } => {
                    (sketch.as_str(), "spatial sketch", &spatial_sketches)
                }
                ProfileRef::Planar(
                    PlanarProfileRef::Sketch(sketch)
                    | PlanarProfileRef::SketchProfiles { sketch, .. }
                    | PlanarProfileRef::SketchRegions { sketch, .. }
                    | PlanarProfileRef::SketchEntities { sketch, .. }
                    | PlanarProfileRef::SketchSelection { sketch, .. },
                ) => (sketch.as_str(), "sketch", sketches),
                ProfileRef::Planar(_) => continue,
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
                        .any(|index| cadmpeg_core::decode::index_from_u32(*index) >= profile_count)
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "spatial sketch profile indices are empty, repeated, or out of range",
                        );
                    }
                }
                ProfileRef::Planar(PlanarProfileRef::SketchProfiles { sketch, profiles }) => {
                    let sketch_profile_count = ir
                        .model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .map_or(0, |sketch| sketch.profiles.len());
                    if profiles.iter().any(|index| {
                        cadmpeg_core::decode::index_from_u32(*index) >= sketch_profile_count
                    }) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "sketch profile indices are empty, repeated, or out of range",
                        );
                    }
                }
                ProfileRef::Planar(PlanarProfileRef::SketchRegions { sketch, regions }) => {
                    let selected_sketch = ir
                        .model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch);
                    let sketch_profile_count =
                        selected_sketch.map_or(0, |sketch| sketch.profiles.len());
                    let invalid = regions.iter().any(|region| match region {
                        crate::features::SketchProfileRegion::Loops { loops } => {
                            cadmpeg_core::decode::index_from_u32(loops.outer())
                                >= sketch_profile_count
                                || loops.holes().iter().any(|index| {
                                    cadmpeg_core::decode::index_from_u32(*index)
                                        >= sketch_profile_count
                                })
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
                ProfileRef::Planar(PlanarProfileRef::SketchEntities { sketch, entities }) => {
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
                ProfileRef::Planar(
                    PlanarProfileRef::Native(_)
                    | PlanarProfileRef::Unresolved(_)
                    | PlanarProfileRef::Feature(_)
                    | PlanarProfileRef::Generated { .. }
                    | PlanarProfileRef::Sketch(_)
                    | PlanarProfileRef::SketchSelection { .. }
                    | PlanarProfileRef::HistoricalFaces { .. }
                    | PlanarProfileRef::Faces(_),
                )
                | ProfileRef::SpatialSketchSelection { .. } => {}
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

#[cfg(test)]
mod tests;
