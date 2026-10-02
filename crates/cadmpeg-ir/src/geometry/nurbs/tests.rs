// SPDX-License-Identifier: Apache-2.0
use crate::{
    geometry::nurbs::NurbsSurface,
    math::Point3,
    test_support::nurbs::{curve, surface},
};

#[test]
fn admitted_nurbs_curve_mapping_refuses_knot_and_pole_limits() {
    use crate::geometry::nurbs::NurbsCurve;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        Some(vec![1.0, 0.5]),
        false,
    ).expect("fixture constructor admission")
    .expect("rational curve");
    for limit in [0, 4] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
        let error = curve
            .map_control_points(&ctx, "mapped NURBS fixture", |point| {
                Point3::new(point.x + 2.0, point.y, point.z)
            })
            .expect_err("knot or pole limit refuses the map");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.operation == "mapped NURBS fixture"
                && resource.dimension == ResourceDimension::CollectionItems)
        );
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let mapped = curve
        .map_control_points(&ctx, "mapped NURBS fixture", |point| {
            Point3::new(point.x + 2.0, point.y, point.z)
        })
        .expect("mapping resources")
        .expect("finite mapped curve");
    assert_eq!(mapped.knots(), curve.knots());
    assert_eq!(mapped.weights(), curve.weights());
    assert_eq!(
        mapped.control_points(),
        vec![Point3::new(3.0, 2.0, 3.0), Point3::new(6.0, 5.0, 6.0),]
    );
    assert!(curve
        .map_control_points(&ctx, "mapped NURBS fixture", |_| Point3::new(
            f64::INFINITY,
            0.0,
            0.0
        ),)
        .expect("mapping resources")
        .is_none());
    assert_eq!(curve.control_points()[0], Point3::new(1.0, 2.0, 3.0));
}

#[test]
fn admitted_nurbs_surface_grid_preserves_constructor_wire_and_errors() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{KnotVector, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    let point = |x, y| FinitePoint3::new(Point3::new(x, y, 0.0)).expect("finite point");
    let rows = vec![
        vec![point(0.0, 0.0), point(0.0, 1.0)],
        vec![point(1.0, 0.0), point(1.0, 1.0)],
    ];
    let axis = || {
        NurbsSurfaceAxis::new(
            1,
            KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 0.0, 1.0, 1.0]).expect("fixture knot admission").expect("finite knots"),
            false,
        )
    };
    let old = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
        axis(),
        axis(),
        NurbsPoleGrid::Polynomial { rows: rows.clone() },
        false,
    ).expect("fixture final NURBS admission")
    .expect("old surface");
    let admitted =
        NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), axis(), axis(), NurbsPoleGrid::Polynomial { rows }, false).expect("fixture final NURBS admission")
            .expect("admitted surface");
    assert_eq!(admitted, old);
    assert_eq!(
        serde_json::to_vec(&admitted).expect("wire"),
        serde_json::to_vec(&old).expect("wire")
    );
    let short = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(
            1,
            KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 1.0]).expect("fixture knot admission").expect("finite knots"),
            false,
        ),
        axis(),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![point(0.0, 0.0), point(0.0, 1.0)],
                vec![point(1.0, 0.0), point(1.0, 1.0)],
            ],
        },
        false,
    ).expect("fixture final NURBS admission")
    .expect_err("short knot axis");
    assert_eq!(short.to_string(), "u_knots must contain 4 values, found 2");
    let ragged = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
        axis(),
        axis(),
        NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![point(0.0, 0.0), point(0.0, 1.0)],
                vec![point(1.0, 0.0)],
            ],
        },
        false,
    ).expect("fixture final NURBS admission")
    .expect_err("ragged pole grid");
    assert_eq!(
        ragged.to_string(),
        "control_points row must contain 2 values, found 1"
    );
}

#[test]
fn knot_copy_refuses_collection_limit_before_allocation() {
    let knots = super::KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 0.0, 1.0, 1.0]).expect("fixture knot admission").expect("valid knots");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty input");
    let error = knots
        .try_clone_for_decode(&ctx, "knot copy")
        .expect_err("limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "knot copy")
    );
}

#[test]
fn knot_copy_refuses_retained_limit_before_allocation() {
    let knots = super::KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 0.0, 1.0, 1.0]).expect("fixture knot admission").expect("valid knots");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 31;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty input");
    let error = knots
        .try_clone_for_decode(&ctx, "knot copy")
        .expect_err("limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && limit.operation == "knot copy")
    );
}

#[test]
fn knot_copy_succeeds_under_service_profile() {
    let knots = super::KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 0.0, 1.0, 1.0]).expect("fixture knot admission").expect("valid knots");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty input");
    assert_eq!(
        knots
            .try_clone_for_decode(&ctx, "knot copy")
            .expect("service budget"),
        knots
    );
}

#[test]
fn consumed_nurbs_parts_keep_knot_and_pole_storage() {
    use crate::geometry::nurbs::NurbsPoles3;

    let curve = curve();
    let knot_storage = curve.knots().as_slice().as_ptr();
    let NurbsPoles3::Rational { points } = curve.pole_rows() else {
        panic!("fixture must be rational");
    };
    let pole_storage = points.as_ptr();
    let (degree, knots, poles, periodic) = curve.into_parts();
    assert_eq!(degree, 1);
    assert!(periodic);
    assert_eq!(knots.as_slice().as_ptr(), knot_storage);
    let NurbsPoles3::Rational { points } = poles else {
        panic!("consumed poles must remain rational");
    };
    assert_eq!(points.as_ptr(), pole_storage);
}

#[test]
fn admitted_nurbs_curve_keeps_pole_storage() {
    use crate::geometry::nurbs::{NurbsCurve, NurbsPoles3};

    let original = curve();
    let (degree, knots, poles, periodic) = original.clone().into_parts();
    let NurbsPoles3::Rational { points } = &poles else {
        panic!("fixture must be rational");
    };
    let pole_storage = points.as_ptr();
    let rebuilt = NurbsCurve::new(&cadmpeg_test_support::service_decode_context(), degree, knots, poles, periodic).expect("fixture final NURBS admission").unwrap();
    let NurbsPoles3::Rational { points } = rebuilt.pole_rows() else {
        panic!("rebuilt curve must remain rational");
    };
    assert_eq!(points.as_ptr(), pole_storage);
    assert_eq!(rebuilt, original);
}

