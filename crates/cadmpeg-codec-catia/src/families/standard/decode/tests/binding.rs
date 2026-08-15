use super::*;

#[test]
fn standard_spline_rows_bind_the_unordered_prebound_side_by_opposite_rank() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Bspline,
    });
    let mut candidates = [vec![[9, 20], [9, 21]], vec![[8, 20], [8, 21]]];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates, [vec![[9, 20]], vec![[8, 21]]]);
}

#[test]
fn same_cone_generator_requires_an_apex_collinear_endpoint_pair() {
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    assert!(same_cone_generator_pair(
        &cone,
        &cone,
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 1.0),
    ));
    assert!(!same_cone_generator_pair(
        &cone,
        &cone,
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 1.0),
    ));
}

#[test]
fn standard_spline_rows_bind_the_cardinality_matched_bipartite_side() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Bspline,
    });
    let domain = vec![[2, 8], [2, 9], [2, 10], [3, 8], [3, 9], [3, 10]];
    let mut candidates = [domain.clone(), domain];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(
        candidates,
        [vec![[2, 8], [2, 9], [2, 10]], vec![[3, 8], [3, 9], [3, 10]],]
    );
}

#[test]
fn standard_spline_ranks_consume_preceding_circle_bindings() {
    let circle = |faces, radius| StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces,
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 2.0),
            radius,
        },
    };
    let spline = || StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces: [68, 69],
        geometry: StandardCurveGeometry::Bspline,
    };
    let supports = [
        circle([69, 71], 4.0),
        circle([69, 71], 4.0),
        circle([68, 70], 5.0),
        circle([68, 70], 5.0),
        spline(),
        spline(),
    ];
    let spline_domain = vec![[4, 5], [4, 75], [4, 76], [5, 75], [5, 76], [75, 76]];
    let mut candidates = [
        vec![[10, 11], [75, 76]],
        vec![[10, 11], [75, 76]],
        vec![[4, 5], [12, 13]],
        vec![[4, 5], [12, 13]],
        spline_domain.clone(),
        spline_domain,
    ];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates[4], [[4, 75]]);
    assert_eq!(candidates[5], [[5, 76]]);
}

#[test]
fn standard_spline_ranks_complete_a_prebound_partition() {
    let fixed = |faces| StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces,
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 2.0),
            radius: 4.0,
        },
    };
    let spline = || StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces: [68, 69],
        geometry: StandardCurveGeometry::Bspline,
    };
    let supports = [fixed([68, 70]), fixed([69, 71]), spline(), spline()];
    let mut candidates = [
        vec![[4, 5]],
        vec![[75, 76]],
        vec![[4, 5], [4, 75], [4, 76]],
        vec![[4, 5], [5, 75], [5, 76]],
    ];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates[2], [[4, 75]]);
    assert_eq!(candidates[3], [[5, 76]]);
}

#[test]
fn standard_circle_rows_bind_equal_domains_by_allocation_rank() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 2.0),
            radius: 4.0,
        },
    });
    let domain = vec![[2, 8], [2, 9]];
    let mut candidates = [domain.clone(), domain];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates, [vec![[2, 8]], vec![[2, 9]]]);
}

#[test]
fn standard_circle_rows_bind_partner_faces_by_allocation_rank() {
    let circle = |faces| StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces,
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 2.0),
            radius: 4.0,
        },
    };
    let supports = [circle([7, 3]), circle([2, 7])];
    let domain = vec![[5, 9], [6, 9]];
    let mut candidates = [domain.clone(), domain];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates, [vec![[6, 9]], vec![[5, 9]]]);
}

