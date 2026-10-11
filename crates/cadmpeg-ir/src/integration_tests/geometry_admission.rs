// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;

use crate::geometry::analytic::EllipseCurve;
use crate::geometry::pcurve::{
    CirclePcurve, EllipsePcurve, HarmonicPcurve, LinePcurve, OffsetPcurve, PcurveGeometry,
    SphericalGreatCirclePcurve, TrimmedPcurve,
};
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
    pcurve::{PcurveNurbs, PcurveNurbsPoles, PolarNurbsPoles, PolarPcurveNurbs},
};
use crate::math::{Point2, Point3, Vector3};
use crate::test_support::nurbs::{curve, pcurve, polar, surface};

#[test]
fn construction_rejects_invalid_knots_and_non_finite_poles() {
    fn rejects_descending_knots<T: serde::Serialize + serde::de::DeserializeOwned>(carrier: T) {
        let mut wire = serde_json::to_value(carrier).unwrap();
        wire["knots"] = serde_json::json!([2.0, 5.0, 2.0, 5.0]);
        assert!(serde_json::from_value::<T>(wire).is_err());
    }

    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut knots = curve().knots().to_vec();
        knots[1] = invalid;
        assert!(NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            knots.clone(),
            curve().pole_rows().raw_points(),
            None,
            false
        )
        .expect("fixture constructor admission")
        .is_err());
        assert!(PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            knots.clone(),
            pcurve().pole_rows().raw_points(),
            None,
            false
        )
        .expect("fixture pcurve construction admission")
        .is_err());
        assert!(PolarPcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            knots,
            vec![
                crate::geometry::pcurve::PolarNurbsPole {
                    radial: Point2::new(0.0, 0.0),
                    axial: 0.0,
                };
                2
            ],
            None,
            false
        )
        .expect("fixture pcurve construction admission")
        .is_err());

        let mut points = curve().pole_rows().raw_points();
        points[1].z = invalid;
        assert!(NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            curve().knots().to_vec(),
            points,
            None,
            false
        )
        .expect("fixture constructor admission")
        .is_err());
        assert!(PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            pcurve().knots().to_vec(),
            vec![Point2::new(0.0, invalid); 2],
            None,
            false
        )
        .expect("fixture pcurve construction admission")
        .is_err());
        assert!(PolarPcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            polar().knots().to_vec(),
            vec![
                crate::geometry::pcurve::PolarNurbsPole {
                    radial: Point2::new(0.0, 0.0),
                    axial: invalid,
                };
                2
            ],
            None,
            false
        )
        .expect("fixture pcurve construction admission")
        .is_err());

        let source = surface();
        let mut points = source.pole_grid().raw_points();
        points[0][1].x = invalid;
        assert!(NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, source.u_knots().to_vec(), false),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, source.v_knots().to_vec(), false),
            crate::geometry::nurbs::NurbsSurfaceLanes::new(points, None),
            false
        )
        .expect("fixture constructor admission")
        .is_err());
    }

    rejects_descending_knots(curve());
    rejects_descending_knots(pcurve());
    rejects_descending_knots(polar());
    for axis in ["u_knots", "v_knots"] {
        let mut wire = serde_json::to_value(surface()).unwrap();
        wire[axis] = serde_json::json!([0.0, 1.0, 0.0, 1.0]);
        assert!(serde_json::from_value::<NurbsSurface>(wire).is_err());
    }
}

fn curve_weights(
    curve: &NurbsCurve,
    weights: Vec<f64>,
) -> Result<NurbsPoles3, crate::geometry::nurbs::NurbsError> {
    NurbsPoles3::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        curve.pole_rows().raw_points(),
        Some(weights),
    )
    .expect("fixture pole pairing admission")
}

fn surface_weights(
    surface: &NurbsSurface,
    weights: Vec<Vec<f64>>,
) -> Result<NurbsPoleGrid, crate::geometry::nurbs::NurbsError> {
    NurbsPoleGrid::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        surface.pole_grid().raw_points(),
        Some(weights),
    )
    .expect("fixture pole pairing admission")
}