#[test]
fn admitted_nurbs_surface_keeps_outer_and_inner_pole_storage() {
    use crate::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};

    let original = surface();
    let grid = original.pole_grid().clone();
    let NurbsPoleGrid::Rational { rows } = &grid else {
        panic!("fixture must be rational");
    };
    let outer_storage = rows.as_ptr();
    let inner_storage = rows.iter().map(Vec::as_ptr).collect::<Vec<_>>();
    let rebuilt = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(
            original.u_degree(),
            original.u_knots().clone(),
            original.u_periodic(),
        ),
        NurbsSurfaceAxis::new(
            original.v_degree(),
            original.v_knots().clone(),
            original.v_periodic(),
        ),
        grid,
        original.normal_reversed(),
    ).expect("fixture final NURBS admission")
    .unwrap();
    let NurbsPoleGrid::Rational { rows } = rebuilt.pole_grid() else {
        panic!("rebuilt surface must remain rational");
    };
    assert_eq!(rows.as_ptr(), outer_storage);
    assert_eq!(
        rows.iter().map(Vec::as_ptr).collect::<Vec<_>>(),
        inner_storage
    );
    assert_eq!(rebuilt, original);
}

#[test]
fn nurbs_curve_copy_refuses_knot_and_pole_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let curve = curve();
    let knot_count = u64::try_from(curve.knots().len()).unwrap();
    let pole_count = u64::try_from(curve.pole_count()).unwrap();
    for (dimension, limit) in [
        (ResourceDimension::CollectionItems, knot_count - 1),
        (
            ResourceDimension::CollectionItems,
            knot_count + pole_count - 1,
        ),
        (ResourceDimension::RetainedBytes, knot_count * 8 - 1),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => panic!("unexpected test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty test input fits input limit");
        let error = curve
            .try_clone_for_decode(&ctx, "copy NURBS curve")
            .expect_err("copy exceeds resource limit");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("expected resource refusal, got {error:?}");
        };
        assert_eq!(refusal.dimension, dimension);
        assert_eq!(refusal.operation, "copy NURBS curve");
    }
}

#[test]
fn admitted_nurbs_parts_preserve_the_existing_curve_and_surface_wire() {
    use crate::geometry::nurbs::{KnotVector, NurbsCurve, NurbsError, NurbsSurfaceAxis};
    use crate::scalar::FiniteReal;

    let finite_knots = |values: &[f64]| {
        values
            .iter()
            .copied()
            .map(|value| FiniteReal::new(value).unwrap())
            .collect::<Vec<_>>()
    };
    let curve = curve();
    let knots = KnotVector::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), finite_knots(curve.knots())).expect("fixture knot admission").unwrap();
    let from_parts = NurbsCurve::new(&cadmpeg_test_support::service_decode_context(), 1, knots.clone(), curve.pole_rows().clone(), true).expect("fixture final NURBS admission").unwrap();
    assert_eq!(from_parts, curve);
    assert_eq!(
        serde_json::to_vec(&from_parts).unwrap(),
        serde_json::to_vec(&curve).unwrap()
    );
    assert_eq!(
        NurbsCurve::new(&cadmpeg_test_support::service_decode_context(), 2, knots, curve.pole_rows().clone(), true).expect("fixture final NURBS admission"),
        Err(NurbsError::Structure(
            "control_points must contain more than degree 2 poles, found 2".into()
        ))
    );
    assert_eq!(
        KnotVector::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), finite_knots(&[0.0, 1.0, 0.5])).expect("fixture knot admission"),
        Err(NurbsError::Structure("knots must be non-decreasing".into()))
    );

    let surface = surface();
    let u = NurbsSurfaceAxis::new(
        1,
        KnotVector::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), finite_knots(surface.u_knots())).expect("fixture knot admission").unwrap(),
        true,
    );
    let v = NurbsSurfaceAxis::new(
        1,
        KnotVector::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), finite_knots(surface.v_knots())).expect("fixture knot admission").unwrap(),
        false,
    );
    let from_parts = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), u, v, surface.pole_grid().clone(), true).expect("fixture final NURBS admission").unwrap();
    assert_eq!(from_parts, surface);
    assert_eq!(
        serde_json::to_vec(&from_parts).unwrap(),
        serde_json::to_vec(&surface).unwrap()
    );
}

#[test]
fn admitted_surface_grid_preserves_the_existing_wire() {
    use crate::geometry::nurbs::NurbsSurfaceAxis;

    let surface = surface();
    let u = NurbsSurfaceAxis::new(1, surface.u_knots().clone(), true);
    let v = NurbsSurfaceAxis::new(1, surface.v_knots().clone(), false);
    let admitted = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), u, v, surface.pole_grid().clone(), true).expect("fixture final NURBS admission")
        .expect("admitted fixture grid");
    assert_eq!(admitted, surface);
    assert_eq!(
        serde_json::to_vec(&admitted).expect("admitted surface wire"),
        serde_json::to_vec(&surface).expect("fixture surface wire")
    );
}

#[test]
fn owned_curve_mapping_preserves_polynomial_and_rational_poles() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::NurbsCurve;

    let rational = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        Some(vec![1.0, 0.5]),
        false,
    ).expect("fixture constructor admission")
    .expect("rational fixture curve");
    for source in [curve(), rational] {
        let mut actual = source.clone();
        let weights = actual.weights();
        actual
            .try_map_control_points(|_, point| {
                FinitePoint3::new(Point3::new(
                    point.get().x + 1.0,
                    point.get().y,
                    point.get().z,
                ))
                .ok_or("finite map")
            })
            .expect("finite point map");
        assert_eq!(actual.weights(), weights);
        assert_eq!(actual.control_points().len(), source.control_points().len());
        assert_eq!(actual.degree(), source.degree());
        assert_eq!(actual.knots(), source.knots());
        assert_eq!(actual.periodic(), source.periodic());
        for (mapped, original) in actual.control_points().iter().zip(source.control_points()) {
            assert_eq!(mapped.get().x, original.get().x + 1.0);
            assert_eq!(mapped.get().y, original.get().y);
            assert_eq!(mapped.get().z, original.get().z);
        }
    }
}

#[test]
fn a_refused_curve_pole_edit_keeps_the_prior_poles() {
    let mut curve = curve();
    let original = curve.clone();
    let refusal = curve.try_map_control_points(|_, _| {
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into(),
        ))
    });
    assert_eq!(
        refusal,
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into()
        ))
    );
    assert_eq!(curve, original);
}

#[test]
fn a_refused_surface_pole_edit_keeps_the_prior_poles() {
    let mut surface = surface();
    let original = surface.clone();
    let refusal = surface.try_map_control_points(|_, _| {
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into(),
        ))
    });
    assert_eq!(
        refusal,
        Err(crate::geometry::nurbs::NurbsError::EditRefused(
            "caller refused this pole".into()
        ))
    );
    assert_eq!(surface, original);
}

