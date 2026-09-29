// SPDX-License-Identifier: Apache-2.0
//! incidence loci tests.

use super::{
    bind_edge_vertices, canonical_point, counted_references, distance_squared,
    incidence_vertex_coordinates, parameter_incidence, pcurve_endpoints, point_index,
    sphere_great_circle_point, test_loop_members, test_loop_metadata, B5IncidenceLane, B5Loop,
    B5OpaquePcurve, B5ParameterIncidence, B5Pcurve, B5PcurveContext, B5PcurveParameterization,
    B5Record, B5SphereGreatCirclePcurve, B5Surface, B5VertexIncidenceControl,
    B5VertexIncidenceLink, BTreeMap, HashMap,
};

#[test]
fn canonical_point_uses_the_on_carrier_tolerance() {
    let points = [[0.0, 0.0, 0.0]].map(crate::test_support::test_b5::point);
    let index = point_index(&points);
    assert_eq!(canonical_point(&points, &index, [1e-3, 0.0, 0.0]), Some(0));
    assert_eq!(
        canonical_point(&points, &index, [1.0001e-3, 0.0, 0.0]),
        None
    );
}

#[test]
fn edge_parameter_incidences_select_typed_pcurve_endpoint_loci() {
    let pcurves = BTreeMap::from([(
        2,
        B5Pcurve {
            object_id: 2,
            surface: 4,
            degree: 1,
            distinct_knots: crate::test_support::test_b5::finite_lane(&[0.0, 1.0]),
            multiplicities: vec![2, 2],
            control_points: vec![
                crate::test_support::test_b5::finite_vector([0.0, 0.0]),
                crate::test_support::test_b5::finite_vector([10.0, 0.0]),
            ],
            weights: None,
            parameter_range: None,
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: None,
            lifted_endpoints: Some(crate::test_support::test_b5::points([
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
            ])),
        },
    )]);
    let surfaces = BTreeMap::from([(
        4,
        B5Surface::Plane {
            origin: crate::test_support::test_b5::point([0.0, 0.0, 0.0]),
            frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
            u_range: crate::test_support::test_b5::increasing([0.0, 10.0]),
            v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        },
    )]);
    let edge_parameter_incidences = BTreeMap::from([(3, [20, 21])]);
    let parameter_incidences = BTreeMap::from([
        (
            20,
            B5ParameterIncidence {
                object_id: 20,
                lanes: vec![B5IncidenceLane {
                    curve: 2,
                    parameter: crate::test_support::test_b5::finite(0.25),
                    control: 1,
                }],
            },
        ),
        (
            21,
            B5ParameterIncidence {
                object_id: 21,
                lanes: vec![B5IncidenceLane {
                    curve: 2,
                    parameter: crate::test_support::test_b5::finite(0.75),
                    control: 1,
                }],
            },
        ),
    ]);
    let opaque_pcurves = BTreeMap::new();
    let profiles = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let loop_ = B5Loop {
        object_id: 1,
        members: test_loop_members(&[2], &[3]),
        metadata: test_loop_metadata(),
        surface: 4,
    };

    assert_eq!(
        pcurve_endpoints(2, 3, &geometry),
        Some(crate::test_support::test_b5::points([
            [2.5, 0.0, 0.0],
            [7.5, 0.0, 0.0]
        ]))
    );
    assert_eq!(
        bind_edge_vertices(
            &BTreeMap::from([(1, loop_)]),
            &geometry,
            &crate::test_support::test_b5::points([[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]]),
        ),
        BTreeMap::from([(3, [0, 1])])
    );
}

