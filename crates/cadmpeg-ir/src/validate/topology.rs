// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.

use super::scratch::Scratch;
use crate::index::identities::BorrowedIdentities;
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
mod composite;
pub(super) mod graphs;

fn collect_pattern_paths<'a>(
    ctx: &DecodeContext<'_>,
    pattern: &'a PatternKind,
    paths: &mut Scratch<'_, &'a crate::features::PathRef>,
) -> Result<(), CodecError> {
    match pattern.definition() {
        PatternTransform::CurveDriven {
            path: Some(path), ..
        } => paths.push(path)?,
        PatternTransform::Composite { stages } => {
            for stage in ctx.admit_iter(&stages[..], "composite pattern path scan")? {
                collect_stage_pattern_paths(&stage.pattern, paths)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A composite stage applies one transform, so its paths do not recurse.
fn collect_stage_pattern_paths<'a>(
    pattern: &'a crate::features::patterns::StagePatternKind,
    paths: &mut Scratch<'_, &'a crate::features::PathRef>,
) -> Result<(), CodecError> {
    if let PatternTransform::CurveDriven {
        path: Some(path), ..
    } = pattern.definition()
    {
        paths.push(path)?;
    }
    Ok(())
}
use super::sketches::locus_entity;
use crate::index::ModelIndex;
use crate::sketches::SketchConstraintDefinitionInput as Definition;

fn non_blank_native_reference(ctx: &DecodeContext<'_>, native: &str) -> Result<bool, CodecError> {
    ctx.charge_work(
        u64_from_index(native.len()),
        "native reference whitespace scan",
    )?;
    Ok(!native.trim().is_empty())
}

fn same_feature_owner(
    ctx: &DecodeContext<'_>,
    left: Option<&crate::features::FeatureId>,
    right: Option<&crate::features::FeatureId>,
) -> Result<bool, CodecError> {
    ctx.charge_work(1, "parameter owner gate")?;
    match (left, right) {
        (Some(left), Some(right)) => Ok(crate::ids::comparison::equal(
            ctx,
            left.as_str(),
            right.as_str(),
            "parameter owner comparison",
        )?),
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

fn ref_error(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    owner: &str,
    target_kind: impl fmt::Display,
    target: &str,
) -> Result<(), CodecError> {
    super::record_finding(
        ctx,
        findings,
        Check::ReferentialIntegrity,
        Severity::Error,
        Some(owner),
        format_args!("references missing {target_kind} `{target}`"),
    )
}

fn check_law_curves<R, V, P>(
    ctx: &DecodeContext<'_>,
    expression: &crate::geometry::LawExpression<R, V, P>,
    ids: &ModelIndex<'_>,
    procedural: &crate::geometry::ProceduralSurface,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("law reference depth")?;
    match expression {
        crate::geometry::LawExpression::Edge { curve, .. } => {
            if ids.curves(curve.id.as_str(), ctx)?.is_none() {
                ref_error(
                    ctx,
                    findings,
                    procedural.id.as_str(),
                    "curve",
                    curve.id.as_str(),
                )?;
            }
        }
        crate::geometry::LawExpression::Algebraic { operands, .. } => {
            for operand in operands {
                ctx.charge_work(1, "topology validation scan")?;
                check_law_curves(ctx, operand, ids, procedural, findings)?;
            }
        }
        _ => {}
    }

    Ok(())
}

pub(super) fn check_tolerances(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    ctx.charge_work(1, "document tolerance check")?;
    if ir.tolerances.linear.get() > 1.0e6 || ir.tolerances.angular.get() > std::f64::consts::TAU {
        super::record_finding(
            ctx,
            findings,
            Check::Tolerances,
            Severity::Warning,
            None,
            format_args!("document tolerance is outside a sane canonical range"),
        )?;
    }
    Ok(())
}

pub(super) fn check_topology_tolerances(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
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
            super::record_finding(
                ctx,
                findings,
                Check::Tolerances,
                Severity::Warning,
                Some(id),
                format_args!("topology tolerance is outside a sane canonical range"),
            )?;
        }
    }
    Ok(())
}

pub(super) fn check_references(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for b in &ir.model.bodies {
        ctx.charge_work(1, "topology validation scan")?;
        for l in &b.regions {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.regions(l.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, b.id.as_str(), "region", l.as_str())?;
            }
        }
    }
    for l in &ir.model.regions {
        ctx.charge_work(1, "topology validation scan")?;
        if ids.bodies(l.body.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, l.id.as_str(), "body", l.body.as_str())?;
        }
        for s in &l.shells {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.shells(s.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, l.id.as_str(), "shell", s.as_str())?;
            }
        }
    }
    for s in &ir.model.shells {
        ctx.charge_work(1, "topology validation scan")?;
        if ids.regions(s.region.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, s.id.as_str(), "region", s.region.as_str())?;
        }
        for f in s.faces() {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.faces(f.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, s.id.as_str(), "face", f.as_str())?;
            }
        }
        for e in s.wire_edges() {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.edges(e.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, s.id.as_str(), "wire edge", e.as_str())?;
            }
        }
        for v in s.free_vertices() {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.vertices(v.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, s.id.as_str(), "free vertex", v.as_str())?;
            }
        }
    }
    for f in &ir.model.faces {
        ctx.charge_work(1, "topology validation scan")?;
        if ids.shells(f.shell.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, f.id.as_str(), "shell", f.shell.as_str())?;
        }
        if ids.surfaces(f.surface.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, f.id.as_str(), "surface", f.surface.as_str())?;
        }
        let mut check_loop = |lp: &crate::ids::LoopId| -> Result<(), CodecError> {
            if ids.loops(lp.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, f.id.as_str(), "loop", lp.as_str())?;
            }
            Ok(())
        };
        match &f.loops {
            crate::topology::FaceLoops::Unspecified { loops } => {
                for lp in ctx.admit_iter(loops, "topology validation scan")? {
                    check_loop(lp)?;
                }
            }
            crate::topology::FaceLoops::Classified { outer, inner } => {
                for lp in ctx.admit_iter(std::slice::from_ref(outer), "topology validation scan")? {
                    check_loop(lp)?;
                }
                for lp in ctx.admit_iter(inner, "topology validation scan")? {
                    check_loop(lp)?;
                }
            }
        }
    }
    for lp in &ir.model.loops {
        ctx.charge_work(1, "topology validation scan")?;
        if ids.faces(lp.face.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, lp.id.as_str(), "face", lp.face.as_str())?;
        }
        match &lp.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, pcurves } => {
                if ids.vertices(vertex.as_str(), ctx)?.is_none() {
                    ref_error(ctx, findings, lp.id.as_str(), "vertex", vertex.as_str())?;
                }
                for pcurve in pcurves {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.pcurves(pcurve.pcurve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            lp.id.as_str(),
                            "pcurve(vertex use)",
                            pcurve.pcurve.as_str(),
                        )?;
                    }
                }
            }
            crate::topology::LoopBoundary::Ring(ring) => {
                for ce in ring.coedges() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.coedges(ce.as_str(), ctx)?.is_none() {
                        ref_error(ctx, findings, lp.id.as_str(), "coedge", ce.as_str())?;
                    }
                }
                for use_ in ring.vertex_uses() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.vertices(use_.vertex.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            lp.id.as_str(),
                            "vertex",
                            use_.vertex.as_str(),
                        )?;
                    }
                    let after = &use_.after;
                    if ids.coedges(after.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            lp.id.as_str(),
                            "coedge(vertex-use after)",
                            after.as_str(),
                        )?;
                    }
                    for pcurve in &use_.pcurves {
                        ctx.charge_work(1, "topology validation scan")?;
                        if ids.pcurves(pcurve.pcurve.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                lp.id.as_str(),
                                "pcurve(vertex use)",
                                pcurve.pcurve.as_str(),
                            )?;
                        }
                    }
                }
            }
        }
    }
    for ce in &ir.model.coedges {
        ctx.charge_work(1, "topology validation scan")?;
        if ids.loops(ce.owner_loop.as_str(), ctx)?.is_none() {
            ref_error(
                ctx,
                findings,
                ce.id.as_str(),
                "loop",
                ce.owner_loop.as_str(),
            )?;
        }
        if ids.edges(ce.edge.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, ce.id.as_str(), "edge", ce.edge.as_str())?;
        }
        if ids.coedges(ce.radial_next.as_str(), ctx)?.is_none() {
            ref_error(
                ctx,
                findings,
                ce.id.as_str(),
                "coedge(radial_next)",
                ce.radial_next.as_str(),
            )?;
        }
        for use_ in &ce.pcurves {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.pcurves(use_.pcurve.as_str(), ctx)?.is_none() {
                ref_error(
                    ctx,
                    findings,
                    ce.id.as_str(),
                    "pcurve",
                    use_.pcurve.as_str(),
                )?;
            }
        }
        if let Some(curve) = &ce.use_curve {
            if ids.curves(curve.curve.as_str(), ctx)?.is_none() {
                ref_error(
                    ctx,
                    findings,
                    ce.id.as_str(),
                    "coedge use curve",
                    curve.curve.as_str(),
                )?;
            }
        }
    }
    for e in &ir.model.edges {
        ctx.charge_work(1, "topology validation scan")?;
        if let Some(c) = e.curve() {
            if ids.curves(c.as_str(), ctx)?.is_none() {
                ref_error(ctx, findings, e.id.as_str(), "curve", c.as_str())?;
            }
        }
        if ids.vertices(e.start.as_str(), ctx)?.is_none() {
            ref_error(
                ctx,
                findings,
                e.id.as_str(),
                "vertex(start)",
                e.start.as_str(),
            )?;
        }
        if ids.vertices(e.end.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, e.id.as_str(), "vertex(end)", e.end.as_str())?;
        }
    }
    for v in &ir.model.vertices {
        ctx.charge_work(1, "topology validation scan")?;
        if ids.points(v.point.as_str(), ctx)?.is_none() {
            ref_error(ctx, findings, v.id.as_str(), "point", v.point.as_str())?;
        }
    }
    for binding in &ir.model.appearance_bindings {
        use crate::appearance::AppearanceTarget;

        ctx.charge_work(1, "topology validation scan")?;
        let mut owner_storage = ctx.reserve_scoped(0, "appearance binding finding identity")?;
        let owner = ctx.format_scoped_text(
            &mut owner_storage,
            format_args!("appearance-binding:{}", binding.appearance.as_str()),
            "appearance binding finding identity",
        )?;
        if ids.appearances(binding.appearance.as_str(), ctx)?.is_none() {
            ref_error(
                ctx,
                findings,
                &owner,
                "appearance",
                binding.appearance.as_str(),
            )?;
        }
        match &binding.target {
            AppearanceTarget::Body(body) if ids.bodies(body.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, &owner, "body", body.as_str())?;
            }
            AppearanceTarget::Face(face) if ids.faces(face.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, &owner, "face", face.as_str())?;
            }
            AppearanceTarget::Edge(edge) if ids.edges(edge.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, &owner, "edge", edge.as_str())?;
            }
            AppearanceTarget::Vertex(vertex) if ids.vertices(vertex.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, &owner, "vertex", vertex.as_str())?;
            }
            AppearanceTarget::Surface(surface)
                if ids.surfaces(surface.as_str(), ctx)?.is_none() =>
            {
                ref_error(ctx, findings, &owner, "surface", surface.as_str())?;
            }
            AppearanceTarget::Curve(curve) if ids.curves(curve.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, &owner, "curve", curve.as_str())?;
            }
            AppearanceTarget::Point(point) if ids.points(point.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, &owner, "point", point.as_str())?;
            }
            AppearanceTarget::Tessellation(tessellation)
                if ids.tessellations(tessellation, ctx)?.is_none() =>
            {
                ref_error(ctx, findings, &owner, "tessellation", tessellation)?;
            }
            AppearanceTarget::Source { .. } => {}
            _ => {}
        }
    }
    for attribute in &ir.model.attributes {
        use crate::attributes::AttributeTarget;

        ctx.charge_work(1, "topology validation scan")?;
        let owner = attribute.id.as_str();
        match &attribute.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if ids.bodies(id.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, owner, "body", id.as_str())?;
            }
            AttributeTarget::Face(id) if ids.faces(id.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, owner, "face", id.as_str())?;
            }
            AttributeTarget::Coedge(id) if ids.coedges(id.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, owner, "coedge", id.as_str())?;
            }
            AttributeTarget::Edge(id) if ids.edges(id.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, owner, "edge", id.as_str())?;
            }
            AttributeTarget::Vertex(id) if ids.vertices(id.as_str(), ctx)?.is_none() => {
                ref_error(ctx, findings, owner, "vertex", id.as_str())?;
            }
            _ => {}
        }
    }
    for s in &ir.model.surfaces {
        ctx.charge_work(1, "topology validation scan")?;
        match &s.geometry {
            SurfaceGeometry::Procedural { construction, .. } => {
                if ids
                    .procedural_surfaces(construction.as_str(), ctx)?
                    .is_none()
                {
                    ref_error(
                        ctx,
                        findings,
                        s.id.as_str(),
                        "procedural surface construction",
                        construction.as_str(),
                    )?;
                }
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: Some(u) })
                if !ids.contains(u.as_str(), ctx)? =>
            {
                ref_error(ctx, findings, s.id.as_str(), "unknown record", u.as_str())?;
            }
            SurfaceGeometry::Solved(_) => {}
        }
    }
    for curve in &ir.model.curves {
        ctx.charge_work(1, "topology validation scan")?;
        match &curve.geometry {
            CurveGeometry::Procedural { construction, .. } => {
                if ids.procedural_curves(construction.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        curve.id.as_str(),
                        "procedural curve construction",
                        construction.as_str(),
                    )?;
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(unknown),
            }) => {
                if !ids.contains(unknown.as_str(), ctx)? {
                    ref_error(
                        ctx,
                        findings,
                        curve.id.as_str(),
                        "unknown record",
                        unknown.as_str(),
                    )?;
                }
            }
            CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) => {
                for segment in ctx.admit_iter(&segments[..], "topology validation scan")? {
                    if ids.curves(segment.curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            curve.id.as_str(),
                            "curve",
                            segment.curve.as_str(),
                        )?;
                    }
                }
            }
            CurveGeometry::Solved(_) => {}
        }
    }
    composite::check(ctx, ir, findings)?;
    for procedural in &ir.model.procedural_surfaces {
        ctx.charge_work(1, "topology validation scan")?;
        match procedural.definition() {
            ProceduralSurfaceDefinition::Exact(..) => {}
            ProceduralSurfaceDefinition::Compound(definition_payload) => {
                let components = definition_payload.components();

                for component in components {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.surfaces(component.component.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            component.component.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::SubSurface(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Replica {
                source: support, ..
            } => {
                if ids.surfaces(support.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    )?;
                }
            }
            ProceduralSurfaceDefinition::Taper(definition_payload) => {
                let support = definition_payload.support();
                let reference = definition_payload.reference();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        )?;
                    }
                    if ids.curves(reference.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            reference.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Loft(definition_payload) => {
                let sections = definition_payload.sections();

                for section in ctx.admit_iter(sections, "loft section scan")? {
                    for entry in ctx.admit_iter(&section.entries, "topology validation scan")? {
                        for curve in entry
                            .path
                            .path
                            .iter()
                            .map(|curve| &curve.id)
                            .chain(entry.path.auxiliaries.iter())
                            .chain(entry.profile.iter().map(|member| &member.profile.id))
                        {
                            ctx.charge_work(1, "topology validation scan")?;
                            if ids.curves(curve.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.as_str(),
                                )?;
                            }
                        }
                        for member in &entry.profile {
                            ctx.charge_work(1, "topology validation scan")?;
                            if let Some(surface) = member.form.surface() {
                                if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                    ref_error(
                                        ctx,
                                        findings,
                                        procedural.id.as_str(),
                                        "surface",
                                        surface.as_str(),
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft(definition_payload) => {
                let construction = definition_payload.construction();

                let check_curve = |curve: &crate::ids::CurveId,
                                   findings: &mut Vec<Finding>|
                 -> Result<(), CodecError> {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                    Ok(())
                };
                let mut scales =
                    Scratch::filter_map(ctx, construction.scales.as_slice().iter(), |scale| {
                        Ok(Some(scale))
                    })?;
                match &construction.tail {
                    crate::geometry::CompoundLoftTail::Six { scale, curve, .. } => {
                        scales.push(scale.as_ref())?;
                        check_curve(curve, findings)?;
                    }
                    crate::geometry::CompoundLoftTail::Seven {
                        first_scale,
                        second_scale,
                        ..
                    } => {
                        scales.extend(first_scale.as_slice(), |scale| scale.as_ref())?;
                        scales.push(second_scale.as_ref())?;
                    }
                    crate::geometry::CompoundLoftTail::Zero { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
                            check_curve(curve, findings)?;
                        }
                    }
                }
                for scale in ctx
                    .admit_iter(&scales[..], "topology validation scan")?
                    .copied()
                {
                    check_curve(&scale.path, findings)?;
                    for curve in &scale.auxiliaries {
                        ctx.charge_work(1, "topology validation scan")?;
                        check_curve(curve, findings)?;
                    }
                    for member in &scale.members {
                        ctx.charge_work(1, "topology validation scan")?;
                        check_curve(&member.curve, findings)?;
                        let surface = &member.data.surface;
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) => {
                let construction = definition_payload.construction();

                let check_curve = |curve: &crate::ids::CurveId,
                                   findings: &mut Vec<Finding>|
                 -> Result<(), CodecError> {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                    Ok(())
                };
                let mut scales =
                    Scratch::filter_map(ctx, construction.scales.as_slice().iter(), |scale| {
                        Ok(Some(scale))
                    })?;
                match &construction.branch {
                    crate::geometry::ScaledCompoundLoftBranch::ExtendedVector {
                        first_scale,
                        second_scale,
                        ..
                    } => {
                        scales.extend(first_scale.as_slice(), |scale| scale.as_ref())?;
                        scales.push(second_scale.as_ref())?;
                    }
                    crate::geometry::ScaledCompoundLoftBranch::ExtendedCurve {
                        scale,
                        curve,
                        ..
                    } => {
                        scales.extend(scale.as_slice(), |scale| scale.as_ref())?;
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
                for scale in ctx
                    .admit_iter(&scales[..], "topology validation scan")?
                    .copied()
                {
                    check_curve(&scale.path, findings)?;
                    for curve in &scale.auxiliaries {
                        ctx.charge_work(1, "topology validation scan")?;
                        check_curve(curve, findings)?;
                    }
                    for member in &scale.members {
                        ctx.charge_work(1, "topology validation scan")?;
                        check_curve(&member.curve, findings)?;
                        let surface = &member.data.surface;
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Skin(definition_payload) => {
                let construction = definition_payload.construction();
                let check_curve = |curve: &crate::ids::CurveId,
                                   findings: &mut Vec<Finding>|
                 -> Result<(), CodecError> {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                    Ok(())
                };
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        check_curve(path, findings)?;
                        for profile in profiles {
                            ctx.charge_work(1, "topology validation scan")?;
                            check_curve(&profile.curve, findings)?;
                            let surface = &profile.data.surface;
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                )?;
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
                    ctx.charge_work(1, "topology validation scan")?;
                    check_law_curves(ctx, variable, ids, procedural, findings)?;
                }
            }
            ProceduralSurfaceDefinition::Law(definition_payload) => {
                let construction = definition_payload.construction();
                for formula in
                    std::iter::once(&construction.primary).chain(&construction.additional)
                {
                    ctx.charge_work(1, "topology validation scan")?;
                    for variable in formula.variables() {
                        ctx.charge_work(1, "topology validation scan")?;
                        check_law_curves(ctx, variable, ids, procedural, findings)?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Net(definition_payload) => {
                let construction = definition_payload.construction();
                for section in ctx.admit_iter(&construction.sections[..], "net section scan")? {
                    for entry in ctx.admit_iter(&section.entries, "topology validation scan")? {
                        for curve in entry
                            .path
                            .path
                            .iter()
                            .map(|curve| &curve.id)
                            .chain(entry.path.auxiliaries.iter())
                            .chain(entry.profile.iter().map(|member| &member.profile.id))
                        {
                            ctx.charge_work(1, "topology validation scan")?;
                            if ids.curves(curve.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.as_str(),
                                )?;
                            }
                        }
                        for member in &entry.profile {
                            ctx.charge_work(1, "topology validation scan")?;
                            if let Some(surface) = member.form.surface() {
                                if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                    ref_error(
                                        ctx,
                                        findings,
                                        procedural.id.as_str(),
                                        "surface",
                                        surface.as_str(),
                                    )?;
                                }
                            }
                        }
                    }
                }
                for formula in
                    ctx.admit_iter(&construction.formulas[..], "topology validation scan")?
                {
                    for variable in formula.variables() {
                        ctx.charge_work(1, "topology validation scan")?;
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
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        )?;
                    }
                }
                if let crate::geometry::G2BlendFirstShape::Full {
                    support: Some(support),
                } = &construction.first_shape
                {
                    if ids.surfaces(support.surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.surface.as_str(),
                        )?;
                    }
                }
                for curve in [
                    &construction.first.curve,
                    &construction.second.curve,
                    &construction.center_curve,
                ] {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
                let construction = definition_payload.construction();

                for side in &construction.sides {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.surface.as_str(),
                            )?;
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(curve.curve.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.curve.as_str(),
                            )?;
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
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                ctx.charge_work(
                    u64_from_index(construction.entries().len()),
                    "compound loft profile entry scan",
                )?;
                for member in construction.base_profile().iter().chain(
                    construction
                        .entries()
                        .iter()
                        .flat_map(|entry| &entry.profile),
                ) {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(member.profile.id.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            member.profile.id.as_str(),
                        )?;
                    }
                    if let Some(surface) = member.form.surface() {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
                ctx.charge_work(1, "compound loft base path scan")?;
                let base_path = construction.base_path();
                for curve in
                    ctx.admit_iter(base_path.path.as_slice(), "topology validation scan")?
                {
                    if ids.curves(curve.id.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.id.as_str(),
                        )?;
                    }
                }
                for curve in ctx.admit_iter(&base_path.auxiliaries, "topology validation scan")? {
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
                for entry in
                    ctx.admit_iter(construction.entries(), "compound loft path entry scan")?
                {
                    for curve in
                        ctx.admit_iter(entry.path.path.as_slice(), "topology validation scan")?
                    {
                        if ids.curves(curve.id.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.id.as_str(),
                            )?;
                        }
                    }
                    for curve in
                        ctx.admit_iter(&entry.path.auxiliaries, "topology validation scan")?
                    {
                        if ids.curves(curve.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.as_str(),
                            )?;
                        }
                    }
                }
                if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                    construction.direction()
                {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
                if let Some(curve) = construction.tail().curve() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.surface.as_str(),
                            )?;
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(curve.curve.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.curve.as_str(),
                            )?;
                        }
                    }
                }
                if ids.curves(construction.center().as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        construction.center().as_str(),
                    )?;
                }
            }
            ProceduralSurfaceDefinition::VertexBlend(definition_payload) => {
                let construction = definition_payload.construction();

                for boundary in &construction.boundaries {
                    ctx.charge_work(1, "topology validation scan")?;
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            if ids.curves(curve.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.as_str(),
                                )?;
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                )?;
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
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            directrix.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
                let directrix = definition_payload.directrix();
                if ids.curves(directrix.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        directrix.as_str(),
                    )?;
                }
            }
            ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                let directrix = definition_payload.directrix();
                {
                    if ids.curves(directrix.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            directrix.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
                let directrix = definition_payload.directrix();
                if ids.curves(directrix.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        directrix.as_str(),
                    )?;
                }
            }
            ProceduralSurfaceDefinition::Sweep(definition_payload) => {
                let profile = definition_payload.profile();
                let spine = definition_payload.spine();
                let native = definition_payload.native();
                for curve in [profile, spine] {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
                if let Some(native) = native {
                    let formulas = match &native.layout {
                        crate::geometry::SweepSurfaceLayout::ProfileFirst { formulas, .. } => {
                            Scratch::filter_map(ctx, formulas.iter(), |formula| Ok(Some(formula)))?
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitFormula {
                            formula, ..
                        } => Scratch::filter_map(ctx, [formula], |formula| Ok(Some(formula)))?,
                        crate::geometry::SweepSurfaceLayout::ExplicitGuide {
                            guide_curve, ..
                        } => {
                            if ids.curves(guide_curve.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    guide_curve.as_str(),
                                )?;
                            }
                            Scratch::new(ctx)?
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            if ids.surfaces(support_surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    support_surface.as_str(),
                                )?;
                            }
                            if let Some(curve) = auxiliary_curve {
                                if ids.curves(curve.as_str(), ctx)?.is_none() {
                                    ref_error(
                                        ctx,
                                        findings,
                                        procedural.id.as_str(),
                                        "curve",
                                        curve.as_str(),
                                    )?;
                                }
                            }
                            Scratch::new(ctx)?
                        }
                        crate::geometry::SweepSurfaceLayout::LawDriven {
                            first_law,
                            second_law,
                            formula,
                            ..
                        } => {
                            check_law_curves(ctx, first_law, ids, procedural, findings)?;
                            check_law_curves(ctx, second_law, ids, procedural, findings)?;
                            Scratch::filter_map(ctx, [formula], |formula| Ok(Some(formula)))?
                        }
                    };
                    for formula in ctx
                        .admit_iter(&formulas[..], "topology validation scan")?
                        .copied()
                    {
                        for variable in formula.variables() {
                            ctx.charge_work(1, "topology validation scan")?;
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
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Subset(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::ParallelOffset(definition_payload) => {
                let support = definition_payload.support();
                {
                    if ids.surfaces(support.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
                for curve in [first, second] {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Sum(definition_payload) => {
                for curve in [definition_payload.first(), definition_payload.second()] {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Blend(definition_payload) => {
                let supports = definition_payload.supports();
                let spine = definition_payload.spine();
                let native = definition_payload.native();

                ctx.charge_work(u64_from_index(supports.len()), "optional support scan")?;
                for support in supports.iter().flatten() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.surfaces(support.surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            support.surface.as_str(),
                        )?;
                    }
                }
                if let Some(spine) = spine {
                    if ids.curves(spine.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            spine.as_str(),
                        )?;
                    }
                }
                if let Some(native) = native {
                    let check_curve = |curve: &crate::ids::CurveId,
                                       findings: &mut Vec<Finding>|
                     -> Result<(), CodecError> {
                        if ids.curves(curve.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.as_str(),
                            )?;
                        }
                        Ok(())
                    };
                    let check_surface = |surface: &crate::ids::SurfaceId,
                                         findings: &mut Vec<Finding>|
                     -> Result<(), CodecError> {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                        Ok(())
                    };
                    check_curve(&native.slice, findings)?;
                    for side in &native.sides {
                        ctx.charge_work(1, "topology validation scan")?;
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
                if !ids.contains(record.as_str(), ctx)? {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "unknown record",
                        record.as_str(),
                    )?;
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
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        support.as_str(),
                    )?;
                }
                for boundary in boundaries {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(boundary.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            boundary.as_str(),
                        )?;
                    }
                }
                for pcurve in boundary_pcurves {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.pcurves(pcurve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "pcurve boundary",
                            pcurve.as_str(),
                        )?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Deformable(definition_payload) => {
                let construction = definition_payload.construction();

                if ids.surfaces(construction.support.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        construction.support.as_str(),
                    )?;
                }
                if let crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    surface, curve, ..
                }
                | crate::geometry::DeformableSurfaceData::Full { surface, curve, .. } =
                    &construction.data
                {
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        )?;
                    }
                    if ids.curves(curve.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            curve.as_str(),
                        )?;
                    }
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        ctx.charge_work(1, "topology validation scan")?;
        match procedural.definition() {
            ProceduralCurveDefinition::Exact { .. } | ProceduralCurveDefinition::Helix(_) => {}
            ProceduralCurveDefinition::Law {
                context,
                primary,
                additional,
                ..
            } => {
                fn check<R, V, P>(
                    ctx: &DecodeContext<'_>,
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
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "curve",
                                    curve.id.as_str(),
                                )?;
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                ctx.charge_work(1, "topology validation scan")?;
                                check(ctx, operand, ids, procedural, findings)?;
                            }
                        }
                        _ => {}
                    }

                    Ok(())
                }
                for side in context.sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    ctx.charge_work(1, "topology validation scan")?;
                    for variable in formula.formula().variables() {
                        ctx.charge_work(1, "topology validation scan")?;
                        check(ctx, variable, ids, procedural, findings)?;
                    }
                }
            }
            ProceduralCurveDefinition::Compound(compound) => {
                let components = compound.components();

                for component in components {
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.curves(component.component.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            component.component.as_str(),
                        )?;
                    }
                }
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in context.sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
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
                    ctx.charge_work(1, "topology validation scan")?;
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        )?;
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) => {
                let context = definition_payload.context();
                let third = definition_payload.third();

                for side in context.sides().iter().chain(std::iter::once(third)) {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { family } => {
                for side in family.context().sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette(definition_payload) => {
                let context = definition_payload.context();
                let cast_surface = definition_payload.cast_surface();
                if ids.surfaces(cast_surface.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "surface",
                        cast_surface.as_str(),
                    )?;
                }
                for side in context.sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset(definition_payload) => {
                let context = definition_payload.context();
                let base = definition_payload.base();
                {
                    if ids.curves(base.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            base.as_str(),
                        )?;
                    }
                    for side in context.sides() {
                        ctx.charge_work(1, "topology validation scan")?;
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                )?;
                            }
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Spring(definition_payload) => {
                for side in definition_payload.support_context().sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
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
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                curve.as_str(),
                            )?;
                        }
                    }
                    for side in context.sides() {
                        ctx.charge_work(1, "topology validation scan")?;
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                )?;
                            }
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Projection(definition_payload) => {
                let context = definition_payload.context();
                let source = definition_payload.source();

                if ids.curves(source.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        source.as_str(),
                    )?;
                }
                for side in context.sides() {
                    ctx.charge_work(1, "topology validation scan")?;
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                surface.as_str(),
                            )?;
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
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            source.as_str(),
                        )?;
                    }
                    if let crate::geometry::OffsetSide::Direction {
                        support: Some(support),
                        ..
                    } = side
                    {
                        if ids.surfaces(support.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "surface",
                                support.as_str(),
                            )?;
                        }
                    }
                    if let Some(crate::geometry::CurveOffsetRange::Variable {
                        distance_law:
                            crate::geometry::CurveOffsetDistanceLaw::Coordinate { function, .. },
                        ..
                    }) = range
                    {
                        if ids.curves(function.as_str(), ctx)?.is_none() {
                            ref_error(
                                ctx,
                                findings,
                                procedural.id.as_str(),
                                "curve",
                                function.as_str(),
                            )?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SpatialOffset(definition_payload) => {
                let source = definition_payload.source();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            source.as_str(),
                        )?;
                    }
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset(definition_payload) => {
                let context = definition_payload.context();
                {
                    for side in context.sides() {
                        ctx.charge_work(1, "topology validation scan")?;
                        if let Some(surface) = &side.surface {
                            if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                                ref_error(
                                    ctx,
                                    findings,
                                    procedural.id.as_str(),
                                    "surface",
                                    surface.as_str(),
                                )?;
                            }
                        }
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset(definition_payload) => {
                let source = definition_payload.source();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            source.as_str(),
                        )?;
                    }
                }
            }
            ProceduralCurveDefinition::Replica { source, .. } => {
                if ids.curves(source.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "curve",
                        source.as_str(),
                    )?;
                }
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                {
                    if ids.curves(source.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "curve",
                            source.as_str(),
                        )?;
                    }
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    if ids.surfaces(surface.as_str(), ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            procedural.id.as_str(),
                            "surface",
                            surface.as_str(),
                        )?;
                    }
                }
            }
            ProceduralCurveDefinition::Unknown {
                native_kind: _,
                record: Some(record),
                ..
            } => {
                if !ids.contains(record.as_str(), ctx)? {
                    ref_error(
                        ctx,
                        findings,
                        procedural.id.as_str(),
                        "unknown record",
                        record.as_str(),
                    )?;
                }
            }
            ProceduralCurveDefinition::Unknown {
                native_kind: _,
                record: None,
                ..
            } => {}
        }
    }
    let features = BorrowedIdentities::build(ctx, |add| {
        for feature in ctx.admit_iter(&ir.model.features, "feature identity scan")? {
            add(feature.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let feature_ordinals = BorrowedIdentities::build(ctx, |add| {
        for feature in ctx.admit_iter(&ir.model.features, "feature ordinal scan")? {
            let (identity, value) = (feature.id.as_str(), feature.ordinal);
            add(identity, value)?;
        }
        Ok(())
    })?;
    let parameters = BorrowedIdentities::build(ctx, |add| {
        for parameter in ctx.admit_iter(&ir.model.parameters, "parameter ordinal scan")? {
            let (identity, value) = (
                parameter.id.as_str(),
                (parameter.owner.as_ref(), parameter.ordinal),
            );
            add(identity, value)?;
        }
        Ok(())
    })?;
    let mut scoped_parameters = Scratch::new(ctx)?;
    for parameter in &ir.model.parameters {
        ctx.charge_work(1, "topology validation scan")?;
        if let Some(owner) = &parameter.owner {
            if !features.contains(ctx, owner.as_str())? {
                ref_error(
                    ctx,
                    findings,
                    parameter.id.as_str(),
                    "feature",
                    owner.as_str(),
                )?;
            }
        }
        if super::scans::any(
            ctx,
            scoped_parameters.iter(),
            |previous: &&crate::features::DesignParameter| {
                Ok(
                    same_feature_owner(ctx, previous.owner.as_ref(), parameter.owner.as_ref())?
                        && crate::ids::comparison::equal(
                            ctx,
                            previous.name.as_str(),
                            parameter.name.as_str(),
                            "parameter name comparison",
                        )?,
                )
            },
        )? {
            super::record_finding(
                ctx,
                findings,
                Check::Counts,
                Severity::Error,
                Some(parameter.id.as_str()),
                format_args!(
                    "parameter scope {:?} repeats parameter name `{}`",
                    parameter.owner, parameter.name
                ),
            )?;
        }
        if super::scans::any(
            ctx,
            scoped_parameters.iter(),
            |previous: &&crate::features::DesignParameter| {
                Ok(
                    same_feature_owner(ctx, previous.owner.as_ref(), parameter.owner.as_ref())?
                        && previous.ordinal == parameter.ordinal,
                )
            },
        )? {
            super::record_finding(
                ctx,
                findings,
                Check::Counts,
                Severity::Error,
                Some(parameter.id.as_str()),
                format_args!(
                    "parameter scope {:?} repeats parameter ordinal {}",
                    parameter.owner, parameter.ordinal
                ),
            )?;
        }
        for dependency in ctx.admit_iter(
            parameter.dependencies.as_slice(),
            "topology validation scan",
        )? {
            let Some((owner, ordinal)) = parameters.get(ctx, dependency.as_str())? else {
                ref_error(
                    ctx,
                    findings,
                    parameter.id.as_str(),
                    "parameter dependency",
                    dependency.as_str(),
                )?;
                continue;
            };
            let precedes = if same_feature_owner(ctx, *owner, parameter.owner.as_ref())? {
                *ordinal < parameter.ordinal
            } else {
                match (*owner, parameter.owner.as_ref()) {
                    (None, Some(_)) => true,
                    (Some(dependency_owner), Some(parameter_owner)) => feature_ordinals
                        .get(ctx, dependency_owner.as_str())?
                        .zip(feature_ordinals.get(ctx, parameter_owner.as_str())?)
                        .is_some_and(|(dependency_owner, parameter_owner)| {
                            dependency_owner < parameter_owner
                        }),
                    (Some(_) | None, None) => false,
                }
            };
            if !precedes {
                super::record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(parameter.id.as_str()),
                    format_args!(
                        "parameter dependency `{}` does not precede its consumer",
                        dependency.as_str()
                    ),
                )?;
            }
        }
        scoped_parameters.push(parameter)?;
    }
    let sketches = BorrowedIdentities::build(ctx, |add| {
        for sketch in ctx.admit_iter(&ir.model.sketches, "sketch identity scan")? {
            add(sketch.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let sketch_entities = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(&ir.model.sketch_entities, "sketch entity identity scan")? {
            add(entity.id().as_str(), ())?;
        }
        Ok(())
    })?;
    let sketch_entity_owners = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(&ir.model.sketch_entities, "sketch entity owner scan")? {
            let (identity, value) = (entity.id().as_str(), entity.sketch.as_str());
            add(identity, value)?;
        }
        Ok(())
    })?;
    let parameters = BorrowedIdentities::build(ctx, |add| {
        for parameter in ctx.admit_iter(&ir.model.parameters, "parameter identity scan")? {
            add(parameter.id.as_str(), ())?;
        }
        Ok(())
    })?;
    for sketch in &ir.model.sketches {
        ctx.charge_work(1, "topology validation scan")?;
        for profile in ctx.admit_iter(&sketch.profiles[..], "sketch profile group scan")? {
            for entity_use in ctx.admit_iter(profile, "topology validation scan")? {
                if !sketch_entities.contains(ctx, entity_use.entity.as_str())? {
                    ref_error(
                        ctx,
                        findings,
                        sketch.id.as_str(),
                        "sketch entity",
                        entity_use.entity.as_str(),
                    )?;
                }
            }
        }
    }
    for entity in &ir.model.sketch_entities {
        ctx.charge_work(1, "topology validation scan")?;
        if !sketches.contains(ctx, entity.sketch.as_str())? {
            ref_error(
                ctx,
                findings,
                entity.id().as_str(),
                "sketch",
                entity.sketch.as_str(),
            )?;
        }
    }
    for constraint in &ir.model.sketch_constraints {
        ctx.charge_work(1, "topology validation scan")?;
        if !sketches.contains(ctx, constraint.sketch.as_str())? {
            ref_error(
                ctx,
                findings,
                constraint.id.as_str(),
                "sketch",
                constraint.sketch.as_str(),
            )?;
        }
        let mut constraint_entities = Scratch::new(ctx)?;
        let parameter = match constraint.definition.kind() {
            Definition::Disabled {} => None,
            Definition::Polygon { polygon } => {
                constraint_entities.extend(polygon.entities(), |entity| entity)?;
                None
            }
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
            } => {
                constraint_entities.extend(entities, |entity| entity)?;
                None
            }
            Definition::RectangularPattern { pattern } => {
                for row in ctx.admit_iter(pattern.rows(), "constraint pattern row scan")? {
                    for instance in ctx.admit_iter(row, "constraint pattern instance scan")? {
                        constraint_entities.extend(&instance.entities, |entity| entity)?;
                    }
                }
                None
            }
            Definition::CircularPattern { pattern } => {
                constraint_entities.extend(&[pattern.center()], |entity| *entity)?;
                for instance in
                    ctx.admit_iter(pattern.instances(), "constraint circular instance scan")?
                {
                    constraint_entities.extend(&instance.entities, |entity| entity)?;
                }
                None
            }
            Definition::TextFrame { text, frame } => {
                constraint_entities.extend(&[text], |value| *value)?;
                constraint_entities.extend(frame.as_slice(), |value| value)?;
                None
            }
            Definition::TextPath { text, path, .. } => {
                constraint_entities.extend(&[text, path], |value| *value)?;
                None
            }
            Definition::Native {
                entities,
                parameter: Some(parameter),
                ..
            } => {
                constraint_entities.extend(entities, |entity| entity)?;
                Some(parameter.as_str())
            }
            Definition::Horizontal { entity }
            | Definition::Vertical { entity }
            | Definition::Fixed { entity }
            | Definition::ArcAngle { entity, .. }
            | Definition::EllipseAngle { entity, .. } => {
                constraint_entities.extend(&[entity], |value| *value)?;
                None
            }
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
            } => {
                constraint_entities.extend(&[first, second], |entity| *entity)?;
                None
            }
            Definition::InternalAlignment { helper, parent, .. } => {
                constraint_entities.extend(&[helper, parent], |value| *value)?;
                None
            }
            Definition::Group { elements } | Definition::Text { elements, .. } => {
                constraint_entities.extend(elements, |element| locus_entity(element))?;
                None
            }
            Definition::CoincidentLoci { loci } => {
                constraint_entities.extend(loci, |locus| locus_entity(locus))?;
                None
            }
            Definition::SameCoordinate { relation } => {
                constraint_entities.extend(
                    &[
                        locus_entity(relation.first()),
                        locus_entity(relation.second()),
                    ],
                    |entity| *entity,
                )?;
                None
            }
            Definition::TangentLoci { first, second } => {
                constraint_entities
                    .extend(&[locus_entity(first), locus_entity(second)], |entity| {
                        *entity
                    })?;
                None
            }
            Definition::PointSymmetric {
                first,
                second,
                center,
            } => {
                constraint_entities.extend(
                    &[
                        locus_entity(first),
                        locus_entity(second),
                        locus_entity(center),
                    ],
                    |entity| *entity,
                )?;
                None
            }
            Definition::Midpoint { point, entity } => {
                constraint_entities.extend(&[locus_entity(point), entity], |entity| *entity)?;
                None
            }
            Definition::PointCoordinateValues { point, .. } => {
                constraint_entities.extend(&[locus_entity(point)], |entity| *entity)?;
                None
            }
            Definition::MidpointCoordinate { first, second, .. } => {
                constraint_entities
                    .extend(&[locus_entity(first), locus_entity(second)], |entity| {
                        *entity
                    })?;
                None
            }
            Definition::AtIntersection {
                point,
                first,
                second,
            } => {
                constraint_entities
                    .extend(&[locus_entity(point), first, second], |entity| *entity)?;
                None
            }
            Definition::Offset {
                pairs, parameter, ..
            } => {
                for pair in ctx.admit_iter(pairs, "constraint offset pair scan")? {
                    constraint_entities.extend(&[&pair.source, &pair.result], |entity| *entity)?;
                }
                parameter.as_ref().map(|parameter| parameter.id.as_str())
            }
            Definition::PointOnObject { point, entity } => {
                constraint_entities.extend(&[locus_entity(point), entity], |entity| *entity)?;
                None
            }
            Definition::Symmetric {
                first,
                second,
                axis,
            } => {
                constraint_entities.extend(
                    &[locus_entity(first), locus_entity(second), axis],
                    |entity| *entity,
                )?;
                None
            }
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
            } => {
                constraint_entities
                    .extend(&[locus_entity(first), locus_entity(second)], |entity| {
                        *entity
                    })?;
                Some(parameter.as_str())
            }
            Definition::PolarDistance {
                first,
                second,
                distance_parameter: None,
                ..
            } => {
                constraint_entities
                    .extend(&[locus_entity(first), locus_entity(second)], |entity| {
                        *entity
                    })?;
                None
            }
            Definition::DistanceLociValue {
                first,
                second,
                parameter: None,
                ..
            } => {
                constraint_entities
                    .extend(&[locus_entity(first), locus_entity(second)], |entity| {
                        *entity
                    })?;
                None
            }
            Definition::AngleDifference { .. } => None,
            Definition::ScalarEquality { .. } => None,
            Definition::EqualDistance { first, second } => {
                constraint_entities.extend(
                    &[
                        locus_entity(&first.first),
                        locus_entity(&first.second),
                        locus_entity(&second.first),
                        locus_entity(&second.second),
                    ],
                    |entity| *entity,
                )?;
                None
            }
            Definition::RepeatedDistance {
                measurements,
                parameter,
            } => {
                for measurement in ctx.admit_iter(measurements, "constraint measurement scan")? {
                    use crate::sketches::SketchDistanceMeasurement as Measurement;
                    let (first, second) = match measurement {
                        Measurement::Distance { first, second }
                        | Measurement::Horizontal { first, second }
                        | Measurement::Vertical { first, second } => (first, second),
                    };
                    constraint_entities
                        .extend(&[locus_entity(first), locus_entity(second)], |entity| {
                            *entity
                        })?;
                }
                Some(parameter.as_str())
            }
            Definition::RepeatedLength {
                entities,
                parameter,
            } => {
                constraint_entities.extend(entities, |entity| entity)?;
                Some(parameter.as_str())
            }
            Definition::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => {
                constraint_entities.extend(first.as_slice(), |entity| entity)?;
                constraint_entities.extend(second.as_slice(), |entity| entity)?;
                Some(parameter.as_str())
            }
            Definition::Angle {
                first,
                second,
                parameter,
            } => {
                constraint_entities.extend(&[first, second], |entity| *entity)?;
                Some(parameter.as_str())
            }
            Definition::AngleToAxis {
                entity, parameter, ..
            } => {
                constraint_entities.extend(&[entity], |value| *value)?;
                Some(parameter.as_str())
            }
            Definition::RepeatedRadius {
                entities,
                parameter,
            }
            | Definition::RepeatedDiameter {
                entities,
                parameter,
            } => {
                constraint_entities.extend(entities, |entity| entity)?;
                Some(parameter.as_str())
            }
            Definition::Radius { entity, parameter }
            | Definition::Diameter { entity, parameter }
            | Definition::Weight { entity, parameter } => {
                constraint_entities.extend(&[entity], |value| *value)?;
                Some(parameter.as_str())
            }
            Definition::SnellsLaw {
                incident,
                refracted,
                interface,
                parameter,
            } => {
                constraint_entities.extend(
                    &[locus_entity(incident), locus_entity(refracted), interface],
                    |entity| *entity,
                )?;
                Some(parameter.as_str())
            }
        };
        let parameter = parameter.or(match constraint.definition.kind() {
            Definition::Distance { parameter, .. } => Some(parameter.as_str()),
            _ => None,
        });
        for entity in ctx
            .admit_iter(&constraint_entities[..], "topology validation scan")?
            .copied()
        {
            if !sketch_entities.contains(ctx, entity.as_str())? {
                ref_error(
                    ctx,
                    findings,
                    constraint.id.as_str(),
                    "sketch entity",
                    entity.as_str(),
                )?;
            } else if match sketch_entity_owners.get(ctx, entity.as_str())? {
                Some(owner) => !crate::ids::comparison::equal(
                    ctx,
                    owner,
                    constraint.sketch.as_str(),
                    "constraint sketch ownership",
                )?,
                None => true,
            } {
                super::record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(constraint.id.as_str()),
                    format_args!(
                        "sketch entity `{}` belongs to a different sketch",
                        entity.as_str()
                    ),
                )?;
            }
        }
        drop(constraint_entities);
        if let Some(parameter) = parameter {
            if !parameters.contains(ctx, parameter)? {
                ref_error(
                    ctx,
                    findings,
                    constraint.id.as_str(),
                    "parameter",
                    parameter,
                )?;
            }
        }
        if let Definition::RectangularPattern { pattern } = constraint.definition.kind() {
            for direction in
                ctx.admit_iter(pattern.directions(), "constraint pattern direction scan")?
            {
                for parameter in [
                    direction
                        .distance
                        .as_ref()
                        .map(crate::sketches::SketchPatternDistance::parameter),
                    direction.count_parameter.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    ctx.charge_work(1, "topology validation scan")?;
                    if !parameters.contains(ctx, parameter.as_str())? {
                        ref_error(
                            ctx,
                            findings,
                            constraint.id.as_str(),
                            "parameter",
                            parameter.as_str(),
                        )?;
                    }
                }
            }
        }
        if let Definition::CircularPattern { pattern } = constraint.definition.kind() {
            for parameter in [pattern.angle_parameter(), pattern.count_parameter()]
                .into_iter()
                .flatten()
            {
                ctx.charge_work(1, "topology validation scan")?;
                if !parameters.contains(ctx, parameter.as_str())? {
                    ref_error(
                        ctx,
                        findings,
                        constraint.id.as_str(),
                        "parameter",
                        parameter.as_str(),
                    )?;
                }
            }
        }
    }
    check_feature_sketch_references(ctx, ir, &sketches, findings)?;
    check_feature_references(ctx, ir, ids, findings)?;
    Ok(())
}