#[test]
fn curve_map_updates_every_polynomial_and_rational_pole_atomically() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::NurbsCurve;

    let polynomial = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        None,
        false,
    ).expect("fixture constructor admission")
    .unwrap();
    for mut curve in [polynomial, curve()] {
        let original = curve.clone();
        let weights = curve.weights();
        let last = curve.pole_count() - 1;
        assert_eq!(
            curve.try_map_control_points(|index, point| {
                if index == last {
                    Err("last pole")
                } else {
                    FinitePoint3::new(Point3::new(
                        point.get().x + 1.0,
                        point.get().y,
                        point.get().z,
                    ))
                    .ok_or("non-finite point")
                }
            }),
            Err("last pole")
        );
        assert_eq!(curve, original);
        curve
            .try_map_control_points(|index, point| {
                FinitePoint3::new(Point3::new(
                    point.get().x + f64::from(u32::try_from(index).unwrap()) + 1.0,
                    point.get().y,
                    point.get().z,
                ))
                .ok_or("non-finite point")
            })
            .unwrap();
        for (index, (mapped, prior)) in curve
            .control_points()
            .iter()
            .zip(original.control_points())
            .enumerate()
        {
            assert_eq!(
                mapped.get().x,
                prior.get().x + f64::from(u32::try_from(index).unwrap()) + 1.0
            );
        }
        assert_eq!(curve.weights(), weights);
    }
}

#[test]
fn surface_map_updates_every_polynomial_and_rational_pole_atomically() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};

    let rational = surface();
    let polynomial = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, rational.u_knots().to_vec(), rational.u_periodic()),
        NurbsSurfaceAxis::new(1, rational.v_knots().to_vec(), rational.v_periodic()),
        NurbsSurfaceLanes::new(
            rational
                .control_grid()
                .into_iter()
                .map(|row| row.into_iter().map(FinitePoint3::get).collect())
                .collect(),
            None::<Vec<Vec<f64>>>,
        ),
        rational.normal_reversed(),
    ).expect("fixture constructor admission")
    .unwrap();
    for mut surface in [polynomial, rational] {
        let original = surface.clone();
        let weights = surface.weights();
        let last = surface.poles().len() - 1;
        assert_eq!(
            surface.try_map_control_points(|index, point| {
                if index == last {
                    Err("last pole")
                } else {
                    FinitePoint3::new(Point3::new(
                        point.get().x + 1.0,
                        point.get().y,
                        point.get().z,
                    ))
                    .ok_or("non-finite point")
                }
            }),
            Err("last pole")
        );
        assert_eq!(surface, original);
        surface
            .try_map_control_points(|index, point| {
                FinitePoint3::new(Point3::new(
                    point.get().x + f64::from(u32::try_from(index).unwrap()) + 1.0,
                    point.get().y,
                    point.get().z,
                ))
                .ok_or("non-finite point")
            })
            .unwrap();
        for (index, (mapped, prior)) in surface.poles().iter().zip(original.poles()).enumerate() {
            assert_eq!(
                mapped.get().x,
                prior.get().x + f64::from(u32::try_from(index).unwrap()) + 1.0
            );
        }
        assert_eq!(surface.weights(), weights);
    }
}

/// The control grid states both pole counts, so the surface wire carries no
/// `u_count` or `v_count`, and rows of unequal length are refused.
#[test]
fn a_nurbs_surface_states_its_pole_counts_in_its_control_grid() {
    let surface = surface();
    let wire = serde_json::to_value(&surface).expect("serializes");
    assert!(wire.get("u_count").is_none());
    assert!(wire.get("v_count").is_none());
    assert_eq!(wire["poles"]["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(surface.u_count(), 2);
    assert_eq!(surface.v_count(), 2);
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(wire.clone()).expect("round trip"),
        surface
    );

    let mut restated = wire.clone();
    restated["u_count"] = serde_json::json!(2);
    let error = serde_json::from_value::<NurbsSurface>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("u_count"), "{error}");

    let mut ragged = wire;
    ragged["poles"]["rows"][1] =
        serde_json::json!([{"point": {"x": 0.0, "y": 0.0, "z": 0.0}, "weight": 1.0}]);
    let error = serde_json::from_value::<NurbsSurface>(ragged)
        .unwrap_err()
        .to_string();
    assert!(error.contains("control_points row"), "{error}");
}

#[test]
fn surface_transposition_preserves_every_pole_and_weight() {
    let points = vec![
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        vec![Point3::new(7.0, 8.0, 9.0), Point3::new(10.0, 11.0, 12.0)],
        vec![Point3::new(13.0, 14.0, 15.0), Point3::new(16.0, 17.0, 18.0)],
    ];
    for weights in [
        None,
        Some(vec![vec![-1.0, 2.0], vec![3.0, -4.0], vec![5.0, 6.0]]),
    ] {
        let mut surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                true,
            ),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false),
            crate::geometry::nurbs::NurbsSurfaceLanes::new(points.clone(), weights),
            true,
        ).expect("fixture constructor admission")
        .unwrap();
        let original = surface.clone();
        surface.transpose_parameter_axes(&cadmpeg_test_support::service_decode_context()).expect("transpose admission");
        assert_eq!((surface.u_count(), surface.v_count()), (2, 3));
        assert_eq!((surface.u_degree(), surface.v_degree()), (1, 2));
        assert_eq!(surface.u_knots(), original.v_knots());
        assert_eq!(surface.v_knots(), original.u_knots());
        assert_eq!((surface.u_periodic(), surface.v_periodic()), (false, true));
        assert!(surface.normal_reversed());
        for u in 0..3 {
            for v in 0..2 {
                assert_eq!(surface.pole(v, u), original.pole(u, v));
                assert_eq!(surface.weight(v, u), original.weight(u, v));
            }
        }
        surface.transpose_parameter_axes(&cadmpeg_test_support::service_decode_context()).expect("transpose admission");
        assert_eq!(surface, original);
    }
}

#[test]
fn bspline_surface_edit_refusal_keeps_control_points() {
    use crate::geometry::nurbs::{BsplineSurface, NurbsError};
    let points = vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let mut surface = BsplineSurface::new(&cadmpeg_test_support::service_decode_context(), 1, 1, knots.clone(), knots, points).expect("fixture B-spline admission").unwrap();
    let original = surface.clone();
    let refusal = surface.try_map_control_points(|index, point| {
        if index == 3 {
            return Err(NurbsError::EditRefused("caller refused this pole".into()));
        }
        let mut moved = point.get();
        moved.z = 3.0;
        crate::features::FinitePoint3::new(moved)
            .ok_or_else(|| NurbsError::Structure("non-finite pole".into()))
    });
    assert_eq!(
        refusal,
        Err(NurbsError::EditRefused("caller refused this pole".into()))
    );
    assert_eq!(surface, original);
}

