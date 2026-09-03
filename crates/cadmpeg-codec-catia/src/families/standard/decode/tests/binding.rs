use super::*;
use std::collections::BTreeMap;

fn unit_square_surface() -> NurbsSurface {
    NurbsSurface {
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
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    }
}

fn owner_tail(lower: [f64; 2], upper: [f64; 2], bounds: [[f32; 2]; 3]) -> B2OwnerNumericTail {
    B2OwnerNumericTail {
        header: [0x84, 0x41, 0, 0, 0x0d],
        lower,
        upper,
        bounds,
    }
}

#[test]
fn owner_carrier_candidate_requires_parameter_and_model_space_containment() {
    let surface = unit_square_surface();
    let admitted = owner_tail(
        [0.25, 0.25],
        [0.75, 0.75],
        [[0.2, 0.8], [0.2, 0.8], [-0.1, 0.1]],
    );
    let outside_parameter_domain = owner_tail(
        [-0.25, 0.25],
        [0.75, 0.75],
        [[-0.3, 0.8], [0.2, 0.8], [-0.1, 0.1]],
    );
    let clipped_model_bounds = owner_tail(
        [0.25, 0.25],
        [0.75, 0.75],
        [[0.3, 0.8], [0.2, 0.8], [-0.1, 0.1]],
    );

    assert!(owner_matches_a5_carrier(&admitted, &surface));
    assert!(!owner_matches_a5_carrier(
        &outside_parameter_domain,
        &surface
    ));
    assert!(!owner_matches_a5_carrier(&clipped_model_bounds, &surface));
}

#[test]
fn owner_face_candidate_requires_complete_trimmed_bounds_containment() {
    let owner = owner_tail(
        [0.0, 0.0],
        [1.0, 1.0],
        [[-1.0, 1.0], [-2.0, 2.0], [-3.0, 3.0]],
    );
    let contained = StandardFaceBounds {
        aabb_center: [0.0, 0.0, 0.0],
        aabb_half_extents: [0.5, 1.5, 2.5],
        sphere_center: [0.0, 0.0, 0.0],
        sphere_radius: 3.0,
    };
    let protruding = StandardFaceBounds {
        aabb_half_extents: [1.5, 1.5, 2.5],
        ..contained
    };

    assert!(owner_contains_face_bounds(
        B2OwnerReferenceEncoding::AllCompact,
        &owner,
        contained,
    ));
    assert!(!owner_contains_face_bounds(
        B2OwnerReferenceEncoding::AllCompact,
        &owner,
        protruding,
    ));
}

#[test]
fn owner_face_bounds_are_not_a_witness_for_other_fixed_nine_dialects() {
    let owner = owner_tail(
        [0.0, 0.0],
        [1.0, 1.0],
        [[-1.0, 1.0], [-2.0, 2.0], [-3.0, 3.0]],
    );
    let face = StandardFaceBounds {
        aabb_center: [0.0, 0.0, 0.0],
        aabb_half_extents: [0.5, 1.5, 2.5],
        sphere_center: [0.0, 0.0, 0.0],
        sphere_radius: 3.0,
    };

    for encoding in [
        B2OwnerReferenceEncoding::TaggedU16Strong,
        B2OwnerReferenceEncoding::WidthCodedStrong,
    ] {
        assert!(!owner_contains_face_bounds(encoding, &owner, face));
    }
}

#[test]
fn owner_face_swaps_bind_when_every_complete_matching_has_one_carrier() {
    let domains = vec![
        vec![(0, vec![7]), (1, vec![7]), (2, vec![7])],
        vec![(0, vec![7]), (1, vec![7])],
    ];

    assert_eq!(
        invariant_face_carrier_bindings(&domains, 3, None),
        Some(vec![Some(7), Some(7)])
    );
}

#[test]
fn owner_face_matching_withholds_carrier_labels_that_change_under_a_swap() {
    let domains = vec![
        vec![(0, vec![7]), (1, vec![9])],
        vec![(0, vec![9]), (1, vec![7])],
    ];

    assert_eq!(
        invariant_face_carrier_bindings(&domains, 2, None),
        Some(vec![None, None])
    );
}

#[test]
fn owner_face_matching_removes_labels_outside_every_complete_matching() {
    let domains = vec![vec![(0, vec![7]), (1, vec![99])], vec![(1, vec![11])]];

    assert_eq!(
        invariant_face_carrier_bindings(&domains, 2, None),
        Some(vec![Some(7), Some(11)])
    );
}

