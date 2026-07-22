// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for carriers parameterization.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use std::collections::VecDeque;

use crate::geometry::PcurveGeometry;

pub(super) fn check_carrier_reachability(ir: &CadIr, findings: &mut Vec<Finding>) {
    let mut surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.0.as_str())
        .collect::<HashSet<_>>();
    let mut curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.as_ref().map(|id| id.0.as_str()))
        .collect::<HashSet<_>>();
    curves.extend(
        ir.model
            .coedges
            .iter()
            .filter_map(|coedge| coedge.use_curve.as_ref().map(|id| id.0.as_str())),
    );
    surfaces.extend(
        ir.model
            .surfaces
            .iter()
            .filter(|surface| surface.source_object.is_some())
            .map(|surface| surface.id.0.as_str()),
    );
    curves.extend(
        ir.model
            .curves
            .iter()
            .filter(|curve| curve.source_object.is_some())
            .map(|curve| curve.id.0.as_str()),
    );
    let pcurves = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.as_str()))
        .chain(ir.model.loops.iter().flat_map(|loop_| {
            loop_
                .vertex_uses
                .iter()
                .flat_map(|use_| use_.pcurves.iter().map(|pcurve| pcurve.pcurve.0.as_str()))
        }))
        .collect::<HashSet<_>>();
    let mut points = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.0.as_str())
        .collect::<HashSet<_>>();
    points.extend(
        ir.model
            .points
            .iter()
            .filter(|point| point.source_object.is_some())
            .map(|point| point.id.0.as_str()),
    );
    for binding in &ir.model.appearance_bindings {
        match &binding.target {
            crate::appearance::AppearanceTarget::Surface(id) => {
                surfaces.insert(id.0.as_str());
            }
            crate::appearance::AppearanceTarget::Curve(id) => {
                curves.insert(id.0.as_str());
            }
            crate::appearance::AppearanceTarget::Point(id) => {
                points.insert(id.0.as_str());
            }
            _ => {}
        }
    }
    for item in ir
        .model
        .presentation_layers
        .iter()
        .flat_map(|layer| &layer.items)
    {
        match item {
            crate::presentation::PresentationItem::Surface { surface } => {
                surfaces.insert(surface.0.as_str());
            }
            crate::presentation::PresentationItem::Curve { curve } => {
                curves.insert(curve.0.as_str());
            }
            crate::presentation::PresentationItem::Point { point } => {
                points.insert(point.0.as_str());
            }
            _ => {}
        }
    }

    for procedural in &ir.model.procedural_surfaces {
        surfaces.insert(&procedural.surface.0);
        match &procedural.definition {
            ProceduralSurfaceDefinition::Exact { .. } => {}
            ProceduralSurfaceDefinition::Compound { components, .. } => {
                surfaces.extend(components.iter().map(|component| component.0.as_str()));
            }
            ProceduralSurfaceDefinition::SubSurface { support, .. } => {
                surfaces.insert(&support.0);
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                surfaces.insert(&support.0);
                curves.insert(&reference.0);
            }
            ProceduralSurfaceDefinition::Loft { sections, .. } => {
                for entry in sections.iter().flat_map(|section| &section.entries) {
                    if let Some(curve) = &entry.path.curve {
                        curves.insert(&curve.0);
                    }
                    curves.extend(entry.path.auxiliaries.iter().map(|curve| curve.0.as_str()));
                    for member in &entry.profile {
                        curves.insert(&member.curve.0);
                        if let Some(surface) = &member.data.surface {
                            surfaces.insert(&surface.0);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft { construction } => {
                let mut scales = construction.scales.iter().flatten().collect::<Vec<_>>();
                scales.extend(construction.fifth_scale.iter().map(Box::as_ref));
                match &construction.tail {
                    crate::geometry::CompoundLoftTail::Six { scale, curve, .. } => {
                        scales.push(scale.as_ref());
                        curves.insert(&curve.0);
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
                            curves.insert(&curve.0);
                        }
                    }
                }
                for scale in scales {
                    curves.insert(&scale.path.0);
                    curves.extend(scale.auxiliaries.iter().map(|curve| curve.0.as_str()));
                    for member in &scale.members {
                        curves.insert(&member.curve.0);
                        if let Some(surface) = &member.data.surface {
                            surfaces.insert(&surface.0);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
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
                        curves.insert(&curve.0);
                    }
                    crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve } = direction {
                            curves.insert(&curve.0);
                        }
                    }
                }
                curves.insert(&construction.tail_curve.0);
                for scale in scales {
                    curves.insert(&scale.path.0);
                    curves.extend(scale.auxiliaries.iter().map(|curve| curve.0.as_str()));
                    for member in &scale.members {
                        curves.insert(&member.curve.0);
                        if let Some(surface) = &member.data.surface {
                            surfaces.insert(&surface.0);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Skin { construction } => {
                fn collect_law_curves<'a>(
                    expression: &'a crate::geometry::LawExpression,
                    curves: &mut HashSet<&'a str>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            curves.insert(&curve.0);
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                collect_law_curves(operand, curves);
                            }
                        }
                        _ => {}
                    }
                }
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        curves.insert(&path.0);
                        for profile in profiles {
                            curves.insert(&profile.curve.0);
                            if let Some(surface) = &profile.data.surface {
                                surfaces.insert(&surface.0);
                            }
                        }
                    }
                    crate::geometry::SkinSurfaceLayout::Compact {
                        curve,
                        secondary_curve,
                        ..
                    } => {
                        curves.insert(&curve.0);
                        curves.insert(&secondary_curve.0);
                    }
                }
                curves.insert(&construction.parameter_curve.0);
                for variable in &construction.formula.variables {
                    collect_law_curves(variable, &mut curves);
                }
            }
            ProceduralSurfaceDefinition::Law { construction } => {
                fn collect_law_curves<'a>(
                    expression: &'a crate::geometry::LawExpression,
                    curves: &mut HashSet<&'a str>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            curves.insert(&curve.0);
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                collect_law_curves(operand, curves);
                            }
                        }
                        _ => {}
                    }
                }
                for formula in
                    std::iter::once(&construction.primary).chain(&construction.additional)
                {
                    for variable in &formula.variables {
                        collect_law_curves(variable, &mut curves);
                    }
                }
            }
            ProceduralSurfaceDefinition::Net { construction } => {
                fn collect_law_curves<'a>(
                    expression: &'a crate::geometry::LawExpression,
                    curves: &mut HashSet<&'a str>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            curves.insert(&curve.0);
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                collect_law_curves(operand, curves);
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
                    if let Some(curve) = &entry.path.curve {
                        curves.insert(&curve.0);
                    }
                    curves.extend(entry.path.auxiliaries.iter().map(|curve| curve.0.as_str()));
                    for member in &entry.profile {
                        curves.insert(&member.curve.0);
                        if let Some(surface) = &member.data.surface {
                            surfaces.insert(&surface.0);
                        }
                    }
                }
                for formula in construction.formulas.iter() {
                    for variable in &formula.variables {
                        collect_law_curves(variable, &mut curves);
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend { construction } => {
                for side in [&construction.first, &construction.second] {
                    surfaces.insert(&side.surface.0);
                    curves.insert(&side.curve.0);
                }
                surfaces.insert(&construction.second_exact_surface.0);
                curves.insert(&construction.center_curve.0);
                if let crate::geometry::G2BlendFirstShape::Full {
                    surface: Some(surface),
                    ..
                } = &construction.first_shape
                {
                    surfaces.insert(&surface.0);
                }
            }
            ProceduralSurfaceDefinition::VariableBlend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                    if let Some(curve) = &side.curve {
                        curves.insert(&curve.0);
                    }
                }
                curves.insert(construction.slice.0.as_str());
                curves.extend(
                    [
                        construction.secondary_curve.as_ref(),
                        construction.post_curve.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(|curve| curve.0.as_str()),
                );
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                for member in construction
                    .base_profile
                    .iter()
                    .chain(construction.entries.iter().flat_map(|entry| &entry.profile))
                {
                    curves.insert(&member.curve.0);
                    if let Some(surface) = &member.data.surface {
                        surfaces.insert(&surface.0);
                    }
                }
                for path in std::iter::once(&construction.base_path)
                    .chain(construction.entries.iter().map(|entry| &entry.path))
                {
                    if let Some(curve) = &path.curve {
                        curves.insert(&curve.0);
                    }
                    curves.extend(path.auxiliaries.iter().map(|curve| curve.0.as_str()));
                }
                curves.extend(
                    [
                        construction.direction_curve.as_ref(),
                        construction.trailing_curve.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(|curve| curve.0.as_str()),
                );
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                    if let Some(curve) = &side.curve {
                        curves.insert(&curve.0);
                    }
                }
                curves.insert(construction.center.0.as_str());
            }
            ProceduralSurfaceDefinition::VertexBlend { construction } => {
                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            curves.insert(&curve.0);
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            surfaces.insert(&surface.0);
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Degenerate { .. } => {}
                    }
                }
            }
            ProceduralSurfaceDefinition::Extrusion { directrix, .. }
            | ProceduralSurfaceDefinition::LinearSweep { directrix, .. }
            | ProceduralSurfaceDefinition::Revolution { directrix, .. }
            | ProceduralSurfaceDefinition::AxisRevolution { directrix, .. } => {
                curves.insert(&directrix.0);
            }
            ProceduralSurfaceDefinition::Sweep {
                profile,
                spine,
                native,
            } => {
                fn collect_law_curves<'a>(
                    expression: &'a crate::geometry::LawExpression,
                    curves: &mut HashSet<&'a str>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            curves.insert(&curve.0);
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                collect_law_curves(operand, curves);
                            }
                        }
                        _ => {}
                    }
                }
                curves.extend([profile.0.as_str(), spine.0.as_str()]);
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
                            curves.insert(&guide_curve.0);
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            surfaces.insert(&support_surface.0);
                            if let Some(curve) = auxiliary_curve {
                                curves.insert(&curve.0);
                            }
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::LawDriven {
                            first_law,
                            second_law,
                            formula,
                            ..
                        } => {
                            collect_law_curves(first_law, &mut curves);
                            collect_law_curves(second_law, &mut curves);
                            vec![formula]
                        }
                    };
                    for formula in formulas {
                        for variable in &formula.variables {
                            collect_law_curves(variable, &mut curves);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset { support, .. } => {
                surfaces.insert(&support.0);
            }
            ProceduralSurfaceDefinition::Subset { support, .. }
            | ProceduralSurfaceDefinition::ParallelOffset { support, .. } => {
                surfaces.insert(&support.0);
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                curves.extend([first.0.as_str(), second.0.as_str()]);
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                curves.extend([first.0.as_str(), second.0.as_str()]);
            }
            ProceduralSurfaceDefinition::Blend {
                supports,
                spine,
                native,
                ..
            } => {
                for support in supports.iter().flatten() {
                    surfaces.insert(&support.surface.0);
                }
                if let Some(spine) = spine {
                    curves.insert(&spine.0);
                }
                if let Some(native) = native {
                    curves.insert(&native.slice.0);
                    for side in native.sides.iter() {
                        if let Some(curve) = &side.curve {
                            curves.insert(&curve.0);
                        }
                        if let Some(surface) = &side.surface {
                            surfaces.insert(&surface.0);
                        }
                    }
                    if let Some(side) = &native.third {
                        curves.insert(&side.curve.0);
                        surfaces.insert(&side.surface.0);
                    }
                }
            }
            ProceduralSurfaceDefinition::Helix { .. }
            | ProceduralSurfaceDefinition::TSpline { .. }
            | ProceduralSurfaceDefinition::DegenerateTorus { .. }
            | ProceduralSurfaceDefinition::Unknown { .. } => {}
            ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                ..
            } => {
                surfaces.insert(&support.0);
                curves.extend(boundaries.iter().map(|curve| curve.0.as_str()));
            }
            ProceduralSurfaceDefinition::Deformable { construction } => {
                surfaces.insert(&construction.support.0);
                if let crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    surface, curve, ..
                }
                | crate::geometry::DeformableSurfaceData::Full { surface, curve, .. } =
                    &construction.data
                {
                    surfaces.insert(&surface.0);
                    curves.insert(&curve.0);
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        curves.insert(&procedural.curve.0);
        match &procedural.definition {
            ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix { .. } => {}
            ProceduralCurveDefinition::Law {
                context,
                primary,
                additional,
                ..
            } => {
                fn collect<'a>(
                    expression: &'a crate::geometry::LawExpression,
                    curves: &mut HashSet<&'a str>,
                ) {
                    match expression {
                        crate::geometry::LawExpression::Edge { curve, .. } => {
                            curves.insert(&curve.0);
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                collect(operand, curves);
                            }
                        }
                        _ => {}
                    }
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    for variable in &formula.variables {
                        collect(variable, &mut curves);
                    }
                }
            }
            ProceduralCurveDefinition::Compound { components, .. } => {
                curves.extend(components.iter().map(|component| component.0.as_str()));
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides.iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette {
                context,
                cast_surface,
                ..
            } => {
                surfaces.insert(&cast_surface.0);
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                curves.insert(&base.0);
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Spring { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Deformable { bend, data, .. } => {
                curves.insert(&bend.0);
                if let crate::geometry::DeformableCurveData::Surface { surface } = data {
                    surfaces.insert(&surface.0);
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                curves.insert(&source.0);
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(&surface.0);
                    }
                }
            }
            ProceduralCurveDefinition::Offset {
                source,
                support,
                distance_law,
                ..
            } => {
                curves.insert(&source.0);
                if let Some(support) = support {
                    surfaces.insert(&support.0);
                }
                if let Some(crate::geometry::CurveOffsetDistanceLaw::Coordinate {
                    function, ..
                }) = distance_law
                {
                    curves.insert(&function.0);
                }
            }
            ProceduralCurveDefinition::SpatialOffset { source, .. } => {
                curves.insert(&source.0);
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.0.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                curves.insert(&source.0);
            }
            ProceduralCurveDefinition::Subset { source, .. } => {
                curves.insert(&source.0);
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    surfaces.insert(&surface.0);
                }
            }
            ProceduralCurveDefinition::Unknown { .. } => {}
        }
    }
    let native_unknowns = ir.all_native_unknowns().unwrap_or_default();
    for link in native_unknowns.iter().flat_map(|record| &record.links) {
        surfaces.insert(link);
        curves.insert(link);
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
        .collect::<HashMap<_, _>>();
    let mut reachable_curves = curves.iter().copied().collect::<VecDeque<_>>();
    while let Some(curve) = reachable_curves.pop_front() {
        for segment in composite_segments.get(curve).into_iter().flatten() {
            if curves.insert(segment) {
                reachable_curves.push_back(segment);
            }
        }
    }

    for (kind, id) in ir
        .model
        .surfaces
        .iter()
        .filter(|entity| !surfaces.contains(entity.id.0.as_str()))
        .map(|entity| ("surface", entity.id.0.as_str()))
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|entity| !curves.contains(entity.id.0.as_str()))
                .map(|entity| ("curve", entity.id.0.as_str())),
        )
        .chain(
            ir.model
                .pcurves
                .iter()
                .filter(|entity| !pcurves.contains(entity.id.0.as_str()))
                .map(|entity| ("pcurve", entity.id.0.as_str())),
        )
        .chain(
            ir.model
                .points
                .iter()
                .filter(|entity| !points.contains(entity.id.0.as_str()))
                .map(|entity| ("point", entity.id.0.as_str())),
        )
    {
        findings.push(Finding {
            check: Check::CarrierReachability,
            severity: Severity::Error,
            message: format!("orphan {kind} carrier"),
            entity: Some(id.into()),
        });
    }
}

