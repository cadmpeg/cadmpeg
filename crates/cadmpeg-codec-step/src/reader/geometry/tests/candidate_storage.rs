// SPDX-License-Identifier: Apache-2.0
//! Rejected geometry releases candidate backing and keeps only emitted text.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;

use super::super::{decode, decode_tessellated_curve_sets, nurbs_curve, nurbs_surface, UnitScales};

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";
const POINT_RECORDS: &str = "#2=CARTESIAN_POINT('',(0.,0.,0.));#3=CARTESIAN_POINT('',(1.,0.,0.));#4=CARTESIAN_POINT('',(0.,1.,0.));#5=CARTESIAN_POINT('',(1.,1.,0.));";
const UNITS: &str = "#9000=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#9001=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));";

fn parse(records: &str) -> crate::parse::Exchange {
    let source = format!("{HEADER}{records}{TAIL}");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("geometry fixture")
        .0
}

fn points() -> BTreeMap<u64, FinitePoint3> {
    BTreeMap::from([
        (2, FinitePoint3::ZERO),
        (
            3,
            FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite point"),
        ),
        (
            4,
            FinitePoint3::new(Point3::new(0.0, 1.0, 0.0)).expect("finite point"),
        ),
        (
            5,
            FinitePoint3::new(Point3::new(1.0, 1.0, 0.0)).expect("finite point"),
        ),
    ])
}

fn assert_retained_bytes(ctx: &DecodeContext<'_>, bytes: usize) {
    let CodecError::ResourceLimit(limit) = ctx
        .charge_retained(u64::MAX, "test live retained storage")
        .expect_err("observation exceeds allowance")
    else {
        panic!("resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        limit.used,
        u64::try_from(bytes).expect("test storage fits u64")
    );
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn rational_curve_late_weight_rejection_retains_only_warning_text() {
    let exchange = parse(&format!("#1=(B_SPLINE_CURVE(1,(#2,#3),.UNSPECIFIED.,.F.,.F.) QUASI_UNIFORM_CURVE() RATIONAL_B_SPLINE_CURVE((1.,0.))); {POINT_RECORDS}"));
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test report backing").expect("owner");
    let mut losses = Vec::new();
    assert!(nurbs_curve(
        1,
        &exchange.records()[&1],
        &points(),
        (&mut losses, &mut storage),
        &ctx
    )
    .expect("candidate admission")
    .is_none());
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .starts_with("B_SPLINE_CURVE #1 is not a curve carrier:"));
    assert_retained_bytes(&ctx, losses[0].message.capacity());
}

#[test]
fn rational_surface_late_weight_rejection_retains_only_warning_text() {
    let exchange = parse(&format!("#1=(B_SPLINE_SURFACE(1,1,((#2,#3),(#4,#5)),.UNSPECIFIED.,.F.,.F.,.F.) QUASI_UNIFORM_SURFACE() RATIONAL_B_SPLINE_SURFACE(((1.,1.),(1.,0.))));{POINT_RECORDS}"));
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test report backing").expect("owner");
    let mut losses = Vec::new();
    assert!(nurbs_surface(
        1,
        &exchange.records()[&1],
        &points(),
        (&mut losses, &mut storage),
        &ctx
    )
    .expect("candidate admission")
    .is_none());
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .starts_with("B_SPLINE_SURFACE #1 is not a surface carrier:"));
    assert_retained_bytes(&ctx, losses[0].message.capacity());
}

#[test]
fn surface_second_knot_axis_rejection_releases_both_axes_and_poles() {
    let exchange = parse(&format!("#1=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#2,#3),(#4,#5)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(1.,0.),.UNSPECIFIED.);{POINT_RECORDS}"));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test report backing").expect("owner");
    let mut losses = Vec::new();
    assert!(nurbs_surface(
        1,
        &exchange.records()[&1],
        &points(),
        (&mut losses, &mut storage),
        &ctx
    )
    .expect("candidate admission")
    .is_none());
    assert!(losses.is_empty());
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test full scratch reuse",
        )
        .expect("all rejected geometry was released");
    drop(reuse);
    drop(losses);
    drop(storage);
    ctx.finish_session().expect("unrefused session");
}

