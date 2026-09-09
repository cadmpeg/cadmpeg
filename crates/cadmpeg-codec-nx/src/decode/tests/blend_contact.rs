// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]

const EPS_TOPOLOGY_TOLERANCE: f64 = 1.0e-8;

use crate::decode::blend::{
    blend_contact_offset_matches, blend_surface_parameters, blend_surface_parameters_for_fit,
    blend_surface_point, blend_surface_u_derivative, closest_pcurve_parameters,
    closest_spine_parameter, coarse_blend_surface_parameters, constant_surface_offset_between,
    refine_blend_surface_parameters, BlendParameterGrid,
};
use crate::decode::offset::{
    continue_surface_intersection_parameters, point_distance, solve_damped_least_squares_4x4,
};
use crate::decode::pcurves::blend_boundary_parameter_from_support_spine;

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};

fn test_surface(
    u_knots: Vec<f64>,
    u_count: u32,
    control_points: Vec<cadmpeg_ir::math::Point3>,
    weights: Option<Vec<f64>>,
    u_periodic: bool,
) -> cadmpeg_ir::geometry::NurbsSurface {
    cadmpeg_ir::geometry::NurbsSurface::new(
        1,
        1,
        u_knots,
        vec![0.0, 0.0, 1.0, 1.0],
        u_count,
        2,
        control_points,
        weights,
        false,
        u_periodic,
        false,
    )
    .unwrap()
}

fn test_pcurve(
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<Point2>,
    weights: Option<Vec<f64>>,
) -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::PcurveNurbs::new(
            degree,
            knots,
            control_points,
            weights,
            false,
        )
        .unwrap(),
    }
}