fn pcurve_weights(
    pcurve: &PcurveNurbs,
    weights: Vec<f64>,
) -> Result<PcurveNurbsPoles, crate::geometry::nurbs::NurbsError> {
    PcurveNurbsPoles::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        pcurve.pole_rows().raw_points(),
        Some(weights),
    )
    .expect("fixture pcurve construction admission")
}

fn polar_weights(
    polar: &PolarPcurveNurbs,
    weights: Vec<f64>,
) -> Result<PolarNurbsPoles, crate::geometry::nurbs::NurbsError> {
    PolarNurbsPoles::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        polar.pole_rows().to_raw().poles(),
        Some(weights),
    )
    .expect("fixture pcurve construction admission")
}

#[test]
fn weight_rules_admit_a_signed_nonzero_weight_in_every_nurbs_carrier() {
    let mut curve = curve();
    let mut surface = surface();
    let mut pcurve = pcurve();
    let mut polar = polar();
    {
        let replacement = curve_weights(&curve, vec![1e-200, -1e-200]).unwrap();
        edit::replace(&mut curve, |previous| {
            crate::geometry::nurbs::NurbsCurve::new(
                &cadmpeg_test_support::service_decode_context(),
                previous.degree(),
                previous.knots().to_vec(),
                replacement,
                previous.periodic(),
            )
            .expect("fixture final NURBS admission")
        })
    }
    .unwrap();
    {
        let replacement = surface_weights(&surface, vec![vec![-1e-200; 2]; 2]).unwrap();
        edit::replace(&mut surface, |previous| {
            crate::geometry::nurbs::NurbsSurface::new(
                &cadmpeg_test_support::service_decode_context(),
                crate::geometry::nurbs::NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    previous.u_periodic(),
                ),
                crate::geometry::nurbs::NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                replacement,
                previous.normal_reversed(),
            )
            .expect("fixture final NURBS admission")
        })
    }
    .unwrap();
    {
        let replacement = pcurve_weights(&pcurve, vec![1e-200, -1e-200]).unwrap();
        edit::replace(&mut pcurve, |previous| {
            crate::geometry::pcurve::PcurveNurbs::new(
                &cadmpeg_test_support::service_decode_context(),
                previous.degree(),
                previous.knots().to_vec(),
                replacement,
                previous.periodic(),
            )
            .expect("fixture pcurve construction admission")
        })
    }
    .unwrap();
    {
        let replacement = polar_weights(&polar, vec![1e-200, -1e-200]).unwrap();
        edit::replace(&mut polar, |previous| {
            crate::geometry::pcurve::PolarPcurveNurbs::new(
                &cadmpeg_test_support::service_decode_context(),
                previous.degree(),
                previous.knots().to_vec(),
                replacement,
                previous.periodic(),
            )
            .expect("fixture pcurve construction admission")
        })
    }
    .unwrap();
    for invalid in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(curve_weights(&curve, vec![invalid, 1.0]).is_err());
        assert!(surface_weights(&surface, vec![vec![invalid; 2]; 2]).is_err());
        assert!(pcurve_weights(&pcurve, vec![invalid, 1.0]).is_err());
        assert!(polar_weights(&polar, vec![invalid, 1.0]).is_err());
    }
    assert!(pcurve_weights(&pcurve, vec![-1.0, 1.0]).is_ok());
    assert!(polar_weights(&polar, vec![-1.0, 1.0]).is_ok());
    assert_eq!(
        serde_json::from_value::<NurbsCurve>(serde_json::to_value(&curve).unwrap()).unwrap(),
        curve
    );
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(serde_json::to_value(&surface).unwrap()).unwrap(),
        surface
    );
    assert_eq!(
        serde_json::from_value::<PcurveNurbs>(serde_json::to_value(&pcurve).unwrap()).unwrap(),
        pcurve
    );
    assert_eq!(
        serde_json::from_value::<PolarPcurveNurbs>(serde_json::to_value(&polar).unwrap()).unwrap(),
        polar
    );

    // A weight travels in its pole row, so a weight list beside the poles is
    // an unknown key and a zero weight is refused at the pole's own scalar
    // mint.
    let mut wire = serde_json::to_value(&curve).unwrap();
    wire["weights"] = serde_json::json!([0.0, 1.0]);
    assert!(serde_json::from_value::<NurbsCurve>(wire).is_err());
    let mut wire = serde_json::to_value(&curve).unwrap();
    wire["poles"]["points"][0]["weight"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<NurbsCurve>(wire).is_err());
    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["poles"]["rows"][0][0]["weight"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<NurbsSurface>(wire).is_err());
    let mut wire = serde_json::to_value(&pcurve).unwrap();
    wire["poles"]["points"][0]["weight"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<PcurveNurbs>(wire).is_err());
    let mut wire = serde_json::to_value(&polar).unwrap();
    wire["poles"]["poles"][0]["weight"] = serde_json::json!(0.0);
    assert!(serde_json::from_value::<PolarPcurveNurbs>(wire).is_err());
}

