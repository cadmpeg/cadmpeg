// SPDX-License-Identifier: Apache-2.0
//! STEP geometry, pcurve, NURBS, unit, replica, and trim tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::CadIr;

use crate::export::Builder;
use crate::test_support::exchange::{decode_inline, export};
use crate::StepSchema;

#[test]
fn explicit_knot_expansion_retains_admitted_bits_and_refusal() {
    use crate::parse::Value;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let counts = Value::List(vec![Value::Integer(2), Value::Integer(1)]);
    let values = Value::List(vec![
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(-0.0).expect("finite fixture")),
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(0.5).expect("finite fixture")),
    ]);
    let knots: cadmpeg_ir::geometry::nurbs::KnotVector =
        super::super::expand_knots(&counts, &values, 3, &ctx)
            .expect("no resource refusal")
            .expect("finite ordered knots");
    assert_eq!(knots.as_slice().len(), 3);
    assert_eq!(knots.as_slice()[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(knots.as_slice()[1].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(knots.as_slice()[2].to_bits(), 0.5_f64.to_bits());

    assert!(cadmpeg_ir::scalar::FiniteReal::new(f64::NAN).is_none());
    let decreasing = Value::List(vec![
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite fixture")),
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(0.5).expect("finite fixture")),
    ]);
    assert!(super::super::expand_knots(&counts, &decreasing, 3, &ctx)
        .expect("no resource refusal")
        .is_none());
}

#[test]
fn explicit_knot_expansion_refuses_caller_collection_limit() {
    use crate::parse::Value;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let counts = Value::List(vec![Value::Integer(2), Value::Integer(1)]);
    let values = Value::List(vec![
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(0.0).expect("finite fixture")),
        Value::Real(cadmpeg_ir::scalar::FiniteReal::new(0.5).expect("finite fixture")),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::expand_knots(&counts, &values, 3, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_expanded_nurbs_knots"
    ));
}

#[test]
fn defaulted_spline_curve_subtypes_derive_knot_vectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=QUASI_UNIFORM_CURVE('quasi',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#5=UNIFORM_CURVE('uniform',1,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#6=BEZIER_CURVE('bezier',2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.);
#7=(BOUNDED_CURVE() B_SPLINE_CURVE(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.) QUASI_UNIFORM_CURVE() RATIONAL_B_SPLINE_CURVE((1.,.5,1.)) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('rational'));
#8=GEOMETRIC_SET('',(#4,#5,#6,#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let nurbs = |id: &str| {
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == id)
            .and_then(|curve| match curve.geometry.solved() {
                Some(SolvedCurveGeometry::Nurbs(nurbs)) => Some(nurbs),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing NURBS curve {id}"))
    };
    assert_eq!(
        nurbs("step:data:curve#4").knots().as_slice(),
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(
        nurbs("step:data:curve#5").knots().as_slice(),
        [-1.0, 0.0, 1.0, 2.0, 3.0]
    );
    assert_eq!(
        nurbs("step:data:curve#6").knots().as_slice(),
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    let rational = nurbs("step:data:curve#7");
    assert_eq!(rational.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(rational.pole_rows().weights(), Some(vec![1.0, 0.5, 1.0]));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn defaulted_spline_surface_subtypes_derive_axis_knot_vectors() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#4=CARTESIAN_POINT('',(0.,1.,0.));
#5=CARTESIAN_POINT('',(1.,1.,0.));
#6=CARTESIAN_POINT('',(2.,1.,0.));
#10=QUASI_UNIFORM_SURFACE('quasi',1,1,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#11=UNIFORM_SURFACE('uniform',1,2,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#12=BEZIER_SURFACE('bezier',1,2,((#1,#2,#3),(#4,#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.);
#13=GEOMETRIC_SET('',(#10,#11,#12));
#14=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#13),#15);
#15=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    let nurbs = |id: &str| {
        result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == id)
            .and_then(|surface| match surface.geometry.solved() {
                Some(SolvedSurfaceGeometry::Nurbs(nurbs)) => Some(nurbs),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing NURBS surface {id}"))
    };
    assert_eq!(
        nurbs("step:data:surface#10").u_knots().as_slice(),
        [0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        nurbs("step:data:surface#10").v_knots().as_slice(),
        [0.0, 0.0, 1.0, 2.0, 2.0]
    );
    assert_eq!(
        nurbs("step:data:surface#11").u_knots().as_slice(),
        [-1.0, 0.0, 1.0, 2.0]
    );
    assert_eq!(
        nurbs("step:data:surface#11").v_knots().as_slice(),
        [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0]
    );
    assert_eq!(
        nurbs("step:data:surface#12").u_knots().as_slice(),
        [0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        nurbs("step:data:surface#12").v_knots().as_slice(),
        [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn complex_rational_quasi_uniform_surface_decodes_with_weight_grid() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=CARTESIAN_POINT('',(0.,1.,0.));
#4=CARTESIAN_POINT('',(1.,1.,0.));
#5=CARTESIAN_POINT('',(0.,2.,0.));
#6=CARTESIAN_POINT('',(1.,2.,0.));
#7=(BOUNDED_SURFACE() B_SPLINE_SURFACE(2,1,((#1,#2),(#3,#4),(#5,#6)),.UNSPECIFIED.,.F.,.F.,.F.) QUASI_UNIFORM_SURFACE() RATIONAL_B_SPLINE_SURFACE(((1.,.5),(1.,.5),(1.,1.))) SURFACE());
#8=GEOMETRIC_SET('',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "step:data:surface#7")
        .expect("complex rational surface");
    let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = surface.geometry.solved() else {
        panic!("complex rational surface is not NURBS")
    };
    assert_eq!(nurbs.u_knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(nurbs.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        nurbs.pole_grid().weights().map(|rows| rows.concat()),
        Some(vec![1.0, 0.5, 1.0, 0.5, 1.0, 1.0])
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn excessive_nurbs_degree_is_rejected_before_knot_allocation() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,0.,0.));
#3=B_SPLINE_CURVE_WITH_KNOTS('',4294967295,(#1,#2),.UNSPECIFIED.,.F.,.F.,(4294967298),(0.),.UNSPECIFIED.);",
    );
    assert!(result.ir().model.curves.is_empty());
}

#[test]
fn deferred_curve_dependencies_resolve_independent_of_record_order() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(1.,0.,0.));
#3=VECTOR('',#2,1.);
#4=LINE('',#1,#3);
#5=OFFSET_CURVE_3D('',#7,1.,.F.,#2);
#6=GEOMETRIC_SET('',(#5));
#7=OFFSET_CURVE_3D('',#4,2.,.F.,#2);
#8=SHAPE_REPRESENTATION('',(#6),#9);
#9=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#5"));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "step:data:curve#7"));
    assert!(result.report().losses.iter().all(|loss| {
        !loss
            .message
            .contains("OFFSET_CURVE_3D #5 has no decoded basis curve")
    }));
}

#[test]
fn deferred_surface_dependencies_resolve_independent_of_record_order() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=PLANE('',#4);
#6=OFFSET_SURFACE('',#7,1.,.F.);
#7=OFFSET_SURFACE('',#5,2.,.F.);
#8=GEOMETRIC_SET('',(#6));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#6"));
    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "step:data:surface#7"));
    assert_eq!(result.ir().model.bodies.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn unknown_recursive_curve_dependency_is_refused_without_panicking() {
    use cadmpeg_ir::geometry::{
        CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
    };

    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("test:model:curve#unknown").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        source_object: None,
    });
    ir.model.curves.push(Curve {
        id: CurveId::mint("test:model:curve#composite").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: cadmpeg_ir::geometry::CompositeCurveSegments::try_from(vec![
                CompositeCurveSegment {
                    curve: CurveId::mint("test:model:curve#unknown").expect("identity grammar"),
                    same_sense: true,
                    transition: CompositeCurveTransition::Continuous,
                },
            ])
            .unwrap(),
            self_intersect: Some(false),
        }),
        source_object: None,
    });
    let output = export(&ir);
    assert!(!output.contains("COMPOSITE_CURVE("));
    let mut builder = Builder::new(&ir, StepSchema::Ap242Edition3);
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.active_curves.is_empty());
    assert!(builder.emit_curve("composite").is_none());
    assert!(builder.active_curves.is_empty());
}

