// SPDX-License-Identifier: Apache-2.0
//! rechart cases tests.

use super::{
    foreign_plane_chart_sites, freeform_surface_carriers, pcurve_lift_reaches_endpoints,
    solve_planar_chart_rechart, with_admission, AnnotationBuilder, CadIr, ConsolidatedCarrierChart,
    PcurveGeometry, Point2, Point3, SolvedSurfaceGeometry, SurfaceGeometry, Vector3,
};

#[test]
fn planar_rechart_refuses_before_absent_chart_candidate() {
    let (target, stored, loci) = foreign_plane_chart_sites(0.4, [1.0, 2.0]);
    let scaled = stored
        .iter()
        .map(|[u, v]| [*u * 1.5, *v])
        .collect::<Vec<_>>();
    assert!(solve_planar_chart_rechart(&scaled, &loci, &target).is_none());
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::super::solve_planar_chart_rechart(ctx, &scaled, &loci, &target)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_chart_images")
    );
}

#[test]
fn planar_rechart_recovers_a_foreign_consolidated_chart() {
    let angle = 0.7;
    let shift = [12.5, -4.25];
    let (target, stored, loci) = foreign_plane_chart_sites(angle, shift);
    let chart = solve_planar_chart_rechart(&stored, &loci, &target)
        .expect("an isometric stored chart recharts onto the target plane");
    for (site, locus) in stored.iter().zip(&loci) {
        let [u, v] = chart.point(*site);
        let lifted = cadmpeg_ir::eval::surface_point(&target, u, v).expect("plane evaluates");
        assert!(
            (lifted.x - locus.x)
                .hypot(lifted.y - locus.y)
                .hypot(lifted.z - locus.z)
                < 1.0e-9,
            "recharted site must lift onto its definition locus"
        );
    }
    // The naive binding this replaces reads the stored chart as the
    // target's own, which lands far from the definition loci.
    let naive = ConsolidatedCarrierChart::Identity;
    let [u, v] = naive.point(stored[0]);
    let lifted = cadmpeg_ir::eval::surface_point(&target, u, v).expect("plane evaluates");
    assert!(
        (lifted.x - loci[0].x)
            .hypot(lifted.y - loci[0].y)
            .hypot(lifted.z - loci[0].z)
            > 1.0,
        "the unrecharted stored chart must not be mistaken for the target chart"
    );
    // The linear part carries derivatives without the translation.
    let derivative = chart.derivative([1.0, 0.0]);
    assert!(
        (derivative[0].hypot(derivative[1]) - 1.0).abs() < 1.0e-12,
        "an isometry preserves derivative magnitude"
    );
}

#[test]
fn planar_rechart_declines_a_chart_that_is_not_an_isometry() {
    let (target, stored, loci) = foreign_plane_chart_sites(0.4, [1.0, 2.0]);
    // Scale one chart axis. No rigid motion reproduces the sites, so no
    // binding may be claimed.
    let scaled = stored
        .iter()
        .map(|[u, v]| [*u * 1.5, *v])
        .collect::<Vec<_>>();
    assert!(solve_planar_chart_rechart(&scaled, &loci, &target).is_none());
    // Loci off the plane have no image in its chart.
    let lifted_loci = loci
        .iter()
        .map(|locus| Point3::new(locus.x + 4.0, locus.y, locus.z))
        .collect::<Vec<_>>();
    assert!(solve_planar_chart_rechart(&stored, &lifted_loci, &target).is_none());
    // A non-plane target has no affine chart to solve against.
    assert!(solve_planar_chart_rechart(
        &stored,
        &loci,
        &SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None })
    )
    .is_none());
    // Two sites leave both orientation choices valid. Three collinear
    // sites have the same ambiguity, so neither admits a unique chart.
    assert!(solve_planar_chart_rechart(&stored[..2], &loci[..2], &target).is_none());
    let collinear_sites = [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]];
    let collinear_loci = collinear_sites
        .iter()
        .map(|[u, v]| {
            cadmpeg_ir::eval::surface_point(&target, *u, *v)
                .expect("plane")
                .get()
        })
        .collect::<Vec<_>>();
    assert!(solve_planar_chart_rechart(&collinear_sites, &collinear_loci, &target).is_none());
}

