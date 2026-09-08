// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for carriers parameterization.
#![allow(clippy::wildcard_imports)]

use super::*;
use std::collections::VecDeque;

use crate::geometry::PcurveGeometry;

const EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9: f64 = 1.0e-9;
const EPS_CARRIERS_PARAMETERIZATION_PARAMETER_IN_DOMAIN_E12: f64 = 1.0e-12;

pub(super) fn check_carrier_reachability(ir: &CadIr, findings: &mut Vec<Finding>) {
    let mut surfaces = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.as_str())
        .collect::<HashSet<_>>();
    let mut curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.as_ref().map(super::super::ids::CurveId::as_str))
        .collect::<HashSet<_>>();
    curves.extend(
        ir.model
            .coedges
            .iter()
            .filter_map(|coedge| coedge.use_curve.as_ref().map(|use_| use_.curve.as_str())),
    );
    surfaces.extend(
        ir.model
            .surfaces
            .iter()
            .filter(|surface| surface.source_object.is_some())
            .map(|surface| surface.id.as_str()),
    );
    curves.extend(
        ir.model
            .curves
            .iter()
            .filter(|curve| curve.source_object.is_some())
            .map(|curve| curve.id.as_str()),
    );
    let mut pcurves = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.as_str()))
        .chain(
            ir.model
                .loops
                .iter()
                .flat_map(crate::topology::Loop::vertex_pcurves)
                .map(|pcurve| pcurve.pcurve.as_str()),
        )
        .collect::<HashSet<_>>();
    for surface in &ir.model.procedural_surfaces {
        if let ProceduralSurfaceDefinition::CurveBounded {
            boundary_pcurves, ..
        } = surface.definition()
        {
            pcurves.extend(
                boundary_pcurves
                    .iter()
                    .map(super::super::ids::PcurveId::as_str),
            );
        }
    }
    let mut points = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.as_str())
        .collect::<HashSet<_>>();
    points.extend(
        ir.model
            .points
            .iter()
            .filter(|point| point.source_object.is_some())
            .map(|point| point.id.as_str()),
    );
    for binding in &ir.model.appearance_bindings {
        match &binding.target {
            crate::appearance::AppearanceTarget::Surface(id) => {
                surfaces.insert(id.as_str());
            }
            crate::appearance::AppearanceTarget::Curve(id) => {
                curves.insert(id.as_str());
            }
            crate::appearance::AppearanceTarget::Point(id) => {
                points.insert(id.as_str());
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
                surfaces.insert(surface.as_str());
            }
            crate::presentation::PresentationItem::Curve { curve } => {
                curves.insert(curve.as_str());
            }
            crate::presentation::PresentationItem::Point { point } => {
                points.insert(point.as_str());
            }
            _ => {}
        }
    }

    for procedural in &ir.model.procedural_surfaces {
        if let Some(surface) = ir.model.procedural_surface_owner(&procedural.id) {
            surfaces.insert(surface.as_str());
        }
        match procedural.definition() {
            ProceduralSurfaceDefinition::Exact { .. } => {}
            ProceduralSurfaceDefinition::Compound { components, .. } => {
                surfaces.extend(
                    components
                        .iter()
                        .map(|component| component.component.as_str()),
                );
            }
            ProceduralSurfaceDefinition::SubSurface { support, .. } => {
                surfaces.insert(support.as_str());
            }
            ProceduralSurfaceDefinition::Taper {
                support, reference, ..
            } => {
                surfaces.insert(support.as_str());
                curves.insert(reference.as_str());
            }
            ProceduralSurfaceDefinition::Loft { sections, .. } => {
                for entry in sections.iter().flat_map(|section| &section.entries) {
                    if let Some(curve) = &entry.path.curve {
                        curves.insert(curve.id.as_str());
                    }
                    curves.extend(
                        entry
                            .path
                            .auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    );
                    for member in &entry.profile {
                        curves.insert(member.curve.id.as_str());
                        if let Some(surface) = member.form.surface() {
                            surfaces.insert(surface.as_str());
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
                        curves.insert(curve.as_str());
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
                            curves.insert(curve.as_str());
                        }
                    }
                }
                for scale in scales {
                    curves.insert(scale.path.as_str());
                    curves.extend(
                        scale
                            .auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    );
                    for member in &scale.members {
                        curves.insert(member.curve.as_str());
                        surfaces.insert(member.data.surface.as_str());
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
                        curves.insert(curve.as_str());
                    }
                    crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
                            curves.insert(curve.as_str());
                        }
                    }
                }
                curves.insert(construction.tail_curve.as_str());
                for scale in scales {
                    curves.insert(scale.path.as_str());
                    curves.extend(
                        scale
                            .auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    );
                    for member in &scale.members {
                        curves.insert(member.curve.as_str());
                        surfaces.insert(member.data.surface.as_str());
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
                            curves.insert(curve.id.as_str());
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
                        curves.insert(path.as_str());
                        for profile in profiles {
                            curves.insert(profile.curve.as_str());
                            surfaces.insert(profile.data.surface.as_str());
                        }
                    }
                    crate::geometry::SkinSurfaceLayout::Compact {
                        curve,
                        secondary_curve,
                        ..
                    } => {
                        curves.insert(curve.as_str());
                        curves.insert(secondary_curve.as_str());
                    }
                }
                curves.insert(construction.parameter_curve.as_str());
                for variable in construction.formula.variables() {
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
                            curves.insert(curve.id.as_str());
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
                    for variable in formula.variables() {
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
                            curves.insert(curve.id.as_str());
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
                        curves.insert(curve.id.as_str());
                    }
                    curves.extend(
                        entry
                            .path
                            .auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    );
                    for member in &entry.profile {
                        curves.insert(member.curve.id.as_str());
                        if let Some(surface) = member.form.surface() {
                            surfaces.insert(surface.as_str());
                        }
                    }
                }
                for formula in construction.formulas.iter() {
                    for variable in formula.variables() {
                        collect_law_curves(variable, &mut curves);
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend { construction } => {
                for side in [&construction.first, &construction.second] {
                    surfaces.insert(side.surface.as_str());
                    curves.insert(side.curve.as_str());
                }
                surfaces.insert(construction.second_exact_surface.as_str());
                curves.insert(construction.center_curve.as_str());
                if let crate::geometry::G2BlendFirstShape::Full {
                    support: Some(support),
                } = &construction.first_shape
                {
                    surfaces.insert(support.surface.as_str());
                }
            }
            ProceduralSurfaceDefinition::VariableBlend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.surface.as_str());
                    }
                    if let Some(curve) = &side.curve {
                        curves.insert(curve.curve.as_str());
                    }
                }
                curves.insert(construction.slice.as_str());
                curves.extend(
                    [
                        construction
                            .secondary_curve
                            .as_ref()
                            .map(|curve| &curve.curve),
                        construction.post_curve.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(super::super::ids::CurveId::as_str),
                );
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                for member in construction
                    .base_profile
                    .iter()
                    .chain(construction.entries.iter().flat_map(|entry| &entry.profile))
                {
                    curves.insert(member.curve.id.as_str());
                    if let Some(surface) = member.form.surface() {
                        surfaces.insert(surface.as_str());
                    }
                }
                for path in std::iter::once(&construction.base_path)
                    .chain(construction.entries.iter().map(|entry| &entry.path))
                {
                    if let Some(curve) = &path.curve {
                        curves.insert(curve.id.as_str());
                    }
                    curves.extend(
                        path.auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    );
                }
                curves.extend(
                    [
                        match &construction.direction {
                            crate::geometry::CompoundLoftDirection::Vector { .. } => None,
                            crate::geometry::CompoundLoftDirection::Curve { curve, .. } => {
                                Some(curve)
                            }
                        },
                        construction.tail.curve(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(super::super::ids::CurveId::as_str),
                );
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides.iter() {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.surface.as_str());
                    }
                    if let Some(curve) = &side.curve {
                        curves.insert(curve.curve.as_str());
                    }
                }
                curves.insert(construction.center.as_str());
            }
            ProceduralSurfaceDefinition::VertexBlend { construction } => {
                for boundary in &construction.boundaries {
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            curves.insert(curve.as_str());
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            surfaces.insert(surface.as_str());
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Degenerate { .. } => {}
                    }
                }
            }
            ProceduralSurfaceDefinition::Extrusion { directrix, .. }
            | ProceduralSurfaceDefinition::LinearSweep { directrix, .. }
            | ProceduralSurfaceDefinition::Revolution { directrix, .. }
            | ProceduralSurfaceDefinition::AxisRevolution { directrix, .. } => {
                curves.insert(directrix.as_str());
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
                            curves.insert(curve.id.as_str());
                        }
                        crate::geometry::LawExpression::Algebraic { operands, .. } => {
                            for operand in operands {
                                collect_law_curves(operand, curves);
                            }
                        }
                        _ => {}
                    }
                }
                curves.extend([profile.as_str(), spine.as_str()]);
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
                            curves.insert(guide_curve.as_str());
                            Vec::new()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            surfaces.insert(support_surface.as_str());
                            if let Some(curve) = auxiliary_curve {
                                curves.insert(curve.as_str());
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
                        for variable in formula.variables() {
                            collect_law_curves(variable, &mut curves);
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset { support, .. } => {
                surfaces.insert(support.as_str());
            }
            ProceduralSurfaceDefinition::Replica {
                source: support, ..
            }
            | ProceduralSurfaceDefinition::Subset { support, .. }
            | ProceduralSurfaceDefinition::ParallelOffset { support, .. } => {
                surfaces.insert(support.as_str());
            }
            ProceduralSurfaceDefinition::Ruled { first, second } => {
                curves.extend([first.as_str(), second.as_str()]);
            }
            ProceduralSurfaceDefinition::Sum { first, second, .. } => {
                curves.extend([first.as_str(), second.as_str()]);
            }
            ProceduralSurfaceDefinition::Blend {
                supports,
                spine,
                native,
                ..
            } => {
                for support in supports.iter().flatten() {
                    surfaces.insert(support.surface.as_str());
                }
                if let Some(spine) = spine {
                    curves.insert(spine.as_str());
                }
                if let Some(native) = native {
                    curves.insert(native.slice.as_str());
                    for side in native.sides.iter() {
                        if let Some(curve) = &side.curve {
                            curves.insert(curve.curve.as_str());
                        }
                        if let Some(surface) = &side.surface {
                            surfaces.insert(surface.surface.as_str());
                        }
                    }
                    if let Some(side) = &native.third {
                        curves.insert(side.curve.as_str());
                        surfaces.insert(side.surface.as_str());
                    }
                }
            }
            ProceduralSurfaceDefinition::RollingBallJet { .. }
            | ProceduralSurfaceDefinition::Helix { .. }
            | ProceduralSurfaceDefinition::TSpline { .. }
            | ProceduralSurfaceDefinition::DegenerateTorus { .. }
            | ProceduralSurfaceDefinition::Unknown { .. } => {}
            ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                ..
            } => {
                surfaces.insert(support.as_str());
                curves.extend(boundaries.iter().map(super::super::ids::CurveId::as_str));
            }
            ProceduralSurfaceDefinition::Deformable { construction } => {
                surfaces.insert(construction.support.as_str());
                if let crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    surface, curve, ..
                }
                | crate::geometry::DeformableSurfaceData::Full { surface, curve, .. } =
                    &construction.data
                {
                    surfaces.insert(surface.as_str());
                    curves.insert(curve.as_str());
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        if let Some(curve) = ir.model.procedural_curve_owner(&procedural.id) {
            curves.insert(curve.as_str());
        }
        match procedural.definition() {
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
                            curves.insert(curve.id.as_str());
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
                        surfaces.insert(surface.as_str());
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    for variable in formula.variables() {
                        collect(variable, &mut curves);
                    }
                }
            }
            ProceduralCurveDefinition::Compound { components, .. } => {
                curves.extend(
                    components
                        .iter()
                        .map(|component| component.component.as_str()),
                );
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::TolerantIntersection { supports, .. } => {
                surfaces.extend(supports.iter().map(super::super::ids::SurfaceId::as_str));
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } => {
                for side in context.sides.iter().chain(std::iter::once(third)) {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { family } => {
                for side in &family.context().sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette {
                context,
                cast_surface,
                ..
            } => {
                surfaces.insert(cast_surface.as_str());
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset { context, base, .. } => {
                curves.insert(base.as_str());
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::Spring { layout, .. } => match layout {
                crate::geometry::SpringLayout::ContextFirst { supports, .. } => {
                    for support in supports {
                        if let crate::geometry::SpringSupport::Surface(surface) = support {
                            surfaces.insert(surface.as_str());
                        }
                    }
                }
                crate::geometry::SpringLayout::CacheFirst { context, .. } => {
                    for side in &context.sides {
                        if let Some(surface) = &side.surface {
                            surfaces.insert(surface.as_str());
                        }
                    }
                }
            },
            ProceduralCurveDefinition::Deformable {
                context, source, ..
            } => {
                if let crate::geometry::DeformableCurveSource::Curve { curve } = source {
                    curves.insert(curve.as_str());
                }
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::Projection {
                context, source, ..
            } => {
                curves.insert(source.as_str());
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::Offset {
                source,
                side,
                range,
                ..
            } => {
                curves.insert(source.as_str());
                if let crate::geometry::OffsetSide::Direction {
                    support: Some(support),
                    ..
                } = side
                {
                    surfaces.insert(support.as_str());
                }
                if let Some(crate::geometry::CurveOffsetRange::Variable {
                    distance_law:
                        crate::geometry::CurveOffsetDistanceLaw::Coordinate { function, .. },
                    ..
                }) = range
                {
                    curves.insert(function.as_str());
                }
            }
            ProceduralCurveDefinition::SpatialOffset { source, .. } => {
                curves.insert(source.as_str());
            }
            ProceduralCurveDefinition::TwoSidedOffset { context, .. } => {
                for side in &context.sides {
                    if let Some(surface) = &side.surface {
                        surfaces.insert(surface.as_str());
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset { source, .. } => {
                curves.insert(source.as_str());
            }
            ProceduralCurveDefinition::Replica { source, .. }
            | ProceduralCurveDefinition::Subset { source, .. } => {
                curves.insert(source.as_str());
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    surfaces.insert(surface.as_str());
                }
            }
            ProceduralCurveDefinition::Unknown { .. } => {}
        }
    }
    // Only the link targets are wanted, so the records are consumed one at a
    // time and dropped; an arena that cannot be read contributes nothing, as it
    // did when the whole population was deserialized in one fallible step.
    let native_links = ir
        .all_native_unknowns_iter()
        .try_fold(Vec::new(), |mut links, record| {
            links.extend(record?.links);
            Ok::<_, crate::native::NativeConvertError>(links)
        })
        .unwrap_or_default();
    for link in &native_links {
        surfaces.insert(link);
        curves.insert(link);
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
        .filter(|entity| !surfaces.contains(entity.id.as_str()))
        .map(|entity| ("surface", entity.id.as_str()))
        .chain(
            ir.model
                .curves
                .iter()
                .filter(|entity| !curves.contains(entity.id.as_str()))
                .map(|entity| ("curve", entity.id.as_str())),
        )
        .chain(
            ir.model
                .pcurves
                .iter()
                .filter(|entity| !pcurves.contains(entity.id.as_str()))
                .map(|entity| ("pcurve", entity.id.as_str())),
        )
        .chain(
            ir.model
                .points
                .iter()
                .filter(|entity| !points.contains(entity.id.as_str()))
                .map(|entity| ("point", entity.id.as_str())),
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
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    for edge in &ir.model.edges {
        let Some([start, end]) = edge.param_range else {
            continue;
        };
        let mut valid = start.is_finite() && end.is_finite();
        // A null carrier has no canonical parameter domain. Keep checking
        // that retained native endpoint values are finite, but do not impose
        // an ordering that belongs to a carrier-backed parameterization.
        if edge.curve.is_some() {
            valid &= start <= end;
        }
        if let Some(curve) = edge.curve.as_ref().and_then(|id| curves.get(id.as_str())) {
            let tau = std::f64::consts::TAU;
            match curve {
                CurveGeometry::Circle(_) => {
                    // Canonical periodic domain: the start angle wrapped into
                    // one turn, the sweep at most a full turn. An arc crossing
                    // the seam ends past `τ`. A full-period edge retains
                    // its serialized phase, which may use any equivalent
                    // angular branch.
                    let sweep = end - start;
                    let full_period = (sweep - tau).abs()
                        < EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9;
                    valid &= sweep
                        <= tau + EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9
                        && (full_period || (0.0..tau).contains(&start));
                }
                CurveGeometry::Ellipse(_) => {
                    // Canonical periodic domain: the start angle wrapped into
                    // one turn, the sweep at most a full turn. An arc crossing
                    // the seam ends past `τ`. A full-period edge retains
                    // its serialized phase, which may use any equivalent
                    // angular branch.
                    let sweep = end - start;
                    let full_period = (sweep - tau).abs()
                        < EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9;
                    valid &= sweep
                        <= tau + EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9
                        && (full_period || (0.0..tau).contains(&start));
                }
                CurveGeometry::Nurbs(nurbs) => {
                    valid &= crate::eval::nurbs_curve_parameter_domain(nurbs).is_some_and(
                        |[lower, upper]| {
                            if nurbs.periodic() {
                                let period = upper - lower;
                                let tolerance = 1.0e-9_f64.max(
                                    period.abs()
                                        * EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9,
                                );
                                end - start <= period + tolerance
                            } else {
                                parameter_in_domain(start, [lower, upper])
                                    && parameter_in_domain(end, [lower, upper])
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
                entity: Some(edge.id.as_str().to_owned()),
            });
        }
    }
    let pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), &pcurve.geometry))
        .collect::<HashMap<_, _>>();
    for coedge in &ir.model.coedges {
        if let Some(use_curve) = &coedge.use_curve {
            let [start, end] = use_curve.parameter_range;
            let geometry = curves.get(use_curve.curve.as_str());
            let mut valid =
                start.is_finite() && end.is_finite() && start <= end && geometry.is_some();
            if let Some(CurveGeometry::Nurbs(nurbs)) = geometry {
                valid &= crate::eval::nurbs_curve_parameter_domain(nurbs).is_some_and(|domain| {
                    parameter_in_domain(start, domain) && parameter_in_domain(end, domain)
                });
            }
            if !valid {
                findings.push(Finding {
                    check: Check::ParameterDomain,
                    severity: Severity::Error,
                    message: "coedge use-curve range is outside its carrier domain".into(),
                    entity: Some(coedge.id.as_str().to_owned()),
                });
            }
        }
        for use_ in &coedge.pcurves {
            let Some([start, end]) = use_.parameter_range else {
                continue;
            };
            let geometry = pcurves.get(use_.pcurve.as_str());
            let mut valid =
                start.is_finite() && end.is_finite() && start != end && geometry.is_some();
            if let Some(geometry) = geometry {
                let domain = pcurve_parameter_domain(geometry);
                match domain {
                    Some([lower, upper]) => {
                        valid &= [start, end]
                            .into_iter()
                            .all(|value| parameter_in_domain(value, [lower, upper]));
                    }
                    None if pcurve_requires_bounded_domain(geometry) => {
                        valid = false;
                    }
                    None => {}
                }
            }
            if !valid {
                findings.push(Finding {
                    check: Check::ParameterDomain,
                    severity: Severity::Error,
                    message: "coedge pcurve range is outside its carrier domain".into(),
                    entity: Some(coedge.id.as_str().to_owned()),
                });
            }
        }
    }
}

fn parameter_in_domain(value: f64, [lower, upper]: [f64; 2]) -> bool {
    // Independent serialization of a carrier and its use range can round the
    // same boundary to adjacent floating-point values.
    let scale = value.abs().max(lower.abs()).max(upper.abs()).max(1.0);
    let tolerance = scale * EPS_CARRIERS_PARAMETERIZATION_PARAMETER_IN_DOMAIN_E12;
    value >= lower - tolerance && value <= upper + tolerance
}

fn pcurve_parameter_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs { nurbs } => crate::eval::nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.control_points().len(),
        ),
        PcurveGeometry::PolarNurbs { nurbs } => crate::eval::nurbs_pcurve_parameter_domain(
            nurbs.degree(),
            nurbs.knots(),
            nurbs.poles().len(),
        ),
        PcurveGeometry::Transformed { basis, .. } => pcurve_parameter_domain(basis),
        _ => None,
    }
}

fn pcurve_requires_bounded_domain(geometry: &PcurveGeometry) -> bool {
    match geometry {
        PcurveGeometry::Nurbs { .. } | PcurveGeometry::PolarNurbs { .. } => true,
        PcurveGeometry::Transformed { basis, .. } => pcurve_requires_bounded_domain(basis),
        PcurveGeometry::Line(_) => false,
        PcurveGeometry::SphericalGreatCircle(_) => false,
        PcurveGeometry::Circle(_) => false,
        PcurveGeometry::Ellipse(_) => false,
        PcurveGeometry::Harmonic(_) => false,
        PcurveGeometry::Parabola(_) => false,
        PcurveGeometry::Hyperbola(_) => false,
        PcurveGeometry::Hyperbolic(_) => false,
        PcurveGeometry::Trimmed(_) => false,
        PcurveGeometry::Offset(_) => false,
        PcurveGeometry::PolarHarmonic(_) => false,
    }
}

#[cfg(test)]
mod tests;