#[test]
fn failed_numeric_edits_preserve_the_whole_carrier() {
    let mut curve = curve();
    let original = curve.clone();
    assert!(curve
        .edit_knots(
            &cadmpeg_test_support::service_decode_context(),
            <[f64]>::reverse
        )
        .expect("knot edit admission")
        .is_err());
    assert!(curve
        .try_map_control_points(
            |_, point| {
                let mut mapped = point.get();
                mapped.x = f64::INFINITY;
                crate::features::FinitePoint3::new(mapped).ok_or(())
            },
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("pole edit admission")
        .is_err());
    assert!(curve_weights(&curve, vec![0.0, 1.0]).is_err());
    assert!(curve_weights(&curve, vec![1.0]).is_err());
    assert_eq!(curve, original);

    let mut surface = surface();
    let original = surface.clone();
    assert!(edit::replace(&mut surface, |previous| {
        let mut knots = previous.u_knots().to_vec();
        (<[f64]>::reverse)(&mut knots);
        crate::geometry::nurbs::NurbsSurface::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.u_degree(),
                knots,
                previous.u_periodic(),
            ),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.v_degree(),
                previous.v_knots().to_vec(),
                previous.v_periodic(),
            ),
            previous.pole_grid().clone(),
            previous.normal_reversed(),
        )
        .expect("fixture final NURBS admission")
    })
    .is_err());
    assert!(edit::replace(&mut surface, |previous| {
        let mut knots = previous.v_knots().to_vec();
        {
            let knots: &mut [f64] = &mut knots;
            knots[1] = f64::NAN;
        };
        crate::geometry::nurbs::NurbsSurface::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.u_degree(),
                previous.u_knots().to_vec(),
                previous.u_periodic(),
            ),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.v_degree(),
                knots,
                previous.v_periodic(),
            ),
            previous.pole_grid().clone(),
            previous.normal_reversed(),
        )
        .expect("fixture final NURBS admission")
    })
    .is_err());
    assert!(surface
        .try_map_control_points(
            |_, point| {
                let mut mapped = point.get();
                mapped.y = f64::NEG_INFINITY;
                crate::features::FinitePoint3::new(mapped).ok_or(())
            },
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("pole edit admission")
        .is_err());
    assert!(surface_weights(&surface, vec![vec![0.0, 1.0], vec![1.0, 1.0]]).is_err());
    assert!(surface_weights(&surface, vec![vec![1.0]]).is_err());
    assert_eq!(surface, original);

    let mut pcurve = pcurve();
    let original = pcurve.clone();
    assert!(edit::replace(&mut pcurve, |previous| {
        let mut knots = previous.knots().to_vec();
        (<[f64]>::reverse)(&mut knots);
        crate::geometry::pcurve::PcurveNurbs::new(
            &cadmpeg_test_support::service_decode_context(),
            previous.degree(),
            knots,
            previous.pole_rows().clone(),
            previous.periodic(),
        )
        .expect("fixture pcurve construction admission")
    })
    .is_err());
    assert!(pcurve
        .try_map_control_points(
            |_, point| {
                crate::units::FinitePoint2::new(crate::math::Point2::new(f64::NAN, point.get().v))
                    .ok_or(())
            },
            &cadmpeg_test_support::service_decode_context()
        )
        .expect("pole edit admission")
        .is_err());
    assert!(pcurve_weights(&pcurve, vec![1.0, 0.0]).is_err());
    assert!(pcurve_weights(&pcurve, vec![1.0]).is_err());
    assert_eq!(pcurve, original);

    let mut polar = polar();
    let original = polar.clone();
    assert!(edit::replace(&mut polar, |previous| {
        let mut knots = previous.knots().to_vec();
        (<[f64]>::reverse)(&mut knots);
        crate::geometry::pcurve::PolarPcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            previous.degree(),
            knots,
            previous.pole_rows().to_raw().poles(),
            previous.pole_rows().weights(),
            previous.periodic(),
        )
        .expect("fixture pcurve construction admission")
    })
    .is_err());
    assert!(edit::replace(&mut polar, |previous| {
        let mut poles = previous.pole_rows().to_raw().poles();
        for pole in &mut poles {
            pole.radial.v = f64::INFINITY;
        }
        PolarPcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            previous.degree(),
            previous.knots().to_vec(),
            poles,
            previous.pole_rows().weights(),
            previous.periodic(),
        )
        .expect("fixture pcurve construction admission")
    })
    .is_err());
    assert!(edit::replace(&mut polar, |previous| {
        let mut poles = previous.pole_rows().to_raw().poles();
        for pole in &mut poles {
            pole.axial = f64::NAN;
        }
        PolarPcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            previous.degree(),
            previous.knots().to_vec(),
            poles,
            previous.pole_rows().weights(),
            previous.periodic(),
        )
        .expect("fixture pcurve construction admission")
    })
    .is_err());
    assert!(polar_weights(&polar, vec![1.0, 0.0]).is_err());
    assert!(polar_weights(&polar, vec![1.0]).is_err());
    assert_eq!(polar, original);
}

