// SPDX-License-Identifier: Apache-2.0
//! carrier records tests.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::{
    a5_surface_stream, b2_circle_stream, b2_cone_stream, b2_construction_use_stream,
    b2_cylinder_stream, b2_edge_parameter_stream, b2_edge_parameter_stream_for,
    b2_implicit_axis_cylinder_stream, b2_nurbs_curve_stream, b2_offset_support_stream,
    b2_range_origin_cylinder_stream, b3_cylinder_stream, b3_offset_support_stream, finite,
    finite_vector, increasing, parsed_b2_nurbs_curves, SolvedSurfaceGeometry, SurfaceGeometry,
};

#[test]
fn consolidated_offset_support_parser_reads_width2_frame() {
    let offsets = crate::families::b2::records::b2_offset_supports(&b3_offset_support_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance.get(), 2.5);
}

#[test]
fn b2_edge_parameter_parser_validates_repeated_range_packet() {
    let packets = crate::families::b2::records::b2_edge_parameters(&b2_edge_parameter_stream());
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].range.endpoints(), [2.0, 7.0]);
    assert_eq!(packets[0].tolerance, finite(1.0e-6));
}

#[test]
fn b2_edge_parameter_parser_rejects_nonincreasing_ranges() {
    assert!(
        crate::families::b2::records::b2_edge_parameters(&b2_edge_parameter_stream_for(7.0, 2.0))
            .is_empty()
    );
    assert!(
        crate::families::b2::records::b2_edge_parameters(&b2_edge_parameter_stream_for(2.0, 2.0))
            .is_empty()
    );
}

#[test]
fn b2_circle_parser_reads_arc_length_parameterization() {
    let circles = crate::families::b2::records::b2_circles(&b2_circle_stream());
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].record_id, 0x1234);
    assert_eq!(circles[0].center_pair, finite_vector([4.0, -2.0]));
    assert_eq!(circles[0].radius.get(), 3.0);
    assert_eq!(circles[0].chart_shift, finite(0.0));
    assert!(circles[0].full_circle());

    let mut malformed = b2_circle_stream();
    malformed[49..57].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_circles(&malformed).is_empty());

    let mut zero_radius = b2_circle_stream();
    zero_radius[24..32].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_circles(&zero_radius).is_empty());

    let mut large = b2_circle_stream();
    let radius = 2_000_000.0_f64;
    large[24..32].copy_from_slice(&radius.to_le_bytes());
    large[40..48].copy_from_slice(&(std::f64::consts::TAU * radius).to_le_bytes());
    assert_eq!(
        crate::families::b2::records::b2_circles(&large)[0]
            .radius
            .get(),
        radius
    );

    let tiny = 1e-200_f64;
    let mut tiny_full = b2_circle_stream();
    tiny_full[24..32].copy_from_slice(&tiny.to_le_bytes());
    tiny_full[40..48].copy_from_slice(&(std::f64::consts::TAU * tiny).to_le_bytes());
    assert!(crate::families::b2::records::b2_circles(&tiny_full)[0].full_circle());

    tiny_full[40..48].copy_from_slice(&1e-10_f64.to_le_bytes());
    assert!(!crate::families::b2::records::b2_circles(&tiny_full)[0].full_circle());
}

#[test]
fn b2_cylinder_parser_reads_arc_length_carrier() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert_eq!(
        cylinders[0].u_range.endpoints(),
        [0.0, 4.0 * std::f64::consts::PI]
    );
    assert_eq!(cylinders[0].v_range.endpoints(), [-4.0, 5.0]);
    match cylinders[0].surface_geometry() {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            assert_eq!([origin.x, origin.y, origin.z], [1.0, 2.0, 3.0]);
            assert_eq!([axis.x, axis.y, axis.z], [1.0, 0.0, 0.0]);
            assert_eq!(radius, 2.0);
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    for range in [5..13, 78..86] {
        let mut malformed = b2_cylinder_stream();
        malformed[range].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
    }

    let mut large = b2_cylinder_stream();
    let radius = 2_000_000.0_f64;
    large[54..62].copy_from_slice(&radius.to_le_bytes());
    large[70..78].copy_from_slice(&(std::f64::consts::TAU * radius).to_le_bytes());
    assert!(
        matches!(crate::families::b2::records::b2_cylinders(&large)[0]
        .surface_geometry(), SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
            if { cylinder_surface.radius().get() == 2_000_000.0 })
    );

    let tiny = 1e-200_f64;
    let mut tiny_full = b2_cylinder_stream();
    tiny_full[54..62].copy_from_slice(&tiny.to_le_bytes());
    tiny_full[70..78].copy_from_slice(&(std::f64::consts::TAU * tiny).to_le_bytes());
    assert_eq!(
        crate::families::b2::records::b2_cylinders(&tiny_full)[0]
            .radius
            .get(),
        tiny
    );

    tiny_full[70..78].copy_from_slice(&1e-10_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cylinders(&tiny_full).is_empty());
}

