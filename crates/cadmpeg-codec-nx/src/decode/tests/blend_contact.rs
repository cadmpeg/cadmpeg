// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};

use crate::decode::point_distance;

#[test]
fn nurbs_parameter_solver_inverts_a_rational_surface_point() {
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.37, 0.61);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = cadmpeg_ir::eval::nurbs_surface_closest_parameter(&surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);

    let after_invalid_seed = cadmpeg_ir::eval::nurbs_surface_closest_parameter(
        &surface,
        point,
        Some(Point2::new(f64::NAN, 0.5)),
    )
    .unwrap();
    assert!((after_invalid_seed.u - expected.u).abs() < 1.0e-10);
    assert!((after_invalid_seed.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn surface_intersection_continuation_corrects_a_chart_selected_branch() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let first = SurfaceId("synthetic:first-intersection-plane".into());
    let second = SurfaceId("synthetic:second-intersection-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let chart = vec![
        Point3::new(1.0e-4, -2.0e-4, 0.0),
        Point3::new(-1.0e-4, 2.0e-4, 2.0),
        Point3::new(2.0e-4, 1.0e-4, 5.0),
    ];
    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &chart,
        1.0e-3,
    )
    .unwrap();
    assert_eq!(lanes[0].len(), chart.len());
    for (ordinal, expected_z) in [0.0, 2.0, 5.0].into_iter().enumerate() {
        let first_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &first,
            lanes[0][ordinal].u,
            lanes[0][ordinal].v,
        )
        .unwrap();
        let second_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &second,
            lanes[1][ordinal].u,
            lanes[1][ordinal].v,
        )
        .unwrap();
        assert!((first_point.x - second_point.x).abs() < 1.0e-10);
        assert!((first_point.y - second_point.y).abs() < 1.0e-10);
        assert!((first_point.z - second_point.z).abs() < 1.0e-10);
        assert!((first_point.z - expected_z).abs() < 1.0e-10);
    }

    let off_branch = [chart[0], Point3::new(1.0, 1.0, 2.0)];
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &off_branch,
        1.0e-3,
    )
    .is_none());
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &first],
        &chart,
        1.0e-3,
    )
    .is_none());

    let cylinder = SurfaceId("synthetic:intersection-cylinder".into());
    let section_plane = SurfaceId("synthetic:intersection-section-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: section_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let circular_chart =
        [0.0_f64, 0.3, 0.8].map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let circular_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &circular_chart,
        1.0e-3,
    )
    .unwrap();
    for (cylinder_uv, plane_uv) in circular_lanes[0].iter().zip(&circular_lanes[1]) {
        let cylinder_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &cylinder,
            cylinder_uv.u,
            cylinder_uv.v,
        )
        .unwrap();
        let plane_point = cadmpeg_ir::eval::model_surface_point_by_id(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &section_plane,
            plane_uv.u,
            plane_uv.v,
        )
        .unwrap();
        assert!((cylinder_point.x - plane_point.x).abs() < 1.0e-8);
        assert!((cylinder_point.y - plane_point.y).abs() < 1.0e-8);
        assert!((cylinder_point.z - plane_point.z).abs() < 1.0e-8);
    }

    let tangent_cylinder = SurfaceId("synthetic:tangent-cylinder".into());
    let tangent_plane = SurfaceId("synthetic:tangent-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: tangent_cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(0.0, 0.0, -1.0),
                radius: 1.0,
            },
            source_object: None,
        },
        Surface {
            id: tangent_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let tangent_chart = [0.0, 1.0, 3.0, 6.0].map(|y| Point3::new(0.0, y, 0.0));
    let tangent_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&tangent_cylinder, &tangent_plane],
        &tangent_chart,
        1.0e-8,
    )
    .unwrap();
    for (ordinal, y) in [0.0, 1.0, 3.0, 6.0].into_iter().enumerate() {
        assert!((tangent_lanes[0][ordinal].v - y).abs() < 1.0e-10);
        assert!((tangent_lanes[1][ordinal].v - y).abs() < 1.0e-10);
    }

    let seam_chart = [3.0_f64, 3.1, 3.2, 3.3]
        .map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let seam_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &seam_chart,
        1.0e-3,
    )
    .unwrap();
    assert!(seam_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(seam_lanes[0].last().unwrap().u > std::f64::consts::PI);

    let periodic_nurbs = SurfaceId("synthetic:periodic-nurbs-prism".into());
    let nurbs_section = SurfaceId("synthetic:periodic-nurbs-section".into());
    let periodic_geometry = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points: [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)]
            .into_iter()
            .flat_map(|(x, y)| [Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)])
            .collect(),
        weights: None,
        normal_reversed: false,
        u_periodic: true,
        v_periodic: false,
    };
    ir.model.surfaces.extend([
        Surface {
            id: periodic_nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(periodic_geometry.clone()),
            source_object: None,
        },
        Surface {
            id: nurbs_section.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let nurbs_chart = [3.8, 3.9, 4.1, 4.2]
        .map(|u| cadmpeg_ir::eval::nurbs_surface_point(&periodic_geometry, u, 0.5).unwrap());
    let nurbs_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&periodic_nurbs, &nurbs_section],
        &nurbs_chart,
        1.0e-8,
    )
    .unwrap();
    assert!(nurbs_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(nurbs_lanes[0].last().unwrap().u > 4.0);
}

#[test]
fn surface_intersection_jacobian_is_stable_at_large_model_coordinates() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let horizontal = SurfaceId("synthetic:large-horizontal-plane".into());
    let vertical = SurfaceId("synthetic:large-vertical-plane".into());
    let origin = Point3::new(1.0e16, 1.0e16, 0.0);
    ir.model.surfaces.extend([
        Surface {
            id: horizontal.clone(),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: vertical.clone(),
            geometry: SurfaceGeometry::Plane {
                origin,
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let chart =
        [0.0, 4.0, 8.0].map(|distance| Point3::new(origin.x + distance, origin.y, origin.z));

    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&horizontal, &vertical],
        &chart,
        0.1,
    )
    .expect("exact plane partials keep the continuation Jacobian full rank");

    for (ordinal, expected) in [0.0, 4.0, 8.0].into_iter().enumerate() {
        assert_eq!(lanes[0][ordinal], Point2::new(expected, 0.0));
        assert_eq!(lanes[1][ordinal], Point2::new(expected, 0.0));
    }
}

#[test]
fn damped_intersection_correction_reduces_a_rank_deficient_system() {
    let matrix = [
        [1.0, 0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, -1.0],
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 1.0, 0.0],
    ];
    let rhs = [2.0, -4.0, 0.0, 6.0];

    let step = crate::decode::solve_damped_least_squares_4x4(matrix, rhs).unwrap();
    let residual = std::array::from_fn::<_, 4, _>(|row| {
        (0..4)
            .map(|column| matrix[row][column] * step[column])
            .sum::<f64>()
            - rhs[row]
    });

    assert!(residual.iter().all(|value| value.abs() < 1.0e-8));
    assert!(step.iter().all(|value| value.is_finite()));
    for (actual, expected) in step.into_iter().zip([4.0, -2.0, 2.0, 2.0]) {
        assert!((actual - expected).abs() < 1.0e-8);
    }
}

#[test]
fn periodic_surface_lookup_rejects_a_cyclic_offset_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let surfaces = [SurfaceId("cycle-a".into()), SurfaceId("cycle-b".into())];
    let constructions = [
        ProceduralSurfaceId("cycle-construction-a".into()),
        ProceduralSurfaceId("cycle-construction-b".into()),
    ];
    for side in 0..2 {
        ir.model.surfaces.push(Surface {
            id: surfaces[side].clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: constructions[side].clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            constructions[side].clone(),
            ProceduralSurfaceDefinition::Offset {
                support: surfaces[1 - side].clone(),
                distance: 1.0,
                u_sense: Some(0),
                v_sense: Some(0),
                support_extension: None,
                extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                ),
            },
            None,
        ));
    }

    let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
    assert_eq!(
        crate::decode::offset::surface_parameter_periods_with_index(&model_index, &surfaces[0]),
        [None, None]
    );
}