fn check_feature_references(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    use crate::features::{
        EdgeSelection, FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef, ScaleCenter,
    };

    if let Err(error) = crate::document::feature_parents::validate(ctx, &[&ir.model])? {
        super::record_finding(
            ctx,
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            Some(error.owner().as_str()),
            format_args!("{error}"),
        )?;
    }

    let mut configuration_ordinals = super::orders::Orders::new(ctx)?;
    let mut configuration_source_indices = super::orders::Orders::new(ctx)?;
    let mut active_configurations = 0;
    let parameter_ids = BorrowedIdentities::build(ctx, |add| {
        for parameter in ctx.admit_iter(&ir.model.parameters, "parameter identity scan")? {
            add(parameter.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let asset_ids = BorrowedIdentities::build(ctx, |add| {
        for asset in ctx.admit_iter(&ir.model.assets, "asset identity scan")? {
            add(asset.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let parameter_values = BorrowedIdentities::build(ctx, |add| {
        for parameter in ctx.admit_iter(&ir.model.parameters, "parameter value index scan")? {
            let (identity, value) = (parameter.id.as_str(), parameter.value.as_ref());
            add(identity, value)?;
        }
        Ok(())
    })?;
    let features = BorrowedIdentities::build(ctx, |add| {
        for feature in ctx.admit_iter(&ir.model.features, "feature ordinal scan")? {
            let (identity, value) = (feature.id.as_str(), feature.ordinal);
            add(identity, value)?;
        }
        Ok(())
    })?;
    for configuration in &ir.model.configurations {
        ctx.charge_work(1, "topology validation scan")?;
        active_configurations += usize::from(configuration.active);
        if !configuration_ordinals.insert(configuration.ordinal)? {
            super::record_finding(
                ctx,
                findings,
                Check::Counts,
                Severity::Error,
                Some(configuration.id.as_str()),
                format_args!(
                    "design repeats configuration ordinal {}",
                    configuration.ordinal
                ),
            )?;
        }
        if let Some(source_index) = configuration.source_index {
            if !configuration_source_indices.insert(source_index)? {
                super::record_finding(
                    ctx,
                    findings,
                    Check::Counts,
                    Severity::Error,
                    Some(configuration.id.as_str()),
                    format_args!("design repeats configuration source index {source_index}"),
                )?;
            }
        }
        if let Some(bodies) = &configuration.bodies {
            for body in ctx.admit_iter(bodies.as_slice(), "topology validation scan")? {
                if ids.bodies(body.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        configuration.id.as_str(),
                        "configuration body",
                        body.as_str(),
                    )?;
                }
            }
        }
        for parameter in configuration.parameter_overrides.keys() {
            ctx.charge_work(1, "topology validation scan")?;
            if !parameter_ids.contains(ctx, parameter.as_str())? {
                ref_error(
                    ctx,
                    findings,
                    configuration.id.as_str(),
                    "configuration parameter override",
                    parameter.as_str(),
                )?;
            }
        }
        let suppressed_features = BorrowedIdentities::build(ctx, |add| {
            for (feature, state) in &configuration.feature_states {
                ctx.charge_work(1, "configuration suppression scan")?;
                if state.evaluation.is_suppressed() {
                    add(feature.as_str(), ())?;
                }
            }
            Ok(())
        })?;
        if configuration.active {
            for feature in &ir.model.features {
                ctx.charge_work(1, "topology validation scan")?;
                if feature
                    .suppressed
                    .map_or(Ok::<_, CodecError>(false), |suppressed| {
                        Ok::<_, CodecError>({
                            suppressed_features.contains(ctx, feature.id.as_str())? != suppressed
                        })
                    })?
                {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(configuration.id.as_str()),
                        format_args!(
                            "active configuration suppression disagrees with current feature state"
                        ),
                    )?;
                }
            }
        }
        for (parameter, value) in &configuration.parameter_values {
            ctx.charge_work(1, "topology validation scan")?;
            match parameter_values.get(ctx, parameter.as_str())? {
                None => ref_error(
                    ctx,
                    findings,
                    configuration.id.as_str(),
                    "configuration parameter value",
                    parameter.as_str(),
                )?,
                Some(baseline) => {
                    let kind = |value: &crate::features::ParameterValue| -> u8 {
                        match value {
                            crate::features::ParameterValue::Length(_) => 0,
                            crate::features::ParameterValue::Angle(_) => 1,
                            crate::features::ParameterValue::Real(_) => 2,
                            crate::features::ParameterValue::Integer(_) => 3,
                            crate::features::ParameterValue::Boolean(_) => 4,
                            crate::features::ParameterValue::String(_) => 5,
                        }
                    };
                    if let Some(baseline) = baseline {
                        if !ctx.equal(
                            &kind(baseline),
                            &kind(value),
                            "configuration parameter value kind",
                        )? {
                            geometry_error(
                                ctx,
                                findings,
                                configuration.id.as_str(),
                                "configuration parameter value is invalid",
                            )?;
                        }
                    }
                }
            }
        }
        for (feature, state) in &configuration.feature_states {
            ctx.charge_work(1, "topology validation scan")?;
            let feature_ordinal = features.get(ctx, feature.as_str())?.copied();
            if feature_ordinal.is_none() {
                ref_error(
                    ctx,
                    findings,
                    configuration.id.as_str(),
                    "configuration feature state",
                    feature.as_str(),
                )?;
            }
            for dependency in
                ctx.admit_iter(state.dependencies.as_slice(), "topology validation scan")?
            {
                match features.get(ctx, dependency.as_str())? {
                    None => ref_error(
                        ctx,
                        findings,
                        configuration.id.as_str(),
                        "configuration feature dependency",
                        dependency.as_str(),
                    )?,
                    Some(dependency_ordinal)
                        if feature_ordinal.is_some_and(|feature_ordinal| {
                            *dependency_ordinal >= feature_ordinal
                        }) =>
                    {
                        super::record_finding(
                            ctx,
                            findings,
                            Check::ReferentialIntegrity,
                            Severity::Error,
                            Some(configuration.id.as_str()),
                            format_args!(
                                "configuration feature dependency `{}` does not precede `{}`",
                                dependency.as_str(),
                                feature.as_str()
                            ),
                        )?;
                    }
                    Some(_) => {}
                }
            }
            let references = regeneration_references(ctx, state.definition.operation())?;
            for reference in ctx
                .admit_iter(&references[..], "topology validation scan")?
                .copied()
            {
                match features.get(ctx, reference.as_str())? {
                    None => ref_error(
                        ctx,
                        findings,
                        configuration.id.as_str(),
                        "configuration definition feature",
                        reference.as_str(),
                    )?,
                    Some(reference_ordinal)
                        if feature_ordinal.is_some_and(|feature_ordinal| {
                            *reference_ordinal >= feature_ordinal
                        }) =>
                    {
                        super::record_finding(
                            ctx,
                            findings,
                            Check::ReferentialIntegrity,
                            Severity::Error,
                            Some(configuration.id.as_str()),
                            format_args!(
                                "configuration definition feature `{}` does not precede `{}`",
                                reference.as_str(),
                                feature.as_str()
                            ),
                        )?;
                    }
                    Some(_)
                        if !super::scans::any(ctx, state.dependencies.iter(), |dependency| {
                            Ok(crate::ids::comparison::equal(
                                ctx,
                                dependency.as_str(),
                                reference.as_str(),
                                "feature dependency comparison",
                            )?)
                        })? =>
                    {
                        super::record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(configuration.id.as_str()), format_args!(
                                "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
                                feature.as_str(), reference.as_str()
                            ))?;
                    }
                    Some(_) => {}
                }
            }
            drop(references);
            for output in state.evaluation.outputs() {
                ctx.charge_work(1, "topology validation scan")?;
                if ids.bodies(output.as_str(), ctx)?.is_none() {
                    ref_error(
                        ctx,
                        findings,
                        configuration.id.as_str(),
                        "configuration feature output",
                        output.as_str(),
                    )?;
                }
            }
        }
        check_configuration_state_closure(ctx, configuration, findings)?;
    }
    if active_configurations > 1 {
        super::record_finding(
            ctx,
            findings,
            Check::Counts,
            Severity::Error,
            None,
            format_args!("design has multiple active configurations"),
        )?;
    }
    let feature_records = BorrowedIdentities::build(ctx, |add| {
        for feature in ctx.admit_iter(&ir.model.features, "feature record scan")? {
            add(feature.id.as_str(), feature)?;
        }
        Ok(())
    })?;
    let sketch_entities = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(&ir.model.sketch_entities, "sketch entity identity scan")? {
            add(entity.id().as_str(), ())?;
        }
        Ok(())
    })?;
    let spatial_sketch_entity_owners = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(
            &ir.model.spatial_sketch_entities,
            "spatial sketch entity owner scan",
        )? {
            let (identity, value) = (entity.id().as_str(), entity.sketch.as_str());
            add(identity, value)?;
        }
        Ok(())
    })?;
    let mut reported_plane_cycles = Scratch::new(ctx)?;
    for feature in &ir.model.features {
        ctx.charge_work(1, "topology validation scan")?;
        let mut path_storage = ctx.reserve_scoped(0, "datum-plane traversal storage")?;
        let mut path: Vec<(&str, cadmpeg_core::decode::DepthGuard<'_>)> = Vec::new();
        let mut positions = BorrowedIdentities::build(ctx, |_| Ok(()))?;
        let mut cursor = feature.id.as_str();
        loop {
            ctx.charge_work(1, "datum-plane traversal")?;

            if let Some(&cycle_start) = positions.get(ctx, cursor)? {
                let mut cycle_storage =
                    ctx.with_scoped_storage("datum-plane cycle identities", || {
                        ctx.collect_vec(
                            path[cycle_start..].iter().map(|(id, _)| *id),
                            "datum-plane cycle identities",
                        )
                    })?;
                let cycle = &mut cycle_storage.0;
                ctx.sort_unstable_by(
                    cycle,
                    |id| *id,
                    Ord::cmp,
                    "sort datum-plane cycle identities",
                )?;
                if !super::scans::any(
                    ctx,
                    reported_plane_cycles.iter(),
                    |previous: &Scratch<'_, &str>| {
                        if previous.len() != cycle.len() {
                            return Ok(false);
                        }
                        super::scans::all(
                            ctx,
                            previous.iter().zip(cycle.iter()),
                            |(left, right)| {
                                Ok(crate::ids::comparison::equal(
                                    ctx,
                                    left,
                                    right,
                                    "datum-plane cycle comparison",
                                )?)
                            },
                        )
                    },
                )? {
                    let finding = Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: ctx.format_retained(
                            format_args!(
                                "datum-plane reference cycle contains {}",
                                PlaneCyclePath(cycle)
                            ),
                            "datum-plane cycle finding",
                        )?,
                        entity: Some(ctx.copy_retained_text(
                            feature.id.as_str(),
                            "datum-plane cycle finding identity",
                        )?),
                    };
                    ctx.push_vec(findings, finding, "datum-plane cycle findings")?;
                    reported_plane_cycles.push(Scratch::filter_map(
                        ctx,
                        cycle.iter().copied(),
                        |identity| Ok(Some(identity)),
                    )?)?;
                }
                break;
            }

            positions.insert(cursor, path.len())?;
            let depth = ctx.enter_nested("datum-plane traversal depth")?;
            ctx.push_scoped_vec(
                &mut path_storage,
                &mut path,
                (cursor, depth),
                "datum-plane traversal path",
            )?;

            let Some(next) = feature_records.get(ctx, cursor)?.and_then(|feature| {
                let FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                    reference: Some(DatumPlaneReference::Feature { feature: reference }),
                    ..
                }) = feature.evaluation.definition()
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
    let parameters_by_id = BorrowedIdentities::build(ctx, |add| {
        for parameter in ctx.admit_iter(&ir.model.parameters, "parameter owner scan")? {
            let (identity, value) = (parameter.id.as_str(), parameter.owner.as_ref());
            add(identity, value)?;
        }
        Ok(())
    })?;
    let input_topologies = BorrowedIdentities::build(ctx, |add| {
        for state in ctx.admit_iter(
            &ir.model.feature_input_topologies,
            "input topology identity scan",
        )? {
            let (identity, value) = (state.id.as_str(), state);
            add(identity, value)?;
        }
        Ok(())
    })?;
    let mut topology_owners = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for state in &ir.model.feature_input_topologies {
        ctx.charge_work(1, "topology validation scan")?;
        if !features.contains(ctx, state.input_of.as_str())? {
            ref_error(
                ctx,
                findings,
                state.id.as_str(),
                "input feature",
                state.input_of.as_str(),
            )?;
        }
        if !topology_owners.insert_unique(state.input_of.as_str(), ())? {
            super::record_finding(
                ctx,
                findings,
                Check::Counts,
                Severity::Error,
                Some(state.input_of.as_str()),
                format_args!("feature has multiple input topology states"),
            )?;
        }
    }
    let mut result_owners = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for state in &ir.model.feature_result_topologies {
        ctx.charge_work(1, "topology validation scan")?;
        if !features.contains(ctx, state.output_of.as_str())? {
            ref_error(
                ctx,
                findings,
                state.id.as_str(),
                "result feature",
                state.output_of.as_str(),
            )?;
        }
        if !result_owners.insert_unique(state.output_of.as_str(), ())? {
            super::record_finding(
                ctx,
                findings,
                Check::Counts,
                Severity::Error,
                Some(state.output_of.as_str()),
                format_args!("feature has multiple result topology states"),
            )?;
        }
    }
    let result_topologies_by_feature = BorrowedIdentities::build(ctx, |add| {
        for state in ctx.admit_iter(
            &ir.model.feature_result_topologies,
            "result topology owner scan",
        )? {
            let (identity, value) = (state.output_of.as_str(), state);
            add(identity, value)?;
        }
        Ok(())
    })?;
    let mut feature_ordinals = super::orders::Orders::new(ctx)?;
    for feature in &ir.model.features {
        ctx.charge_work(1, "topology validation scan")?;
        if !feature_ordinals.insert(feature.ordinal)? {
            super::record_finding(
                ctx,
                findings,
                Check::Counts,
                Severity::Error,
                Some(feature.id.as_str()),
                format_args!("design repeats feature ordinal {}", feature.ordinal),
            )?;
        }
        for dependency in
            ctx.admit_iter(feature.dependencies.as_slice(), "topology validation scan")?
        {
            match features.get(ctx, dependency.as_str())? {
                None => ref_error(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "dependency feature",
                    dependency.as_str(),
                )?,
                Some(ordinal) if *ordinal >= feature.ordinal => super::record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!(
                        "dependency feature `{}` does not precede its consumer",
                        dependency.as_str()
                    ),
                )?,
                Some(_) => {}
            }
        }
        for item in ctx.admit_iter(&feature.source_content[..], "topology validation scan")? {
            match item {
                FeatureSourceContent::Text(_) => {}
                FeatureSourceContent::Parameter(parameter) => {
                    match parameters_by_id.get(ctx, parameter.as_str())? {
                        None => {
                            ref_error(
                                ctx,
                                findings,
                                feature.id.as_str(),
                                "content parameter",
                                parameter.as_str(),
                            )?;
                        }
                        Some(owner) if !same_feature_owner(ctx, *owner, Some(&feature.id))? => {
                            super::record_finding(
                                ctx,
                                findings,
                                Check::ReferentialIntegrity,
                                Severity::Error,
                                Some(feature.id.as_str()),
                                format_args!(
                                    "content parameter `{}` belongs to another feature",
                                    parameter.as_str()
                                ),
                            )?;
                        }
                        Some(_) => {}
                    }
                }
                FeatureSourceContent::Feature(child) => match features.get(ctx, child.as_str())? {
                    None => ref_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "content child",
                        child.as_str(),
                    )?,
                    Some(ordinal) if *ordinal <= feature.ordinal => super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(feature.id.as_str()),
                        format_args!(
                            "content child `{}` does not follow its parent",
                            child.as_str()
                        ),
                    )?,
                    Some(_) => {}
                },
            }
        }
        for body in feature.evaluation.outputs() {
            ctx.charge_work(1, "topology validation scan")?;
            if ids.bodies(body.as_str(), ctx)?.is_none() {
                ref_error(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "output body",
                    body.as_str(),
                )?;
            }
        }

        let mut paths = Scratch::new(ctx)?;
        let mut edge_selections = Scratch::new(ctx)?;
        let mut face_selections = Scratch::new(ctx)?;
        let mut vertex_selections = Scratch::new(ctx)?;
        let mut body_selections = Scratch::new(ctx)?;
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
                if !asset_ids.contains(ctx, asset.as_str())? {
                    ref_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "reference-image asset",
                        asset.as_str(),
                    )?;
                }
            }
            FeatureOperation::Decal { asset, faces, .. } => {
                if !asset_ids.contains(ctx, asset.as_str())? {
                    ref_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "decal asset",
                        asset.as_str(),
                    )?;
                }
                face_selections.push(faces)?;
            }
            FeatureOperation::Block { .. } => {}

            FeatureOperation::ExtractBody { source } => body_selections.push(source)?,
            FeatureOperation::FaceBlend { operands, .. } => {
                face_selections.push(operands.first_faces())?;
                face_selections.push(operands.second_faces())?;
            }
            FeatureOperation::FullRoundFillet { groups } => {
                for group in ctx.admit_iter(groups.as_slice(), "topology validation scan")? {
                    face_selections.push(group.center_faces())?;
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        ctx.charge_work(1, "topology validation scan")?;
                        if let crate::features::edge_treatments::FullRoundSideSelection::Explicit(
                            selection,
                        ) = side
                        {
                            face_selections.push(selection)?;
                        }
                    }
                }
            }
            FeatureOperation::SewBodies { bodies, .. } => body_selections.push(bodies)?,
            FeatureOperation::BaseFeature { bodies } => body_selections.push(bodies)?,
            FeatureOperation::MeshImport { tessellations } => {
                for tessellation in
                    ctx.admit_iter(tessellations.as_slice(), "topology validation scan")?
                {
                    if ids.tessellations(tessellation, ctx)?.is_none() {
                        ref_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "mesh import tessellation",
                            tessellation,
                        )?;
                    }
                }
            }
            FeatureOperation::InsertBodies { .. } => {}
            FeatureOperation::InsertComponent { occurrence } => {
                if !super::scans::any(ctx, ir.model.occurrences.iter(), |candidate| {
                    Ok::<_, CodecError>(crate::ids::comparison::equal(
                        ctx,
                        candidate.id.as_str(),
                        occurrence.as_str(),
                        "topology reference comparison",
                    )?)
                })? {
                    ref_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "inserted component occurrence",
                        occurrence.as_str(),
                    )?;
                }
            }
            FeatureOperation::AssemblyJoint { joint } => {
                if !super::scans::any(ctx, ir.model.assembly_joints.iter(), |candidate| {
                    Ok::<_, CodecError>(crate::ids::comparison::equal(
                        ctx,
                        candidate.id.as_str(),
                        joint.as_str(),
                        "topology reference comparison",
                    )?)
                })? {
                    ref_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "assembly joint",
                        joint.as_str(),
                    )?;
                }
            }
            FeatureOperation::Form { cages } => {
                check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "Form control cage",
                    cages.iter().map(super::super::ids::SubdId::as_str),
                    |identity| Ok(ids.subds(identity, ctx)?.is_some()),
                )?;
            }
            FeatureOperation::CosmeticThread { face, .. } => face_selections.push(face)?,
            FeatureOperation::Extrude {
                direction, start, ..
            } => {
                if let crate::features::ExtrudeDirection::Explicit {
                    source: Some(crate::features::ExtrusionDirectionSource::Edge { reference }),
                    ..
                } = direction
                {
                    paths.push(reference)?;
                }
                if let ExtrudeStart::FromFace { face, .. } = start {
                    face_selections.push(face)?;
                }
            }
            FeatureOperation::SheetMetalEdgeFlange { edges, height, .. } => {
                edge_selections.push(edges)?;
                if matches!(height, crate::features::SheetMetalFlangeHeight::ToObject {
                    target: crate::features::SheetMetalFlangeHeightTarget::Native(native), ..
                } if native.is_empty())
                {
                    geometry_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "sheet-metal edge-flange height is invalid",
                    )?;
                }
            }
            FeatureOperation::SheetMetalHem { edges, .. } => edge_selections.push(edges)?,
            FeatureOperation::Revolve { construction, .. } => {
                paths.extend(
                    construction
                        .axis()
                        .and_then(|axis| axis.reference.as_ref())
                        .as_slice(),
                    |path| *path,
                )?;
            }
            FeatureOperation::Sweep {
                path,
                orientation,
                guide_rail,
                ..
            } => {
                paths.extend(path.as_slice(), |path| path)?;
                if let Some(guide_rail) = guide_rail {
                    paths.push(&guide_rail.path)?;
                }
                if let Some(crate::features::SweepOrientation::Auxiliary { path, .. }) = orientation
                {
                    paths.push(path)?;
                }
                if let Some(crate::features::SweepOrientation::GuideSurface { faces }) = orientation
                {
                    face_selections.push(faces)?;
                }
            }
            FeatureOperation::Loft {
                sections, guidance, ..
            } => {
                for section in sections {
                    ctx.charge_work(1, "topology validation scan")?;
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
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "loft section vertex",
                            std::iter::once(vertex.as_str()),
                            |identity| Ok(ids.vertices(identity, ctx)?.is_some()),
                        )?,
                    }
                }
                match guidance {
                    crate::features::LoftGuidance::Guides(guides) => {
                        paths.extend(guides, |guide| guide)?;
                    }
                    crate::features::LoftGuidance::Centerline(centerline) => {
                        paths.push(centerline)?;
                    }
                }
            }
            FeatureOperation::Rib { .. } => {}
            FeatureOperation::Fillet { groups } => {
                edge_selections.extend(groups.as_slice(), |group| &group.edges)?;
            }
            FeatureOperation::Chamfer { groups, .. } => {
                edge_selections.extend(groups.as_slice(), |group| &group.edges)?;
            }
            FeatureOperation::Shell {
                bodies,
                removed_faces,
                ..
            } => {
                if let Some(bodies) = bodies {
                    body_selections.push(bodies)?;
                }
                face_selections.push(removed_faces)?;
            }
            FeatureOperation::OffsetShape { source, .. } => body_selections.push(source)?,
            FeatureOperation::Compound { members } => body_selections.push(members)?,
            FeatureOperation::RefineShape { source }
            | FeatureOperation::ReverseShape { source } => body_selections.push(source)?,
            FeatureOperation::RuledBetweenCurves { first, second, .. } => {
                paths.push(first)?;
                paths.push(second)?;
            }
            FeatureOperation::SectionShape { operands, .. } => {
                body_selections.push(operands.first())?;
                body_selections.push(operands.second())?;
            }
            FeatureOperation::MirrorShape {
                source,
                plane_reference,
                ..
            } => {
                body_selections.push(source)?;
                face_selections.extend(plane_reference.as_slice(), |plane| plane)?;
            }
            FeatureOperation::Thicken { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::OffsetSurface { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::KnitSurface { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::FilledSurface {
                boundary,
                support_faces,
                ..
            } => {
                match boundary {
                    crate::features::SurfaceBoundary::Edges(edges) => {
                        edge_selections.push(edges)?;
                    }
                    crate::features::SurfaceBoundary::Path(path) => paths.push(path)?,
                }
                face_selections.push(support_faces)?;
            }
            FeatureOperation::TrimSurface { faces, tool, .. } => {
                face_selections.push(faces)?;
                paths.push(tool)?;
            }
            FeatureOperation::ExtendSurface { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::RuledSurface {
                edges,
                support_faces,
                ..
            } => {
                edge_selections.push(edges)?;
                face_selections.push(support_faces)?;
            }
            FeatureOperation::Draft { faces, anchor, .. } => {
                face_selections.push(faces)?;
                match anchor {
                    crate::features::DraftAnchor::NeutralPlane { plane, .. } => {
                        face_selections.push(plane)?;
                    }
                    crate::features::DraftAnchor::PartingLine { tool, .. } => {
                        face_selections.push(tool)?;
                    }
                }
                if let Some(pull_plane) = anchor.pull().and_then(|pull| pull.plane.as_ref()) {
                    check_plane_feature_reference(
                        ctx,
                        findings,
                        feature,
                        pull_plane,
                        &feature_records,
                        "draft pull plane",
                    )?;
                }
            }
            FeatureOperation::BoundaryFill { tools, cells } => {
                body_selections.push(tools)?;
                body_selections.extend(cells.as_slice(), |cell| cell)?;
            }
            FeatureOperation::SplitBody { targets, tools } => {
                body_selections.push(targets)?;
                face_selections.push(tools)?;
            }
            FeatureOperation::SplitFace { targets, tool } => {
                face_selections.push(targets)?;
                match tool {
                    SplitFaceTool::Path(path) => paths.push(path)?,
                    SplitFaceTool::Plane { plane } => check_plane_feature_reference(
                        ctx,
                        findings,
                        feature,
                        plane,
                        &feature_records,
                        "split-face tool plane",
                    )?,
                    SplitFaceTool::Planes { planes } => {
                        for plane in ctx.admit_iter(&planes[..], "topology validation scan")? {
                            check_plane_feature_reference(
                                ctx,
                                findings,
                                feature,
                                plane,
                                &feature_records,
                                "split-face tool plane",
                            )?;
                        }
                    }
                }
            }
            FeatureOperation::DeleteFace { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::ReplaceFace { operands } => {
                face_selections.push(operands.targets())?;
                face_selections.push(operands.replacements())?;
            }
            FeatureOperation::MoveFace { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::MoveBody { bodies, .. } => {
                body_selections.push(bodies)?;
            }
            FeatureOperation::Dome { faces, .. } => {
                face_selections.push(faces)?;
            }
            FeatureOperation::Flex { .. } => {}
            FeatureOperation::Scale { bodies, center, .. } => {
                body_selections.push(bodies)?;
                let center_valid = center.as_ref().is_none_or(|center| match center {
                    ScaleCenter::Point(_) => true,
                    ScaleCenter::Native(reference) => !reference.is_empty(),
                    ScaleCenter::Centroid | ScaleCenter::ModelOrigin => true,
                });
                if !center_valid {
                    geometry_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "scale transform is invalid",
                    )?;
                }
            }
            FeatureOperation::Combine { operands, .. } => {
                body_selections.push(operands.target())?;
                body_selections.push(operands.tools())?;
            }
            FeatureOperation::CutWithSurface { targets, tools, .. } => {
                body_selections.push(targets)?;
                face_selections.push(tools)?;
            }
            FeatureOperation::TrimBodies { operands, .. } => {
                body_selections.push(operands.targets())?;
                body_selections.push(operands.tools())?;
            }
            FeatureOperation::DeleteBody { bodies, .. } => {
                body_selections.push(bodies)?;
            }
            FeatureOperation::Hole { face, .. } => {
                face_selections.extend(face.as_slice(), |face| face)?;
            }
            FeatureOperation::Pattern { seeds, pattern } => {
                collect_pattern_paths(ctx, pattern, &mut paths)?;
                for seed in seeds {
                    ctx.charge_work(1, "topology validation scan")?;
                    match seed {
                        PatternSeed::Feature(seed) => match features.get(ctx, seed.as_str())? {
                            None => ref_error(
                                ctx,
                                findings,
                                feature.id.as_str(),
                                "seed feature",
                                seed.as_str(),
                            )?,
                            Some(ordinal) if *ordinal >= feature.ordinal => {
                                super::record_finding(
                                    ctx,
                                    findings,
                                    Check::ReferentialIntegrity,
                                    Severity::Error,
                                    Some(feature.id.as_str()),
                                    format_args!(
                                        "seed feature `{}` does not precede its pattern",
                                        seed.as_str()
                                    ),
                                )?;
                            }
                            Some(_)
                                if !super::scans::any(
                                    ctx,
                                    feature.dependencies.iter(),
                                    |dependency| {
                                        Ok(crate::ids::comparison::equal(
                                            ctx,
                                            dependency.as_str(),
                                            seed.as_str(),
                                            "feature dependency comparison",
                                        )?)
                                    },
                                )? =>
                            {
                                super::record_finding(
                                    ctx,
                                    findings,
                                    Check::ReferentialIntegrity,
                                    Severity::Error,
                                    Some(feature.id.as_str()),
                                    format_args!(
                                        "pattern omits seed feature `{}` from its dependencies",
                                        seed.as_str()
                                    ),
                                )?;
                            }
                            Some(_) => {}
                        },
                        PatternSeed::Faces(selection) => face_selections.push(selection)?,
                        PatternSeed::Bodies(selection) => body_selections.push(selection)?,
                        PatternSeed::Occurrences(occurrences) => {
                            for occurrence in
                                ctx.admit_iter(occurrences.as_slice(), "topology validation scan")?
                            {
                                if !super::scans::any(
                                    ctx,
                                    ir.model.occurrences.iter(),
                                    |candidate| {
                                        Ok::<_, CodecError>(crate::ids::comparison::equal(
                                            ctx,
                                            candidate.id.as_str(),
                                            occurrence.as_str(),
                                            "topology reference comparison",
                                        )?)
                                    },
                                )? {
                                    ref_error(
                                        ctx,
                                        findings,
                                        feature.id.as_str(),
                                        "seed occurrence",
                                        occurrence.as_str(),
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
            FeatureOperation::Sketch { sketch, .. } => {
                if let Some(sketch) = sketch.id() {
                    if !super::scans::any(ctx, ir.model.sketches.iter(), |value| {
                        Ok::<_, CodecError>(crate::ids::comparison::equal(
                            ctx,
                            value.id.as_str(),
                            sketch.as_str(),
                            "topology reference comparison",
                        )?)
                    })? {
                        ref_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "owned sketch",
                            sketch.as_str(),
                        )?;
                    }
                }
            }
            FeatureOperation::SpatialSketch { sketch } => {
                if let Some(sketch) = sketch {
                    if !super::scans::any(ctx, ir.model.spatial_sketches.iter(), |value| {
                        Ok::<_, CodecError>(crate::ids::comparison::equal(
                            ctx,
                            value.id.as_str(),
                            sketch.as_str(),
                            "topology reference comparison",
                        )?)
                    })? {
                        ref_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "owned spatial sketch",
                            sketch.as_str(),
                        )?;
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
                paths.push(source)?;
                face_selections.push(target_faces)?;
            }
            FeatureOperation::ProjectOnSurface {
                sources,
                support_face,
                ..
            } => {
                paths.push(sources)?;
                face_selections.push(support_face)?;
            }
            FeatureOperation::CompositeCurve { segments, .. } => {
                paths.extend(segments.as_slice(), |segment| segment)?;
            }
            FeatureOperation::Helix { .. } => {}
            FeatureOperation::HelixNativeAxis { .. } => {}
            FeatureOperation::Coil { result, .. } => {
                use crate::features::CoilResult;
                if let CoilResult::Boolean { targets, .. } = result {
                    body_selections.push(targets)?;
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
                    ctx.charge_work(1, "topology validation scan")?;
                    if let crate::features::BinderTarget::Feature { feature: target } = target {
                        match features.get(ctx, target.as_str())? {
                            None => ref_error(
                                ctx,
                                findings,
                                feature.id.as_str(),
                                "binder target feature",
                                target.as_str(),
                            )?,
                            Some(ordinal) if *ordinal >= feature.ordinal => {
                                super::record_finding(
                                    ctx,
                                    findings,
                                    Check::ReferentialIntegrity,
                                    Severity::Error,
                                    Some(feature.id.as_str()),
                                    format_args!(
                                        "binder target feature `{}` does not precede its binder",
                                        target.as_str()
                                    ),
                                )?;
                            }
                            Some(_) => {}
                        }
                    }
                }
            }
            FeatureOperation::Wrap { face, .. } => {
                face_selections.push(face)?;
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
                body_selections.push(sources)?;
            }
            FeatureOperation::TreeNode { children, .. } => {
                for child in ctx.admit_iter(&children[..], "topology validation scan")? {
                    if !super::scans::any(ctx, ir.model.features.iter(), |candidate| {
                        Ok::<_, CodecError>(crate::ids::comparison::equal(
                            ctx,
                            candidate.id.as_str(),
                            child.as_str(),
                            "topology reference comparison",
                        )?)
                    })? {
                        ref_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "tree child",
                            child.as_str(),
                        )?;
                    }
                }
            }
            FeatureOperation::DatumPlane { .. } => {}
            FeatureOperation::DatumThreePointPlane { points, .. } => {
                for point in ctx.admit_iter(&points[..], "topology validation scan")? {
                    vertex_selections.push((point, "three-point datum-plane"))?;
                }
            }
            FeatureOperation::DatumAxis { .. } => {}
            FeatureOperation::DatumPoint { construction, .. } => {
                let mut plane_references = Scratch::new(ctx)?;
                if let Some(construction) = construction.as_deref() {
                    match construction {
                        crate::features::DatumPointConstruction::CircleCenter { edge }
                        | crate::features::DatumPointConstruction::DistanceOnEdge {
                            edge, ..
                        } => edge_selections.push(edge)?,
                        crate::features::DatumPointConstruction::TwoEdgeIntersection { edges } => {
                            edge_selections.extend(&edges[..], |edge| edge)?;
                        }
                        crate::features::DatumPointConstruction::ThreePlaneIntersection {
                            planes,
                        } => plane_references.extend(planes.as_ref(), |plane| plane)?,
                        crate::features::DatumPointConstruction::Vertex { vertex } => {
                            vertex_selections.push((vertex, "datum-point"))?;
                        }
                        crate::features::DatumPointConstruction::SketchPoint { .. } => {}
                        crate::features::DatumPointConstruction::EdgePlaneIntersection {
                            edge,
                            plane,
                        } => {
                            edge_selections.push(edge)?;
                            plane_references.push(plane)?;
                        }
                    }
                }
                for plane in ctx
                    .admit_iter(&plane_references[..], "topology validation scan")?
                    .copied()
                {
                    match plane {
                        DatumPlaneReference::Feature { feature: reference } => {
                            match feature_records.get(ctx, reference.as_str())? {
                                None => ref_error(
                                    ctx,
                                    findings,
                                    feature.id.as_str(),
                                    "datum-point plane",
                                    reference.as_str(),
                                )?,
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
                                    geometry_error(
                                        ctx,
                                        findings,
                                        feature.id.as_str(),
                                        "datum-point plane reference does not name a plane",
                                    )?;
                                }
                                Some(record) if record.ordinal >= feature.ordinal => {
                                    geometry_error(
                                        ctx,
                                        findings,
                                        feature.id.as_str(),
                                        "datum-point plane does not precede the point",
                                    )?;
                                }
                                Some(_)
                                    if !super::scans::any(
                                        ctx,
                                        feature.dependencies.iter(),
                                        |dependency| {
                                            Ok(crate::ids::comparison::equal(
                                                ctx,
                                                dependency.as_str(),
                                                reference.as_str(),
                                                "feature dependency comparison",
                                            )?)
                                        },
                                    )? =>
                                {
                                    super::record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(feature.id.as_str()), format_args!(
                                            "datum point omits plane feature `{}` from its dependencies",
                                            reference.as_str()
                                        ))?;
                                }
                                Some(_) => {}
                            }
                        }
                        DatumPlaneReference::Face { face } => face_selections.push(face)?,
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
                    match features.get(ctx, block.as_str())? {
                        None => ref_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "sketch block",
                            block.as_str(),
                        )?,
                        Some(ordinal) if *ordinal >= feature.ordinal => geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "sketch block does not precede its instance",
                        )?,
                        Some(_)
                            if !super::scans::any(
                                ctx,
                                ir.model.features.iter(),
                                |candidate| {
                                    Ok::<_, CodecError>({
                                        crate::ids::comparison::equal(
                                            ctx,
                                            candidate.id.as_str(),
                                            block.as_str(),
                                            "topology reference comparison",
                                        )? && matches!(
                                            candidate.evaluation.definition().operation(),
                                            FeatureOperation::SketchBlockDefinition { .. }
                                        )
                                    })
                                },
                            )? =>
                        {
                            geometry_error(
                                ctx,
                                findings,
                                feature.id.as_str(),
                                "sketch block target is not a block definition",
                            )?;
                        }
                        Some(_)
                            if !super::scans::any(
                                ctx,
                                feature.dependencies.iter(),
                                |dependency| {
                                    Ok(crate::ids::comparison::equal(
                                        ctx,
                                        dependency.as_str(),
                                        block.as_str(),
                                        "feature dependency comparison",
                                    )?)
                                },
                            )? =>
                        {
                            super::record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(feature.id.as_str()), format_args!(
                                    "sketch block instance omits block feature `{}` from its dependencies",
                                    block.as_str()
                                ))?;
                        }
                        Some(_) => {}
                    }
                }
            }
            FeatureOperation::DerivedGeometry { source } => match features
                .get(ctx, source.as_str())?
            {
                None => ref_error(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "source feature",
                    source.as_str(),
                )?,
                Some(ordinal) if *ordinal >= feature.ordinal => super::record_finding(
                    ctx,
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(feature.id.as_str()),
                    format_args!(
                        "source feature `{}` does not precede its derived geometry",
                        source.as_str()
                    ),
                )?,
                Some(_)
                    if !super::scans::any(ctx, feature.dependencies.iter(), |dependency| {
                        Ok(crate::ids::comparison::equal(
                            ctx,
                            dependency.as_str(),
                            source.as_str(),
                            "feature dependency comparison",
                        )?)
                    })? =>
                {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(feature.id.as_str()),
                        format_args!(
                            "derived geometry omits source feature `{}` from its dependencies",
                            source.as_str()
                        ),
                    )?;
                }
                Some(_) => {}
            },
            FeatureOperation::ImportedGeometry { .. } => {}
            FeatureOperation::DatumOffsetPlane { reference, .. } => {
                if let Some(reference) = reference {
                    match reference {
                        DatumPlaneReference::Feature { feature: reference } => {
                            match feature_records.get(ctx, reference.as_str())? {
                                None => {
                                    ref_error(
                                        ctx,
                                        findings,
                                        feature.id.as_str(),
                                        "reference plane",
                                        reference.as_str(),
                                    )?;
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
                                    geometry_error(
                                        ctx,
                                        findings,
                                        feature.id.as_str(),
                                        "datum-plane feature reference does not name a plane",
                                    )?;
                                }
                                Some(record) if record.ordinal >= feature.ordinal => {
                                    super::record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(feature.id.as_str()), format_args!(
                                            "reference plane `{}` does not precede its offset plane",
                                            reference.as_str()
                                        ))?;
                                }
                                Some(_)
                                    if !super::scans::any(
                                        ctx,
                                        feature.dependencies.iter(),
                                        |dependency| {
                                            Ok(crate::ids::comparison::equal(
                                                ctx,
                                                dependency.as_str(),
                                                reference.as_str(),
                                                "feature dependency comparison",
                                            )?)
                                        },
                                    )? =>
                                {
                                    super::record_finding(ctx, findings, Check::ReferentialIntegrity, Severity::Error, Some(feature.id.as_str()), format_args!(
                                            "offset plane omits reference feature `{}` from its dependencies",
                                            reference.as_str()
                                        ))?;
                                }
                                Some(_) => {}
                            }
                        }
                        DatumPlaneReference::Face { face } => face_selections.push(face)?,
                        DatumPlaneReference::ResolvedPlane { .. } => {}
                    }
                }
            }
        }
        let definition_profiles = definition_profiles(ctx, definition)?;
        for profile in ctx
            .admit_iter(&definition_profiles[..], "topology validation scan")?
            .copied()
        {
            match profile {
                PlanarProfileRef::Faces(faces) => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "profile face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?,
                PlanarProfileRef::HistoricalFaces { state, faces, .. } => {
                    check_historical_members(
                        ctx,
                        findings,
                        &feature.id,
                        (state, faces.as_slice()),
                        crate::ids::HistoricalFaceId::as_str,
                        "profile face",
                        &input_topologies,
                        |topology| {
                            Scratch::filter_map(
                                ctx,
                                topology
                                    .faces
                                    .iter()
                                    .map(crate::ids::HistoricalFaceId::as_str),
                                |id| Ok(Some(id)),
                            )
                        },
                    )?;
                }
                PlanarProfileRef::Feature(producer) => {
                    match features.get(ctx, producer.as_str())? {
                        None => ref_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "profile feature",
                            producer.as_str(),
                        )?,
                        Some(ordinal)
                            if *ordinal >= feature.ordinal
                                || !super::scans::any(
                                    ctx,
                                    feature.dependencies.iter(),
                                    |dependency| {
                                        Ok(crate::ids::comparison::equal(
                                            ctx,
                                            dependency.as_str(),
                                            producer.as_str(),
                                            "feature dependency comparison",
                                        )?)
                                    },
                                )? =>
                        {
                            geometry_error(
                                ctx,
                                findings,
                                feature.id.as_str(),
                                "profile feature is not a preceding dependency",
                            )?;
                        }
                        Some(_) => {}
                    }
                }
                PlanarProfileRef::Generated { curves, .. }
                    if super::scans::any(ctx, curves.iter(), |curve| {
                        Ok::<_, CodecError>({
                            features
                                .get(ctx, curve.feature.as_str())?
                                .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                                || !super::scans::any(
                                    ctx,
                                    feature.dependencies.iter(),
                                    |dependency| {
                                        Ok(crate::ids::comparison::equal(
                                            ctx,
                                            dependency.as_str(),
                                            curve.feature.as_str(),
                                            "feature dependency comparison",
                                        )?)
                                    },
                                )?
                        })
                    })? =>
                {
                    geometry_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "generated profile curve is invalid",
                    )?;
                }
                _ => {}
            }
        }
        drop(definition_profiles);
        for path in ctx
            .admit_iter(&paths[..], "topology validation scan")?
            .copied()
        {
            match path {
                PathRef::Edges(edges) => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "path edge",
                    edges.iter().map(super::super::ids::EdgeId::as_str),
                    |identity| Ok(ids.edges(identity, ctx)?.is_some()),
                )?,
                PathRef::Curves(curves) => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "path curve",
                    curves.iter().map(super::super::ids::CurveId::as_str),
                    |identity| Ok(ids.curves(identity, ctx)?.is_some()),
                )?,
                PathRef::SketchCurves { curves, .. } => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "sketch path curve",
                    curves.iter().map(crate::sketches::SketchEntityId::as_str),
                    |identity| sketch_entities.contains(ctx, identity),
                )?,
                PathRef::SpatialSketchCurves { curves, .. } => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "spatial sketch path curve",
                    curves
                        .iter()
                        .map(crate::sketches::SpatialSketchEntityId::as_str),
                    |identity| spatial_sketch_entity_owners.contains(ctx, identity),
                )?,
                PathRef::HistoricalEdges { state, edges, .. } => check_historical_members(
                    ctx,
                    findings,
                    &feature.id,
                    (state, edges.as_slice()),
                    crate::ids::HistoricalEdgeId::as_str,
                    "path edge",
                    &input_topologies,
                    |topology| {
                        Scratch::filter_map(
                            ctx,
                            topology
                                .edges
                                .iter()
                                .map(crate::ids::HistoricalEdgeId::as_str),
                            |id| Ok(Some(id)),
                        )
                    },
                )?,
                PathRef::Unresolved(_)
                | PathRef::Native(_)
                | PathRef::Sketch(_)
                | PathRef::SpatialSketchSelection { .. } => {}
            }
        }
        drop(paths);
        let terminations = definition_terminations(ctx, definition)?;
        for termination in ctx
            .admit_iter(&terminations[..], "topology validation scan")?
            .copied()
        {
            if let Some(FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. }) =
                termination.face()
            {
                check_ids(
                    ctx,
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
                check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "termination shape face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?;
            }
            if let Some(vertex) = termination.vertex() {
                vertex_selections.push((vertex, "termination"))?;
            }
        }
        drop(terminations);
        for (selection, consumer) in ctx
            .admit_iter(&vertex_selections[..], "topology validation scan")?
            .copied()
        {
            match selection {
                crate::features::VertexSelection::Generated { vertex, .. } => {
                    if features
                        .get(ctx, vertex.feature.as_str())?
                        .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                        || !super::scans::any(ctx, feature.dependencies.iter(), |dependency| {
                            Ok(crate::ids::comparison::equal(
                                ctx,
                                dependency.as_str(),
                                vertex.feature.as_str(),
                                "feature dependency comparison",
                            )?)
                        })?
                        || result_topologies_by_feature
                            .get(ctx, vertex.feature.as_str())?
                            .map_or(Ok::<_, CodecError>(false), |state| {
                                Ok::<_, CodecError>({
                                    !super::scans::any(ctx, state.vertices().iter(), |id| {
                                        Ok::<_, CodecError>(crate::ids::comparison::equal(
                                            ctx,
                                            id.as_str(),
                                            vertex.local_id.as_str(),
                                            "generated topology identity",
                                        )?)
                                    })?
                                })
                            })?
                    {
                        geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            format_args!("generated {consumer} vertex is invalid"),
                        )?;
                    }
                }
                crate::features::VertexSelection::Historical {
                    state,
                    vertex,
                    native: _,
                } => check_historical_members(
                    ctx,
                    findings,
                    &feature.id,
                    (state, std::slice::from_ref(vertex)),
                    crate::ids::HistoricalVertexId::as_str,
                    "vertex",
                    &input_topologies,
                    |topology| {
                        Scratch::filter_map(
                            ctx,
                            topology
                                .vertices
                                .iter()
                                .map(crate::ids::HistoricalVertexId::as_str),
                            |id| Ok(Some(id)),
                        )
                    },
                )?,
                crate::features::VertexSelection::Unresolved
                | crate::features::VertexSelection::Native(_) => {}
            }
        }
        drop(vertex_selections);
        for selection in ctx
            .admit_iter(&edge_selections[..], "topology validation scan")?
            .copied()
        {
            let historical = match selection {
                EdgeSelection::Historical { state, edges, .. } => Some((state, edges.as_slice())),
                EdgeSelection::HistoricalPartial { state, edges, .. } => {
                    Some((state, edges.as_slice()))
                }
                _ => None,
            };
            if let Some((state, selected)) = historical {
                check_historical_members(
                    ctx,
                    findings,
                    &feature.id,
                    (state, selected),
                    crate::ids::HistoricalEdgeId::as_str,
                    "edge",
                    &input_topologies,
                    |topology| {
                        Scratch::filter_map(
                            ctx,
                            topology
                                .edges
                                .iter()
                                .map(crate::ids::HistoricalEdgeId::as_str),
                            |id| Ok(Some(id)),
                        )
                    },
                )?;
            }
            match selection {
                EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "selected edge",
                    edges.iter().map(super::super::ids::EdgeId::as_str),
                    |identity| Ok(ids.edges(identity, ctx)?.is_some()),
                )?,
                EdgeSelection::Historical { .. } | EdgeSelection::HistoricalPartial { .. } => {}
                EdgeSelection::Generated { edges, .. } => {
                    if super::scans::any(ctx, edges.iter(), |edge| {
                        Ok::<_, CodecError>({
                            !super::scans::any(ctx, feature.dependencies.iter(), |dependency| {
                                Ok(crate::ids::comparison::equal(
                                    ctx,
                                    dependency.as_str(),
                                    edge.feature.as_str(),
                                    "feature dependency comparison",
                                )?)
                            })? || result_topologies_by_feature
                                .get(ctx, edge.feature.as_str())?
                                .map_or(Ok::<_, CodecError>(false), |state| {
                                    Ok::<_, CodecError>({
                                        !super::scans::any(ctx, state.edges().iter(), |id| {
                                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                                ctx,
                                                id.as_str(),
                                                edge.local_id.as_str(),
                                                "generated topology identity",
                                            )?)
                                        })?
                                    })
                                })?
                        })
                    })? {
                        geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "generated edge selection is invalid",
                        )?;
                    }
                }
                EdgeSelection::All | EdgeSelection::Unresolved | EdgeSelection::Native(_) => {}
            }
        }
        drop(edge_selections);
        for selection in ctx
            .admit_iter(&face_selections[..], "topology validation scan")?
            .copied()
        {
            let historical = match selection {
                FaceSelection::Historical { state, faces, .. } => Some((state, faces.as_slice())),
                FaceSelection::HistoricalPartial { state, faces, .. } => {
                    Some((state, faces.as_slice()))
                }
                _ => None,
            };
            if let Some((state, selected)) = historical {
                check_historical_members(
                    ctx,
                    findings,
                    &feature.id,
                    (state, selected),
                    crate::ids::HistoricalFaceId::as_str,
                    "face",
                    &input_topologies,
                    |topology| {
                        Scratch::filter_map(
                            ctx,
                            topology
                                .faces
                                .iter()
                                .map(crate::ids::HistoricalFaceId::as_str),
                            |id| Ok(Some(id)),
                        )
                    },
                )?;
            }
            match selection {
                FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => check_ids(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    "selected face",
                    faces.iter().map(super::super::ids::FaceId::as_str),
                    |identity| Ok(ids.faces(identity, ctx)?.is_some()),
                )?,
                FaceSelection::Historical { .. } | FaceSelection::HistoricalPartial { .. } => {}
                FaceSelection::Generated { faces, .. } => {
                    if super::scans::any(ctx, faces.iter(), |face| {
                        Ok::<_, CodecError>({
                            !super::scans::any(ctx, feature.dependencies.iter(), |dependency| {
                                Ok(crate::ids::comparison::equal(
                                    ctx,
                                    dependency.as_str(),
                                    face.feature.as_str(),
                                    "feature dependency comparison",
                                )?)
                            })? || result_topologies_by_feature
                                .get(ctx, face.feature.as_str())?
                                .map_or(Ok::<_, CodecError>(false), |state| {
                                    Ok::<_, CodecError>({
                                        !super::scans::any(ctx, state.faces().iter(), |id| {
                                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                                ctx,
                                                id.as_str(),
                                                face.local_id.as_str(),
                                                "generated topology identity",
                                            )?)
                                        })?
                                    })
                                })?
                        })
                    })? {
                        geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "generated face selection is invalid",
                        )?;
                    }
                }
                FaceSelection::Unresolved | FaceSelection::Native(_) => {}
            }
        }
        drop(face_selections);
        for selection in ctx
            .admit_iter(&body_selections[..], "topology validation scan")?
            .copied()
        {
            match selection {
                BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
                    check_ids(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "selected body",
                        bodies.iter().map(super::super::ids::BodyId::as_str),
                        |identity| Ok(ids.bodies(identity, ctx)?.is_some()),
                    )?;
                }
                BodySelection::ResolvedSet { members } => {
                    check_ids(
                        ctx,
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
                        ctx,
                        findings,
                        &feature.id,
                        (state, bodies.as_slice()),
                        crate::ids::HistoricalBodyId::as_str,
                        "body",
                        &input_topologies,
                        |topology| {
                            Scratch::filter_map(
                                ctx,
                                topology
                                    .bodies
                                    .iter()
                                    .map(crate::ids::HistoricalBodyId::as_str),
                                |id| Ok(Some(id)),
                            )
                        },
                    )?;
                }
                BodySelection::HistoricalSet { state, members } => {
                    check_historical_members(
                        ctx,
                        findings,
                        &feature.id,
                        (state, members.iter().as_slice()),
                        |member| member.body().as_str(),
                        "body",
                        &input_topologies,
                        |topology| {
                            Scratch::filter_map(
                                ctx,
                                topology
                                    .bodies
                                    .iter()
                                    .map(crate::ids::HistoricalBodyId::as_str),
                                |id| Ok(Some(id)),
                            )
                        },
                    )?;
                }
                BodySelection::Generated { bodies, .. } => {
                    if super::scans::any(ctx, bodies.iter(), |body| {
                        Ok::<_, CodecError>({
                            !super::scans::any(ctx, feature.dependencies.iter(), |dependency| {
                                Ok(crate::ids::comparison::equal(
                                    ctx,
                                    dependency.as_str(),
                                    body.feature.as_str(),
                                    "feature dependency comparison",
                                )?)
                            })? || result_topologies_by_feature
                                .get(ctx, body.feature.as_str())?
                                .map_or(Ok::<_, CodecError>(false), |state| {
                                    Ok::<_, CodecError>({
                                        !super::scans::any(ctx, state.bodies().iter(), |id| {
                                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                                ctx,
                                                id.as_str(),
                                                body.local_id.as_str(),
                                                "generated topology identity",
                                            )?)
                                        })?
                                    })
                                })?
                        })
                    })? {
                        geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "generated body selection is invalid",
                        )?;
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
            if index != 0 {
                output.write_str(", ")?;
            }
            write!(output, "`{id}`")?;
        }
        Ok(())
    }
}