#[test]
fn analytic_point_lifts_bound_tiny_parameter_domains_by_span() {
    let tiny = 1e-200_f64;
    let mut cylinder = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream()).remove(0);
    cylinder.u_range = cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, tiny])
        .expect("increasing u range");
    cylinder.v_range = cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, tiny])
        .expect("increasing v range");
    assert!(crate::families::b2::records::b2_cylinder_point(&cylinder, [tiny, tiny]).is_some());
    assert!(
        crate::families::b2::records::b2_cylinder_point(&cylinder, [2.0 * tiny, tiny]).is_none()
    );
    assert!(
        crate::families::b2::records::b2_cylinder_point(&cylinder, [tiny, 2.0 * tiny]).is_none()
    );

    let mut cone = crate::families::b2::records::b2_cones(&b2_cone_stream()).remove(0);
    cone.slant_range = cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, tiny])
        .expect("increasing slant range");
    assert!(crate::families::b2::records::b2_cone_point(&cone, [0.0, tiny]).is_some());
    assert!(crate::families::b2::records::b2_cone_point(&cone, [0.0, 2.0 * tiny]).is_none());
}

#[test]
fn analytic_point_lifts_admit_finite_parameters_across_wide_domains() {
    let wide = cadmpeg_ir::topology::IncreasingParameterInterval::new([-f64::MAX, f64::MAX])
        .expect("finite wide range");
    let mut cylinder = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream()).remove(0);
    cylinder.u_range = wide;
    cylinder.v_range = wide;
    assert!(
        crate::families::b2::records::b2_cylinder_point(&cylinder, [0.0, 0.0])
            .is_some_and(|point| point.is_finite())
    );

    let mut cone = crate::families::b2::records::b2_cones(&b2_cone_stream()).remove(0);
    cone.slant_range = wide;
    assert!(
        crate::families::b2::records::b2_cone_point(&cone, [0.0, 0.0])
            .is_some_and(|point| point.is_finite())
    );
}

#[test]
fn b2_circle_turn_checks_preserve_finite_wide_radius_ranges() {
    let radius = 4.0e307;
    let half_turn_range = [-std::f64::consts::PI * radius, 0.0];
    assert!(
        crate::families::b2::records::circle_range_is_within_full_turn(radius, half_turn_range,)
    );
    let full_turn_range = [
        -std::f64::consts::PI * radius,
        std::f64::consts::PI * radius,
    ];
    assert!(crate::families::b2::records::circle_range_is_full_turn(
        radius,
        full_turn_range,
    ));
}

#[test]
fn consolidated_cylinder_parser_reads_width2_frame() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b3_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert!(matches!(
        cylinders[0].layout,
        crate::families::b2::records::B2CylinderLayout::Full5a { .. }
    ));
    assert!(matches!(
        cylinders[0].surface_geometry(),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
    ));
}

#[test]
fn consolidated_frame_width_and_flag_are_independent() {
    let mut width1_flag13 = b2_cylinder_stream();
    width1_flag13[1] = 0x13;
    let mut width2_flag83 = b3_cylinder_stream();
    width2_flag83[1] = 0x83;
    assert_eq!(
        crate::families::b2::records::b2_cylinders(&width1_flag13).len(),
        1
    );
    assert_eq!(
        crate::families::b2::records::b2_cylinders(&width2_flag83).len(),
        1
    );
}

#[test]
fn b2_cylinder_parser_reads_implicit_axis_layout() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_implicit_axis_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert!(matches!(
        cylinders[0].layout,
        crate::families::b2::records::B2CylinderLayout::Full52
    ));
    assert!(
        matches!(cylinders[0].surface_geometry(), SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
        if {
            let axis = cylinder_surface.frame().axis().as_raw();
            [axis.x, axis.y, axis.z] == [1.0, 0.0, 0.0]
        })
    );

    let mut malformed = b2_implicit_axis_cylinder_stream();
    malformed[70..78].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
}