#[test]
fn completed_adjacent_branches_fix_same_incidence_allocation_rank() {
    let spline = |faces| StandardCurveSupport {
        pos: 0,
        tag: 0,
        faces,
        geometry: StandardCurveGeometry::Bspline,
    };
    let supports = [
        spline([0, 2]),
        spline([1, 3]),
        spline([0, 4]),
        spline([1, 5]),
        spline([0, 1]),
        spline([0, 1]),
    ];
    let branch_domain = vec![[10, 11], [10, 20], [10, 21], [11, 20], [11, 21], [20, 21]];
    let candidates = [
        vec![[10, 11], [10, 12], [11, 12]],
        vec![[10, 11], [10, 13], [11, 13]],
        vec![[20, 21], [20, 22], [21, 22]],
        vec![[20, 21], [20, 23], [21, 23]],
        branch_domain.clone(),
        branch_domain,
    ];
    let ranked = [[10, 11], [10, 11], [20, 21], [20, 21], [10, 20], [11, 21]].map(Some);
    let mut crossed = ranked;
    crossed[4] = Some([10, 21]);
    crossed[5] = Some([11, 20]);
    let mut partial = crossed;
    partial[0] = None;
    partial[1] = None;
    let groups = standard_curve_branch_groups(&supports, &candidates);

    assert!(standard_curve_branch_assignment_is_ranked(
        &supports,
        &candidates,
        &groups,
        &ranked,
        None,
    ));
    assert!(!standard_curve_branch_assignment_is_ranked(
        &supports,
        &candidates,
        &groups,
        &crossed,
        None,
    ));
    assert!(standard_curve_branch_assignment_is_ranked(
        &supports,
        &candidates,
        &groups,
        &partial,
        None,
    ));
}

#[test]
fn standard_spline_branch_rank_uses_complete_relation_after_mesh_frontier_pruning() {
    let fixed = |tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 1.0,
        },
    };
    let spline = |tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let supports = [fixed(0), fixed(1), spline(2), spline(3)];
    let candidates = [
        vec![[0, 1]],
        vec![[2, 3]],
        vec![[0, 2], [1, 2], [2, 4]],
        vec![[0, 3], [1, 3], [3, 4]],
    ];
    let groups = standard_curve_branch_groups(&supports, &candidates);
    let ranked = [Some([0, 1]), Some([2, 3]), Some([0, 2]), Some([1, 3])];
    let crossed = [Some([0, 1]), Some([2, 3]), Some([1, 2]), Some([0, 3])];

    assert_eq!(groups.len(), 1);
    assert!(standard_curve_branch_assignment_is_ranked(
        &supports,
        &candidates,
        &groups,
        &ranked,
        None,
    ));
    assert!(!standard_curve_branch_assignment_is_ranked(
        &supports,
        &candidates,
        &groups,
        &crossed,
        None,
    ));
}

#[test]
fn standard_spline_branch_candidates_narrow_after_fixed_frontiers() {
    let fixed = |tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 1.0,
        },
    };
    let spline = |tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let supports = [fixed(0), fixed(1), spline(2), spline(3)];
    let candidates = [
        vec![[0, 1]],
        vec![[2, 3]],
        vec![[0, 2], [1, 2], [2, 4]],
        vec![[0, 3], [1, 3], [3, 4]],
    ];
    let groups = standard_curve_branch_groups(&supports, &candidates);
    let assignment = [Some([0, 1]), Some([2, 3]), None, None];
    let narrowed = standard_curve_branch_candidates_after_partial_assignment(
        &supports,
        &candidates,
        &groups,
        &assignment,
        None,
    )
    .expect("fixed frontiers establish a valid branch relation");

    assert_eq!(narrowed[2], vec![[0, 2]]);
    assert_eq!(narrowed[3], vec![[1, 3]]);
}

#[test]
fn standard_edge_allocation_binds_two_successor_vertices() {
    let supports = [
        StandardCurveSupport {
            pos: 8,
            tag: 100,
            faces: [1, 2],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 9,
            tag: 200,
            faces: [2, 3],
            geometry: StandardCurveGeometry::Bspline,
        },
    ];

    assert_eq!(
        standard_successor_endpoint_pairs(
            &supports,
            &[99, 101, 102, 202],
            &[vec![1, 2], vec![0, 3]],
        ),
        [Some([1, 2]), None]
    );
}