#[test]
fn bspline_surface_numeric_admission_and_transactional_edit() {
    use crate::geometry::nurbs::BsplineSurface;
    use crate::math::Point3;
    let points = vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    assert!(BsplineSurface::new(&cadmpeg_test_support::service_decode_context(),
        1,
        1,
        vec![0.0, 1.0, 0.0, 1.0],
        knots.clone(),
        points.clone()
    ).expect("fixture B-spline admission")
    .is_err());
    let mut surface = BsplineSurface::new(&cadmpeg_test_support::service_decode_context(), 1, 1, knots.clone(), knots, points).expect("fixture B-spline admission").unwrap();
    let original = surface.clone();
    assert!(surface
        .try_map_control_points(|_, point| {
            let mut moved = point.get();
            moved.x = f64::NAN;
            crate::features::FinitePoint3::new(moved).ok_or(())
        })
        .is_err());
    assert_eq!(surface, original);
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["u_knots"] = serde_json::json!([0.0, 1.0, 0.0, 1.0]);
    assert!(serde_json::from_value::<BsplineSurface>(wire).is_err());
    surface
        .try_map_control_points(|_, point| {
            let mut moved = point.get();
            moved.z = 2.0;
            crate::features::FinitePoint3::new(moved).ok_or(())
        })
        .unwrap();
    assert!(surface
        .control_points
        .iter()
        .flatten()
        .all(|point| point.z == 2.0));
}

#[test]
fn nurbs_stores_hand_out_their_admitted_poles_knots_and_weights() {
    use crate::test_support::nurbs::{pcurve, polar};

    let curve = curve();
    assert_eq!(curve.control_points(), curve.pole_rows().raw_points());
    assert_eq!(curve.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        curve.knots().iter().copied().collect::<Vec<_>>(),
        [2.0, 2.0, 5.0, 5.0]
    );
    assert_eq!(
        curve.weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect()),
        curve.pole_rows().weights()
    );
    assert_eq!(curve.full_knot_endpoints().endpoints(), [2.0, 5.0]);
    assert_eq!(
        crate::eval::nurbs_curve_parameter_domain(&curve)
            .map(crate::topology::IncreasingParameterInterval::endpoints),
        Some([2.0, 5.0])
    );
    assert_eq!(
        crate::eval::nurbs_pcurve_parameter_domain(1, &[0.0, 1.0, 1.0, 2.0], 2),
        None
    );
    let mut reversed = curve.clone();
    reversed.reverse_parameterization();
    assert_eq!(reversed.knots().as_slice(), [-5.0, -5.0, -2.0, -2.0]);

    let surface = surface();
    assert_eq!(surface.control_grid(), surface.pole_grid().raw_points());
    assert_eq!(surface.poles(), surface.pole_grid().raw_points().concat());
    assert_eq!(
        surface.pole(1, 0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(surface.pole(2, 0), None);
    assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        surface.weight(1, 1).map(crate::scalar::NonZeroReal::get),
        Some(-2.0)
    );
    assert_eq!(
        surface.pole_weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect()),
        surface.pole_grid().weights().map(|rows| rows.concat())
    );

    let pcurve = pcurve();
    assert_eq!(pcurve.control_points(), pcurve.pole_rows().raw_points());
    assert_eq!(pcurve.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        pcurve.weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect()),
        pcurve.pole_rows().weights()
    );

    let polar = polar();
    assert_eq!(polar.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        polar.weights().map(|weights| weights
            .into_iter()
            .map(crate::scalar::NonZeroReal::get)
            .collect::<Vec<_>>()),
        Some(vec![1.0, 2.0])
    );
}

#[test]
fn finite_knot_lanes_keep_the_raw_wire_and_order_refusal() {
    use crate::geometry::nurbs::KnotVector;
    use crate::scalar::FiniteReal;

    let raw = vec![0.0, 0.0, 1.0, 1.0];
    let admitted = raw
        .iter()
        .copied()
        .map(FiniteReal::new)
        .collect::<Option<Vec<_>>>()
        .unwrap();
    let from_finite = KnotVector::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), admitted).expect("fixture knot admission").unwrap();
    let from_raw = KnotVector::new(&cadmpeg_test_support::service_decode_context(), raw).expect("fixture knot admission").unwrap();
    assert_eq!(from_finite, from_raw);
    assert_eq!(
        serde_json::to_vec(&from_finite).unwrap(),
        serde_json::to_vec(&from_raw).unwrap()
    );
    assert!(KnotVector::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), vec![
        FiniteReal::new(1.0).unwrap(),
        FiniteReal::new(0.0).unwrap(),
    ]).expect("fixture knot admission")
    .is_err());
}

#[test]
fn finite_nurbs_lanes_match_raw_curve_surface_and_pcurve_routes() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{NurbsCurve, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::pcurve::PcurveNurbs;
    use crate::math::Point2;
    use crate::scalar::FiniteReal;
    use crate::units::FinitePoint2;

    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let finite_knots = || {
        knots
            .iter()
            .copied()
            .map(|value| FiniteReal::new(value).unwrap())
            .collect()
    };
    let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let finite_points = || {
        points
            .iter()
            .copied()
            .map(|point| FinitePoint3::new(point).unwrap())
            .collect()
    };
    let weights = vec![1.0, 2.0];
    let finite_weights = || {
        weights
            .iter()
            .copied()
            .map(|value| FiniteReal::new(value).unwrap())
            .collect()
    };
    assert_eq!(
        NurbsCurve::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            finite_knots(),
            finite_points(),
            Some(finite_weights()),
            false
        ).expect("fixture pole pairing admission"),
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(),
            1,
            knots.clone(),
            points.clone(),
            Some(weights.clone()),
            false
        ).expect("fixture constructor admission"),
    );
    assert_eq!(
        NurbsCurve::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            finite_knots(),
            finite_points(),
            Some(vec![FiniteReal::ZERO]),
            false
        ).expect("fixture pole pairing admission"),
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1, knots.clone(), points.clone(), Some(vec![0.0]), false).expect("fixture constructor admission"),
    );
    let grid = vec![points.clone(), points.clone()];
    let finite_grid = || vec![finite_points(), finite_points()];
    let finite_weight_grid = || vec![finite_weights(), finite_weights()];
    assert_eq!(
        NurbsSurface::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), 
            NurbsSurfaceAxis::new(1, finite_knots(), false),
            NurbsSurfaceAxis::new(1, finite_knots(), false),
            NurbsSurfaceLanes::new(finite_grid(), Some(finite_weight_grid())),
            false,
        ).expect("fixture pole pairing admission"),
        NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, knots.clone(), false),
            NurbsSurfaceAxis::new(1, knots.clone(), false),
            NurbsSurfaceLanes::new(grid, Some(vec![weights.clone(), weights.clone()])),
            false,
        ).expect("fixture constructor admission"),
    );
    let uv = vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)];
    assert_eq!(
        PcurveNurbs::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            finite_knots(),
            uv.iter()
                .copied()
                .map(|point| FinitePoint2::new(point).unwrap())
                .collect(),
            Some(finite_weights()),
            false,
        ).expect("fixture pcurve construction admission"),
        PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 1, knots, uv, Some(weights), false).expect("fixture pcurve construction admission"),
    );
}