#[test]
fn missing_edge_parameter_incidence_uses_complete_pcurve_domain() {
    let pcurves = BTreeMap::from([(
        2,
        B5Pcurve {
            object_id: 2,
            surface: 4,
            degree: 1,
            distinct_knots: crate::test_support::test_b5::finite_lane(&[2.0, 8.0]),
            multiplicities: vec![2, 2],
            control_points: vec![
                crate::test_support::test_b5::finite_vector([2.0, 0.0]),
                crate::test_support::test_b5::finite_vector([8.0, 0.0]),
            ],
            weights: None,
            parameter_range: Some(crate::test_support::test_b5::finite_pair([2.0, 8.0])),
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        },
    )]);
    let surfaces = BTreeMap::from([(
        4,
        B5Surface::Plane {
            origin: crate::test_support::test_b5::point([0.0, 0.0, 0.0]),
            frame: crate::test_support::test_b5::plane_frame([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            direction_v: crate::test_support::test_b5::exact_unit([0.0, 1.0, 0.0]),
            u_range: crate::test_support::test_b5::increasing([0.0, 10.0]),
            v_range: crate::test_support::test_b5::increasing([-1.0, 1.0]),
        },
    )]);
    let opaque_pcurves = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };

    assert_eq!(
        pcurve_endpoints(2, 3, &geometry),
        Some(crate::test_support::test_b5::points([
            [2.0, 0.0, 0.0],
            [8.0, 0.0, 0.0]
        ]))
    );
}

#[test]
fn sphere_great_circle_pcurve_binds_native_incidence_coordinates() {
    let chart_scale = 8.0;
    let parameter = chart_scale * std::f64::consts::FRAC_PI_2;
    let opaque_pcurves = BTreeMap::from([(
        2,
        B5OpaquePcurve {
            object_id: 2,
            surface: 4,
            class: 0x1d,
            payload: Vec::new(),
            sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                u_bounds: crate::test_support::test_b5::increasing([0.0, parameter]),
                v_bounds: crate::test_support::test_b5::finite_pair([
                    0.0,
                    chart_scale * std::f64::consts::TAU,
                ]),
                chart_shift: crate::test_support::test_b5::finite(0.0),
                chart_scale: crate::test_support::test_b5::positive(chart_scale),
                slope: crate::test_support::test_b5::finite(0.0),
                phase: crate::test_support::test_b5::finite(0.0),
            }),
        },
    )]);
    let surfaces = BTreeMap::from([(
        4,
        B5Surface::Sphere {
            center: crate::test_support::test_b5::point([0.0, 0.0, 0.0]),
            frame: crate::test_support::test_b5::frame([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            direction_y: crate::test_support::test_b5::unit([0.0, 1.0, 0.0]),
            radius: crate::test_support::test_b5::positive_length(5.0),
            azimuth_range: crate::test_support::test_b5::increasing([0.0, std::f64::consts::TAU]),
            latitude_range: crate::test_support::test_b5::increasing([
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ]),
            construction_radius: crate::test_support::test_b5::positive_length(chart_scale),
            chart_origin: crate::test_support::test_b5::finite(0.0),
        },
    )]);
    let mut incidence_payload = vec![0x81, 0x82, 0x81];
    incidence_payload.extend_from_slice(&parameter.to_le_bytes());
    incidence_payload.push(0x01);
    let mut records = [
        B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x05,
            object_id: 20,
            payload: vec![0x82, 0x9e, 0x9f],
        },
        B5Record {
            offset: 1,
            family: 0xb5,
            class: 0x06,
            object_id: 30,
            payload: incidence_payload.clone(),
        },
        B5Record {
            offset: 2,
            family: 0xb5,
            class: 0x06,
            object_id: 31,
            payload: incidence_payload,
        },
    ];
    let by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let pcurves = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            counted_references(ctx, &records[0], 0x05).expect("service budget")
        }),
        Some(vec![30, 31])
    );
    let incidence = crate::test_support::with_service_context(|ctx| {
        parameter_incidence(ctx, &records[1]).expect("service budget")
    })
    .expect("parameter incidence");
    assert_eq!(
        incidence.lanes,
        [B5IncidenceLane {
            curve: 2,
            parameter: crate::test_support::test_b5::finite(parameter),
            // The compact token 0x01 encodes 4 * 0 + 1.
            control: 0,
        }]
    );
    assert!(
        distance_squared(
            sphere_great_circle_point(
                opaque_pcurves[&2]
                    .sphere_great_circle
                    .as_ref()
                    .expect("great circle"),
                &surfaces[&4],
                crate::test_support::test_b5::finite(parameter),
            )
            .map(crate::test_support::test_b5::coordinates)
            .expect("sphere endpoint"),
            [0.0, 5.0, 0.0]
        ) < 1e-24
    );
    let coordinates = crate::test_support::with_service_context(|ctx| {
        incidence_vertex_coordinates(
            ctx,
            &BTreeMap::from([(40, [10, 11])]),
            &BTreeMap::from([(
                10,
                B5VertexIncidenceLink {
                    object_id: 10,
                    incidence: 20,
                    terminal_control: B5VertexIncidenceControl::Control00,
                },
            )]),
            &by_id,
            &geometry,
        )
    })
    .expect("service budget");

    assert_eq!(coordinates.len(), 1);
    assert!(
        distance_squared(
            crate::test_support::test_b5::coordinates(
                *coordinates.get(&10).expect("native vertex coordinate")
            ),
            [0.0, 5.0, 0.0]
        ) < 1e-24
    );

    drop(by_id);
    records[2].payload[3..11].copy_from_slice(&0.0f64.to_le_bytes());
    let conflicting_by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let conflicting = crate::test_support::with_service_context(|ctx| {
        incidence_vertex_coordinates(
            ctx,
            &BTreeMap::from([(40, [10, 11])]),
            &BTreeMap::from([(
                10,
                B5VertexIncidenceLink {
                    object_id: 10,
                    incidence: 20,
                    terminal_control: B5VertexIncidenceControl::Control00,
                },
            )]),
            &conflicting_by_id,
            &geometry,
        )
    })
    .expect("service budget");
    assert!(conflicting.is_empty());

    drop(conflicting_by_id);
    records[2].payload[3..11].copy_from_slice(&(parameter + 1.0).to_le_bytes());
    let out_of_domain_by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let out_of_domain = crate::test_support::with_service_context(|ctx| {
        incidence_vertex_coordinates(
            ctx,
            &BTreeMap::from([(40, [10, 11])]),
            &BTreeMap::from([(
                10,
                B5VertexIncidenceLink {
                    object_id: 10,
                    incidence: 20,
                    terminal_control: B5VertexIncidenceControl::Control00,
                },
            )]),
            &out_of_domain_by_id,
            &geometry,
        )
    })
    .expect("service budget");
    assert!(out_of_domain.is_empty());
}

