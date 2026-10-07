// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for carriers parameterization.

use crate::index::identities::BorrowedIdentities;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::{
    pcurve::PcurveGeometry, CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition,
    SolvedCurveGeometry,
};
use crate::report::{
    check::{Check, Finding},
    Severity,
};

use super::pcurve_parameter_domain;

const EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9: f64 = 1.0e-9;
const EPS_CARRIERS_PARAMETERIZATION_PARAMETER_IN_DOMAIN_E12: f64 = 1.0e-12;

fn collect_law_curves<'a, R, V, P>(
    expression: &'a crate::geometry::LawExpression<R, V, P>,
    curves: &mut BorrowedIdentities<'_, 'a>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("carrier law nesting")?;
    ctx.charge_work(1, "carrier law visit")?;
    match expression {
        crate::geometry::LawExpression::Edge { curve, .. } => {
            curves.insert_unique(curve.id.as_str(), ())?;
        }
        crate::geometry::LawExpression::Algebraic { operands, .. } => {
            for operand in operands {
                ctx.charge_work(1, "carrier reference scan")?;
                collect_law_curves(operand, curves, ctx)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn check_carrier_reachability(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    view: crate::native::view::NativeView<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let ir = view.ir;
    let mut surfaces = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut curves = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut pcurves = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut points = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for face in &ir.model.faces {
        ctx.charge_work(1, "carrier reference scan")?;
        surfaces.insert_unique(face.surface.as_str(), ())?;
    }
    for edge in &ir.model.edges {
        ctx.charge_work(1, "carrier reference scan")?;
        if let Some(curve) = edge.curve() {
            curves.insert_unique(curve.as_str(), ())?;
        }
    }
    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "carrier reference scan")?;
        if let Some(use_) = &coedge.use_curve {
            curves.insert_unique(use_.curve.as_str(), ())?;
        }
        for use_ in &coedge.pcurves {
            ctx.charge_work(1, "carrier reference scan")?;
            pcurves.insert_unique(use_.pcurve.as_str(), ())?;
        }
    }
    for surface in &ir.model.surfaces {
        ctx.charge_work(1, "carrier reference scan")?;
        if surface.source_object.is_some() {
            surfaces.insert_unique(surface.id.as_str(), ())?;
        }
    }
    for curve in &ir.model.curves {
        ctx.charge_work(1, "carrier reference scan")?;
        if curve.source_object.is_some() {
            curves.insert_unique(curve.id.as_str(), ())?;
        }
    }
    for loop_ in &ir.model.loops {
        ctx.charge_work(1, "carrier reference scan")?;
        for use_ in loop_.vertex_pcurves() {
            ctx.charge_work(1, "carrier reference scan")?;
            pcurves.insert_unique(use_.pcurve.as_str(), ())?;
        }
    }
    for surface in &ir.model.procedural_surfaces {
        ctx.charge_work(1, "carrier reference scan")?;
        if let ProceduralSurfaceDefinition::CurveBounded {
            boundary_pcurves, ..
        } = surface.definition()
        {
            pcurves.extend_unique(
                boundary_pcurves
                    .iter()
                    .map(super::super::ids::PcurveId::as_str),
            )?;
        }
    }
    for vertex in &ir.model.vertices {
        ctx.charge_work(1, "carrier reference scan")?;
        points.insert_unique(vertex.point.as_str(), ())?;
    }
    for point in &ir.model.points {
        ctx.charge_work(1, "carrier reference scan")?;
        if point.source_object.is_some() {
            points.insert_unique(point.id.as_str(), ())?;
        }
    }
    let surface_owners = BorrowedIdentities::build(ctx, |add| {
        for surface in &ir.model.surfaces {
            ctx.charge_work(1, "carrier reference scan")?;
            if let Some(procedural) = surface.geometry.procedural_construction() {
                add(procedural.as_str(), &surface.id)?;
            }
        }
        Ok(())
    })?;
    let curve_owners = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves {
            ctx.charge_work(1, "carrier reference scan")?;
            if let Some(procedural) = curve.geometry.procedural_construction() {
                add(procedural.as_str(), &curve.id)?;
            }
        }
        Ok(())
    })?;
    for binding in &ir.model.appearance_bindings {
        ctx.charge_work(1, "carrier reference scan")?;
        match &binding.target {
            crate::appearance::AppearanceTarget::Surface(id) => {
                surfaces.insert_unique(id.as_str(), ())?;
            }
            crate::appearance::AppearanceTarget::Curve(id) => {
                curves.insert_unique(id.as_str(), ())?;
            }
            crate::appearance::AppearanceTarget::Point(id) => {
                points.insert_unique(id.as_str(), ())?;
            }
            _ => {}
        }
    }
    for layer in ctx.admit_iter(
        &ir.model.presentation_layers,
        "carrier presentation layer scan",
    )? {
        for item in ctx.admit_iter(&layer.items, "carrier reference scan")? {
            match item {
                crate::presentation::PresentationItem::Surface { surface } => {
                    surfaces.insert_unique(surface.as_str(), ())?;
                }
                crate::presentation::PresentationItem::Curve { curve } => {
                    curves.insert_unique(curve.as_str(), ())?;
                }
                crate::presentation::PresentationItem::Point { point } => {
                    points.insert_unique(point.as_str(), ())?;
                }
                _ => {}
            }
        }
    }

    for procedural in &ir.model.procedural_surfaces {
        ctx.charge_work(1, "carrier reference scan")?;
        if let Some(surface) = surface_owners
            .get_unique(ctx, procedural.id.as_str())?
            .copied()
        {
            surfaces.insert_unique(surface.as_str(), ())?;
        }
        match procedural.definition() {
            ProceduralSurfaceDefinition::Exact(..) => {}
            ProceduralSurfaceDefinition::Compound(definition_payload) => {
                let components = definition_payload.components();

                surfaces.extend_unique(
                    components
                        .iter()
                        .map(|component| component.component.as_str()),
                )?;
            }
            ProceduralSurfaceDefinition::SubSurface(definition_payload) => {
                let support = definition_payload.support();
                {
                    surfaces.insert_unique(support.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::Taper(definition_payload) => {
                let support = definition_payload.support();
                let reference = definition_payload.reference();
                {
                    surfaces.insert_unique(support.as_str(), ())?;
                    curves.insert_unique(reference.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::Loft(definition_payload) => {
                let sections = definition_payload.sections();

                for section in ctx.admit_iter(sections, "carrier loft section scan")? {
                    for entry in ctx.admit_iter(&section.entries, "carrier reference scan")? {
                        if let Some(curve) = &entry.path.path {
                            curves.insert_unique(curve.id.as_str(), ())?;
                        }
                        curves.extend_unique(
                            entry
                                .path
                                .auxiliaries
                                .iter()
                                .map(super::super::ids::CurveId::as_str),
                        )?;
                        for member in &entry.profile {
                            ctx.charge_work(1, "carrier reference scan")?;
                            curves.insert_unique(member.profile.id.as_str(), ())?;
                            if let Some(surface) = member.form.surface() {
                                surfaces.insert_unique(surface.as_str(), ())?;
                            }
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::CompoundLoft(definition_payload) => {
                let construction = definition_payload.construction();

                let mut extra_scales = [None, None];
                match &construction.tail {
                    crate::geometry::CompoundLoftTail::Six { scale, curve, .. } => {
                        extra_scales[0] = Some(scale.as_ref());
                        curves.insert_unique(curve.as_str(), ())?;
                    }
                    crate::geometry::CompoundLoftTail::Seven {
                        first_scale,
                        second_scale,
                        ..
                    } => {
                        extra_scales[0] = first_scale.as_deref();
                        extra_scales[1] = Some(second_scale.as_ref());
                    }
                    crate::geometry::CompoundLoftTail::Zero { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
                            curves.insert_unique(curve.as_str(), ())?;
                        }
                    }
                }
                for scale in construction
                    .scales
                    .as_slice()
                    .iter()
                    .chain(extra_scales.into_iter().flatten())
                {
                    ctx.charge_work(1, "carrier reference scan")?;
                    curves.insert_unique(scale.path.as_str(), ())?;
                    curves.extend_unique(
                        scale
                            .auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    )?;
                    for member in &scale.members {
                        ctx.charge_work(1, "carrier reference scan")?;
                        curves.insert_unique(member.curve.as_str(), ())?;
                        surfaces.insert_unique(member.data.surface.as_str(), ())?;
                    }
                }
            }
            ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) => {
                let construction = definition_payload.construction();

                let mut extra_scales = [None, None];
                match &construction.branch {
                    crate::geometry::ScaledCompoundLoftBranch::ExtendedVector {
                        first_scale,
                        second_scale,
                        ..
                    } => {
                        extra_scales[0] = first_scale.as_deref();
                        extra_scales[1] = Some(second_scale.as_ref());
                    }
                    crate::geometry::ScaledCompoundLoftBranch::ExtendedCurve {
                        scale,
                        curve,
                        ..
                    } => {
                        extra_scales[0] = scale.as_deref();
                        curves.insert_unique(curve.as_str(), ())?;
                    }
                    crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                        if let crate::geometry::CompoundLoftDirection::Curve { curve, .. } =
                            direction
                        {
                            curves.insert_unique(curve.as_str(), ())?;
                        }
                    }
                }
                curves.insert_unique(construction.tail_curve.as_str(), ())?;
                for scale in construction
                    .scales
                    .as_slice()
                    .iter()
                    .chain(extra_scales.into_iter().flatten())
                {
                    ctx.charge_work(1, "carrier reference scan")?;
                    curves.insert_unique(scale.path.as_str(), ())?;
                    curves.extend_unique(
                        scale
                            .auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    )?;
                    for member in &scale.members {
                        ctx.charge_work(1, "carrier reference scan")?;
                        curves.insert_unique(member.curve.as_str(), ())?;
                        surfaces.insert_unique(member.data.surface.as_str(), ())?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Skin(definition_payload) => {
                let construction = definition_payload.construction();
                match &construction.layout {
                    crate::geometry::SkinSurfaceLayout::Profiles { profiles, path, .. } => {
                        curves.insert_unique(path.as_str(), ())?;
                        for profile in profiles {
                            ctx.charge_work(1, "carrier reference scan")?;
                            curves.insert_unique(profile.curve.as_str(), ())?;
                            surfaces.insert_unique(profile.data.surface.as_str(), ())?;
                        }
                    }
                    crate::geometry::SkinSurfaceLayout::Compact {
                        curve,
                        secondary_curve,
                        ..
                    } => {
                        curves.insert_unique(curve.as_str(), ())?;
                        curves.insert_unique(secondary_curve.as_str(), ())?;
                    }
                }
                curves.insert_unique(construction.parameter_curve.as_str(), ())?;
                for variable in construction.formula.variables() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    collect_law_curves(variable, &mut curves, ctx)?;
                }
            }
            ProceduralSurfaceDefinition::Law(definition_payload) => {
                let construction = definition_payload.construction();
                for formula in
                    std::iter::once(&construction.primary).chain(&construction.additional)
                {
                    ctx.charge_work(1, "carrier reference scan")?;
                    for variable in formula.variables() {
                        ctx.charge_work(1, "carrier reference scan")?;
                        collect_law_curves(variable, &mut curves, ctx)?;
                    }
                }
            }
            ProceduralSurfaceDefinition::Net(definition_payload) => {
                let construction = definition_payload.construction();
                for section in
                    ctx.admit_iter(&construction.sections[..], "carrier net section scan")?
                {
                    for entry in ctx.admit_iter(&section.entries, "carrier reference scan")? {
                        if let Some(curve) = &entry.path.path {
                            curves.insert_unique(curve.id.as_str(), ())?;
                        }
                        curves.extend_unique(
                            entry
                                .path
                                .auxiliaries
                                .iter()
                                .map(super::super::ids::CurveId::as_str),
                        )?;
                        for member in &entry.profile {
                            ctx.charge_work(1, "carrier reference scan")?;
                            curves.insert_unique(member.profile.id.as_str(), ())?;
                            if let Some(surface) = member.form.surface() {
                                surfaces.insert_unique(surface.as_str(), ())?;
                            }
                        }
                    }
                }
                for formula in
                    ctx.admit_iter(&construction.formulas[..], "carrier reference scan")?
                {
                    for variable in formula.variables() {
                        ctx.charge_work(1, "carrier reference scan")?;
                        collect_law_curves(variable, &mut curves, ctx)?;
                    }
                }
            }
            ProceduralSurfaceDefinition::G2Blend(definition_payload) => {
                let construction = definition_payload.construction();

                for side in [&construction.first, &construction.second] {
                    ctx.charge_work(1, "carrier reference scan")?;
                    surfaces.insert_unique(side.surface.as_str(), ())?;
                    curves.insert_unique(side.curve.as_str(), ())?;
                }
                surfaces.insert_unique(construction.second_exact_surface.as_str(), ())?;
                curves.insert_unique(construction.center_curve.as_str(), ())?;
                if let crate::geometry::G2BlendFirstShape::Full {
                    support: Some(support),
                } = &construction.first_shape
                {
                    surfaces.insert_unique(support.surface.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
                let construction = definition_payload.construction();

                for side in &construction.sides {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.surface.as_str(), ())?;
                    }
                    if let Some(curve) = &side.curve {
                        curves.insert_unique(curve.curve.as_str(), ())?;
                    }
                }
                curves.insert_unique(construction.slice.as_str(), ())?;
                curves.extend_unique(
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
                )?;
            }
            ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
                ctx.charge_work(
                    u64_from_index(construction.entries().len()),
                    "carrier revision profile entry scan",
                )?;
                for member in construction.base_profile().iter().chain(
                    construction
                        .entries()
                        .iter()
                        .flat_map(|entry| &entry.profile),
                ) {
                    ctx.charge_work(1, "carrier reference scan")?;
                    curves.insert_unique(member.profile.id.as_str(), ())?;
                    if let Some(surface) = member.form.surface() {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
                for path in std::iter::once(construction.base_path())
                    .chain(construction.entries().iter().map(|entry| &entry.path))
                {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(curve) = &path.path {
                        curves.insert_unique(curve.id.as_str(), ())?;
                    }
                    curves.extend_unique(
                        path.auxiliaries
                            .iter()
                            .map(super::super::ids::CurveId::as_str),
                    )?;
                }
                curves.extend_unique(
                    [
                        match construction.direction() {
                            crate::geometry::CompoundLoftDirection::Vector { .. } => None,
                            crate::geometry::CompoundLoftDirection::Curve { curve, .. } => {
                                Some(curve)
                            }
                        },
                        construction.tail().curve(),
                    ]
                    .into_iter()
                    .flatten()
                    .map(super::super::ids::CurveId::as_str),
                )?;
            }
            ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
                for side in construction.sides() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.surface.as_str(), ())?;
                    }
                    if let Some(curve) = &side.curve {
                        curves.insert_unique(curve.curve.as_str(), ())?;
                    }
                }
                curves.insert_unique(construction.center().as_str(), ())?;
            }
            ProceduralSurfaceDefinition::VertexBlend(definition_payload) => {
                let construction = definition_payload.construction();

                for boundary in &construction.boundaries {
                    ctx.charge_work(1, "carrier reference scan")?;
                    match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle { curve, .. }
                        | crate::geometry::VertexBlendBoundaryGeometry::Plane { curve, .. } => {
                            curves.insert_unique(curve.as_str(), ())?;
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            surface, ..
                        } => {
                            surfaces.insert_unique(surface.as_str(), ())?;
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Degenerate { .. } => {}
                    }
                }
            }
            ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
                let directrix = definition_payload.directrix();
                {
                    curves.insert_unique(directrix.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
                curves.insert_unique(definition_payload.directrix().as_str(), ())?;
            }
            ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                let directrix = definition_payload.directrix();
                {
                    curves.insert_unique(directrix.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::AxisRevolution(definition_payload) => {
                curves.insert_unique(definition_payload.directrix().as_str(), ())?;
            }
            ProceduralSurfaceDefinition::Sweep(definition_payload) => {
                let profile = definition_payload.profile();
                let spine = definition_payload.spine();
                let native = definition_payload.native();
                curves.extend_unique([profile.as_str(), spine.as_str()])?;
                if let Some(native) = native {
                    let formulas = match &native.layout {
                        crate::geometry::SweepSurfaceLayout::ProfileFirst { formulas, .. } => {
                            formulas.as_slice()
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitFormula {
                            formula, ..
                        } => std::slice::from_ref(formula),
                        crate::geometry::SweepSurfaceLayout::ExplicitGuide {
                            guide_curve, ..
                        } => {
                            curves.insert_unique(guide_curve.as_str(), ())?;
                            &[]
                        }
                        crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                            support_surface,
                            auxiliary_curve,
                            ..
                        } => {
                            surfaces.insert_unique(support_surface.as_str(), ())?;
                            if let Some(curve) = auxiliary_curve {
                                curves.insert_unique(curve.as_str(), ())?;
                            }
                            &[]
                        }
                        crate::geometry::SweepSurfaceLayout::LawDriven {
                            first_law,
                            second_law,
                            formula,
                            ..
                        } => {
                            collect_law_curves(first_law, &mut curves, ctx)?;
                            collect_law_curves(second_law, &mut curves, ctx)?;
                            std::slice::from_ref(formula)
                        }
                    };
                    for formula in formulas {
                        ctx.charge_work(1, "carrier reference scan")?;
                        for variable in formula.variables() {
                            ctx.charge_work(1, "carrier reference scan")?;
                            collect_law_curves(variable, &mut curves, ctx)?;
                        }
                    }
                }
            }
            ProceduralSurfaceDefinition::Offset(definition_payload) => {
                let support = definition_payload.support();
                {
                    surfaces.insert_unique(support.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::Replica {
                source: support, ..
            } => {
                surfaces.insert_unique(support.as_str(), ())?;
            }
            ProceduralSurfaceDefinition::Subset(definition_payload) => {
                let support = definition_payload.support();
                {
                    surfaces.insert_unique(support.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::ParallelOffset(definition_payload) => {
                let support = definition_payload.support();
                {
                    surfaces.insert_unique(support.as_str(), ())?;
                }
            }
            ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
                curves.extend_unique([first.as_str(), second.as_str()])?;
            }
            ProceduralSurfaceDefinition::Sum(definition_payload) => {
                curves.extend_unique([
                    definition_payload.first().as_str(),
                    definition_payload.second().as_str(),
                ])?;
            }
            ProceduralSurfaceDefinition::Blend(definition_payload) => {
                let supports = definition_payload.supports();
                let spine = definition_payload.spine();
                let native = definition_payload.native();

                for support in supports.iter().flatten() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    surfaces.insert_unique(support.surface.as_str(), ())?;
                }
                if let Some(spine) = spine {
                    curves.insert_unique(spine.as_str(), ())?;
                }
                if let Some(native) = native {
                    curves.insert_unique(native.slice.as_str(), ())?;
                    for side in &native.sides {
                        ctx.charge_work(1, "carrier reference scan")?;
                        if let Some(curve) = &side.curve {
                            curves.insert_unique(curve.curve.as_str(), ())?;
                        }
                        if let Some(surface) = &side.surface {
                            surfaces.insert_unique(surface.surface.as_str(), ())?;
                        }
                    }
                    if let Some(side) = &native.third {
                        curves.insert_unique(side.curve.as_str(), ())?;
                        surfaces.insert_unique(side.surface.as_str(), ())?;
                    }
                }
            }
            ProceduralSurfaceDefinition::RollingBallJet(_)
            | ProceduralSurfaceDefinition::Helix { .. }
            | ProceduralSurfaceDefinition::TSpline { .. }
            | ProceduralSurfaceDefinition::DegenerateTorus { .. }
            | ProceduralSurfaceDefinition::Unknown { .. } => {}
            ProceduralSurfaceDefinition::CurveBounded {
                support,
                boundaries,
                ..
            } => {
                surfaces.insert_unique(support.as_str(), ())?;
                curves.extend_unique(boundaries.iter().map(super::super::ids::CurveId::as_str))?;
            }
            ProceduralSurfaceDefinition::Deformable(definition_payload) => {
                let construction = definition_payload.construction();

                surfaces.insert_unique(construction.support.as_str(), ())?;
                if let crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    surface, curve, ..
                }
                | crate::geometry::DeformableSurfaceData::Full { surface, curve, .. } =
                    &construction.data
                {
                    surfaces.insert_unique(surface.as_str(), ())?;
                    curves.insert_unique(curve.as_str(), ())?;
                }
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        ctx.charge_work(1, "carrier reference scan")?;
        if let Some(curve) = curve_owners
            .get_unique(ctx, procedural.id.as_str())?
            .copied()
        {
            curves.insert_unique(curve.as_str(), ())?;
        }
        match procedural.definition() {
            ProceduralCurveDefinition::Exact { .. } | ProceduralCurveDefinition::Helix(_) => {}
            ProceduralCurveDefinition::Law {
                context,
                primary,
                additional,
                ..
            } => {
                for side in context.sides() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
                for formula in std::iter::once(primary).chain(additional) {
                    ctx.charge_work(1, "carrier reference scan")?;
                    for variable in formula.formula().variables() {
                        ctx.charge_work(1, "carrier reference scan")?;
                        collect_law_curves(variable, &mut curves, ctx)?;
                    }
                }
            }
            ProceduralCurveDefinition::Compound(compound) => {
                let components = compound.components();

                curves.extend_unique(
                    components
                        .iter()
                        .map(|component| component.component.as_str()),
                )?;
            }
            ProceduralCurveDefinition::Intersection { context, .. } => {
                for side in context.sides() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
            }
            ProceduralCurveDefinition::TolerantIntersection {
                construction: intersection,
                ..
            } => {
                let supports = intersection.supports();

                surfaces
                    .extend_unique(supports.iter().map(super::super::ids::SurfaceId::as_str))?;
            }
            ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) => {
                let context = definition_payload.context();
                let third = definition_payload.third();

                for side in context.sides().iter().chain(std::iter::once(third)) {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceCurve { family } => {
                for side in family.context().sides() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
            }
            ProceduralCurveDefinition::Silhouette(definition_payload) => {
                surfaces.insert_unique(definition_payload.cast_surface().as_str(), ())?;
                for side in definition_payload.context().sides() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
            }
            ProceduralCurveDefinition::SurfaceOffset(definition_payload) => {
                let context = definition_payload.context();
                let base = definition_payload.base();
                {
                    curves.insert_unique(base.as_str(), ())?;
                    for side in context.sides() {
                        ctx.charge_work(1, "carrier reference scan")?;
                        if let Some(surface) = &side.surface {
                            surfaces.insert_unique(surface.as_str(), ())?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Spring(definition_payload) => {
                let layout = definition_payload.layout();
                match layout {
                    crate::geometry::SpringLayout::ContextFirst { supports, .. } => {
                        for support in supports {
                            ctx.charge_work(1, "carrier reference scan")?;
                            if let crate::geometry::SpringSupport::Surface(surface) = support {
                                surfaces.insert_unique(surface.as_str(), ())?;
                            }
                        }
                    }
                    crate::geometry::SpringLayout::CacheFirst { context, .. } => {
                        for side in context.sides() {
                            ctx.charge_work(1, "carrier reference scan")?;
                            if let Some(surface) = &side.surface {
                                surfaces.insert_unique(surface.as_str(), ())?;
                            }
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Deformable(definition_payload) => {
                let context = definition_payload.context();
                let source = definition_payload.source();
                {
                    if let crate::geometry::DeformableCurveSource::Curve { curve } = source {
                        curves.insert_unique(curve.as_str(), ())?;
                    }
                    for side in context.sides() {
                        ctx.charge_work(1, "carrier reference scan")?;
                        if let Some(surface) = &side.surface {
                            surfaces.insert_unique(surface.as_str(), ())?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::Projection(definition_payload) => {
                let context = definition_payload.context();
                let source = definition_payload.source();

                curves.insert_unique(source.as_str(), ())?;
                for side in context.sides() {
                    ctx.charge_work(1, "carrier reference scan")?;
                    if let Some(surface) = &side.surface {
                        surfaces.insert_unique(surface.as_str(), ())?;
                    }
                }
            }
            ProceduralCurveDefinition::Offset(definition_payload) => {
                let source = definition_payload.source();
                let side = definition_payload.side();
                let range = definition_payload.range();
                {
                    curves.insert_unique(source.as_str(), ())?;
                    if let crate::geometry::OffsetSide::Direction {
                        support: Some(support),
                        ..
                    } = side
                    {
                        surfaces.insert_unique(support.as_str(), ())?;
                    }
                    if let Some(crate::geometry::CurveOffsetRange::Variable {
                        distance_law:
                            crate::geometry::CurveOffsetDistanceLaw::Coordinate { function, .. },
                        ..
                    }) = range
                    {
                        curves.insert_unique(function.as_str(), ())?;
                    }
                }
            }
            ProceduralCurveDefinition::SpatialOffset(definition_payload) => {
                let source = definition_payload.source();
                {
                    curves.insert_unique(source.as_str(), ())?;
                }
            }
            ProceduralCurveDefinition::TwoSidedOffset(definition_payload) => {
                let context = definition_payload.context();
                {
                    for side in context.sides() {
                        ctx.charge_work(1, "carrier reference scan")?;
                        if let Some(surface) = &side.surface {
                            surfaces.insert_unique(surface.as_str(), ())?;
                        }
                    }
                }
            }
            ProceduralCurveDefinition::VectorOffset(definition_payload) => {
                let source = definition_payload.source();
                {
                    curves.insert_unique(source.as_str(), ())?;
                }
            }
            ProceduralCurveDefinition::Replica { source, .. } => {
                curves.insert_unique(source.as_str(), ())?;
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let source = definition_payload.source();
                {
                    curves.insert_unique(source.as_str(), ())?;
                }
            }
            ProceduralCurveDefinition::BlendSpine { blend_surface } => {
                if let Some(surface) = blend_surface {
                    surfaces.insert_unique(surface.as_str(), ())?;
                }
            }
            ProceduralCurveDefinition::Unknown { .. } => {}
        }
    }
    // NativeLinks reports malformed link fields. Read each source link here so
    // one malformed record cannot erase reachability from the other records.
    view.visit(
        &crate::index::DecodeStorage(ctx),
        "carrier native arena scan",
        |_, arena, records| -> Result<(), CodecError> {
            ctx.charge_work(
                u64_from_index(arena.len()),
                "carrier native arena comparison",
            )?;
            if arena == "unknowns" {
                for record in
                    records.records(&crate::index::DecodeStorage(ctx), "carrier reference scan")?
                {
                    match record {
                        crate::native::view::NativeEntity::Product(product) => {
                            for link in
                                crate::native::view::NativeEntity::Product(product).links(ctx)?
                            {
                                surfaces.insert_unique(link, ())?;
                                curves.insert_unique(link, ())?;
                            }
                        }
                        crate::native::view::NativeEntity::Source(source) => {
                            for link in
                                ctx.admit_iter(source.links(), "native outgoing link scan")?
                            {
                                surfaces.insert_unique(link.as_str(), ())?;
                                curves.insert_unique(link.as_str(), ())?;
                            }
                        }
                    }
                }
            }
            Ok(())
        },
    )?;
    let composite_segments = BorrowedIdentities::build(ctx, |add| {
        for curve in &ir.model.curves {
            ctx.charge_work(1, "carrier reference scan")?;
            if let CurveGeometry::Solved(SolvedCurveGeometry::Composite { segments, .. }) =
                &curve.geometry
            {
                add(curve.id.as_str(), segments)?;
            }
        }
        Ok(())
    })?;
    let mut queue_storage = ctx.reserve_scoped(0, "carrier traversal queue")?;
    let mut reachable_curves = Vec::new();
    for curve in curves.identities("carrier reference scan")? {
        queue_storage.with_storage(|| {
            ctx.push_vec(&mut reachable_curves, curve, "carrier traversal slots")
        })?;
    }
    let mut next = 0;
    while let Some(curve) = reachable_curves.get(next).copied() {
        ctx.charge_work(1, "carrier traversal pop")?;
        next += 1;
        if let Some(segments) = composite_segments.get(ctx, curve)? {
            for segment in ctx.admit_iter(&segments[..], "carrier reference scan")? {
                let identity = segment.curve.as_str();
                if curves.insert_unique(identity, ())? {
                    queue_storage.with_storage(|| {
                        ctx.push_vec(&mut reachable_curves, identity, "carrier traversal slots")
                    })?;
                }
            }
        }
    }
    macro_rules! check_orphans {
        ($arena:ident, $reachable:ident, $kind:literal) => {
            for entity in &ir.model.$arena {
                ctx.charge_work(1, "carrier orphan row")?;
                let id = entity.id.as_str();
                if !$reachable.contains(ctx, id)? {
                    super::record_finding(
                        ctx,
                        findings,
                        Check::CarrierReachability,
                        Severity::Error,
                        Some(id),
                        format_args!("orphan {} carrier", $kind),
                    )?;
                }
            }
        };
    }
    check_orphans!(surfaces, surfaces, "surface");
    check_orphans!(curves, curves, "curve");
    check_orphans!(pcurves, pcurves, "pcurve");
    check_orphans!(points, points, "point");

    Ok(())
}

pub(super) fn check_parameter_domains(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), cadmpeg_core::CodecError> {
    let curves = crate::index::identities::BorrowedIdentities::build(ctx, |add| {
        for curve in ctx.admit_iter(&ir.model.curves, "parameter-domain curve identity scan")? {
            add(curve.id.as_str(), &curve.geometry)?;
        }
        Ok(())
    })?;
    for edge in &ir.model.edges {
        ctx.charge_work(1, "parameter-domain edge scan")?;
        let Some([start, end]) = edge.param_range().map(crate::units::FiniteVector::get) else {
            continue;
        };
        let mut valid = true;
        let curve = match edge.curve() {
            Some(id) => curves.get(ctx, id.as_str())?.copied(),
            None => None,
        };
        if let Some(curve) = curve {
            let tau = std::f64::consts::TAU;
            match curve {
                CurveGeometry::Solved(
                    SolvedCurveGeometry::Circle(_) | SolvedCurveGeometry::Ellipse(_),
                ) => {
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
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    valid &= crate::eval::nurbs_curve_parameter_domain(nurbs)
                        .is_some_and(|domain| {
                            let [lower, upper] = domain.endpoints();
                            if nurbs.periodic() {
                                let period = upper - lower;
                                let tolerance = EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9.max(
                                    period.abs()
                                        * EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9,
                                );
                                let edge_span = end - start;
                                let allowed = period + tolerance;
                                if edge_span.is_finite() && allowed.is_finite() {
                                    edge_span <= allowed
                                } else if start >= end {
                                    true
                                } else {
                                    crate::topology::IncreasingParameterInterval::new([start, end])
                                        .is_some_and(|edge| {
                                            edge.scaled_span()
                                                .quotient(domain.scaled_span())
                                                .is_ok_and(|ratio| {
                                                    ratio.get()
                                                        <= 1.0
                                                            + EPS_CARRIERS_PARAMETERIZATION_CHECK_PARAMETER_DOMAINS_E9
                                                })
                                        })
                                }
                            } else {
                                parameter_in_domain(start, [lower, upper])
                                    && parameter_in_domain(end, [lower, upper])
                            }
                        });
                }
                _ => {}
            }
        }
        if !valid {
            super::record_finding(
                ctx,
                findings,
                Check::ParameterDomain,
                Severity::Error,
                Some(edge.id.as_str()),
                format_args!("edge parameter range is outside its canonical carrier domain"),
            )?;
        }
    }
    let pcurves = crate::index::identities::BorrowedIdentities::build(ctx, |add| {
        for pcurve in ctx.admit_iter(&ir.model.pcurves, "parameter-domain pcurve identity scan")? {
            add(pcurve.id.as_str(), &pcurve.geometry)?;
        }
        Ok(())
    })?;
    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "parameter-domain coedge scan")?;
        if let Some(use_curve) = &coedge.use_curve {
            let [start, end] = use_curve.parameter_range.endpoints();
            let geometry = curves.get(ctx, use_curve.curve.as_str())?.copied();
            let mut valid = geometry.is_some();
            if let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) = geometry {
                valid &= crate::eval::nurbs_curve_parameter_domain(nurbs)
                    .map(crate::topology::IncreasingParameterInterval::endpoints)
                    .is_some_and(|domain| {
                        parameter_in_domain(start, domain) && parameter_in_domain(end, domain)
                    });
            }
            if !valid {
                super::record_finding(
                    ctx,
                    findings,
                    Check::ParameterDomain,
                    Severity::Error,
                    Some(coedge.id.as_str()),
                    format_args!("coedge use-curve range is outside its carrier domain"),
                )?;
            }
        }
        for use_ in &coedge.pcurves {
            ctx.charge_work(1, "parameter-domain pcurve use scan")?;
            let Some([start, end]) = use_
                .parameter_range
                .map(crate::geometry::DirectedParameterRange::endpoints)
            else {
                continue;
            };
            let geometry = pcurves.get(ctx, use_.pcurve.as_str())?.copied();
            let mut valid = geometry.is_some();
            if let Some(geometry) = geometry {
                let domain = pcurve_parameter_domain(ctx, geometry)?
                    .map(crate::topology::IncreasingParameterInterval::endpoints);
                match domain {
                    Some([lower, upper]) => {
                        valid &= [start, end]
                            .into_iter()
                            .all(|value| parameter_in_domain(value, [lower, upper]));
                    }
                    None if pcurve_requires_bounded_domain(ctx, geometry)? => {
                        valid = false;
                    }
                    None => {}
                }
            }
            if !valid {
                super::record_finding(
                    ctx,
                    findings,
                    Check::ParameterDomain,
                    Severity::Error,
                    Some(coedge.id.as_str()),
                    format_args!("coedge pcurve range is outside its carrier domain"),
                )?;
            }
        }
    }
    Ok(())
}

fn parameter_in_domain(value: f64, domain: [f64; 2]) -> bool {
    crate::math::parameter_in_domain(
        value,
        domain,
        EPS_CARRIERS_PARAMETERIZATION_PARAMETER_IN_DOMAIN_E12,
    )
}

/// Whether a carrier that states no parameter domain is defective.
///
/// [`pcurve_parameter_domain`](super::pcurve_parameter_domain) answers `None`
/// for two different reasons, and only one of them is a defect. An analytic
/// carrier has no bounded domain because it has none to state. A NURBS carrier
/// always has one, so a `None` there means the knot vector cannot yield an
/// evaluable interval and any declared use range over it is unusable.
///
/// The three nesting carriers keep their basis's answer. A placement and an
/// offset keep the basis's parameterization outright. A trim reaches this
/// question only with equal endpoints, where it states no interval of its own
/// and the basis's parameterization governs — the same fallback
/// `pcurve_parameter_domain` takes. So an unusable NURBS is reported under any
/// mixture of wrappers exactly as it is reported bare.
///
/// The match is exhaustive, so a new pcurve variant states its answer here.
/// Each carrier visit enters the caller session and admits its work.
fn pcurve_requires_bounded_domain(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &PcurveGeometry,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let _depth = ctx.enter_nested_limit("pcurve bounded domain nesting")?;
    ctx.charge_work_limit(1, "pcurve bounded domain visit")?;
    Ok(match geometry {
        PcurveGeometry::Nurbs { .. } | PcurveGeometry::PolarNurbs { .. } => true,
        PcurveGeometry::Transformed(placed) => pcurve_requires_bounded_domain(ctx, placed.basis())?,
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            pcurve_requires_bounded_domain(ctx, trimmed_pcurve.basis())?
        }
        PcurveGeometry::Offset(offset_pcurve) => {
            pcurve_requires_bounded_domain(ctx, offset_pcurve.basis())?
        }
        PcurveGeometry::Line(_) => false,
        PcurveGeometry::SphericalGreatCircle(_) => false,
        PcurveGeometry::Circle(_) => false,
        PcurveGeometry::Ellipse(_) => false,
        PcurveGeometry::Harmonic(_) => false,
        PcurveGeometry::Parabola(_) => false,
        PcurveGeometry::Hyperbola(_) => false,
        PcurveGeometry::Hyperbolic(_) => false,
        PcurveGeometry::PolarHarmonic(_) => false,
    })
}

#[cfg(test)]
mod tests;