#[test]
fn a_bspline_surface_holds_its_admitted_knots_and_poles() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{BsplineSurface, KnotVector};
    use crate::math::Point3;

    let points = vec![
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 2.0)],
    ];
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let surface = BsplineSurface::new(&cadmpeg_test_support::service_decode_context(), 1, 1, knots.clone(), knots.clone(), points.clone()).expect("fixture B-spline admission").unwrap();
    assert_eq!(surface.u_knots, KnotVector::new(&cadmpeg_test_support::service_decode_context(), knots.clone()).expect("fixture knot admission").unwrap());
    assert_eq!(
        surface.control_points[1][1],
        FinitePoint3::new(Point3::new(1.0, 1.0, 2.0)).unwrap()
    );
    let wire = serde_json::to_value(&surface).unwrap();
    assert_eq!(wire["u_knots"], serde_json::json!(knots));
    assert_eq!(wire["control_points"], serde_json::json!(points));
    assert_eq!(
        serde_json::from_value::<BsplineSurface>(wire).unwrap(),
        surface
    );
}

#[test]
fn nurbs_stores_hold_admitted_poles_and_take_admitted_lanes() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{
        bezier::positive_controls, NurbsCurve, NurbsError, NurbsPoles3, NurbsSurfaceAxis,
        NurbsSurfaceLanes,
    };
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles, PolarPcurveNurbs};
    use crate::math::Point2;
    use crate::scalar::{FiniteReal, NonZeroReal};
    use crate::test_support::nurbs::{pcurve, polar};
    use crate::units::FinitePoint2;

    let curve = curve();
    let held: &NurbsPoles3<FinitePoint3> = curve.pole_rows();
    assert_eq!(
        NurbsCurve::new(&cadmpeg_test_support::service_decode_context(), 1, curve.knots().to_vec(), held.clone(), true).expect("fixture final NURBS admission"),
        Ok(curve.clone())
    );
    assert_eq!(held.to_raw().points(), curve.pole_rows().raw_points());
    assert_eq!(
        NurbsCurve::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            curve.knots().clone(),
            curve.control_points(),
            curve.weights(),
            true,
        ).expect("fixture pole pairing admission"),
        Ok(curve.clone())
    );
    let weight = NonZeroReal::new(1.0).unwrap();
    assert_eq!(
        NurbsCurve::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            curve.knots().clone(),
            curve.control_points(),
            Some(vec![weight]),
            true,
        ).expect("fixture pole pairing admission"),
        Err(NurbsError::WeightLaneLength {
            field: "poles".to_owned(),
            poles: 2,
            weights: 1,
        })
    );
    let non_finite = vec![Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let raw_refusal =
        NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1, vec![0.0, 0.0, 1.0, 1.0], non_finite.clone(), None, false).expect("fixture constructor admission")
            .unwrap_err();
    assert_eq!(
        NurbsCurve::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            super::KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 0.0, 1.0, 1.0]).expect("fixture knot admission").unwrap(),
            non_finite,
            None,
            false,
        ).expect("fixture pole pairing admission"),
        Err(raw_refusal)
    );
    assert_eq!(
        NurbsCurve::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            4,
            super::KnotVector::new(&cadmpeg_test_support::service_decode_context(), vec![0.0, 0.0, 1.0, 1.0]).expect("fixture knot admission").unwrap(),
            vec![Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        ).expect("fixture pole pairing admission"),
        Err(NurbsError::Structure(
            "control_points must contain more than degree 4 poles, found 2".into()
        ))
    );
    // At t = 3 the weights -1 and 2 blend to -1 * 2/3 + 2 * 1/3 = 0, so the
    // homogeneous point has no projection.
    assert_eq!(
        crate::eval::nurbs_curve_point_at(&curve, 3.0),
        Err(crate::eval::EvaluationFailure::NoValue)
    );

    let mut mapped = curve.clone();
    let refusal = mapped.try_map_control_points(|_, _| Err(NurbsError::EditRefused("kept".into())));
    assert_eq!(refusal, Err(NurbsError::EditRefused("kept".into())));
    assert_eq!(mapped, curve);
    mapped
        .try_map_control_points(|_, point| Ok::<_, NurbsError>(point.negated()))
        .unwrap();
    assert_eq!(
        mapped.control_points(),
        vec![Point3::new(-1.0, -2.0, -3.0), Point3::new(-4.0, -5.0, -6.0)]
    );
    assert_eq!(mapped.weights(), curve.weights());

    let surface = surface();
    let u = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], true);
    let v = || NurbsSurfaceAxis::new(1, vec![2.0, 2.0, 5.0, 5.0], false);
    assert_eq!(
        crate::geometry::nurbs::NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), u(), v(), surface.pole_grid().clone(), true).expect("fixture final NURBS admission"),
        Ok(surface.clone())
    );
    assert_eq!(
        crate::geometry::nurbs::NurbsSurface::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            NurbsSurfaceAxis::new(1, surface.u_knots().clone(), true),
            NurbsSurfaceAxis::new(1, surface.v_knots().clone(), false),
            NurbsSurfaceLanes::new(surface.control_grid(), surface.weights()),
            true,
        ).expect("fixture pole pairing admission"),
        Ok(surface.clone())
    );
    assert_eq!(
        positive_controls(&surface.poles(), Some(&[1.0, 1.0, 2.0, 2.0]))
            .expect("resource allocation did not fail"),
        positive_controls(
            &surface.pole_grid().raw_points().concat(),
            Some(&[1.0, 1.0, 2.0, 2.0])
        )
        .expect("resource allocation did not fail")
    );
    assert_eq!(
        positive_controls(&surface.poles(), None).expect("resource allocation did not fail"),
        positive_controls(&surface.poles(), Some(&[1.0; 4]))
            .expect("resource allocation did not fail")
    );
    assert_eq!(
        positive_controls(&[Point3::new(f64::INFINITY, 0.0, 0.0)], Some(&[1.0]))
            .expect("resource allocation did not fail"),
        None
    );
    let mut mapped = surface.clone();
    mapped
        .try_map_control_points(|_, point| Ok::<_, NurbsError>(point.negated()))
        .unwrap();
    assert_eq!(
        mapped.pole(1, 1).map(FinitePoint3::get),
        Some(Point3::new(-1.0, -1.0, 0.0))
    );
    assert_eq!(mapped.weights(), surface.weights());

    let pcurve = pcurve();
    let held: &PcurveNurbsPoles<FinitePoint2> = pcurve.pole_rows();
    assert_eq!(
        PcurveNurbs::new(&cadmpeg_test_support::service_decode_context(), 1, pcurve.knots().to_vec(), held.clone(), true).expect("fixture pcurve construction admission"),
        Ok(pcurve.clone())
    );
    assert_eq!(
        PcurveNurbs::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            pcurve.knots().clone(),
            pcurve.control_points(),
            pcurve.weights(),
            true,
        ).expect("fixture pcurve construction admission"),
        Ok(pcurve.clone())
    );
    let mut mapped = pcurve.clone();
    mapped
        .try_map_control_points(|_, point| Ok::<_, NurbsError>(point.negated()))
        .unwrap();
    assert_eq!(
        mapped.control_points(),
        vec![Point2::new(-1.0, -2.0), Point2::new(-3.0, -4.0)]
    );
    let lifted = pcurve
        .lift(|point| Point3::new(point.u, point.v, 0.0))
        .unwrap();
    assert_eq!(lifted.weights(), pcurve.weights());
    assert_eq!(
        pcurve.lift(|_| Point3::new(f64::NAN, 0.0, 0.0)),
        Err(NurbsError::Structure(
            "control_points contains a non-finite point".into()
        ))
    );

    let polar = polar();
    assert_eq!(
        PolarPcurveNurbs::new(&cadmpeg_test_support::service_decode_context(), 1, polar.knots().to_vec(), polar.pole_rows().clone(), true).expect("fixture pcurve construction admission"),
        Ok(polar.clone())
    );
    assert_eq!(
        PolarPcurveNurbs::from_checked_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            polar.knots().clone(),
            polar.poles(),
            polar.weights(),
            true,
        ).expect("fixture pcurve construction admission"),
        Ok(polar.clone())
    );
    assert_eq!(
        polar.axial_control_values(),
        vec![FiniteReal::new(5.0).unwrap(), FiniteReal::new(6.0).unwrap()]
    );
}