#[test]
fn endpoint_lift_witness_refuses_a_pcurve_from_a_foreign_chart() {
    let (target, stored, loci) = foreign_plane_chart_sites(0.9, [-6.0, 3.5]);
    let endpoints = [*loci.first().expect("sites"), *loci.last().expect("sites")];
    let range = [0.0, 1.0];
    let line_through = |first: [f64; 2], last: [f64; 2]| PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![range[0], range[0], range[1], range[1]],
            vec![
                Point2::new(first[0], first[1]),
                Point2::new(last[0], last[1]),
            ],
            None,
            false,
        )
        .expect("valid endpoint witness pcurve"),
    };
    let chart = solve_planar_chart_rechart(&stored, &loci, &target).expect("isometry");
    let first = *stored.first().expect("sites");
    let last = *stored.last().expect("sites");
    let recharted = line_through(chart.point(first), chart.point(last));
    assert!(
        crate::test_support::with_service_context(|ctx| pcurve_lift_reaches_endpoints(
            ctx,
            &recharted,
            target.solved().expect("solved carrier"),
            range,
            endpoints,
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ))
        .expect("evaluator allocation succeeds"),
        "the recharted pcurve lifts onto the edge's vertex positions"
    );
    let naive = line_through(first, last);
    assert!(
        !crate::test_support::with_service_context(|ctx| pcurve_lift_reaches_endpoints(
            ctx,
            &naive,
            target.solved().expect("solved carrier"),
            range,
            endpoints,
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ))
        .expect("evaluator allocation succeeds"),
        "a pcurve stored in a foreign chart has no witness on this carrier"
    );
    // The witness is independent of endpoint order.
    assert!(
        crate::test_support::with_service_context(|ctx| pcurve_lift_reaches_endpoints(
            ctx,
            &recharted,
            target.solved().expect("solved carrier"),
            range,
            [endpoints[1], endpoints[0]],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ))
        .expect("evaluator allocation succeeds")
    );
    // A carrier with no geometry has no chart and admits no witness.
    assert!(
        !crate::test_support::with_service_context(|ctx| pcurve_lift_reaches_endpoints(
            ctx,
            &naive,
            &SolvedSurfaceGeometry::Unknown { record: None },
            range,
            endpoints,
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        ))
        .expect("evaluator allocation succeeds")
    );
}