#[test]
fn nurbs_parameter_solver_inverts_a_rational_surface_point() {
    let surface = test_surface(
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 3.0, 4.0]),
        false,
    );
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
    let first = SurfaceId::mint("test:model:entity#synthetic:first-intersection-plane")
        .expect("identity grammar");
    let second = SurfaceId::mint("test:model:entity#synthetic:second-intersection-plane")
        .expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let chart = vec![
        Point3::new(1.0e-4, -2.0e-4, 0.0),
        Point3::new(-1.0e-4, 2.0e-4, 2.0),
        Point3::new(2.0e-4, 1.0e-4, 5.0),
    ];
    let lanes =
        continue_surface_intersection_parameters(&ir, [&first, &second], &chart, 1.0e-3).unwrap();
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
    assert!(
        continue_surface_intersection_parameters(&ir, [&first, &second], &off_branch, 1.0e-3,)
            .is_none()
    );
    assert!(
        continue_surface_intersection_parameters(&ir, [&first, &first], &chart, 1.0e-3,).is_none()
    );

    let cylinder = SurfaceId::mint("test:model:entity#synthetic:intersection-cylinder")
        .expect("identity grammar");
    let section_plane = SurfaceId::mint("test:model:entity#synthetic:intersection-section-plane")
        .expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: section_plane.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let circular_chart =
        [0.0_f64, 0.3, 0.8].map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let circular_lanes = continue_surface_intersection_parameters(
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

    let tangent_cylinder =
        SurfaceId::mint("test:model:entity#synthetic:tangent-cylinder").expect("identity grammar");
    let tangent_plane =
        SurfaceId::mint("test:model:entity#synthetic:tangent-plane").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: tangent_cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::CylinderSurface::try_new(
                    Point3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, -1.0),
                    1.0,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: tangent_plane.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let tangent_chart = [0.0, 1.0, 3.0, 6.0].map(|y| Point3::new(0.0, y, 0.0));
    let tangent_lanes = continue_surface_intersection_parameters(
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
    let seam_lanes = continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &seam_chart,
        1.0e-3,
    )
    .unwrap();
    assert!(seam_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(seam_lanes[0].last().unwrap().u > std::f64::consts::PI);

    let periodic_nurbs = SurfaceId::mint("test:model:entity#synthetic:periodic-nurbs-prism")
        .expect("identity grammar");
    let nurbs_section = SurfaceId::mint("test:model:entity#synthetic:periodic-nurbs-section")
        .expect("identity grammar");
    let periodic_geometry = test_surface(
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        5,
        [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)]
            .into_iter()
            .flat_map(|(x, y)| [Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)])
            .collect(),
        None,
        true,
    );
    ir.model.surfaces.extend([
        Surface {
            id: periodic_nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(periodic_geometry.clone()),
            source_object: None,
        },
        Surface {
            id: nurbs_section.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.5),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let nurbs_chart = [3.8, 3.9, 4.1, 4.2]
        .map(|u| cadmpeg_ir::eval::nurbs_surface_point(&periodic_geometry, u, 0.5).unwrap());
    let nurbs_lanes = continue_surface_intersection_parameters(
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
    let horizontal = SurfaceId::mint("test:model:entity#synthetic:large-horizontal-plane")
        .expect("identity grammar");
    let vertical = SurfaceId::mint("test:model:entity#synthetic:large-vertical-plane")
        .expect("identity grammar");
    let origin = Point3::new(1.0e16, 1.0e16, 0.0);
    ir.model.surfaces.extend([
        Surface {
            id: horizontal.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    origin,
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: vertical.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    origin,
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let chart =
        [0.0, 4.0, 8.0].map(|distance| Point3::new(origin.x + distance, origin.y, origin.z));

    let lanes =
        continue_surface_intersection_parameters(&ir, [&horizontal, &vertical], &chart, 0.1)
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

    let step = solve_damped_least_squares_4x4(matrix, rhs).unwrap();
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
    let surfaces = [
        SurfaceId::mint("test:model:entity#cycle-a").expect("identity grammar"),
        SurfaceId::mint("test:model:entity#cycle-b").expect("identity grammar"),
    ];
    let constructions = [
        ProceduralSurfaceId::mint("test:model:entity#cycle-construction-a")
            .expect("identity grammar"),
        ProceduralSurfaceId::mint("test:model:entity#cycle-construction-b")
            .expect("identity grammar"),
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
        ir.model.procedural_surfaces.push(
            ProceduralSurface::new(
                constructions[side].clone(),
                ProceduralSurfaceDefinition::Offset(
                    cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                        surfaces[1 - side].clone(),
                        1.0,
                        Some(0),
                        Some(0),
                        None,
                        cadmpeg_ir::geometry::OffsetExtension::Legacy(
                            cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                        ),
                    )
                    .unwrap(),
                ),
                None,
            )
            .unwrap(),
        );
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
    let surface = test_surface(
        vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        5,
        control_points,
        None,
        false,
    );
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
    let surface = test_surface(
        vec![0.0, 0.0, 0.4999, 0.5, 0.5001, 1.0, 1.0],
        5,
        control_points,
        Some(vec![1.0, 1.2, 1.0, 1.2, 1.0, 1.2, 1.0, 1.2, 1.0, 1.2]),
        false,
    );
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
    let curve =
        CurveId::mint("test:model:entity#synthetic:piecewise-spine").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 0.5, 1.0, 1.0],
                vec![
                    cadmpeg_ir::math::Point3::new(-10.0, 0.0, 0.0),
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
                ],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });

    let actual = closest_spine_parameter(
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
    let pcurve = test_pcurve(
        1,
        vec![2.0, 2.0, 5.0, 9.0, 9.0],
        vec![
            Point2::new(-1.0, 3.0),
            Point2::new(2.0, 6.0),
            Point2::new(6.0, 4.0),
        ],
        None,
    );

    let first = closest_pcurve_parameters(&pcurve, Point2::new(0.5, 4.5), None).unwrap()[0];
    let second = closest_pcurve_parameters(&pcurve, Point2::new(5.0, 4.5), None).unwrap()[0];

    assert!((first - 3.5).abs() < 1.0e-12);
    assert!((second - 8.0).abs() < 1.0e-12);

    let rational = test_pcurve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, 2.0]),
    );
    let rational_parameter =
        closest_pcurve_parameters(&rational, Point2::new(0.5, 0.0), None).unwrap()[0];
    assert!((rational_parameter - 1.0 / 3.0).abs() < 1.0e-10);

    let quadratic = test_pcurve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 0.0),
        ],
        None,
    );
    let quadratic_parameter =
        closest_pcurve_parameters(&quadratic, Point2::new(1.0, 0.5), None).unwrap()[0];
    assert!((quadratic_parameter - 0.5).abs() < 1.0e-10);

    let folded = test_pcurve(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        None,
    );
    let first_fold =
        closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(0.1)).unwrap()[0];
    let second_fold =
        closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(1.9)).unwrap()[0];
    assert_eq!(first_fold, 0.0);
    assert_eq!(second_fold, 2.0);
    assert_eq!(
        closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(0.1)).unwrap(),
        [0.0, 2.0]
    );
    assert_eq!(
        closest_pcurve_parameters(&folded, Point2::new(0.0, 0.0), Some(1.9)).unwrap(),
        [2.0, 0.0]
    );

    let mut rational_folded = folded.clone();
    let PcurveGeometry::Nurbs { nurbs } = &mut rational_folded else {
        unreachable!("folded test pcurve is NURBS");
    };
    *nurbs = cadmpeg_ir::geometry::PcurveNurbs::new(
        nurbs.degree(),
        nurbs.knots().to_vec(),
        nurbs.control_points().to_vec(),
        Some(vec![1.0; 3]),
        nurbs.periodic(),
    )
    .unwrap();
    assert_eq!(
        closest_pcurve_parameters(&rational_folded, Point2::new(0.0, 0.0), Some(0.1),).unwrap(),
        [0.0, 2.0]
    );
    assert_eq!(
        closest_pcurve_parameters(&rational_folded, Point2::new(0.0, 0.0), Some(1.9),).unwrap(),
        [2.0, 0.0]
    );

    let quadratic_folded = test_pcurve(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 0.0),
        ],
        None,
    );
    assert_eq!(
        closest_pcurve_parameters(&quadratic_folded, Point2::new(0.0, 0.0), Some(0.1),).unwrap(),
        [0.0, 1.0]
    );
    assert_eq!(
        closest_pcurve_parameters(&quadratic_folded, Point2::new(0.0, 0.0), Some(0.9),).unwrap(),
        [1.0, 0.0]
    );
}