#[test]
fn nurbs_parameter_solver_rejects_a_remote_local_minimum_seed() {
    let mut control_points = Vec::new();
    for (x, z) in [
        (-10.0, 0.0),
        (0.0, 0.0),
        (10.0, 2.0),
        (0.0, 4.0),
        (-10.0, 4.0),
    ] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: None,
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.125, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = cadmpeg_ir::eval::nurbs_surface_closest_parameter(
        &surface,
        point,
        Some(Point2::new(0.875, 0.3)),
    )
    .unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_parameter_solver_preserves_close_equal_branches() {
    let mut control_points = Vec::new();
    for (x, z) in [(-1.0, 0.0), (0.0, 0.0), (1.0, 1.0), (0.0, 0.0), (-1.0, 2.0)] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.4999, 0.5, 0.5001, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: Some(vec![1.0, 1.2, 1.0, 1.2, 1.0, 1.2, 1.0, 1.2, 1.0, 1.2]),
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.5001, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = cadmpeg_ir::eval::nurbs_surface_closest_parameter(
        &surface,
        point,
        Some(Point2::new(0.50011, 0.3)),
    )
    .unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_curve_closest_parameter_does_not_trust_a_remote_seed() {
    use cadmpeg_ir::geometry::{Curve, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let curve = CurveId("synthetic:piecewise-spine".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(-10.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
            ],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });

    let actual = crate::decode::closest_spine_parameter(
        &ir,
        &curve,
        cadmpeg_ir::math::Point3::new(-5.0, 2.0, 0.0),
        Some(0.9),
    )
    .unwrap();

    assert!((actual - 0.25).abs() < 1.0e-10);
}

#[test]
fn spine_contact_pcurve_inverts_linear_and_rational_support_parameters() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 9.0, 9.0],
        control_points: vec![
            Point2::new(-1.0, 3.0),
            Point2::new(2.0, 6.0),
            Point2::new(6.0, 4.0),
        ],
        weights: None,
        periodic: false,
    };

    let first =
        crate::decode::closest_pcurve_parameters(&pcurve, Point2::new(0.5, 4.5), None).unwrap()[0];
    let second =
        crate::decode::closest_pcurve_parameters(&pcurve, Point2::new(5.0, 4.5), None).unwrap()[0];

    assert!((first - 3.5).abs() < 1.0e-12);
    assert!((second - 8.0).abs() < 1.0e-12);

    let rational = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };
    let rational_parameter =
        crate::decode::closest_pcurve_parameters(&rational, Point2::new(0.5, 0.0), None).unwrap()
            [0];
    assert!((rational_parameter - 1.0 / 3.0).abs() < 1.0e-10);

    let quadratic = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let quadratic_parameter =
        crate::decode::closest_pcurve_parameters(&quadratic, Point2::new(1.0, 0.5), None).unwrap()
            [0];
    assert!((quadratic_parameter - 0.5).abs() < 1.0e-10);

    let folded = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let first_fold =
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(0.1))
            .unwrap()[0];
    let second_fold =
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(1.9))
            .unwrap()[0];
    assert_eq!(first_fold, 0.0);
    assert_eq!(second_fold, 2.0);
    assert_eq!(
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(0.1))
            .unwrap(),
        [0.0, 2.0]
    );
    assert_eq!(
        crate::decode::closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(1.9))
            .unwrap(),
        [2.0, 0.0]
    );

    let mut rational_folded = folded.clone();
    let PcurveGeometry::Nurbs { weights, .. } = &mut rational_folded else {
        unreachable!("folded test pcurve is NURBS");
    };
    *weights = Some(vec![1.0; 3]);
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &rational_folded,
            Point2::new(0.0, 0.0),
            Some(0.1),
        )
        .unwrap(),
        [0.0, 2.0]
    );
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &rational_folded,
            Point2::new(0.0, 0.0),
            Some(1.9),
        )
        .unwrap(),
        [2.0, 0.0]
    );

    let quadratic_folded = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &quadratic_folded,
            Point2::new(0.0, 0.0),
            Some(0.1),
        )
        .unwrap(),
        [0.0, 1.0]
    );
    assert_eq!(
        crate::decode::closest_pcurve_parameters(
            &quadratic_folded,
            Point2::new(0.0, 0.0),
            Some(0.9),
        )
        .unwrap(),
        [1.0, 0.0]
    );
}