#[test]
fn freeform_fallback_retains_exact_consolidated_spheres() {
    let bytes = crate::test_support::test_b2::b2_sphere_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let carriers = crate::test_support::with_service_context(|ctx| {
        freeform_surface_carriers(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert!(matches!(carriers.as_slice(), [carrier]
            if matches!(carrier.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))
            if {
                let center = sphere_surface.center().get();
    let axis = sphere_surface.frame().axis().as_raw();
    let ref_direction = sphere_surface.frame().reference().as_raw();
                (sphere_surface.radius().get() == 5.0)
                    && (center == Point3::new(1.0, 2.0, 3.0)
                        && *axis == Vector3::new(0.0, 0.0, 1.0)
                        && *ref_direction == Vector3::new(1.0, 0.0, 0.0))
            })));
}

#[test]
fn freeform_surface_carrier_refuses_collection_limit() {
    let bytes = crate::test_support::test_b2::b2_sphere_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        freeform_surface_carriers(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_freeform_surface_carriers")
    );
    let service = crate::test_support::with_service_context(|ctx| {
        freeform_surface_carriers(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits sphere carrier");
    assert_eq!(service.len(), 1);
}

#[test]
fn freeform_fallback_carrier_identity_refuses_retained_limit() {
    let file = crate::test_support::test_container::outer_body_catpart(
        &crate::test_support::test_b2::b2_sphere_stream(),
    );
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, file.clone())
    })
    .expect("service scan");
    let service = crate::test_support::with_service_context(|ctx| {
        super::super::try_decode_freeform_surfaces(
            ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service budget");
    assert!(service.is_some());
    let mut found = false;
    for cap in 0..4096 {
        let limited = crate::test_support::with_retained_limit(cap, |ctx| {
            super::super::try_decode_freeform_surfaces(
                ctx,
                &scan,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        match limited {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_freeform_payload_id" =>
            {
                found = true;
                break;
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(_)) => {}
            _ => panic!("fallback carrier passed without its identity refusal"),
        }
    }
    assert!(found, "the retained sweep must reach the payload identity");
}

#[test]
fn freeform_fallback_retains_exact_consolidated_tori() {
    let bytes = crate::test_support::test_b2::b2_torus_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let carriers = crate::test_support::with_service_context(|ctx| {
        freeform_surface_carriers(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert!(matches!(carriers.as_slice(), [carrier]
            if matches!(carrier.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))
            if {
                let center = torus_surface.center().get();
    let axis = torus_surface.frame().axis().as_raw();
    let ref_direction = torus_surface.frame().reference().as_raw();
                (torus_surface.major_radius().get() == 7.0)
                    && (torus_surface.minor_radius().get() == 2.0)
                    && (center == Point3::new(1.0, 2.0, 3.0)
                        && *axis == Vector3::new(0.0, 0.0, 1.0)
                        && *ref_direction == Vector3::new(1.0, 0.0, 0.0))
            })));
}

/// A cone record that is read builds its carrier. The frame witness has a
/// componentwise cross-product deviation of `1e-9` and `|t1·axis| ≈ 1.41e-9`;
/// the overflow witness has a finite apex whose carrier origin, the axis
/// point at the slant start, is not finite. The record read refuses both, so
/// the freeform carriers are built without them.
#[test]
fn a_cone_record_is_refused_when_read_or_builds_its_freeform_carrier() {
    let set = |stream: &mut Vec<u8>, index: usize, values: &[f64]| {
        for (offset, value) in values.iter().enumerate() {
            let start = 5 + 8 * (index + offset);
            stream[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
    };
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut frame_witness = crate::test_support::test_b2::b2_cone_stream();
    set(
        &mut frame_witness,
        3,
        &[s, s, 0.0, -s, s, 0.0, 1.0e-9, 1.0e-9, 1.0],
    );
    let mut overflow_witness = crate::test_support::test_b2::b2_cone_stream();
    set(&mut overflow_witness, 0, &[f64::MAX, 0.0, 0.0]);
    set(
        &mut overflow_witness,
        3,
        &[0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
    );
    set(&mut overflow_witness, 16, &[1.0e308, 1.5e308]);
    for bytes in [frame_witness, overflow_witness] {
        let records = crate::wire::records::consolidated_records(&bytes);
        let carriers = crate::test_support::with_service_context(|ctx| {
            freeform_surface_carriers(
                ctx,
                &bytes,
                &records,
                &mut crate::nurbs::LaneRefusals::new(),
            )
            .expect("service decode")
        });
        assert!(carriers.is_empty());
        assert!(
            crate::families::b2::records::b2_cones_from_records(&bytes, &records)
                .next()
                .is_none()
        );
    }
}

#[test]
fn a_revolution_frame_admitted_when_read_converts_to_a_torus_without_a_second_axis_test() {
    // A right-handed frame whose profile-circle normal deviates from the
    // axis cross product by 0.9e-12 in each component. The record's
    // frame admission holds it, and the normal meets the axis at about
    // 1.56e-12, above 1e-12.
    let (a, b) = (std::f64::consts::FRAC_1_SQRT_2, 1.0 / 3.0_f64.sqrt());
    let deviation = 0.9e-12;
    let axis = [b, b, b];
    let direction_y = [a, -a, 0.0];
    let direction_x = [
        -a * b + deviation,
        -a * b + deviation,
        2.0 * a * b + deviation,
    ];
    let normal_meets_axis =
        direction_x[0] * axis[0] + direction_x[1] * axis[1] + direction_x[2] * axis[2];
    assert!(normal_meets_axis > 1.0e-12 && normal_meets_axis < 2.0e-12);

    let mut bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
    let frame_start = bytes.len() - 0xae + 3;
    let frame = [[0.0; 3], direction_x, direction_y, axis].concat();
    for (index, value) in frame.into_iter().enumerate() {
        let at = frame_start + 8 * index;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    let records = crate::wire::records::consolidated_records(&bytes);
    let resolved = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("service decode");
    assert_eq!(resolved.len(), 1);

    let bindings = with_admission(|admission| {
        super::super::append_consolidated_revolutions(
            &mut CadIr::empty(),
            &mut AnnotationBuilder::<()>::default(),
            &resolved,
            admission,
        )
    })
    .expect("service limits admit freeform model records");
    let [binding] = bindings.as_slice() else {
        panic!("the admitted revolution converts to one torus");
    };
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) = &binding.geometry else {
        panic!("the conversion is a torus");
    };
    assert!((torus.major_radius().get() - 4.0).abs() <= 1.0e-9);
    assert_eq!(torus.minor_radius().get(), 3.0);
}

#[test]
fn a_torus_reference_tilted_off_the_axis_by_rounding_keeps_the_perpendicularity_refusal() {
    // The record admits an axis whose squared length is 1 + 9.8e-13, a
    // frame whose second direction crosses the axis to the first within
    // 4.9e-13, and an axis origin 1e5 along the axis. The torus
    // reference is the radial part of the profile-center offset over
    // its length: the axial part leaves 2·4.9e-13·1e5 ≈ 9.8e-8 of radial
    // length along the axis, so the reference meets the axis at about
    // 2.45e-8, above the 1e-9 of OrthonormalFrame3::from_units. Every
    // other condition of the conversion holds. With an exact unit axis
    // the same revolution converts.
    let deviation = 4.9e-13;
    let convert = |axis_z: f64| {
        let mut bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
        let frame_start = bytes.len() - 0xae + 3;
        let frame = [
            [0.0, 0.0, 1.0e5],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, axis_z],
        ]
        .concat();
        for (index, value) in frame.into_iter().enumerate() {
            let at = frame_start + 8 * index;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        let records = crate::wire::records::consolidated_records(&bytes);
        let resolved = crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::b2_resolved_revolutions_from_records(
                ctx, &bytes, &records,
            )
        })
        .expect("service decode");
        assert_eq!(resolved.len(), 1);
        with_admission(|admission| {
            super::super::append_consolidated_revolutions(
                &mut CadIr::empty(),
                &mut AnnotationBuilder::<()>::default(),
                &resolved,
                admission,
            )
        })
        .expect("service limits admit freeform model records")
    };
    assert!(convert(1.0 + deviation).is_empty());
    let bindings = convert(1.0);
    let [binding] = bindings.as_slice() else {
        panic!("the exact unit axis converts to one torus");
    };
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) = &binding.geometry else {
        panic!("the conversion is a torus");
    };
    assert_eq!(torus.major_radius().get(), 4.0);
}

#[test]
fn freeform_fallback_retains_range_origin_cylinder_carriers() {
    let bytes = crate::test_support::test_b2::b2_range_origin_cylinder_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let carriers = crate::test_support::with_service_context(|ctx| {
        freeform_surface_carriers(
            ctx,
            &bytes,
            &records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert!(matches!(carriers.as_slice(), [carrier]
            if matches!(carrier.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
            if {
                let origin = cylinder_surface.origin().get();
    let axis = cylinder_surface.frame().axis().as_raw();
    let ref_direction = cylinder_surface.frame().reference().as_raw();
                (cylinder_surface.radius().get() == 4.0)
                    && (origin == Point3::new(0.0, 0.0, 0.0)
                        && *axis == Vector3::new(0.0, 1.0, 0.0)
                        && *ref_direction == Vector3::new(0.0, 0.0, 1.0))
            })));
}

#[test]
fn large_planar_sites_recover_the_identity_chart() {
    use cadmpeg_ir::{
        geometry::{analytic::PlaneSurface, SolvedSurfaceGeometry, SurfaceGeometry},
        math::{Point3, Vector3},
    };
    let target = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .expect("orthonormal target plane"),
    ));
    for (a, origin) in [(1., 0.), (1e200, 0.), (1., 1e10)] {
        let sites = [
            [origin + a, origin],
            [origin, origin + a],
            [origin - a, origin],
            [origin, origin - a],
        ];
        let loci = sites.map(|p| Point3::new(p[0], p[1], 0.));
        let chart = solve_planar_chart_rechart(&sites, &loci, &target)
            .expect("planar chart for the four sites");
        for point in sites {
            assert_eq!(chart.point(point), point);
        }
    }
}

/// A unit-radius cone about +Z whose cross-section radius overflows at
/// v = 1e308, and the pcurve from its overflowing section at t = 0 to
/// its unit circle at t = 1.
fn overflowing_cone_lift() -> (SurfaceGeometry, PcurveGeometry) {
    (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
                1.0,
                1.5,
            )
            .expect("valid ConeSurface fixture"),
        )),
        PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 1.0e308),
                Point2::new(0.0, -1.0e308),
            )
            .expect("valid LinePcurve fixture"),
        ),
    )
}

#[test]
fn standard_carrier_endpoint_loci_keep_an_overflowing_lift() {
    let (cone, pcurve) = overflowing_cone_lift();
    let loci = crate::test_support::with_service_context(|ctx| {
        super::super::standard_carrier_endpoint_loci(ctx, &pcurve, &cone, [0.0, 1.0])
    })
    .expect("evaluator allocation succeeds")
    .expect("both ends lift");
    assert!(!loci[0].is_finite());
    assert_eq!(loci[1], Point3::new(1.0, 0.0, 0.0));
}

#[test]
fn a_pcurve_lift_with_an_overflowing_end_is_measured_at_its_finite_end() {
    let (cone, pcurve) = overflowing_cone_lift();
    assert!(
        crate::test_support::with_service_context(|ctx| pcurve_lift_reaches_endpoints(
            ctx,
            &pcurve,
            cone.solved().expect("solved carrier"),
            [0.0, 1.0],
            [Point3::new(5.0, 5.0, 5.0), Point3::new(1.0, 0.0, 0.0)],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE,
        ))
        .expect("evaluator allocation succeeds")
    );
}

/// The overflowing cone lift with the cone under the identity placement.
fn placed_overflowing_cone_lift() -> (SurfaceGeometry, PcurveGeometry) {
    let (cone, pcurve) = overflowing_cone_lift();
    let SurfaceGeometry::Solved(cone) = cone else {
        panic!("the cone fixture is solved");
    };
    (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
            cadmpeg_ir::geometry::PlacedSurface::try_new(
                Box::new(cone),
                cadmpeg_ir::transform::Transform::identity(),
            )
            .expect("valid PlacedSurface fixture"),
        )),
        pcurve,
    )
}

#[test]
fn standard_carrier_endpoint_loci_keep_an_overflowing_placed_lift() {
    let (cone, pcurve) = placed_overflowing_cone_lift();
    let loci = crate::test_support::with_service_context(|ctx| {
        super::super::standard_carrier_endpoint_loci(ctx, &pcurve, &cone, [0.0, 1.0])
    })
    .expect("evaluator allocation succeeds")
    .expect("both ends lift");
    assert!(!loci[0].is_finite());
    assert_eq!(loci[1], Point3::new(1.0, 0.0, 0.0));
}

#[test]
fn a_pcurve_lift_with_an_overflowing_placed_end_is_measured_at_its_finite_end() {
    let (cone, pcurve) = placed_overflowing_cone_lift();
    assert!(
        crate::test_support::with_service_context(|ctx| pcurve_lift_reaches_endpoints(
            ctx,
            &pcurve,
            cone.solved().expect("solved carrier"),
            [0.0, 1.0],
            [Point3::new(5.0, 5.0, 5.0), Point3::new(1.0, 0.0, 0.0)],
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE,
        ))
        .expect("evaluator allocation succeeds")
    );
}

#[test]
fn standard_carrier_endpoints_refuse_caller_depth() {
    let (cone, pcurve) = overflowing_cone_lift();
    crate::test_support::with_depth_limit(0, |ctx| {
        let error = super::super::standard_carrier_endpoint_loci(ctx, &pcurve, &cone, [0.0, 1.0])
            .expect_err("carrier evaluation exceeds caller depth");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::RecursionDepth
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}
