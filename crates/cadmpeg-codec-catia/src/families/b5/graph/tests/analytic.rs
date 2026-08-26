use super::super::*;
use super::*;

#[test]
fn circle_pcurve_rejects_unbounded_subdivision_counts() {
    let mut payload = vec![0x81, 0x81];
    payload.extend_from_slice(&[0; 16]);
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [1.0e-300, 0.0, 1.0, 1.0, 0.0] {
        payload.extend_from_slice(&f64::to_le_bytes(value));
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x19,
        object_id: 0,
        payload,
    };
    assert!(parse_circle_pcurve(&record).is_none());
}

#[test]
fn sparse_reference_tokens_fill_selected_id_bytes() {
    let mut position = 0;
    assert_eq!(
        wire::object_ref(&[0x28, 0x34, 0x02], &mut position, true),
        Some(0x02_0034)
    );
    assert_eq!(position, 3);
    position = 0;
    assert_eq!(
        wire::object_ref(&[0x20, 0x07], &mut position, true),
        Some(0x07_0000)
    );
    assert_eq!(position, 2);
    position = 0;
    assert_eq!(wire::object_ref(&[0x8b], &mut position, true), Some(11));
    assert_eq!(position, 1);
}

#[test]
fn counted_cardinality_widens_with_reference_tokens() {
    let mut position = 0;
    assert_eq!(counted_cardinality(&[0x81], &mut position), Some(1));
    assert_eq!(position, 1);
    position = 0;
    assert_eq!(counted_cardinality(&[0x08, 0x81], &mut position), Some(129));
    assert_eq!(position, 2);
    position = 0;
    assert_eq!(
        counted_cardinality(&[0x18, 0x35, 0x01], &mut position),
        Some(309)
    );
    assert_eq!(position, 3);
}