#[test]
fn blend_contact_offset_requires_the_radius_magnitude() {
    assert!(crate::decode::blend_contact_offset_matches(2.0, 5.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(2.0, -1.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(
        2.0,
        f64::from_bits(5.0f64.to_bits() + 1),
        3.0,
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0, 5.001, 3.0
    ));
}

#[test]
fn blend_contact_matches_separate_analytic_offset_carriers() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let support = SurfaceId("synthetic:support-cylinder".into());
    let offset = SurfaceId("synthetic:offset-cylinder".into());
    let cylinder = |id, radius| Surface {
        id,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(-46.75, 0.0, -112.06),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 0.0, -1.0),
            radius,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        cylinder(support.clone(), 294.0),
        cylinder(offset.clone(), 299.0),
    ]);

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    origin.y = 1.0;
    assert!(crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0).is_none());

    let support_plane = SurfaceId("synthetic:support-plane".into());
    let offset_plane = SurfaceId("synthetic:offset-plane".into());
    let plane = |id, origin| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(support_plane.clone(), Point3::new(10.0, 20.0, 30.0)),
        plane(offset_plane.clone(), Point3::new(10.0, 20.0, 35.0)),
    ]);
    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Plane { origin, .. } = &mut ir.model.surfaces[3].geometry else {
        unreachable!()
    };
    origin.x += 1.0;
    assert!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0)
            .is_none()
    );
}