#[test]
fn b2_cylinder_parser_admits_the_full_stored_pair_tolerance_band() {
    let deviation = 6.0e-10_f64;
    let component = 1.0 + deviation;
    let squared_deviation = (component * component - 1.0).abs();
    assert!(
        squared_deviation > 1.0e-9,
        "the pair is inside the stored-length band and outside the squared-length one"
    );

    let mut stream = b2_cylinder_stream();
    stream[30..38].copy_from_slice(&component.to_le_bytes());
    stream[38..46].copy_from_slice(&0.0_f64.to_le_bytes());
    let cylinders = crate::families::b2::records::b2_cylinders(&stream);
    let [cylinder] = cylinders.as_slice() else {
        panic!("one B2 cylinder with a stored pair at the tolerance edge")
    };
    assert_eq!(cylinder.frame.axis().get(), [component, 0.0, 0.0]);
    assert_eq!(cylinder.frame.reference().get(), [-0.0, component, 0.0]);

    let mut quarter_turned = b2_cylinder_stream();
    quarter_turned[29] = 0x1c;
    quarter_turned[30..38].copy_from_slice(&component.to_le_bytes());
    quarter_turned[38..46].copy_from_slice(&0.0_f64.to_le_bytes());
    let cylinders = crate::families::b2::records::b2_cylinders(&quarter_turned);
    let [cylinder] = cylinders.as_slice() else {
        panic!("one quarter-turned B2 cylinder at the tolerance edge")
    };
    assert_eq!(cylinder.frame.axis().get(), [0.0, -component, 0.0]);
    assert_eq!(cylinder.frame.reference().get(), [component, 0.0, 0.0]);

    let mut range_origin = b2_range_origin_cylinder_stream();
    range_origin[30..38].copy_from_slice(&0.0_f64.to_le_bytes());
    range_origin[38..46].copy_from_slice(&component.to_le_bytes());
    let cylinders = crate::families::b2::records::b2_cylinders(&range_origin);
    let [cylinder] = cylinders.as_slice() else {
        panic!("one range-origin B2 cylinder at the tolerance edge")
    };
    assert_eq!(cylinder.frame.axis().get(), [0.0, 1.0, 0.0]);
    assert_eq!(cylinder.frame.reference().get(), [0.0, 0.0, component]);

    for mut outside in [b2_cylinder_stream(), b2_range_origin_cylinder_stream()] {
        let far = 1.0 + 2.0e-9_f64;
        outside[30..38].copy_from_slice(&far.to_le_bytes());
        outside[38..46].copy_from_slice(&0.0_f64.to_le_bytes());
        assert!(crate::families::b2::records::b2_cylinders(&outside).is_empty());
    }
}

#[test]
fn b2_cylinder_parser_resolves_and_validates_partial_range_origin() {
    let cylinders = crate::families::b2::records::b2_cylinders(&b2_range_origin_cylinder_stream());
    assert_eq!(cylinders.len(), 1);
    assert!(matches!(
        cylinders[0].layout,
        crate::families::b2::records::B2CylinderLayout::RangeOrigin { stored_vector }
            if stored_vector.get() == [0.0, 1.0]
    ));
    assert!(
        matches!(cylinders[0].surface_geometry(), SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
                if {
                    let axis = cylinder_surface.frame().axis().as_raw();
        let ref_direction = cylinder_surface.frame().reference().as_raw();
                    [axis.x, axis.y, axis.z] == [0.0, 1.0, 0.0]
                        && [ref_direction.x, ref_direction.y, ref_direction.z] == [0.0, 0.0, 1.0]
                })
    );
    assert_eq!(
        cylinders[0].range_origin().map(f64::to_bits),
        Some(((0.0 + 8.0) * 0.5 - std::f64::consts::PI * 4.0).to_bits())
    );

    for range in [30..38, 46..54, 95..103] {
        let mut malformed = b2_range_origin_cylinder_stream();
        malformed[range].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(crate::families::b2::records::b2_cylinders(&malformed).is_empty());
    }
    let mut inconsistent = b2_range_origin_cylinder_stream();
    inconsistent[95..103].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cylinders(&inconsistent).is_empty());
}

