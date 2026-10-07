// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP geometry reader.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn uncertainty_note_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        ctx.format_retained(format_args!("ambiguous uncertainty values ({})", "0.1, 0.2"), "step_uncertainty_note_text"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_uncertainty_note_text"
    ));
}

fn pcurve_geometry_refusal(collection_limit: u64, depth_limit: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid curve record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    let mut workspace = super::super::PcurveWorkspace {
        records: BTreeSet::new(),
        storage: ctx
            .reserve_scoped(0, "test pcurve workspace")
            .expect("empty scope"),
    };
    super::super::decode_pcurve_geometry(
        1,
        &exchange,
        super::super::PcurveSources {
            points: &BTreeMap::new(),
            vectors: &BTreeMap::new(),
            placements: &BTreeMap::new(),
            transformations: &BTreeMap::new(),
            angle_scale: 1.0,
        },
        &mut Vec::new(),
        &mut super::super::PcurveWalk {
            active: &mut BTreeSet::new(),
            workspace: &mut workspace,
        },
        0,
        &ctx,
    )
    .expect_err("pcurve geometry exceeds the limit")
}

#[test]
fn pcurve_geometry_active_refuses_collection_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(0, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_pcurve_geometry_active")
    );
}

#[test]
fn pcurve_source_records_refuse_collection_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(1, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_pcurve_source_records")
    );
}

#[test]
fn pcurve_geometry_walk_refuses_depth_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(2, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_pcurve_geometry_walk")
    );
}

fn surface_scale_refusal(
    collection_limit: u64,
    retained_limit: u64,
    depth_limit: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_materialized_bytes = retained_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let ir = cadmpeg_ir::document::CadIr::empty();
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#1").expect("valid identity");
    let (index, _workspace) =
        super::super::SurfaceScaleIndex::build(&ir, &ctx).expect("empty model index");
    super::super::procedural_surface_parameter_scales(
        &ir,
        &index,
        &id,
        &cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
        ),
        [1.0, 1.0],
        &BTreeMap::new(),
        &ctx,
    )
    .expect_err("surface scale exceeds limit")
}

#[test]
fn surface_scale_active_refuses_collection_limit() {
    assert!(
        matches!(surface_scale_refusal(0, u64::MAX, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_surface_scale_active")
    );
}

#[test]
fn surface_parameter_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(surface_scale_refusal(128, u64::MAX, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_surface_parameter_scale_walk")
    );
}

#[test]
fn surface_geometry_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(surface_scale_refusal(128, u64::MAX, 1), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_surface_geometry_scale_walk")
    );
}

#[test]
fn directrix_geometry_scale_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::directrix_geometry_parameter_scale(
            &cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
            1.0, 1.0, &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_directrix_geometry_scale_walk"
    ));
}

macro_rules! association_name_refusal_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 4;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits policy");
            assert!(matches!(
                super::super::attach_geometry_source(&mut None, 1, Some("named"), &ctx, $operation),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == $operation
            ));
        }
    };
}

association_name_refusal_test!(
    geometric_set_association_name_copy_refuses_retained_limit,
    "step_geometric_set_association_name_copy"
);
association_name_refusal_test!(
    representation_association_name_copy_refuses_retained_limit,
    "step_representation_association_name_copy"
);
association_name_refusal_test!(
    presentation_association_name_copy_refuses_retained_limit,
    "step_presentation_association_name_copy"
);

fn line_scale_refusal(source: &[u8], collection_limit: u64, depth_limit: u64) -> CodecError {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid curve record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::line_parameter_scale(
        &exchange,
        1,
        cadmpeg_ir::scalar::PositiveReal::new(1.0).expect("positive scale"),
        &mut Vec::new(),
        &ctx,
    )
    .expect_err("line scale exceeds limit")
}

#[test]
fn line_parameter_scale_active_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 0, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_line_parameter_scale_active")
    );
}

#[test]
fn line_parameter_scale_walk_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 1, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_line_parameter_scale_walk")
    );
}

#[test]
fn line_parameter_scale_losses_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=LINE('',#2,#3);#2=CARTESIAN_POINT('',(0.,0.,0.));#3=VECTOR('',#4,-1.);#4=DIRECTION('',(1.,0.,0.));ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 1, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_line_parameter_scale_losses")
    );
}