#[test]
fn standard_edge_allocation_rejects_geometrically_unrelated_successors() {
    let supports = [StandardCurveSupport {
        pos: 8,
        tag: 100,
        faces: [1, 2],
        geometry: StandardCurveGeometry::Bspline,
    }];

    assert_eq!(
        standard_successor_endpoint_pairs(&supports, &[99, 101, 102], &[vec![0, 1]]),
        [None]
    );
}

#[test]
fn standard_edge_allocation_binds_one_present_successor_vertex() {
    let supports = [
        StandardCurveSupport {
            pos: 8,
            tag: 100,
            faces: [1, 2],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 9,
            tag: u32::MAX,
            faces: [2, 3],
            geometry: StandardCurveGeometry::Bspline,
        },
    ];

    assert_eq!(
        standard_successor_endpoint_points(&supports, &[99, 101, 202]),
        [Some(1), None]
    );
}

#[test]
fn lone_successor_vertex_requires_geometric_corroboration() {
    let mut options = [
        vec![[2, 4], [2, 5], [3, 5]],
        vec![[7, 8], [7, 9]],
        vec![[10, 11]],
    ];

    corroborate_successor_endpoint_points(&mut options, &[Some(5), Some(6), None]);

    assert_eq!(
        options,
        [vec![[2, 5], [3, 5]], vec![[7, 8], [7, 9]], vec![[10, 11]],]
    );
}

#[test]
fn standard_spline_rows_exclude_adjacent_fixed_boundary_relations() {
    let supports = [
        StandardCurveSupport {
            pos: 8,
            tag: 8,
            faces: [1, 3],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 1.0,
            },
        },
        StandardCurveSupport {
            pos: 9,
            tag: 9,
            faces: [3, 4],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 1.0),
                radius: 1.0,
            },
        },
        StandardCurveSupport {
            pos: 10,
            tag: 10,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        },
        StandardCurveSupport {
            pos: 11,
            tag: 11,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        },
    ];
    let complete = vec![[2, 3], [2, 8], [2, 9], [3, 8], [3, 9], [8, 9]];
    let mut candidates = [vec![[2, 3]], vec![[8, 9]], complete.clone(), complete];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates[2], [[2, 8]]);
    assert_eq!(candidates[3], [[3, 9]]);
}

#[test]
fn standard_spline_branch_rank_leaves_incomplete_relations_unresolved() {
    let supports = [10, 11].map(|tag| StandardCurveSupport {
        pos: tag as usize,
        tag,
        faces: [3, 7],
        geometry: StandardCurveGeometry::Bspline,
    });
    let domain = vec![[2, 8], [2, 9], [3, 9]];
    let mut candidates = [domain.clone(), domain.clone()];

    bind_ordered_standard_curve_branches(&supports, &mut candidates);

    assert_eq!(candidates, [domain.clone(), domain]);
}

#[test]
fn standard_circle_endpoint_domain_uses_the_explicit_curve_carrier() {
    let points = [
        Point {
            id: PointId("on".to_string()),
            position: Point3::new(3.0, 4.0, 7.0),
            source_object: None,
        },
        Point {
            id: PointId("off".to_string()),
            position: Point3::new(3.0, 4.01, 7.0),
            source_object: None,
        },
    ];
    assert_eq!(
        standard_circle_endpoint_candidates(&points, Point3::new(0.0, 0.0, 7.0), 5.0, None,),
        [0]
    );
}