#[test]
fn context_free_pole_reconstruction_does_not_enter_a_decode_constructor() {
    use super::{NurbsCurve, NurbsError, NurbsPoleGrid, NurbsPoles3, NurbsSurfaceAxis, PoleValue};
    use crate::features::FinitePoint3;

    #[derive(Clone, Copy)]
    struct Pole(Point3);
    impl PoleValue<FinitePoint3> for Pole {
        fn admit(self) -> Option<FinitePoint3> { FinitePoint3::new(self.0) }
        fn admit_curve_poles<E>(poles: NurbsPoles3<Self>, convert: impl FnOnce(NurbsPoles3<Self>) -> Result<NurbsPoles3<FinitePoint3>, E>) -> Result<NurbsPoles3<FinitePoint3>, E> {
            if std::any::type_name::<E>() != std::any::type_name::<NurbsError>() {
                panic!("context-free curve reconstruction must not start a decode session");
            }
            convert(poles)
        }
        fn admit_surface_poles<E>(grid: NurbsPoleGrid<Self>, convert: impl FnOnce(NurbsPoleGrid<Self>) -> Result<NurbsPoleGrid<FinitePoint3>, E>) -> Result<NurbsPoleGrid<FinitePoint3>, E> {
            if std::any::type_name::<E>() != std::any::type_name::<NurbsError>() {
                panic!("context-free surface reconstruction must not start a decode session");
            }
            convert(grid)
        }
    }
    let points = vec![Pole(Point3::new(0.0, 0.0, 0.0)), Pole(Point3::new(1.0, 0.0, 0.0))];
    let curve = NurbsCurve::new(&cadmpeg_test_support::service_decode_context(), 1, vec![0.0, 0.0, 1.0, 1.0], NurbsPoles3::Polynomial { points: points.clone() }, false).expect("fixture final NURBS admission").unwrap();
    assert_eq!(curve.control_points(), vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]);
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let surface = NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), axis(), axis(), NurbsPoleGrid::Polynomial { rows: vec![points.clone(), points] }, false).expect("fixture final NURBS admission").unwrap();
    assert_eq!(surface.u_count(), 2);
    assert_eq!(surface.v_count(), 2);
    assert_eq!(serde_json::from_value::<NurbsCurve>(serde_json::to_value(&curve).unwrap()).unwrap(), curve);
    assert_eq!(serde_json::from_value::<NurbsSurface>(serde_json::to_value(&surface).unwrap()).unwrap(), surface);
}

