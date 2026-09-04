// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.
#![allow(clippy::wildcard_imports)]

use std::collections::BTreeSet;

use super::*;
use crate::features::{
    BodySelection, ChamferSpec, DatumPlaneReference, ExtrudeStart, FaceMotion, FaceSelection,
    FeatureSourceContent, FlexMode, HoleKind, Length, PatternKind, PatternSeed,
    PatternStageCombination, PrimitiveSolid, RadiusSpec, SplitFaceTool,
};
use crate::math::Point3;

const EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9: f64 = 1.0e-9;

const EPS_TORUS_AXES_ORTHO: f64 = 1.0e-9;

fn pattern_is_valid(pattern: &PatternKind, nested: bool) -> bool {
    match pattern {
        PatternKind::Unresolved { .. } => true,
        PatternKind::Linear {
            direction,
            spacing,
            count,
            second,
        } => {
            direction.is_none_or(valid_feature_direction)
                && positive_feature_length(*spacing)
                && *count > 0
                && second.as_ref().is_none_or(|second| {
                    valid_feature_direction(second.direction)
                        && positive_feature_length(second.spacing)
                        && second.count > 0
                })
        }
        PatternKind::LinearOffsets { direction, offsets } => {
            direction.is_none_or(valid_feature_direction)
                && valid_increasing_locations(offsets.iter().map(|offset| offset.0))
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
        PatternKind::CircularAngles {
            axis_origin,
            axis_dir,
            angles,
        } => {
            axis_origin.x.is_finite()
                && axis_origin.y.is_finite()
                && axis_origin.z.is_finite()
                && valid_feature_direction(*axis_dir)
                && valid_increasing_locations(angles.iter().map(|angle| angle.0))
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
        PatternKind::MirrorReference { plane } => match plane {
            FaceSelection::Native(reference) => !reference.is_empty(),
            _ => true,
        },
        PatternKind::Scale {
            center,
            final_factor,
            count,
        } => {
            let center_valid = match center {
                crate::features::PatternScaleCenter::Point(point) => {
                    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
                }
                crate::features::PatternScaleCenter::FirstSeedCentroid
                | crate::features::PatternScaleCenter::Native(_) => true,
            };
            center_valid && final_factor.is_finite() && *final_factor > 0.0 && *count >= 2
        }
        PatternKind::Composite { stages } => {
            let structure_valid = !nested
                && !stages.is_empty()
                && stages.iter().enumerate().all(|(index, stage)| {
                    stage.combination
                        == if index == 0 {
                            PatternStageCombination::Initialize
                        } else if matches!(*stage.pattern, PatternKind::Scale { .. }) {
                            PatternStageCombination::AlignedSlices
                        } else {
                            PatternStageCombination::CartesianProduct
                        }
                        && pattern_is_valid(&stage.pattern, true)
                        && !matches!(*stage.pattern, PatternKind::Composite { .. })
                });
            structure_valid && composite_composition_is_valid(stages)
        }
    }
}

fn composite_composition_is_valid(stages: &[crate::features::PatternStage]) -> bool {
    let mut occurrences = None;
    stages.iter().enumerate().all(|(index, stage)| {
        let Some(stage_count) = pattern_occurrence_count(&stage.pattern) else {
            return true;
        };
        if stage_count == 0 {
            return false;
        }
        if index == 0 {
            occurrences = Some(stage_count);
            return true;
        }
        match stage.combination {
            PatternStageCombination::CartesianProduct => {
                if let Some(count) = occurrences {
                    occurrences = count.checked_mul(stage_count);
                    occurrences.is_some()
                } else {
                    true
                }
            }
            PatternStageCombination::AlignedSlices => {
                occurrences.is_none_or(|count| count % stage_count == 0)
            }
            PatternStageCombination::Initialize => false,
        }
    })
}

fn pattern_occurrence_count(pattern: &PatternKind) -> Option<usize> {
    match pattern {
        PatternKind::Linear { count, .. }
        | PatternKind::Circular { count, .. }
        | PatternKind::CurveDriven { count, .. }
        | PatternKind::Scale { count, .. } => usize::try_from(*count).ok(),
        PatternKind::LinearOffsets { offsets, .. } => Some(offsets.len()),
        PatternKind::CircularAngles { angles, .. } => Some(angles.len()),
        PatternKind::Mirror { .. } | PatternKind::MirrorReference { .. } => Some(2),
        PatternKind::Unresolved { .. } | PatternKind::Composite { .. } => None,
    }
}

fn valid_increasing_locations(locations: impl Iterator<Item = f64>) -> bool {
    let mut locations = locations;
    let Some(first) = locations.next() else {
        return false;
    };
    first == 0.0
        && locations
            .try_fold(first, |previous, location| {
                (location.is_finite() && location > previous).then_some(location)
            })
            .is_some()
}

fn collect_pattern_paths<'a>(
    pattern: &'a PatternKind,
    paths: &mut Vec<&'a crate::features::PathRef>,
) {
    match pattern {
        PatternKind::CurveDriven {
            path: Some(path), ..
        } => paths.push(path),
        PatternKind::Composite { stages } => {
            for stage in stages {
                collect_pattern_paths(&stage.pattern, paths);
            }
        }
        _ => {}
    }
}
use crate::index::ModelIndex;
use crate::sketches::{SketchConstraintDefinition as Definition, SketchLocus};

pub(super) fn ref_error(findings: &mut Vec<Finding>, owner: &str, target_kind: &str, target: &str) {
    findings.push(Finding {
        check: Check::ReferentialIntegrity,
        severity: Severity::Error,
        message: format!("references missing {target_kind} `{target}`"),
        entity: Some(owner.to_string()),
    });
}

pub(super) fn check_tolerances(ir: &CadIr, findings: &mut Vec<Finding>) {
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

pub(super) fn check_references(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    for b in &ir.model.bodies {
        for l in &b.regions {
            if ids.regions(&l.0).is_none() {
                ref_error(findings, &b.id.0, "region", &l.0);
            }
        }
    }
    for l in &ir.model.regions {
        if ids.bodies(&l.body.0).is_none() {
            ref_error(findings, &l.id.0, "body", &l.body.0);
        }
        for s in &l.shells {
            if ids.shells(&s.0).is_none() {
                ref_error(findings, &l.id.0, "shell", &s.0);
            }
        }
    }
    for s in &ir.model.shells {
        if ids.regions(&s.region.0).is_none() {
            ref_error(findings, &s.id.0, "region", &s.region.0);
        }
        for f in &s.faces {
            if ids.faces(&f.0).is_none() {
                ref_error(findings, &s.id.0, "face", &f.0);
            }
        }
        for e in &s.wire_edges {
            if ids.edges(&e.0).is_none() {
                ref_error(findings, &s.id.0, "wire edge", &e.0);
            }
        }
        for v in &s.free_vertices {
            if ids.vertices(&v.0).is_none() {
                ref_error(findings, &s.id.0, "free vertex", &v.0);
            }
        }
    }
    for f in &ir.model.faces {
        if ids.shells(&f.shell.0).is_none() {
            ref_error(findings, &f.id.0, "shell", &f.shell.0);
        }
        if ids.surfaces(&f.surface.0).is_none() {
            ref_error(findings, &f.id.0, "surface", &f.surface.0);
        }
        for lp in &f.loops {
            if ids.loops(&lp.0).is_none() {
                ref_error(findings, &f.id.0, "loop", &lp.0);
            }
        }
    }
    for lp in &ir.model.loops {
        if ids.faces(&lp.face.0).is_none() {
            ref_error(findings, &lp.id.0, "face", &lp.face.0);
        }
        match &lp.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, pcurves } => {
                if ids.vertices(&vertex.0).is_none() {
                    ref_error(findings, &lp.id.0, "vertex", &vertex.0);
                }
                for pcurve in pcurves {
                    if ids.pcurves(&pcurve.pcurve.0).is_none() {
                        ref_error(findings, &lp.id.0, "pcurve(vertex use)", &pcurve.pcurve.0);
                    }
                }
            }
            crate::topology::LoopBoundary::Ring {
                coedges,
                vertex_uses,
            } => {
                for ce in coedges {
                    if ids.coedges(&ce.0).is_none() {
                        ref_error(findings, &lp.id.0, "coedge", &ce.0);
                    }
                }
                for use_ in vertex_uses {
                    if ids.vertices(&use_.vertex.0).is_none() {
                        ref_error(findings, &lp.id.0, "vertex", &use_.vertex.0);
                    }
                    let after = &use_.after;
                    if ids.coedges(&after.0).is_none() {
                        ref_error(findings, &lp.id.0, "coedge(vertex-use after)", &after.0);
                    }
                    for pcurve in &use_.pcurves {
                        if ids.pcurves(&pcurve.pcurve.0).is_none() {
                            ref_error(findings, &lp.id.0, "pcurve(vertex use)", &pcurve.pcurve.0);
                        }
                    }
                }
            }
        }
    }
    for ce in &ir.model.coedges {
        if ids.loops(&ce.owner_loop.0).is_none() {
            ref_error(findings, &ce.id.0, "loop", &ce.owner_loop.0);
        }
        if ids.edges(&ce.edge.0).is_none() {
            ref_error(findings, &ce.id.0, "edge", &ce.edge.0);
        }
        if ids.coedges(&ce.next.0).is_none() {
            ref_error(findings, &ce.id.0, "coedge(next)", &ce.next.0);
        }
        if ids.coedges(&ce.previous.0).is_none() {
            ref_error(findings, &ce.id.0, "coedge(previous)", &ce.previous.0);
        }
        if ids.coedges(&ce.radial_next.0).is_none() {
            ref_error(findings, &ce.id.0, "coedge(radial_next)", &ce.radial_next.0);
        }
        for use_ in &ce.pcurves {
            if ids.pcurves(&use_.pcurve.0).is_none() {
                ref_error(findings, &ce.id.0, "pcurve", &use_.pcurve.0);
            }
        }
        if let Some(curve) = &ce.use_curve {
            if ids.curves(&curve.curve.0).is_none() {
                ref_error(findings, &ce.id.0, "coedge use curve", &curve.curve.0);
            }
        }
    }
    for e in &ir.model.edges {
        if let Some(c) = &e.curve {
            if ids.curves(&c.0).is_none() {
                ref_error(findings, &e.id.0, "curve", &c.0);
            }
        }
        if ids.vertices(&e.start.0).is_none() {
            ref_error(findings, &e.id.0, "vertex(start)", &e.start.0);
        }
        if ids.vertices(&e.end.0).is_none() {
            ref_error(findings, &e.id.0, "vertex(end)", &e.end.0);
        }
    }
    for v in &ir.model.vertices {
        if ids.points(&v.point.0).is_none() {
            ref_error(findings, &v.id.0, "point", &v.point.0);
        }
    }
    for binding in &ir.model.appearance_bindings {
        use crate::appearance::AppearanceTarget;
        let owner = format!("appearance-binding:{}", binding.appearance.0);
        if ids.appearances(&binding.appearance.0).is_none() {
            ref_error(findings, &owner, "appearance", &binding.appearance.0);
        }
        match &binding.target {
            AppearanceTarget::Body(body) if ids.bodies(&body.0).is_none() => {
                ref_error(findings, &owner, "body", &body.0);
            }
            AppearanceTarget::Face(face) if ids.faces(&face.0).is_none() => {
                ref_error(findings, &owner, "face", &face.0);
            }
            AppearanceTarget::Edge(edge) if ids.edges(&edge.0).is_none() => {
                ref_error(findings, &owner, "edge", &edge.0);
            }
            AppearanceTarget::Vertex(vertex) if ids.vertices(&vertex.0).is_none() => {
                ref_error(findings, &owner, "vertex", &vertex.0);
            }
            AppearanceTarget::Surface(surface) if ids.surfaces(&surface.0).is_none() => {
                ref_error(findings, &owner, "surface", &surface.0);
            }
            AppearanceTarget::Curve(curve) if ids.curves(&curve.0).is_none() => {
                ref_error(findings, &owner, "curve", &curve.0);
            }
            AppearanceTarget::Point(point) if ids.points(&point.0).is_none() => {
                ref_error(findings, &owner, "point", &point.0);
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
        let owner = &attribute.id.0;
        match &attribute.target {
            AttributeTarget::Document => {}
            AttributeTarget::Body(id) if ids.bodies(&id.0).is_none() => {
                ref_error(findings, owner, "body", &id.0);
            }
            AttributeTarget::Face(id) if ids.faces(&id.0).is_none() => {
                ref_error(findings, owner, "face", &id.0);
            }
            AttributeTarget::Coedge(id) if ids.coedges(&id.0).is_none() => {
                ref_error(findings, owner, "coedge", &id.0);
            }
            AttributeTarget::Edge(id) if ids.edges(&id.0).is_none() => {
                ref_error(findings, owner, "edge", &id.0);
            }
            AttributeTarget::Vertex(id) if ids.vertices(&id.0).is_none() => {
                ref_error(findings, owner, "vertex", &id.0);
            }
            _ => {}
        }
    }
    for s in &ir.model.surfaces {
        match &s.geometry {
            SurfaceGeometry::Procedural { construction, .. } => {
                if ids.procedural_surfaces(&construction.0).is_none() {
                    ref_error(
                        findings,
                        &s.id.0,
                        "procedural surface construction",
                        &construction.0,
                    );
                }
            }
            SurfaceGeometry::Unknown { record: Some(u) } if !ids.contains(&u.0) => {
                ref_error(findings, &s.id.0, "unknown record", &u.0);
            }
            _ => {}
        }
    }
    for curve in &ir.model.curves {
        match &curve.geometry {
            CurveGeometry::Procedural { construction, .. } => {
                if ids.procedural_curves(&construction.0).is_none() {
                    ref_error(
                        findings,
                        &curve.id.0,
                        "procedural curve construction",
                        &construction.0,
                    );
                }
            }
            CurveGeometry::Unknown {
                record: Some(unknown),
            } => {
                if !ids.contains(&unknown.0) {
                    ref_error(findings, &curve.id.0, "unknown record", &unknown.0);
                }
            }
            CurveGeometry::Composite { segments, .. } => {
                for segment in segments {
                    if ids.curves(&segment.curve.0).is_none() {
                        ref_error(findings, &curve.id.0, "curve", &segment.curve.0);
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
                curve.id.0.as_str(),
                segments
                    .iter()
                    .map(|segment| segment.curve.0.as_str())
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
                    if ids.surfaces(&component.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &component.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::SubSurface { support, .. }
            | ProceduralSurfaceDefinition::Replica {
                source: support, ..
            } => {
                if ids.surfaces(&support.0).is_none() {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                if ids.surfaces(&support.0).is_none() {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
                if ids.curves(&reference.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &reference.0);
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
                        if ids.curves(&curve.0).is_none() {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = member.form.surface() {
                            if ids.surfaces(&surface.0).is_none() {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if ids.curves(&curve.0).is_none() {
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
                        if let Some(surface) = &member.data.surface {
                            if ids.surfaces(&surface.0).is_none() {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if ids.curves(&curve.0).is_none() {
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
                        if let Some(surface) = &member.data.surface {
                            if ids.surfaces(&surface.0).is_none() {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
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
                            if ids.curves(&curve.id.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &curve.id.0);
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
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                };
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        check_curve(path, findings);
                        for profile in profiles {
                            check_curve(&profile.curve, findings);
                            if let Some(surface) = &profile.data.surface {
                                if ids.surfaces(&surface.0).is_none() {
                                    ref_error(findings, &procedural.id.0, "surface", &surface.0);
                                }
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
            ProceduralSurfaceDefinition::Law { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(&curve.id.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &curve.id.0);
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
                    for variable in &formula.variables {
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
                            if ids.curves(&curve.id.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &curve.id.0);
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
                        if ids.curves(&curve.0).is_none() {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = member.form.surface() {
                            if ids.surfaces(&surface.0).is_none() {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
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
                    if ids.surfaces(&surface.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
                if let crate::geometry::G2BlendFirstShape::Full {
                    support: Some(support),
                } = &construction.first_shape
                {
                    if ids.surfaces(&support.surface.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &support.surface.0);
                    }
                }
                for curve in [
                    &construction.first.curve,
                    &construction.second.curve,
                    &construction.center_curve,
                ] {
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::VariableBlend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(&curve.0).is_none() {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                }
                for curve in [
                    Some(&construction.slice),
                    construction.secondary_curve.as_ref(),
                    construction.post_curve.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                for member in construction
                    .base_profile
                    .iter()
                    .chain(construction.entries.iter().flat_map(|entry| &entry.profile))
                {
                    if ids.curves(&member.curve.id.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &member.curve.id.0);
                    }
                    if let Some(surface) = member.form.surface() {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
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
                    .chain(construction.trailing_curve.iter())
                {
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if ids.curves(&curve.0).is_none() {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                }
                if ids.curves(&construction.center.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &construction.center.0);
                }
            }
            ProceduralSurfaceDefinition::VertexBlend { construction } => {
                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            if ids.curves(&curve.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            if ids.surfaces(&surface.0).is_none() {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
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
                if ids.curves(&directrix.0).is_none() {
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
                    ids: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if ids.curves(&curve.id.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &curve.id.0);
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
                    if ids.curves(&curve.0).is_none() {
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
                            if ids.curves(&guide_curve.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &guide_curve.0);
                            }
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            if ids.surfaces(&support_surface.0).is_none() {
                                ref_error(
                                    findings,
                                    &procedural.id.0,
                                    "surface",
                                    &support_surface.0,
                                );
                            }
                            if let Some(curve) = auxiliary_curve {
                                if ids.curves(&curve.0).is_none() {
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
                if ids.surfaces(&support.0).is_none() {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Subset { support, .. }
            | ProceduralSurfaceDefinition::ParallelOffset { support, .. } => {
                if ids.surfaces(&support.0).is_none() {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                for curve in [first, second] {
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                for curve in [first, second] {
                    if ids.curves(&curve.0).is_none() {
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
                    if ids.surfaces(&support.surface.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &support.surface.0);
                    }
                }
                if let Some(spine) = spine {
                    if ids.curves(&spine.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &spine.0);
                    }
                }
                if let Some(native) = native {
                    let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                        if ids.curves(&curve.0).is_none() {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    };
                    let check_surface =
                        |surface: &crate::ids::SurfaceId, findings: &mut Vec<Finding>| {
                            if ids.surfaces(&surface.0).is_none() {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        };
                    check_curve(&native.slice, findings);
                    for side in native.sides.iter() {
                        if let Some(curve) = &side.curve {
                            check_curve(curve, findings);
                        }
                        if let Some(surface) = &side.surface {
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
                if !ids.contains(&record.0) {
                    ref_error(findings, &procedural.id.0, "unknown record", &record.0);
                }
            }
            ProceduralSurfaceDefinition::RollingBallJet { .. }
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
                if ids.surfaces(&support.0).is_none() {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
                for boundary in boundaries {
                    if ids.curves(&boundary.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &boundary.0);
                    }
                }
                for pcurve in boundary_pcurves {
                    if ids.pcurves(&pcurve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "pcurve boundary", &pcurve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Deformable { construction } => {
                if ids.surfaces(&construction.support.0).is_none() {
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
                    if ids.surfaces(&surface.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        match procedural.definition() {
            ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix { .. } => {}
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
                            if ids.curves(&curve.id.0).is_none() {
                                ref_error(findings, &procedural.id.0, "curve", &curve.id.0);
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
                        if ids.surfaces(&surface.0).is_none() {
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
                    if ids.curves(&component.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &component.0);
                    }
                }
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::TolerantIntersection { supports, .. } => {
                for surface in supports {
                    if ids.surfaces(&surface.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides.iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
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
                if ids.surfaces(&cast_surface.0).is_none() {
                    ref_error(findings, &procedural.id.0, "surface", &cast_surface.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                if ids.curves(&base.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &base.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Spring { layout, .. } => {
                let context = layout.support_context();
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Deformable {
                context, source, ..
            } => {
                if let crate::geometry::DeformableCurveSource::Curve { curve } = source {
                    if ids.curves(&curve.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                if ids.curves(&source.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Offset {
                source,
                support,
                distance_law,
                ..
            } => {
                if ids.curves(&source.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
                if let Some(support) = support {
                    if ids.surfaces(&support.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &support.0);
                    }
                }
                if let Some(crate::geometry::CurveOffsetDistanceLaw::Coordinate {
                    function, ..
                }) = distance_law
                {
                    if ids.curves(&function.0).is_none() {
                        ref_error(findings, &procedural.id.0, "curve", &function.0);
                    }
                }
            }
            ProceduralCurveDefinition::SpatialOffset { source, .. } => {
                if ids.curves(&source.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if ids.surfaces(&surface.0).is_none() {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                if ids.curves(&source.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::Replica { source, .. }
            | ProceduralCurveDefinition::Subset { source, .. } => {
                if ids.curves(&source.0).is_none() {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    if ids.surfaces(&surface.0).is_none() {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Unknown {
                native_kind: _,
                record: Some(record),
            } => {
                if !ids.contains(&record.0) {
                    ref_error(findings, &procedural.id.0, "unknown record", &record.0);
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
        .map(|parameter| (&parameter.id, (parameter.owner.as_ref(), parameter.ordinal)))
        .collect::<HashMap<_, _>>();
    let mut parameter_names = HashSet::new();
    let mut parameter_ordinals = HashSet::new();
    for parameter in &ir.model.parameters {
        if let Some(owner) = &parameter.owner {
            if !features.contains(owner.0.as_str()) {
                ref_error(findings, &parameter.id.0, "feature", &owner.0);
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
                entity: Some(parameter.id.0.clone()),
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
                entity: Some(parameter.id.0.clone()),
            });
        }
        if parameter
            .value
            .as_ref()
            .is_some_and(|value| !parameter_value_is_valid(value))
        {
            geometry_error(findings, &parameter.id.0, "parameter value is invalid");
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
        .map(|entity| entity.id().0.as_str())
        .collect::<HashSet<_>>();
    let sketch_entity_owners = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (entity.id().0.as_str(), entity.sketch.0.as_str()))
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
            ref_error(findings, entity.id().0.as_str(), "sketch", &entity.sketch.0);
        }
    }
    for constraint in &ir.model.sketch_constraints {
        if !sketches.contains(constraint.sketch.0.as_str()) {
            ref_error(findings, &constraint.id.0, "sketch", &constraint.sketch.0);
        }
        let (entities, parameter) = match &constraint.definition {
            Definition::Disabled => (Vec::new(), None),
            Definition::Coincident { entities }
            | Definition::Polygon { entities }
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
            } => (entities.clone(), Some(parameter.0.as_str())),
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
            Definition::SameCoordinate { first, second, .. }
            | Definition::TangentLoci { first, second } => (
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
                parameter.as_ref().map(|parameter| parameter.id.0.as_str()),
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
                Some(parameter.0.as_str()),
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
                Some(parameter.0.as_str()),
            ),
            Definition::RepeatedLength {
                entities,
                parameter,
            } => (entities.clone(), Some(parameter.0.as_str())),
            Definition::ParallelLineSetDistance {
                first,
                second,
                parameter,
            } => (
                first.iter().chain(second).cloned().collect(),
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
            Definition::AngleToAxis {
                entity, parameter, ..
            } => (vec![entity.clone()], Some(parameter.0.as_str())),
            Definition::RepeatedRadius {
                entities,
                parameter,
            }
            | Definition::RepeatedDiameter {
                entities,
                parameter,
            } => (entities.clone(), Some(parameter.0.as_str())),
            Definition::Radius { entity, parameter }
            | Definition::Diameter { entity, parameter }
            | Definition::Weight { entity, parameter } => {
                (vec![entity.clone()], Some(parameter.0.as_str()))
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
                Some(parameter.0.as_str()),
            ),
        };
        let parameter = parameter.or(match &constraint.definition {
            Definition::Distance { parameter, .. } => Some(parameter.0.as_str()),
            _ => None,
        });
        if let Definition::Polygon { entities } = &constraint.definition {
            let distinct = entities.iter().collect::<HashSet<_>>();
            if entities.len() < 3 || distinct.len() != entities.len() {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: "polygon constraint requires at least three distinct members".into(),
                    entity: Some(constraint.id.0.clone()),
                });
            }
        }
        if let Definition::SameCoordinate { first, second, .. } = &constraint.definition {
            if first == second {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: "axis-alignment constraint requires two distinct loci".into(),
                    entity: Some(constraint.id.0.clone()),
                });
            }
        }
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
        if let Definition::RectangularPattern { pattern } = &constraint.definition {
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
                if !parameters.contains(parameter.0.as_str()) {
                    ref_error(
                        findings,
                        &constraint.id.0,
                        "parameter",
                        parameter.0.as_str(),
                    );
                }
            }
        }
        if let Definition::CircularPattern { pattern } = &constraint.definition {
            for parameter in [pattern.angle_parameter(), pattern.count_parameter()]
                .into_iter()
                .flatten()
            {
                if !parameters.contains(parameter.0.as_str()) {
                    ref_error(
                        findings,
                        &constraint.id.0,
                        "parameter",
                        parameter.0.as_str(),
                    );
                }
            }
        }
    }
    check_feature_sketch_references(ir, &sketches, findings);
    check_feature_references(ir, ids, findings);
}

fn check_feature_references(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    use crate::features::{
        EdgeSelection, ExtrudeExtent, FeatureDefinition, PathRef, ProfileRef, ScaleCenter,
        Termination,
    };

    let mut configuration_ordinals = HashSet::new();
    let mut configuration_source_indices = HashSet::new();
    let mut active_configurations = 0;
    let parameter_ids = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.0.as_str())
        .collect::<HashSet<_>>();
    let asset_ids = ir
        .model
        .assets
        .iter()
        .map(|asset| asset.id.0.as_str())
        .collect::<HashSet<_>>();
    let parameter_values = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.0.as_str(), parameter.value.as_ref()))
        .collect::<HashMap<_, _>>();
    let feature_ids = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.0.as_str())
        .collect::<HashSet<_>>();
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.id.0.as_str(), feature.ordinal))
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
            if ids.bodies(&body.0).is_none() {
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
        for parameter in configuration.parameter_overrides.keys() {
            if !parameter_ids.contains(parameter.0.as_str()) {
                ref_error(
                    findings,
                    &configuration.id.0,
                    "configuration parameter override",
                    &parameter.0,
                );
            }
        }
        let mut suppressed_features = HashSet::new();
        for feature in &configuration.suppressed_features {
            if !feature_ids.contains(feature.0.as_str()) {
                ref_error(
                    findings,
                    &configuration.id.0,
                    "configuration suppressed feature",
                    &feature.0,
                );
            }
            if !suppressed_features.insert(feature) {
                findings.push(Finding {
                    check: Check::Counts,
                    severity: Severity::Error,
                    message: format!("configuration repeats suppressed feature `{}`", feature.0),
                    entity: Some(configuration.id.0.clone()),
                });
            }
        }
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
                        entity: Some(configuration.id.0.clone()),
                    });
                }
            }
        }
        for (parameter, value) in &configuration.parameter_values {
            match parameter_values.get(parameter.0.as_str()) {
                None => ref_error(
                    findings,
                    &configuration.id.0,
                    "configuration parameter value",
                    &parameter.0,
                ),
                Some(baseline)
                    if !parameter_value_is_valid(value)
                        || baseline.is_some_and(|baseline| {
                            std::mem::discriminant(baseline) != std::mem::discriminant(value)
                        }) =>
                {
                    geometry_error(
                        findings,
                        &configuration.id.0,
                        "configuration parameter value is invalid",
                    );
                }
                Some(_) => {}
            }
        }
        for (feature, state) in &configuration.feature_states {
            if suppressed_features.contains(feature) != state.suppressed {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message:
                        "configuration feature suppression disagrees with suppressed feature list"
                            .into(),
                    entity: Some(configuration.id.0.clone()),
                });
            }
            let feature_ordinal = features.get(feature.0.as_str()).copied();
            if feature_ordinal.is_none() {
                ref_error(
                    findings,
                    &configuration.id.0,
                    "configuration feature state",
                    &feature.0,
                );
            }
            let mut dependencies = HashSet::new();
            for dependency in &state.dependencies {
                match features.get(dependency.0.as_str()) {
                    None => ref_error(
                        findings,
                        &configuration.id.0,
                        "configuration feature dependency",
                        &dependency.0,
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
                                dependency.0, feature.0
                            ),
                            entity: Some(configuration.id.0.clone()),
                        });
                    }
                    Some(_) => {}
                }
                if !dependencies.insert(dependency) {
                    findings.push(Finding {
                        check: Check::Counts,
                        severity: Severity::Error,
                        message: format!(
                            "configuration feature state repeats dependency `{}`",
                            dependency.0
                        ),
                        entity: Some(configuration.id.0.clone()),
                    });
                }
            }
            for reference in regeneration_references(&state.definition) {
                match features.get(reference.0.as_str()) {
                    None => ref_error(
                        findings,
                        &configuration.id.0,
                        "configuration definition feature",
                        &reference.0,
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
                                reference.0, feature.0
                            ),
                            entity: Some(configuration.id.0.clone()),
                        });
                    }
                    Some(_) if !state.dependencies.contains(reference) => {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "configuration feature state `{}` omits referenced feature `{}` from its dependencies",
                                feature.0, reference.0
                            ),
                            entity: Some(configuration.id.0.clone()),
                        });
                    }
                    Some(_) => {}
                }
            }
            for termination in definition_terminations(&state.definition) {
                if !termination_magnitude_is_valid(termination) {
                    geometry_error(
                        findings,
                        &configuration.id.0,
                        "configuration feature extent magnitude is invalid",
                    );
                }
                if matches!(
                    termination,
                    Termination::ToVertex {
                        vertex: crate::features::VertexSelection::Generated { vertex, native },
                    } if native.trim().is_empty() || vertex.local_id.trim().is_empty()
                ) {
                    geometry_error(
                        findings,
                        &configuration.id.0,
                        "configuration generated termination vertex is invalid",
                    );
                }
            }
            if let FeatureDefinition::DatumOffsetPlane { distance, .. } = &state.definition {
                if !distance.0.is_finite() {
                    geometry_error(
                        findings,
                        &configuration.id.0,
                        "configuration datum-plane offset is invalid",
                    );
                }
            }
            if state.suppressed && !state.outputs.is_empty() {
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: "suppressed configuration feature state has output bodies".into(),
                    entity: Some(configuration.id.0.clone()),
                });
            }
            let mut outputs = HashSet::new();
            for output in &state.outputs {
                if ids.bodies(&output.0).is_none() {
                    ref_error(
                        findings,
                        &configuration.id.0,
                        "configuration feature output",
                        &output.0,
                    );
                }
                if !outputs.insert(output) {
                    findings.push(Finding {
                        check: Check::Counts,
                        severity: Severity::Error,
                        message: format!(
                            "configuration feature state repeats output body `{}`",
                            output.0
                        ),
                        entity: Some(configuration.id.0.clone()),
                    });
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
        .map(|feature| (feature.id.0.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let sketch_entities = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.id().0.clone())
        .collect::<HashSet<_>>();
    let spatial_sketch_entity_owners = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (entity.id().0.as_str(), entity.sketch.0.as_str()))
        .collect::<HashMap<_, _>>();
    let mut reported_plane_cycles = HashSet::new();
    for feature in &ir.model.features {
        let mut path = Vec::new();
        let mut positions = HashMap::new();
        let mut cursor = feature.id.0.as_str();
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
                        entity: Some(feature.id.0.clone()),
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
                } = &feature.definition
                else {
                    return None;
                };
                Some(reference.0.as_str())
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
                entity: Some(state.input_of.0.clone()),
            });
        }
        for (kind, members) in [
            (
                "historical body",
                state
                    .bodies
                    .iter()
                    .map(crate::ids::HistoricalBodyId::as_str)
                    .collect::<Vec<_>>(),
            ),
            (
                "historical face",
                state
                    .faces
                    .iter()
                    .map(crate::ids::HistoricalFaceId::as_str)
                    .collect::<Vec<_>>(),
            ),
            (
                "historical edge",
                state
                    .edges
                    .iter()
                    .map(crate::ids::HistoricalEdgeId::as_str)
                    .collect::<Vec<_>>(),
            ),
            (
                "historical vertex",
                state
                    .vertices
                    .iter()
                    .map(crate::ids::HistoricalVertexId::as_str)
                    .collect::<Vec<_>>(),
            ),
        ] {
            let mut seen = HashSet::new();
            for member in members {
                if member.is_empty() || !seen.insert(member) {
                    findings.push(Finding {
                        check: Check::Counts,
                        severity: Severity::Error,
                        message: format!("input topology has empty or repeated {kind} `{member}`"),
                        entity: Some(state.id.0.clone()),
                    });
                }
            }
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
                entity: Some(state.output_of.0.clone()),
            });
        }
        if state.bodies.is_empty()
            && state.faces.is_empty()
            && state.edges.is_empty()
            && state.vertices.is_empty()
        {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: "feature result topology is empty".into(),
                entity: Some(state.id.0.clone()),
            });
        }
        for (kind, members) in [
            ("body", &state.bodies),
            ("face", &state.faces),
            ("edge", &state.edges),
            ("vertex", &state.vertices),
        ] {
            let mut seen = HashSet::new();
            for member in members {
                if member.trim().is_empty() || !seen.insert(member) {
                    findings.push(Finding {
                        check: Check::Counts,
                        severity: Severity::Error,
                        message: format!(
                            "result topology has empty or repeated generated {kind} `{member}`"
                        ),
                        entity: Some(state.id.0.clone()),
                    });
                }
            }
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
                        Some(owner) if *owner != Some(&feature.id) => findings.push(Finding {
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
            if ids.bodies(&body.0).is_none() {
                ref_error(findings, &feature.id.0, "output body", &body.0);
            }
        }

        let mut paths = Vec::new();
        let mut edge_selections = Vec::new();
        let mut face_selections = Vec::new();
        let mut vertex_selections = Vec::new();
        let mut body_selections = Vec::new();
        let definition = match &feature.definition {
            FeatureDefinition::PostProcess {
                operation,
                fuzzy_tolerance,
                ..
            } => {
                if matches!(fuzzy_tolerance, crate::features::FuzzyTolerance::Explicit(value) if !value.is_finite() || *value <= 0.0)
                {
                    feature_geometry_error(findings, feature, "feature fuzzy tolerance is invalid");
                }
                operation.as_ref()
            }
            definition => definition,
        };
        match definition {
            FeatureDefinition::DatumAxisUnresolved
            | FeatureDefinition::DatumPointUnresolved
            | FeatureDefinition::DatumCoordinateSystemUnresolved
            | FeatureDefinition::BridgeCurveUnresolved
            | FeatureDefinition::LoftUnresolved
            | FeatureDefinition::ThroughCurveMeshUnresolved
            | FeatureDefinition::FreeformSurfaceUnresolved
            | FeatureDefinition::ExtractFaceUnresolved
            | FeatureDefinition::CopyFaceUnresolved
            | FeatureDefinition::LinkedFaceUnresolved
            | FeatureDefinition::FillHoleUnresolved
            | FeatureDefinition::MoveObjectUnresolved
            | FeatureDefinition::BoundarySurfaceUnresolved
            | FeatureDefinition::DeleteFaceUnresolved
            | FeatureDefinition::MirrorFaceUnresolved
            | FeatureDefinition::SubdivisionBodyUnresolved
            | FeatureDefinition::TopologyOptimizationUnresolved
            | FeatureDefinition::ExtrudeUnresolved
            | FeatureDefinition::RevolveUnresolved
            | FeatureDefinition::FilletUnresolved => {}
            FeatureDefinition::ReferenceImage {
                asset,
                origin,
                u_axis,
                v_axis,
                bounds,
                opacity,
                ..
            } => {
                if !asset_ids.contains(asset.0.as_str()) {
                    ref_error(findings, &feature.id.0, "reference-image asset", &asset.0);
                }
                let frame_is_valid = [origin.x, origin.y, origin.z]
                    .into_iter()
                    .all(f64::is_finite)
                    && (u_axis.norm() - 1.0).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    && (v_axis.norm() - 1.0).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    && u_axis.dot(*v_axis).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9;
                let [first, second] = bounds;
                let bounds_are_valid = [first.u, first.v, second.u, second.v]
                    .into_iter()
                    .all(f64::is_finite)
                    && first.u != second.u
                    && first.v != second.v;
                if !frame_is_valid
                    || !bounds_are_valid
                    || opacity
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "reference-image placement is invalid",
                    );
                }
            }
            FeatureDefinition::Decal {
                asset,
                faces,
                opacity,
                ..
            } => {
                if !asset_ids.contains(asset.0.as_str()) {
                    ref_error(findings, &feature.id.0, "decal asset", &asset.0);
                }
                if opacity.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
                {
                    feature_geometry_error(findings, feature, "decal opacity is invalid");
                }
                face_selections.push(faces);
            }
            FeatureDefinition::Block {
                dimensions,
                placement,
                ..
            } => {
                if dimensions.is_some_and(|values| {
                    values
                        .into_iter()
                        .any(|value| !positive_feature_length(value))
                }) {
                    feature_geometry_error(findings, feature, "block dimensions are invalid");
                }
                if placement.is_some_and(|placement| !placement.is_proper_rigid()) {
                    feature_geometry_error(findings, feature, "block placement is invalid");
                }
            }
            FeatureDefinition::ExtractBody { source } => body_selections.push(source),
            FeatureDefinition::FaceBlend {
                first_faces,
                second_faces,
                radius,
            } => {
                face_selections.push(first_faces);
                face_selections.push(second_faces);
                if face_selections_overlap(first_faces, second_faces) {
                    feature_geometry_error(findings, feature, "face blend supports overlap");
                }
                if !radius_spec_is_valid(radius) {
                    feature_geometry_error(findings, feature, "face blend radius is invalid");
                }
            }
            FeatureDefinition::FullRoundFillet { groups } => {
                let valid = !groups.is_empty()
                    && groups.iter().all(|group| {
                        face_selections.push(&group.center_faces);
                        let side_one = match &group.side_one_faces {
                            crate::features::FullRoundSideSelection::Explicit(selection) => {
                                face_selections.push(selection);
                                Some(selection)
                            }
                            crate::features::FullRoundSideSelection::Automatic
                            | crate::features::FullRoundSideSelection::Unresolved => None,
                        };
                        let side_two = match &group.side_two_faces {
                            crate::features::FullRoundSideSelection::Explicit(selection) => {
                                face_selections.push(selection);
                                Some(selection)
                            }
                            crate::features::FullRoundSideSelection::Automatic
                            | crate::features::FullRoundSideSelection::Unresolved => None,
                        };
                        !side_one.is_some_and(|selection| {
                            face_selections_overlap(&group.center_faces, selection)
                        }) && !side_two.is_some_and(|selection| {
                            face_selections_overlap(&group.center_faces, selection)
                        }) && !side_one
                            .zip(side_two)
                            .is_some_and(|(first, second)| face_selections_overlap(first, second))
                    });
                if !valid {
                    feature_geometry_error(
                        findings,
                        feature,
                        "full-round fillet face sets are invalid",
                    );
                }
            }
            FeatureDefinition::SewBodies {
                bodies,
                gap_tolerance,
            } => {
                body_selections.push(bodies);
                let body_count = match bodies {
                    BodySelection::Bodies(bodies)
                    | BodySelection::Resolved { bodies, .. }
                    | BodySelection::ResolvedSet { bodies, .. } => Some(bodies.len()),
                    BodySelection::Historical { bodies, .. }
                    | BodySelection::HistoricalSet { bodies, .. }
                    | BodySelection::HistoricalUnorderedSet { bodies, .. } => Some(bodies.len()),
                    BodySelection::Generated { bodies, .. } => Some(bodies.len()),
                    BodySelection::Local { bodies, .. } => Some(bodies.len()),
                    BodySelection::Unresolved
                    | BodySelection::Native(_)
                    | BodySelection::NativeSet(_) => None,
                };
                if body_count.is_some_and(|count| count < 2) {
                    feature_geometry_error(findings, feature, "sew requires at least two bodies");
                }
                if gap_tolerance.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "sew tolerance is invalid");
                }
            }
            FeatureDefinition::BaseFeature { bodies } => body_selections.push(bodies),
            FeatureDefinition::MeshImport { tessellations } => {
                if tessellations.is_empty() {
                    feature_geometry_error(
                        findings,
                        feature,
                        "mesh import has no tessellation geometry",
                    );
                }
                let mut seen = HashSet::new();
                for tessellation in tessellations {
                    if !seen.insert(tessellation) || ids.tessellations(tessellation).is_none() {
                        ref_error(
                            findings,
                            &feature.id.0,
                            "mesh import tessellation",
                            tessellation,
                        );
                    }
                }
            }
            FeatureDefinition::InsertBodies { bodies } => {
                body_selections.push(bodies);
                if let BodySelection::Resolved { bodies, .. } = bodies {
                    if feature.outputs != *bodies {
                        feature_geometry_error(
                            findings,
                            feature,
                            "inserted bodies do not match feature outputs",
                        );
                    }
                }
            }
            FeatureDefinition::InsertComponent { occurrence } => {
                if !ir
                    .model
                    .occurrences
                    .iter()
                    .any(|candidate| candidate.id == *occurrence)
                {
                    ref_error(
                        findings,
                        &feature.id.0,
                        "inserted component occurrence",
                        &occurrence.0,
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
                    ref_error(findings, &feature.id.0, "assembly joint", &joint.0);
                }
            }
            FeatureDefinition::Form { cages } => {
                check_ids(
                    findings,
                    &feature.id.0,
                    "Form control cage",
                    cages.iter().map(|cage| cage.0.as_str()),
                    |identity| ids.subds(identity).is_some(),
                );
            }
            FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } => {
                face_selections.push(face);
                let extent_valid = extent.as_ref().is_none_or(|extent| match extent {
                    crate::features::CosmeticThreadExtent::Blind { length } => {
                        positive_feature_length(*length)
                    }
                    crate::features::CosmeticThreadExtent::Through => true,
                });
                if diameter.is_some_and(|value| !positive_feature_length(value)) || !extent_valid {
                    feature_geometry_error(
                        findings,
                        feature,
                        "cosmetic-thread geometry is invalid",
                    );
                }
            }
            FeatureDefinition::Primitive { solid, .. } => {
                let positive = |value: Length| value.0.is_finite() && value.0 > 0.0;
                let finite_angle = |value: crate::features::Angle| value.0.is_finite();
                let valid = match solid {
                    PrimitiveSolid::Box {
                        length,
                        width,
                        height,
                    } => positive(*length) && positive(*width) && positive(*height),
                    PrimitiveSolid::Cylinder {
                        radius,
                        height,
                        angle,
                    } => positive(*radius) && positive(*height) && finite_angle(*angle),
                    PrimitiveSolid::Cone {
                        radius1,
                        radius2,
                        height,
                        angle,
                    } => {
                        radius1.0.is_finite()
                            && radius1.0 >= 0.0
                            && radius2.0.is_finite()
                            && radius2.0 >= 0.0
                            && (radius1.0 > 0.0 || radius2.0 > 0.0)
                            && positive(*height)
                            && finite_angle(*angle)
                    }
                    PrimitiveSolid::Sphere {
                        radius,
                        latitude1,
                        latitude2,
                        longitude,
                    } => {
                        positive(*radius)
                            && finite_angle(*latitude1)
                            && finite_angle(*latitude2)
                            && latitude1.0 < latitude2.0
                            && finite_angle(*longitude)
                    }
                    PrimitiveSolid::Ellipsoid {
                        x_radius,
                        y_radius,
                        z_radius,
                        latitude1,
                        latitude2,
                        longitude,
                    } => {
                        positive(*x_radius)
                            && positive(*y_radius)
                            && positive(*z_radius)
                            && finite_angle(*latitude1)
                            && finite_angle(*latitude2)
                            && latitude1.0 < latitude2.0
                            && finite_angle(*longitude)
                    }
                    PrimitiveSolid::Torus {
                        major_radius,
                        minor_radius,
                        latitude1,
                        latitude2,
                        longitude,
                    } => {
                        positive(*major_radius)
                            && positive(*minor_radius)
                            && finite_angle(*latitude1)
                            && finite_angle(*latitude2)
                            && latitude1.0 < latitude2.0
                            && finite_angle(*longitude)
                    }
                    PrimitiveSolid::Prism {
                        sides,
                        circumradius,
                        height,
                    } => *sides >= 3 && positive(*circumradius) && positive(*height),
                    PrimitiveSolid::Wedge {
                        xmin,
                        ymin,
                        zmin,
                        x2min,
                        z2min,
                        xmax,
                        ymax,
                        zmax,
                        x2max,
                        z2max,
                    } => {
                        [
                            xmin, ymin, zmin, x2min, z2min, xmax, ymax, zmax, x2max, z2max,
                        ]
                        .into_iter()
                        .all(|value| value.0.is_finite())
                            && xmax.0 > xmin.0
                            && ymax.0 > ymin.0
                            && zmax.0 > zmin.0
                            && x2max.0 >= x2min.0
                            && z2max.0 >= z2min.0
                    }
                };
                if !valid {
                    feature_geometry_error(findings, feature, "primitive dimensions are invalid");
                }
            }
            FeatureDefinition::Extrude {
                direction,
                start,
                extent,
                direction_source,
                face_maker,
                ..
            } => {
                let sides = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        vec![side]
                    }
                    ExtrudeExtent::TwoSided { first, second } => vec![first, second],
                };
                if let crate::features::ExtrudeDirection::Explicit(vector) = direction {
                    if !valid_feature_direction(*vector) {
                        feature_geometry_error(findings, feature, "extrusion direction is invalid");
                    }
                }
                if let Some(crate::features::ExtrusionDirectionSource::Edge { reference }) =
                    direction_source
                {
                    paths.push(reference);
                }
                if sides.iter().any(|side| {
                    side.draft.is_some_and(|angle| {
                        !angle.0.is_finite() || angle.0.abs() >= std::f64::consts::FRAC_PI_2
                    })
                }) {
                    feature_geometry_error(findings, feature, "extrusion draft is invalid");
                }
                if sides
                    .iter()
                    .any(|side| side.offset.is_some_and(|offset| !offset.0.is_finite()))
                    || face_maker
                        .as_ref()
                        .is_some_and(|maker| maker.class.is_empty())
                {
                    feature_geometry_error(findings, feature, "extrusion construction is invalid");
                }
                match start {
                    ExtrudeStart::Unresolved => {}
                    ExtrudeStart::ProfilePlane => {}
                    ExtrudeStart::OffsetProfilePlane { offset } => {
                        if !offset.0.is_finite() {
                            feature_geometry_error(
                                findings,
                                feature,
                                "extrusion start offset is invalid",
                            );
                        }
                    }
                    ExtrudeStart::FromFace { face, offset } => {
                        face_selections.push(face);
                        if offset.is_some_and(|offset| !offset.0.is_finite()) {
                            feature_geometry_error(
                                findings,
                                feature,
                                "extrusion start offset is invalid",
                            );
                        }
                    }
                }
            }
            FeatureDefinition::SheetMetalBaseFlange { thickness, .. } => {
                if !positive_feature_length(*thickness) {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal base-flange thickness is invalid",
                    );
                }
            }
            FeatureDefinition::SheetMetalEdgeFlange {
                edges,
                height,
                angle,
                width,
                bend_radius,
                ..
            } => {
                edge_selections.push(edges);
                let height_valid = match height {
                    crate::features::SheetMetalFlangeHeight::Distance(height) => {
                        positive_feature_length(*height)
                    }
                    crate::features::SheetMetalFlangeHeight::ToObject { target, offset } => {
                        offset.0.is_finite()
                            && !matches!(
                                target,
                                crate::features::SheetMetalFlangeHeightTarget::Native(native)
                                    if native.is_empty()
                            )
                    }
                };
                if !height_valid {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal edge-flange height is invalid",
                    );
                }
                if !angle.0.is_finite() {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal edge-flange angle is invalid",
                    );
                }
                if !positive_feature_length(*bend_radius) {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal edge-flange bend radius is invalid",
                    );
                }
                let widths = match width {
                    crate::features::SheetMetalFlangeWidth::FullEdge => Vec::new(),
                    crate::features::SheetMetalFlangeWidth::Symmetric { width } => vec![*width],
                    crate::features::SheetMetalFlangeWidth::TwoSides { first, second } => {
                        vec![*first, *second]
                    }
                    crate::features::SheetMetalFlangeWidth::TwoSidesPerEdge { widths } => widths
                        .iter()
                        .flat_map(|width| [width.first, width.second])
                        .collect(),
                };
                let per_edge_widths_are_nonempty = !matches!(
                    width,
                    crate::features::SheetMetalFlangeWidth::TwoSidesPerEdge { widths }
                        if widths.is_empty()
                );
                if !per_edge_widths_are_nonempty
                    || !widths.iter().copied().all(positive_feature_length)
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal edge-flange width is invalid",
                    );
                }
            }
            FeatureDefinition::SheetMetalHem {
                edges,
                form,
                bend_radius,
                ..
            } => {
                edge_selections.push(edges);
                if !positive_feature_length(*bend_radius) {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal hem bend radius is invalid",
                    );
                }
                let gap_is_valid = |gap: crate::features::Length| gap.0.is_finite() && gap.0 >= 0.0;
                let form_is_valid = match form {
                    crate::features::SheetMetalHemForm::Flat { length } => {
                        positive_feature_length(*length)
                    }
                    crate::features::SheetMetalHemForm::Open { gap, length } => {
                        gap_is_valid(*gap) && positive_feature_length(*length)
                    }
                    crate::features::SheetMetalHemForm::GapLength { gap, length } => {
                        gap_is_valid(*gap) && positive_feature_length(*length)
                    }
                    crate::features::SheetMetalHemForm::Rolled { radius, angle } => {
                        positive_feature_length(*radius) && angle.0.is_finite()
                    }
                    crate::features::SheetMetalHemForm::Teardrop {
                        gap,
                        length,
                        radius,
                    } => {
                        gap_is_valid(*gap)
                            && positive_feature_length(*length)
                            && positive_feature_length(*radius)
                    }
                };
                if !form_is_valid {
                    feature_geometry_error(findings, feature, "sheet-metal hem form is invalid");
                }
            }
            FeatureDefinition::Revolve { construction, .. } => {
                paths.extend(&construction.axis_reference);
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
                section,
                sections,
                path,
                mode,
                orientation,
                twist,
                path_extent,
                guide_rail,
                taper,
                scale,
                ..
            } => {
                paths.extend(path);
                let invalid_section =
                    std::iter::once(section)
                        .chain(sections)
                        .any(|section| match section {
                            crate::features::SweepSection::Unresolved(_) => false,
                            crate::features::SweepSection::Profile(_) => false,
                            crate::features::SweepSection::Generated(
                                crate::features::GeneratedSweepSection::CircularRegion {
                                    outer_radius,
                                    wall_thickness,
                                },
                            ) => {
                                !positive_feature_length(*outer_radius)
                                    || wall_thickness.is_some_and(|thickness| {
                                        !positive_feature_length(thickness)
                                            || thickness.0 >= outer_radius.0
                                    })
                                    || !matches!(mode, crate::features::SweepMode::Solid { .. })
                            }
                        });
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
                if invalid_section
                    || twist.is_some_and(|value| !value.0.is_finite())
                    || taper.is_some_and(|value| !value.0.is_finite())
                    || path_extent.is_some_and(|extent| {
                        !(0.0..=1.0).contains(&extent.along_fraction)
                            || !(0.0..=1.0).contains(&extent.against_fraction)
                    })
                    || guide_rail.as_ref().is_some_and(|guide| {
                        !(0.0..=1.0).contains(&guide.extent.along_fraction)
                            || !(0.0..=1.0).contains(&guide.extent.against_fraction)
                    })
                    || scale.is_some_and(|value| !value.is_finite() || value <= 0.0)
                    || matches!(orientation, Some(crate::features::SweepOrientation::Binormal { direction }) if !valid_feature_direction(*direction))
                {
                    feature_geometry_error(findings, feature, "sweep magnitude is invalid");
                }
            }
            FeatureDefinition::Loft {
                sections,
                guides,
                centerline,
                max_degree,
                ..
            } => {
                for section in sections {
                    match section {
                        crate::features::LoftSection::Profile(_) => {}
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Native(native),
                        ) if native.is_empty() => {
                            feature_geometry_error(
                                findings,
                                feature,
                                "loft point section has an empty native reference",
                            );
                        }
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Native(_),
                        ) => {}
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Point(point),
                        ) if !point.x.is_finite()
                            || !point.y.is_finite()
                            || !point.z.is_finite() =>
                        {
                            feature_geometry_error(
                                findings,
                                feature,
                                "loft point section is invalid",
                            );
                        }
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Point(_),
                        ) => {}
                        crate::features::LoftSection::Point(
                            crate::features::LoftPointSection::Vertex(vertex),
                        ) => check_ids(
                            findings,
                            &feature.id.0,
                            "loft section vertex",
                            std::iter::once(vertex.0.as_str()),
                            |identity| ids.vertices(identity).is_some(),
                        ),
                    }
                }
                paths.extend(guides);
                paths.extend(centerline);
                if centerline.is_some() && !guides.is_empty() {
                    feature_geometry_error(findings, feature, "loft construction is invalid");
                }
                if max_degree.is_some_and(|value| value == 0) {
                    feature_geometry_error(findings, feature, "loft maximum degree is invalid");
                }
            }
            FeatureDefinition::Rib { construction, .. } => {
                if construction
                    .direction
                    .is_some_and(|value| !valid_feature_direction(value))
                    || construction
                        .thickness
                        .is_some_and(|value| !positive_feature_length(value))
                    || matches!(
                        construction.draft,
                        crate::features::RibDraft::Angle(value)
                            if !valid_draft_angle(value)
                    )
                {
                    feature_geometry_error(findings, feature, "rib geometry is invalid");
                }
            }
            FeatureDefinition::Fillet { groups } => {
                let valid = !groups.is_empty()
                    && groups.iter().all(|group| {
                        edge_selections.push(&group.edges);
                        group.tangency_weight.is_none_or(f64::is_finite)
                            && match &group.radius {
                                RadiusSpec::Unresolved { .. } => true,
                                RadiusSpec::Constant { radius } => positive_feature_length(*radius),
                                RadiusSpec::Chordal { chord_length } => {
                                    positive_feature_length(*chord_length)
                                }
                                RadiusSpec::Asymmetric {
                                    offset_one,
                                    offset_two,
                                } => {
                                    positive_feature_length(*offset_one)
                                        && positive_feature_length(*offset_two)
                                }
                                RadiusSpec::Variable { points } => {
                                    points.len() >= 2
                                        && points.iter().all(|point| {
                                            point.parameter.is_finite()
                                                && (0.0..=1.0).contains(&point.parameter)
                                                && point.radius.0.is_finite()
                                                && point.radius.0 >= 0.0
                                        })
                                        && points.iter().any(|point| point.radius.0 > 0.0)
                                        && points
                                            .windows(2)
                                            .all(|pair| pair[0].parameter < pair[1].parameter)
                                }
                            }
                    });
                if !valid {
                    feature_geometry_error(findings, feature, "fillet radius is invalid");
                }
            }
            FeatureDefinition::Chamfer { groups, .. } => {
                let valid = !groups.is_empty()
                    && groups.iter().all(|group| {
                        edge_selections.push(&group.edges);
                        match group.spec {
                            ChamferSpec::Unresolved { .. } => true,
                            ChamferSpec::Distance { distance } => positive_feature_length(distance),
                            ChamferSpec::TwoDistances { first, second } => {
                                positive_feature_length(first) && positive_feature_length(second)
                            }
                            ChamferSpec::DistanceAngle { distance, angle } => {
                                positive_feature_length(distance)
                                    && angle.0.is_finite()
                                    && angle.0 > 0.0
                                    && angle.0 < std::f64::consts::PI
                            }
                        }
                    });
                if !valid {
                    feature_geometry_error(findings, feature, "chamfer dimensions are invalid");
                }
            }
            FeatureDefinition::Shell {
                bodies,
                removed_faces,
                thickness,
                ..
            } => {
                if let Some(bodies) = bodies {
                    body_selections.push(bodies);
                }
                face_selections.push(removed_faces);
                if thickness.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "shell thickness is invalid");
                }
            }
            FeatureDefinition::OffsetShape {
                source, distance, ..
            } => {
                body_selections.push(source);
                if !distance.0.is_finite() || distance.0 == 0.0 {
                    feature_geometry_error(findings, feature, "shape offset is invalid");
                }
            }
            FeatureDefinition::Compound { members } => body_selections.push(members),
            FeatureDefinition::RefineShape { source }
            | FeatureDefinition::ReverseShape { source } => body_selections.push(source),
            FeatureDefinition::RuledBetweenCurves { first, second, .. } => {
                paths.push(first);
                paths.push(second);
            }
            FeatureDefinition::SectionShape { first, second, .. } => {
                body_selections.push(first);
                body_selections.push(second);
                if body_selections_overlap(first, second) {
                    feature_geometry_error(findings, feature, "section operands overlap");
                }
            }
            FeatureDefinition::MirrorShape {
                source,
                plane_origin,
                plane_normal,
                plane_reference,
            } => {
                body_selections.push(source);
                face_selections.extend(plane_reference);
                if ![plane_origin.x, plane_origin.y, plane_origin.z]
                    .into_iter()
                    .all(f64::is_finite)
                    || !valid_feature_direction(*plane_normal)
                {
                    feature_geometry_error(findings, feature, "mirror plane is invalid");
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
                if distance.is_some_and(|value| !value.0.is_finite()) {
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
            FeatureDefinition::ExtendSurface {
                faces, distance, ..
            } => {
                face_selections.push(faces);
                if distance.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "surface extension is invalid");
                }
            }
            FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                mode,
                angle,
                ..
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
                if !valid || angle.is_some_and(|value| !value.0.is_finite()) {
                    feature_geometry_error(findings, feature, "ruled surface is invalid");
                }
            }
            FeatureDefinition::Draft {
                faces,
                anchor,
                angle,
                ..
            } => {
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
                if anchor
                    .pull()
                    .is_some_and(|pull| !valid_feature_direction(pull.direction))
                    || angle.is_some_and(|value| !valid_draft_angle(value))
                {
                    feature_geometry_error(findings, feature, "draft geometry is invalid");
                }
            }
            FeatureDefinition::DraftUnresolved => {}
            FeatureDefinition::BoundaryFill { tools, cells } => {
                body_selections.push(tools);
                body_selections.extend(cells);
                if cells.is_empty() {
                    feature_geometry_error(
                        findings,
                        feature,
                        "boundary fill has no selected cells",
                    );
                }
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
                        if planes.len() < 2 {
                            feature_geometry_error(
                                findings,
                                feature,
                                "split-face plane set has fewer than two planes",
                            );
                        }
                        if planes.iter().collect::<HashSet<_>>().len() != planes.len() {
                            feature_geometry_error(
                                findings,
                                feature,
                                "split-face plane set contains repeated planes",
                            );
                        }
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
            FeatureDefinition::ReplaceFace {
                targets,
                replacements,
            } => {
                face_selections.push(targets);
                face_selections.push(replacements);
                if face_selections_overlap(targets, replacements) {
                    feature_geometry_error(findings, feature, "replacement face operands overlap");
                }
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
                    FlexMode::Unresolved(_) => true,
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
                let factors_valid = factors.resolved().is_none_or(|factors| {
                    [factors.x, factors.y, factors.z]
                        .into_iter()
                        .all(|factor| factor.is_finite() && factor != 0.0)
                });
                if !center_valid || !factors_valid {
                    feature_geometry_error(findings, feature, "scale transform is invalid");
                }
            }
            FeatureDefinition::Combine { target, tools, .. } => {
                body_selections.push(target);
                body_selections.push(tools);
                if body_selections_overlap(target, tools) {
                    feature_geometry_error(findings, feature, "body combine operands overlap");
                }
                let target_count = match target {
                    BodySelection::Bodies(bodies)
                    | BodySelection::Resolved { bodies, .. }
                    | BodySelection::ResolvedSet { bodies, .. } => Some(bodies.len()),
                    BodySelection::Historical { bodies, .. }
                    | BodySelection::HistoricalSet { bodies, .. }
                    | BodySelection::HistoricalUnorderedSet { bodies, .. } => Some(bodies.len()),
                    BodySelection::Generated { bodies, .. } => Some(bodies.len()),
                    BodySelection::Local { bodies, .. } => Some(bodies.len()),
                    BodySelection::Unresolved
                    | BodySelection::Native(_)
                    | BodySelection::NativeSet(_) => None,
                };
                if target_count.is_some_and(|count| count != 1) {
                    feature_geometry_error(findings, feature, "body combine target is invalid");
                }
            }
            FeatureDefinition::CutWithSurface { targets, tools, .. } => {
                body_selections.push(targets);
                face_selections.push(tools);
            }
            FeatureDefinition::TrimBodies { targets, tools, .. } => {
                body_selections.push(targets);
                body_selections.push(tools);
                if body_selections_overlap(targets, tools) {
                    feature_geometry_error(findings, feature, "body trim operands overlap");
                }
            }
            FeatureDefinition::DeleteBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureDefinition::Hole {
                profile_filter,
                face,
                kind,
                exit_kind,
                diameter,
                direction,
                position,
                bottom,
                taper_angle,
                specification,
                placements,
                ..
            } => {
                face_selections.extend(face);
                let treatment_diameter_valid = |value: Length| {
                    positive_feature_length(value) && diameter.is_some_and(|bore| value.0 > bore.0)
                };
                let kind_valid = |kind: &HoleKind| match kind {
                    HoleKind::Unresolved(_) => true,
                    HoleKind::PartialCounterbore { diameter, depth } => {
                        diameter.is_none_or(positive_feature_length)
                            && depth.is_none_or(positive_feature_length)
                    }
                    HoleKind::PartialCountersink { diameter, angle } => {
                        diameter.is_none_or(positive_feature_length)
                            && angle.is_none_or(|value| {
                                value.0.is_finite()
                                    && value.0 > 0.0
                                    && value.0 < std::f64::consts::PI
                            })
                    }
                    HoleKind::Simple => true,
                    HoleKind::Chamfer { diameter, angle } => {
                        treatment_diameter_valid(*diameter)
                            && angle.0.is_finite()
                            && angle.0 > 0.0
                            && angle.0 < std::f64::consts::PI
                    }
                    HoleKind::SimpleDrilled { drill_point_angle } => {
                        drill_point_angle.0.is_finite()
                            && drill_point_angle.0 > 0.0
                            && drill_point_angle.0 < std::f64::consts::PI
                    }
                    HoleKind::Counterbore { diameter, depth } => {
                        treatment_diameter_valid(*diameter) && positive_feature_length(*depth)
                    }
                    HoleKind::CounterboreDrilled {
                        diameter,
                        depth,
                        drill_point_angle,
                    } => {
                        treatment_diameter_valid(*diameter)
                            && positive_feature_length(*depth)
                            && drill_point_angle.0.is_finite()
                            && drill_point_angle.0 > 0.0
                            && drill_point_angle.0 < std::f64::consts::PI
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        treatment_diameter_valid(*diameter)
                            && angle.0.is_finite()
                            && angle.0 > 0.0
                            && angle.0 < std::f64::consts::PI
                    }
                    HoleKind::Threaded {
                        major_diameter,
                        thread_depth,
                        pitch,
                        drill_point_angle,
                    } => {
                        positive_feature_length(*major_diameter)
                            && positive_feature_length(*thread_depth)
                            && pitch.is_none_or(positive_feature_length)
                            && drill_point_angle.0.is_finite()
                            && drill_point_angle.0 > 0.0
                            && drill_point_angle.0 < std::f64::consts::PI
                            && diameter.is_some_and(|diameter| major_diameter.0 > diameter.0)
                    }
                    HoleKind::Counterdrill {
                        diameter,
                        entry_diameter,
                        depth,
                        angle,
                    } => {
                        treatment_diameter_valid(*diameter)
                            && entry_diameter.is_none_or(|entry| {
                                positive_feature_length(entry) && entry.0 > diameter.0
                            })
                            && positive_feature_length(*depth)
                            && angle.0.is_finite()
                            && angle.0 > 0.0
                            && angle.0 < std::f64::consts::PI
                    }
                };
                let position_valid = position.is_none_or(|point| {
                    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
                });
                let placements_valid = placements.iter().all(|placement| {
                    let (point, direction) = match placement {
                        crate::features::HolePlacement::Directed {
                            position,
                            direction,
                        } => (position, direction),
                        crate::features::HolePlacement::Axis { origin, axis } => (origin, axis),
                    };
                    finite_feature_point(*point) && valid_feature_direction(*direction)
                });
                let filter_valid = profile_filter
                    .is_none_or(|filter| filter.points || filter.circles || filter.arcs);
                let bottom_valid = bottom.is_none_or(|bottom| match bottom {
                    crate::features::HoleBottom::Flat => true,
                    crate::features::HoleBottom::Angled { included_angle, .. } => {
                        included_angle.0.is_finite()
                            && included_angle.0 > 0.0
                            && included_angle.0 < std::f64::consts::PI
                    }
                });
                let taper_valid = taper_angle.is_none_or(|angle| {
                    angle.0.is_finite() && angle.0 > 0.0 && angle.0 < std::f64::consts::PI
                });
                let specification_valid = specification.as_deref().is_none_or(|specification| {
                    !specification.standard.is_empty()
                        && specification.pitch.is_none_or(positive_feature_length)
                        && specification
                            .major_diameter
                            .is_none_or(positive_feature_length)
                        && specification
                            .clearance
                            .is_none_or(|value| value.0.is_finite())
                        && match specification.depth {
                            crate::features::HoleThreadDepth::Blind { depth } => {
                                positive_feature_length(depth)
                            }
                            crate::features::HoleThreadDepth::HoleDepth
                            | crate::features::HoleThreadDepth::TappedStandard => true,
                        }
                });
                if diameter.is_some_and(|value| !positive_feature_length(value))
                    || !kind_valid(kind)
                    || exit_kind.as_ref().is_some_and(|kind| !kind_valid(kind))
                    || !position_valid
                    || !placements_valid
                    || !filter_valid
                    || !bottom_valid
                    || !taper_valid
                    || !specification_valid
                    || direction.is_some_and(|value| !valid_feature_direction(value))
                {
                    feature_geometry_error(findings, feature, "hole geometry is invalid");
                }
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                collect_pattern_paths(pattern, &mut paths);
                for seed in seeds {
                    match seed {
                        PatternSeed::Feature(seed) => match features.get(seed.0.as_str()) {
                            None => ref_error(findings, &feature.id.0, "seed feature", &seed.0),
                            Some(ordinal) if *ordinal >= feature.ordinal => {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "seed feature `{}` does not precede its pattern",
                                        seed.0
                                    ),
                                    entity: Some(feature.id.0.clone()),
                                });
                            }
                            Some(_) if !feature.dependencies.contains(seed) => {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "pattern omits seed feature `{}` from its dependencies",
                                        seed.0
                                    ),
                                    entity: Some(feature.id.0.clone()),
                                });
                            }
                            Some(_) => {}
                        },
                        PatternSeed::Faces(selection) => face_selections.push(selection),
                        PatternSeed::Bodies(selection) => body_selections.push(selection),
                        PatternSeed::Occurrences(occurrences) => {
                            if occurrences.is_empty() {
                                feature_geometry_error(
                                    findings,
                                    feature,
                                    "pattern occurrence seed is empty",
                                );
                            }
                            let mut unique = HashSet::new();
                            for occurrence in occurrences {
                                if !unique.insert(occurrence) {
                                    feature_geometry_error(
                                        findings,
                                        feature,
                                        "pattern occurrence seed is repeated",
                                    );
                                }
                                if !ir
                                    .model
                                    .occurrences
                                    .iter()
                                    .any(|candidate| candidate.id == *occurrence)
                                {
                                    ref_error(
                                        findings,
                                        &feature.id.0,
                                        "seed occurrence",
                                        &occurrence.0,
                                    );
                                }
                            }
                        }
                    }
                }
                let valid = pattern_is_valid(pattern, false);
                if !valid {
                    feature_geometry_error(findings, feature, "pattern geometry is invalid");
                }
            }
            FeatureDefinition::Sketch { sketch, .. } => {
                if let Some(sketch) = sketch {
                    if !ir.model.sketches.iter().any(|value| value.id == *sketch) {
                        ref_error(findings, &feature.id.0, "owned sketch", &sketch.0);
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
                        ref_error(findings, &feature.id.0, "owned spatial sketch", &sketch.0);
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
                    && [x_axis, y_axis, z_axis].into_iter().all(|axis| {
                        (axis.norm() - 1.0).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    })
                    && dot(*x_axis, *y_axis).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    && dot(*x_axis, *z_axis).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    && dot(*y_axis, *z_axis).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    && dot(cross, *z_axis) >= 1.0 - EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9;
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
                if matches!(direction, crate::features::CurveProjectionDirection::Vector(value) if !valid_feature_direction(*value))
                {
                    feature_geometry_error(findings, feature, "projection direction is invalid");
                }
            }
            FeatureDefinition::ProjectOnSurface {
                sources,
                support_face,
                direction,
                height,
                offset,
                ..
            } => {
                paths.push(sources);
                face_selections.push(support_face);
                if !valid_feature_direction(*direction)
                    || !height.0.is_finite()
                    || height.0 < 0.0
                    || !offset.0.is_finite()
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "projection-on-surface construction is invalid",
                    );
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
                shape,
                revolutions,
                segment_turns,
                ..
            } => {
                let shape_valid = match shape {
                    crate::features::HelixShape::Cylindrical { .. } => true,
                    crate::features::HelixShape::Conical { cone_angle, .. } => {
                        cone_angle.0.is_finite() && cone_angle.0.abs() < std::f64::consts::FRAC_PI_2
                    }
                    crate::features::HelixShape::Spiral { radial_growth } => {
                        radial_growth.0.is_finite()
                    }
                };
                let valid = [axis_origin.x, axis_origin.y, axis_origin.z]
                    .into_iter()
                    .all(f64::is_finite)
                    && valid_feature_direction(*axis_direction)
                    && radius.0.is_finite()
                    && radius.0 > 0.0
                    && revolutions.is_finite()
                    && *revolutions > 0.0
                    && shape_valid
                    && segment_turns.is_none_or(|value| value.is_finite() && value > 0.0);
                if !valid {
                    feature_geometry_error(findings, feature, "helix geometry is invalid");
                }
            }
            FeatureDefinition::HelixNativeAxis {
                axis_native_ref,
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                ..
            } => {
                let valid = !axis_native_ref.is_empty()
                    && axial_rise.0.is_finite()
                    && pitch.0.is_finite()
                    && revolutions.is_finite()
                    && *revolutions > 0.0
                    && start_angle.0.is_finite();
                if !valid {
                    feature_geometry_error(findings, feature, "native-axis helix is invalid");
                }
            }
            FeatureDefinition::Coil {
                construction,
                result,
            } => {
                use crate::features::{CoilExtent, CoilPlacement, CoilResult, CoilSection};

                let placement_valid = match &construction.placement {
                    CoilPlacement::Explicit {
                        origin,
                        axis,
                        radial,
                    } => {
                        let dot = axis.x * radial.x + axis.y * radial.y + axis.z * radial.z;
                        [origin.x, origin.y, origin.z]
                            .into_iter()
                            .all(f64::is_finite)
                            && (axis.norm() - 1.0).abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                            && (radial.norm() - 1.0).abs()
                                <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                            && dot.abs() <= EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                    }
                    CoilPlacement::Native { native_ref } => !native_ref.trim().is_empty(),
                };
                let extent_valid = match construction.extent {
                    CoilExtent::RevolutionsHeight {
                        revolutions,
                        height,
                    } => revolutions.is_finite() && revolutions > 0.0 && height.0.is_finite(),
                    CoilExtent::RevolutionsPitch { revolutions, pitch } => {
                        revolutions.is_finite()
                            && revolutions > 0.0
                            && pitch.0.is_finite()
                            && pitch.0 != 0.0
                    }
                    CoilExtent::HeightPitch { height, pitch } => {
                        height.0.is_finite()
                            && height.0 != 0.0
                            && pitch.0.is_finite()
                            && pitch.0 != 0.0
                    }
                    CoilExtent::Spiral {
                        revolutions,
                        radial_pitch,
                    } => {
                        revolutions.is_finite()
                            && revolutions > 0.0
                            && radial_pitch.0.is_finite()
                            && radial_pitch.0 != 0.0
                    }
                };
                let section_size = match construction.section {
                    CoilSection::Circular { diameter } => diameter,
                    CoilSection::Square { size }
                    | CoilSection::ExternalTriangle { size }
                    | CoilSection::InternalTriangle { size } => size,
                };
                if !placement_valid
                    || !positive_feature_length(construction.diameter)
                    || !extent_valid
                    || !positive_feature_length(section_size)
                    || !construction.taper.0.is_finite()
                {
                    feature_geometry_error(findings, feature, "coil geometry is invalid");
                }
                if let CoilResult::Boolean { operation, targets } = result {
                    if matches!(
                        operation,
                        crate::features::BooleanOp::Unresolved
                            | crate::features::BooleanOp::NewBody
                    ) {
                        feature_geometry_error(findings, feature, "coil Boolean result is invalid");
                    }
                    body_selections.push(targets);
                }
            }
            FeatureDefinition::HelicalSweep { construction, .. } => {
                let valid = [
                    construction.axis_origin.x,
                    construction.axis_origin.y,
                    construction.axis_origin.z,
                    construction.pitch.0,
                    construction.height.0,
                    construction.radial_growth.0,
                    construction.cone_angle.0,
                ]
                .into_iter()
                .all(f64::is_finite)
                    && valid_feature_direction(construction.axis_direction)
                    && construction.pitch.0 >= 0.0
                    && construction.turns.is_finite()
                    && construction.turns > 0.0
                    && construction
                        .tolerance
                        .is_none_or(|tolerance| tolerance.is_finite() && tolerance > 0.0)
                    && (construction.height.0 != 0.0 || construction.radial_growth.0 != 0.0);
                if !valid {
                    feature_geometry_error(findings, feature, "helical sweep is invalid");
                }
            }
            FeatureDefinition::Binder {
                sources,
                construction,
            } => {
                let target_valid = |target: &crate::features::BinderTarget| match target {
                    crate::features::BinderTarget::Feature { .. } => true,
                    crate::features::BinderTarget::External { document, object } => {
                        !document.is_empty() && !object.is_empty()
                    }
                    crate::features::BinderTarget::Native { reference } => !reference.is_empty(),
                };
                let sources_valid = sources.iter().all(|source| {
                    target_valid(&source.target)
                        && source
                            .subelements
                            .iter()
                            .all(|selector| !selector.is_empty())
                });
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
                        match features.get(target.0.as_str()) {
                            None => ref_error(
                                findings,
                                &feature.id.0,
                                "binder target feature",
                                &target.0,
                            ),
                            Some(ordinal) if *ordinal >= feature.ordinal => {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "binder target feature `{}` does not precede its binder",
                                        target.0
                                    ),
                                    entity: Some(feature.id.0.clone()),
                                });
                            }
                            Some(_) => {}
                        }
                    }
                }
                let construction_valid = match construction {
                    crate::features::BinderConstruction::Shape { .. } => true,
                    crate::features::BinderConstruction::SubShape {
                        offset, context, ..
                    } => {
                        context.as_ref().is_none_or(target_valid)
                            && offset.is_none_or(|offset| {
                                offset.distance.0.is_finite() && offset.distance.0 != 0.0
                            })
                    }
                };
                if !sources_valid || !construction_valid {
                    feature_geometry_error(findings, feature, "binder construction is invalid");
                }
            }
            FeatureDefinition::Wrap { face, .. } => {
                face_selections.push(face);
            }
            FeatureDefinition::Sphere { center, radius, .. } => {
                if ![center.x, center.y, center.z]
                    .into_iter()
                    .all(f64::is_finite)
                    || !positive_feature_length(*radius)
                {
                    feature_geometry_error(findings, feature, "sphere primitive is invalid");
                }
            }
            FeatureDefinition::Torus {
                center,
                axis,
                major_radius,
                minor_radius,
                ..
            } => {
                if ![center.x, center.y, center.z]
                    .into_iter()
                    .all(f64::is_finite)
                    || !valid_feature_direction(*axis)
                    || !positive_feature_length(*major_radius)
                    || !positive_feature_length(*minor_radius)
                {
                    feature_geometry_error(findings, feature, "torus primitive is invalid");
                }
            }
            FeatureDefinition::PointGeometry { position } => {
                if !finite_feature_point(*position) {
                    feature_geometry_error(findings, feature, "point geometry is invalid");
                }
            }
            FeatureDefinition::LineSegment { start, end } => {
                if !finite_feature_point(*start) || !finite_feature_point(*end) || *start == *end {
                    feature_geometry_error(findings, feature, "line segment is invalid");
                }
            }
            FeatureDefinition::CircularArc {
                center,
                normal,
                radius,
                start_angle,
                end_angle,
            } => {
                if !finite_feature_point(*center)
                    || !valid_feature_direction(*normal)
                    || !positive_feature_length(*radius)
                    || !start_angle.0.is_finite()
                    || !end_angle.0.is_finite()
                    || start_angle == end_angle
                {
                    feature_geometry_error(findings, feature, "circular arc is invalid");
                }
            }
            FeatureDefinition::EllipticArc {
                center,
                normal,
                major_axis,
                major_radius,
                minor_radius,
                start_angle,
                end_angle,
            } => {
                if !finite_feature_point(*center)
                    || !valid_feature_direction(*normal)
                    || !valid_feature_direction(*major_axis)
                    || (normal.x * major_axis.x + normal.y * major_axis.y + normal.z * major_axis.z)
                        .abs()
                        > EPS_TORUS_AXES_ORTHO
                    || !positive_feature_length(*major_radius)
                    || !positive_feature_length(*minor_radius)
                    || minor_radius.0 > major_radius.0
                    || !start_angle.0.is_finite()
                    || !end_angle.0.is_finite()
                    || start_angle == end_angle
                {
                    feature_geometry_error(findings, feature, "elliptic arc is invalid");
                }
            }
            FeatureDefinition::Polyline { points, closed } => {
                if points.len() < 2
                    || (*closed && points.len() < 3)
                    || points.iter().any(|point| !finite_feature_point(*point))
                    || points.windows(2).any(|pair| pair[0] == pair[1])
                {
                    feature_geometry_error(findings, feature, "polyline is invalid");
                }
            }
            FeatureDefinition::RegularPolygonCurve {
                sides,
                circumradius,
            } => {
                if *sides < 3 || !positive_feature_length(*circumradius) {
                    feature_geometry_error(findings, feature, "regular polygon is invalid");
                }
            }
            FeatureDefinition::PlanarPatch { length, width } => {
                if !positive_feature_length(*length) || !positive_feature_length(*width) {
                    feature_geometry_error(findings, feature, "planar patch is invalid");
                }
            }
            FeatureDefinition::FaceFromShapes {
                sources,
                face_maker_class,
            } => {
                body_selections.push(sources);
                if face_maker_class.is_empty() {
                    feature_geometry_error(findings, feature, "face construction is invalid");
                }
            }
            FeatureDefinition::TreeNode {
                children,
                active_child,
                ..
            } => {
                let mut seen = HashSet::new();
                for child in children {
                    let child_record = ir
                        .model
                        .features
                        .iter()
                        .find(|candidate| candidate.id == *child);
                    match child_record {
                        None => ref_error(findings, &feature.id.0, "tree child", &child.0),
                        Some(_) if !seen.insert(child) => findings.push(Finding {
                            check: Check::Counts,
                            severity: Severity::Error,
                            message: format!("tree node repeats child `{}`", child.0),
                            entity: Some(feature.id.0.clone()),
                        }),
                        Some(child_record) if child_record.parent.as_ref() != Some(&feature.id) => {
                            findings.push(Finding {
                                check: Check::ReferentialIntegrity,
                                severity: Severity::Error,
                                message: format!(
                                    "tree child `{}` does not name its owning parent",
                                    child.0
                                ),
                                entity: Some(feature.id.0.clone()),
                            });
                        }
                        Some(_) => {}
                    }
                }
                if let Some(active_child) = active_child {
                    if !children.contains(active_child) {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "active tree child `{}` is not an owned child",
                                active_child.0
                            ),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
                }
            }
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } => {
                let scale = normal.norm() * u_axis.norm();
                if !finite_feature_point(*origin)
                    || !valid_feature_direction(*normal)
                    || !valid_feature_direction(*u_axis)
                    || !scale.is_finite()
                    || normal.dot(*u_axis).abs() > EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9 * scale
                {
                    feature_geometry_error(findings, feature, "datum-plane frame is invalid");
                }
            }
            FeatureDefinition::DatumThreePointPlane {
                origin,
                normal,
                u_axis,
                points,
            } => {
                let scale = normal.norm() * u_axis.norm();
                if !finite_feature_point(*origin)
                    || !valid_feature_direction(*normal)
                    || !valid_feature_direction(*u_axis)
                    || !scale.is_finite()
                    || normal.dot(*u_axis).abs() > EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9 * scale
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "three-point datum-plane frame is invalid",
                    );
                }
                if same_vertex_target(&points[0], &points[1])
                    || same_vertex_target(&points[0], &points[2])
                    || same_vertex_target(&points[1], &points[2])
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "three-point datum plane requires three distinct vertices",
                    );
                }
                let mut historical_states = points.iter().filter_map(|point| match point {
                    crate::features::VertexSelection::Historical { state, .. } => Some(state),
                    _ => None,
                });
                if let Some(state) = historical_states.next() {
                    if historical_states.any(|candidate| candidate != state) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "three-point datum-plane vertices use different input topologies",
                        );
                    }
                }
                for point in points.iter() {
                    vertex_selections.push((point, "three-point datum-plane"));
                }
            }
            FeatureDefinition::DatumAxis { origin, direction } => {
                if !finite_feature_point(*origin) || !valid_feature_direction(*direction) {
                    feature_geometry_error(findings, feature, "datum-axis frame is invalid");
                }
            }
            FeatureDefinition::DatumPoint {
                position,
                construction,
            } => {
                if !finite_feature_point(*position) {
                    feature_geometry_error(findings, feature, "datum-point position is invalid");
                }
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
                    if matches!(
                        construction,
                        crate::features::DatumPointConstruction::DistanceOnEdge { fraction, .. }
                            if !fraction.is_finite() || !(0.0..=1.0).contains(fraction)
                    ) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "datum-point path fraction is invalid",
                        );
                    }
                }
                for plane in plane_references {
                    match plane {
                        DatumPlaneReference::Feature(reference) => {
                            match feature_records.get(reference.0.as_str()) {
                                None => ref_error(
                                    findings,
                                    &feature.id.0,
                                    "datum-point plane",
                                    &reference.0,
                                ),
                                Some(record)
                                    if !matches!(
                                        record.definition,
                                        FeatureDefinition::DatumPrincipalPlane { .. }
                                            | FeatureDefinition::DatumPlane { .. }
                                            | FeatureDefinition::DatumPlaneUnresolved
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
                                            reference.0
                                        ),
                                        entity: Some(feature.id.0.clone()),
                                    });
                                }
                                Some(_) => {}
                            }
                        }
                        DatumPlaneReference::Face {
                            face,
                            origin,
                            normal,
                            u_axis,
                        } => {
                            face_selections.push(face);
                            if !finite_feature_point(*origin)
                                || !valid_feature_direction(*normal)
                                || !valid_feature_direction(*u_axis)
                                || normal.dot(*u_axis).abs()
                                    > EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                                        * normal.norm()
                                        * u_axis.norm()
                            {
                                feature_geometry_error(
                                    findings,
                                    feature,
                                    "datum-point plane support frame is invalid",
                                );
                            }
                        }
                    }
                }
            }
            FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlaneUnresolved
            | FeatureDefinition::BrepUnresolved
            | FeatureDefinition::MoveFaceUnresolved
            | FeatureDefinition::CylinderUnresolved
            | FeatureDefinition::ConeUnresolved
            | FeatureDefinition::SphereUnresolved
            | FeatureDefinition::ThreadUnresolved
            | FeatureDefinition::DetailedThreadUnresolved
            | FeatureDefinition::SketchBlockDefinition { .. }
            | FeatureDefinition::StoredGeometry
            | FeatureDefinition::Native { .. } => {}
            FeatureDefinition::SketchBlockInstance { block, placement } => {
                if let Some(block) = block {
                    match features.get(block.0.as_str()) {
                        None => ref_error(findings, &feature.id.0, "sketch block", &block.0),
                        Some(ordinal) if *ordinal >= feature.ordinal => feature_geometry_error(
                            findings,
                            feature,
                            "sketch block does not precede its instance",
                        ),
                        Some(_)
                            if !ir.model.features.iter().any(|candidate| {
                                candidate.id == *block
                                    && matches!(
                                        candidate.definition,
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
                                    block.0
                                ),
                                entity: Some(feature.id.0.clone()),
                            });
                        }
                        Some(_) => {}
                    }
                }
                if placement.is_some_and(|placement| !placement.is_affine()) {
                    feature_geometry_error(findings, feature, "sketch block placement is invalid");
                }
            }
            FeatureDefinition::DerivedGeometry { source } => {
                match features.get(source.0.as_str()) {
                    None => ref_error(findings, &feature.id.0, "source feature", &source.0),
                    Some(ordinal) if *ordinal >= feature.ordinal => findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "source feature `{}` does not precede its derived geometry",
                            source.0
                        ),
                        entity: Some(feature.id.0.clone()),
                    }),
                    Some(_) if !feature.dependencies.contains(source) => findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "derived geometry omits source feature `{}` from its dependencies",
                            source.0
                        ),
                        entity: Some(feature.id.0.clone()),
                    }),
                    Some(_) => {}
                }
            }
            FeatureDefinition::ImportedGeometry { path, .. } => {
                if path.is_empty() || path.contains('\0') {
                    feature_geometry_error(findings, feature, "geometry import path is invalid");
                }
            }
            FeatureDefinition::PostProcess { .. } => feature_geometry_error(
                findings,
                feature,
                "nested feature post-processing is invalid",
            ),
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } => {
                if let Some(reference) = reference {
                    match reference {
                        DatumPlaneReference::Feature(reference) => {
                            match feature_records.get(reference.0.as_str()) {
                                None => {
                                    ref_error(
                                        findings,
                                        &feature.id.0,
                                        "reference plane",
                                        &reference.0,
                                    );
                                }
                                Some(record)
                                    if !matches!(
                                        record.definition,
                                        FeatureDefinition::DatumPrincipalPlane { .. }
                                            | FeatureDefinition::DatumPlane { .. }
                                            | FeatureDefinition::DatumPlaneUnresolved
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
                                            reference.0
                                        ),
                                        entity: Some(feature.id.0.clone()),
                                    });
                                }
                                Some(_) if !feature.dependencies.contains(reference) => {
                                    findings.push(Finding {
                                        check: Check::ReferentialIntegrity,
                                        severity: Severity::Error,
                                        message: format!(
                                            "offset plane omits reference feature `{}` from its dependencies",
                                            reference.0
                                        ),
                                        entity: Some(feature.id.0.clone()),
                                    });
                                }
                                Some(_) => {}
                            }
                        }
                        DatumPlaneReference::Face {
                            face,
                            origin,
                            normal,
                            u_axis,
                        } => {
                            face_selections.push(face);
                            if !origin.x.is_finite()
                                || !origin.y.is_finite()
                                || !origin.z.is_finite()
                                || !valid_feature_direction(*normal)
                                || !valid_feature_direction(*u_axis)
                                || (normal.x * u_axis.x + normal.y * u_axis.y + normal.z * u_axis.z)
                                    .abs()
                                    > EPS_TOPOLOGY_CHECK_FEATURE_REFERENCES_E9
                                        * normal.norm()
                                        * u_axis.norm()
                            {
                                feature_geometry_error(
                                    findings,
                                    feature,
                                    "datum-plane face support frame is invalid",
                                );
                            }
                        }
                    }
                }
                if !distance.0.is_finite() {
                    feature_geometry_error(findings, feature, "datum-plane offset is invalid");
                }
            }
        }
        for profile in definition_profiles(definition) {
            match profile {
                ProfileRef::Faces(faces) => check_ids(
                    findings,
                    &feature.id.0,
                    "profile face",
                    faces.iter().map(|id| id.0.as_str()),
                    |identity| ids.faces(identity).is_some(),
                ),
                ProfileRef::HistoricalFaces {
                    state,
                    faces,
                    native,
                } => {
                    if native.is_empty()
                        || native.iter().any(String::is_empty)
                        || native.iter().collect::<HashSet<_>>().len() != native.len()
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "historical profile source groups are empty or repeated",
                        );
                    }
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            faces.iter().map(crate::ids::HistoricalFaceId::as_str),
                            native.first().map_or("", String::as_str),
                        ),
                        "profile face",
                        false,
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
                ProfileRef::Feature(producer) => match features.get(producer.0.as_str()) {
                    None => ref_error(findings, &feature.id.0, "profile feature", &producer.0),
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
                ProfileRef::Generated { curves, native }
                    if curves.is_empty()
                        || native.trim().is_empty()
                        || curves.iter().any(|curve| {
                            curve.local_id.trim().is_empty()
                                || features
                                    .get(curve.feature.0.as_str())
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
                    &feature.id.0,
                    "path edge",
                    edges.iter().map(|id| id.0.as_str()),
                    |identity| ids.edges(identity).is_some(),
                ),
                PathRef::Curves(curves) => check_ids(
                    findings,
                    &feature.id.0,
                    "path curve",
                    curves.iter().map(|id| id.0.as_str()),
                    |identity| ids.curves(identity).is_some(),
                ),
                PathRef::SketchCurves { curves, .. } => check_ids(
                    findings,
                    &feature.id.0,
                    "sketch path curve",
                    curves.iter().map(|id| id.0.as_str()),
                    |identity| sketch_entities.contains(identity),
                ),
                PathRef::SpatialSketchCurves { curves, .. } => check_ids(
                    findings,
                    &feature.id.0,
                    "spatial sketch path curve",
                    curves.iter().map(|id| id.0.as_str()),
                    |identity| spatial_sketch_entity_owners.contains_key(identity),
                ),
                PathRef::HistoricalEdges {
                    state,
                    edges,
                    native,
                } => check_historical_selection(
                    findings,
                    &feature.id,
                    (
                        state,
                        edges.iter().map(crate::ids::HistoricalEdgeId::as_str),
                        native,
                    ),
                    "path edge",
                    false,
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
            if !termination_magnitude_is_valid(termination) {
                findings.push(Finding {
                    check: Check::GeometricConsistency,
                    severity: Severity::Error,
                    message: "feature extent magnitude is invalid".into(),
                    entity: Some(feature.id.0.clone()),
                });
            }
            if let Termination::ToFace {
                face: FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. },
                ..
            } = termination
            {
                check_ids(
                    findings,
                    &feature.id.0,
                    "termination face",
                    faces.iter().map(|id| id.0.as_str()),
                    |identity| ids.faces(identity).is_some(),
                );
            }
            if let Termination::ToShape {
                target: FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. },
            } = termination
            {
                check_ids(
                    findings,
                    &feature.id.0,
                    "termination shape face",
                    faces.iter().map(|id| id.0.as_str()),
                    |identity| ids.faces(identity).is_some(),
                );
            }
            if let Termination::ToVertex { vertex } = termination {
                vertex_selections.push((vertex, "termination"));
            }
        }
        for (selection, consumer) in vertex_selections {
            match selection {
                crate::features::VertexSelection::Generated { vertex, native } => {
                    if native.trim().is_empty()
                        || vertex.local_id.trim().is_empty()
                        || features
                            .get(vertex.feature.0.as_str())
                            .is_none_or(|ordinal| *ordinal >= feature.ordinal)
                        || !feature.dependencies.contains(&vertex.feature)
                        || result_topologies_by_feature
                            .get(vertex.feature.as_str())
                            .is_some_and(|state| !state.vertices.contains(&vertex.local_id))
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
                    native,
                } => check_historical_selection(
                    findings,
                    &feature.id,
                    (state, std::iter::once(vertex.as_str()), native),
                    "vertex",
                    false,
                    &input_topologies,
                    |topology| {
                        topology
                            .vertices
                            .iter()
                            .map(crate::ids::HistoricalVertexId::as_str)
                            .collect()
                    },
                ),
                crate::features::VertexSelection::Native(native) if native.trim().is_empty() => {
                    feature_geometry_error(
                        findings,
                        feature,
                        &format!("native {consumer} vertex is invalid"),
                    );
                }
                crate::features::VertexSelection::Unresolved
                | crate::features::VertexSelection::Native(_) => {}
            }
        }
        for selection in edge_selections {
            let allow_empty = matches!(selection, EdgeSelection::HistoricalPartial { .. });
            match selection {
                EdgeSelection::Edges(edges) | EdgeSelection::Resolved { edges, .. } => check_ids(
                    findings,
                    &feature.id.0,
                    "selected edge",
                    edges.iter().map(|id| id.0.as_str()),
                    |identity| ids.edges(identity).is_some(),
                ),
                EdgeSelection::Historical {
                    state,
                    edges,
                    native,
                }
                | EdgeSelection::HistoricalPartial {
                    state,
                    edges,
                    native,
                    ..
                } => {
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            edges.iter().map(crate::ids::HistoricalEdgeId::as_str),
                            native,
                        ),
                        "edge",
                        allow_empty,
                        &input_topologies,
                        |topology| {
                            topology
                                .edges
                                .iter()
                                .map(crate::ids::HistoricalEdgeId::as_str)
                                .collect()
                        },
                    );
                    if let EdgeSelection::HistoricalPartial { unresolved, .. } = selection {
                        let mut identities = HashSet::new();
                        for identity in unresolved {
                            if identity.trim().is_empty() {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: "partial historical edge selection has an empty unresolved operand identity".into(),
                                    entity: Some(feature.id.0.clone()),
                                });
                            } else if !identities.insert(identity) {
                                findings.push(Finding {
                                    check: Check::ReferentialIntegrity,
                                    severity: Severity::Error,
                                    message: format!(
                                        "partial historical edge selection repeats unresolved operand `{identity}`"
                                    ),
                                    entity: Some(feature.id.0.clone()),
                                });
                            }
                        }
                        if unresolved.is_empty() {
                            findings.push(Finding {
                                check: Check::ReferentialIntegrity,
                                severity: Severity::Error,
                                message:
                                    "partial historical edge selection has no unresolved operands"
                                        .into(),
                                entity: Some(feature.id.0.clone()),
                            });
                        }
                    }
                }
                EdgeSelection::Generated { edges, native } => {
                    if edges.is_empty()
                        || native.trim().is_empty()
                        || edges.iter().any(|edge| {
                            edge.local_id.trim().is_empty()
                                || !feature.dependencies.contains(&edge.feature)
                                || result_topologies_by_feature
                                    .get(edge.feature.as_str())
                                    .is_some_and(|state| !state.edges.contains(&edge.local_id))
                        })
                    {
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
            match selection {
                FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => check_ids(
                    findings,
                    &feature.id.0,
                    "selected face",
                    faces.iter().map(|id| id.0.as_str()),
                    |identity| ids.faces(identity).is_some(),
                ),
                FaceSelection::Historical {
                    state,
                    faces,
                    native,
                } => {
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            faces.iter().map(crate::ids::HistoricalFaceId::as_str),
                            native,
                        ),
                        "face",
                        false,
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
                FaceSelection::HistoricalPartial {
                    state,
                    faces,
                    unresolved,
                    native,
                } => {
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            faces.iter().map(crate::ids::HistoricalFaceId::as_str),
                            native,
                        ),
                        "face",
                        true,
                        &input_topologies,
                        |topology| {
                            topology
                                .faces
                                .iter()
                                .map(crate::ids::HistoricalFaceId::as_str)
                                .collect()
                        },
                    );
                    let mut identities = HashSet::new();
                    for identity in unresolved {
                        if identity.trim().is_empty() {
                            findings.push(Finding {
                                check: Check::ReferentialIntegrity,
                                severity: Severity::Error,
                                message: "partial historical face selection has an empty unresolved operand identity".into(),
                                entity: Some(feature.id.0.clone()),
                            });
                        } else if !identities.insert(identity) {
                            findings.push(Finding {
                                check: Check::ReferentialIntegrity,
                                severity: Severity::Error,
                                message: format!("partial historical face selection repeats unresolved operand `{identity}`"),
                                entity: Some(feature.id.0.clone()),
                            });
                        }
                    }
                    if unresolved.is_empty() {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: "partial historical face selection has no unresolved operands"
                                .into(),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
                }
                FaceSelection::Generated { faces, native } => {
                    if faces.is_empty()
                        || native.trim().is_empty()
                        || faces.iter().any(|face| {
                            face.local_id.trim().is_empty()
                                || !feature.dependencies.contains(&face.feature)
                                || result_topologies_by_feature
                                    .get(face.feature.as_str())
                                    .is_some_and(|state| !state.faces.contains(&face.local_id))
                        })
                    {
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
                        &feature.id.0,
                        "selected body",
                        bodies.iter().map(|id| id.0.as_str()),
                        |identity| ids.bodies(identity).is_some(),
                    );
                }
                BodySelection::ResolvedSet { bodies, native } => {
                    check_ids(
                        findings,
                        &feature.id.0,
                        "selected body",
                        bodies.iter().map(|id| id.0.as_str()),
                        |identity| ids.bodies(identity).is_some(),
                    );
                    if bodies.len() != native.len()
                        || native.is_empty()
                        || bodies.iter().collect::<HashSet<_>>().len() != bodies.len()
                        || native.iter().any(|member| member.trim().is_empty())
                        || native.iter().collect::<HashSet<_>>().len() != native.len()
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "resolved body selection set is invalid",
                        );
                    }
                }
                BodySelection::Historical {
                    state,
                    bodies,
                    native,
                } => {
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            bodies.iter().map(crate::ids::HistoricalBodyId::as_str),
                            native,
                        ),
                        "body",
                        false,
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
                BodySelection::HistoricalSet {
                    state,
                    bodies,
                    native,
                } => {
                    let native_is_valid = bodies.len() == native.len()
                        && !native.is_empty()
                        && native.iter().all(|member| !member.trim().is_empty())
                        && native.iter().collect::<HashSet<_>>().len() == native.len();
                    if !native_is_valid {
                        feature_geometry_error(
                            findings,
                            feature,
                            "historical body selection set is invalid",
                        );
                    }
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            bodies.iter().map(crate::ids::HistoricalBodyId::as_str),
                            native.first().map_or("", String::as_str),
                        ),
                        "body",
                        false,
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
                BodySelection::HistoricalUnorderedSet {
                    state,
                    bodies,
                    native,
                } => {
                    let set_is_valid = bodies.len() == native.len()
                        && !native.is_empty()
                        && native.iter().all(|member| !member.trim().is_empty())
                        && native.iter().collect::<HashSet<_>>().len() == native.len();
                    if !set_is_valid {
                        feature_geometry_error(
                            findings,
                            feature,
                            "historical unordered body selection set is invalid",
                        );
                    }
                    check_historical_selection(
                        findings,
                        &feature.id,
                        (
                            state,
                            bodies.iter().map(crate::ids::HistoricalBodyId::as_str),
                            native.first().map_or("", String::as_str),
                        ),
                        "body",
                        false,
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
                BodySelection::Generated { bodies, native } => {
                    if bodies.is_empty()
                        || native.trim().is_empty()
                        || bodies.iter().any(|body| {
                            body.local_id.trim().is_empty()
                                || !feature.dependencies.contains(&body.feature)
                                || result_topologies_by_feature
                                    .get(body.feature.as_str())
                                    .is_some_and(|state| !state.bodies.contains(&body.local_id))
                        })
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "generated body selection is invalid",
                        );
                    }
                }
                BodySelection::Local { bodies, native } => {
                    if bodies.is_empty()
                        || native.trim().is_empty()
                        || bodies.iter().any(|body| body.trim().is_empty())
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "local body selection is invalid",
                        );
                    }
                }
                BodySelection::NativeSet(members) => {
                    if members.is_empty() || members.iter().any(|member| member.trim().is_empty()) {
                        feature_geometry_error(
                            findings,
                            feature,
                            "native body selection set is invalid",
                        );
                    }
                }
                BodySelection::Unresolved | BodySelection::Native(_) => {}
            }
        }
    }
}