#[test]
fn standard_circle_endpoint_domain_requires_both_face_carriers() {
    let points = [
        Point {
            id: PointId("incident".to_string()),
            position: Point3::new(3.0, 4.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("other-occurrence".to_string()),
            position: Point3::new(3.0, -4.0, 0.0),
            source_object: None,
        },
    ];
    let left = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 4.0, 0.0),
        normal: Vector3::new(0.0, 1.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let right = SurfaceGeometry::Unknown { record: None };
    assert_eq!(
        standard_circle_endpoint_candidates(
            &points,
            Point3::new(0.0, 0.0, 0.0),
            5.0,
            Some([(&left, None), (&right, None)]),
        ),
        [0]
    );
}

#[test]
fn standard_circle_endpoint_domain_requires_both_trimmed_face_bounds() {
    let points = [
        Point {
            id: PointId("incident".to_string()),
            position: Point3::new(3.0, 4.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("other-occurrence".to_string()),
            position: Point3::new(3.0, -4.0, 0.0),
            source_object: None,
        },
    ];
    let surface = SurfaceGeometry::Unknown { record: None };
    let bounds = crate::families::standard::records::StandardFaceBounds {
        aabb_center: [3.0, 4.0, 0.0],
        aabb_half_extents: [0.1, 0.1, 0.1],
        sphere_center: [3.0, 4.0, 0.0],
        sphere_radius: 0.2,
    };

    assert_eq!(
        standard_circle_endpoint_candidates(
            &points,
            Point3::new(0.0, 0.0, 0.0),
            5.0,
            Some([(&surface, Some(bounds)), (&surface, Some(bounds))]),
        ),
        [0]
    );
}

#[test]
fn native_endpoint_pairs_extend_geometric_candidate_domains() {
    let mut candidates = vec![vec![1], Vec::new()];
    include_native_endpoint_pairs(&mut candidates, &[Some([1, 2]), Some([3, 4])]);
    assert_eq!(candidates, [vec![1, 2], vec![3, 4]]);
}

#[test]
fn native_endpoint_evidence_rejects_directed_pair_conflicts() {
    let graph = [Some([0, 1]), None];
    let roster = [Some([0, 1]), Some([2, 3])];
    assert_eq!(
        merge_native_endpoint_evidence(Some(&graph), Some(&roster)),
        Ok(Some(vec![Some([0, 1]), Some([2, 3])]))
    );
    assert_eq!(
        merge_native_endpoint_evidence(Some(&graph), Some(&[Some([1, 0]), None])),
        Err("conflicting native endpoint evidence")
    );
}

#[test]
fn complete_vertex_roster_supersedes_partial_graph_coordinates() {
    let graph = [Some([4, 5]), None];
    let roster = [Some([0, 1]), Some([2, 3])];
    assert_eq!(
        merge_native_endpoint_evidence(Some(&graph), Some(&roster)),
        Ok(Some(roster.to_vec()))
    );
}

#[test]
fn complete_mesh_endpoint_quotient_overrides_table_local_ports() {
    let raw = Some(vec![Some([0, 1]), Some([2, 3])]);
    let mesh = Some(vec![Some([0, 1]), Some([1, 2])]);
    assert_eq!(
        combine_propagated_endpoint_pairs(raw, mesh),
        Some(vec![Some([0, 1]), Some([1, 2])])
    );
}

#[test]
fn native_identity_locus_binds_only_one_coordinate_row_within_tolerance() {
    let points = [
        Point {
            id: PointId("a".to_string()),
            position: Point3::new(1.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("b".to_string()),
            position: Point3::new(1.01, 0.0, 0.0),
            source_object: None,
        },
    ];
    let tolerances = [(2usize, 0.02)].into_iter().collect();
    let ambiguous =
        unique_native_identity_points(&[7], &[[1.0, 0.0, 0.0]], 2, &tolerances, &points);
    assert!(ambiguous.is_empty());

    let exact =
        unique_native_identity_points(&[7], &[[1.0, 0.0, 0.0]], 2, &BTreeMap::new(), &points);
    assert_eq!(exact.get(&7), Some(&0));
}

#[test]
fn reverse_angular_interval_becomes_an_increasing_nurbs_domain() {
    let range = ordered_range([0.0, -std::f64::consts::PI]);
    let arc = rational_pcurve_arc([0.0, 0.0], 2.0, range).expect("reverse semicircle");
    let PcurveGeometry::Nurbs { knots, .. } = &arc else {
        panic!("expected rational NURBS arc");
    };
    assert!(knots.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(range, [-std::f64::consts::PI, 0.0]);
    let start = pcurve_uv(&arc, range[0]).expect("start evaluation");
    let end = pcurve_uv(&arc, range[1]).expect("end evaluation");
    assert!((start.u + 2.0).abs() < 1e-12);
    assert!(start.v.abs() < 1e-12);
    assert!((end.u - 2.0).abs() < 1e-12);
    assert!(end.v.abs() < 1e-12);
}

#[test]
fn canonical_periodic_range_snaps_roundoff_at_the_turn_seam() {
    let tau = std::f64::consts::TAU;
    let range = crate::nurbs::canonical_periodic_range([tau - 1e-14, tau + 0.25])
        .expect("canonical seam range");
    assert_eq!(range[0], 0.0);
    assert!((range[1] - 0.25).abs() < 2e-14);
}

#[test]
fn coincident_planes_do_not_impose_a_line_direction() {
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    assert!(intersection_line_direction(&plane, &plane).is_none());
}

#[test]
fn plane_intersection_preserves_tiny_nonzero_direction() {
    let left = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(1.0, 0.0, 0.0),
        u_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let right = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(1.0, 1e-200, 0.0),
        u_axis: Vector3::new(0.0, 0.0, 1.0),
    };
    assert_eq!(
        intersection_line_direction(&left, &right),
        Some(Vector3::new(0.0, 0.0, 1e-200))
    );
}

#[test]
fn plane_intersection_preserves_tiny_nonzero_angle_and_finite_origin() {
    let tiny = 1e-200;
    assert_eq!(
        plane_intersection_line(
            Point3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, tiny, 0.0),
        ),
        Some((Point3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),))
    );
}

#[test]
fn cylinder_generator_direction_requires_compatible_support_axes() {
    let cylinder = |axis| SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis,
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    let containing_plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 1.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let transverse_plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let axial = cylinder(Vector3::new(0.0, 0.0, 1.0));
    let oblique = cylinder(Vector3::new(0.0, 1.0, 0.0));

    assert_eq!(
        intersection_line_direction(&containing_plane, &axial),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    assert!(intersection_line_direction(&transverse_plane, &axial).is_none());
    assert!(intersection_line_direction(&axial, &oblique).is_none());
}

#[test]
fn unsupported_surface_membership_does_not_reject_endpoint_candidates() {
    assert!(point_on_standard_face(
        Point3::new(100.0, -50.0, 7.0),
        &SurfaceGeometry::Unknown { record: None },
        None,
    ));
    let nurbs = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    });
    assert!(point_on_standard_face(
        Point3::new(100.0, -50.0, 7.0),
        &nurbs,
        None,
    ));
}