#[test]
fn weighted_pole_pairing_admits_each_slot_and_visit_before_weight() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    for (dimension, cap, completed) in [
        (ResourceDimension::RetainedBytes, 0, 0),
        (ResourceDimension::CollectionItems, 0, 0),
        (ResourceDimension::MaterializedBytes, 0, 0),
        (ResourceDimension::WorkUnits, 0, 0),
        (ResourceDimension::WorkUnits, 1, 1),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("fixture dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut storage = ctx.reserve_scoped(0, "test pairing scope").expect("scope");
        let visits = Cell::new(0);
        let run = || super::weighted_poles(
            vec![3_u32, 7], vec![1.0, 2.0],
            |output| ctx.reserve_retained_vec(output, 1, "test weighted pairing"),
            || ctx.charge_work(1, "test weighted pairing"),
            |_, value| {
                visits.set(visits.get() + 1);
                Ok::<_, CodecError>(crate::scalar::NonZeroReal::new(value).expect("weight"))
            },
        );
        let result = if dimension == ResourceDimension::MaterializedBytes {
            storage.with_storage(run)
        } else {
            run()
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("pair admission must refuse before conversion");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "test weighted pairing");
        assert_eq!(visits.get(), completed);
        drop(storage);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}

#[test]
fn standard_weighted_pole_pairing_preserves_order_and_first_refusal() {
    use super::{NurbsError, NurbsPoles3, WeightedPole3};
    use crate::scalar::{FiniteReal, NonZeroReal};

    let points = vec![3_u32, 7, 11];
    let weights = vec![1.0, -2.0, 3.0];
    let expected = NurbsPoles3::Rational {
        points: vec![
            WeightedPole3 { point: 3, weight: NonZeroReal::new(1.0).expect("weight") },
            WeightedPole3 { point: 7, weight: NonZeroReal::new(-2.0).expect("weight") },
            WeightedPole3 { point: 11, weight: NonZeroReal::new(3.0).expect("weight") },
        ],
    };
    assert_eq!(NurbsPoles3::from_lanes(&cadmpeg_test_support::service_decode_context(), points.clone(), Some(weights.clone())).expect("fixture pole pairing admission").expect("raw"), expected);
    assert_eq!(NurbsPoles3::from_finite_lanes(&cadmpeg_test_support::service_decode_context(), points.clone(), Some(weights.into_iter()
        .map(|weight| FiniteReal::new(weight).expect("finite weight")).collect())).expect("fixture pole pairing admission").expect("finite"), expected);
    assert_eq!(NurbsPoles3::from_lanes(&cadmpeg_test_support::service_decode_context(), points, Some(vec![1.0, 0.0, f64::NAN])).expect("fixture pole pairing admission"),
        Err(NurbsError::UnusableWeight { field: "poles".to_owned(), index: 1, weight: 0.0 }));
    let wire = serde_json::to_value(&expected).expect("wire");
    assert_eq!(serde_json::from_value::<NurbsPoles3<u32>>(wire).expect("standard reconstruction"), expected);
}

#[test]
fn raw_lane_constructors_share_caller_work_and_keep_refusal() {
    use crate::features::FinitePoint3;
    use crate::geometry::nurbs::{NurbsCurve, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite pole");
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let original_knots = knots.as_ptr();
    let curve = NurbsCurve::from_lanes(&ctx, 1, knots, vec![point; 2], None, false)
        .expect("both knot scans fit")
        .expect("valid curve");
    assert_eq!(curve.knots().as_ptr(), original_knots);
    let result = NurbsSurface::from_lanes(
        &ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![point; 2]; 2], None),
        false,
    );
    let Err(CodecError::ResourceLimit(limit)) = result else {
        panic!("surface construction must use the exhausted caller account");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "IR NURBS grid row shape");
    assert_eq!(limit.used, 8);
    assert_eq!(limit.additional, 1);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn shared_pole_conversion_refuses_before_visits_and_keeps_order() {
    use super::{NurbsPoleGrid, NurbsPoles3, PoleValue, WeightedPole3};
    use crate::features::FinitePoint3;
    use crate::scalar::NonZeroReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    #[derive(Clone, Copy)]
    struct Pole<'a>(&'a Cell<usize>, Point3);
    impl PoleValue<FinitePoint3> for Pole<'_> {
        fn admit(self) -> Option<FinitePoint3> {
            self.0.set(self.0.get() + 1);
            FinitePoint3::new(self.1)
        }
    }

    for shape in 0..4 {
        let grid = shape >= 2;
        let mut cases = vec![
            (ResourceDimension::RetainedBytes, 0, 0),
            (ResourceDimension::CollectionItems, 0, 0),
            (ResourceDimension::MaterializedBytes, 0, 0),
            (ResourceDimension::WorkUnits, 0, 0),
            (ResourceDimension::WorkUnits, 1, if grid { 0 } else { 1 }),
        ];
        if grid {
            cases.extend([
                (ResourceDimension::WorkUnits, 2, 1),
                (ResourceDimension::WorkUnits, 3, 2),
                (ResourceDimension::WorkUnits, 4, 2),
                (ResourceDimension::WorkUnits, 5, 3),
            ]);
        }
        for (dimension, cap, completed) in cases {
            let visits = Cell::new(0);
            let points = vec![Pole(&visits, Point3::new(2.0, 3.0, 5.0)); 2];
            let weighted = || points.iter().copied().map(|point| WeightedPole3 {
                point,
                weight: NonZeroReal::new(3.0).expect("weight"),
            }).collect::<Vec<_>>();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut storage = ctx.reserve_scoped(0, "pole conversion scope").expect("empty scope");
            let mut convert = || match shape {
                0 => super::map_curve_poles(&ctx, NurbsPoles3::Polynomial { points: points.clone() }).map(|_| ()),
                1 => super::map_curve_poles(&ctx, NurbsPoles3::Rational { points: weighted() }).map(|_| ()),
                2 => super::map_surface_poles(&ctx, NurbsPoleGrid::Polynomial { rows: vec![points.clone(); 2] }).map(|_| ()),
                3 => super::map_surface_poles(&ctx, NurbsPoleGrid::Rational { rows: vec![weighted(); 2] }).map(|_| ()),
                _ => unreachable!(),
            };
            let result = if dimension == ResourceDimension::MaterializedBytes {
                storage.with_storage(&mut convert)
            } else {
                convert()
            };
            let Err(super::admitted::ConstructionError::Resource(CodecError::ResourceLimit(limit))) = result else {
                panic!("storage and work refusal must precede conversion");
            };
            assert_eq!(visits.get(), completed);
            assert_eq!(limit.dimension, dimension);
            let operation = if grid && (dimension != ResourceDimension::WorkUnits || cap == 0 || cap == 3) {
                "IR NURBS admitted grid rows"
            } else {
                "IR NURBS admitted poles"
            };
            assert_eq!(limit.operation, operation);
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
    }
}

#[test]
fn admitted_pole_conversion_keeps_all_four_owned_storage_shapes() {
    use super::{NurbsPoleGrid, NurbsPoles3, PoleValue, WeightedPole3};
    use crate::features::FinitePoint3;
    use crate::scalar::NonZeroReal;
    use cadmpeg_core::CodecError;

    let point = FinitePoint3::new(Point3::new(2.0, 3.0, 5.0)).expect("finite point");
    let points = vec![point; 2];
    let pointer = points.as_ptr();
    let NurbsPoles3::Polynomial { points } = FinitePoint3::admit_curve_poles::<CodecError>(
        NurbsPoles3::Polynomial { points }, |_| panic!("admitted polynomial lane must move"),
    ).expect("move") else { panic!("polynomial"); };
    assert_eq!(points.as_ptr(), pointer);

    let points = vec![WeightedPole3 { point, weight: NonZeroReal::new(3.0).expect("weight") }; 2];
    let pointer = points.as_ptr();
    let NurbsPoles3::Rational { points } = FinitePoint3::admit_curve_poles::<CodecError>(
        NurbsPoles3::Rational { points }, |_| panic!("admitted rational lane must move"),
    ).expect("move") else { panic!("rational"); };
    assert_eq!(points.as_ptr(), pointer);
    assert_eq!(points[0].weight.get(), 3.0);

    let rows = vec![vec![point; 2]; 2];
    let pointer = rows.as_ptr();
    let first = rows[0].as_ptr();
    let NurbsPoleGrid::Polynomial { rows } = FinitePoint3::admit_surface_poles::<CodecError>(
        NurbsPoleGrid::Polynomial { rows }, |_| panic!("admitted polynomial grid must move"),
    ).expect("move") else { panic!("polynomial grid"); };
    assert_eq!(rows.as_ptr(), pointer);
    assert_eq!(rows[0].as_ptr(), first);

    let rows = vec![points; 2];
    let pointer = rows.as_ptr();
    let first = rows[0].as_ptr();
    let NurbsPoleGrid::Rational { rows } = FinitePoint3::admit_surface_poles::<CodecError>(
        NurbsPoleGrid::Rational { rows }, |_| panic!("admitted rational grid must move"),
    ).expect("move") else { panic!("rational grid"); };
    assert_eq!(rows.as_ptr(), pointer);
    assert_eq!(rows[0].as_ptr(), first);
    assert_eq!(rows[1][1].weight.get(), 3.0);
}