fn same_vertex_target(
    first: &crate::features::VertexSelection,
    second: &crate::features::VertexSelection,
) -> bool {
    use crate::features::VertexSelection;

    match (first, second) {
        (
            VertexSelection::Generated { vertex: first, .. },
            VertexSelection::Generated { vertex: second, .. },
        ) => first == second,
        (
            VertexSelection::Historical {
                state: first_state,
                vertex: first_vertex,
                ..
            },
            VertexSelection::Historical {
                state: second_state,
                vertex: second_vertex,
                ..
            },
        ) => first_state == second_state && first_vertex == second_vertex,
        (VertexSelection::Native(first), VertexSelection::Native(second)) => first == second,
        (VertexSelection::Unresolved, VertexSelection::Unresolved) => true,
        _ => false,
    }
}

fn check_historical_selection<'a, I, F>(
    findings: &mut Vec<Finding>,
    feature: &crate::features::FeatureId,
    selection: (&crate::ids::FeatureInputTopologyId, I, &str),
    kind: &str,
    allow_empty: bool,
    states: &HashMap<&str, &crate::features::FeatureInputTopology>,
    members: F,
) where
    I: IntoIterator<Item = &'a str>,
    F: FnOnce(&crate::features::FeatureInputTopology) -> Vec<&str>,
{
    let (state_id, selected, native) = selection;
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
            entity: Some(feature.0.clone()),
        });
    }
    if native.is_empty() {
        findings.push(Finding {
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: format!("historical {kind} selection has an empty native reference"),
            entity: Some(feature.0.clone()),
        });
    }
    let available = members(state).into_iter().collect::<HashSet<_>>();
    let selected = selected.into_iter().collect::<Vec<_>>();
    if selected.is_empty() && !allow_empty {
        findings.push(Finding {
            check: Check::Counts,
            severity: Severity::Error,
            message: format!("historical {kind} selection is empty"),
            entity: Some(feature.0.clone()),
        });
    }
    let mut seen = HashSet::new();
    for id in selected {
        if !seen.insert(id) {
            findings.push(Finding {
                check: Check::Counts,
                severity: Severity::Error,
                message: format!("historical {kind} selection repeats `{id}`"),
                entity: Some(feature.0.clone()),
            });
        }
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

fn positive_feature_length(value: Length) -> bool {
    value.0.is_finite() && value.0 > 0.0
}

fn valid_draft_angle(value: crate::features::Angle) -> bool {
    value.0.is_finite() && value.0.abs() < std::f64::consts::FRAC_PI_2
}

fn radius_spec_is_valid(radius: &RadiusSpec) -> bool {
    match radius {
        RadiusSpec::Unresolved { .. } => true,
        RadiusSpec::Constant { radius } => positive_feature_length(*radius),
        RadiusSpec::Chordal { chord_length } => positive_feature_length(*chord_length),
        RadiusSpec::Asymmetric {
            offset_one,
            offset_two,
        } => positive_feature_length(*offset_one) && positive_feature_length(*offset_two),
        RadiusSpec::Variable { points } => {
            points.len() >= 2
                && points.iter().all(|point| {
                    point.parameter.is_finite()
                        && (0.0..=1.0).contains(&point.parameter)
                        && point.radius.0.is_finite()
                        && point.radius.0 >= 0.0
                })
                && points.iter().any(|point| point.radius.0 > 0.0)
                && points
                    .windows(2)
                    .all(|pair| pair[0].parameter < pair[1].parameter)
        }
    }
}

fn finite_feature_point(value: Point3) -> bool {
    [value.x, value.y, value.z].into_iter().all(f64::is_finite)
}

fn valid_feature_direction(value: Vector3) -> bool {
    value.norm().is_finite() && value.norm() > 0.0
}

fn parameter_value_is_valid(value: &crate::features::ParameterValue) -> bool {
    match value {
        crate::features::ParameterValue::Length(value) => value.0.is_finite(),
        crate::features::ParameterValue::Angle(value) => value.0.is_finite(),
        crate::features::ParameterValue::Real(value) => value.is_finite(),
        crate::features::ParameterValue::Integer(_)
        | crate::features::ParameterValue::Boolean(_)
        | crate::features::ParameterValue::String(_) => true,
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
    references.extend(definition_terminations(definition).filter_map(
        |termination| match termination {
            crate::features::Termination::ToVertex {
                vertex: crate::features::VertexSelection::Generated { vertex, .. },
            } => Some(&vertex.feature),
            _ => None,
        },
    ));
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
    let mut profiles = Vec::new();
    match definition {
        crate::features::FeatureDefinition::Extrude { profile, .. }
        | crate::features::FeatureDefinition::SheetMetalBaseFlange { profile, .. }
        | crate::features::FeatureDefinition::Wrap { profile, .. } => profiles.push(profile),
        crate::features::FeatureDefinition::Revolve { construction, .. } => {
            profiles.extend(&construction.profile);
        }
        crate::features::FeatureDefinition::Rib { construction, .. } => {
            profiles.extend(&construction.profile);
        }
        crate::features::FeatureDefinition::Sweep {
            section, sections, ..
        } => {
            profiles.extend(section.referenced_profile());
            profiles.extend(
                sections
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

fn definition_terminations(
    definition: &crate::features::FeatureDefinition,
) -> impl Iterator<Item = &crate::features::Termination> {
    let mut terminations = Vec::new();
    match definition {
        crate::features::FeatureDefinition::Extrude { extent, .. } => match extent {
            crate::features::ExtrudeExtent::OneSided { side }
            | crate::features::ExtrudeExtent::Symmetric { side } => {
                terminations.push(&side.termination);
            }
            crate::features::ExtrudeExtent::TwoSided { first, second } => {
                terminations.extend([&first.termination, &second.termination]);
            }
        },
        crate::features::FeatureDefinition::Revolve { construction, .. } => {
            match &construction.extent {
                Some(
                    crate::features::RevolveExtent::OneSided { termination }
                    | crate::features::RevolveExtent::Symmetric { termination },
                ) => terminations.push(termination),
                Some(crate::features::RevolveExtent::TwoSided { first, second }) => {
                    terminations.extend([first, second]);
                }
                None => {}
            }
        }
        crate::features::FeatureDefinition::Hole {
            extent: Some(extent),
            ..
        } => terminations.push(extent),
        _ => {}
    }
    terminations.into_iter()
}

fn termination_magnitude_is_valid(termination: &crate::features::Termination) -> bool {
    match termination {
        crate::features::Termination::Blind { length } => length.0.is_finite() && length.0 != 0.0,
        crate::features::Termination::Angle { angle } => angle.0.is_finite() && angle.0 > 0.0,
        crate::features::Termination::OffsetFromFace { offset, .. } => {
            offset.0.is_finite() && offset.0 > 0.0
        }
        crate::features::Termination::ToFace { offset, .. } => {
            offset.is_none_or(|offset| offset.0.is_finite())
        }
        crate::features::Termination::Unresolved
        | crate::features::Termination::ThroughAll
        | crate::features::Termination::ThroughNext
        | crate::features::Termination::ToFirst
        | crate::features::Termination::ToLast
        | crate::features::Termination::ToVertex { .. }
        | crate::features::Termination::ToShape { .. } => true,
    }
}

/// Check that a configuration's stated design state is self-contained: every
/// dependency an unsuppressed feature state names must itself have an
/// unsuppressed state in the same configuration.
///
/// Does not check body-to-feature attribution. Sparse `feature_states` are
/// allowed: a missing dependency state inherits the model-level one. Only an
/// explicitly suppressed dependency is incoherent.
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
        .filter(|(_, state)| !state.suppressed)
        .map(|(feature, _)| feature.clone())
        .collect::<HashSet<_>>();
    let mut pending = closure.iter().cloned().collect::<Vec<_>>();
    while let Some(feature) = pending.pop() {
        let state = &configuration.feature_states[&feature];
        for dependency in &state.dependencies {
            match configuration.feature_states.get(dependency) {
                None => {}
                Some(dependency_state) if dependency_state.suppressed => {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "configuration state closure uses suppressed dependency state `{}`",
                            dependency.0
                        ),
                        entity: Some(configuration.id.0.clone()),
                    });
                }
                Some(_) if closure.insert(dependency.clone()) => pending.push(dependency.clone()),
                Some(_) => {}
            }
        }
    }
}

fn face_selections_overlap(first: &FaceSelection, second: &FaceSelection) -> bool {
    fn direct(selection: &FaceSelection) -> Option<&[crate::ids::FaceId]> {
        match selection {
            FaceSelection::Faces(faces) | FaceSelection::Resolved { faces, .. } => {
                Some(faces.as_slice())
            }
            _ => None,
        }
    }
    fn historical(
        selection: &FaceSelection,
    ) -> Option<(
        &crate::ids::FeatureInputTopologyId,
        &[crate::ids::HistoricalFaceId],
    )> {
        match selection {
            FaceSelection::Historical { state, faces, .. }
            | FaceSelection::HistoricalPartial { state, faces, .. } => {
                Some((state, faces.as_slice()))
            }
            _ => None,
        }
    }
    if let Some((first, second)) = direct(first).zip(direct(second)) {
        return first.iter().any(|face| second.contains(face));
    }
    if let Some(((first_state, first), (second_state, second))) =
        historical(first).zip(historical(second))
    {
        return first_state == second_state && first.iter().any(|face| second.contains(face));
    }
    match (first, second) {
        (
            FaceSelection::Generated { faces: first, .. },
            FaceSelection::Generated { faces: second, .. },
        ) => first.iter().any(|face| second.contains(face)),
        _ => false,
    }
}

fn body_selections_overlap(first: &BodySelection, second: &BodySelection) -> bool {
    fn direct(selection: &BodySelection) -> Option<&[crate::ids::BodyId]> {
        match selection {
            BodySelection::Bodies(bodies)
            | BodySelection::Resolved { bodies, .. }
            | BodySelection::ResolvedSet { bodies, .. } => Some(bodies.as_slice()),
            _ => None,
        }
    }
    fn historical(
        selection: &BodySelection,
    ) -> Option<(
        &crate::ids::FeatureInputTopologyId,
        &[crate::ids::HistoricalBodyId],
    )> {
        match selection {
            BodySelection::Historical { state, bodies, .. }
            | BodySelection::HistoricalSet { state, bodies, .. }
            | BodySelection::HistoricalUnorderedSet { state, bodies, .. } => Some((state, bodies)),
            _ => None,
        }
    }
    if let Some((first, second)) = direct(first).zip(direct(second)) {
        return first.iter().any(|body| second.contains(body));
    }
    if let Some(((first_state, first), (second_state, second))) =
        historical(first).zip(historical(second))
    {
        return first_state == second_state && first.iter().any(|body| second.contains(body));
    }
    match (first, second) {
        (
            BodySelection::Generated { bodies: first, .. },
            BodySelection::Generated { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        (
            BodySelection::Local { bodies: first, .. },
            BodySelection::Local { bodies: second, .. },
        ) => first.iter().any(|body| second.contains(body)),
        _ => false,
    }
}

fn feature_geometry_error(findings: &mut Vec<Finding>, feature: &Feature, message: &str) {
    geometry_error(findings, &feature.id.0, message);
}

fn check_plane_feature_reference(
    findings: &mut Vec<Finding>,
    feature: &Feature,
    reference: &crate::features::FeatureId,
    feature_records: &HashMap<&str, &Feature>,
    reference_kind: &str,
) {
    match feature_records.get(reference.as_str()) {
        None => ref_error(findings, &feature.id.0, reference_kind, reference.as_str()),
        Some(record)
            if !matches!(
                &record.definition,
                crate::features::FeatureDefinition::DatumPrincipalPlane { .. }
                    | crate::features::FeatureDefinition::DatumPlane { .. }
                    | crate::features::FeatureDefinition::DatumPlaneUnresolved
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
                reference.0
            ),
            entity: Some(feature.id.0.clone()),
        }),
        Some(_) if !feature.dependencies.contains(reference) => findings.push(Finding {
            check: Check::ReferentialIntegrity,
            severity: Severity::Error,
            message: format!(
                "feature omits {reference_kind} dependency `{}`",
                reference.0
            ),
            entity: Some(feature.id.0.clone()),
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
        .map(|sketch| sketch.id.0.as_str())
        .collect::<HashSet<_>>();
    let sketch_entity_owners = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| (entity.id().0.as_str(), entity.sketch.0.as_str()))
        .collect::<HashMap<_, _>>();
    let spatial_sketch_entity_owners = ir
        .model
        .spatial_sketch_entities
        .iter()
        .map(|entity| (entity.id().0.as_str(), entity.sketch.0.as_str()))
        .collect::<HashMap<_, _>>();
    let mut owners = HashMap::new();
    for feature in &ir.model.features {
        let sketch = match &feature.definition {
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } => sketch.0.as_str(),
            FeatureDefinition::SpatialSketch {
                sketch: Some(sketch),
            } => sketch.0.as_str(),
            _ => continue,
        };
        if owners
            .insert(sketch, (feature.id.0.as_str(), feature.ordinal))
            .is_some()
        {
            findings.push(Finding {
                check: Check::ReferentialIntegrity,
                severity: Severity::Error,
                message: format!("sketch `{sketch}` has multiple owning features"),
                entity: Some(feature.id.0.clone()),
            });
        }
    }

    for feature in &ir.model.features {
        let FeatureDefinition::DatumPoint {
            construction: Some(construction),
            ..
        } = &feature.definition
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
                    && sketches.contains(sketch.0.as_str())
                    && sketch_entity_owners
                        .get(point.0.as_str())
                        .is_some_and(|owner| *owner == sketch.0.as_str())
                    && ir.model.sketch_entities.iter().any(|entity| {
                        entity.id() == point
                            && entity.sketch == *sketch
                            && matches!(
                                &entity.geometry,
                                crate::sketches::SketchGeometry::Point { .. }
                            )
                    })
            }
            SketchPointSelection::Spatial {
                sketch,
                point,
                native,
            } => {
                !native.trim().is_empty()
                    && spatial_sketches.contains(sketch.0.as_str())
                    && spatial_sketch_entity_owners
                        .get(point.0.as_str())
                        .is_some_and(|owner| *owner == sketch.0.as_str())
                    && ir.model.spatial_sketch_entities.iter().any(|entity| {
                        entity.id() == point
                            && entity.sketch == *sketch
                            && matches!(
                                &entity.geometry,
                                crate::sketches::SpatialSketchGeometry::Point { .. }
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
            SketchPointSelection::Planar { sketch, .. } => Some(sketch.0.as_str()),
            SketchPointSelection::Spatial { sketch, .. } => Some(sketch.0.as_str()),
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
                        entity: Some(feature.id.0.clone()),
                    });
                }
            }
        }
    }

    for feature in &ir.model.features {
        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        let definition = match &feature.definition {
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
                profiles.extend(&construction.profile);
            }
            FeatureDefinition::Revolve { construction, .. } => {
                profiles.extend(&construction.profile);
                paths.extend(&construction.axis_reference);
            }
            FeatureDefinition::Sweep {
                section,
                sections,
                path,
                guide_rail,
                ..
            } => {
                profiles.extend(section.referenced_profile());
                profiles.extend(
                    sections
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
                sections,
                guides,
                centerline,
                ..
            } => {
                profiles.extend(sections.iter().filter_map(|section| match section {
                    crate::features::LoftSection::Profile(profile) => Some(profile),
                    crate::features::LoftSection::Point(_) => None,
                }));
                paths.extend(guides);
                paths.extend(centerline);
            }
            FeatureDefinition::Pattern { pattern, .. } => {
                collect_pattern_paths(pattern, &mut paths);
            }
            _ => {}
        }
        for profile in profiles {
            if let ProfileRef::SpatialSketchProfiles { sketch, .. }
            | ProfileRef::SpatialSketchSelection { sketch, .. } = profile
            {
                if !matches!(
                    feature.definition,
                    FeatureDefinition::Extrude { .. } | FeatureDefinition::Loft { .. }
                ) {
                    feature_geometry_error(
                        findings,
                        feature,
                        "spatial sketch profiles are only supported by extrude and loft features",
                    );
                }
                if !spatial_sketches.contains(sketch.0.as_str()) {
                    ref_error(findings, &feature.id.0, "spatial sketch profile", &sketch.0);
                } else if let Some((owner, ordinal)) = owners.get(sketch.0.as_str()) {
                    if *ordinal >= feature.ordinal {
                        findings.push(Finding {
                            check: Check::ReferentialIntegrity,
                            severity: Severity::Error,
                            message: format!(
                                "spatial sketch owner `{owner}` does not precede its profile consumer"
                            ),
                            entity: Some(feature.id.0.clone()),
                        });
                    }
                }
                match profile {
                    ProfileRef::SpatialSketchProfiles { profiles, .. } => {
                        let profile_count = ir
                            .model
                            .spatial_sketches
                            .iter()
                            .find(|candidate| candidate.id == *sketch)
                            .map_or(0, |sketch| sketch.profiles.len());
                        let unique = profiles.iter().copied().collect::<HashSet<_>>();
                        if profiles.is_empty()
                            || unique.len() != profiles.len()
                            || profiles
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
                    ProfileRef::SpatialSketchSelection { selections, .. }
                        if selections.is_empty()
                            || selections.iter().any(String::is_empty)
                            || selections.iter().collect::<HashSet<_>>().len()
                                != selections.len() =>
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "native spatial sketch profile selections are empty or repeated",
                        );
                    }
                    ProfileRef::SpatialSketchSelection { .. } => {}
                    _ => unreachable!(),
                }
                continue;
            }
            let sketch = match profile {
                ProfileRef::Sketch(sketch)
                | ProfileRef::SketchProfiles { sketch, .. }
                | ProfileRef::SketchRegions { sketch, .. }
                | ProfileRef::SketchEntities { sketch, .. }
                | ProfileRef::SketchSelection { sketch, .. } => sketch,
                ProfileRef::Native(_)
                | ProfileRef::Unresolved(_)
                | ProfileRef::Feature(_)
                | ProfileRef::Generated { .. }
                | ProfileRef::SpatialSketchProfiles { .. }
                | ProfileRef::SpatialSketchSelection { .. }
                | ProfileRef::HistoricalFaces { .. }
                | ProfileRef::Faces(_) => continue,
            };
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
            match profile {
                ProfileRef::SketchProfiles { profiles, .. } => {
                    let sketch_profile_count = ir
                        .model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .map_or(0, |sketch| sketch.profiles.len());
                    let unique = profiles.iter().copied().collect::<HashSet<_>>();
                    if profiles.is_empty()
                        || unique.len() != profiles.len()
                        || profiles
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
                ProfileRef::SketchRegions { regions, .. } => {
                    let selected_sketch = ir
                        .model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch);
                    let sketch_profile_count =
                        selected_sketch.map_or(0, |sketch| sketch.profiles.len());
                    let duplicate_regions = regions.iter().enumerate().any(|(index, region)| {
                        regions.iter().skip(index + 1).any(|other| other == region)
                    });
                    let invalid = regions.is_empty()
                        || duplicate_regions
                        || regions.iter().any(|region| match region {
                            crate::features::SketchProfileRegion::Loops { outer, holes } => {
                                let unique_holes = holes.iter().copied().collect::<HashSet<_>>();
                                *outer as usize >= sketch_profile_count
                                    || unique_holes.len() != holes.len()
                                    || unique_holes.contains(outer)
                                    || unique_holes
                                        .iter()
                                        .any(|index| *index as usize >= sketch_profile_count)
                            }
                            crate::features::SketchProfileRegion::Trimmed {
                                outer_boundary,
                                hole_boundaries,
                            } => {
                                let valid_ring =
                                    |ring: &[crate::features::SketchProfileBoundaryUse]| {
                                        !ring.is_empty()
                                            && ring.iter().all(|use_| {
                                                use_.parameter_range[0].is_finite()
                                                    && use_.parameter_range[1].is_finite()
                                                    && use_.parameter_range[0]
                                                        != use_.parameter_range[1]
                                                    && ir.model.sketch_entities.iter().any(
                                                        |entity| {
                                                            entity.id() == &use_.entity
                                                                && entity.sketch == *sketch
                                                        },
                                                    )
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
                ProfileRef::SketchEntities { entities, .. } => {
                    let unique = entities.iter().collect::<HashSet<_>>();
                    if entities.is_empty()
                        || unique.len() != entities.len()
                        || entities.iter().any(|entity| {
                            sketch_entity_owners
                                .get(entity.0.as_str())
                                .is_none_or(|owner| *owner != sketch.0.as_str())
                        })
                    {
                        feature_geometry_error(
                            findings,
                            feature,
                            "sketch profile entities are empty, repeated, missing, or owned by another sketch",
                        );
                    }
                }
                ProfileRef::SketchSelection { selections, .. }
                    if selections.is_empty()
                        || selections.iter().any(String::is_empty)
                        || selections.iter().collect::<HashSet<_>>().len() != selections.len() =>
                {
                    feature_geometry_error(
                        findings,
                        feature,
                        "native sketch profile selections are empty or repeated",
                    );
                }
                ProfileRef::Native(_)
                | ProfileRef::Unresolved(_)
                | ProfileRef::Feature(_)
                | ProfileRef::Generated { .. }
                | ProfileRef::Sketch(_)
                | ProfileRef::SketchSelection { .. }
                | ProfileRef::SpatialSketchProfiles { .. }
                | ProfileRef::SpatialSketchSelection { .. }
                | ProfileRef::HistoricalFaces { .. }
                | ProfileRef::Faces(_) => {}
            }
        }
        for path in paths {
            if let PathRef::SketchCurves { sketch, curves } = path {
                let invalid = curves.is_empty()
                    || curves.iter().collect::<HashSet<_>>().len() != curves.len()
                    || curves.iter().any(|curve| {
                        sketch_entity_owners
                            .get(curve.0.as_str())
                            .is_none_or(|owner| *owner != sketch.0.as_str())
                    });
                if invalid {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sketch path curves are empty, repeated, or owned by another sketch",
                    );
                }
            }
            let (sketch, known_sketches, description, selections) = match path {
                PathRef::Sketch(sketch) => (sketch.0.as_str(), sketches, "sketch path", None),
                PathRef::SketchCurves { sketch, .. } => {
                    (sketch.0.as_str(), sketches, "sketch curve path", None)
                }
                PathRef::SpatialSketchCurves { sketch, curves } => {
                    let invalid = curves.is_empty()
                        || curves.iter().collect::<HashSet<_>>().len() != curves.len()
                        || curves.iter().any(|curve| {
                            spatial_sketch_entity_owners
                                .get(curve.0.as_str())
                                .is_none_or(|owner| *owner != sketch.0.as_str())
                        });
                    if invalid {
                        feature_geometry_error(
                            findings,
                            feature,
                            "spatial sketch path curves are empty, repeated, missing, or owned by another sketch",
                        );
                    }
                    (
                        sketch.0.as_str(),
                        &spatial_sketches,
                        "spatial sketch curve path",
                        None,
                    )
                }
                PathRef::SpatialSketchSelection { sketch, selections } => (
                    sketch.0.as_str(),
                    &spatial_sketches,
                    "spatial sketch path",
                    Some(selections),
                ),
                _ => continue,
            };
            if !known_sketches.contains(sketch) {
                ref_error(findings, &feature.id.0, description, sketch);
            } else if let Some((owner, ordinal)) = owners.get(sketch) {
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
            if selections.is_some_and(|selections| {
                selections.is_empty()
                    || selections.iter().any(String::is_empty)
                    || selections.iter().collect::<HashSet<_>>().len() != selections.len()
            }) {
                feature_geometry_error(
                    findings,
                    feature,
                    "native spatial sketch path selections are empty or repeated",
                );
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

pub(super) fn check_loops(ir: &CadIr, ids: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    let by_id: HashMap<&str, &Coedge> = ir
        .model
        .coedges
        .iter()
        .map(|c| (c.id.0.as_str(), c))
        .collect();

    for face in &ir.model.faces {
        let outer_count = face
            .loops
            .iter()
            .filter_map(|id| ids.loops(&id.0))
            .filter(|loop_| loop_.boundary_role == crate::topology::LoopBoundaryRole::Outer)
            .count();
        if outer_count > 1 {
            findings.push(Finding {
                check: Check::LoopClosure,
                severity: Severity::Error,
                message: "face has more than one explicit outer loop".into(),
                entity: Some(face.id.0.clone()),
            });
        }
    }

    for lp in &ir.model.loops {
        let crate::topology::LoopBoundary::Ring { coedges, .. } = &lp.boundary else {
            continue;
        };
        // Walk the `next` chain from the first listed coedge and confirm it is a
        // simple cycle whose members are exactly the loop's coedge set.
        let expected: HashSet<&str> = coedges.iter().map(|c| c.0.as_str()).collect();
        let Some(start) = coedges.first().map(|coedge| coedge.0.as_str()) else {
            continue;
        };
        let mut visited: HashSet<&str> = HashSet::new();
        let mut cur = start;
        let mut broke = false;
        for _ in 0..coedges.len() {
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
                    coedges.len()
                ),
                entity: Some(lp.id.0.clone()),
            });
        }
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
        .map(|c| (c.id.0.as_str(), c))
        .collect();
    let mut statuses = HashMap::<&str, RadialStatus>::new();
    for coedge in &ir.model.coedges {
        let start = coedge.id.0.as_str();
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
            let Some(next) = by_id.get(current_coedge.radial_next.0.as_str()) else {
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
            current = next.id.0.as_str();
        }
    }
    for coedge in &ir.model.coedges {
        match statuses[coedge.id.0.as_str()] {
            RadialStatus::CrossesEdge => {
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring crosses edges".into(),
                    entity: Some(coedge.id.0.clone()),
                });
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring does not close".into(),
                    entity: Some(coedge.id.0.clone()),
                });
            }
            RadialStatus::DoesNotClose => {
                findings.push(Finding {
                    check: Check::CoedgePairing,
                    severity: Severity::Error,
                    message: "radial ring does not close".into(),
                    entity: Some(coedge.id.0.clone()),
                });
            }
            RadialStatus::Closed(2) => {
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
            RadialStatus::Closed(_) => {}
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
    let loop_vertices = ir
        .model
        .loops
        .iter()
        .flat_map(crate::topology::Loop::vertices)
        .map(|vertex| vertex.0.as_str())
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
        let owner_count = free_owners.get(vertex.id.0.as_str()).copied().unwrap_or(0);
        if owner_count > 1
            || (!edge_vertices.contains(vertex.id.0.as_str())
                && !loop_vertices.contains(vertex.id.0.as_str())
                && owner_count != 1)
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
        if body
            .transform
            .is_some_and(|transform| !transform.is_finite())
        {
            findings.push(Finding {
                check: Check::Bounds,
                severity: Severity::Error,
                message: "body transform contains a non-finite coefficient".into(),
                entity: Some(body.id.0.clone()),
            });
        }
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

pub(super) fn check_shell_connectivity(ir: &CadIr, findings: &mut Vec<Finding>) {
    let faces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.0.as_str(), face))
        .collect::<HashMap<_, _>>();
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.0.as_str(), loop_.face.0.as_str()))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.0.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let mut faces_by_edge = HashMap::<&str, HashSet<&str>>::new();
    let mut faces_by_vertex = HashMap::<&str, HashSet<&str>>::new();
    for coedge in &ir.model.coedges {
        let Some(face) = loop_faces.get(coedge.owner_loop.0.as_str()) else {
            continue;
        };
        faces_by_edge
            .entry(coedge.edge.0.as_str())
            .or_default()
            .insert(*face);
        if let Some(edge) = edges.get(coedge.edge.0.as_str()) {
            faces_by_vertex
                .entry(edge.start.0.as_str())
                .or_default()
                .insert(*face);
            faces_by_vertex
                .entry(edge.end.0.as_str())
                .or_default()
                .insert(*face);
        }
    }
    for loop_ in &ir.model.loops {
        let Some(face) = loop_faces.get(loop_.id.0.as_str()) else {
            continue;
        };
        match &loop_.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, .. } => {
                faces_by_vertex
                    .entry(vertex.0.as_str())
                    .or_default()
                    .insert(*face);
            }
            crate::topology::LoopBoundary::Ring { vertex_uses, .. } => {
                for vertex_use in vertex_uses {
                    faces_by_vertex
                        .entry(vertex_use.vertex.0.as_str())
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
        if shell.faces.len() < 2
            || shell.faces.iter().any(|face| {
                faces
                    .get(face.0.as_str())
                    .is_none_or(|face| face.loops.is_empty())
            })
        {
            continue;
        }
        let owned = shell
            .faces
            .iter()
            .map(|face| face.0.as_str())
            .collect::<HashSet<_>>();
        let mut reached = HashSet::from([shell.faces[0].0.as_str()]);
        let mut pending = vec![shell.faces[0].0.as_str()];
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
                entity: Some(shell.id.0.clone()),
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