#[test]
fn freeform_face_bounds_constrain_unknown_surface_endpoints() {
    let bounds = StandardFaceBounds {
        aabb_center: [2.0, 3.0, 4.0],
        aabb_half_extents: [1.0, 2.0, 3.0],
        sphere_center: [2.0, 3.0, 4.0],
        sphere_radius: 3.5,
    };
    let surface = SurfaceGeometry::Unknown { record: None };
    assert!(point_on_standard_face(
        Point3::new(2.0, 4.0, 6.0),
        &surface,
        Some(bounds),
    ));
    assert!(!point_on_standard_face(
        Point3::new(3.01, 3.0, 4.0),
        &surface,
        Some(bounds),
    ));
    assert!(!point_on_standard_face(
        Point3::new(3.0, 5.0, 7.0),
        &surface,
        Some(bounds),
    ));
}

#[test]
fn standard_plane_line_inverts_to_exact_parameter_line() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(1.0, 2.0, 3.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    let (geometry, range) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(2.0, 4.0, 3.0),
        Point3::new(5.0, 8.0, 3.0),
        None,
        None,
    )
    .expect("plane line pcurve");
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(
        geometry,
        PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(1.0, 2.0),
            direction: cadmpeg_ir::math::Point2::new(3.0, 4.0),
        }
    );
}