#[test]
fn geometry_stage_claims_use_no_retained_storage() {
    let mut records = UNITS.to_owned();
    for id in 1..=64 {
        write!(records, "#{id}=DIRECTION('',(1.,0.,0.));").expect("direction");
    }
    let exchange = parse(&records);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65536;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let stage = decode(&exchange, &mut ir, &ctx).expect("temporary claims fit");
    assert_eq!(stage.claims.len(), 66);
    assert!(stage.losses.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.points.is_empty());
    drop(stage);
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test stage scratch reuse",
        )
        .expect("stage backing was released");
    drop(reuse);
    ctx.finish_session().expect("unrefused session");
}

#[test]
fn geometry_stage_losses_retain_text_without_retaining_stage_backing() {
    let exchange = parse(&format!(
        "{UNITS}#1=CARTESIAN_POINT('',());#2=CARTESIAN_POINT('',());"
    ));
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("empty root");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let stage = decode(&exchange, &mut ir, &ctx).expect("invalid coordinates stay recoverable");
    assert_eq!(stage.losses.len(), 2);
    assert_eq!(
        stage.losses[0].message,
        "CARTESIAN_POINT #1 has invalid coordinates"
    );
    assert_eq!(
        stage.losses[1].message,
        "CARTESIAN_POINT #2 has invalid coordinates"
    );
    assert_retained_bytes(
        &ctx,
        stage
            .losses
            .iter()
            .map(|loss| loss.message.capacity())
            .sum(),
    );
}