#[test]
fn conflicting_geometric_endpoints_defer_one_edge_to_native_identity() {
    let loops = BTreeMap::from([
        (
            1,
            B5Loop {
                object_id: 1,
                members: test_loop_members(&[10], &[20]),
                metadata: test_loop_metadata(),
                surface: 30,
            },
        ),
        (
            2,
            B5Loop {
                object_id: 2,
                members: test_loop_members(&[11], &[20]),
                metadata: test_loop_metadata(),
                surface: 31,
            },
        ),
        (
            3,
            B5Loop {
                object_id: 3,
                members: test_loop_members(&[12], &[21]),
                metadata: test_loop_metadata(),
                surface: 32,
            },
        ),
    ]);
    let pcurve = |object_id, endpoints| B5Pcurve {
        object_id,
        surface: object_id + 20,
        degree: 1,
        distinct_knots: crate::test_support::test_b5::finite_lane(&[0.0, 1.0]),
        multiplicities: vec![2, 2],
        control_points: vec![
            crate::test_support::test_b5::finite_vector([0.0, 0.0]),
            crate::test_support::test_b5::finite_vector([1.0, 0.0]),
        ],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: Some(crate::test_support::test_b5::points(endpoints)),
    };
    let pcurves = BTreeMap::from([
        (10, pcurve(10, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]])),
        (11, pcurve(11, [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]])),
        (12, pcurve(12, [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]])),
    ]);
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    let opaque_pcurves = BTreeMap::new();
    let surfaces = BTreeMap::new();
    let profiles = BTreeMap::new();
    let edge_parameter_incidences = BTreeMap::new();
    let parameter_incidences = BTreeMap::new();
    let geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };

    assert_eq!(
        bind_edge_vertices(
            &loops,
            &geometry,
            &points.map(crate::test_support::test_b5::point)
        ),
        BTreeMap::from([(21, [0, 2])])
    );
}