#[test]
fn standard_emission_reverses_only_face_pcurve_use_range() {
    for reversed in [false, true] {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.extend([
            Point {
                id: PointId("point-0".into()),
                position: Point3::new(0.0, 0.0, 0.0),
                source_object: None,
            },
            Point {
                id: PointId("point-1".into()),
                position: Point3::new(1.0, 0.0, 0.0),
                source_object: None,
            },
        ]);
        ir.model.surfaces.push(Surface {
            id: SurfaceId("surface-0".into()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        ir.model.faces.push(Face {
            id: FaceId("catia:standard:face#0".into()),
            shell: ShellId("shell-0".into()),
            surface: SurfaceId("surface-0".into()),
            sense: Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        });
        let bindings = [(SurfaceId("surface-0".into()), false, 0)];
        let surface_indices = HashMap::from([(bindings[0].0.clone(), 0)]);
        let supports = [StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        }];
        let topology = crate::families::standard::topology::StandardTopology {
            faces: vec![crate::families::standard::topology::FaceTopology {
                boundaries: vec![crate::families::standard::topology::Boundary {
                    coedges: vec![crate::families::standard::topology::CoedgeUse {
                        edge_row: 0,
                        reversed,
                        start_vertex: 0,
                        end_vertex: 1,
                    }],
                }],
            }],
            edge_rows: vec![crate::families::standard::topology::EdgeRow {
                kind: 1,
                handles: vec![0, 1],
                boundary_layout:
                    crate::families::standard::topology::EdgeBoundaryLayout::CompleteBoundaryRun,
            }],
            vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_count: 2,
        };
        let mut annotations = AnnotationBuilder::new();
        emit_standard_topology(
            &mut ir,
            &mut annotations,
            &bindings,
            &[],
            &surface_indices,
            &supports,
            &[[0, 1]],
            &[0, 1],
            &topology,
            &[None],
            &[None],
            &[],
        );

        let [loop_] = ir.model.loops.as_slice() else {
            panic!("standard edge emission must create one loop");
        };
        let [vertex_use] = loop_.vertex_uses.as_slice() else {
            panic!("standard edge emission must retain one vertex use");
        };
        assert_eq!(
            vertex_use.vertex,
            VertexId("catia:standard:v#1".to_string())
        );
        assert_eq!(
            vertex_use.after,
            Some(cadmpeg_ir::ids::CoedgeId(
                "catia:standard:coedge#0:0:0".to_string()
            ))
        );

        let [pcurve] = ir.model.coedges[0].pcurves.as_slice() else {
            panic!("standard line occurrence must retain its pcurve");
        };
        assert_eq!(pcurve.parameter_range, reversed.then_some([1.0, 0.0]));
    }
}

#[test]
fn standard_plane_circle_pcurve_preserves_contained_carrier() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let center = Point3::new(0.0, 0.0, 0.0);
    let radius = 2.0;
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle { center, radius },
    };
    let carrier = CurveGeometry::Circle {
        center,
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    let start = Point3::new(radius, 0.0, 0.0);
    let end = Point3::new(0.0, radius, 0.0);
    let (geometry, range) =
        standard_pcurve_geometry(&surface, &support, start, end, None, Some(&carrier))
            .expect("contained plane circle pcurve");
    let mapped = range.map(|parameter| {
        let uv = pcurve_uv(&geometry, parameter).expect("plane circle pcurve endpoint");
        surface_point(&surface, uv.u, uv.v).expect("plane circle surface endpoint")
    });
    assert!(mapped[0].distance(start) <= 1e-9);
    assert!(mapped[1].distance(end) <= 1e-9);
}

#[test]
fn standard_plane_full_circle_pcurve_preserves_closed_carrier() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let center = Point3::new(0.0, 0.0, 0.0);
    let radius = 2.0;
    let start = Point3::new(radius, 0.0, 0.0);
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle { center, radius },
    };
    let carrier = CurveGeometry::Circle {
        center,
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    let (geometry, range) =
        standard_pcurve_geometry(&surface, &support, start, start, None, Some(&carrier))
            .expect("closed contained plane circle pcurve");
    assert_eq!(range, [0.0, std::f64::consts::TAU]);
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        ..
    } = &geometry
    else {
        panic!("closed plane circle must use a rational arc");
    };
    assert_eq!(*degree, 2);
    assert_eq!(knots.len(), 12);
    assert_eq!(control_points.len(), 9);
    assert_eq!(weights.as_ref().map(Vec::len), Some(9));
    for parameter in [range[0], range[1]] {
        let uv = pcurve_uv(&geometry, parameter).expect("closed pcurve endpoint");
        let point = surface_point(&surface, uv.u, uv.v).expect("closed surface endpoint");
        assert!(point.distance(start) <= 1e-9);
    }
    let midpoint_uv = pcurve_uv(&geometry, std::f64::consts::PI).expect("closed pcurve midpoint");
    let midpoint =
        surface_point(&surface, midpoint_uv.u, midpoint_uv.v).expect("closed surface midpoint");
    assert!(midpoint.distance(Point3::new(-radius, 0.0, 0.0)) <= 1e-9);
}