#[test]
fn tessellated_record_child_refusal_does_not_admit_the_unvisited_suffix() {
    let mut records = String::from("#1=TESSELLATED_CURVE_SET($,());");
    for id in 2..=4097 {
        write!(records, "#{id}=DUMMY();").expect("dummy");
    }
    let exchange = parse(&records);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The first record's lookups and warning fit; the 4096 unused visits do not.
    policy.limits.max_work_units = 1024;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let units = UnitScales {
        default_length: cadmpeg_ir::scalar::PositiveReal::ONE,
        default_angle: cadmpeg_ir::scalar::PositiveReal::ONE,
        length: BTreeMap::new(),
        angle: BTreeMap::new(),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut claims = std::collections::BTreeSet::new();
    let mut claim_storage = ctx.reserve_scoped(0, "test claims").expect("owner");
    let mut storage = ctx.reserve_scoped(0, "test report backing").expect("owner");
    let mut losses = Vec::new();
    let CodecError::ResourceLimit(first) = decode_tessellated_curve_sets(
        &exchange,
        &units,
        &mut ir,
        (&mut claims, &mut claim_storage),
        (&mut losses, &mut storage),
        &ctx,
    )
    .expect_err("first loss slot refuses") else {
        panic!("resource refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "step_geometry_losses");
    assert!(claims.is_empty());
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(first));
}

fn replica_rejection_releases_cloned_basis(surface: bool) {
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, PlacedCurve, PlacedSurface, SolvedCurveGeometry,
        SolvedSurfaceGeometry, Surface, SurfaceGeometry, MAX_GEOMETRY_NESTING,
    };
    use cadmpeg_ir::transform::Transform;

    let mut ir = cadmpeg_ir::CadIr::empty();
    if surface {
        let exchange = parse(&format!("#1=QUASI_UNIFORM_SURFACE('',1,1,((#2,#3),(#4,#5)),.UNSPECIFIED.,.F.,.F.,.F.);{POINT_RECORDS}"));
        let nurbs = crate::test_support::with_service_context(b"", |_, ctx| {
            let mut storage = ctx.reserve_scoped(0, "test report backing").expect("owner");
            nurbs_surface(
                1,
                &exchange.records()[&1],
                &points(),
                (&mut Vec::new(), &mut storage),
                ctx,
            )
        })
        .expect("surface admission")
        .expect("surface");
        let mut basis = SolvedSurfaceGeometry::Nurbs(nurbs);
        for _ in 0..MAX_GEOMETRY_NESTING {
            basis = SolvedSurfaceGeometry::Transformed(
                PlacedSurface::try_new(Box::new(basis), Transform::identity())
                    .expect("admitted depth"),
            );
        }
        ir.model.surfaces.push(Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("step:data:surface#10").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(basis),
            source_object: None,
        });
    } else {
        let exchange = parse(&format!(
            "#1=QUASI_UNIFORM_CURVE('',1,(#2,#3),.UNSPECIFIED.,.F.,.F.);{POINT_RECORDS}"
        ));
        let nurbs = crate::test_support::with_service_context(b"", |_, ctx| {
            let mut storage = ctx.reserve_scoped(0, "test report backing").expect("owner");
            nurbs_curve(
                1,
                &exchange.records()[&1],
                &points(),
                (&mut Vec::new(), &mut storage),
                ctx,
            )
        })
        .expect("curve admission")
        .expect("curve");
        let mut basis = SolvedCurveGeometry::Nurbs(nurbs);
        for _ in 0..MAX_GEOMETRY_NESTING {
            basis = SolvedCurveGeometry::Transformed(
                PlacedCurve::try_new(Box::new(basis), Transform::identity())
                    .expect("admitted depth"),
            );
        }
        ir.model.curves.push(Curve {
            id: cadmpeg_ir::ids::CurveId::mint("step:data:curve#10").expect("identity grammar"),
            geometry: CurveGeometry::Solved(basis),
            source_object: None,
        });
    }
    let mut records = format!("{UNITS}#10=DUMMY();#11=CARTESIAN_POINT('',(0.,0.,0.));#12=DIRECTION('',(1.,0.,0.));#13=DIRECTION('',(0.,1.,0.));#14=DIRECTION('',(0.,0.,1.));#20=CARTESIAN_TRANSFORMATION_OPERATOR_3D('',#12,#13,#11,1.,#14);");
    let kind = if surface {
        "SURFACE_REPLICA"
    } else {
        "CURVE_REPLICA"
    };
    for id in 1..=4 {
        write!(records, "#{id}={kind}('',#10,#20);").expect("replica");
    }
    let exchange = parse(&records);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let stage = decode(&exchange, &mut ir, &ctx).expect("depth refusal is recoverable");
    assert_eq!(
        stage
            .losses
            .iter()
            .filter(|loss| loss
                .message
                .contains("nests past the admitted inline basis depth"))
            .count(),
        4
    );
    assert_eq!(
        if surface {
            ir.model.surfaces.len()
        } else {
            ir.model.curves.len()
        },
        5
    );
    assert!(ir.model.procedural_curves.is_empty());
    assert!(ir.model.procedural_surfaces.is_empty());
    let CodecError::ResourceLimit(limit) = ctx
        .charge_retained(u64::MAX, "test replica retained backing")
        .expect_err("observation exceeds allowance")
    else {
        panic!("resource refusal")
    };
    // Retained output includes arena backing, unknown-record text and diagnostics. A copied
    // 256-placement NURBS basis alone exceeds this bound for each rejection.
    let unknown_text = ir
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Unknown {
                record: Some(record),
            }) => Some(record.as_str().len()),
            _ => None,
        })
        .chain(
            ir.model
                .surfaces
                .iter()
                .filter_map(|surface| match surface.geometry.solved() {
                    Some(SolvedSurfaceGeometry::Unknown {
                        record: Some(record),
                    }) => Some(record.as_str().len()),
                    _ => None,
                }),
        )
        .sum::<usize>();
    let output_bound = unknown_text
        + ir.model.curves.capacity() * std::mem::size_of::<Curve>()
        + ir.model.surfaces.capacity() * std::mem::size_of::<Surface>()
        + stage
            .losses
            .iter()
            .map(|loss| loss.message.capacity())
            .sum::<usize>();
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert!(
        limit.used <= u64::try_from(output_bound).expect("test output fits u64"),
        "{} retained bytes exceed {output_bound} output bytes",
        limit.used
    );
}

