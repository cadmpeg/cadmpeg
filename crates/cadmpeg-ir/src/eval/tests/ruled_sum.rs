// SPDX-License-Identifier: Apache-2.0

use crate::eval::model_surface_partials_by_id;
use crate::eval::model_surface_point;
use crate::eval::model_surface_point_by_id;
use crate::eval::model_surface_second_partials_by_id;
use crate::geometry::Curve;
use crate::geometry::CurveGeometry;
use crate::geometry::ProceduralSurface;
use crate::geometry::ProceduralSurfaceDefinition;
use crate::geometry::SolvedCurveGeometry;
use crate::geometry::Surface;
use crate::geometry::SurfaceGeometry;
use crate::ids::CurveId;
use crate::ids::ProceduralSurfaceId;
use crate::ids::SurfaceId;
use crate::math::Point3;
use crate::math::Vector3;
use crate::CadIr;

fn direct_surface_fixture(
    definition: ProceduralSurfaceDefinition,
    surface_name: &str,
) -> (CadIr, SurfaceId) {
    let construction_id =
        ProceduralSurfaceId::mint(format!("test:model:construction#{surface_name}"))
            .expect("valid identity");
    let surface_id =
        SurfaceId::mint(format!("test:model:surface#{surface_name}")).expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: CurveId::mint("test:model:entity#first").expect("valid identity"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                crate::geometry::nurbs::NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(1.0, 2.0, 3.0), Point3::new(3.0, 2.0, 3.0)],
                    None,
                    false,
                )
                .expect("fixture constructor admission")
                .unwrap(),
            )),
            source_object: None,
        },
        Curve {
            id: CurveId::mint("test:model:entity#second").expect("valid identity"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                crate::geometry::nurbs::NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(5.0, 10.0, 13.0), Point3::new(5.0, 13.0, 13.0)],
                    None,
                    false,
                )
                .expect("fixture constructor admission")
                .unwrap(),
            )),
            source_object: None,
        },
    ];
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction_id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction_id,
        definition: definition,
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    (ir, surface_id)
}

