use super::{
    curve_parameter_scale, scale_curve_geometry, scale_finite_point3, scale_length,
    scale_positive_length, scale_procedural_curve, scale_procedural_surface,
    scale_surface_geometry, scale_transform_translation, surface_parameter_scales,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{
    AnyConstruction, Construction, ConstructionInput, ConstructionKind, CurveValue, InputReference,
    PostStep, ScalarDimension,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::design::selection::{
    MemberOrder, Members, PickDomain, Picked, Picks, Propagation, Selection, SelectionRule,
};
use cadmpeg_ir::features::datums::DatumAxisDraft;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::scalar::{Length, PositiveReal};

pub(super) fn axis(
    ctx: &DecodeContext<'_>,
    operation: &mut FeatureOperation,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let FeatureOperation::DatumAxis { axis } = std::mem::replace(
        operation,
        FeatureOperation::Unresolved {
            family: cadmpeg_ir::features::UnresolvedFamily::DatumAxis,
        },
    ) else {
        return Err(CodecError::malformed("expected datum axis"));
    };
    let mut draft = DatumAxisDraft::from(axis);
    if let Some(point) = &mut draft.origin {
        scale_finite_point3(point, scale)?;
    }
    inputs(ctx, &mut draft.inputs, scale)?;
    options(ctx, &mut draft.native_options, scale)?;
    options(ctx, &mut draft.source_settings, scale)?;
    *operation = draft.into_operation().map_err(CodecError::malformed)?;
    Ok(())
}
fn dimension(value: &mut Dimension<Length>, scale: PositiveReal) -> Result<(), CodecError> {
    if let Dimension::Literal(value) = value {
        scale_length(value, scale)?;
    }
    Ok(())
}
fn options(
    ctx: &DecodeContext<'_>,
    values: &mut NativeOptions,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    for value in values.values_mut() {
        match value {
            NativeOption::Length(value) => dimension(value, scale)?,
            NativeOption::Selection(value) => selection(ctx, value, scale)?,
            NativeOption::OrderedSelection(value) => selection(ctx, value, scale)?,
            NativeOption::Bool(_)
            | NativeOption::Integer(_)
            | NativeOption::Real(_)
            | NativeOption::Angle(_)
            | NativeOption::Text(_) => {}
        }
    }
    Ok(())
}
fn inputs(
    ctx: &DecodeContext<'_>,
    values: &mut [ConstructionInput],
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("construction length conversion")?;
    for input in values {
        if let Some(value) = &mut input.value {
            match value {
                ScalarDimension::Length(value) => dimension(value, scale)?,
                ScalarDimension::Position { x, y, z } => {
                    dimension(x, scale)?;
                    dimension(y, scale)?;
                    dimension(z, scale)?;
                }
                ScalarDimension::PlaneEquation { d, .. } => dimension(d, scale)?,
                ScalarDimension::Angle(_)
                | ScalarDimension::Ratio(_)
                | ScalarDimension::Uv { .. }
                | ScalarDimension::Components { .. }
                | ScalarDimension::ParameterRatio(_) => {}
            }
        }
        if let Some(reference) = &mut input.reference {
            match reference {
                InputReference::Picks(picks) => selection(ctx, picks, scale)?,
                InputReference::Built(value) => match value.as_mut() {
                    AnyConstruction::Point(value) => {
                        construction(ctx, value, scale, scale_finite_point3)?;
                    }
                    AnyConstruction::Direction(value) => {
                        construction(ctx, value, scale, |_, _| Ok(()))?;
                    }
                    AnyConstruction::Axis(value) => construction(ctx, value, scale, |v, s| {
                        scale_finite_point3(&mut v.origin, s)
                    })?,
                    AnyConstruction::Plane(value) => construction(ctx, value, scale, |v, s| {
                        scale_finite_point3(&mut v.origin, s)
                    })?,
                    AnyConstruction::Frame(value) => construction(ctx, value, scale, |v, s| {
                        scale_finite_point3(&mut v.origin, s)
                    })?,
                    AnyConstruction::Curve(value) => construction(ctx, value, scale, curve_value)?,
                },
            }
        }
    }
    Ok(())
}
fn construction<K: ConstructionKind>(
    ctx: &DecodeContext<'_>,
    value: &mut Construction<K>,
    scale: PositiveReal,
    mut resolved: impl FnMut(&mut K::Value, PositiveReal) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    inputs(ctx, &mut value.inputs, scale)?;
    for step in &mut value.after {
        match step {
            PostStep::Translate { distance, .. } => dimension(distance, scale)?,
            PostStep::Fixed(transform) => scale_transform_translation(transform, scale)?,
            PostStep::Flip(_) | PostStep::Rotate { .. } | PostStep::AxisRotate { .. } => {}
        }
    }
    if let Some(mode) = &mut value.mode {
        options(ctx, &mut mode.native_options, scale)?;
        options(ctx, &mut mode.source_settings, scale)?;
    }
    if let Some(value) = &mut value.resolved {
        resolved(value, scale)?;
    }
    for value in value.resolved_in.values_mut() {
        resolved(value, scale)?;
    }
    Ok(())
}
fn curve_value(value: &mut CurveValue, scale: PositiveReal) -> Result<(), CodecError> {
    match value {
        CurveValue::Segment { start, end } => {
            scale_finite_point3(start, scale)?;
            scale_finite_point3(end, scale)?;
        }
        CurveValue::Arc {
            center,
            radius,
            start,
            end,
            ..
        } => {
            scale_finite_point3(center, scale)?;
            scale_positive_length(radius, scale)?;
            scale_finite_point3(start, scale)?;
            scale_finite_point3(end, scale)?;
        }
        CurveValue::Circle { center, radius, .. } => {
            scale_finite_point3(center, scale)?;
            scale_positive_length(radius, scale)?;
        }
    }
    Ok(())
}
fn members<O: MemberOrder>(
    ctx: &DecodeContext<'_>,
    values: &mut Members<Picked, O>,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let mut items = std::mem::take(values).into_vec();
    for value in &mut items {
        pick(ctx, value, scale)?;
    }
    *values = Members::new(items).map_err(CodecError::malformed)?;
    Ok(())
}
fn selection<O: MemberOrder>(
    ctx: &DecodeContext<'_>,
    value: &mut Selection<O>,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    match &mut value.picks {
        Picks::Unread => {}
        Picks::Named(values) => members(ctx, values, scale)?,
        Picks::Rule {
            terms,
            excluded,
            resolved,
        } => {
            for term in terms.iter_mut().chain(excluded) {
                members(ctx, &mut term.seeds, scale)?;
                match &mut term.rule {
                    SelectionRule::Chain { end: Some(end) } => pick(ctx, end, scale)?,
                    SelectionRule::Region { boundary } => members(ctx, boundary, scale)?,
                    SelectionRule::Intersection { with } => members(ctx, with, scale)?,
                    _ => {}
                }
                if let Some(Propagation::Connected { gap: Some(gap), .. }) = &mut term.propagation {
                    dimension(gap, scale)?;
                }
            }
            if let Some(values) = resolved {
                members(ctx, values, scale)?;
            }
        }
    }
    Ok(())
}
fn pick(
    ctx: &DecodeContext<'_>,
    value: &mut Picked,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("pick length conversion")?;
    match &mut value.domain {
        Some(PickDomain::Containing(point)) => scale_finite_point3(point, scale)?,
        Some(PickDomain::Limits { start, end }) => {
            for value in start.iter_mut().chain(end) {
                pick(ctx, value, scale)?;
            }
        }
        Some(PickDomain::EdgeInterval { .. } | PickDomain::Fragment(_)) | None => {}
    }
    if let Some(w) = &mut value.witness {
        if let Some(point) = &mut w.point {
            scale_finite_point3(point, scale)?;
        }
        if let Some(points) = &mut w.endpoints {
            for point in points {
                scale_finite_point3(point, scale)?;
            }
        }
        if let Some(frame) = &mut w.frame {
            scale_finite_point3(&mut frame.origin, scale)?;
        }
        for curve in &mut w.curves {
            scale_curve_geometry(ctx, curve, scale)?;
        }
        if let Some(surface) = &mut w.surface {
            scale_surface_geometry(ctx, surface, scale)?;
        }
        if let Some(body) = &mut w.body {
            topology(ctx, body, scale)?;
        }
    }
    Ok(())
}
fn topology(
    ctx: &DecodeContext<'_>,
    value: &mut cadmpeg_ir::document::Topology,
    scale: PositiveReal,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
    use std::collections::HashMap;
    let mut curve_scales = HashMap::new();
    for curve in &value.curves {
        if let Some(geometry) = curve.geometry.solved() {
            if let Some(s) = curve_parameter_scale(ctx, geometry, scale)? {
                curve_scales.insert(curve.id.clone(), s);
            }
        }
    }
    let mut chart_scales = HashMap::new();
    for surface in &value.surfaces {
        if let Some(geometry) = surface.geometry.solved() {
            chart_scales.insert(
                surface.id.clone(),
                surface_parameter_scales(ctx, geometry, scale.get())?,
            );
        }
    }
    let faces: HashMap<_, _> = value.faces.iter().map(|v| (&v.id, &v.surface)).collect();
    let loops: HashMap<_, _> = value.loops.iter().map(|v| (&v.id, &v.face)).collect();
    let mut pcurve_scales = HashMap::new();
    let mut assign = |id: &cadmpeg_ir::ids::PcurveId, chart: Option<[f64; 2]>| {
        let chart = chart
            .ok_or_else(|| CodecError::malformed("witness pcurve has no solved support chart"))?;
        if pcurve_scales
            .insert(id.clone(), chart)
            .is_some_and(|old| old != chart)
        {
            return Err(CodecError::malformed(
                "witness pcurve has incompatible support charts",
            ));
        }
        Ok(())
    };
    for coedge in &value.coedges {
        let chart = loops
            .get(&coedge.owner_loop)
            .and_then(|id| faces.get(id))
            .and_then(|id| chart_scales.get(id))
            .copied();
        for usage in &coedge.pcurves {
            assign(&usage.pcurve, chart)?;
        }
    }
    for loop_ in &value.loops {
        let chart = faces
            .get(&loop_.face)
            .and_then(|id| chart_scales.get(id))
            .copied();
        for usage in loop_.vertex_pcurves() {
            assign(&usage.pcurve, chart)?;
        }
    }
    for surface in &value.procedural_surfaces {
        if let cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition::CurveBounded {
            support,
            boundary_pcurves,
            ..
        } = surface.definition()
        {
            let chart = chart_scales.get(support).copied();
            for pcurve in boundary_pcurves {
                assign(pcurve, chart)?;
            }
        }
    }
    if value
        .pcurves
        .iter()
        .any(|pcurve| !pcurve_scales.contains_key(&pcurve.id))
    {
        return Err(CodecError::malformed("witness pcurve has no support chart"));
    }
    for point in &mut value.points {
        let mut position = point.position();
        scale_finite_point3(&mut position, scale)?;
        point.set_position(position);
    }
    for body in &mut value.bodies {
        if let Some(transform) = &mut body.transform {
            scale_transform_translation(transform, scale)?;
        }
    }
    for face in &mut value.faces {
        tolerance(&mut face.tolerance, scale)?;
    }
    for vertex in &mut value.vertices {
        tolerance(&mut vertex.tolerance, scale)?;
    }
    for edge in &mut value.edges {
        tolerance(&mut edge.tolerance, scale)?;
        if let Some(s) = edge.curve().and_then(|id| curve_scales.get(id)) {
            if let Some(range) = edge.param_range() {
                edge.set_param_range(Some(
                    cadmpeg_ir::topology::ParameterInterval::new(range.get())
                        .map_err(CodecError::malformed)?
                        .scaled(*s)
                        .ok_or_else(|| {
                            CodecError::malformed("scaled witness edge interval overflow")
                        })?,
                ));
            }
        }
    }
    for coedge in &mut value.coedges {
        if let Some(usage) = &mut coedge.use_curve {
            if let Some(s) = curve_scales.get(&usage.curve) {
                usage.parameter_range = usage.parameter_range.scaled(*s).ok_or_else(|| {
                    CodecError::malformed("scaled witness coedge interval overflow")
                })?;
            }
        }
    }
    for curve in &mut value.curves {
        if let Some(s) = curve_scales.get(&curve.id) {
            if let Some(range) = curve.parameter_range {
                curve.parameter_range = Some(
                    cadmpeg_ir::topology::IncreasingParameterInterval::new(
                        range.endpoints().map(|v| v * s.get()),
                    )
                    .ok_or_else(|| {
                        CodecError::malformed("scaled witness carrier interval overflow")
                    })?,
                );
            }
        }
        match &mut curve.geometry {
            CurveGeometry::Solved(g) => scale_curve_geometry(ctx, g, scale)?,
            CurveGeometry::Procedural { cache, .. } => {
                if let Some(g) = cache {
                    scale_curve_geometry(ctx, g, scale)?;
                }
            }
        }
    }
    for surface in &mut value.surfaces {
        match &mut surface.geometry {
            SurfaceGeometry::Solved(g) => scale_surface_geometry(ctx, g, scale)?,
            SurfaceGeometry::Procedural { cache, .. } => {
                if let Some(g) = cache {
                    scale_surface_geometry(ctx, g, scale)?;
                }
            }
        }
    }
    for pcurve in &mut value.pcurves {
        let scales = pcurve_scales
            .get(&pcurve.id)
            .ok_or_else(|| CodecError::malformed("witness pcurve has no support chart"))?;
        let geometry = std::mem::replace(
            &mut pcurve.geometry,
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::U_AXIS,
            ),
        );
        pcurve.geometry = geometry
            .scaled_coordinates_owned(ctx, *scales)?
            .map_err(CodecError::malformed)?;
        pcurve
            .metadata
            .scale_chart(*scales)
            .map_err(CodecError::malformed)?;
    }
    for procedural in &mut value.procedural_surfaces {
        scale_procedural_surface(procedural, scale)?;
    }
    for procedural in &mut value.procedural_curves {
        scale_procedural_curve(procedural, scale)?;
    }
    for subd in &mut value.subds {
        subd.cage
            .scale_lengths(scale)
            .map_err(|e| CodecError::malformed(e.to_string()))?;
    }
    Ok(())
}
fn tolerance(value: &mut Option<PositiveReal>, scale: PositiveReal) -> Result<(), CodecError> {
    if let Some(v) = value {
        *v = PositiveReal::new(v.get() * scale.get())
            .ok_or_else(|| CodecError::malformed("scaled witness tolerance overflow"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    mod witness_charts;
    use super::super::scale_feature_definition;
    use super::*;
    use cadmpeg_ir::design::construction::{kinds, FrameAxis};
    use cadmpeg_ir::design::selection::{Anchor, PickTarget, PrincipalEntity, Witness};
    use cadmpeg_ir::features::geometry::{FeatureDirection3, FinitePoint3};
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    #[test]
    fn axis_conversion_reaches_nested_values_options_and_steps() {
        let p = || FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("finite test point");
        let length = || Dimension::Literal(Length::new(2.0).expect("positive test length"));
        let bound: cadmpeg_ir::features::ParameterId = "test:design:parameter#length"
            .try_into()
            .expect("test parameter identity");
        let mut draft = DatumAxisDraft {
            origin: Some(p()),
            direction: Some(FeatureDirection3::from(
                cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(0.0, 0.0, 1.0))
                    .expect("nonzero test direction"),
            )),
            ..Default::default()
        };
        let mut nested = Construction::<kinds::Point> {
            resolved: Some(p()),
            after: vec![PostStep::Translate {
                axis: FrameAxis::X,
                distance: length(),
            }],
            ..Default::default()
        };
        nested.resolved_in.insert(
            "test:design:configuration#other"
                .try_into()
                .expect("test configuration identity"),
            p(),
        );
        draft.inputs.push(ConstructionInput {
            role: None,
            reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
                nested,
            )))),
            value: Some(ScalarDimension::Position {
                x: length(),
                y: Dimension::Parameter(bound.clone()),
                z: length(),
            }),
            reversed: None,
        });
        draft.native_options.insert(
            cadmpeg_core::nonblank_literal!("f3d:distance"),
            NativeOption::Length(length()),
        );
        draft.source_settings.insert(
            cadmpeg_core::nonblank_literal!("f3d:bound"),
            NativeOption::Length(Dimension::Parameter(bound.clone())),
        );
        let mut definition = FeatureDefinition::Operation(
            draft
                .into_operation()
                .expect("valid axis conversion fixture"),
        );
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        scale_feature_definition(
            &ctx,
            &mut definition,
            PositiveReal::new(10.0).expect("positive scale"),
        )
        .expect("axis length conversion");
        let FeatureOperation::DatumAxis { axis } = definition.operation() else {
            panic!("datum axis")
        };
        assert_eq!(
            axis.origin().expect("known axis origin").get(),
            Point3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(
            axis.native_options()[&cadmpeg_core::nonblank_literal!("f3d:distance")],
            NativeOption::Length(Dimension::Literal(
                Length::new(20.0).expect("expected scaled length")
            ))
        );
        assert_eq!(
            axis.source_settings()[&cadmpeg_core::nonblank_literal!("f3d:bound")],
            NativeOption::Length(Dimension::Parameter(bound))
        );
        let Some(InputReference::Built(value)) = &axis.inputs()[0].reference else {
            panic!("nested point")
        };
        let AnyConstruction::Point(point) = value.as_ref() else {
            panic!("point")
        };
        assert_eq!(
            point.resolved.expect("known nested point").get(),
            Point3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(
            point
                .resolved_in
                .values()
                .next()
                .expect("configuration point")
                .get(),
            Point3::new(10.0, 20.0, 30.0)
        );
        assert!(
            matches!(&point.after[0],PostStep::Translate{distance:Dimension::Literal(v),..} if v.get()==20.0)
        );
    }

    #[test]
    fn axis_conversion_reaches_witness_topology_and_chart_metadata() {
        use cadmpeg_ir::geometry::legacy_cache::FitTolerance;
        use cadmpeg_ir::geometry::pcurve::{
            LinePcurve, Pcurve, PcurveGeneralForm, PcurveGeometry, PcurveMetadata,
        };
        use cadmpeg_ir::units::FiniteVector;
        let mut body = cadmpeg_ir::examples::unit_cube()
            .expect("cube witness")
            .geometry;
        body.subds = cadmpeg_ir::examples::directed_subd_sum()
            .expect("subdivision witness")
            .geometry
            .subds;
        let before_points = body
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>();
        let before_subd = serde_json::to_value(&body.subds[0].cage).expect("serialize source cage");
        let pcurve: cadmpeg_ir::ids::PcurveId = "test:model:pcurve#witness"
            .try_into()
            .expect("test pcurve identity");
        let mut form = PcurveGeneralForm::new(
            Some(false),
            Some(FiniteVector::new([2.0, 3.0]).expect("finite UV interval")),
            Some(FitTolerance::try_new(0.25).expect("positive fit tolerance")),
        );
        form.uv_offset = Some(FiniteVector::new([3.0, 4.0]).expect("finite chart offset"));
        body.pcurves.push(Pcurve {
            id: pcurve.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0))
                    .expect("finite pcurve line"),
            ),
            metadata: PcurveMetadata::General { form },
            native_ref: None,
        });
        body.coedges[0]
            .pcurves
            .push(cadmpeg_ir::topology::PcurveUse {
                pcurve,
                isoparametric: None,
                parameter_range: None,
            });
        let picked = Picked {
            target: PickTarget::Principal {
                entity: PrincipalEntity::Origin,
                container: None,
            },
            context: None,
            native: None,
            domain: None,
            resolved: None,
            witness: Some(Witness {
                point: None,
                direction: None,
                endpoints: None,
                frame: None,
                curves: Vec::new(),
                surface: None,
                body: Some(Box::new(body)),
            }),
        };
        let selection = Selection {
            at: Anchor::Input,
            picks: Picks::Named(Members::new(vec![picked]).expect("distinct witness selection")),
            native: None,
        };
        let mut draft = DatumAxisDraft::default();
        draft.source_settings.insert(
            cadmpeg_core::nonblank_literal!("test:witness"),
            NativeOption::Selection(selection),
        );
        let mut definition = FeatureDefinition::Operation(
            draft
                .into_operation()
                .expect("valid axis conversion fixture"),
        );
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        scale_feature_definition(
            &ctx,
            &mut definition,
            PositiveReal::new(10.0).expect("positive scale"),
        )
        .expect("witness length conversion");
        let FeatureOperation::DatumAxis { axis } = definition.operation() else {
            panic!("datum axis")
        };
        let NativeOption::Selection(selection) =
            &axis.source_settings()[&cadmpeg_core::nonblank_literal!("test:witness")]
        else {
            panic!("selection")
        };
        let Picks::Named(picks) = &selection.picks else {
            panic!("picks")
        };
        let body = picks.as_slice()[0]
            .witness
            .as_ref()
            .expect("pick witness")
            .body
            .as_ref()
            .expect("closed witness topology");
        for (before, after) in before_points.iter().zip(&body.points) {
            assert_eq!(
                after.position().get(),
                Point3::new(before.x * 10.0, before.y * 10.0, before.z * 10.0)
            );
        }
        assert_eq!(
            body.edges[0]
                .param_range()
                .expect("line parameter interval")
                .get(),
            [0.0, 100.0]
        );
        let pcurve = &body.pcurves[0];
        let PcurveGeometry::Line(line) = &pcurve.geometry else {
            panic!("line")
        };
        assert_eq!(line.origin().get(), Point2::new(10.0, 20.0));
        assert_eq!(*line.direction().as_raw(), Point2::new(30.0, 40.0));
        assert_eq!(
            pcurve
                .metadata
                .uv_offset()
                .expect("scaled chart offset")
                .get(),
            [30.0, 40.0]
        );
        assert_eq!(
            pcurve
                .metadata
                .fit_tolerance()
                .expect("scaled fit tolerance")
                .get(),
            2.5
        );
        assert_eq!(
            pcurve
                .metadata
                .parameter_range()
                .expect("native parameter interval")
                .get(),
            [2.0, 3.0]
        );
        assert_eq!(pcurve.metadata.wrapper_reversed(), Some(false));
        let after_subd = serde_json::to_value(&body.subds[0].cage).expect("serialize scaled cage");
        for (before, after) in before_subd["vertices"]
            .as_array()
            .expect("source cage vertices")
            .iter()
            .zip(
                after_subd["vertices"]
                    .as_array()
                    .expect("scaled cage vertices"),
            )
        {
            for coordinate in ["x", "y", "z"] {
                assert_eq!(
                    after["point"][coordinate]
                        .as_f64()
                        .expect("scaled cage coordinate"),
                    before["point"][coordinate]
                        .as_f64()
                        .expect("source cage coordinate")
                        * 10.0
                );
            }
        }
    }
}