fn check_historical_members<'ctx, 'a, 'selected, T, F, M>(
    ctx: &'ctx DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    feature: &crate::features::FeatureId,
    selection: (&crate::ids::FeatureInputTopologyId, &'selected [T]),
    selected_id: F,
    kind: &str,
    states: &BorrowedIdentities<'_, 'a, &'a crate::features::FeatureInputTopology>,
    members: M,
) -> Result<(), CodecError>
where
    F: Fn(&'selected T) -> &'selected str,
    M: FnOnce(
        &'a crate::features::FeatureInputTopology,
    ) -> Result<Scratch<'ctx, &'a str>, CodecError>,
{
    let (state_id, selected) = selection;
    let Some(state) = states.get(ctx, state_id.as_str())? else {
        ref_error(
            ctx,
            findings,
            feature.as_str(),
            "feature input topology",
            state_id.as_str(),
        )?;
        return Ok(());
    };
    if !crate::ids::comparison::equal(
        ctx,
        state.input_of.as_str(),
        feature.as_str(),
        "historical topology owner",
    )? {
        super::record_finding(
            ctx,
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            Some(feature.as_str()),
            format_args!("historical {kind} selection uses another feature's input topology"),
        )?;
    }
    let available = BorrowedIdentities::build(ctx, |add| {
        let available_members = members(state)?;
        for id in ctx.admit_iter(&available_members[..], "historical member scan")? {
            add(*id, ())?;
        }
        drop(available_members);
        Ok(())
    })?;
    for member in ctx.admit_iter(selected, "historical selection scan")? {
        let id = selected_id(member);
        if !available.contains(ctx, id)? {
            ref_error(
                ctx,
                findings,
                feature.as_str(),
                format_args!("historical {kind}"),
                id,
            )?;
        }
    }

    Ok(())
}

fn regeneration_references<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    definition: &'a crate::features::FeatureOperation,
) -> Result<Scratch<'ctx, &'a crate::features::FeatureId>, CodecError> {
    let mut references = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    match definition {
        // A datum offset plane regenerates from its reference only when that
        // reference names a feature; a face-supported plane carries its frame
        // inline and is checked through the face selection instead.
        crate::features::FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: reference }),
            ..
        } => {
            references.insert_unique(reference.as_str(), reference)?;
        }
        crate::features::FeatureOperation::DatumPoint {
            construction: Some(construction),
            ..
        } => match construction.as_ref() {
            crate::features::DatumPointConstruction::ThreePlaneIntersection { planes } => {
                for plane in ctx.admit_iter(planes.as_ref(), "regeneration reference scan")? {
                    if let DatumPlaneReference::Feature { feature } = plane {
                        references.insert_unique(feature.as_str(), feature)?;
                    }
                }
            }
            crate::features::DatumPointConstruction::EdgePlaneIntersection {
                plane: DatumPlaneReference::Feature { feature },
                ..
            } => {
                ctx.charge_work(1, "regeneration reference scan")?;
                references.insert_unique(feature.as_str(), feature)?;
            }
            crate::features::DatumPointConstruction::Vertex {
                vertex: crate::features::VertexSelection::Generated { vertex, .. },
            } => {
                ctx.charge_work(1, "regeneration reference scan")?;
                references.insert_unique(vertex.feature.as_str(), &vertex.feature)?;
            }
            crate::features::DatumPointConstruction::CircleCenter { .. }
            | crate::features::DatumPointConstruction::TwoEdgeIntersection { .. }
            | crate::features::DatumPointConstruction::Vertex { .. }
            | crate::features::DatumPointConstruction::SketchPoint { .. }
            | crate::features::DatumPointConstruction::EdgePlaneIntersection { .. }
            | crate::features::DatumPointConstruction::DistanceOnEdge { .. } => {}
        },
        crate::features::FeatureOperation::DatumThreePointPlane { points, .. } => {
            for point in ctx.admit_iter(&points[..], "regeneration point filter scan")? {
                if let crate::features::VertexSelection::Generated { vertex, .. } = point {
                    ctx.charge_work(1, "regeneration reference scan")?;
                    references.insert_unique(vertex.feature.as_str(), &vertex.feature)?;
                }
            }
        }
        crate::features::FeatureOperation::DerivedGeometry { source: reference }
        | crate::features::FeatureOperation::SketchBlockInstance {
            block: Some(reference),
            ..
        } => {
            references.insert_unique(reference.as_str(), reference)?;
        }
        crate::features::FeatureOperation::Pattern { seeds, .. } => {
            ctx.charge_work(u64_from_index(seeds.len()), "regeneration seed filter scan")?;
            for reference in seeds.iter().filter_map(|seed| match seed {
                crate::features::patterns::PatternSeed::Feature(feature) => Some(feature),
                crate::features::patterns::PatternSeed::Faces(_)
                | crate::features::patterns::PatternSeed::Bodies(_)
                | crate::features::patterns::PatternSeed::Occurrences(_) => None,
            }) {
                ctx.charge_work(1, "regeneration reference scan")?;
                references.insert_unique(reference.as_str(), reference)?;
            }
        }
        _ => {}
    }
    let terminations = definition_terminations(ctx, definition)?;
    for termination in ctx
        .admit_iter(&terminations[..], "regeneration termination scan")?
        .copied()
    {
        if let Some(crate::features::VertexSelection::Generated { vertex, .. }) =
            termination.vertex()
        {
            references.insert_unique(vertex.feature.as_str(), &vertex.feature)?;
        }
    }
    drop(terminations);
    let profiles = definition_profiles(ctx, definition)?;
    for profile in ctx
        .admit_iter(&profiles[..], "topology validation scan")?
        .copied()
    {
        match profile {
            crate::features::PlanarProfileRef::Feature(feature) => {
                references.insert_unique(feature.as_str(), feature)?;
            }
            crate::features::PlanarProfileRef::Generated { curves, .. } => {
                for curve in ctx.admit_iter(curves.as_slice(), "regeneration reference scan")? {
                    references.insert_unique(curve.feature.as_str(), &curve.feature)?;
                }
            }
            _ => {}
        }
    }
    drop(profiles);
    let mut ordered =
        Scratch::filter_map(ctx, references.values(), |reference| Ok(Some(*reference)))?;
    ordered.stable_sort_by(|reference| *reference, Ord::cmp)?;
    Ok(ordered)
}