#[test]
fn reversal_preserves_weight_and_parameter_correspondence() {
    let mut curve = curve();
    let original = curve.clone();
    curve
        .reverse_parameterization(&cadmpeg_test_support::service_decode_context())
        .expect("signed reversal admission");
    assert_eq!(curve.knots().as_slice(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(
        curve.control_points(),
        vec![original.control_points()[1], original.control_points()[0]]
    );
    assert_eq!(curve.pole_rows().weights(), Some(vec![2.0, -1.0]));
    curve
        .reverse_parameterization(&cadmpeg_test_support::service_decode_context())
        .expect("signed reversal admission");
    assert_eq!(curve, original);

    let mut pcurve = pcurve();
    let original = pcurve.clone();
    pcurve
        .reverse_parameterization(&cadmpeg_test_support::service_decode_context())
        .expect("signed reversal admission");
    assert_eq!(pcurve.knots().as_slice(), &[-5.0, -5.0, -2.0, -2.0]);
    assert_eq!(
        pcurve.control_points(),
        vec![original.control_points()[1], original.control_points()[0]]
    );
    assert_eq!(pcurve.pole_rows().weights(), Some(vec![2.0, 1.0]));
    pcurve
        .reverse_parameterization(&cadmpeg_test_support::service_decode_context())
        .expect("signed reversal admission");
    assert_eq!(pcurve, original);
}

#[test]
fn analytic_pcurve_admission_preserves_nonunit_axes_and_unordered_radii() {
    let origin = Point2::new(0.0, 0.0);
    let x = Point2::new(2.0, 0.0);
    let y = Point2::new(1.0, 3.0);
    assert!(CirclePcurve::try_new(origin, x, y, 1.0).is_ok());
    assert!(EllipsePcurve::try_new(origin, x, y, 1.0, 2.0).is_ok());
    assert!(EllipseCurve::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        1.0,
        2.0,
    )
    .is_err());
    assert!(LinePcurve::try_new(origin, origin).is_err());
    assert!(HarmonicPcurve::try_new(origin, origin, x).is_ok());
    assert!(HarmonicPcurve::try_new(origin, origin, origin).is_err());
    assert!(SphericalGreatCirclePcurve::try_new(0.0, 1e-200, 0.0, 0.0).is_ok());
    assert!(SphericalGreatCirclePcurve::try_new(0.0, 0.0, 0.0, 0.0).is_err());
    let line = PcurveGeometry::Line(LinePcurve::try_new(origin, x).unwrap());
    assert!(TrimmedPcurve::try_new([2.0, 1.0], true, Box::new(line.clone())).is_err());
    assert!(TrimmedPcurve::try_new([1.0, 1.0], false, Box::new(line.clone())).is_ok());
    assert!(OffsetPcurve::try_new(f64::INFINITY, Box::new(line.clone())).is_err());
    let offset = PcurveGeometry::Offset(OffsetPcurve::try_new(-2.0, Box::new(line)).unwrap());
    let mut wire = serde_json::to_value(offset).unwrap();
    wire["basis"]["direction"] = serde_json::json!({"u": 0.0, "v": 0.0});
    assert!(serde_json::from_value::<PcurveGeometry>(wire).is_err());
}

#[test]
fn signed_reversal_refuses_every_pass_before_any_carrier_changes() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn check<T: Clone + PartialEq + std::fmt::Debug>(
        original: &T,
        pole_count: usize,
        knot_count: usize,
        reverse: impl Fn(&mut T, &DecodeContext<'_>) -> Result<(), CodecError>,
    ) {
        // Admit each complete lane once; knot reversal and negation share one pass.
        let poles = u64::try_from(pole_count).expect("pole visits");
        let total = poles + u64::try_from(knot_count).expect("knot visits");
        for cap in 0..total {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut edited = original.clone();
            let Err(CodecError::ResourceLimit(limit)) = reverse(&mut edited, &ctx) else {
                panic!("every reversal pass requires admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(
                limit.operation,
                if cap < poles {
                    "IR signed pole reversal"
                } else {
                    "IR signed knot reversal"
                }
            );
            assert_eq!(&edited, original);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2 * total;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut edited = original.clone();
        reverse(&mut edited, &ctx).expect("first reversal");
        assert_ne!(&edited, original);
        reverse(&mut edited, &ctx).expect("second reversal");
        assert_eq!(&edited, original);
        ctx.finish_session().expect("exact work and zero storage");
    }

    let curve = curve();
    check(
        &curve,
        curve.pole_rows().count(),
        curve.knots().len(),
        NurbsCurve::reverse_parameterization,
    );
    let pcurve = pcurve();
    check(
        &pcurve,
        pcurve.pole_rows().count(),
        pcurve.knots().len(),
        PcurveNurbs::reverse_parameterization,
    );
}

#[test]
fn pole_mapping_admits_both_passes_before_callbacks_and_preserves_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;

    macro_rules! check {
        ($original:expr, $count:expr) => {{
            let original = $original;
            let count = u64::try_from($count).expect("pole count");
            for cap in 0..2 * count {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let mut edited = original.clone();
                let called = Cell::new(false);
                let Err(CodecError::ResourceLimit(limit)) = edited.try_map_control_points(|_, point| {
                    called.set(true);
                    Ok::<_, ()>(point.negated())
                }, &ctx) else { panic!("both passes require admission"); };
                assert!(!called.get());
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, if cap < count { "IR pole edit validation" } else { "IR pole edit mutation" });
                assert_eq!(edited, original);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
            }
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 4 * count;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut edited = original.clone();
            let calls = Cell::new(0_u64);
            for _ in 0..2 {
                edited.try_map_control_points(|_, point| {
                    calls.set(calls.get() + 1);
                    Ok::<_, ()>(point.negated())
                }, &ctx).expect("exact work").expect("valid map");
            }
            assert_eq!(calls.get(), 4 * count);
            assert_eq!(edited, original);
            ctx.finish_session().expect("zero storage and exact visits");

            let arena = DecodeArena::new();
            policy.limits.max_work_units = 2 * count;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut edited = original.clone();
            assert_eq!(edited.try_map_control_points(|index, point| {
                if u64::try_from(index).expect("index") + 1 == count { Err("last pole") } else { Ok(point.negated()) }
            }, &ctx).expect("admission"), Err("last pole"));
            assert_eq!(edited, original);
            ctx.finish_session().expect("semantic refusal stays distinct");
        }};
    }
    check!(curve(), curve().pole_count());
    check!(surface(), surface().u_count() * surface().v_count());
    check!(pcurve(), pcurve().pole_rows().count());
    let ctx = cadmpeg_test_support::service_decode_context();
    check!(
        NurbsCurve::from_lanes(
            &ctx,
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(1., 2., 3.); 2],
            None,
            false
        )
        .expect("admission")
        .expect("curve"),
        2_usize
    );
    check!(
        NurbsSurface::from_checked_lanes(
            &ctx,
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            crate::geometry::nurbs::NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            crate::geometry::nurbs::NurbsSurfaceLanes::new(
                vec![vec![Point3::new(1., 2., 3.); 2]; 2],
                None
            ),
            false
        )
        .expect("admission")
        .expect("surface"),
        4_usize
    );
    check!(
        PcurveNurbs::from_lanes(
            &ctx,
            1,
            vec![0., 0., 1., 1.],
            vec![Point2::new(1., 2.); 2],
            None,
            false
        )
        .expect("admission")
        .expect("pcurve"),
        2_usize
    );
    check!(
        crate::geometry::nurbs::BsplineSurface::new(
            &ctx,
            1,
            1,
            vec![0., 0., 1., 1.],
            vec![0., 0., 1., 1.],
            vec![vec![Point3::new(1., 2., 3.); 2]; 2]
        )
        .expect("admission")
        .expect("B-spline"),
        4_usize
    );
    let spatial_curve = NurbsCurve::from_lanes(
        &ctx,
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(1., 2., 3.); 2],
        Some(vec![2., 1.]),
        false,
    )
    .expect("admission")
    .expect("positive-weight curve");
    check!(
        crate::sketches::SpatialSketchNurbsCurve::try_from(spatial_curve).expect("spatial curve"),
        2_usize
    );
}