#[test]
fn blend_contact_matches_concentric_blend_carriers() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let first = SurfaceId("synthetic:first".into());
    let second = SurfaceId("synthetic:second".into());
    let first_offset = SurfaceId("synthetic:first-offset".into());
    let second_offset = SurfaceId("synthetic:second-offset".into());
    let plane = |id, origin, normal, u_axis| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(
            first.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            first_offset.clone(),
            Point3::new(3.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second_offset.clone(),
            Point3::new(0.0, 3.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
    ]);

    let spine = CurveId("synthetic:shared-spine".into());
    let inner = SurfaceId("synthetic:inner-blend".into());
    let outer = SurfaceId("synthetic:outer-blend".into());
    for (surface, supports, radius) in [
        (inner.clone(), [first, second], 0.7),
        (outer.clone(), [first_offset, second_offset], 3.7),
    ] {
        let construction = ProceduralSurfaceId(format!("{}:construction", surface.0));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            construction,
            ProceduralSurfaceDefinition::Blend {
                supports: supports.map(|surface| {
                    Some(BlendSupport {
                        surface,
                        reversed: false,
                    })
                }),
                spine: Some(spine.clone()),
                radius: BlendRadiusLaw::Constant {
                    signed_radius: radius,
                },
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            None,
        ));
    }

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0),
        Some(3.0)
    );
    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| {
            candidate.id == ProceduralSurfaceId("synthetic:outer-blend:construction".into())
        })
        .unwrap();
    outer_definition.edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Blend { supports, .. } = definition else {
            unreachable!()
        };
        supports[0].as_mut().unwrap().reversed = true;
    });
    assert!(crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0).is_none());
}