fn definition_profiles<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    definition: &'a crate::features::FeatureOperation,
) -> Result<Scratch<'ctx, &'a crate::features::PlanarProfileRef>, CodecError> {
    let mut profiles = Scratch::new(ctx)?;
    match definition {
        crate::features::FeatureOperation::Extrude { profile, .. } => {
            profiles.extend(profile.planar().as_slice(), |profile| *profile)?;
        }
        crate::features::FeatureOperation::SheetMetalBaseFlange { profile, .. }
        | crate::features::FeatureOperation::Wrap { profile, .. } => profiles.push(profile)?,
        crate::features::FeatureOperation::Revolve { construction, .. } => {
            profiles.extend(construction.profile().as_slice(), |profile| *profile)?;
        }
        crate::features::FeatureOperation::Rib { construction, .. } => {
            profiles.extend(construction.profile.as_ref().as_slice(), |profile| *profile)?;
        }
        crate::features::FeatureOperation::Sweep { shape, .. } => {
            ctx.charge_work(1, "primary sweep profile scan")?;
            match shape {
                crate::features::SweepShape::Unresolved { section, sections }
                | crate::features::SweepShape::Surface { section, sections } => {
                    if let Some(profile) = section.referenced_profile() {
                        profiles.push(profile)?;
                    }
                    for section in ctx.admit_iter(sections, "additional sweep profile scan")? {
                        if let Some(profile) = section.referenced_profile() {
                            profiles.push(profile)?;
                        }
                    }
                }
                crate::features::SweepShape::Solid {
                    section, sections, ..
                } => {
                    if let Some(profile) = section.referenced_profile() {
                        profiles.push(profile)?;
                    }
                    for section in ctx.admit_iter(sections, "additional sweep profile scan")? {
                        if let Some(profile) = section.referenced_profile() {
                            profiles.push(profile)?;
                        }
                    }
                }
            }
        }
        crate::features::FeatureOperation::HelicalSweep { construction, .. } => {
            profiles.push(&construction.profile)?;
        }
        crate::features::FeatureOperation::Loft { sections, .. } => {
            for section in sections {
                ctx.charge_work(1, "planar loft profile scan")?;
                if let crate::features::LoftSection::Profile(profile) = section {
                    profiles.extend(profile.planar().as_slice(), |profile| *profile)?;
                }
            }
        }
        crate::features::FeatureOperation::Hole {
            profile: Some(profile),
            ..
        } => profiles.push(profile)?,
        _ => {}
    }
    Ok(profiles)
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

fn definition_terminations<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'_>,
    definition: &'a crate::features::FeatureOperation,
) -> Result<Scratch<'ctx, TerminationRef<'a>>, CodecError> {
    let mut terminations = Scratch::new(ctx)?;
    match definition {
        crate::features::FeatureOperation::Extrude { extent, .. } => match extent {
            crate::features::ExtrudeExtent::OneSided { side }
            | crate::features::ExtrudeExtent::Symmetric { side } => {
                terminations.push(TerminationRef::Linear(&side.termination))?;
            }
            crate::features::ExtrudeExtent::TwoSided { first, second } => {
                terminations.extend(
                    &[
                        TerminationRef::Linear(&first.termination),
                        TerminationRef::Linear(&second.termination),
                    ],
                    |termination| *termination,
                )?;
            }
        },
        crate::features::FeatureOperation::Revolve { construction, .. } => {
            match construction.extent() {
                Some(
                    crate::features::RevolveExtent::OneSided { termination }
                    | crate::features::RevolveExtent::Symmetric { termination },
                ) => terminations.push(TerminationRef::Angular(termination))?,
                Some(crate::features::RevolveExtent::TwoSided { first, second }) => {
                    terminations.extend(
                        &[
                            TerminationRef::Angular(first),
                            TerminationRef::Angular(second),
                        ],
                        |termination| *termination,
                    )?;
                }
                None => {}
            }
        }
        crate::features::FeatureOperation::Hole {
            extent: Some(extent),
            ..
        } => terminations.push(TerminationRef::Linear(extent))?,
        _ => {}
    }
    Ok(terminations)
}

