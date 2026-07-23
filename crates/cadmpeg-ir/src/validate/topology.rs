// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for topology.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::features::{
    ChamferSpec, ExtrudeStart, FaceMotion, FeatureSourceContent, FlexMode, HoleKind, Length,
    PatternKind, PatternSeed, PatternStageCombination, PrimitiveSolid, RadiusSpec,
};
use crate::math::Point3;

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
        PatternKind::Mirror { .. } => Some(2),
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
use crate::sketches::{SketchConstraintDefinition as Definition, SketchLocus};

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

pub(super) fn check_references(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    for b in &ir.model.bodies {
        for l in &b.regions {
            if !index.regions.contains_key(&l.0) {
                ref_error(findings, &b.id.0, "region", &l.0);
            }
        }
    }
    for l in &ir.model.regions {
        if !index.bodies.contains_key(&l.body.0) {
            ref_error(findings, &l.id.0, "body", &l.body.0);
        }
        for s in &l.shells {
            if !index.shells.contains_key(&s.0) {
                ref_error(findings, &l.id.0, "shell", &s.0);
            }
        }
    }
    for s in &ir.model.shells {
        if !index.regions.contains_key(&s.region.0) {
            ref_error(findings, &s.id.0, "region", &s.region.0);
        }
        for f in &s.faces {
            if !index.faces.contains_key(&f.0) {
                ref_error(findings, &s.id.0, "face", &f.0);
            }
        }
        for e in &s.wire_edges {
            if !index.edges.contains_key(&e.0) {
                ref_error(findings, &s.id.0, "wire edge", &e.0);
            }
        }
        for v in &s.free_vertices {
            if !index.vertices.contains_key(&v.0) {
                ref_error(findings, &s.id.0, "free vertex", &v.0);
            }
        }
    }
    for f in &ir.model.faces {
        if !index.shells.contains_key(&f.shell.0) {
            ref_error(findings, &f.id.0, "shell", &f.shell.0);
        }
        if !index.surfaces.contains_key(&f.surface.0) {
            ref_error(findings, &f.id.0, "surface", &f.surface.0);
        }
        for lp in &f.loops {
            if !index.loops.contains_key(&lp.0) {
                ref_error(findings, &f.id.0, "loop", &lp.0);
            }
        }
    }
    for lp in &ir.model.loops {
        if !index.faces.contains_key(&lp.face.0) {
            ref_error(findings, &lp.id.0, "face", &lp.face.0);
        }
        for ce in &lp.coedges {
            if !index.coedges.contains_key(&ce.0) {
                ref_error(findings, &lp.id.0, "coedge", &ce.0);
            }
        }
        for use_ in &lp.vertex_uses {
            if !index.vertices.contains_key(&use_.vertex.0) {
                ref_error(findings, &lp.id.0, "vertex", &use_.vertex.0);
            }
            if let Some(after) = &use_.after {
                if !index.coedges.contains_key(&after.0) {
                    ref_error(findings, &lp.id.0, "coedge(vertex-use after)", &after.0);
                }
            }
            for pcurve in &use_.pcurves {
                if !index.pcurves.contains_key(&pcurve.pcurve.0) {
                    ref_error(findings, &lp.id.0, "pcurve(vertex use)", &pcurve.pcurve.0);
                }
            }
        }
    }
    for ce in &ir.model.coedges {
        if !index.loops.contains_key(&ce.owner_loop.0) {
            ref_error(findings, &ce.id.0, "loop", &ce.owner_loop.0);
        }
        if !index.edges.contains_key(&ce.edge.0) {
            ref_error(findings, &ce.id.0, "edge", &ce.edge.0);
        }
        if !index.coedges.contains_key(&ce.next.0) {
            ref_error(findings, &ce.id.0, "coedge(next)", &ce.next.0);
        }
        if !index.coedges.contains_key(&ce.previous.0) {
            ref_error(findings, &ce.id.0, "coedge(previous)", &ce.previous.0);
        }
        if !index.coedges.contains_key(&ce.radial_next.0) {
            ref_error(findings, &ce.id.0, "coedge(radial_next)", &ce.radial_next.0);
        }
        for use_ in &ce.pcurves {
            if !index.pcurves.contains_key(&use_.pcurve.0) {
                ref_error(findings, &ce.id.0, "pcurve", &use_.pcurve.0);
            }
        }
        if let Some(curve) = &ce.use_curve {
            if !index.curves.contains_key(&curve.0) {
                ref_error(findings, &ce.id.0, "coedge use curve", &curve.0);
            }
        }
    }
    for e in &ir.model.edges {
        if let Some(c) = &e.curve {
            if !index.curves.contains_key(&c.0) {
                ref_error(findings, &e.id.0, "curve", &c.0);
            }
        }
        if !index.vertices.contains_key(&e.start.0) {
            ref_error(findings, &e.id.0, "vertex(start)", &e.start.0);
        }
        if !index.vertices.contains_key(&e.end.0) {
            ref_error(findings, &e.id.0, "vertex(end)", &e.end.0);
        }
    }
    for v in &ir.model.vertices {
        if !index.points.contains_key(&v.point.0) {
            ref_error(findings, &v.id.0, "point", &v.point.0);
        }
    }
    for binding in &ir.model.appearance_bindings {
        use crate::appearance::AppearanceTarget;
        let owner = format!("appearance-binding:{}", binding.appearance.0);
        if !index.appearances.contains_key(&binding.appearance.0) {
            ref_error(findings, &owner, "appearance", &binding.appearance.0);
        }
        match &binding.target {
            AppearanceTarget::Body(body) if !index.bodies.contains_key(&body.0) => {
                ref_error(findings, &owner, "body", &body.0);
            }
            AppearanceTarget::Face(face) if !index.faces.contains_key(&face.0) => {
                ref_error(findings, &owner, "face", &face.0);
            }
            AppearanceTarget::Edge(edge) if !index.edges.contains_key(&edge.0) => {
                ref_error(findings, &owner, "edge", &edge.0);
            }
            AppearanceTarget::Vertex(vertex) if !index.vertices.contains_key(&vertex.0) => {
                ref_error(findings, &owner, "vertex", &vertex.0);
            }
            AppearanceTarget::Surface(surface) if !index.surfaces.contains_key(&surface.0) => {
                ref_error(findings, &owner, "surface", &surface.0);
            }
            AppearanceTarget::Curve(curve) if !index.curves.contains_key(&curve.0) => {
                ref_error(findings, &owner, "curve", &curve.0);
            }
            AppearanceTarget::Point(point) if !index.points.contains_key(&point.0) => {
                ref_error(findings, &owner, "point", &point.0);
            }
            AppearanceTarget::Tessellation(tessellation)
                if !index.tessellations.contains_key(tessellation) =>
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
            AttributeTarget::Body(id) if !index.bodies.contains_key(&id.0) => {
                ref_error(findings, owner, "body", &id.0);
            }
            AttributeTarget::Face(id) if !index.faces.contains_key(&id.0) => {
                ref_error(findings, owner, "face", &id.0);
            }
            AttributeTarget::Coedge(id) if !index.coedges.contains_key(&id.0) => {
                ref_error(findings, owner, "coedge", &id.0);
            }
            AttributeTarget::Edge(id) if !index.edges.contains_key(&id.0) => {
                ref_error(findings, owner, "edge", &id.0);
            }
            AttributeTarget::Vertex(id) if !index.vertices.contains_key(&id.0) => {
                ref_error(findings, owner, "vertex", &id.0);
            }
            _ => {}
        }
    }
    for s in &ir.model.surfaces {
        match &s.geometry {
            SurfaceGeometry::Procedural { construction } => {
                if !index.procedural_surfaces.contains_key(&construction.0) {
                    ref_error(
                        findings,
                        &s.id.0,
                        "procedural surface construction",
                        &construction.0,
                    );
                } else if !ir
                    .model
                    .procedural_surfaces
                    .iter()
                    .any(|procedural| procedural.id == *construction && procedural.surface == s.id)
                {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "procedural surface construction `{construction}` does not produce surface `{}`",
                            s.id
                        ),
                        entity: Some(s.id.0.clone()),
                    });
                } else if ir.model.procedural_surfaces.iter().any(|procedural| {
                    procedural.id == *construction && procedural.cache_fit_tolerance.is_some()
                }) {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: "construction-backed surface cannot carry a cache-fit tolerance"
                            .into(),
                        entity: Some(s.id.0.clone()),
                    });
                }
            }
            SurfaceGeometry::Unknown { record: Some(u) } if !index.unknown_ids.contains(&u.0) => {
                ref_error(findings, &s.id.0, "unknown record", &u.0);
            }
            _ => {}
        }
    }
    for curve in &ir.model.curves {
        match &curve.geometry {
            CurveGeometry::Procedural { construction } => {
                if !index.procedural_curves.contains_key(&construction.0) {
                    ref_error(
                        findings,
                        &curve.id.0,
                        "procedural curve construction",
                        &construction.0,
                    );
                } else if !ir.model.procedural_curves.iter().any(|procedural| {
                    procedural.id == *construction && procedural.curve == curve.id
                }) {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "procedural curve construction `{construction}` does not produce curve `{}`",
                            curve.id
                        ),
                        entity: Some(curve.id.0.clone()),
                    });
                } else if ir.model.procedural_curves.iter().any(|procedural| {
                    procedural.id == *construction && procedural.cache_fit_tolerance.is_some()
                }) {
                    findings.push(Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: "construction-backed curve cannot carry a cache-fit tolerance"
                            .into(),
                        entity: Some(curve.id.0.clone()),
                    });
                }
            }
            CurveGeometry::Unknown {
                record: Some(unknown),
            } => {
                if !index.unknown_ids.contains(&unknown.0) {
                    ref_error(findings, &curve.id.0, "unknown record", &unknown.0);
                }
            }
            CurveGeometry::Composite { segments, .. } => {
                for segment in segments {
                    if !index.curves.contains_key(&segment.curve.0) {
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
        if !index.surfaces.contains_key(&procedural.surface.0) {
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
                    if !index.surfaces.contains_key(&component.0) {
                        ref_error(findings, &procedural.id.0, "surface", &component.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::SubSurface { support, .. } => {
                if !index.surfaces.contains_key(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                if !index.surfaces.contains_key(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
                if !index.curves.contains_key(&reference.0) {
                    ref_error(findings, &procedural.id.0, "curve", &reference.0);
                }
            }
            ProceduralSurfaceDefinition::Loft { sections, .. } => {
                for entry in sections.iter().flat_map(|section| &section.entries) {
                    for curve in entry
                        .path
                        .curve
                        .iter()
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.curve))
                    {
                        if !index.curves.contains_key(&curve.0) {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = &member.data.surface {
                            if !index.surfaces.contains_key(&surface.0) {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if !index.curves.contains_key(&curve.0) {
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
                        if let Some(surface) = &member.data.surface {
                            if !index.surfaces.contains_key(&surface.0) {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if !index.curves.contains_key(&curve.0) {
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
                        if let Some(surface) = &member.data.surface {
                            if !index.surfaces.contains_key(&surface.0) {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Skin { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    index: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !index.curves.contains_key(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, index, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                    if !index.curves.contains_key(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                };
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        check_curve(path, findings);
                        for profile in profiles {
                            check_curve(&profile.curve, findings);
                            if let Some(surface) = &profile.data.surface {
                                if !index.surfaces.contains_key(&surface.0) {
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
                    check_law_curves(variable, index, procedural, findings);
                }
            }
            ProceduralSurfaceDefinition::Law { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    index: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !index.curves.contains_key(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, index, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                for formula in
                    std::iter::once(&construction.primary).chain(&construction.additional)
                {
                    for variable in &formula.variables {
                        check_law_curves(variable, index, procedural, findings);
                    }
                }
            }
            ProceduralSurfaceDefinition::Net { construction } => {
                fn check_law_curves(
                    expression: &crate::geometry::LawExpression,
                    index: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !index.curves.contains_key(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, index, procedural, findings);
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
                        .chain(entry.path.auxiliaries.iter())
                        .chain(entry.profile.iter().map(|member| &member.curve))
                    {
                        if !index.curves.contains_key(&curve.0) {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                    for member in &entry.profile {
                        if let Some(surface) = &member.data.surface {
                            if !index.surfaces.contains_key(&surface.0) {
                                ref_error(findings, &procedural.id.0, "surface", &surface.0);
                            }
                        }
                    }
                }
                for formula in construction.formulas.iter() {
                    for variable in &formula.variables {
                        check_law_curves(variable, index, procedural, findings);
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend { construction } => {
                for surface in [&construction.first.surface, &construction.second.surface]
                    .into_iter()
                    .chain(std::iter::once(&construction.second_exact_surface))
                {
                    if !index.surfaces.contains_key(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
                if let crate::geometry::G2BlendFirstShape::Full {
                    surface: Some(surface),
                    ..
                } = &construction.first_shape
                {
                    if !index.surfaces.contains_key(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
                for curve in [
                    &construction.first.curve,
                    &construction.second.curve,
                    &construction.center_curve,
                ] {
                    if !index.curves.contains_key(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::VariableBlend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if !index.curves.contains_key(&curve.0) {
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
                    if !index.curves.contains_key(&curve.0) {
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
                    if !index.curves.contains_key(&member.curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &member.curve.0);
                    }
                    if let Some(surface) = &member.data.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
                for curve in std::iter::once(&construction.base_path)
                    .chain(construction.entries.iter().map(|entry| &entry.path))
                    .flat_map(|path| path.curve.iter().chain(path.auxiliaries.iter()))
                    .chain(construction.direction_curve.iter())
                    .chain(construction.trailing_curve.iter())
                {
                    if !index.curves.contains_key(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                    if let Some(curve) = &side.curve {
                        if !index.curves.contains_key(&curve.0) {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    }
                }
                if !index.curves.contains_key(&construction.center.0) {
                    ref_error(findings, &procedural.id.0, "curve", &construction.center.0);
                }
            }
            ProceduralSurfaceDefinition::VertexBlend { construction } => {
                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            if !index.curves.contains_key(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            if !index.surfaces.contains_key(&surface.0) {
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
                if !index.curves.contains_key(&directrix.0) {
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
                    index: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralSurface,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !index.curves.contains_key(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check_law_curves(operand, index, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                for curve in [profile, spine] {
                    if !index.curves.contains_key(&curve.0) {
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
                            if !index.curves.contains_key(&guide_curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &guide_curve.0);
                            }
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            if !index.surfaces.contains_key(&support_surface.0) {
                                ref_error(
                                    findings,
                                    &procedural.id.0,
                                    "surface",
                                    &support_surface.0,
                                );
                            }
                            if let Some(curve) = auxiliary_curve {
                                if !index.curves.contains_key(&curve.0) {
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
                            check_law_curves(first_law, index, procedural, findings);
                            check_law_curves(second_law, index, procedural, findings);
                            vec![formula]
                        }
                    };
                    for formula in formulas {
                        for variable in &formula.variables {
                            check_law_curves(variable, index, procedural, findings);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset { support, .. } => {
                if !index.surfaces.contains_key(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Subset { support, .. }
            | ProceduralSurfaceDefinition::ParallelOffset { support, .. } => {
                if !index.surfaces.contains_key(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                for curve in [first, second] {
                    if !index.curves.contains_key(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                for curve in [first, second] {
                    if !index.curves.contains_key(&curve.0) {
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
                    if !index.surfaces.contains_key(&support.surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &support.surface.0);
                    }
                }
                if let Some(spine) = spine {
                    if !index.curves.contains_key(&spine.0) {
                        ref_error(findings, &procedural.id.0, "curve", &spine.0);
                    }
                }
                if let Some(native) = native {
                    let check_curve = |curve: &crate::ids::CurveId, findings: &mut Vec<Finding>| {
                        if !index.curves.contains_key(&curve.0) {
                            ref_error(findings, &procedural.id.0, "curve", &curve.0);
                        }
                    };
                    let check_surface =
                        |surface: &crate::ids::SurfaceId, findings: &mut Vec<Finding>| {
                            if !index.surfaces.contains_key(&surface.0) {
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
                if !index.unknown_ids.contains(&record.0) {
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
                ..
            } => {
                if !index.surfaces.contains_key(&support.0) {
                    ref_error(findings, &procedural.id.0, "surface", &support.0);
                }
                for boundary in boundaries {
                    if !index.curves.contains_key(&boundary.0) {
                        ref_error(findings, &procedural.id.0, "curve", &boundary.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Deformable { construction } => {
                if !index.surfaces.contains_key(&construction.support.0) {
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
                    if !index.surfaces.contains_key(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                    if !index.curves.contains_key(&curve.0) {
                        ref_error(findings, &procedural.id.0, "curve", &curve.0);
                    }
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        if !index.curves.contains_key(&procedural.curve.0) {
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
                    index: &ModelIndex<'_>,
                    procedural: &crate::geometry::ProceduralCurve,
                    findings: &mut Vec<Finding>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            if !index.curves.contains_key(&curve.0) {
                                ref_error(findings, &procedural.id.0, "curve", &curve.0);
                            }
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                check(operand, index, procedural, findings);
                            }
                        }
                        _ => {}
                    }
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    for variable in &formula.variables {
                        check(variable, index, procedural, findings);
                    }
                }
            }
            ProceduralCurveDefinition::Compound { components, .. } => {
                for component in components {
                    if !index.curves.contains_key(&component.0) {
                        ref_error(findings, &procedural.id.0, "curve", &component.0);
                    }
                }
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides.iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
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
                if !index.surfaces.contains_key(&cast_surface.0) {
                    ref_error(findings, &procedural.id.0, "surface", &cast_surface.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                if !index.curves.contains_key(&base.0) {
                    ref_error(findings, &procedural.id.0, "curve", &base.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Spring { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Deformable { bend, data, .. } => {
                if !index.curves.contains_key(&bend.0) {
                    ref_error(findings, &procedural.id.0, "curve", &bend.0);
                }
                if let crate::geometry::DeformableCurveData::Surface { surface } = data {
                    if !index.surfaces.contains_key(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                if !index.curves.contains_key(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
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
                if !index.curves.contains_key(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
                if let Some(support) = support {
                    if !index.surfaces.contains_key(&support.0) {
                        ref_error(findings, &procedural.id.0, "surface", &support.0);
                    }
                }
                if let Some(crate::geometry::CurveOffsetDistanceLaw::Coordinate {
                    function, ..
                }) = distance_law
                {
                    if !index.curves.contains_key(&function.0) {
                        ref_error(findings, &procedural.id.0, "curve", &function.0);
                    }
                }
            }
            ProceduralCurveDefinition::SpatialOffset { source, .. } => {
                if !index.curves.contains_key(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        if !index.surfaces.contains_key(&surface.0) {
                            ref_error(findings, &procedural.id.0, "surface", &surface.0);
                        }
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                if !index.curves.contains_key(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::Subset { source, .. } => {
                if !index.curves.contains_key(&source.0) {
                    ref_error(findings, &procedural.id.0, "curve", &source.0);
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    if !index.surfaces.contains_key(&surface.0) {
                        ref_error(findings, &procedural.id.0, "surface", &surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Unknown {
                native_kind: _,
                record: Some(record),
            } => {
                if !index.unknown_ids.contains(&record.0) {
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
            Definition::RectangularPattern { instances, .. } => (
                instances
                    .iter()
                    .flat_map(|instance| instance.entities.iter().cloned())
                    .collect(),
                None,
            ),
            Definition::CircularPattern {
                center, instances, ..
            } => (
                std::iter::once(center.clone())
                    .chain(
                        instances
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
            | Definition::Collinear { first, second } => {
                (vec![first.clone(), second.clone()], None)
            }
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
            Definition::HorizontalPoints { first, second }
            | Definition::VerticalPoints { first, second } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
                None,
            ),
            Definition::Midpoint { point, entity } => {
                (vec![locus_entity(point).clone(), entity.clone()], None)
            }
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
                parameter.as_ref().map(|parameter| parameter.0.as_str()),
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
            Definition::HorizontalLoci { first, second }
            | Definition::VerticalLoci { first, second } => (
                vec![locus_entity(first).clone(), locus_entity(second).clone()],
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
        if let Definition::HorizontalLoci { first, second }
        | Definition::VerticalLoci { first, second } = &constraint.definition
        {
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
        if let Definition::RectangularPattern { directions, .. } = &constraint.definition {
            for parameter in directions.iter().flat_map(|direction| {
                [
                    direction.span_parameter.as_ref(),
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
        if let Definition::CircularPattern {
            angle_parameter,
            count_parameter,
            ..
        } = &constraint.definition
        {
            for parameter in [angle_parameter.as_ref(), count_parameter.as_ref()]
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
    check_feature_references(ir, index, findings);
}

fn check_feature_references(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    use crate::features::{
        BodySelection, EdgeSelection, ExtrudeExtent, FaceSelection, FeatureDefinition, PathRef,
        ProfileRef, RevolveExtent, ScaleCenter, Termination,
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
            if !index.bodies.contains_key(&body.0) {
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
            if !index.parameters.contains_key(parameter.0.as_str()) {
                ref_error(
                    findings,
                    &configuration.id.0,
                    "configuration parameter override",
                    &parameter.0,
                );
            }
        }
        for feature in &configuration.suppressed_features {
            if !index.features.contains_key(feature.0.as_str()) {
                ref_error(
                    findings,
                    &configuration.id.0,
                    "configuration suppressed feature",
                    &feature.0,
                );
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
            if !index.bodies.contains_key(&body.0) {
                ref_error(findings, &feature.id.0, "output body", &body.0);
            }
        }

        let mut profiles = Vec::new();
        let mut paths = Vec::new();
        let mut extents = Vec::new();
        let mut edge_selections = Vec::new();
        let mut face_selections = Vec::new();
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
            FeatureDefinition::DatumPointUnresolved
            | FeatureDefinition::DatumCoordinateSystemUnresolved
            | FeatureDefinition::LoftUnresolved
            | FeatureDefinition::FreeformSurfaceUnresolved
            | FeatureDefinition::BoundarySurfaceUnresolved => {}
            FeatureDefinition::Block { dimensions, .. } => {
                if dimensions.is_some_and(|values| {
                    values
                        .into_iter()
                        .any(|value| !positive_feature_length(value))
                }) {
                    feature_geometry_error(findings, feature, "block dimensions are invalid");
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
                if !radius_spec_is_valid(radius) {
                    feature_geometry_error(findings, feature, "face blend radius is invalid");
                }
            }
            FeatureDefinition::SewBodies {
                bodies,
                gap_tolerance,
            } => {
                body_selections.push(bodies);
                if gap_tolerance.is_some_and(|value| !positive_feature_length(value)) {
                    feature_geometry_error(findings, feature, "sew tolerance is invalid");
                }
            }
            FeatureDefinition::BaseFeature { bodies } => body_selections.push(bodies),
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
            FeatureDefinition::Form { cages } => {
                if cages.is_empty() {
                    feature_geometry_error(findings, feature, "Form operation has no control cage");
                }
                check_ids(
                    findings,
                    &feature.id.0,
                    "Form control cage",
                    cages.iter().map(|cage| cage.0.as_str()),
                    &index.subds,
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
                profile,
                direction,
                start,
                extent,
                direction_source,
                face_maker,
                ..
            } => {
                profiles.push(profile);
                let sides = match extent {
                    ExtrudeExtent::OneSided { side } | ExtrudeExtent::Symmetric { side } => {
                        vec![side]
                    }
                    ExtrudeExtent::TwoSided { first, second } => vec![first, second],
                };
                for side in &sides {
                    extents.push(&side.termination);
                }
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
            FeatureDefinition::SheetMetalBaseFlange {
                profile, thickness, ..
            } => {
                profiles.push(profile);
                if !positive_feature_length(*thickness) {
                    feature_geometry_error(
                        findings,
                        feature,
                        "sheet-metal base-flange thickness is invalid",
                    );
                }
            }
            FeatureDefinition::Revolve { construction, .. } => {
                profiles.extend(&construction.profile);
                match &construction.extent {
                    Some(
                        RevolveExtent::OneSided { termination }
                        | RevolveExtent::Symmetric { termination },
                    ) => extents.push(termination),
                    Some(RevolveExtent::TwoSided { first, second }) => {
                        extents.extend([first, second]);
                    }
                    None => {}
                }
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
                profile,
                sections,
                path,
                orientation,
                twist,
                scale,
                ..
            } => {
                profiles.extend(profile);
                profiles.extend(sections);
                paths.extend(path);
                if let Some(crate::features::SweepOrientation::Auxiliary { path, .. }) = orientation
                {
                    paths.push(path);
                }
                if twist.is_some_and(|value| !value.0.is_finite())
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
                        crate::features::LoftSection::Profile(profile) => profiles.push(profile),
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
                            &index.vertices,
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
                removed_faces,
                thickness,
                ..
            } => {
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
                if pull_direction.is_some_and(|value| !valid_feature_direction(value))
                    || angle.is_some_and(|value| !value.0.is_finite())
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
            FeatureDefinition::TrimBodies { targets, tools, .. } => {
                body_selections.push(targets);
                body_selections.push(tools);
            }
            FeatureDefinition::DeleteBody { bodies, .. } => {
                body_selections.push(bodies);
            }
            FeatureDefinition::Hole {
                profile,
                profile_filter,
                face,
                kind,
                diameter,
                extent,
                direction,
                position,
                bottom,
                taper_angle,
                specification,
                placements,
                ..
            } => {
                profiles.extend(profile);
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
                    HoleKind::Chamfer { diameter, angle } => {
                        positive_feature_length(*diameter)
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
                        positive_feature_length(*diameter) && positive_feature_length(*depth)
                    }
                    HoleKind::CounterboreDrilled {
                        diameter,
                        depth,
                        drill_point_angle,
                    } => {
                        positive_feature_length(*diameter)
                            && positive_feature_length(*depth)
                            && drill_point_angle.0.is_finite()
                            && drill_point_angle.0 > 0.0
                            && drill_point_angle.0 < std::f64::consts::PI
                    }
                    HoleKind::Countersink { diameter, angle } => {
                        positive_feature_length(*diameter)
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
                        depth,
                        angle,
                    } => {
                        positive_feature_length(*diameter)
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
                    || !kind_valid
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
                            Some(_) => {}
                        },
                        PatternSeed::Faces(selection) => face_selections.push(selection),
                        PatternSeed::Bodies(selection) => body_selections.push(selection),
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
                pitch,
                revolutions,
                radial_growth,
                cone_angle,
                segment_turns,
                ..
            } => {
                let valid = [axis_origin.x, axis_origin.y, axis_origin.z, pitch.0]
                    .into_iter()
                    .all(f64::is_finite)
                    && valid_feature_direction(*axis_direction)
                    && radius.0.is_finite()
                    && radius.0 > 0.0
                    && revolutions.is_finite()
                    && *revolutions > 0.0
                    && radial_growth.is_none_or(|value| value.0.is_finite())
                    && cone_angle.is_none_or(|value| {
                        value.0.is_finite() && value.0.abs() < std::f64::consts::FRAC_PI_2
                    })
                    && segment_turns.is_none_or(|value| value.is_finite() && value > 0.0)
                    && !(radial_growth.is_some() && cone_angle.is_some());
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
                            && (axis.norm() - 1.0).abs() <= 1.0e-9
                            && (radial.norm() - 1.0).abs() <= 1.0e-9
                            && dot.abs() <= 1.0e-9
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
                profiles.push(&construction.profile);
                let valid = [
                    construction.axis_origin.x,
                    construction.axis_origin.y,
                    construction.axis_origin.z,
                    construction.pitch.0,
                    construction.height.0,
                    construction.radial_growth.0,
                    construction.cone_angle.0,
                    construction.tolerance,
                ]
                .into_iter()
                .all(f64::is_finite)
                    && valid_feature_direction(construction.axis_direction)
                    && construction.pitch.0 >= 0.0
                    && construction.turns.is_finite()
                    && construction.turns > 0.0
                    && construction.tolerance > 0.0
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
                        > 1e-9
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
            FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlaneUnresolved
            | FeatureDefinition::DatumPlane { .. }
            | FeatureDefinition::DatumAxis { .. }
            | FeatureDefinition::DatumPoint { .. }
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
                        Some(_) => {}
                    }
                }
                if placement.is_some_and(|placement| {
                    !placement
                        .rows
                        .iter()
                        .flatten()
                        .all(|value| value.is_finite())
                        || placement.rows[3] != [0.0, 0.0, 0.0, 1.0]
                }) {
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
            match profile {
                ProfileRef::Faces(faces) => check_ids(
                    findings,
                    &feature.id.0,
                    "profile face",
                    faces.iter().map(|id| id.0.as_str()),
                    &index.faces,
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
                    &index.edges,
                ),
                PathRef::Curves(curves) => check_ids(
                    findings,
                    &feature.id.0,
                    "path curve",
                    curves.iter().map(|id| id.0.as_str()),
                    &index.curves,
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
        for termination in extents {
            let valid_magnitude = match termination {
                Termination::Blind { length } => length.0.is_finite() && length.0 != 0.0,
                Termination::Angle { angle } => angle.0.is_finite() && angle.0 > 0.0,
                Termination::OffsetFromFace { offset, .. } => {
                    offset.0.is_finite() && offset.0 > 0.0
                }
                Termination::ToFace { offset, .. } => {
                    offset.is_none_or(|offset| offset.0.is_finite())
                }
                Termination::Unresolved
                | Termination::ThroughAll
                | Termination::ThroughNext
                | Termination::ToFirst
                | Termination::ToLast
                | Termination::ToVertex { .. }
                | Termination::ToShape { .. } => true,
            };
            if !valid_magnitude {
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
                    &index.faces,
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
                    &index.faces,
                );
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
                    &index.edges,
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
                    &index.faces,
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
                        &index.bodies,
                    );
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
                BodySelection::Generated { bodies, native } => {
                    if bodies.is_empty()
                        || native.trim().is_empty()
                        || bodies.iter().any(|body| {
                            body.local_id.trim().is_empty()
                                || !feature.dependencies.contains(&body.feature)
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
                BodySelection::Unresolved | BodySelection::Native(_) => {}
            }
        }
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

fn radius_spec_is_valid(radius: &RadiusSpec) -> bool {
    match radius {
        RadiusSpec::Unresolved { .. } => true,
        RadiusSpec::Constant { radius } => positive_feature_length(*radius),
        RadiusSpec::Chordal { chord_length } => positive_feature_length(*chord_length),
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

fn feature_geometry_error(findings: &mut Vec<Finding>, feature: &Feature, message: &str) {
    findings.push(Finding {
        check: Check::GeometricConsistency,
        severity: Severity::Error,
        message: message.into(),
        entity: Some(feature.id.0.clone()),
    });
}

/// Probe an index arena map by id, reporting each unresolved reference in the
/// order `values` yields them. Only the keys are read, so finding order tracks
/// the traversal, never the map's iteration order.
fn check_ids<'a, V>(
    findings: &mut Vec<Finding>,
    owner: &str,
    kind: &str,
    values: impl Iterator<Item = &'a str>,
    valid: &HashMap<String, V>,
) {
    for value in values {
        if !valid.contains_key(value) {
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

    let spatial_sketches = ir
        .model
        .spatial_sketches
        .iter()
        .map(|sketch| sketch.id.0.as_str())
        .collect::<HashSet<_>>();
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
            FeatureDefinition::Sweep { profile, path, .. } => {
                profiles.extend(profile);
                paths.extend(path);
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
                                                            entity.id == use_.entity
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
            let (sketch, known_sketches, description, selections) = match path {
                PathRef::Sketch(sketch) => (sketch.0.as_str(), sketches, "sketch path", None),
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

pub(super) fn check_loops(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
    for face in &ir.model.faces {
        let outer_count = face
            .loops
            .iter()
            .filter_map(|id| ir.model.loops.iter().find(|loop_| loop_.id == *id))
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
        let vertex_only =
            lp.coedges.is_empty() && lp.vertex_uses.len() == 1 && lp.vertex_uses[0].after.is_none();
        let edge_loop = !lp.coedges.is_empty()
            && lp.vertex_uses.iter().all(|use_| {
                use_.after
                    .as_ref()
                    .is_some_and(|after| lp.coedges.contains(after))
            });
        if !vertex_only && !edge_loop {
            findings.push(Finding {
                check: Check::LoopClosure,
                severity: Severity::Error,
                message: "loop must contain a coedge ring with anchored vertex uses or one unanchored vertex use".into(),
                entity: Some(lp.id.0.clone()),
            });
            continue;
        }
        if lp.coedges.is_empty() {
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
            match index.coedges.get(cur) {
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

pub(super) fn check_coedge_pairing(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    for coedge in &ir.model.coedges {
        let mut current = coedge;
        let mut closed = false;
        let mut members = 1usize;
        for _ in 0..=ir.model.coedges.len() {
            let Some(next) = index.coedges.get(current.radial_next.0.as_str()) else {
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
            if let Some(other) = index.coedges.get(coedge.radial_next.0.as_str()) {
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

pub(super) fn check_wire_topology(ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) {
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
        .flat_map(|loop_| loop_.vertex_uses.iter().map(|use_| use_.vertex.0.as_str()))
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
                index
                    .regions
                    .get(region_id.0.as_str())
                    .is_some_and(|region| {
                        region.shells.iter().any(|shell_id| {
                            index
                                .shells
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

pub(super) fn check_shell_connectivity(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let loop_faces = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.0.as_str(), loop_.face.0.as_str()))
        .collect::<HashMap<_, _>>();
    let mut faces_by_edge = HashMap::<&str, HashSet<&str>>::new();
    for coedge in &ir.model.coedges {
        let Some(face) = loop_faces.get(coedge.owner_loop.0.as_str()) else {
            continue;
        };
        faces_by_edge
            .entry(coedge.edge.0.as_str())
            .or_default()
            .insert(*face);
    }
    let mut neighbors = HashMap::<&str, HashSet<&str>>::new();
    for edge_faces in faces_by_edge.values() {
        for &face in edge_faces {
            neighbors
                .entry(face)
                .or_default()
                .extend(edge_faces.iter().copied().filter(|other| *other != face));
        }
    }

    for shell in &ir.model.shells {
        if shell.faces.len() < 2
            || shell.faces.iter().any(|face| {
                index
                    .faces
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
                message: "shell faces are disconnected through shared edges".into(),
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
mod tests {
    use super::*;

    #[test]
    fn zero_count_composite_stage_is_compositionally_invalid() {
        let stages = [
            crate::features::PatternStage {
                pattern: Box::new(PatternKind::Linear {
                    direction: None,
                    spacing: Length(1.0),
                    count: 1,
                    second: None,
                }),
                combination: PatternStageCombination::Initialize,
            },
            crate::features::PatternStage {
                pattern: Box::new(PatternKind::Scale {
                    center: crate::features::PatternScaleCenter::FirstSeedCentroid,
                    final_factor: 2.0,
                    count: 0,
                }),
                combination: PatternStageCombination::AlignedSlices,
            },
        ];
        assert!(!composite_composition_is_valid(&stages));
    }

    #[test]
    fn unresolved_composite_count_can_feed_a_cartesian_stage() {
        let stages = [
            crate::features::PatternStage {
                pattern: Box::new(PatternKind::Unresolved { form: None }),
                combination: PatternStageCombination::Initialize,
            },
            crate::features::PatternStage {
                pattern: Box::new(PatternKind::Linear {
                    direction: None,
                    spacing: Length(1.0),
                    count: 2,
                    second: None,
                }),
                combination: PatternStageCombination::CartesianProduct,
            },
        ];
        assert!(composite_composition_is_valid(&stages));
    }
}