#[test]
fn surface_shape_visits_refuse_before_each_row_and_keep_semantic_order() {
    use super::{NurbsError, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::features::FinitePoint3;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let point = FinitePoint3::new(Point3::new(2.0, 3.0, 5.0)).expect("point");
    for ragged in [false, true] {
        let rows = vec![vec![point; 2], vec![point; if ragged { 1 } else { 2 }]];
        for cap in [0, 1] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::require_surface_shape(&ctx, 1, 4, 1, 4,
                &NurbsPoleGrid::Polynomial { rows: rows.clone() });
            let Err(super::admitted::ConstructionError::Resource(CodecError::ResourceLimit(limit))) = result else {
                panic!("each row width needs caller work before comparison");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR NURBS grid row shape");
            assert_eq!(limit.used, cap);
            assert_eq!(limit.additional, 1);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let standard = super::require_surface_shape(&super::StandardNurbsAdmission, 1, 4, 1, 4,
            &NurbsPoleGrid::Polynomial { rows: rows.clone() });
        let admitted = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(rows, None), false).expect("admission").map(|_| ());
        assert_eq!(admitted, standard);
        if ragged {
            assert_eq!(standard, Err(NurbsError::Structure(
                "control_points row must contain 2 values, found 1".to_owned())));
        } else {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_work_units = 2;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            assert!(super::require_surface_shape(&ctx, 1, 4, 1, 4,
                &NurbsPoleGrid::Polynomial { rows: vec![vec![point; 2]; 2] }).is_ok());
            assert!(ctx.finish_session().is_ok());
        }
    }
}

#[test]
fn surface_pairing_rows_refuse_first_and_later_visits() {
    use super::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::features::FinitePoint3;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let point = FinitePoint3::new(Point3::new(2.0, 3.0, 5.0)).expect("point");
    for (cap, operation) in [
        (0, "IR NURBS paired grid rows"),
        (1, "IR NURBS paired poles"),
        (3, "IR NURBS paired grid rows"),
        (4, "IR NURBS paired poles"),
        (6, "IR NURBS grid row shape"),
        (7, "IR NURBS grid row shape"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = NurbsSurface::from_lanes(&ctx,
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(vec![vec![point; 2]; 2], Some(vec![vec![3.0; 2]; 2])), false);
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("pair and shape visits must use the same caller account");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, cap);
        assert_eq!(limit.additional, 1);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}

#[test]
fn shared_knot_checks_preserve_prefix_order_and_original_refusal() {
    use super::NurbsError;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for prefix in ["", "u_", "v_"] {
        for (knots, message) in [
            (vec![f64::NAN, 1.0, 0.0, 1.0], format!("{prefix}knots contains a non-finite value")),
            (vec![0.0, 1.0, 0.0, 1.0], format!("{prefix}knots must be non-decreasing")),
        ] {
            let standard = super::require_nondecreasing_knots(&super::StandardNurbsAdmission, &knots, prefix);
            assert_eq!(standard, Err(NurbsError::Structure(message.clone())));
            let ctx = cadmpeg_test_support::service_decode_context();
            let admitted = super::require_nondecreasing_knots(&ctx, &knots, prefix);
            assert!(matches!(admitted, Err(super::admitted::ConstructionError::Geometry(NurbsError::Structure(text))) if text == message));
            assert!(ctx.finish_session().is_ok());
        }
        for (cap, operation) in [(0, "IR NURBS knot finiteness"), (4, "IR NURBS knot order")] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = super::require_nondecreasing_knots(&ctx, &[0.0, 0.0, 1.0, 1.0], prefix);
            let Err(super::admitted::ConstructionError::Resource(CodecError::ResourceLimit(limit))) = result else {
                panic!("both knot scans need the caller account");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, operation);
            assert_eq!(limit.used, cap);
            assert_eq!(limit.additional, 4);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
    }
}

#[test]
fn knot_constructors_share_work_keep_storage_and_preserve_refusal() {
    use super::{KnotVector, NurbsError};
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let values = vec![0.0, 0.0, 1.0, 1.0];
    let storage = values.as_ptr();
    let knots = KnotVector::new(&ctx, values).expect("two admitted scans").expect("ordered knots");
    assert_eq!(knots.as_ptr(), storage);
    let retained = super::admit_knots(&ctx, knots, "").unwrap_or_else(|_| panic!("admitted knots must keep storage without admission"));
    assert_eq!(retained.as_ptr(), storage);
    let Err(CodecError::ResourceLimit(limit)) = KnotVector::new(&ctx, vec![0.0; 4]) else {
        panic!("second construction must use the exhausted account");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "IR NURBS knot finiteness");
    assert_eq!(limit.used, 8);
    assert_eq!(limit.additional, 4);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));

    for (dimension, cap, operation) in [
        (ResourceDimension::RetainedBytes, 0, "IR finite knot values"),
        (ResourceDimension::CollectionItems, 0, "IR finite knot values"),
        (ResourceDimension::WorkUnits, 0, "IR finite knot values"),
        (ResourceDimension::WorkUnits, 3, "IR finite knot values"),
        (ResourceDimension::WorkUnits, 4, "IR NURBS knot order"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) = KnotVector::from_finite_lanes(&ctx, vec![FiniteReal::ZERO; 4]) else {
            panic!("finite conversion must refuse admission");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }

    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(KnotVector::new(&ctx, vec![0.0, 2.0, 1.0, f64::NAN]).expect("diagnostic admitted"),
        Err(NurbsError::Structure("knots contains a non-finite value".into())));
    assert_eq!(KnotVector::new(&ctx, vec![0.0, 2.0, 1.0]).expect("diagnostic admitted"),
        Err(NurbsError::Structure("knots must be non-decreasing".into())));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) = KnotVector::new(&ctx, vec![f64::NAN]) else {
        panic!("diagnostic storage refusal must stay outer");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "IR NURBS refusal text");
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}

mod bspline;

mod pairing;

mod construction;

mod transposition;

mod knot_edits;