#[test]
fn b2_cone_parser_reads_orthonormal_slant_chart() {
    let cones = crate::families::b2::records::b2_cones(&b2_cone_stream());
    assert_eq!(cones.len(), 1);
    assert_eq!(<[f64; 3]>::from(cones[0].apex.get()), [1.0, 2.0, 3.0]);
    assert_eq!(cones[0].frame.axis().get(), [0.0, 0.0, 1.0]);
    assert_eq!(cones[0].half_angle.get(), 0.25);
    assert_eq!(cones[0].reference_radius, finite(4.0));
    assert_eq!(
        cones[0].angular_range,
        increasing([0.5, 0.5 + std::f64::consts::PI])
    );
    assert_eq!(cones[0].slant_range.endpoints(), [2.0, 8.0]);
    assert_eq!(cones[0].angular_scale.get(), 3.0);
    assert_eq!(
        cones[0].angular_domain,
        increasing([
            0.5 - std::f64::consts::FRAC_PI_2,
            0.5 + 3.0 * std::f64::consts::FRAC_PI_2
        ])
    );

    let mut large = b2_cone_stream();
    large[141..149].copy_from_slice(&2_000_000.0_f64.to_le_bytes());
    large[149..157].copy_from_slice(&3_000_000.0_f64.to_le_bytes());
    let cones = crate::families::b2::records::b2_cones(&large);
    assert_eq!(cones[0].slant_range.endpoints(), [2.0, 2_000_000.0]);
    assert_eq!(cones[0].angular_scale.get(), 3_000_000.0);
}

#[test]
fn b2_cone_parser_accepts_and_canonicalizes_an_apex_origin() {
    let mut stream = b2_cone_stream();
    stream[133..141].copy_from_slice(&(-5e-13f64).to_le_bytes());
    let cones = crate::families::b2::records::b2_cones(&stream);
    assert_eq!(cones.len(), 1);
    assert_eq!(cones[0].slant_range.endpoints(), [0.0, 8.0]);

    stream[133..141].copy_from_slice(&(-2e-12f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());
}

#[test]
fn b2_cone_parser_rejects_a_left_handed_or_nonfinite_payload() {
    let mut stream = b2_cone_stream();
    stream[93..101].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[157..165].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[157..165].copy_from_slice(&2.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[173..181].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[101..109].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());

    let mut stream = b2_cone_stream();
    stream[149..157].copy_from_slice(&0.0_f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_cones(&stream).is_empty());
}

#[test]
fn b2_construction_use_parser_reorders_offset_domain() {
    let offsets = crate::families::b2::records::b2_offset_supports(&b2_construction_use_stream());
    assert_eq!(offsets.len(), 1);
    assert_eq!(offsets[0].support_id, 0x1234);
    assert_eq!(offsets[0].distance.get(), -2.0);
    assert_eq!(
        [
            offsets[0].u_range.endpoints(),
            offsets[0].v_range.endpoints()
        ],
        [[0.0, 4.0], [-1.0, 3.0]]
    );
}

#[test]
fn b2_offset_support_parser_rejects_nonincreasing_domains() {
    let mut direct = b2_offset_support_stream();
    direct[32..40].copy_from_slice(&0.0f64.to_le_bytes());
    assert!(crate::families::b2::records::b2_offset_supports(&direct).is_empty());

    let mut construction = b2_construction_use_stream();
    construction[26..34].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(crate::families::b2::records::b2_offset_supports(&construction).is_empty());
}

/// The offset domain is two increasing intervals, so a record cannot hold a
/// non-increasing domain and the binding does not test for one.
#[test]
fn an_offset_support_holds_only_increasing_domains_and_binds_them() {
    use cadmpeg_ir::topology::IncreasingParameterInterval;

    let interval = |range: [f64; 2]| {
        IncreasingParameterInterval::new(range).expect("fixture interval is finite and increasing")
    };
    let offset = crate::families::b2::records::B2OffsetSupport {
        pos: 0,
        support_id: 1,
        distance: cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite distance"),
        u_range: interval([0.0, 1.0]),
        v_range: interval([0.0, 1.0]),
    };
    let carriers = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("service decode")
    });
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::b2::records::offset_support_carriers(ctx, &[offset], &carriers)
                .expect("service decode")
        }),
        [Some(0)]
    );
    assert!(IncreasingParameterInterval::new([0.0, 0.0]).is_none());
}