#[test]
fn rejected_curve_replicas_release_nested_nurbs_basis_clones() {
    replica_rejection_releases_cloned_basis(false);
}

#[test]
fn rejected_surface_replicas_release_nested_nurbs_basis_clones() {
    replica_rejection_releases_cloned_basis(true);
}

#[test]
fn rejected_bounded_surfaces_release_boundary_vectors() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};

    for implicit_outer in [".U.", ".F."] {
        let boundaries = "#11,".repeat(255) + "#11";
        let exchange = parse(&format!(
            "{UNITS}#1=CURVE_BOUNDED_SURFACE('',#10,({boundaries}),{implicit_outer});#10=DUMMY();#11=DUMMY();"
        ));
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("step:data:surface#10").expect("identity grammar"),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        let initial_capacity = ir.model.surfaces.capacity();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
            .expect("empty root");
        let stage = decode(&exchange, &mut ir, &ctx).expect("boundary refusal is recoverable");
        assert!(ir.model.procedural_surfaces.is_empty());
        assert_eq!(ir.model.surfaces.len(), 2);
        assert!(stage.losses.iter().any(|loss| loss.message
            == "CURVE_BOUNDED_SURFACE #1 has invalid or unresolved support/boundaries"));
        let Some(SolvedSurfaceGeometry::Unknown {
            record: Some(record),
        }) = ir.model.surfaces[1].geometry.solved()
        else {
            panic!("unresolved bounded surface carrier")
        };
        let output_bytes = (ir.model.surfaces.capacity() - initial_capacity)
            * std::mem::size_of::<Surface>()
            + record.as_str().len()
            + stage
                .losses
                .iter()
                .map(|loss| loss.message.capacity())
                .sum::<usize>();
        assert_retained_bytes(&ctx, output_bytes);
    }
}

#[test]
fn pcurve_coordinate_overflow_releases_cloned_nurbs_backing() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

    let units = UNITS.replace(".MILLI.", "$");
    let exchange = parse(&format!(
        "{units}#1=QUASI_UNIFORM_CURVE('',1,(#2,#3),.UNSPECIFIED.,.F.,.F.);#2=CARTESIAN_POINT('',(0.,0.));#3=CARTESIAN_POINT('',(1.E308,1.));#4=DEFINITIONAL_REPRESENTATION('',(#1),#5);#5=(GEOMETRIC_REPRESENTATION_CONTEXT(2) PARAMETRIC_REPRESENTATION_CONTEXT() REPRESENTATION_CONTEXT('uv','2D'));#6=PCURVE('',#10,#4);#10=DUMMY();"
    ));
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("step:data:surface#10").expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::new(
                FinitePoint3::ZERO,
                OrthonormalFrame3::from_units(UnitVector3::Z_AXIS, UnitVector3::X_AXIS)
                    .expect("orthonormal frame"),
            ),
        )),
        source_object: None,
    });
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("empty root");
    let stage = decode(&exchange, &mut ir, &ctx).expect("scaling refusal is recoverable");
    assert!(ir.model.pcurves.is_empty());
    assert!(ir.model.curves.is_empty());
    assert!(ir.model.points.is_empty());
    assert_eq!(ir.model.surfaces.len(), 1);
    assert!(stage.losses.iter().any(|loss| loss.message
        == "PCURVE #6 has a 2D carrier that cannot be scaled into the owning surface parameter units"));
    assert_retained_bytes(
        &ctx,
        stage
            .losses
            .iter()
            .map(|loss| loss.message.capacity())
            .sum(),
    );
}