fn check_configuration_state_closure(
    ctx: &DecodeContext<'_>,
    configuration: &crate::features::DesignConfiguration,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    if configuration.feature_states.is_empty() {
        return Ok(());
    }
    let states = BorrowedIdentities::build(ctx, |add| {
        for (feature, state) in ctx.admit_iter(
            &configuration.feature_states,
            "configuration state index scan",
        )? {
            add(feature.as_str(), state)?;
        }
        Ok(())
    })?;
    let mut closure = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut pending = Scratch::new(ctx)?;
    for (feature, state) in &configuration.feature_states {
        ctx.charge_work(1, "configuration closure seed scan")?;
        if !state.evaluation.is_suppressed() && closure.insert_unique(feature.as_str(), ())? {
            pending.push(feature.as_str())?;
        }
    }
    while let Some(feature) = pending.pop() {
        ctx.charge_work(1, "configuration closure traversal")?;
        let Some(state) = states.get(ctx, feature)? else {
            continue;
        };
        for dependency in ctx.admit_iter(
            state.dependencies.as_slice(),
            "configuration dependency traversal",
        )? {
            match states.get(ctx, dependency.as_str())? {
                None => {}
                Some(dependency_state) if dependency_state.evaluation.is_suppressed() => {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(configuration.id.as_str()),
                        format_args!(
                            "configuration state closure uses suppressed dependency state `{}`",
                            dependency.as_str()
                        ),
                    )?;
                }
                Some(_) if closure.insert_unique(dependency.as_str(), ())? => {
                    pending.push(dependency.as_str())?;
                }
                Some(_) => {}
            }
        }
    }

    Ok(())
}