#[test]
fn in_place_pcurve_mapping_admits_work_and_keeps_partial_semantic_edits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let original = pcurve();
    let count = u64::try_from(original.pole_rows().count()).expect("count");
    for cap in 0..count {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut edited = original.clone();
        let mut called = false;
        let Err(CodecError::ResourceLimit(limit)) = edited.try_map_control_points_in_place(
            |point| {
                called = true;
                Ok::<_, ()>(point.negated())
            },
            &ctx,
        ) else {
            panic!("in-place pass requires admission");
        };
        assert!(!called);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(edited, original);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = count;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut edited = original.clone();
    let mut calls = 0;
    assert_eq!(
        edited
            .try_map_control_points_in_place(
                |point| {
                    calls += 1;
                    if calls == 2 {
                        Err("second pole")
                    } else {
                        Ok(point.negated())
                    }
                },
                &ctx
            )
            .expect("work"),
        Err("second pole")
    );
    assert_eq!(
        edited.pole_rows().point_at(0),
        original
            .pole_rows()
            .point_at(0)
            .map(crate::units::FinitePoint2::negated)
    );
    assert_eq!(
        edited.pole_rows().point_at(1),
        original.pole_rows().point_at(1)
    );
    ctx.finish_session()
        .expect("partial semantic edit has no resource refusal");
}