#[test]
fn standard_full_circle_edge_uses_vertex_seam_and_radian_domain() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("point-0".into()),
        position: Point3::new(2.0, 0.0, 0.0),
        source_object: None,
    });
    let surface_id = SurfaceId("surface-0".into());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 0],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 2.0,
        },
    };
    let (curve, range) = build_standard_edge_curve(
        &mut ir,
        &mut AnnotationBuilder::new(),
        &[(surface_id.clone(), false, 0)],
        &HashMap::from([(surface_id, 0)]),
        &[],
        &support,
        [0, 0],
        None,
        None,
    );
    assert_eq!(range, Some([0.0, std::f64::consts::TAU]));
    let curve = curve.expect("closed circle support identifies a curve");
    assert!(matches!(
        ir.model.curves.iter().find(|candidate| candidate.id == curve),
        Some(Curve {
            geometry: CurveGeometry::Circle {
                axis,
                ref_direction,
                radius,
                ..
            },
            ..
        }) if *axis == Vector3::new(0.0, 0.0, 1.0)
            && *ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && *radius == 2.0
    ));
}

#[test]
fn standard_plane_circle_pcurve_rejects_carrier_outside_face_plane() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let center = Point3::new(0.0, 0.0, 1.0);
    let radius = 2.0_f64.sqrt();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle { center, radius },
    };
    let carrier = CurveGeometry::Circle {
        center,
        axis: Vector3::new(1.0, 0.0, 0.0),
        ref_direction: Vector3::new(0.0, 1.0, 0.0),
        radius,
    };
    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, -1.0, 0.0),
        None,
        Some(&carrier),
    )
    .is_none());
}

#[test]
fn standard_plane_circle_pcurve_rejects_tilted_carrier() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let center = Point3::new(0.0, 0.0, 0.0);
    let radius = 2.0;
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle { center, radius },
    };
    let carrier = CurveGeometry::Circle {
        center,
        axis: Vector3::new(1.0, 0.0, 0.0),
        ref_direction: Vector3::new(0.0, 1.0, 0.0),
        radius,
    };
    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(0.0, radius, 0.0),
        Point3::new(0.0, -radius, 0.0),
        None,
        Some(&carrier),
    )
    .is_none());
}

#[test]
fn solved_planar_spline_line_inverts_to_exact_parameter_line() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(1.0, 2.0, 3.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Bspline,
    };
    let start = Point3::new(2.0, 4.0, 3.0);
    let end = Point3::new(5.0, 8.0, 3.0);
    let carrier = CurveGeometry::Line {
        origin: start,
        direction: Vector3::new(3.0, 4.0, 0.0),
    };
    let (geometry, range) =
        standard_pcurve_geometry(&surface, &support, start, end, None, Some(&carrier))
            .expect("solved spline line pcurve");

    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(
        geometry,
        PcurveGeometry::Line {
            origin: Point2::new(1.0, 2.0),
            direction: Point2::new(3.0, 4.0),
        }
    );
}

#[test]
fn standard_pcurve_rejects_endpoints_outside_the_face_carrier() {
    let surface = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 1.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    };
    assert!(standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.0),
        None,
        None,
    )
    .is_none());
}