#[test]
fn cacheless_ruled_surface_interpolates_profiles_and_partials() {
    let (ir, surface_id) = direct_surface_fixture(
        ProceduralSurfaceDefinition::Ruled {
            first: CurveId::mint("test:model:entity#first").expect("valid identity"),
            second: CurveId::mint("test:model:entity#second").expect("valid identity"),
            cache: None,
        },
        "ruled",
    );
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let point = model_surface_point_by_id(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface_id,
        0.25,
        0.5,
    )
    .expect("cacheless ruled point")
    .get();
    assert_eq!(point, Point3::new(3.25, 6.375, 8.0));
    assert_eq!(
        model_surface_point(
            crate::eval::admission::EvaluationAdmission::Standard,
            &ir,
            &ir.model.surfaces[0].geometry,
            0.25,
            0.5
        )
        .map(crate::features::FinitePoint3::get),
        Ok(point)
    );
    let partials = model_surface_second_partials_by_id(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface_id,
        0.25,
        0.5,
    )
    .expect("cacheless ruled second partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(1.0, 1.5, 0.0));
    assert_eq!(partials.dv, Vector3::new(3.5, 8.75, 10.0));
    assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(partials.duv, Vector3::new(-2.0, 3.0, 0.0));
    assert_eq!(partials.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn cacheless_sum_surface_adds_independent_curve_parameters() {
    let (ir, surface_id) = direct_surface_fixture(
        ProceduralSurfaceDefinition::Sum(
            crate::geometry::surface_payloads::SumSurfaceConstruction::try_new(
                CurveId::mint("test:model:entity#first").expect("valid identity"),
                CurveId::mint("test:model:entity#second").expect("valid identity"),
                Vector3::new(0.5, 1.0, 2.0),
                crate::geometry::CacheContract::from_form(None),
            )
            .expect("valid sum"),
        ),
        "sum",
    );
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let point = model_surface_point_by_id(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface_id,
        0.25,
        0.5,
    )
    .expect("cacheless sum point")
    .get();
    assert_eq!(point, Point3::new(6.0, 12.5, 14.0));
    let partials = model_surface_partials_by_id(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface_id,
        0.25,
        0.5,
    )
    .expect("cacheless sum partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 3.0, 0.0));
}

#[test]
fn a_ruled_surface_whose_point_overflows_reports_the_point_it_reached() {
    // At v = MAX the displacement between the profiles carries every
    // coordinate past the finite range.
    let (ir, surface_id) = direct_surface_fixture(
        ProceduralSurfaceDefinition::Ruled {
            first: CurveId::mint("test:model:entity#first").expect("valid identity"),
            second: CurveId::mint("test:model:entity#second").expect("valid identity"),
            cache: None,
        },
        "ruled",
    );
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let reached =
        |point: Result<crate::features::FinitePoint3, crate::eval::EvaluationFailure<Point3>>| {
            matches!(point, Err(crate::eval::EvaluationFailure::NonFinite(point))
            if point.x.is_nan() && point.y.is_nan() && point.z.is_nan())
        };
    assert!(reached(model_surface_point_by_id(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface_id,
        0.25,
        f64::MAX
    )));
    assert!(reached(model_surface_point(
        crate::eval::admission::EvaluationAdmission::Standard,
        &ir,
        &ir.model.surfaces[0].geometry,
        0.25,
        f64::MAX
    )));
}

#[test]
fn a_sum_surface_whose_point_overflows_reports_the_point_it_reached() {
    let (mut ir, surface_id) = direct_surface_fixture(
        ProceduralSurfaceDefinition::Sum(
            crate::geometry::surface_payloads::SumSurfaceConstruction::try_new(
                CurveId::mint("test:model:entity#first").expect("valid identity"),
                CurveId::mint("test:model:entity#second").expect("valid identity"),
                Vector3::new(0.5, 1.0, 2.0),
                crate::geometry::CacheContract::from_form(None),
            )
            .expect("valid sum"),
        ),
        "sum",
    );
    // Both profiles lie at x = MAX, so their sum leaves the finite range in
    // x only.
    for (curve, poles) in ir.model.curves.iter_mut().zip([
        [
            Point3::new(f64::MAX, 2.0, 3.0),
            Point3::new(f64::MAX, 4.0, 3.0),
        ],
        [
            Point3::new(f64::MAX, 10.0, 13.0),
            Point3::new(f64::MAX, 13.0, 13.0),
        ],
    ]) {
        curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            crate::geometry::nurbs::NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                poles.to_vec(),
                None,
                false,
            )
            .expect("fixture constructor admission")
            .unwrap(),
        ));
    }
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    assert_eq!(
        model_surface_point_by_id(
            crate::eval::admission::EvaluationAdmission::Standard,
            &index,
            &surface_id,
            0.25,
            0.5
        ),
        Err(crate::eval::EvaluationFailure::NonFinite(Point3::new(
            f64::INFINITY,
            13.0,
            14.0
        )))
    );
}

fn direct_construction_point_without_derivative_storage(case: usize) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use crate::eval::admission::EvaluationAdmission;
    use crate::eval::EvaluationFailure;
    use crate::geometry::surface_payloads::{ExtrusionSurfaceConstruction, RevolutionSurfaceConstruction};
    use crate::geometry::CacheContract;

    let first = CurveId::mint("test:model:entity#first").unwrap();
    let second = CurveId::mint("test:model:entity#second").unwrap();
    let (definition, v, expected) = match case {
        0 => (ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(first, None, Vector3::new(0.0, 0.0, 1.0),
                None, CacheContract::from_form(None)).unwrap()),
            0.5, Point3::new(1.5, 2.0, 3.5)),
        1 => (ProceduralSurfaceDefinition::Revolution(
            RevolutionSurfaceConstruction::try_new(first,
                (crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                 crate::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()),
                [0.0, std::f64::consts::TAU], None, None, false,
                CacheContract::from_form(None)).unwrap()),
            0.0, Point3::new(1.5, 2.0, 3.0)),
        2 => (ProceduralSurfaceDefinition::Ruled { first, second, cache: None },
            0.5, Point3::new(3.25, 6.375, 8.0)),
        3 => (ProceduralSurfaceDefinition::Sum(
            crate::geometry::surface_payloads::SumSurfaceConstruction::try_new(first, second,
                Vector3::new(0.5, 1.0, 2.0), CacheContract::from_form(None)).unwrap()),
            0.5, Point3::new(6.0, 12.5, 14.0)),
        _ => unreachable!("four direct constructions"),
    };
    let (ir, surface) = direct_surface_fixture(definition, "point-demand");
    // Index construction is fixture setup. The consuming decode point path
    // uses this original session; this does not certify index construction.
    let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
    let mut policy = DecodePolicy::service();
    // Surface and curve frames own paths of one and two slots.
    // No remaining allowance can hold copied derivative poles.
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
        3 * std::mem::size_of::<Option<crate::eval::ModelEvaluationIdentity>>());
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(model_surface_point_by_id(EvaluationAdmission::Decode(&ctx), &index,
        &surface, 0.25, v).unwrap().get(), expected);
    assert_eq!(ctx.resource_refusal(), None);
    // A derivative request needs dispatcher, mapping and curve paths.
    // Its larger live frame backing exceeds the point allowance.
    let Err(EvaluationFailure::ResourceLimit(original)) = model_surface_partials_by_id(
        EvaluationAdmission::Decode(&ctx), &index, &surface, 0.25, v)
    else { panic!("derivative storage must refuse") };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.operation, "model evaluation cycle path");
    assert_eq!(original.limit, policy.limits.max_materialized_bytes);
    assert!(original.used <= original.limit);
    assert!(original.additional > 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert_eq!(model_surface_point_by_id(EvaluationAdmission::Decode(&ctx), &index,
        &surface, f64::NAN, v), Err(EvaluationFailure::ResourceLimit(original)));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn native_extrusion_point_does_not_allocate_derivative_poles() {
    direct_construction_point_without_derivative_storage(0);
}

#[test]
fn native_revolution_point_does_not_allocate_derivative_poles() {
    direct_construction_point_without_derivative_storage(1);
}

#[test]
fn ruled_surface_point_does_not_allocate_derivative_poles() {
    direct_construction_point_without_derivative_storage(2);
}

#[test]
fn sum_surface_point_does_not_allocate_derivative_poles() {
    direct_construction_point_without_derivative_storage(3);
}