fn check_plane_feature_reference(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    feature: &Feature,
    reference: &crate::features::FeatureId,
    feature_records: &BorrowedIdentities<'_, '_, &Feature>,
    reference_kind: &str,
) -> Result<(), CodecError> {
    match feature_records.get(ctx, reference.as_str())? {
        None => ref_error(
            ctx,
            findings,
            feature.id.as_str(),
            reference_kind,
            reference.as_str(),
        )?,
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
            geometry_error(
                ctx,
                findings,
                feature.id.as_str(),
                "feature reference does not name a datum plane",
            )?;
        }
        Some(record) if record.ordinal >= feature.ordinal => super::record_finding(
            ctx,
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            Some(feature.id.as_str()),
            format_args!(
                "{reference_kind} `{}` does not precede its consuming feature",
                reference.as_str()
            ),
        )?,
        Some(_)
            if !super::scans::any(ctx, feature.dependencies.iter(), |dependency| {
                Ok(crate::ids::comparison::equal(
                    ctx,
                    dependency.as_str(),
                    reference.as_str(),
                    "feature dependency comparison",
                )?)
            })? =>
        {
            super::record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(feature.id.as_str()),
                format_args!(
                    "feature omits {reference_kind} dependency `{}`",
                    reference.as_str()
                ),
            )?;
        }
        Some(_) => {}
    }

    Ok(())
}