pub(super) fn check_parameter_domains(ir: &CadIr, findings: &mut Vec<Finding>) {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.0.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    for edge in &ir.model.edges {
        let Some([start, end]) = edge.param_range else {
            continue;
        };
        let mut valid = start.is_finite() && end.is_finite() && start <= end;
        if let Some(curve) = edge.curve.as_ref().and_then(|id| curves.get(id.0.as_str())) {
            let tau = std::f64::consts::TAU;
            match curve {
                CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
                    // Canonical periodic domain: the start angle wrapped into
                    // one turn, the sweep at most a full turn. An arc crossing
                    // the seam ends past `τ`. A full-period edge retains
                    // its serialized phase, which may use any equivalent
                    // angular branch.
                    let sweep = end - start;
                    let full_period = (sweep - tau).abs() < 1.0e-9;
                    valid &= sweep <= tau + 1.0e-9 && (full_period || (0.0..tau).contains(&start));
                }
                CurveGeometry::Nurbs(nurbs) => {
                    valid &= crate::eval::nurbs_curve_parameter_domain(nurbs).is_some_and(
                        |[lower, upper]| {
                            if nurbs.periodic {
                                let period = upper - lower;
                                let tolerance = 1.0e-9_f64.max(period.abs() * 1.0e-9);
                                end - start <= period + tolerance
                            } else {
                                start >= lower && end <= upper
                            }
                        },
                    );
                }
                _ => {}
            }
        }
        if !valid {
            findings.push(Finding {
                check: Check::ParameterDomain,
                severity: Severity::Error,
                message: "edge parameter range is outside its canonical carrier domain".into(),
                entity: Some(edge.id.0.clone()),
            });
        }
    }
    let pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.0.as_str(), &pcurve.geometry))
        .collect::<HashMap<_, _>>();
    for coedge in &ir.model.coedges {
        if coedge.use_curve.is_some() != coedge.use_curve_parameter_range.is_some() {
            findings.push(Finding {
                check: Check::ParameterDomain,
                severity: Severity::Error,
                message: "coedge use curve and parameter range must occur together".into(),
                entity: Some(coedge.id.0.clone()),
            });
        }
        if let Some([start, end]) = coedge.use_curve_parameter_range {
            let geometry = coedge
                .use_curve
                .as_ref()
                .and_then(|id| curves.get(id.0.as_str()));
            let mut valid =
                start.is_finite() && end.is_finite() && start <= end && geometry.is_some();
            if let Some(CurveGeometry::Nurbs(nurbs)) = geometry {
                if let (Some(first), Some(last)) = (nurbs.knots.first(), nurbs.knots.last()) {
                    valid &= start >= *first && end <= *last;
                }
            }
            if !valid {
                findings.push(Finding {
                    check: Check::ParameterDomain,
                    severity: Severity::Error,
                    message: "coedge use-curve range is outside its carrier domain".into(),
                    entity: Some(coedge.id.0.clone()),
                });
            }
        }
        for use_ in &coedge.pcurves {
            let Some([start, end]) = use_.parameter_range else {
                continue;
            };
            let geometry = pcurves.get(use_.pcurve.0.as_str());
            let mut valid =
                start.is_finite() && end.is_finite() && start != end && geometry.is_some();
            if let Some(PcurveGeometry::Nurbs { knots, .. }) = geometry {
                if let (Some(first), Some(last)) = (knots.first(), knots.last()) {
                    valid &= [start, end]
                        .into_iter()
                        .all(|value| value >= *first && value <= *last);
                }
            }
            if !valid {
                findings.push(Finding {
                    check: Check::ParameterDomain,
                    severity: Severity::Error,
                    message: "coedge pcurve range is outside its carrier domain".into(),
                    entity: Some(coedge.id.0.clone()),
                });
            }
        }
    }
}