fn trim_fallback_refusal(master: super::super::TrimMasterRepresentation) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let points = BTreeMap::new();
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
    );
    let mut losses = Vec::new();
    let mut context = super::super::TrimParameterContext {
        points: &points,
        geometry: &geometry,
        angle_scale: 1.0,
        linear_parameter_scale: 1.0,
        parameter_offset: 0.0,
        tolerance: 1.0,
        master_representation: master,
        record_id: 1,
        losses: &mut losses,
        ctx: &ctx,
    };
    let parameter = crate::parse::Value::Integer(1);
    let cartesian = crate::parse::Value::Reference(2);
    let (parameter, cartesian) = match master {
        super::super::TrimMasterRepresentation::Parameter => (None, Some(&cartesian)),
        super::super::TrimMasterRepresentation::Cartesian => (Some(&parameter), None),
        super::super::TrimMasterRepresentation::Unspecified => (None, None),
    };
    super::super::select_trim_parameter(parameter, cartesian, &mut context)
        .expect_err("fallback loss exceeds limit")
}

#[test]
fn parameter_trim_fallback_loss_refuses_collection_limit() {
    assert!(
        matches!(trim_fallback_refusal(super::super::TrimMasterRepresentation::Parameter),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_trim_parameter_fallback_losses")
    );
}

#[test]
fn cartesian_trim_fallback_loss_refuses_collection_limit() {
    assert!(
        matches!(trim_fallback_refusal(super::super::TrimMasterRepresentation::Cartesian),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_trim_parameter_fallback_losses")
    );
}

#[test]
fn trim_parameter_scale_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(super::super::parameter_scale(
        &cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None }, 1.0, 1.0, &ctx,
    ), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_trim_parameter_scale_walk"));
}

#[test]
fn trim_parameter_value_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let points = BTreeMap::new();
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
    );
    let mut losses = Vec::new();
    let context = super::super::TrimParameterContext {
        points: &points,
        geometry: &geometry,
        angle_scale: 1.0,
        linear_parameter_scale: 1.0,
        parameter_offset: 0.0,
        tolerance: 1.0,
        master_representation: super::super::TrimMasterRepresentation::Parameter,
        record_id: 1,
        losses: &mut losses,
        ctx: &ctx,
    };
    assert!(matches!(
        super::super::trim_parameter_value(&crate::parse::Value::Integer(1), &context),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_trim_parameter_value_walk"
    ));
}

fn deferred_dependency_refusal(
    limit: u64,
    group: &'static str,
    member: &'static str,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    ctx.push_hash_group(&mut HashMap::new(), 1, 2, group, member)
        .expect_err("dependency exceeds the limit")
}

#[test]
fn deferred_curve_groups_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(0, "step_deferred_curve_groups", "step_deferred_curve_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_groups")
    );
}

#[test]
fn deferred_curve_members_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(1, "step_deferred_curve_groups", "step_deferred_curve_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_members")
    );
}

#[test]
fn deferred_surface_groups_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(0, "step_deferred_surface_groups", "step_deferred_surface_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_groups")
    );
}

#[test]
fn deferred_surface_members_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(1, "step_deferred_surface_groups", "step_deferred_surface_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_members")
    );
}

fn deferred_wake_refusal(operation: &'static str) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut waiting = HashMap::from([(1, vec![2])]);
    super::super::wake_deferred_dependents(1, &mut waiting, &mut VecDeque::new(), &ctx, operation)
        .expect_err("wake queue exceeds the limit")
}

#[test]
fn deferred_curve_queue_refuses_collection_limit() {
    assert!(matches!(deferred_wake_refusal("step_deferred_curve_queue"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_queue"));
}

#[test]
fn deferred_surface_queue_refuses_collection_limit() {
    assert!(
        matches!(deferred_wake_refusal("step_deferred_surface_queue"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_queue")
    );
}

#[test]
fn curve_coordinate_rows_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.)));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid coordinate list");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(matches!(
        super::super::coordinate_rows(&exchange.records()[&1], 1.0, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_curve_coordinate_rows"
    ));
}

fn strip_refusal(collection_limit: u64) -> CodecError {
    use crate::parse::Value;

    let strips = Value::List(vec![Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
    ])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    super::super::tessellated_line_strips(Some(&strips), 2, &ctx)
        .expect_err("strip exceeds collection limit")
}

#[test]
fn curve_strip_indices_refuse_collection_limit() {
    assert!(
        matches!(strip_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_curve_strip_indices")
    );
}

#[test]
fn curve_strips_refuse_collection_limit() {
    assert!(
        matches!(strip_refusal(2), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_curve_strips")
    );
}

#[test]
fn curve_strip_source_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        ctx.format_retained(format_args!("{}", "curve"), "step_curve_strip_source_name"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_curve_strip_source_name"
    ));
}