#[test]
fn a_weight_lane_shorter_than_its_pole_lane_is_stated_as_a_loss() {
    let result = decode_inline(
        "#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=CARTESIAN_POINT('',(1.,1.,0.));
#3=CARTESIAN_POINT('',(2.,0.,0.));
#7=(BOUNDED_CURVE() B_SPLINE_CURVE(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.) QUASI_UNIFORM_CURVE() RATIONAL_B_SPLINE_CURVE((1.,.5)) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('short'));
#8=GEOMETRIC_SET('',(#7));
#9=GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION('',(#8),#10);
#10=(GEOMETRIC_REPRESENTATION_CONTEXT(3)REPRESENTATION_CONTEXT('',''));",
    );
    assert!(
        result
            .ir()
            .model
            .curves
            .iter()
            .all(|curve| curve.id.as_str() != "step:data:curve#7"),
        "a refused carrier states no curve"
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("B_SPLINE_CURVE #7")
                && loss.message.contains("pole(s) against")),
        "the refusal names the record and both lane counts: {:#?}",
        result.report().losses
    );
}

#[test]
fn knot_construction_retains_only_the_surviving_backing() {
    use crate::parse::Value;
    use cadmpeg_core::decode::DecodePolicy;
    for defaulted in [false, true] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1024;
        policy.limits.max_materialized_bytes = 1024;
        crate::test_support::with_policy_context(b"finite knots", &policy, |_, ctx| {
            let knots = if defaulted {
                super::super::default_nurbs_knots(
                    3,
                    2,
                    super::super::DefaultNurbsKnotKind::Bezier,
                    ctx,
                )
            } else {
                super::super::expand_knots(
                    &Value::List(vec![Value::Integer(3), Value::Integer(3)]),
                    &Value::List(vec![Value::Integer(0), Value::Integer(1)]),
                    6,
                    ctx,
                )
            }
            .expect("admitted knots")
            .expect("ordered knots")
            .into_values();
            assert_eq!(knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            // The admitted knot vector owns the only surviving scalar backing.
            let retained =
                u64::try_from(knots.capacity() * std::mem::size_of::<f64>()).expect("small knots");
            ctx.charge_retained(1024 - retained, "remaining retained allowance")
                .expect("no retained staging");
            let _released = ctx
                .reserve_scoped(1024, "released finite knot staging")
                .expect("staging released");
        });
    }
}