#[test]
fn reverse_blend_contact_transfers_a_boundary_sample_to_its_support() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    const FIT_TOLERANCE: f64 = 1.0e-10;

    let support = SurfaceId("synthetic:reverse-contact-support".into());
    let support_offset = SurfaceId("synthetic:reverse-contact-support-offset".into());
    let other = SurfaceId("synthetic:reverse-contact-other".into());
    let blend = SurfaceId("synthetic:reverse-contact-blend".into());
    let spine = CurveId("synthetic:reverse-contact-spine".into());
    let spine_procedural = ProceduralCurveId("synthetic:reverse-contact-spine-record".into());
    let support_offset_construction =
        ProceduralSurfaceId("synthetic:reverse-contact-support-offset-record".into());
    let blend_construction = ProceduralSurfaceId("synthetic:reverse-contact-blend-record".into());
    let plane = |id, origin, normal| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.extend([
        plane(
            support.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(
            support_offset.clone(),
            Point3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(
            other.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
        Surface {
            id: blend.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: blend_construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ]);
    let _attached = ir.model.add_procedural_surface(
        support_offset.clone(),
        ProceduralSurface::new(
            support_offset_construction.clone(),
            ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 1.0,
                u_sense: None,
                v_sense: None,
                support_extension: None,
                extension: cadmpeg_ir::geometry::OffsetExtension::Legacy(
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                ),
            },
            None,
        ),
    );
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        blend_construction,
        ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: support.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: other.clone(),
                    reversed: false,
                }),
            ],
            spine: Some(spine.clone()),
            radius: BlendRadiusLaw::Constant { signed_radius: 1.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        None,
    ));
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let contact_pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: None,
        periodic: false,
    };
    let _attached = ir.model.add_procedural_curve(
        spine,
        ProceduralCurve::new(
            spine_procedural,
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(support_offset),
                            pcurve: Some(contact_pcurve.clone()),
                            pcurve_parameter_range: None,
                        },
                        IntcurveSupportSide {
                            surface: Some(other),
                            pcurve: Some(contact_pcurve),
                            pcurve_parameter_range: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
        ),
    );

    let source_pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: [Point2::new(0.0, 1.0), Point2::new(1.0, 1.0)].to_vec(),
        weights: None,
        periodic: false,
    };
    let parameter = 0.35;
    let expected = Point2::new(parameter, 0.0);
    let point = Point3::new(0.0, 0.0, parameter);
    let index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    let mut contact_seeds = crate::decode::blend::BlendContactSeedCache::default();
    let actual =
        crate::decode::blend::blend_support_parameter_from_source_pcurve_with_index_and_budget_and_seed_cache(
            &index,
            &blend,
            &support,
            &source_pcurve,
            parameter,
            crate::decode::blend::BoundaryInverseTarget {
                point,
                seed: None,
                tolerance: FIT_TOLERANCE,
            },
            &mut contact_seeds,
            &geometry_budget,
        )
        .expect("reverse contact relation transfers the certified boundary");
    assert!((actual.u - expected.u).abs() <= FIT_TOLERANCE);
    assert!((actual.v - expected.v).abs() <= FIT_TOLERANCE);
}