#[test]
fn owner_face_matching_requires_every_face_to_have_a_distinct_owner() {
    let domains = vec![vec![(0, vec![7])], Vec::new()];

    assert_eq!(invariant_face_carrier_bindings(&domains, 2, None), None);
}

#[test]
fn standard_object_journal_binds_ordered_edge_endpoints_through_roster_position() {
    let supports = [70, 90, 110].map(|tag| StandardCurveSupport {
        pos: usize::try_from(tag).expect("fixture tag"),
        tag,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Line,
    });
    let native_edges = BTreeMap::from([(70, [500, 300]), (90, [100, 500])]);
    let roster = [100, 300, 500];

    assert_eq!(
        standard_serialized_endpoint_pairs(&supports, &native_edges, &roster),
        Some(vec![Some([2, 1]), Some([0, 2]), None])
    );

    assert!(
        standard_serialized_endpoint_pairs(&supports, &native_edges, &[100, 300, 100]).is_none()
    );
}

#[test]
fn standard_object_journal_merges_matching_edge_dialects_and_rejects_conflicts() {
    let mut edges = BTreeMap::from([(70, [500, 300])]);
    assert!(merge_standard_edge_vertex_references(
        &mut edges,
        [(70, [500, 300]), (90, [100, 500])],
    ));
    assert_eq!(edges, BTreeMap::from([(70, [500, 300]), (90, [100, 500])]));
    assert!(!merge_standard_edge_vertex_references(
        &mut edges,
        [(70, [300, 500])],
    ));
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
fn standard_edge_successor_points_are_only_domain_corroboration() {
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
        standard_successor_endpoint_points(&supports, &[99, 101, 102]),
        [[Some(1), Some(2)], [None, None]]
    );
}

#[test]
fn successor_endpoint_points_filter_independently_and_jointly() {
    let mut options = [
        vec![[2, 4], [2, 5], [3, 5]],
        vec![[7, 8], [7, 9]],
        vec![[10, 11]],
    ];

    corroborate_successor_endpoint_points(
        &mut options,
        &[[None, Some(5)], [Some(6), None], [None, None]],
    );

    assert_eq!(
        options,
        [vec![[2, 5], [3, 5]], vec![[7, 8], [7, 9]], vec![[10, 11]],]
    );

    let mut joint_options = vec![vec![[2, 4], [2, 5], [3, 5]]];
    corroborate_successor_endpoint_points(&mut joint_options, &[[Some(2), Some(5)]]);
    assert_eq!(joint_options, [vec![[2, 5]]]);
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
fn derived_endpoint_sources_corroborate_reversed_native_direction() {
    let mut pairs = vec![Some([34, 33])];
    assert!(merge_derived_endpoint_pair(&mut pairs, 0, [33, 34]));
    assert_eq!(pairs, [Some([34, 33])]);
    assert!(!merge_derived_endpoint_pair(&mut pairs, 0, [33, 35]));
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
    assert!((start.u + 2.0).abs() < 1.0e-12);
    assert!(start.v.abs() < 1.0e-12);
    assert!((end.u - 2.0).abs() < 1.0e-12);
    assert!(end.v.abs() < 1.0e-12);
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
fn unknown_surface_membership_stays_open_but_nurbs_membership_is_geometric() {
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
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    });
    assert!(point_on_standard_face(
        Point3::new(0.5, 0.5, 0.0),
        &nurbs,
        None,
    ));
    assert!(!point_on_standard_face(
        Point3::new(0.5, 0.5, 0.1),
        &nurbs,
        None,
    ));
    let mut unresolved = nurbs.clone();
    if let SurfaceGeometry::Nurbs(surface) = &mut unresolved {
        surface.weights = Some(vec![1.0]);
    }
    assert!(point_on_standard_face(
        Point3::new(0.5, 0.5, 0.1),
        &unresolved,
        None,
    ));
    assert!(!point_on_standard_face(
        Point3::new(100.0, -50.0, 7.0),
        &nurbs,
        None,
    ));
}