#[test]
fn revolution_surface_requires_complete_sparse_reference_chart() {
    let mut payload = vec![0; 175];
    payload[0] = 0x81;
    payload[1..4].copy_from_slice(&[0x30, 0x86, 0x16]);
    payload[4..12].copy_from_slice(&1.0f64.to_le_bytes());
    payload[28..36].copy_from_slice(&1.0f64.to_le_bytes());
    payload[60..68].copy_from_slice(&1.0f64.to_le_bytes());
    payload[92..100].copy_from_slice(&1.0f64.to_le_bytes());
    for (offset, value) in [
        (100, 0.0f64),
        (108, 2.0 * std::f64::consts::PI),
        (116, -1.0),
        (124, 1.0),
        (134, 2.0),
        (142, 1.0),
        (150, 1.0),
        (158, 0.0),
        (167, 2.0 * std::f64::consts::PI),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[132..134].copy_from_slice(&[0x05, 0x05]);
    payload[166] = 0x01;
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2d,
        object_id: 0x16_8601,
        payload,
    };
    assert_eq!(
        parse_surface(&record),
        Some(B5Surface::Revolution {
            profile_curve: 0x16_8600,
            axis_origin: [1.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            reference_y: [0.0, 1.0, 0.0],
            axis_direction: [0.0, 0.0, 1.0],
            profile_range: [-1.0, 1.0],
            angular_range: [0.0, 2.0 * std::f64::consts::PI],
            angular_scale: 2.0,
        })
    );
    let mut left_handed = record.clone();
    left_handed.payload[92..100].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(parse_surface(&left_handed), None);
    let mut wrong_lead = record.clone();
    wrong_lead.payload[0] = 0x80;
    assert_eq!(parse_surface(&wrong_lead), None);
    let mut wrong_half_period = record;
    wrong_half_period.payload[167..175].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(parse_surface(&wrong_half_period), None);
}

#[test]
fn line_profile_requires_its_complete_unit_metric_chart() {
    let mut payload = vec![0; 73];
    payload[0] = 0x80;
    for (offset, values) in [(1, [1.0f64, 2.0, 3.0]), (25, [0.0, 0.0, 1.0])] {
        for (index, value) in values.into_iter().enumerate() {
            payload[offset + 8 * index..offset + 8 * index + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    payload[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    payload[57..65].copy_from_slice(&(-2.0f64).to_le_bytes());
    payload[65..73].copy_from_slice(&4.0f64.to_le_bytes());
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x0e,
        object_id: 7,
        payload,
    };
    assert_eq!(
        parse_profile(&record),
        Some(B5Profile::Line {
            point: [1.0, 2.0, 3.0],
            direction: [0.0, 0.0, 1.0],
            parameter_range: [-2.0, 4.0],
        })
    );

    let mut nonunit = record.clone();
    nonunit.payload[41..49].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(parse_profile(&nonunit), None);
    let mut wrong_metric = record.clone();
    wrong_metric.payload[49..57].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(parse_profile(&wrong_metric), None);
    let mut unordered = record.clone();
    unordered.payload[65..73].copy_from_slice(&(-3.0f64).to_le_bytes());
    assert_eq!(parse_profile(&unordered), None);
    let mut wrong_lead = record;
    wrong_lead.payload[0] = 0x81;
    assert_eq!(parse_profile(&wrong_lead), None);
}

#[test]
fn arc_profile_requires_its_complete_centered_periodic_chart() {
    let mut payload = vec![0; 113];
    payload[0] = 0x80;
    for (offset, values) in [
        (1, [1.0f64, 2.0, 3.0]),
        (25, [1.0, 0.0, 0.0]),
        (49, [0.0, 1.0, 0.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            payload[offset + 8 * index..offset + 8 * index + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    let radius = 2.0f64;
    let parameter_range = [0.5 * radius, 1.5 * radius];
    let chart_origin =
        (parameter_range[0] + parameter_range[1]) * 0.5 - std::f64::consts::PI * radius;
    payload[73..81].copy_from_slice(&radius.to_le_bytes());
    payload[81..89].copy_from_slice(&parameter_range[0].to_le_bytes());
    payload[89..97].copy_from_slice(&parameter_range[1].to_le_bytes());
    payload[97..105].copy_from_slice(&1.0f64.to_le_bytes());
    payload[105..113].copy_from_slice(&chart_origin.to_le_bytes());
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x0f,
        object_id: 8,
        payload,
    };
    assert_eq!(
        parse_profile(&record),
        Some(B5Profile::Arc {
            center: [1.0, 2.0, 3.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            radius,
            parameter_range,
        })
    );

    let mut nonorthogonal = record.clone();
    nonorthogonal.payload[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(parse_profile(&nonorthogonal), None);
    let mut overlong = record.clone();
    overlong.payload[89..97].copy_from_slice(
        &(parameter_range[0] + std::f64::consts::TAU * radius + 1.0).to_le_bytes(),
    );
    assert_eq!(parse_profile(&overlong), None);
    let mut wrong_fixed = record.clone();
    wrong_fixed.payload[97..105].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(parse_profile(&wrong_fixed), None);
    let mut wrong_origin = record;
    wrong_origin.payload[105..113].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(parse_profile(&wrong_origin), None);
}

#[test]
fn cone_surface_reads_the_native_slant_chart() {
    let mut payload = vec![0; 185];
    payload[0] = 0x80;
    for (offset, values) in [
        (1, [1.0f64, 2.0, 3.0]),
        (25, [1.0, 0.0, 0.0]),
        (49, [0.0, 1.0, 0.0]),
        (73, [0.0, 0.0, 1.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            payload[offset + 8 * index..offset + 8 * index + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for (offset, value) in [
        (97, 0.25f64),
        (105, 4.0),
        (113, 0.5),
        (121, 0.5 + std::f64::consts::PI),
        (129, 0.0),
        (137, 8.0),
        (145, 3.0),
        (153, 1.0),
        (169, 0.5 - std::f64::consts::FRAC_PI_2),
        (177, 0.5 + 3.0 * std::f64::consts::FRAC_PI_2),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x29,
        object_id: 7,
        payload,
    };
    assert_eq!(
        parse_surface(&record),
        Some(B5Surface::Cone {
            apex: [1.0, 2.0, 3.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle: 0.25,
            reference_radius: 4.0,
            angular_range: [0.5, 0.5 + std::f64::consts::PI],
            slant_range: [0.0, 8.0],
            angular_scale: 3.0,
            angular_domain: [
                0.5 - std::f64::consts::FRAC_PI_2,
                0.5 + 3.0 * std::f64::consts::FRAC_PI_2,
            ],
        })
    );

    let mut opposite_handed = record.clone();
    opposite_handed.payload[89..97].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert!(parse_surface(&opposite_handed).is_some());

    let mut malformed = record.clone();
    malformed.payload[169..177].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(parse_surface(&malformed), None);
    let mut degenerate = record.clone();
    degenerate.payload[97..105].copy_from_slice(&0.0_f64.to_le_bytes());
    assert_eq!(parse_surface(&degenerate), None);
    let mut nonunit = record.clone();
    nonunit.payload[25..33].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(parse_surface(&nonunit), None);
    let mut nonorthogonal = record.clone();
    nonorthogonal.payload[49..57].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(parse_surface(&nonorthogonal), None);
    let mut invalid_axis = record.clone();
    invalid_axis.payload[73..81].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(parse_surface(&invalid_axis), None);
    let mut wrong_lead = record;
    wrong_lead.payload[0] = 0x81;
    assert_eq!(parse_surface(&wrong_lead), None);
}

#[test]
fn plane_surface_requires_complete_unit_chart_frame() {
    let mut payload = vec![0; 121];
    payload[0] = 0x80;
    for (offset, value) in [
        (1usize, 1.0f64),
        (9, 2.0),
        (17, 3.0),
        (25, 1.0),
        (57, 1.0),
        (73, 1.0),
        (81, 1.0),
        (89, -4.0),
        (97, 8.0),
        (105, -2.0),
        (113, 6.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x27,
        object_id: 7,
        payload,
    };
    assert_eq!(
        parse_surface(&record),
        Some(B5Surface::Plane {
            origin: [1.0, 2.0, 3.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-4.0, 8.0],
            v_range: [-2.0, 6.0],
        })
    );
    let mut wrong_family = record.clone();
    wrong_family.family = 0xa8;
    assert_eq!(parse_surface(&wrong_family), None);

    let mut nonunit = record.clone();
    nonunit.payload[25..33].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(parse_surface(&nonunit), None);
    let mut reversed_range = record;
    reversed_range.payload[97..105].copy_from_slice(&(-5.0f64).to_le_bytes());
    assert_eq!(parse_surface(&reversed_range), None);
}

#[test]
fn cylinder_surface_retains_independent_angular_gauge_and_domain() {
    let radius = 6.0;
    let angular_factor = 2.0;
    let angular_scale = radius / angular_factor;
    let chart_origin = 1.0;
    let mut payload = vec![0; 137];
    payload[0] = 0x80;
    for (offset, value) in [
        (1usize, 1.0f64),
        (9, 2.0),
        (17, 3.0),
        (25, 1.0),
        (57, 1.0),
        (73, radius),
        (81, 2.0),
        (89, 10.0),
        (97, -4.0),
        (105, 5.0),
        (113, angular_factor),
        (121, 1.0),
        (129, chart_origin),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x28,
        object_id: 7,
        payload,
    };
    assert_eq!(
        parse_surface(&record),
        Some(B5Surface::Cylinder {
            origin: [1.0, 2.0, 3.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius,
            u_range: [2.0, 10.0],
            v_range: [-4.0, 5.0],
            angular_scale,
            chart_origin,
        })
    );

    let mut outside_domain = record.clone();
    outside_domain.payload[89..97].copy_from_slice(
        &(chart_origin + std::f64::consts::TAU * angular_scale + 1.0).to_le_bytes(),
    );
    assert_eq!(parse_surface(&outside_domain), None);

    let mut overflowing_domain = record.clone();
    overflowing_domain.payload[73..81].copy_from_slice(&f64::MAX.to_le_bytes());
    overflowing_domain.payload[113..121].copy_from_slice(&1.0f64.to_le_bytes());
    assert_eq!(parse_surface(&overflowing_domain), None);

    let mut wrong_fixed_scalar = record;
    wrong_fixed_scalar.payload[121..129].copy_from_slice(&2.0f64.to_le_bytes());
    assert_eq!(parse_surface(&wrong_fixed_scalar), None);
}

#[test]
fn sphere_surface_validates_radius_scaled_frame_and_chart() {
    let construction_radius = 3.0;
    let azimuth_range = [0.0, 1.0];
    let chart_origin =
        construction_radius * ((azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI);
    let mut payload = vec![0; 153];
    payload[0] = 0x80;
    for (offset, value) in [
        (1usize, 1.0f64),
        (9, 2.0),
        (17, 3.0),
        (25, 2.0),
        (57, 2.0),
        (89, 2.0),
        (97, 2.0),
        (105, 0.0),
        (113, 1.0),
        (121, -1.0),
        (129, 1.0),
        (137, construction_radius),
        (145, chart_origin),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2a,
        object_id: 7,
        payload,
    };
    assert_eq!(
        parse_surface(&record),
        Some(B5Surface::Sphere {
            center: [1.0, 2.0, 3.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
            azimuth_range,
            latitude_range: [-1.0, 1.0],
            construction_radius,
            chart_origin,
        })
    );
    let tiny_radius = 1e-200_f64;
    let mut tiny = record.clone();
    for (offset, value) in [
        (25usize, tiny_radius),
        (57, tiny_radius),
        (89, tiny_radius),
        (97, tiny_radius),
    ] {
        tiny.payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    assert!(matches!(
        parse_surface(&tiny),
        Some(B5Surface::Sphere {
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius,
            ..
        }) if radius == tiny_radius
    ));
    tiny.payload[25..33].copy_from_slice(&(2.0 * tiny_radius).to_le_bytes());
    assert_eq!(parse_surface(&tiny), None);

    let tiny_construction_radius = 1e-200_f64;
    let tiny_chart_origin = tiny_construction_radius
        * ((azimuth_range[0] + azimuth_range[1]) * 0.5 - std::f64::consts::PI);
    let mut tiny_chart = record.clone();
    tiny_chart.payload[137..145].copy_from_slice(&tiny_construction_radius.to_le_bytes());
    tiny_chart.payload[145..153].copy_from_slice(&tiny_chart_origin.to_le_bytes());
    assert!(parse_surface(&tiny_chart).is_some());
    tiny_chart.payload[145..153].copy_from_slice(&(tiny_chart_origin + f64::EPSILON).to_le_bytes());
    assert!(parse_surface(&tiny_chart).is_some());
    tiny_chart.payload[145..153].copy_from_slice(&1e-12_f64.to_le_bytes());
    assert_eq!(parse_surface(&tiny_chart), None);

    let mut left_handed = record.clone();
    left_handed.payload[89..97].copy_from_slice(&(-2.0f64).to_le_bytes());
    assert_eq!(parse_surface(&left_handed), None);

    let mut overlong_azimuth = record.clone();
    overlong_azimuth.payload[113..121]
        .copy_from_slice(&(std::f64::consts::TAU + 1.0).to_le_bytes());
    assert_eq!(parse_surface(&overlong_azimuth), None);

    let mut invalid_latitude = record.clone();
    invalid_latitude.payload[121..129].copy_from_slice(&(-std::f64::consts::PI).to_le_bytes());
    assert_eq!(parse_surface(&invalid_latitude), None);

    let mut wrong_chart_origin = record;
    wrong_chart_origin.payload[145..153].copy_from_slice(&0.0f64.to_le_bytes());
    assert_eq!(parse_surface(&wrong_chart_origin), None);
}

#[test]
fn torus_surface_separates_geometric_radii_from_chart_scales() {
    let mut payload = vec![0; 201];
    payload[0] = 0x80;
    for (offset, values) in [
        (1, [1.0f64, 2.0, 3.0]),
        (25, [1.0, 0.0, 0.0]),
        (49, [0.0, 1.0, 0.0]),
        (73, [0.0, 0.0, 1.0]),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            payload[offset + 8 * index..offset + 8 * index + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    payload[97..105].copy_from_slice(&5.0f64.to_le_bytes());
    payload[105..113].copy_from_slice(&2.0f64.to_le_bytes());
    for (offset, value) in [
        (113, 0.0),
        (121, std::f64::consts::TAU),
        (129, 0.0),
        (137, std::f64::consts::TAU),
        (145, 0.0),
        (153, std::f64::consts::PI),
        (161, -std::f64::consts::FRAC_PI_2),
        (169, 3.0 * std::f64::consts::FRAC_PI_2),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[177..185].copy_from_slice(&4.0f64.to_le_bytes());
    payload[185..193].copy_from_slice(&3.0f64.to_le_bytes());
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2b,
        object_id: 7,
        payload,
    };
    assert_eq!(
        parse_surface(&record),
        Some(B5Surface::Torus {
            center: [1.0, 2.0, 3.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius: 5.0,
            minor_radius: 2.0,
            major_angular_range: [0.0, std::f64::consts::TAU],
            major_angular_domain: [0.0, std::f64::consts::TAU],
            minor_angular_range: [0.0, std::f64::consts::PI],
            minor_angular_domain: [
                -std::f64::consts::FRAC_PI_2,
                3.0 * std::f64::consts::FRAC_PI_2,
            ],
            major_scale: 4.0,
            minor_scale: 3.0,
        })
    );

    let mut malformed = record.clone();
    malformed.payload[129..137].copy_from_slice(&0.5f64.to_le_bytes());
    assert_eq!(parse_surface(&malformed), None);
    let mut left_handed = record.clone();
    left_handed.payload[89..97].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(parse_surface(&left_handed), None);
    let mut wrong_lead = record.clone();
    wrong_lead.payload[0] = 0x81;
    assert_eq!(parse_surface(&wrong_lead), None);
    let mut nonzero_tail = record;
    nonzero_tail.payload[200] = 1;
    assert_eq!(parse_surface(&nonzero_tail), None);
}

#[test]
fn line_pcurve_decodes_every_complete_mode() {
    let record = |mode: u8, values: &[f64]| {
        let mut payload = vec![0x81, 0x82, mode];
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x18,
            object_id: 7,
            payload,
        }
    };

    let general = parse_line_pcurve(&record(0x01, &[2.0, 3.0, 4.0, -2.0, 1.0, 5.0]))
        .expect("general line pcurve");
    assert_eq!(general.surface, 2);
    assert_eq!(general.distinct_knots, [1.0, 5.0]);
    assert_eq!(general.control_points, [[6.0, 1.0], [22.0, -7.0]]);
    let tiny = parse_line_pcurve(&record(0x01, &[0.0, 0.0, 1e-200, -1e-200, 1.0, 5.0]))
        .expect("tiny nonzero line direction");
    assert_eq!(tiny.control_points, [[1e-200, -1e-200], [5e-200, -5e-200]]);
    let mut wrong_family = record(0x01, &[2.0, 3.0, 4.0, -2.0, 1.0, 5.0]);
    wrong_family.family = 0xa8;
    assert!(parse_line_pcurve(&wrong_family).is_none());

    let constant_u =
        parse_line_pcurve(&record(0x05, &[3.0, -2.0, 7.0])).expect("constant-U pcurve");
    assert_eq!(constant_u.distinct_knots, [-2.0, 7.0]);
    assert_eq!(constant_u.control_points, [[3.0, -2.0], [3.0, 7.0]]);

    let constant_v =
        parse_line_pcurve(&record(0x09, &[3.0, -2.0, 7.0])).expect("constant-V pcurve");
    assert_eq!(constant_v.distinct_knots, [-2.0, 7.0]);
    assert_eq!(constant_v.control_points, [[-2.0, 3.0], [7.0, 3.0]]);
}

#[test]
fn line_pcurve_rejects_degenerate_or_unclosed_payloads() {
    let record = |mode: u8, values: &[f64]| {
        let mut payload = vec![0x81, 0x82, mode];
        for value in values {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x18,
            object_id: 7,
            payload,
        }
    };

    assert!(parse_line_pcurve(&record(0x01, &[2.0, 3.0, 0.0, 0.0, 1.0, 5.0])).is_none());
    assert!(parse_line_pcurve(&record(0x05, &[3.0, 2.0, 2.0])).is_none());
    assert!(parse_line_pcurve(&record(0x0d, &[3.0, -2.0, 7.0])).is_none());

    let mut tailed = record(0x09, &[3.0, -2.0, 7.0]);
    tailed.payload.push(0);
    assert!(parse_line_pcurve(&tailed).is_none());
}

#[test]
fn circle_pcurve_preserves_arc_length_parameterization() {
    let mut payload = vec![0x81, 0x18, 0x34, 0x12];
    for value in [0.0, 0.0, 2.0, 0.0, 2.0 * std::f64::consts::PI, 1.0, 0.0] {
        if payload.len() == 20 {
            payload.extend_from_slice(&[0x05, 0x05]);
        }
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x19,
        object_id: 0x1235,
        payload,
    };
    let pcurve = parse_circle_pcurve(&record).expect("circle pcurve");
    assert_eq!(pcurve.surface, 0x1234);
    assert_eq!(pcurve.degree, 2);
    assert_eq!(
        pcurve.distinct_knots,
        [0.0, std::f64::consts::PI, 2.0 * std::f64::consts::PI]
    );
    assert_eq!(pcurve.multiplicities, [3, 2, 3]);
    assert_eq!(pcurve.control_points.len(), 5);
    let weights = pcurve.weights.expect("rational weights");
    assert_eq!(weights.len(), 5);
    assert!((weights[1] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1.0e-12);
    assert!((pcurve.control_points[0][0] - 2.0).abs() < 1.0e-12);
    assert!((pcurve.control_points[4][0] + 2.0).abs() < 1.0e-12);
    let mut wrong_family = record;
    wrong_family.family = 0xa8;
    assert!(parse_circle_pcurve(&wrong_family).is_none());
}

#[test]
fn class_1a_pcurve_uses_diameter_period_parameterization() {
    let mut payload = vec![0x81, 0x18, 0x34, 0x12];
    for value in [3.0_f64, 4.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [
        2.0_f64,
        0.0,
        std::f64::consts::FRAC_PI_2,
        0.0,
        std::f64::consts::PI,
        1.0,
        2.0 * std::f64::consts::PI,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x1a,
        object_id: 0x1235,
        payload,
    };
    let pcurve = parse_class_1a_pcurve(&record).expect("class-1a pcurve");
    assert_eq!(pcurve.surface, 0x1234);
    assert_eq!(
        pcurve.distinct_knots,
        [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
    );
    assert_eq!(pcurve.multiplicities, [3, 2, 3]);
    assert_eq!(pcurve.control_points.len(), 5);
    assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([4.0, 4.0]));
    let end = evaluate_pcurve(&pcurve, std::f64::consts::PI).expect("end point");
    assert!((end[0] - 2.0).abs() < 1.0e-12);
    assert!((end[1] - 4.0).abs() < 1.0e-12);

    let diameter = 1e-200_f64;
    let period = std::f64::consts::PI * diameter;
    let mut tiny = record.clone();
    tiny.payload[22..30].copy_from_slice(&diameter.to_le_bytes());
    tiny.payload[54..62].copy_from_slice(&(period * 0.5).to_le_bytes());
    tiny.payload[70..78].copy_from_slice(&period.to_le_bytes());
    assert!(parse_class_1a_pcurve(&tiny).is_some());

    let wrong_period = 1e-13_f64;
    tiny.payload[54..62].copy_from_slice(&(wrong_period * 0.5).to_le_bytes());
    tiny.payload[70..78].copy_from_slice(&wrong_period.to_le_bytes());
    assert!(parse_class_1a_pcurve(&tiny).is_none());

    let mut wrong_family = record;
    wrong_family.family = 0xa8;
    assert!(parse_class_1a_pcurve(&wrong_family).is_none());
}

#[test]
fn class_1a_pcurve_accepts_a_finite_nonzero_diameter() {
    for diameter in [1e-200_f64, 1e200] {
        let period = std::f64::consts::PI * diameter;
        let mut payload = vec![0x81, 0x82];
        for value in [0.0_f64, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&[0x05, 0x05]);
        for value in [
            diameter,
            0.0,
            std::f64::consts::FRAC_PI_2,
            0.0,
            period * 0.25,
            1.0,
            period,
        ] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x1a,
            object_id: 7,
            payload,
        };
        let pcurve = parse_class_1a_pcurve(&record).expect("class-1a pcurve");
        assert_eq!(pcurve.control_points[0], [diameter * 0.5, 0.0]);
    }
}

#[test]
fn noncanonical_class_1a_payload_remains_opaque() {
    let mut payload = vec![0x81, 0x82];
    for value in [0.0_f64, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    for value in [2.0_f64, 0.0, 1.0, 0.0, 1.0, 1.0, 3.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x1a,
        object_id: 7,
        payload,
    };
    assert!(parse_class_1a_pcurve(&record).is_none());
    assert!(parse_opaque_pcurve(&record).is_some());
}

#[test]
fn pcurve_evaluation_preserves_the_native_parameter() {
    let pcurve = B5Pcurve {
        object_id: 1,
        surface: 2,
        degree: 1,
        distinct_knots: vec![-1.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[2.0, 3.0], [4.0, 7.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: None,
    };
    assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([3.0, 5.0]));
    assert_eq!(evaluate_pcurve(&pcurve, -1.0), Some([2.0, 3.0]));
    assert_eq!(evaluate_pcurve(&pcurve, 1.0), Some([4.0, 7.0]));
}

#[test]
fn opaque_conic_pcurves_retain_support_identity_and_payload() {
    let mut ellipse = vec![0x81, 0x82];
    ellipse.extend_from_slice(&[0; 16]);
    ellipse.extend_from_slice(&[0x05, 0x05]);
    ellipse.extend_from_slice(&[0; 56]);
    let ellipse_record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x1a,
        object_id: 7,
        payload: ellipse.clone(),
    };
    assert_eq!(
        parse_opaque_pcurve(&ellipse_record),
        Some(B5OpaquePcurve {
            object_id: 7,
            surface: 2,
            class: 0x1a,
            payload: ellipse,
            sphere_great_circle: None,
        })
    );

    let mut class_1d = vec![0x81, 0x82];
    class_1d.extend_from_slice(&[0; 32]);
    class_1d.extend_from_slice(&[0x05, 0x81]);
    class_1d.extend_from_slice(&[0; 24]);
    class_1d.push(0x1d);
    class_1d.extend_from_slice(&[0; 40]);
    let class_1d_record = B5Record {
        class: 0x1d,
        payload: class_1d.clone(),
        ..ellipse_record
    };
    assert_eq!(
        parse_opaque_pcurve(&class_1d_record),
        Some(B5OpaquePcurve {
            object_id: 7,
            surface: 2,
            class: 0x1d,
            payload: class_1d,
            sphere_great_circle: None,
        })
    );
    let mut wrong_family = class_1d_record;
    wrong_family.family = 0xa8;
    assert_eq!(parse_opaque_pcurve(&wrong_family), None);
}

#[test]
fn class_1d_pcurve_decodes_a_sphere_great_circle_plane() {
    let radius = 5.0;
    let chart_scale = 8.0;
    let chart_origin = 11.0;
    let mut payload = vec![0x81, 0x82];
    for value in [
        chart_scale * 0.25,
        chart_scale * 1.25,
        chart_origin,
        chart_origin + std::f64::consts::TAU * chart_scale,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x05, 0x81]);
    for value in [2.0_f64, -1.0, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(0x1d);
    for value in [chart_scale, -0.75, 1.0 / chart_scale, -1.25, 0.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x1d,
        object_id: 7,
        payload,
    };
    let sphere = B5Surface::Sphere {
        center: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius,
        azimuth_range: [0.0, 1.5],
        latitude_range: [-1.0, 1.0],
        construction_radius: chart_scale,
        chart_origin,
    };

    assert_eq!(
        parse_sphere_great_circle_pcurve(&record, &sphere),
        Some(B5SphereGreatCirclePcurve {
            chart_bounds: [
                [chart_scale * 0.25, chart_scale * 1.25],
                [
                    chart_origin,
                    chart_origin + std::f64::consts::TAU * chart_scale,
                ],
            ],
            chart_shift: 2.0,
            chart_scale,
            slope: -0.75,
            phase: -1.25,
        })
    );

    let mut outside_surface_chart = record.clone();
    outside_surface_chart.payload[2..10].copy_from_slice(&(-1.0_f64).to_le_bytes());
    assert_eq!(
        parse_sphere_great_circle_pcurve(&outside_surface_chart, &sphere),
        None
    );

    let mut approximate_reciprocal = outside_surface_chart.clone();
    approximate_reciprocal.payload[2..10].copy_from_slice(&(chart_scale * 0.25).to_le_bytes());
    let reciprocal_offset = 2 + 32 + 2 + 24 + 1 + 16;
    approximate_reciprocal.payload[reciprocal_offset..reciprocal_offset + 8]
        .copy_from_slice(&f64::from_bits((1.0 / chart_scale).to_bits() + 1).to_le_bytes());
    assert_eq!(
        parse_sphere_great_circle_pcurve(&approximate_reciprocal, &sphere),
        None
    );

    let mut approximate_chart_scale = outside_surface_chart;
    approximate_chart_scale.payload[2..10].copy_from_slice(&(chart_scale * 0.25).to_le_bytes());
    let scale_offset = 2 + 32 + 2 + 24 + 1;
    approximate_chart_scale.payload[scale_offset..scale_offset + 8]
        .copy_from_slice(&f64::from_bits(chart_scale.to_bits() + 1).to_le_bytes());
    assert_eq!(
        parse_sphere_great_circle_pcurve(&approximate_chart_scale, &sphere),
        None
    );

    let mut rounded_surface_bounds = sphere;
    let rounded_lower = f64::from_bits(0.25_f64.to_bits() + 1);
    let B5Surface::Sphere { azimuth_range, .. } = &mut rounded_surface_bounds else {
        unreachable!()
    };
    *azimuth_range = [rounded_lower, 1.5];
    assert!(parse_sphere_great_circle_pcurve(&record, &rounded_surface_bounds).is_some());
}

#[test]
fn surface_aliases_require_their_complete_class_layout() {
    let alias = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2e,
        object_id: 9,
        payload: vec![0x38, 0x34, 0x12, 0x00],
    };
    assert_eq!(surface_alias_target(&alias), Some(0x1234));

    let counted = B5Record {
        payload: vec![0x81, 0x38, 0x34, 0x12, 0x00],
        ..alias.clone()
    };
    assert_eq!(surface_alias_target(&counted), Some(0x1234));

    let mut tailed = alias.clone();
    tailed.payload.push(0x05);
    assert_eq!(surface_alias_target(&tailed), None);

    let chart_alias = B5Record {
        class: 0x38,
        payload: vec![0x81, 0x38, 0x34, 0x12, 0x00, 0x05, 0x05, 0x09],
        ..alias
    };
    assert_eq!(surface_alias_target(&chart_alias), Some(0x1234));

    let mut truncated_chart_alias = chart_alias;
    truncated_chart_alias.payload.pop();
    assert_eq!(surface_alias_target(&truncated_chart_alias), None);
}

#[test]
fn offset_surface_separates_result_carrier_source_and_bounds() {
    let carrier = B5Surface::Plane {
        origin: [0.0; 3],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let source = B5Surface::Plane {
        origin: [0.0, 0.0, 0.5],
        direction_u: [0.0, 1.0, 0.0],
        direction_v: [-1.0, 0.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let surfaces = BTreeMap::from([(2, carrier), (3, source)]);
    let mut payload = vec![0x82, 0x82, 0x83];
    payload.extend_from_slice(&(-0.5f64).to_le_bytes());
    payload.push(0x15);
    for value in [-2.0f64, 3.0, -4.0, 5.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x30,
        object_id: 9,
        payload,
    };
    assert_eq!(
        parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
        Some(B5OffsetSurface {
            object_id: 9,
            carrier_surface: 2,
            source_surface: 3,
            distance: -0.5,
            carrier_kind: 0x15,
            parameter_bounds: [[-2.0, 3.0], [-4.0, 5.0]],
        })
    );
}

#[test]
fn offset_surface_accepts_a_sphere_result_carrier() {
    let carrier = B5Surface::Sphere {
        center: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 2.0,
        azimuth_range: [0.0, 1.0],
        latitude_range: [-1.0, 1.0],
        construction_radius: 2.0,
        chart_origin: -2.0,
    };
    let source = B5Surface::Sphere {
        center: [0.0; 3],
        direction_x: [0.0, 1.0, 0.0],
        direction_y: [-1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 8.5,
        azimuth_range: [0.0, 1.0],
        latitude_range: [-1.0, 1.0],
        construction_radius: 8.5,
        chart_origin: -2.0,
    };
    let surfaces = BTreeMap::from([(2, carrier), (3, source)]);
    let mut payload = vec![0x82, 0x82, 0x83];
    payload.extend_from_slice(&(-6.5_f64).to_le_bytes());
    payload.push(0x09);
    for value in [0.0_f64, 2.0, -2.0, 4.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x30,
        object_id: 9,
        payload,
    };

    assert_eq!(
        parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
        Some(B5OffsetSurface {
            object_id: 9,
            carrier_surface: 2,
            source_surface: 3,
            distance: -6.5,
            carrier_kind: 0x09,
            parameter_bounds: [[0.0, 2.0], [-2.0, 4.0]],
        })
    );
}

#[test]
fn offset_surface_does_not_infer_cone_construction_from_result_class() {
    let carrier = B5Surface::Cone {
        apex: [0.0; 3],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        half_angle: 0.25,
        reference_radius: 0.0,
        angular_range: [0.0, std::f64::consts::TAU],
        slant_range: [-2.0, 4.0],
        angular_scale: 3.0,
        angular_domain: [0.0, std::f64::consts::TAU],
    };
    let surfaces = BTreeMap::from([(2, carrier)]);
    let mut payload = vec![0x82, 0x82, 0x83];
    payload.extend_from_slice(&1.5_f64.to_le_bytes());
    payload.push(0x11);
    for value in [-3.0_f64, 3.0, -2.0, 4.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x30,
        object_id: 9,
        payload,
    };

    assert_eq!(
        parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &HashMap::new()),
        None
    );
}

#[test]
fn analytic_offset_gate_requires_coaxial_equal_family_carriers() {
    let plane = |origin| B5Surface::Plane {
        origin,
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let tiny = 1e-200_f64;
    assert!(analytic_offset_magnitude_agrees(
        &plane([0.0, 0.0, tiny]),
        &plane([0.0; 3]),
        tiny
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &plane([0.0, 0.0, tiny]),
        &plane([0.0; 3]),
        1e-13
    ));

    let cylinder = |origin, radius| B5Surface::Cylinder {
        origin,
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius,
        u_range: [0.0, std::f64::consts::TAU * radius],
        v_range: [-1.0, 1.0],
        angular_scale: radius,
        chart_origin: 0.0,
    };
    assert!(analytic_offset_magnitude_agrees(
        &cylinder([0.0, 0.0, 4.0], 3.0),
        &cylinder([0.0; 3], 5.0),
        -2.0
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &cylinder([0.25, 0.0, 4.0], 3.0),
        &cylinder([0.0; 3], 5.0),
        -2.0
    ));
    assert!(analytic_offset_magnitude_agrees(
        &cylinder([0.0, 0.0, tiny], 2.0 * tiny),
        &cylinder([0.0; 3], tiny),
        tiny
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &cylinder([0.0, 0.0, tiny], 2.0 * tiny),
        &cylinder([0.0; 3], tiny),
        1e-13
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &cylinder([tiny, 0.0, tiny], 2.0 * tiny),
        &cylinder([0.0; 3], tiny),
        tiny
    ));

    let torus = |center, major_radius, minor_radius| B5Surface::Torus {
        center,
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        major_radius,
        minor_radius,
        major_angular_range: [0.0, std::f64::consts::TAU],
        major_angular_domain: [0.0, std::f64::consts::TAU],
        minor_angular_range: [0.0, std::f64::consts::TAU],
        minor_angular_domain: [0.0, std::f64::consts::TAU],
        major_scale: major_radius,
        minor_scale: minor_radius,
    };
    assert!(analytic_offset_magnitude_agrees(
        &torus([0.0; 3], 8.0, 3.0),
        &torus([0.0; 3], 8.0, 2.5),
        0.5
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &torus([0.0; 3], 9.0, 3.0),
        &torus([0.0; 3], 8.0, 2.5),
        0.5
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &torus([0.0; 3], 2.0 * tiny, 2.0 * tiny),
        &torus([0.0; 3], tiny, tiny),
        tiny
    ));

    let sphere = |center, radius| B5Surface::Sphere {
        center,
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius,
        azimuth_range: [0.0, 1.0],
        latitude_range: [-1.0, 1.0],
        construction_radius: radius,
        chart_origin: 0.0,
    };
    assert!(analytic_offset_magnitude_agrees(
        &sphere([0.0; 3], 2.0 * tiny),
        &sphere([0.0; 3], tiny),
        tiny
    ));
    assert!(!analytic_offset_magnitude_agrees(
        &sphere([1e-13, 0.0, 0.0], 2.0 * tiny),
        &sphere([0.0; 3], tiny),
        tiny
    ));
}

#[test]
fn offset_surface_accepts_an_identity_checked_class_31_cache() {
    assert!(is_referenced_geometry_class(0xb5, 0x31));
    let source = B5Surface::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_count: 2,
        v_count: 2,
        control_points: vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 4],
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });
    let surfaces = BTreeMap::from([(3, source.clone()), (4, source)]);
    let mut cache_payload = vec![0x81, 0x84];
    for value in [-0.5f64, -2.0, -4.0, 3.0, 5.0] {
        cache_payload.extend_from_slice(&value.to_le_bytes());
    }
    let cache = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x31,
        object_id: 2,
        payload: cache_payload,
    };
    let records = HashMap::from([(2, &cache)]);
    let mut payload = vec![0x82, 0x82, 0x83];
    payload.extend_from_slice(&(-0.5f64).to_le_bytes());
    payload.push(0x01);
    for value in [-2.0f64, 3.0, -4.0, 5.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x30,
        object_id: 9,
        payload,
    };

    assert_eq!(
        parse_offset_surface(&record, &surfaces, &BTreeMap::new(), &records),
        Some(B5OffsetSurface {
            object_id: 9,
            carrier_surface: 2,
            source_surface: 3,
            distance: -0.5,
            carrier_kind: 0x01,
            parameter_bounds: [[-2.0, 3.0], [-4.0, 5.0]],
        })
    );
}

#[test]
fn extrusion_surface_binds_two_mapped_directrix_supports() {
    let mut pcurve_payload = vec![0x81, 0x86, 0x05];
    for value in [2.0f64, -3.0, 4.0] {
        pcurve_payload.extend_from_slice(&value.to_le_bytes());
    }
    let pcurve = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x18,
        object_id: 3,
        payload: pcurve_payload,
    };
    let mut wrapper_payload = vec![0x81, 0x83, 0x81, 0x01];
    for value in [-3.0f64, 4.0, 0.0] {
        wrapper_payload.extend_from_slice(&value.to_le_bytes());
    }
    wrapper_payload.push(0x01);
    let wrapper = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x24,
        object_id: 2,
        payload: wrapper_payload,
    };
    let mut directrix_payload = vec![0x82, 0x82, 0x84, 0x00];
    for value in [-3.0f64, 4.0, 0.01] {
        directrix_payload.extend_from_slice(&value.to_le_bytes());
    }
    directrix_payload.push(0x01);
    let directrix = B5Record {
        offset: 0,
        family: 0xa8,
        class: 0x25,
        object_id: 5,
        payload: directrix_payload,
    };
    let records = HashMap::from([(2, &wrapper), (3, &pcurve), (5, &directrix)]);
    let pcurves = BTreeMap::from([(4, object_stream_pcurve(7, vec![10.0, 20.0], None))]);
    let mut payload = vec![0x81, 0x85];
    for value in [0.0f64, 0.0, 1.0, -2.0, 6.0, 1.0, 0.0, -3.0, 4.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[0x05, 0x05]);
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2c,
        object_id: 8,
        payload,
    };

    assert_eq!(
        parse_extrusion_surface(&record, &records, &pcurves),
        Some(B5ExtrusionSurface {
            object_id: 8,
            direction: [0.0, 0.0, 1.0],
            parameter_bounds: [[-2.0, 6.0], [-3.0, 4.0]],
            directrix: B5ExtrusionDirectrix::Intersection {
                object_id: 5,
                supports: [(6, 3, [-3.0, 4.0]), (7, 4, [10.0, 20.0])],
                parameter_range: [-3.0, 4.0],
                cache_fit_tolerance: 0.01,
            },
        })
    );
    let directrix_lower_bound = 2 + 7 * 8;
    let mut trimmed_interval = record.clone();
    trimmed_interval.payload[directrix_lower_bound..directrix_lower_bound + 8]
        .copy_from_slice(&(-2.0_f64).to_le_bytes());
    assert!(
        parse_extrusion_surface(&trimmed_interval, &records, &pcurves).is_some(),
        "the extrusion may use a strict subinterval of its directrix"
    );
    let mut outside_interval = record.clone();
    outside_interval.payload[directrix_lower_bound..directrix_lower_bound + 8]
        .copy_from_slice(&(-3.0_f64 - 1.0e-9).to_le_bytes());
    assert_eq!(
        parse_extrusion_surface(&outside_interval, &records, &pcurves),
        None,
        "the active interval must remain inside the directrix domain"
    );

    for controls in [[0x05, 0x05], [0x01, 0x29], [0x05, 0x29]] {
        let mut candidate = record.clone();
        let tail = candidate.payload.len() - 2;
        candidate.payload[tail..].copy_from_slice(&controls);
        assert!(
            parse_extrusion_surface(&candidate, &records, &pcurves).is_some(),
            "terminal controls {controls:02x?}"
        );
    }
    for controls in [
        [0x01, 0x05],
        [0x05, 0x09],
        [0x05, 0x11],
        [0x05, 0x15],
        [0x05, 0x19],
        [0x01, 0x09],
        [0x01, 0x15],
        [0x01, 0x19],
        [0x09, 0x29],
    ] {
        let mut candidate = record.clone();
        let tail = candidate.payload.len() - 2;
        candidate.payload[tail..].copy_from_slice(&controls);
        assert_eq!(
            parse_extrusion_surface(&candidate, &records, &pcurves),
            None,
            "terminal controls {controls:02x?}"
        );
    }
}