#[test]
fn standard_cone_apex_uses_the_other_endpoint_angular_gauge() {
    for half_angle in [0.25f64, 1e-200] {
        let surface = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.0,
            ratio: 1.0,
            half_angle,
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        };
        let height = 4.0;
        let radius = height * half_angle.tan();
        let (geometry, range) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, radius, height),
            None,
            None,
        )
        .expect("cone generator through the apex");
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
                direction: cadmpeg_ir::math::Point2::new(0.0, height),
            }
        );
    }
}

#[test]
fn standard_cone_latitude_inverts_to_isoparametric_line() {
    let half_angle = 0.25f64;
    let radius = 3.0 + 2.0 * half_angle.tan();
    let surface = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
        ratio: 1.0,
        half_angle,
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 2.0),
            radius,
        },
    };
    let (geometry, range) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(radius, 0.0, 2.0),
        Point3::new(0.0, radius, 2.0),
        None,
        None,
    )
    .expect("cone latitude pcurve");
    assert_eq!(range, [0.0, 1.0]);
    assert_eq!(
        geometry,
        PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 2.0),
            direction: cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
        }
    );
}

#[test]
fn standard_cylinder_witness_selects_complementary_arc() {
    let surface = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 3.0),
            radius: 2.0,
        },
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(2.0, 0.0, 3.0),
        Point3::new(0.0, 2.0, 3.0),
        Some(Point3::new(-2.0, 0.0, 3.0)),
        None,
    )
    .expect("witnessed cylinder section");
    assert_eq!(
        geometry,
        PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 3.0),
            direction: cadmpeg_ir::math::Point2::new(-3.0 * std::f64::consts::FRAC_PI_2, 0.0,),
        }
    );
}

#[test]
fn standard_cylinder_endpoint_witness_preserves_geometric_arc() {
    let surface = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 3.0),
            radius: 2.0,
        },
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(-2.0, 0.0, 3.0),
        Point3::new(0.0, -2.0, 3.0),
        Some(Point3::new(-1.0, 0.0, 4.0)),
        None,
    )
    .expect("endpoint-aligned witness does not reject the arc");
    assert_eq!(
        geometry,
        PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(std::f64::consts::PI, 3.0),
            direction: cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
        }
    );
}

#[test]
fn standard_torus_witness_selects_complementary_latitude_arc() {
    let surface = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 5.0,
        minor_radius: 2.0,
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 7.0,
        },
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(7.0, 0.0, 0.0),
        Point3::new(0.0, 7.0, 0.0),
        Some(Point3::new(-7.0, 0.0, 0.0)),
        None,
    )
    .expect("witnessed torus latitude");
    let PcurveGeometry::Line { origin, direction } = geometry else {
        panic!("expected torus chart line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(
        direction,
        cadmpeg_ir::math::Point2::new(-3.0 * std::f64::consts::FRAC_PI_2, 0.0)
    );
    let range = circle_parameter_range_from_surface_branch(
        &surface,
        Point3::new(0.0, 0.0, 0.0),
        7.0,
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        Point3::new(7.0, 0.0, 0.0),
        Point3::new(0.0, 7.0, 0.0),
        origin,
        direction,
    )
    .expect("torus circle range");
    assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn arc_witness_selects_tiny_nonzero_sweep() {
    let sweep = 1e-200;
    assert_eq!(witness_arc_end(0.0, sweep, sweep * 0.5), Some(sweep));
}

#[test]
fn standard_sphere_latitude_inverts_to_isoparametric_line() {
    let latitude = 0.4f64;
    let radius = 5.0;
    let ring = radius * latitude.cos();
    let height = radius * latitude.sin();
    let surface = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, height),
            radius: ring,
        },
    };
    let (geometry, _) = standard_pcurve_geometry(
        &surface,
        &support,
        Point3::new(ring, 0.0, height),
        Point3::new(0.0, ring, height),
        None,
        None,
    )
    .expect("sphere latitude pcurve");
    let PcurveGeometry::Line { origin, direction } = geometry else {
        panic!("expected line pcurve");
    };
    assert!(origin.u.abs() < 1e-12);
    assert!((origin.v - latitude).abs() < 1e-12);
    assert!((direction.u - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!(direction.v.abs() < 1e-12);
}