fn geometry_error(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    entity: &str,
    message: impl fmt::Display,
) -> Result<(), CodecError> {
    super::record_finding(
        ctx,
        findings,
        Check::GeometricConsistency,
        Severity::Error,
        Some(entity),
        format_args!("{message}"),
    )
}

fn check_ids<'a>(
    ctx: &DecodeContext<'_>,
    findings: &mut Vec<Finding>,
    owner: &str,
    kind: &str,
    values: impl Iterator<Item = &'a str>,
    valid: impl Fn(&str) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    for value in values {
        ctx.charge_work(1, "selected identity validation")?;
        if !valid(value)? {
            ref_error(ctx, findings, owner, kind, value)?;
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ProfileReference<'a> {
    Planar(&'a crate::features::PlanarProfileRef),
    SpatialSketchProfiles {
        sketch: &'a crate::sketches::SpatialSketchId,
        profiles: &'a [u32],
    },
    SpatialSketchSelection {
        sketch: &'a crate::sketches::SpatialSketchId,
    },
}

impl<'a> From<&'a crate::features::ProfileRef> for ProfileReference<'a> {
    fn from(profile: &'a crate::features::ProfileRef) -> Self {
        match profile {
            crate::features::ProfileRef::Planar(profile) => Self::Planar(profile),
            crate::features::ProfileRef::SpatialSketchProfiles { sketch, profiles } => {
                Self::SpatialSketchProfiles { sketch, profiles }
            }
            crate::features::ProfileRef::SpatialSketchSelection { sketch, .. } => {
                Self::SpatialSketchSelection { sketch }
            }
        }
    }
}

fn check_feature_sketch_references(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    sketches: &BorrowedIdentities<'_, '_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    use crate::features::{
        FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef, SketchPointSelection,
    };

    let spatial_sketches = BorrowedIdentities::build(ctx, |add| {
        for sketch in ctx.admit_iter(&ir.model.spatial_sketches, "spatial sketch identity scan")? {
            add(sketch.id.as_str(), ())?;
        }
        Ok(())
    })?;
    let sketch_entity_owners = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(&ir.model.sketch_entities, "sketch entity owner scan")? {
            let (identity, value) = (entity.id().as_str(), entity.sketch.as_str());
            add(identity, value)?;
        }
        Ok(())
    })?;
    let spatial_sketch_entity_owners = BorrowedIdentities::build(ctx, |add| {
        for entity in ctx.admit_iter(
            &ir.model.spatial_sketch_entities,
            "spatial sketch entity owner scan",
        )? {
            let (identity, value) = (entity.id().as_str(), entity.sketch.as_str());
            add(identity, value)?;
        }
        Ok(())
    })?;
    let mut owners = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for feature in &ir.model.features {
        ctx.charge_work(1, "topology validation scan")?;
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
            .insert(sketch, (feature.id.as_str(), feature.ordinal))?
            .is_some()
        {
            super::record_finding(
                ctx,
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(feature.id.as_str()),
                format_args!("sketch `{sketch}` has multiple owning features"),
            )?;
        }
    }

    for feature in &ir.model.features {
        ctx.charge_work(1, "topology validation scan")?;
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
                non_blank_native_reference(ctx, native)?
                    && sketches.contains(ctx, sketch.as_str())?
                    && sketch_entity_owners.get(ctx, point.as_str())?.map_or(
                        Ok::<_, CodecError>(false),
                        |owner| {
                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                ctx,
                                owner,
                                sketch.as_str(),
                                "profile sketch owner",
                            )?)
                        },
                    )?
                    && super::scans::any(ctx, ir.model.sketch_entities.iter(), |entity| {
                        Ok::<_, CodecError>({
                            crate::ids::comparison::equal(
                                ctx,
                                entity.id().as_str(),
                                point.as_str(),
                                "sketch point identity",
                            )? && crate::ids::comparison::equal(
                                ctx,
                                entity.sketch.as_str(),
                                sketch.as_str(),
                                "sketch ownership comparison",
                            )? && matches!(
                                entity.geometry.definition(),
                                crate::sketches::SketchGeometryDefinition::Point { .. }
                            )
                        })
                    })?
            }
            SketchPointSelection::Spatial {
                sketch,
                point,
                native,
            } => {
                non_blank_native_reference(ctx, native)?
                    && spatial_sketches.contains(ctx, sketch.as_str())?
                    && spatial_sketch_entity_owners
                        .get(ctx, point.as_str())?
                        .map_or(Ok::<_, CodecError>(false), |owner| {
                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                ctx,
                                owner,
                                sketch.as_str(),
                                "profile sketch owner",
                            )?)
                        })?
                    && super::scans::any(ctx, ir.model.spatial_sketch_entities.iter(), |entity| {
                        Ok::<_, CodecError>({
                            crate::ids::comparison::equal(
                                ctx,
                                entity.id().as_str(),
                                point.as_str(),
                                "sketch point identity",
                            )? && crate::ids::comparison::equal(
                                ctx,
                                entity.sketch.as_str(),
                                sketch.as_str(),
                                "sketch ownership comparison",
                            )? && matches!(
                                entity.geometry.definition(),
                                crate::sketches::SpatialSketchGeometryDefinition::Point { .. }
                            )
                        })
                    })?
            }
            SketchPointSelection::Native(native) => non_blank_native_reference(ctx, native)?,
            SketchPointSelection::Unresolved => true,
        };
        if !valid {
            geometry_error(
                ctx,
                findings,
                feature.id.as_str(),
                "datum-point sketch-point selection is invalid",
            )?;
        }
        let sketch_id = match point {
            SketchPointSelection::Planar { sketch, .. } => Some(sketch.as_str()),
            SketchPointSelection::Spatial { sketch, .. } => Some(sketch.as_str()),
            SketchPointSelection::Native(_) | SketchPointSelection::Unresolved => None,
        };
        if let Some(sketch_id) = sketch_id {
            if let Some((owner, ordinal)) = owners.get(ctx, sketch_id)? {
                if *ordinal >= feature.ordinal {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(feature.id.as_str()),
                        format_args!(
                            "sketch owner `{owner}` does not precede its datum-point consumer"
                        ),
                    )?;
                }
            }
        }
    }

    for feature in &ir.model.features {
        ctx.charge_work(1, "topology validation scan")?;
        let mut profiles = Scratch::new(ctx)?;
        let mut paths = Scratch::new(ctx)?;
        let definition = match feature.evaluation.definition() {
            FeatureDefinition::PostProcess { operation, .. }
            | FeatureDefinition::Operation(operation) => operation,
        };
        match definition {
            FeatureOperation::Extrude { profile, .. } => {
                profiles.push(ProfileReference::from(profile))?;
            }
            FeatureOperation::SheetMetalBaseFlange { profile, .. } => {
                profiles.push(ProfileReference::Planar(profile))?;
            }
            FeatureOperation::Rib { construction, .. } => {
                profiles.extend(
                    construction
                        .profile
                        .as_ref()
                        .map(ProfileReference::Planar)
                        .as_slice(),
                    |profile| *profile,
                )?;
            }
            FeatureOperation::Revolve { construction, .. } => {
                profiles.extend(
                    construction
                        .profile()
                        .map(ProfileReference::Planar)
                        .as_slice(),
                    |profile| *profile,
                )?;
                paths.extend(
                    construction
                        .axis()
                        .and_then(|axis| axis.reference.as_ref())
                        .as_slice(),
                    |path| *path,
                )?;
            }
            FeatureOperation::Sweep {
                shape,
                path,
                guide_rail,
                ..
            } => {
                ctx.charge_work(1, "primary sweep profile scan")?;
                match shape {
                    crate::features::SweepShape::Unresolved { section, sections }
                    | crate::features::SweepShape::Surface { section, sections } => {
                        if let Some(profile) = section.referenced_profile() {
                            profiles.push(ProfileReference::Planar(profile))?;
                        }
                        for section in ctx.admit_iter(sections, "additional sweep profile scan")? {
                            if let Some(profile) = section.referenced_profile() {
                                profiles.push(ProfileReference::Planar(profile))?;
                            }
                        }
                    }
                    crate::features::SweepShape::Solid {
                        section, sections, ..
                    } => {
                        if let Some(profile) = section.referenced_profile() {
                            profiles.push(ProfileReference::Planar(profile))?;
                        }
                        for section in ctx.admit_iter(sections, "additional sweep profile scan")? {
                            if let Some(profile) = section.referenced_profile() {
                                profiles.push(ProfileReference::Planar(profile))?;
                            }
                        }
                    }
                }
                paths.extend(path.as_slice(), |path| path)?;
                if let Some(guide_rail) = guide_rail {
                    paths.push(&guide_rail.path)?;
                }
            }
            FeatureOperation::HelicalSweep { construction, .. } => {
                profiles.push(ProfileReference::Planar(&construction.profile))?;
            }
            FeatureOperation::Loft {
                sections, guidance, ..
            } => {
                for section in sections {
                    ctx.charge_work(1, "loft profile reference scan")?;
                    if let crate::features::LoftSection::Profile(profile) = section {
                        profiles.push(ProfileReference::from(profile))?;
                    }
                }
                match guidance {
                    crate::features::LoftGuidance::Guides(guides) => {
                        paths.extend(guides, |guide| guide)?;
                    }
                    crate::features::LoftGuidance::Centerline(centerline) => {
                        paths.push(centerline)?;
                    }
                }
            }
            FeatureOperation::Pattern { pattern, .. } => {
                collect_pattern_paths(ctx, pattern, &mut paths)?;
            }
            _ => {}
        }
        for profile in ctx
            .admit_iter(&profiles[..], "topology validation scan")?
            .copied()
        {
            let (sketch, sketch_kind, defined_sketches) = match profile {
                ProfileReference::SpatialSketchProfiles { sketch, .. }
                | ProfileReference::SpatialSketchSelection { sketch, .. } => {
                    (sketch.as_str(), "spatial sketch", &spatial_sketches)
                }
                ProfileReference::Planar(
                    PlanarProfileRef::Sketch(sketch)
                    | PlanarProfileRef::SketchProfiles { sketch, .. }
                    | PlanarProfileRef::SketchRegions { sketch, .. }
                    | PlanarProfileRef::SketchEntities { sketch, .. }
                    | PlanarProfileRef::SketchSelection { sketch, .. },
                ) => (sketch.as_str(), "sketch", sketches),
                ProfileReference::Planar(_) => continue,
            };
            if !defined_sketches.contains(ctx, sketch)? {
                ref_error(
                    ctx,
                    findings,
                    feature.id.as_str(),
                    format_args!("{sketch_kind} profile"),
                    sketch,
                )?;
            } else if let Some((owner, ordinal)) = owners.get(ctx, sketch)? {
                if *ordinal >= feature.ordinal {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(feature.id.as_str()),
                        format_args!(
                            "{sketch_kind} owner `{owner}` does not precede its profile consumer"
                        ),
                    )?;
                }
            }
            match profile {
                ProfileReference::SpatialSketchProfiles { sketch, profiles } => {
                    let profile_count =
                        super::scans::find(ctx, ir.model.spatial_sketches.iter(), |candidate| {
                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                ctx,
                                candidate.id.as_str(),
                                sketch.as_str(),
                                "topology reference comparison",
                            )?)
                        })?
                        .map_or(0, |sketch| sketch.profiles.len());
                    if super::scans::any(ctx, profiles.iter(), |index| {
                        Ok::<_, CodecError>(
                            cadmpeg_core::decode::index_from_u32(*index) >= profile_count,
                        )
                    })? {
                        geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "spatial sketch profile indices are empty, repeated, or out of range",
                        )?;
                    }
                }
                ProfileReference::Planar(PlanarProfileRef::SketchProfiles { sketch, profiles }) => {
                    let sketch_profile_count =
                        super::scans::find(ctx, ir.model.sketches.iter(), |candidate| {
                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                ctx,
                                candidate.id.as_str(),
                                sketch.as_str(),
                                "topology reference comparison",
                            )?)
                        })?
                        .map_or(0, |sketch| sketch.profiles.len());
                    if super::scans::any(ctx, profiles.iter(), |index| {
                        Ok::<_, CodecError>({
                            cadmpeg_core::decode::index_from_u32(*index) >= sketch_profile_count
                        })
                    })? {
                        geometry_error(
                            ctx,
                            findings,
                            feature.id.as_str(),
                            "sketch profile indices are empty, repeated, or out of range",
                        )?;
                    }
                }
                ProfileReference::Planar(PlanarProfileRef::SketchRegions { sketch, regions }) => {
                    let selected_sketch =
                        super::scans::find(ctx, ir.model.sketches.iter(), |candidate| {
                            Ok::<_, CodecError>(crate::ids::comparison::equal(
                                ctx,
                                candidate.id.as_str(),
                                sketch.as_str(),
                                "topology reference comparison",
                            )?)
                        })?;
                    let sketch_profile_count =
                        selected_sketch.map_or(0, |sketch| sketch.profiles.len());
                    let invalid = super::scans::any(ctx, regions.iter(), |region| {
                        Ok::<_, CodecError>(match region {
                            crate::features::SketchProfileRegion::Loops { loops } => {
                                cadmpeg_core::decode::index_from_u32(loops.outer())
                                    >= sketch_profile_count
                                    || super::scans::any(ctx, loops.holes().iter(), |index| {
                                        Ok::<_, CodecError>({
                                            cadmpeg_core::decode::index_from_u32(*index)
                                                >= sketch_profile_count
                                        })
                                    })?
                            }
                            crate::features::SketchProfileRegion::Trimmed {
                                outer_boundary,
                                hole_boundaries,
                            } => {
                                let valid_ring =
                                |ring: &[crate::features::SketchProfileBoundaryUse]| -> Result<bool, CodecError> {
                                    super::scans::all(ctx, ring.iter(), |use_| Ok::<_, CodecError>({
                                        super::scans::any(ctx, ir.model.sketch_entities.iter(), |entity| Ok::<_, CodecError>({
                                            crate::ids::comparison::equal(ctx, entity.id().as_str(), use_.entity.as_str(), "sketch boundary identity")? && crate::ids::comparison::equal(ctx, entity.sketch.as_str(), sketch.as_str(), "sketch ownership comparison")?
                                        }))?
                                    }))
                                };
                                !valid_ring(outer_boundary)?
                                    || super::scans::any(ctx, hole_boundaries.iter(), |ring| {
                                        Ok::<_, CodecError>(!valid_ring(ring)?)
                                    })?
                            }
                        })
                    })?;
                    if invalid {
                        geometry_error(ctx, findings, feature.id.as_str(), "sketch regions have empty, repeated, invalid, or out-of-range boundaries")?;
                    }
                }
                ProfileReference::Planar(PlanarProfileRef::SketchEntities { sketch, entities }) => {
                    if super::scans::any(ctx, entities.iter(), |entity| {
                        Ok::<_, CodecError>({
                            sketch_entity_owners.get(ctx, entity.as_str())?.map_or(
                                Ok::<_, CodecError>(true),
                                |owner| {
                                    Ok::<_, CodecError>(!crate::ids::comparison::equal(
                                        ctx,
                                        owner,
                                        sketch.as_str(),
                                        "profile sketch owner",
                                    )?)
                                },
                            )?
                        })
                    })? {
                        geometry_error(ctx, findings, feature.id.as_str(), "sketch profile entities are empty, repeated, missing, or owned by another sketch")?;
                    }
                }
                ProfileReference::Planar(
                    PlanarProfileRef::Native(_)
                    | PlanarProfileRef::Unresolved(_)
                    | PlanarProfileRef::Feature(_)
                    | PlanarProfileRef::Generated { .. }
                    | PlanarProfileRef::Sketch(_)
                    | PlanarProfileRef::SketchSelection { .. }
                    | PlanarProfileRef::HistoricalFaces { .. }
                    | PlanarProfileRef::Faces(_),
                )
                | ProfileReference::SpatialSketchSelection { .. } => {}
            }
        }
        drop(profiles);
        for path in ctx
            .admit_iter(&paths[..], "topology validation scan")?
            .copied()
        {
            if let PathRef::SketchCurves { sketch, curves } = path {
                let invalid = super::scans::any(ctx, curves.iter(), |curve| {
                    Ok::<_, CodecError>({
                        sketch_entity_owners.get(ctx, curve.as_str())?.map_or(
                            Ok::<_, CodecError>(true),
                            |owner| {
                                Ok::<_, CodecError>(!crate::ids::comparison::equal(
                                    ctx,
                                    owner,
                                    sketch.as_str(),
                                    "profile sketch owner",
                                )?)
                            },
                        )?
                    })
                })?;
                if invalid {
                    geometry_error(
                        ctx,
                        findings,
                        feature.id.as_str(),
                        "sketch path curves are empty, repeated, or owned by another sketch",
                    )?;
                }
            }
            let (sketch, known_sketches, description) = match path {
                PathRef::Sketch(sketch) => (sketch.as_str(), sketches, "sketch path"),
                PathRef::SketchCurves { sketch, .. } => {
                    (sketch.as_str(), sketches, "sketch curve path")
                }
                PathRef::SpatialSketchCurves { sketch, curves } => {
                    let invalid = super::scans::any(ctx, curves.iter(), |curve| {
                        Ok::<_, CodecError>({
                            spatial_sketch_entity_owners
                                .get(ctx, curve.as_str())?
                                .map_or(Ok::<_, CodecError>(true), |owner| {
                                    Ok::<_, CodecError>(!crate::ids::comparison::equal(
                                        ctx,
                                        owner,
                                        sketch.as_str(),
                                        "profile sketch owner",
                                    )?)
                                })?
                        })
                    })?;
                    if invalid {
                        geometry_error(ctx, findings, feature.id.as_str(), "spatial sketch path curves are empty, repeated, missing, or owned by another sketch")?;
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
            if !known_sketches.contains(ctx, sketch)? {
                ref_error(ctx, findings, feature.id.as_str(), description, sketch)?;
            } else if let Some((owner, ordinal)) = owners.get(ctx, sketch)? {
                if *ordinal >= feature.ordinal {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(feature.id.as_str()),
                        format_args!("sketch owner `{owner}` does not precede its path consumer"),
                    )?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