#[test]
fn rational_nurbs_curve_separate_lanes_have_materialized_boundaries() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use std::collections::BTreeMap;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(1.,1.,0.));#3=CARTESIAN_POINT('',(2.,0.,0.));#4=(BOUNDED_CURVE() B_SPLINE_CURVE(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.) BEZIER_CURVE() RATIONAL_B_SPLINE_CURVE((1.,.5,1.)) CURVE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('rational'));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let points = BTreeMap::from([
        (1, FinitePoint3::ZERO),
        (
            2,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0)).expect("point"),
        ),
        (
            3,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0)).expect("point"),
        ),
    ]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_nurbs_curve_control_points",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");

                super::super::nurbs_curve(4, &exchange.records()[&4], &points, (&mut Vec::new(), &mut loss_storage), ctx)
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "step_nurbs_curve_control_points"));
    crate::test_support::with_service_context(source, |_, ctx| {
        let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");

        let curve =
            super::super::nurbs_curve(4, &exchange.records()[&4], &points, (&mut Vec::new(), &mut loss_storage), ctx)
                .expect("curve admission")
                .expect("curve");
        assert_eq!(
            curve.pole_rows().points(),
            vec![points[&1], points[&2], points[&3]]
        );
        assert_eq!(curve.pole_rows().weights(), Some(vec![1.0, 0.5, 1.0]));
    });
}

#[test]
fn rational_nurbs_surface_separate_lanes_have_materialized_boundaries() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::features::FinitePoint3;
    use std::collections::BTreeMap;
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(1.,0.,0.));#3=CARTESIAN_POINT('',(0.,1.,0.));#4=CARTESIAN_POINT('',(1.,1.,0.));#5=(BOUNDED_SURFACE() B_SPLINE_SURFACE(1,1,((#1,#2),(#3,#4)),.UNSPECIFIED.,.F.,.F.,.F.) BEZIER_SURFACE() RATIONAL_B_SPLINE_SURFACE(((1.,1.),(1.,1.))) SURFACE() GEOMETRIC_REPRESENTATION_ITEM() REPRESENTATION_ITEM('rational'));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let points = BTreeMap::from([
        (1, FinitePoint3::ZERO),
        (
            2,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)).expect("point"),
        ),
        (
            3,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0)).expect("point"),
        ),
        (
            4,
            FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0)).expect("point"),
        ),
    ]);
    let run = |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        crate::test_support::with_policy_context(source, &policy, |_, ctx| {
        let mut loss_storage = ctx.reserve_scoped(0, "test geometry report backing").expect("report owner");

            super::super::nurbs_surface(5, &exchange.records()[&5], &points, (&mut Vec::new(), &mut loss_storage), ctx)
        })
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_nurbs_surface_control_points",
        run,
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "step_nurbs_surface_control_points"));
    let surface = run(u64::MAX).expect("surface admission").expect("surface");
    assert_eq!(
        surface.control_grid(),
        vec![vec![points[&1], points[&2]], vec![points[&3], points[&4]]]
    );
    assert_eq!(
        surface.pole_grid().weights(),
        Some(vec![vec![1.0, 1.0], vec![1.0, 1.0]])
    );
}