#[test]
fn b2_offset_support_binding_refuses_collection_and_work_limits() {
    let offsets = crate::families::b2::records::b2_offset_supports(&b2_offset_support_stream());
    let carriers = crate::test_support::with_service_context(|ctx| {
        crate::families::a5a8::records::a5_surfaces(
            ctx,
            &a5_surface_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service profile admits surface");
    let collection = crate::test_support::with_collection_limit(0, |ctx| {
        crate::families::b2::records::offset_support_carriers(ctx, &offsets, &carriers)
    });
    assert!(
        matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_b2_offset_bindings")
    );
    let work = crate::test_support::with_work_limit(0, |ctx| {
        crate::families::b2::records::offset_support_carriers(ctx, &offsets, &carriers)
    });
    assert!(
        matches!(work, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_b2_offset_carrier_scan")
    );
    let service = crate::test_support::with_service_context(|ctx| {
        crate::families::b2::records::offset_support_carriers(ctx, &offsets, &carriers)
    })
    .expect("service profile admits offset binding");
    assert_eq!(service.len(), 1);
}

#[test]
fn b2_offset_support_parser_ignores_other_construction_kinds() {
    let mut record = b2_construction_use_stream();
    record[17] = 0x19;

    assert!(crate::families::b2::records::b2_offset_supports(&record).is_empty());
}

#[test]
fn b2_nurbs_curve_parser_preserves_asymmetric_weights_in_source_order() {
    let curves = parsed_b2_nurbs_curves(&b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]));
    let [curve] = curves.as_slice() else {
        panic!("one rational curve");
    };
    assert_eq!(curve.geometry.degree(), 3);
    assert_eq!(curve.geometry.control_points().len(), 4);
    let weights = curve.geometry.pole_rows().weights();
    assert_eq!(weights, Some(vec![1.0, 0.72, 1.31, 0.93]));
    assert_eq!(curve.geometry.knots().len(), 8);
    assert_eq!(curve.geometry.knots()[..4], [0.0; 4]);
    assert_eq!(curve.geometry.knots()[4..], [41.693_759_535_8; 4]);
}

#[test]
fn b2_nurbs_control_points_refuse_collection_limit_before_materialization() {
    assert_b2_nurbs_collection_refusal(3, "catia_b2_nurbs_control_points");
}

#[test]
fn b2_nurbs_weights_refuse_collection_limit_before_materialization() {
    assert_b2_nurbs_collection_refusal(7, "catia_b2_nurbs_weights");
}

#[test]
fn b2_nurbs_knots_refuse_collection_limit_before_materialization() {
    assert_b2_nurbs_collection_refusal(15, "catia_b2_nurbs_knots");
}

#[test]
fn b2_nurbs_curves_refuse_collection_limit_before_retention() {
    assert_b2_nurbs_collection_refusal(16, "IR NURBS paired poles");
    assert_b2_nurbs_collection_refusal(20, "catia_b2_nurbs_curves");
}

fn assert_b2_nurbs_collection_refusal(limit: u64, operation: &'static str) {
    let bytes = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    let result = crate::test_support::with_collection_limit(limit, |ctx| {
        crate::families::b2::records::b2_nurbs_curves(ctx, &bytes)
    });
    assert!(matches!(result,
        Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == operation
    ));
    assert_eq!(parsed_b2_nurbs_curves(&bytes).len(), 1);
}

#[test]
fn b2_nurbs_curve_parser_rejects_broken_frame_invariants() {
    let valid = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    for offset in [6, 7, 8, 16, 24, 153, 154, 155, 163, 171, 179, 187, 188] {
        let mut broken = valid.clone();
        broken[offset] ^= 1;
        assert!(
            parsed_b2_nurbs_curves(&broken).is_empty(),
            "offset {offset}"
        );
    }
    let mut nonpositive_weight = valid;
    nonpositive_weight[5 + 3 + 16 + 1 + 4 * 24..5 + 3 + 16 + 1 + 4 * 24 + 8]
        .copy_from_slice(&0.0f64.to_le_bytes());
    assert!(parsed_b2_nurbs_curves(&nonpositive_weight).is_empty());
}

#[test]
fn b2_nurbs_curve_parser_rejects_nonfinite_knots_poles_and_weights() {
    let mut nonfinite_knot = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    nonfinite_knot[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
    nonfinite_knot[155..163].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parsed_b2_nurbs_curves(&nonfinite_knot).is_empty());

    let mut nonfinite_pole = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    nonfinite_pole[25..33].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parsed_b2_nurbs_curves(&nonfinite_pole).is_empty());

    let mut infinite_weight = b2_nurbs_curve_stream([1.0, 0.72, 1.31, 0.93]);
    infinite_weight[121..129].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(parsed_b2_nurbs_curves(&infinite_weight).is_empty());
}