#[test]
fn blend_contact_offset_requires_the_radius_magnitude() {
    assert!(blend_contact_offset_matches(2.0, 5.0, 3.0));
    assert!(blend_contact_offset_matches(2.0, -1.0, 3.0));
    assert!(blend_contact_offset_matches(
        2.0,
        f64::from_bits(5.0f64.to_bits() + 1),
        3.0,
    ));
    assert!(!blend_contact_offset_matches(2.0, 5.001, 3.0));
}

#[test]
fn blend_contact_matches_separate_analytic_offset_carriers() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let support =
        SurfaceId::mint("test:model:entity#synthetic:support-cylinder").expect("identity grammar");
    let offset =
        SurfaceId::mint("test:model:entity#synthetic:offset-cylinder").expect("identity grammar");
    let cylinder = |id, radius| Surface {
        id,
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(-46.75, 0.0, -112.06),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                radius,
            )
            .unwrap(),
        ),
        source_object: None,
    };
    ir.model.surfaces.extend([
        cylinder(support.clone(), 294.0),
        cylinder(offset.clone(), 299.0),
    ]);

    assert_eq!(
        constant_surface_offset_between(&ir, &support, &offset, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Cylinder(cylinder_surface) = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    let origin = cylinder_surface.origin();
    let axis = cylinder_surface.axis();
    let ref_direction = cylinder_surface.ref_direction();
    let radius = &cylinder_surface.radius();
    let mut origin = *origin;
    origin.y = 1.0;
    *cylinder_surface =
        cadmpeg_ir::geometry::CylinderSurface::try_new(origin, *axis, *ref_direction, *radius)
            .unwrap();
    assert!(constant_surface_offset_between(&ir, &support, &offset, 0).is_none());

    let support_plane =
        SurfaceId::mint("test:model:entity#synthetic:support-plane").expect("identity grammar");
    let offset_plane =
        SurfaceId::mint("test:model:entity#synthetic:offset-plane").expect("identity grammar");
    let plane = |id, origin| Surface {
        id,
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                origin,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(support_plane.clone(), Point3::new(10.0, 20.0, 30.0)),
        plane(offset_plane.clone(), Point3::new(10.0, 20.0, 35.0)),
    ]);
    assert_eq!(
        constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Plane(plane_surface) = &mut ir.model.surfaces[3].geometry else {
        unreachable!()
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.normal();
    let u_axis = plane_surface.u_axis();
    let mut origin = *origin;
    origin.x += 1.0;
    *plane_surface = cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
    assert!(constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0).is_none());
}

#[test]
fn blend_contact_matches_concentric_blend_carriers() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let first = SurfaceId::mint("test:model:entity#synthetic:first").expect("identity grammar");
    let second = SurfaceId::mint("test:model:entity#synthetic:second").expect("identity grammar");
    let first_offset =
        SurfaceId::mint("test:model:entity#synthetic:first-offset").expect("identity grammar");
    let second_offset =
        SurfaceId::mint("test:model:entity#synthetic:second-offset").expect("identity grammar");
    let plane = |id, origin, normal, u_axis| Surface {
        id,
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(origin, normal, u_axis).unwrap(),
        ),
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

    let spine =
        CurveId::mint("test:model:entity#synthetic:shared-spine").expect("identity grammar");
    let inner =
        SurfaceId::mint("test:model:entity#synthetic:inner-blend").expect("identity grammar");
    let outer =
        SurfaceId::mint("test:model:entity#synthetic:outer-blend").expect("identity grammar");
    for (surface, supports, radius) in [
        (inner.clone(), [first, second], 0.7),
        (outer.clone(), [first_offset, second_offset], 3.7),
    ] {
        let construction =
            ProceduralSurfaceId::mint(format!("{surface}:construction")).expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(
            ProceduralSurface::new(
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
            )
            .unwrap(),
        );
    }

    assert_eq!(
        constant_surface_offset_between(&ir, &inner, &outer, 0),
        Some(3.0)
    );
    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| {
            candidate.id
                == ProceduralSurfaceId::mint("test:model:entity#synthetic:outer-blend:construction")
                    .expect("identity grammar")
        })
        .unwrap();
    outer_definition
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend { supports, .. } = definition else {
                unreachable!()
            };
            supports[0].as_mut().unwrap().reversed = true;
        })
        .unwrap();
    assert!(constant_surface_offset_between(&ir, &inner, &outer, 0).is_none());
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

    let support = SurfaceId::mint("test:model:entity#synthetic:reverse-contact-support")
        .expect("identity grammar");
    let support_offset =
        SurfaceId::mint("test:model:entity#synthetic:reverse-contact-support-offset")
            .expect("identity grammar");
    let other = SurfaceId::mint("test:model:entity#synthetic:reverse-contact-other")
        .expect("identity grammar");
    let blend = SurfaceId::mint("test:model:entity#synthetic:reverse-contact-blend")
        .expect("identity grammar");
    let spine = CurveId::mint("test:model:entity#synthetic:reverse-contact-spine")
        .expect("identity grammar");
    let spine_procedural =
        ProceduralCurveId::mint("test:model:entity#synthetic:reverse-contact-spine-record")
            .expect("identity grammar");
    let support_offset_construction = ProceduralSurfaceId::mint(
        "test:model:entity#synthetic:reverse-contact-support-offset-record",
    )
    .expect("identity grammar");
    let blend_construction =
        ProceduralSurfaceId::mint("test:model:entity#synthetic:reverse-contact-blend-record")
            .expect("identity grammar");
    let plane = |id, origin, normal| Surface {
        id,
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                origin,
                normal,
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
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
            ProceduralSurfaceDefinition::Offset(
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    support.clone(),
                    1.0,
                    None,
                    None,
                    None,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy(
                        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent,
                    ),
                )
                .unwrap(),
            ),
            None,
        )
        .unwrap(),
    );
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
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
        )
        .unwrap(),
    );
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let contact_pcurve = test_pcurve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        None,
    );
    let _attached = ir.model.add_procedural_curve(
        spine,
        ProceduralCurve::new(
            spine_procedural,
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(support_offset),
                            pcurve: Some(contact_pcurve.clone().into()),
                        },
                        IntcurveSupportSide {
                            surface: Some(other),
                            pcurve: Some(contact_pcurve.into()),
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );

    let source_pcurve = test_pcurve(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        [Point2::new(0.0, 1.0), Point2::new(1.0, 1.0)].to_vec(),
        None,
    );
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
    let ellipse =
        CurveId::mint("test:model:entity#synthetic:ellipse-spine").expect("identity grammar");
    let geometry = CurveGeometry::Ellipse(
        cadmpeg_ir::geometry::EllipseCurve::try_new(
            Point3::new(2.0, 3.0, 4.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            12.0,
            5.0,
        )
        .unwrap(),
    );
    let parameter = 1.2;
    let mut point = cadmpeg_ir::eval::curve_point(&geometry, parameter).unwrap();
    point.y += 3.0;
    ir.model.curves.push(Curve {
        id: ellipse.clone(),
        geometry,
        source_object: None,
    });

    let first = closest_spine_parameter(&ir, &ellipse, point, None).unwrap();
    let continued = closest_spine_parameter(
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
    let upper = closest_spine_parameter(&ir, &ellipse, center, Some(1.4)).unwrap();
    let lower = closest_spine_parameter(&ir, &ellipse, center, Some(4.8)).unwrap();
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
    let first =
        SurfaceId::mint("test:model:entity#synthetic:first-plane").expect("identity grammar");
    let second =
        SurfaceId::mint("test:model:entity#synthetic:second-plane").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let first_spine_side =
        SurfaceId::mint("test:model:entity#synthetic:first-spine-side").expect("identity grammar");
    let second_spine_side =
        SurfaceId::mint("test:model:entity#synthetic:second-spine-side").expect("identity grammar");
    ir.model.surfaces.extend([
        Surface {
            id: first_spine_side.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: second_spine_side.clone(),
            geometry: SurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ]);
    let spine = CurveId::mint("test:model:entity#synthetic:spine").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let surface = SurfaceId::mint("test:model:entity#synthetic:blend").expect("identity grammar");
    let construction = ProceduralSurfaceId::mint("test:model:entity#synthetic:blend-construction")
        .expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
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
        )
        .unwrap(),
    );
    let expected = Point2::new(8.0, 0.35);
    let point = blend_surface_point(&ir, &surface, expected.u, expected.v).unwrap();
    let boundary_without_contact_chart = blend_surface_point(&ir, &surface, expected.u, 1.0)
        .expect("analytic supports provide a blend boundary without a spine pcurve");
    let boundary_without_contact_parameters =
        blend_surface_parameters(&ir, &surface, boundary_without_contact_chart, None)
            .expect("blend inverse evaluates an analytic-support boundary");
    assert!((0.0..=1.0).contains(&boundary_without_contact_parameters.v));

    assert_eq!(
        crate::decode::support_uv::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        0.25
    );
    let procedural = ProceduralCurve::try_new(
        ProceduralCurveId::mint("test:model:entity#synthetic:spine-construction")
            .expect("identity grammar"),
        ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext::try_new(
                [
                    IntcurveSupportSide {
                        surface: Some(first_spine_side),
                        pcurve: Some(
                            PcurveGeometry::Line(
                                cadmpeg_ir::geometry::LinePcurve::try_new(
                                    Point2::new(0.0, -2.0),
                                    Point2::new(1.0, 0.0),
                                )
                                .unwrap(),
                            )
                            .into(),
                        ),
                    },
                    IntcurveSupportSide {
                        surface: Some(second_spine_side),
                        pcurve: Some(
                            PcurveGeometry::Line(
                                cadmpeg_ir::geometry::LinePcurve::try_new(
                                    Point2::new(0.0, 2.0),
                                    Point2::new(1.0, 0.0),
                                )
                                .unwrap(),
                            )
                            .into(),
                        ),
                    },
                ],
                [0.0, 10.0],
                [Vec::new(), Vec::new(), Vec::new()],
            )
            .unwrap(),
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

    let actual = blend_surface_parameters(&ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let boundary_point = blend_surface_point(&ir, &surface, expected.u, 1.0).unwrap();
    let boundary_parameters = blend_surface_parameters(&ir, &surface, boundary_point, None)
        .expect("blend inverse returns the section boundary");
    assert!((0.0..=1.0).contains(&boundary_parameters.v));

    let outside_boundary_point =
        blend_surface_point(&ir, &surface, expected.u, 1.0 + OUTSIDE_BLEND_SECTION_DELTA).unwrap();
    let outside_parameters = blend_surface_parameters(&ir, &surface, outside_boundary_point, None);
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
            BlendParameterGrid::Disabled,
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

    let continued = blend_surface_parameters_for_fit(
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
    let carrier = varying_frame
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap();
    let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry else {
        panic!("procedural spine carrier");
    };
    *cache = Some(
        cadmpeg_ir::geometry::SolvedCurveGeometry::new(CurveGeometry::Parabola(
            cadmpeg_ir::geometry::ParabolaCurve::try_new(
                cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                0.5,
            )
            .unwrap(),
        ))
        .expect("solved parabola"),
    );
    varying_frame
        .model
        .procedural_curves
        .iter_mut()
        .find(|curve| {
            curve.id
                == ProceduralCurveId::mint("test:model:entity#synthetic:spine-construction")
                    .expect("identity grammar")
        })
        .unwrap()
        .edit_definition(|definition| {
            let ProceduralCurveDefinition::Intersection { context, .. } = definition else {
                unreachable!()
            };
            context
                .edit(|context_sides, _, _| {
                    (*context_sides)[0].pcurve = Some(
                        PcurveGeometry::Offset(
                            cadmpeg_ir::geometry::OffsetPcurve::try_new(
                                0.1,
                                Box::new((*context_sides)[0].pcurve.take().unwrap().geometry),
                            )
                            .unwrap(),
                        )
                        .into(),
                    );
                })
                .unwrap();
        })
        .unwrap();
    let parameters = Point2::new(0.4, 0.35);
    let exact = blend_surface_u_derivative(&varying_frame, &surface, parameters.u, parameters.v, 0)
        .expect("complete rolling-ball frame has an exact derivative");
    let step = 1.0e-6;
    let before =
        blend_surface_point(&varying_frame, &surface, parameters.u - step, parameters.v).unwrap();
    let after =
        blend_surface_point(&varying_frame, &surface, parameters.u + step, parameters.v).unwrap();
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
        if let SurfaceGeometry::Plane(plane_surface) = &mut carrier.geometry {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
            let mut origin = *origin;
            origin.x += 1.0e12;
            origin.y += 1.0e12;
            origin.z += 1.0e12;
            *plane_surface =
                cadmpeg_ir::geometry::PlaneSurface::try_new(origin, *normal, *u_axis).unwrap();
        }
    }
    let carrier = translated
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .expect("translated spine");
    let CurveGeometry::Procedural {
        cache: Some(cache), ..
    } = &mut carrier.geometry
    else {
        panic!("procedural spine cache");
    };
    let mut geometry = cache.as_geometry().clone();
    let CurveGeometry::Line(line_curve) = &mut geometry else {
        panic!("line spine cache");
    };
    let origin = line_curve.origin();
    let direction = line_curve.direction();
    let mut origin = *origin;
    origin.x += 1.0e12;
    origin.y += 1.0e12;
    origin.z += 1.0e12;
    *line_curve = cadmpeg_ir::geometry::LineCurve::try_new(origin, *direction).unwrap();
    *cache = cadmpeg_ir::geometry::SolvedCurveGeometry::new(geometry).expect("translated line");
    let translated_point =
        blend_surface_point(&translated, &surface, expected.u, expected.v).unwrap();
    let translated_parameters = blend_surface_parameters_for_fit(
        &translated,
        &surface,
        translated_point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-3,
    )
    .expect("exact section tangent is independent of model-space magnitude");
    assert!((translated_parameters.u - expected.u).abs() < 1.0e-3);
    assert!((translated_parameters.v - expected.v).abs() < 1.0e-3);

    let boundary_curve = CurveId::mint("test:model:entity#synthetic:blend-boundary-curve")
        .expect("identity grammar");
    ir.model.curves.push(Curve {
        id: boundary_curve.clone(),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: None,
    });
    let _attached = ir.model.add_procedural_curve(
        boundary_curve.clone(),
        ProceduralCurve::new(
            ProceduralCurveId::mint("test:model:entity#synthetic:blend-boundary")
                .expect("identity grammar"),
            ProceduralCurveDefinition::Intersection {
                context: IntcurveSupportContext::try_new(
                    [
                        IntcurveSupportSide {
                            surface: Some(first.clone()),
                            pcurve: Some(
                                PcurveGeometry::Line(
                                    cadmpeg_ir::geometry::LinePcurve::try_new(
                                        Point2::new(0.0, -2.0),
                                        Point2::new(1.0, 0.0),
                                    )
                                    .unwrap(),
                                )
                                .into(),
                            ),
                        },
                        IntcurveSupportSide {
                            surface: Some(surface.clone()),
                            pcurve: None,
                        },
                    ],
                    [0.0, 1.0],
                    [Vec::new(), Vec::new(), Vec::new()],
                )
                .unwrap(),
                discontinuity_flag: false,
            },
        )
        .unwrap(),
    );
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#synthetic:blend-boundary-edge")
            .expect("identity grammar"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(boundary_curve), Some([0.0, 1.0]))
            .unwrap(),
        start: VertexId::mint("test:model:entity#synthetic:blend-boundary-start")
            .expect("identity grammar"),
        end: VertexId::mint("test:model:entity#synthetic:blend-boundary-end")
            .expect("identity grammar"),
        tolerance: Some(
            cadmpeg_ir::units::PositiveScalar::new(EPS_TOPOLOGY_TOLERANCE)
                .expect("positive finite tolerance"),
        ),
    });
    crate::decode::pcurves::complete_intersection_pcurves_from_opposite_charts(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        ir.model.procedural_curves.last().unwrap().definition()
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { nurbs } = &context.sides()[1].pcurve.as_ref().unwrap().geometry
    else {
        unreachable!()
    };
    assert_eq!(nurbs.control_points().first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(nurbs.control_points().last(), Some(&Point2::new(1.0, 0.0)));
    assert_eq!(
        blend_boundary_parameter_from_support_spine(
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
            procedural.id
                == ProceduralCurveId::mint("test:model:entity#synthetic:spine-construction")
                    .expect("identity grammar")
        })
        .unwrap()
        .replace_definition(ProceduralCurveDefinition::Unknown {
            native_kind: None,
            record: None,
        })
        .unwrap();
    assert_eq!(
        blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );

    let carrier = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap();
    let CurveGeometry::Procedural { cache, .. } = &mut carrier.geometry else {
        panic!("procedural spine carrier");
    };
    *cache = Some(
        cadmpeg_ir::geometry::SolvedCurveGeometry::new(CurveGeometry::Nurbs(
            cadmpeg_ir::geometry::NurbsCurve::new(
                1,
                vec![0.0, 0.0, 10.0, 10.0],
                vec![
                    cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
                    cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
                ],
                None,
                false,
            )
            .unwrap(),
        ))
        .expect("solved NURBS spine"),
    );
    let coarse = coarse_blend_surface_parameters(&ir, &surface, point, 0).unwrap();
    let coarse_point = blend_surface_point(&ir, &surface, coarse.u, coarse.v).unwrap();
    assert!(
        ((coarse_point.x - point.x).powi(2)
            + (coarse_point.y - point.y).powi(2)
            + (coarse_point.z - point.z).powi(2))
        .sqrt()
            < 1.0
    );

    let refined = refine_blend_surface_parameters(
        &ir,
        &surface,
        point,
        Point2::new(expected.u + 0.5, expected.v + 0.1),
        0,
    )
    .unwrap();
    let refined_point = blend_surface_point(&ir, &surface, refined.u, refined.v).unwrap();
    let refined_error = ((refined_point.x - point.x).powi(2)
        + (refined_point.y - point.y).powi(2)
        + (refined_point.z - point.z).powi(2))
    .sqrt();
    assert!(refined_error < 1.0e-9);

    let third =
        SurfaceId::mint("test:model:entity#synthetic:third-plane").expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: third.clone(),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let outer_spine =
        CurveId::mint("test:model:entity#synthetic:outer-spine").expect("identity grammar");
    ir.model.curves.push(Curve {
        id: outer_spine.clone(),
        geometry: CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let outer =
        SurfaceId::mint("test:model:entity#synthetic:outer-blend").expect("identity grammar");
    let outer_construction =
        ProceduralSurfaceId::mint("test:model:entity#synthetic:outer-blend-construction")
            .expect("identity grammar");
    ir.model.surfaces.push(Surface {
        id: outer.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: outer_construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(
        ProceduralSurface::new(
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
        )
        .unwrap(),
    );
    let expected = Point2::new(4.0, 0.2);
    let point = blend_surface_point(&ir, &outer, expected.u, expected.v).unwrap();
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
    let actual = blend_surface_parameters(&ir, &outer, point, None).unwrap();
    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| {
            candidate.id
                == ProceduralSurfaceId::mint("test:model:entity#synthetic:outer-blend-construction")
                    .expect("identity grammar")
        })
        .unwrap();
    outer_definition
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend { supports, .. } = definition else {
                panic!("blend definition");
            };
            supports[0].as_mut().unwrap().surface = outer.clone();
        })
        .unwrap();
    assert!(blend_surface_point(&ir, &outer, expected.u, expected.v).is_none());
}