#[test]
fn transformation_operator_partial_refusal_stays_error() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',$,$,#3,1.,$);#2=CARTESIAN_TRANSFORMATION_OPERATOR_2D('',$,$,#3,1.);#3=DUMMY();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid transformation operator records");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    for dimension in [3, 2] {
        crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
            let error = if dimension == 3 {
                super::super::cartesian_transformation_operator(
                    ctx,
                    exchange.records().get(&1).expect("3D operator"),
                    &std::collections::BTreeMap::new(),
                    &std::collections::BTreeMap::new(),
                )
                .expect_err("3D partial scan exceeds work")
            } else {
                super::super::cartesian_transformation_operator_2d(
                    ctx,
                    exchange.records().get(&2).expect("2D operator"),
                    &std::collections::BTreeMap::new(),
                    &std::collections::BTreeMap::new(),
                )
                .expect_err("2D partial scan exceeds work")
            };
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "STEP transformation attribute partial traversal" && Some(limit) == ctx.resource_refusal())
            );
        });
    }
}

#[test]
fn nested_trim_value_walks_the_geometry_scale_once() {
    use cadmpeg_ir::geometry::{CurveGeometry, PlacedCurve, SolvedCurveGeometry};
    let mut geometry = SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("line"),
    );
    for _ in 0..4 {
        geometry = SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(
                Box::new(geometry),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .expect("placed line"),
        );
    }
    let geometry = CurveGeometry::Solved(geometry);
    let mut value = crate::parse::Value::Integer(2);
    for _ in 0..8 {
        value = crate::parse::Value::Typed("PARAMETER_VALUE".into(), Box::new(value));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Eight typed-value steps plus four basis steps; the geometry scale is computed once.
    policy.limits.max_work_units = 8 + 4;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut losses = Vec::new();
    let context = super::super::TrimParameterContext {
        points: &BTreeMap::new(),
        geometry: &geometry,
        angle_scale: 1.0,
        linear_parameter_scale: 7.0,
        parameter_offset: 3.0,
        tolerance: 1.0,
        master_representation: super::super::TrimMasterRepresentation::Parameter,
        record_id: 1,
        losses: &mut losses,
        ctx: &ctx,
    };
    assert_eq!(
        super::super::trim_parameter_value(&value, &context).expect("each walk fits the budget"),
        Some(17.0)
    );
}

#[test]
fn transformed_curve_parameter_walks_refuse_work_limits() {
    use cadmpeg_ir::geometry::{PlacedCurve, SolvedCurveGeometry};
    let mut geometry = SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("line"),
    );
    for _ in 0..4 {
        geometry = SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(
                Box::new(geometry),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .expect("placed line"),
        );
    }
    for operation in [
        "step_trim_parameter_scale_walk",
        "step_directrix_geometry_scale_walk",
        "step curve point parameter basis walk",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                    .expect("empty root fits policy");
                match operation {
                    "step_trim_parameter_scale_walk" => {
                        super::super::parameter_scale(&geometry, 2.0, 7.0, &ctx).map(Some)
                    }
                    "step_directrix_geometry_scale_walk" => {
                        super::super::directrix_geometry_parameter_scale(&geometry, 7.0, 2.0, &ctx)
                    }
                    _ => super::super::curve_parameter_at_point(
                        &ctx,
                        &geometry,
                        cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                        1.0,
                    ),
                }
            },
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == operation)
        );
    }
}

#[test]
fn composite_segment_scratch_is_scoped_and_released() {
    use cadmpeg_ir::geometry::CompositeCurveTransition;

    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=LINE('',#10,#11);#2=COMPOSITE_CURVE_SEGMENT(.CONTINUOUS.,.T.,#1);#3=COMPOSITE_CURVE('',(#2),.F.);#10=CARTESIAN_POINT('',(0.,0.,0.));#11=VECTOR('',#12,1.);#12=DIRECTION('',(1.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid composite records");
    let decoded = super::super::CarrierIndex {
        curves: HashMap::from([(1, super::super::CurveIndex(0))]),
        points: HashMap::new(),
        surfaces: HashMap::new(),
    };
    let run = |cap| -> Result<(), CodecError> {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
        let ((segments, self_intersect), storage) =
            super::super::composite_curve(&exchange.records()[&3], &exchange, &decoded, &ctx)?
                .expect("composite segments");
        assert_eq!(self_intersect, Some(false));
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].0, 2);
        assert_eq!(segments[0].1.curve.as_str(), "step:data:curve#1");
        assert!(segments[0].1.same_sense);
        assert_eq!(
            segments[0].1.transition,
            CompositeCurveTransition::Continuous
        );
        drop(segments);
        drop(storage);
        let _released = ctx.reserve_scoped(cap, "test composite scratch released")?;
        Ok(())
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_composite_curve_segments",
        run,
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("expected a composite storage refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(refusal.operation, "step_composite_curve_segments");
    // The first segment admits the vector capacity; releasing it frees the whole allowance.
    run(refusal.used + refusal.additional)
        .expect("scratch needs no retained bytes and releases its reservation");
}