#[test]
fn closest_spine_parameter_inverts_periodic_analytic_curves() {
    use cadmpeg_ir::geometry::Curve;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let ellipse = CurveId("synthetic:ellipse-spine".into());
    let geometry = CurveGeometry::Ellipse {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 12.0,
        minor_radius: 5.0,
    };
    let parameter = 1.2;
    let mut point = cadmpeg_ir::eval::curve_point(&geometry, parameter).unwrap();
    point.y += 3.0;
    ir.model.curves.push(Curve {
        id: ellipse.clone(),
        geometry,
        source_object: None,
    });

    let first = crate::decode::closest_spine_parameter(&ir, &ellipse, point, None).unwrap();
    let continued = crate::decode::closest_spine_parameter(
        &ir,
        &ellipse,
        point,
        Some(parameter + std::f64::consts::TAU),
    )
    .unwrap();

    assert!((first - parameter).abs() < 1.0e-8, "{first}");
    assert!(
        (continued - parameter - std::f64::consts::TAU).abs() < 1.0e-8,
        "{continued}"
    );

    let center = Point3::new(2.0, 3.0, 4.0);
    let upper = crate::decode::closest_spine_parameter(&ir, &ellipse, center, Some(1.4)).unwrap();
    let lower = crate::decode::closest_spine_parameter(&ir, &ellipse, center, Some(4.8)).unwrap();
    assert!(
        (upper - std::f64::consts::FRAC_PI_2).abs() < 1.0e-8,
        "{upper}"
    );
    assert!(
        (lower - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-8,
        "{lower}"
    );
}

#[test]
fn rolling_ball_blend_parameters_invert_the_canal_surface_law() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::Edge;

    const OUTSIDE_BLEND_SECTION_DELTA: f64 = 1.0e-6;
    const DIRECT_INVERSE_TOLERANCE: f64 = 1.0e-8;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let first = SurfaceId("synthetic:first-plane".into());
    let second = SurfaceId("synthetic:second-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let first_spine_side = SurfaceId("synthetic:first-spine-side".into());
    let second_spine_side = SurfaceId("synthetic:second-spine-side".into());
    ir.model.surfaces.extend([
        Surface {
            id: first_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let surface = SurfaceId("synthetic:blend".into());
    let construction = ProceduralSurfaceId("synthetic:blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: first.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second.clone(),
                    reversed: false,
                }),
            ],
            spine: Some(spine.clone()),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        None,
    ));
    let expected = Point2::new(8.0, 0.35);
    let point = crate::decode::blend_surface_point(&ir, &surface, expected.u, expected.v).unwrap();
    let boundary_without_contact_chart =
        crate::decode::blend_surface_point(&ir, &surface, expected.u, 1.0)
            .expect("analytic supports provide a blend boundary without a spine pcurve");
    let boundary_without_contact_parameters = crate::decode::blend_surface_parameters(
        &ir,
        &surface,
        boundary_without_contact_chart,
        None,
    )
    .expect("blend inverse evaluates an analytic-support boundary");
    assert!((0.0..=1.0).contains(&boundary_without_contact_parameters.v));

    assert_eq!(
        crate::decode::support_uv::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        0.25
    );
    let procedural = ProceduralCurve::try_new(
        ProceduralCurveId("synthetic:spine-construction".into()),
        ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first_spine_side),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(second_spine_side),
                        pcurve_parameter_range: None,
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 10.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        Some(0.75),
    )
    .unwrap();
    ir.model
        .add_procedural_curve(spine.clone(), procedural)
        .unwrap();
    assert_eq!(
        crate::decode::support_uv::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        1.0
    );

    let actual = crate::decode::blend_surface_parameters(&ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let boundary_point =
        crate::decode::blend_surface_point(&ir, &surface, expected.u, 1.0).unwrap();
    let boundary_parameters =
        crate::decode::blend_surface_parameters(&ir, &surface, boundary_point, None)
            .expect("blend inverse returns the section boundary");
    assert!((0.0..=1.0).contains(&boundary_parameters.v));

    let outside_boundary_point = crate::decode::blend_surface_point(
        &ir,
        &surface,
        expected.u,
        1.0 + OUTSIDE_BLEND_SECTION_DELTA,
    )
    .unwrap();
    let outside_parameters =
        crate::decode::blend_surface_parameters(&ir, &surface, outside_boundary_point, None);
    assert!(outside_parameters.is_none());
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    let continuation_parameters =
        crate::decode::blend::blend_surface_parameters_for_fit_with_source_continuation_and_budget(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &surface,
            outside_boundary_point,
            None,
            1.0e-8,
            crate::decode::BlendParameterGrid::Disabled,
            &geometry_budget,
        )
        .expect("bounded source continuation admits the certified section point");
    assert!((continuation_parameters.u - expected.u).abs() < 1.0e-8);
    assert!((continuation_parameters.v - (1.0 + OUTSIDE_BLEND_SECTION_DELTA)).abs() < 1.0e-8);

    let direct_geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    let mut direct_contact_seeds = crate::decode::blend::BlendContactSeedCache::default();
    let direct_parameters =
        crate::decode::blend::blend_surface_parameters_from_point_with_index_and_budget(
            &cadmpeg_ir::index::ModelIndex::new(&ir),
            &surface,
            outside_boundary_point,
            None,
            DIRECT_INVERSE_TOLERANCE,
            &mut direct_contact_seeds,
            &direct_geometry_budget,
        )
        .expect("direct blend inverse admits a certified continuation point");
    assert!((direct_parameters.u - expected.u).abs() < DIRECT_INVERSE_TOLERANCE);
    assert!(
        (direct_parameters.v - (1.0 + OUTSIDE_BLEND_SECTION_DELTA)).abs()
            < DIRECT_INVERSE_TOLERANCE
    );

    let continued = crate::decode::blend_surface_parameters_for_fit(
        &ir,
        &surface,
        point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-8,
    )
    .unwrap();
    assert!((continued.u - expected.u).abs() < 1.0e-8);
    assert!((continued.v - expected.v).abs() < 1.0e-8);

    let mut varying_frame = ir.clone();
    varying_frame
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Parabola {
        vertex: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        focal_distance: 0.5,
    };
    varying_frame
        .model
        .procedural_curves
        .iter_mut()
        .find(|curve| curve.id == ProceduralCurveId("synthetic:spine-construction".into()))
        .unwrap()
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                unreachable!()
            };
            context.sides[0].pcurve = Some(PcurveGeometry::Offset {
                distance: 0.1,
                basis: Box::new(context.sides[0].pcurve.take().unwrap()),
            });
        });
    let parameters = Point2::new(0.4, 0.35);
    let exact = crate::decode::blend_surface_u_derivative(
        &varying_frame,
        &surface,
        parameters.u,
        parameters.v,
        0,
    )
    .expect("complete rolling-ball frame has an exact derivative");
    let step = 1.0e-6;
    let before = crate::decode::blend_surface_point(
        &varying_frame,
        &surface,
        parameters.u - step,
        parameters.v,
    )
    .unwrap();
    let after = crate::decode::blend_surface_point(
        &varying_frame,
        &surface,
        parameters.u + step,
        parameters.v,
    )
    .unwrap();
    let numerical = Vector3::new(
        (after.x - before.x) / (2.0 * step),
        (after.y - before.y) / (2.0 * step),
        (after.z - before.z) / (2.0 * step),
    );
    assert!((exact.x - numerical.x).abs() < 1.0e-7);
    assert!((exact.y - numerical.y).abs() < 1.0e-7);
    assert!((exact.z - numerical.z).abs() < 1.0e-7);

    let mut translated = ir.clone();
    for carrier in &mut translated.model.surfaces {
        if let SurfaceGeometry::Plane { origin, .. } = &mut carrier.geometry {
            origin.x += 1.0e12;
            origin.y += 1.0e12;
            origin.z += 1.0e12;
        }
    }
    let CurveGeometry::Line { origin, .. } = &mut translated
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .expect("translated spine")
        .geometry
    else {
        unreachable!()
    };
    origin.x += 1.0e12;
    origin.y += 1.0e12;
    origin.z += 1.0e12;
    let translated_point =
        crate::decode::blend_surface_point(&translated, &surface, expected.u, expected.v).unwrap();
    let translated_parameters = crate::decode::blend_surface_parameters_for_fit(
        &translated,
        &surface,
        translated_point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-3,
    )
    .expect("exact section tangent is independent of model-space magnitude");
    assert!((translated_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((translated_parameters.v - expected.v).abs() < 1.0e-3);

    let boundary_curve = CurveId("synthetic:blend-boundary-curve".into());
    ir.model.curves.push(Curve {
        id: boundary_curve.clone(),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        boundary_curve.clone(),
        ProceduralCurve::new(
            ProceduralCurveId("synthetic:blend-boundary".into()),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(first.clone()),
                            pcurve_parameter_range: None,
                            pcurve: Some(PcurveGeometry::Line {
                                origin: Point2::new(0.0, -2.0),
                                direction: Point2::new(1.0, 0.0),
                            }),
                        },
                        IntcurveSupportSide {
                            surface: Some(surface.clone()),
                            pcurve_parameter_range: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
        ),
    );
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:blend-boundary-edge".into()),
        curve: Some(boundary_curve),
        start: VertexId("synthetic:blend-boundary-start".into()),
        end: VertexId("synthetic:blend-boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });
    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves.last().unwrap().definition()
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|procedural| {
            procedural.id == ProceduralCurveId("synthetic:spine-construction".into())
        })
        .unwrap()
        .replace_definition(ProceduralCurveDefinition::Unknown {
            native_kind: None,
            record: None,
        });
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );

    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
        ],
        weights: None,
        periodic: false,
    });
    let coarse = crate::decode::coarse_blend_surface_parameters(&ir, &surface, point, 0).unwrap();
    let coarse_point =
        crate::decode::blend_surface_point(&ir, &surface, coarse.u, coarse.v).unwrap();
    assert!(
        ((coarse_point.x - point.x).powi(2)
            + (coarse_point.y - point.y).powi(2)
            + (coarse_point.z - point.z).powi(2))
        .sqrt()
            < 1.0
    );

    let refined = crate::decode::refine_blend_surface_parameters(
        &ir,
        &surface,
        point,
        Point2::new(expected.u + 0.5, expected.v + 0.1),
        0,
    )
    .unwrap();
    let refined_point =
        crate::decode::blend_surface_point(&ir, &surface, refined.u, refined.v).unwrap();
    let refined_error = ((refined_point.x - point.x).powi(2)
        + (refined_point.y - point.y).powi(2)
        + (refined_point.z - point.z).powi(2))
    .sqrt();
    assert!(refined_error < 1.0e-9);

    let third = SurfaceId("synthetic:third-plane".into());
    ir.model.surfaces.push(Surface {
        id: third.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer_spine = CurveId("synthetic:outer-spine".into());
    ir.model.curves.push(Curve {
        id: outer_spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer = SurfaceId("synthetic:outer-blend".into());
    let outer_construction = ProceduralSurfaceId("synthetic:outer-blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: outer.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: outer_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        outer_construction,
        ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface,
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: third,
                    reversed: false,
                }),
            ],
            spine: Some(outer_spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 1.5 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        None,
    ));
    let expected = Point2::new(4.0, 0.2);
    let point = crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).unwrap();
    let outer_geometry = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| candidate.id == outer)
        .map(|surface| &surface.geometry)
        .unwrap();
    let index = cadmpeg_ir::index::ModelIndex::new(&ir);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );
    let evaluated = crate::decode::blend::decoded_surface_point_with_geometry_and_budget(
        &index,
        &outer,
        outer_geometry,
        expected.u,
        expected.v,
        0,
        &geometry_budget,
    )
    .expect("budgeted evaluation handles a nested blend support");
    assert!(point_distance(evaluated, point) <= 64.0 * f64::EPSILON);
    let actual = crate::decode::blend_surface_parameters(&ir, &outer, point, None).unwrap();
    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| {
            candidate.id == ProceduralSurfaceId("synthetic:outer-blend-construction".into())
        })
        .unwrap();
    outer_definition.edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Blend { supports, .. } = definition else {
            panic!("blend definition");
        };
        supports[0].as_mut().unwrap().surface = outer.clone();
    });
    assert!(crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).is_none());
}