#[test]
fn standard_freeform_face_uses_exact_e5_surface_wrapper_identity() {
    let mut stream = e5_torus_stream();
    let mut wrapper = vec![0x85, 0x80, 0x81, 0x82, 0x83, 0x84];
    wrapper.extend_from_slice(&[0; 38]);
    append_e5_record(&mut stream, 0xf1, 8, &wrapper);
    append_e5_record(&mut stream, 0x00, 7, &[0x82, 0x88, 0x89, 1, 0]);

    let records = [StandardSurfaceRecord::Freeform {
        pos: 0,
        tag: 7,
        bounds: StandardFaceBounds {
            aabb_center: [0.0, 0.0, 0.0],
            aabb_half_extents: [1.0, 1.0, 1.0],
            sphere_center: [0.0, 0.0, 0.0],
            sphere_radius: 1.0,
        },
        forward: true,
    }];

    let associated = associate_standard_freeform_e5_surfaces(&records, &stream);
    assert!(matches!(
        associated.get(&7),
        Some(SurfaceGeometry::Torus { .. })
    ));
}

#[test]
fn standard_freeform_face_uses_exact_e5_d8_rolling_ball_identity() {
    let mut stream = e5_d8_rolling_ball_stream();
    let mut wrapper = vec![0x85, 0xaa, 0x81, 0x82, 0x83, 0x84];
    wrapper.extend_from_slice(&[0; 38]);
    append_e5_record(&mut stream, 0xf1, 8, &wrapper);
    append_e5_record(&mut stream, 0x00, 7, &[0x82, 0x88, 0x89, 1, 0]);

    let records = [StandardSurfaceRecord::Freeform {
        pos: 0,
        tag: 7,
        bounds: StandardFaceBounds {
            aabb_center: [0.0, 0.0, 0.0],
            aabb_half_extents: [1.0, 1.0, 1.0],
            sphere_center: [0.0, 0.0, 0.0],
            sphere_radius: 1.0,
        },
        forward: true,
    }];

    let associated = associate_standard_freeform_e5_rolling_ball_jets(&records, &stream);
    assert!(matches!(
        associated.get(&7),
        Some(StandardSurfaceProcedure::RollingBall {
            carrier_object_id: 42,
            source: StandardRollingBallSource::E5D8,
            definition: ProceduralSurfaceDefinition::RollingBallJet {
                degree: 5,
                knots,
                multiplicities,
                sites,
            },
    }) if knots == &vec![2.0, 5.0]
            && multiplicities == &vec![6, 6]
            && sites.len() == 2
    ));

    let mut opposite_records = records.clone();
    let StandardSurfaceRecord::Freeform { forward, .. } = &mut opposite_records[0] else {
        unreachable!("synthetic D8 face record");
    };
    *forward = false;
    assert!(
        associate_standard_freeform_e5_rolling_ball_jets(&opposite_records, &stream).is_empty()
    );

    let mut reverse_stream = stream.clone();
    let d8_payload_size = usize::from(u16::from_le_bytes(
        reverse_stream[5..7].try_into().expect("D8 payload size"),
    ));
    let sense_offset = 13 + d8_payload_size - 63 + 5 * std::mem::size_of::<f64>();
    let encoded_sense_offset = reverse_stream
        .windows(std::mem::size_of::<i32>())
        .position(|bytes| bytes == (-1_i32).to_le_bytes());
    assert_eq!(Some(sense_offset), encoded_sense_offset);
    reverse_stream[sense_offset..sense_offset + std::mem::size_of::<i32>()]
        .copy_from_slice(&1_i32.to_le_bytes());
    assert_eq!(
        crate::families::e5::records::e5_rolling_ball_jets(&reverse_stream)[0].sense,
        1
    );
    assert!(
        associate_standard_freeform_e5_rolling_ball_jets(&opposite_records, &reverse_stream,)
            .contains_key(&7)
    );
}

#[test]
fn cached_face_point_membership_matches_the_source_predicate() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.extend([
        Point {
            id: PointId("point-0".into()),
            position: Point3::new(1.0, 2.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("point-1".into()),
            position: Point3::new(1.0, 2.0, 1.0),
            source_object: None,
        },
    ]);
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
    let bindings = [(surface_id.clone(), false, 0)];
    let surface_indices = HashMap::from([(surface_id, 0)]);
    let membership = standard_face_point_membership(&ir, &bindings, &surface_indices, None)
        .expect("complete face membership");

    assert!(membership[0][0]);
    assert!(!membership[0][1]);
    assert!(membership[0].iter().enumerate().all(|(point, cached)| {
        *cached
            == point_on_standard_face(
                ir.model.points[point].position,
                &ir.model.surfaces[0].geometry,
                None,
            )
    }));
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
        let [vertex_use] = loop_.anchored_vertex_uses() else {
            panic!("standard edge emission must retain one vertex use");
        };
        assert_eq!(
            vertex_use.vertex,
            VertexId("catia:standard:v#1".to_string())
        );
        assert_eq!(
            vertex_use.after,
            cadmpeg_ir::ids::CoedgeId("catia:standard:coedge#0:0:0".to_string())
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
    assert!(mapped[0].distance(start) <= 1.0e-9);
    assert!(mapped[1].distance(end) <= 1.0e-9);
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
        assert!(point.distance(start) <= 1.0e-9);
    }
    let midpoint_uv = pcurve_uv(&geometry, std::f64::consts::PI).expect("closed pcurve midpoint");
    let midpoint =
        surface_point(&surface, midpoint_uv.u, midpoint_uv.v).expect("closed surface midpoint");
    assert!(midpoint.distance(Point3::new(-radius, 0.0, 0.0)) <= 1.0e-9);
}

#[test]
fn spherical_section_endpoint_pair_survives_topology_admission_without_pcurve() {
    let surface = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 5.0,
    };
    let section_radius = 21.0_f64.sqrt();
    let support = StandardCurveSupport {
        pos: 0,
        tag: 1,
        faces: [0, 1],
        geometry: StandardCurveGeometry::Circle {
            center: Point3::new(0.0, 2.0, 0.0),
            radius: section_radius,
        },
    };
    let start = Point3::new(section_radius, 2.0, 0.0);
    let end = Point3::new(0.0, 2.0, section_radius);

    assert!(standard_pcurve_geometry(&surface, &support, start, end, None, None).is_none());
    assert!(standard_endpoint_pair_supports_topology(
        &surface, &support, start, end, None
    ));
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
    assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
}

#[test]
fn standard_torus_witness_selects_complementary_meridian_arc() {
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
            center: Point3::new(5.0, 0.0, 0.0),
            radius: 2.0,
        },
    };
    let start = Point3::new(7.0, 0.0, 0.0);
    let end = Point3::new(5.0, 0.0, 2.0);
    let witness = Point3::new(5.0, 0.0, -2.0);
    let (geometry, _) =
        standard_pcurve_geometry(&surface, &support, start, end, Some(witness), None)
            .expect("witnessed torus meridian");
    let PcurveGeometry::Line { origin, direction } = geometry else {
        panic!("expected torus meridian chart line");
    };
    let long_sweep = std::f64::consts::FRAC_PI_2 - std::f64::consts::TAU;
    assert_eq!(origin, cadmpeg_ir::math::Point2::new(0.0, 0.0));
    assert_eq!(direction, cadmpeg_ir::math::Point2::new(0.0, long_sweep));

    let range = circle_parameter_range_from_surface_branch(
        &surface,
        Point3::new(5.0, 0.0, 0.0),
        2.0,
        Vector3::new(0.0, -1.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        start,
        end,
        origin,
        direction,
    )
    .expect("torus meridian circle range");
    assert_eq!(range, [0.0, long_sweep]);
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
    assert!(origin.u.abs() < 1.0e-12);
    assert!((origin.v - latitude).abs() < 1.0e-12);
    assert!((direction.u - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!(direction.v.abs() < 1.0e-12);
}

#[test]
fn generated_analytic_curve_ranges_use_angular_parameters() {
    const ANGLE_TOLERANCE: f64 = 1e-12;

    let geometry = CurveGeometry::Ellipse {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 4.0,
        minor_radius: 2.0,
    };
    let start = curve_point(&geometry, 0.0).expect("ellipse start");
    let end = curve_point(&geometry, std::f64::consts::FRAC_PI_2).expect("ellipse end");
    let witness = curve_point(&geometry, 0.75 * std::f64::consts::PI).expect("ellipse witness");
    let short = standard_analytic_curve_parameter_range(&geometry, start, end, None)
        .expect("short angular range");
    let mut oriented = geometry.clone();
    let long = standard_oriented_analytic_curve_parameter_range(&mut oriented, start, end, witness)
        .expect("witnessed angular range");
    assert!((short[0] - 0.0).abs() < ANGLE_TOLERANCE);
    assert!((short[1] - std::f64::consts::FRAC_PI_2).abs() < ANGLE_TOLERANCE);
    assert!((long[1] - 1.5 * std::f64::consts::PI).abs() < ANGLE_TOLERANCE);
}
