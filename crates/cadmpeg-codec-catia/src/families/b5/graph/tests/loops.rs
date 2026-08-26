use super::super::*;
use super::*;

#[test]
fn unit_preserves_tiny_finite_direction() {
    assert_eq!(unit([1e-200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
    assert_eq!(unit([0.0, 0.0, 0.0]), None);
}

#[test]
fn loop_metadata_accepts_exact_base_and_extended_forms() {
    let base = [
        0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff,
        0x01,
    ];
    assert_eq!(
        loop_metadata(&base, 2),
        Some(B5LoopMetadata {
            framing_controls: [0x05, 0x05],
            edge_controls: vec![[1, -1, 1], [-1, 1, -1]],
            extension: None,
        })
    );

    for metadata_control in [0x05, 0x09, 0x21, 0x41, 0x71] {
        let extended = extended_loop_metadata(metadata_control);
        let metadata = loop_metadata(&extended, 1).expect("complete extended metadata");
        assert_eq!(metadata.framing_controls, [0x03, 0x05]);
        assert_eq!(metadata.edge_controls, [[1, -1, 1]]);
        assert_eq!(
            metadata.extension,
            Some(B5LoopMetadataExtension {
                scalars: [1.0, -2.0, 3.5, 4.25],
                control: metadata_control,
                floats: [1.0, -2.0, 3.5, 4.25, 5.5, -6.75],
            })
        );
    }

    let alternate_framing_control = [
        0x05, 0x03, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00, 0xff, 0xff,
        0x01,
    ];
    let metadata = loop_metadata(&alternate_framing_control, 2).expect("alternate framing control");
    assert_eq!(metadata.framing_controls, [0x05, 0x03]);
    assert_eq!(metadata.edge_controls, [[1, -1, 1], [-1, 1, -1]]);
    assert_eq!(metadata.extension, None);
}

#[test]
fn loop_references_require_exact_matching_edge_count_and_metadata() {
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x62,
        object_id: 400,
        payload: vec![
            0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
            0x01,
        ],
    };
    let (references, metadata) = loop_references_and_metadata(&record).expect("exact loop payload");
    assert_eq!(references, [9, 10, 11]);
    assert_eq!(metadata.edge_controls, [[1, -1, 1]]);

    let mut mismatched = record.clone();
    mismatched.payload[4] = 0x82;
    assert!(loop_references(&mismatched).is_none());

    let mut residual = record;
    residual.payload.push(0);
    assert!(loop_references(&residual).is_none());
}

#[test]
fn loop_metadata_rejects_every_malformed_boundary_and_numeric_domain() {
    assert!(loop_metadata(&[], 0).is_none());
    assert!(loop_metadata(&[0x05, 0x05, 0x03], 0).is_none());
    assert!(loop_metadata(&[0x05, 0x05, 0x03, 0x01, 0x00, 0x01], 0).is_none());
    assert!(loop_metadata(&[0x09, 0x05, 0x03, 0x01], 0).is_none());
    assert!(loop_metadata(&[0x05, 0x03, 0x05, 0x01], 0).is_none());
    assert!(loop_metadata(
        &[0x05, 0x05, 0x03, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x01],
        1
    )
    .is_none());

    for metadata_control in [0x00, 0x04, 0x20, 0x70] {
        assert!(loop_metadata(&extended_loop_metadata(metadata_control), 1).is_none());
    }

    let mut non_finite_scalar = extended_loop_metadata(0x05);
    non_finite_scalar[10..18].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(loop_metadata(&non_finite_scalar, 1).is_none());

    let mut non_finite_float = extended_loop_metadata(0x05);
    non_finite_float[47..51].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert!(loop_metadata(&non_finite_float, 1).is_none());
}

#[test]
fn pcurve_candidate_merge_collapses_repeats_and_permanently_rejects_conflicts() {
    let mut pcurves = BTreeMap::new();
    let mut conflicts = HashSet::new();
    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
    assert_eq!(pcurves.get(&1), Some(&test_pcurve(1, 10)));

    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 11));
    merge_pcurve_candidate(&mut pcurves, &mut conflicts, test_pcurve(1, 10));
    assert!(!pcurves.contains_key(&1));
    assert!(conflicts.contains(&1));
}

#[test]
fn loop_rejects_a_pcurve_bound_to_another_surface() {
    let loop_ = B5Loop {
        object_id: 1,
        pcurves: vec![2],
        edges: vec![3],
        metadata: test_loop_metadata(1),
        surface: 10,
    };
    let edge = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x5e,
        object_id: 3,
        payload: Vec::new(),
    };
    let records = HashMap::from([(3, &edge)]);
    let pcurves = BTreeMap::from([(2, test_pcurve(2, 11))]);
    let surfaces = BTreeMap::from([(
        10,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x27,
            payload: Vec::new(),
        },
    )]);

    assert!(parse_loop(
        &loop_,
        &records,
        &pcurves,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &surfaces,
    )
    .is_none());
}

#[test]
fn pcurve_requires_one_complete_clamped_bezier_frame() {
    let payload = crate::test_support::b5_linear_pcurve_payload(1, [0.0, 0.0], [1.0, 0.0]);
    let record = |payload| B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x21,
        object_id: 2,
        payload,
    };
    assert_eq!(
        parse_pcurve(&record(payload.clone()))
            .expect("complete class-21 pcurve")
            .class_21_suffix_scalar,
        Some(1.0)
    );
    let tail = payload.len() - 36;
    let mut alternate_scalar = payload.clone();
    alternate_scalar[tail + 10..tail + 18].copy_from_slice(&2.5_f64.to_le_bytes());
    assert!(parse_pcurve(&record(alternate_scalar.clone())).is_none());
    let base = parse_pcurve(&record(payload.clone())).expect("base pcurve");
    for parameter in [0.0, 0.25, 0.5, 1.0] {
        assert_eq!(
            evaluate_pcurve(&base, parameter),
            Some([parameter, 0.0]),
            "zero-origin class-21 pcurve evaluates at its local station {parameter}"
        );
    }
    let mut wrong_family = record(payload.clone());
    wrong_family.family = 0xa8;
    assert!(parse_pcurve(&wrong_family).is_none());

    for (offset, value) in [(6, 0), (7, 0), (8, 0x0d), (9, 0x08), (26, 0x0d)] {
        let mut malformed = payload.clone();
        malformed[offset] = value;
        assert!(parse_pcurve(&record(malformed)).is_none());
    }

    let mut truncated = payload.clone();
    truncated.pop();
    assert!(parse_pcurve(&record(truncated)).is_none());

    let mut residual = payload.clone();
    residual.push(0);
    assert!(parse_pcurve(&record(residual)).is_none());

    for (offset, value) in [
        (tail + 2, 1.0_f64),
        (tail + 10, 0.0),
        (tail + 18, 0.0),
        (tail + 26, 1.0),
    ] {
        let mut malformed = payload.clone();
        malformed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(parse_pcurve(&record(malformed)).is_none());
    }

    let mut non_finite = payload;
    non_finite[tail + 10..tail + 18].copy_from_slice(&f64::INFINITY.to_le_bytes());
    assert!(parse_pcurve(&record(non_finite)).is_none());
}

#[test]
fn class21_pcurve_rebases_nonzero_origin_to_zero_based_stations() {
    let payload = crate::test_support::b5_linear_pcurve_payload_with_knots(
        7,
        [10.0, 20.0],
        [0.0, 0.0],
        [1.0, 0.0],
    );
    let pcurve = parse_pcurve(&B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x21,
        object_id: 2,
        payload,
    })
    .expect("translated class-21 pcurve");

    assert_eq!(
        pcurve.parameterization,
        B5PcurveParameterization::Translated {
            native_origin: 10.0,
        }
    );
    assert_eq!(pcurve.distinct_knots, [10.0, 20.0]);
    assert_eq!(pcurve.class_21_suffix_scalar, Some(10.0));
    assert_eq!(pcurve_parameter_domain(&pcurve), Some([0.0, 10.0]));
    assert_eq!(
        pcurve_nurbs_knots(&pcurve),
        Some(vec![0.0, 0.0, 10.0, 10.0])
    );
    assert_eq!(evaluate_pcurve(&pcurve, 0.0), Some([0.0, 0.0]));
    assert_eq!(evaluate_pcurve(&pcurve, 10.0), Some([1.0, 0.0]));

    let pcurves = BTreeMap::from([(2, pcurve)]);
    let surfaces = BTreeMap::from([(
        7,
        B5Surface::Plane {
            origin: [0.0, 0.0, 0.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
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
        lift_parameter_incidence(2, 0.0, &geometry),
        Some([0.0, 0.0, 0.0])
    );
    assert_eq!(
        lift_parameter_incidence(2, 10.0, &geometry),
        Some([1.0, 0.0, 0.0])
    );
}

#[test]
fn surface_candidate_merge_refines_opaque_wrappers_and_rejects_exact_conflicts() {
    let unknown = B5Surface::Unknown {
        family: 0xb5,
        class: 0x2e,
        payload: vec![0x81, 0x82],
    };
    let plane = B5Surface::Plane {
        origin: [0.0; 3],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let cylinder = B5Surface::Cylinder {
        origin: [0.0; 3],
        reference_x: [1.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 1.0,
        u_range: [0.0, std::f64::consts::TAU],
        v_range: [-1.0, 1.0],
        angular_scale: 1.0,
        chart_origin: 0.0,
    };
    let mut surfaces = BTreeMap::from([(1, unknown)]);
    let mut conflicts = HashSet::new();
    assert!(merge_surface_candidate(
        &mut surfaces,
        &mut conflicts,
        1,
        plane.clone(),
    ));
    assert_eq!(surfaces.get(&1), Some(&plane));

    assert!(!merge_surface_candidate(
        &mut surfaces,
        &mut conflicts,
        1,
        cylinder,
    ));
    assert!(!merge_surface_candidate(
        &mut surfaces,
        &mut conflicts,
        1,
        plane,
    ));
    assert!(!surfaces.contains_key(&1));
    assert!(conflicts.contains(&1));
}

#[test]
fn full_surface_alias_closure_is_order_independent_unbounded_and_cycle_safe() {
    let alias = |object_id: u32, target: u32| B5Record {
        offset: usize::try_from(object_id).expect("small object id"),
        family: 0xb5,
        class: 0x2e,
        object_id,
        payload: vec![0x81, 0x80 + u8::try_from(target).expect("compact target")],
    };
    let mut records = (1..30)
        .rev()
        .map(|object_id| alias(object_id, object_id + 1))
        .collect::<Vec<_>>();
    let cycle_start = records.len();
    records.push(alias(40, 41));
    records.push(alias(41, 40));
    let by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let plane = B5Surface::Plane {
        origin: [0.0; 3],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let mut surfaces = BTreeMap::from([(30, plane.clone())]);
    let mut conflicts = HashSet::new();
    assert!(resolve_surface_aliases(
        &records,
        &by_id,
        &mut surfaces,
        &mut conflicts,
    ));
    assert_eq!(surfaces.get(&1), Some(&plane));
    assert_eq!(surfaces.get(&29), Some(&plane));
    assert_eq!(
        surface_alias_carrier(records[cycle_start].object_id, &by_id, &surfaces),
        None
    );
    assert!(!surfaces.contains_key(&40));
    assert!(!surfaces.contains_key(&41));
}

#[test]
fn canonical_surface_identity_follows_unbounded_aliases_and_rejects_cycles() {
    let aliases = (1..30)
        .map(|object_id| (object_id, object_id + 1))
        .chain([(40, 41), (41, 40)])
        .collect();

    assert_eq!(canonical_surface_id(&aliases, 1), Some(30));
    assert_eq!(canonical_surface_id(&aliases, 29), Some(30));
    assert_eq!(canonical_surface_id(&aliases, 30), Some(30));
    assert_eq!(canonical_surface_id(&aliases, 40), None);
    assert_eq!(canonical_surface_id(&aliases, 41), None);
}

#[test]
fn surface_alias_closes_after_its_terminal_construction_resolves() {
    let records = vec![B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x2e,
        object_id: 1,
        payload: vec![0x81, 0x82],
    }];
    let by_id = HashMap::from([(1, &records[0])]);
    let mut surfaces = BTreeMap::new();
    let mut conflicts = HashSet::new();
    assert!(!resolve_surface_aliases(
        &records,
        &by_id,
        &mut surfaces,
        &mut conflicts,
    ));

    let plane = B5Surface::Plane {
        origin: [0.0; 3],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    surfaces.insert(2, plane.clone());
    assert!(resolve_surface_aliases(
        &records,
        &by_id,
        &mut surfaces,
        &mut conflicts,
    ));
    assert_eq!(surfaces.get(&1), Some(&plane));
}

#[test]
fn targeted_surface_resolution_follows_a_supported_surface_to_a_rolling_ball_carrier() {
    let mut payload = vec![0x85];
    for reference in 1u16..=5 {
        payload.push(0x18);
        payload.extend_from_slice(&reference.to_le_bytes());
    }
    payload.extend_from_slice(&[0x01, 0x02]);
    payload.extend_from_slice(&2.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x03, 0x04]);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    payload.extend_from_slice(&[0x05, 0x06]);
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x37,
        object_id: 10,
        payload,
    };
    let records = HashMap::from([(10, Some(record))]);
    let rolling = HashMap::from([(
        1,
        Some(B5Surface::RollingBall {
            carrier_object_id: 1,
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
        }),
    )]);
    assert_eq!(
        resolve_targeted_surface(10, &records, &HashMap::new(), &HashMap::new(), &rolling),
        rolling.get(&1).cloned().flatten()
    );
}

#[test]
fn targeted_surface_resolution_validates_an_analytic_offset_carrier() {
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
    let mut payload = vec![0x82, 0x82, 0x83];
    payload.extend_from_slice(&(-0.5f64).to_le_bytes());
    payload.push(0x15);
    for value in [-2.0f64, 3.0, -4.0, 5.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    let offset = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x30,
        object_id: 9,
        payload,
    };
    let records = HashMap::from([(9, Some(offset.clone()))]);
    let resolved = HashMap::from([(2, Some(carrier.clone())), (3, Some(source))]);

    assert_eq!(
        resolve_targeted_surface(9, &records, &HashMap::new(), &resolved, &HashMap::new(),),
        Some(carrier)
    );

    let mut wrong_distance = offset.clone();
    wrong_distance.payload[3..11].copy_from_slice(&(-0.25f64).to_le_bytes());
    assert!(resolve_targeted_surface(
        9,
        &HashMap::from([(9, Some(wrong_distance))]),
        &HashMap::new(),
        &resolved,
        &HashMap::new(),
    )
    .is_none());
    let mut wrong_kind = offset;
    wrong_kind.payload[11] = 0x05;
    assert!(resolve_targeted_surface(
        9,
        &HashMap::from([(9, Some(wrong_kind))]),
        &HashMap::new(),
        &resolved,
        &HashMap::new(),
    )
    .is_none());
}

#[test]
fn targeted_surface_resolution_has_no_alias_depth_limit() {
    let records = (1u32..=20)
        .map(|object_id| {
            (
                object_id,
                Some(B5Record {
                    offset: usize::try_from(object_id).expect("small object id"),
                    family: 0xb5,
                    class: 0x2e,
                    object_id,
                    payload: vec![
                        0x81,
                        0x80 + u8::try_from(object_id + 1).expect("compact target"),
                    ],
                }),
            )
        })
        .collect();
    let rolling = HashMap::from([(
        21,
        Some(B5Surface::RollingBall {
            carrier_object_id: 21,
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
        }),
    )]);
    assert_eq!(
        resolve_targeted_surface(1, &records, &HashMap::new(), &HashMap::new(), &rolling),
        rolling.get(&21).cloned().flatten()
    );
}

#[test]
fn targeted_surface_resolution_rejects_alias_cycles() {
    let alias = |object_id, target| {
        Some(B5Record {
            offset: usize::try_from(object_id).expect("small object id"),
            family: 0xb5,
            class: 0x2e,
            object_id,
            payload: vec![0x81, 0x80 + u8::try_from(target).expect("compact target")],
        })
    };
    let records = HashMap::from([(1, alias(1, 2)), (2, alias(2, 1))]);
    assert!(resolve_targeted_surface(
        1,
        &records,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    )
    .is_none());
}

#[test]
fn targeted_surface_resolution_rejects_conflicting_exact_carriers() {
    let rolling = HashMap::from([(
        1,
        Some(B5Surface::RollingBall {
            carrier_object_id: 1,
            definition: ProceduralSurfaceDefinition::Unknown { record: None },
        }),
    )]);
    let resolved = HashMap::from([(
        1,
        Some(B5Surface::Nurbs(NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_count: 2,
            v_count: 2,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 4],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        })),
    )]);
    assert!(
        resolve_targeted_surface(1, &HashMap::new(), &HashMap::new(), &resolved, &rolling,)
            .is_none()
    );
}

#[test]
fn requested_edge_support_scan_closes_through_its_unique_wrapper() {
    let mut bytes = Vec::new();
    let append = |bytes: &mut Vec<u8>, class, object_id: u32, payload: &[u8]| {
        bytes.extend_from_slice(&[0xb5, 0x03, class, payload.len() as u8]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    };
    append(
        &mut bytes,
        0x23,
        20,
        &[0x82, 0x18, 30, 0, 0x18, 31, 0, 0x01],
    );
    append(
        &mut bytes,
        0x5e,
        40,
        &[
            0x85, 0x18, 20, 0, 0x18, 1, 0, 0x18, 2, 0, 0x18, 3, 0, 0x18, 4, 0, 0x22,
        ],
    );
    assert_eq!(
        edge_support_pcurve_references(&bytes, &HashSet::from([40])),
        BTreeMap::from([(40, [30, 31])])
    );
    assert!(edge_support_pcurve_references(&bytes, &HashSet::from([41])).is_empty());
}

#[test]
fn face_surface_references_do_not_require_resolved_loops() {
    let mut bytes = Vec::new();
    for (object_id, surface_id) in [(500u32, 100u8), (501, 100), (500, 101)] {
        bytes.extend_from_slice(&[0xb5, 0x03, 0x5f, 5]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(&[0x82, 0x08, surface_id, 0x00, 0x05]);
    }
    assert_eq!(
        face_surface_references(&bytes),
        vec![(500, 100), (501, 100), (500, 101)]
    );
}

#[test]
fn counted_face_references_accept_both_exact_terminal_controls() {
    for terminal_control in [0x03, 0x05] {
        let record = B5Record {
            offset: 0,
            family: 0xb5,
            class: 0x5f,
            object_id: 3,
            payload: vec![0x82, 0x81, 0x82, terminal_control],
        };
        assert_eq!(
            parse_face_record(&record),
            Some(B5FaceRecord {
                object_id: 3,
                references: vec![1, 2],
                terminal_control: Some(terminal_control),
            })
        );

        let mut overlong = record;
        overlong.payload.push(terminal_control);
        assert_eq!(parse_face_record(&overlong), None);
    }
}

#[test]
fn counted_face_references_reject_unknown_terminal_controls() {
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x5f,
        object_id: 3,
        payload: vec![0x82, 0x81, 0x82, 0x04],
    };
    assert_eq!(parse_face_record(&record), None);

    let mut empty = record;
    empty.payload.clear();
    assert_eq!(parse_face_record(&empty), None);
    empty.payload.extend_from_slice(&[0x80, 0x03]);
    assert_eq!(parse_face_record(&empty), None);
}

#[test]
fn face_references_can_repeat_one_carrier_through_an_alias() {
    let plane = B5Surface::Plane {
        origin: [0.0; 3],
        direction_u: [1.0, 0.0, 0.0],
        direction_v: [0.0, 1.0, 0.0],
        u_range: [-1.0, 1.0],
        v_range: [-1.0, 1.0],
    };
    let record = B5FaceRecord {
        object_id: 30,
        references: vec![10, 11, 20],
        terminal_control: Some(0x05),
    };
    let loops = BTreeMap::from([(
        20,
        B5Loop {
            object_id: 20,
            pcurves: Vec::new(),
            edges: Vec::new(),
            metadata: test_loop_metadata(0),
            surface: 10,
        },
    )]);
    let surfaces = BTreeMap::from([(10, plane.clone()), (11, plane)]);
    let aliases = BTreeMap::from([(11, 10)]);

    let face = parse_face(&record, &loops, &surfaces, &aliases).expect("aliased face");
    assert_eq!(face.surface, 10);
    assert_eq!(face.loops, vec![20]);

    assert!(parse_face(&record, &loops, &surfaces, &BTreeMap::new()).is_none());
}

#[test]
fn one_edge_loop_closes_on_one_native_vertex() {
    let loop_ = B5Loop {
        object_id: 1,
        pcurves: vec![2],
        edges: vec![3],
        metadata: test_loop_metadata(1),
        surface: 4,
    };

    assert!(loop_chain_closes(&loop_, &BTreeMap::from([(3, [0, 0])])));
    assert!(!loop_chain_closes(&loop_, &BTreeMap::from([(3, [0, 1])])));
}

#[test]
fn loop_chain_requires_each_source_native_edge_sense() {
    let mut loop_ = B5Loop {
        object_id: 1,
        pcurves: vec![4, 5, 6],
        edges: vec![1, 2, 3],
        metadata: test_loop_metadata(3),
        surface: 7,
    };
    loop_.metadata.edge_controls[1][0] = -1;
    let edge_vertices = BTreeMap::from([(1, [0, 1]), (2, [2, 1]), (3, [2, 0])]);
    assert!(loop_chain_closes(&loop_, &edge_vertices));

    loop_.metadata.edge_controls[1][0] = 1;
    assert!(!loop_chain_closes(&loop_, &edge_vertices));
}

#[test]
fn opaque_pcurve_occurrences_defer_endpoint_binding_to_native_edges() {
    let loop_ = B5Loop {
        object_id: 1,
        pcurves: vec![2],
        edges: vec![3],
        metadata: test_loop_metadata(1),
        surface: 4,
    };
    let pcurves = BTreeMap::new();
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
        bind_edge_vertices(&BTreeMap::from([(1, loop_)]), &geometry, &[],),
        BTreeMap::new()
    );
}

#[test]
fn sphere_great_circle_pcurve_binds_endpoint_rows() {
    let chart_scale = 8.0;
    let parameter_end = chart_scale * std::f64::consts::FRAC_PI_2;
    let pcurve = B5OpaquePcurve {
        object_id: 2,
        surface: 4,
        class: 0x1d,
        payload: Vec::new(),
        sphere_great_circle: Some(B5SphereGreatCirclePcurve {
            chart_bounds: [
                [0.0, parameter_end],
                [0.0, chart_scale * std::f64::consts::TAU],
            ],
            chart_shift: 0.0,
            chart_scale,
            slope: 0.0,
            phase: 0.0,
        }),
    };
    let loop_ = B5Loop {
        object_id: 1,
        pcurves: vec![2],
        edges: vec![3],
        metadata: test_loop_metadata(1),
        surface: 4,
    };
    let surface = B5Surface::Sphere {
        center: [0.0, 0.0, 0.0],
        direction_x: [1.0, 0.0, 0.0],
        direction_y: [0.0, 1.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        radius: 5.0,
        azimuth_range: [0.0, std::f64::consts::TAU],
        latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
        construction_radius: chart_scale,
        chart_origin: 0.0,
    };
    let opaque_pcurves = BTreeMap::from([(2, pcurve.clone())]);
    let surfaces = BTreeMap::from([(4, surface)]);
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
    let endpoints = pcurve_endpoints(2, 3, &geometry).expect("validated sphere pcurve endpoints");
    assert!(distance_squared(endpoints[0], [5.0, 0.0, 0.0]) < 1e-24);
    assert!(distance_squared(endpoints[1], [0.0, 5.0, 0.0]) < 1e-24);

    assert_eq!(
        bind_edge_vertices(
            &BTreeMap::from([(1, loop_.clone())]),
            &geometry,
            &[[5.0, 0.0, 0.0], [0.0, 5.0, 0.0]],
        ),
        BTreeMap::from([(3, [0, 1])])
    );

    let trimmed_start = parameter_end * 0.25;
    let trimmed_end = parameter_end * 0.75;
    let edge_parameter_incidences = BTreeMap::from([(3, [20, 21])]);
    let parameter_incidences = BTreeMap::from([
        (
            20,
            B5ParameterIncidence {
                object_id: 20,
                curves: vec![2],
                parameters: vec![trimmed_start],
                controls: vec![1],
            },
        ),
        (
            21,
            B5ParameterIncidence {
                object_id: 21,
                curves: vec![2],
                parameters: vec![trimmed_end],
                controls: vec![1],
            },
        ),
    ]);
    let trimmed_geometry = B5PcurveContext {
        pcurves: &pcurves,
        opaque_pcurves: &opaque_pcurves,
        surfaces: &surfaces,
        profiles: &profiles,
        edge_parameter_incidences: &edge_parameter_incidences,
        parameter_incidences: &parameter_incidences,
    };
    let trimmed_points = [
        sphere_great_circle_point(
            opaque_pcurves[&2]
                .sphere_great_circle
                .as_ref()
                .expect("great circle"),
            &surfaces[&4],
            trimmed_start,
        )
        .expect("trimmed start"),
        sphere_great_circle_point(
            opaque_pcurves[&2]
                .sphere_great_circle
                .as_ref()
                .expect("great circle"),
            &surfaces[&4],
            trimmed_end,
        )
        .expect("trimmed end"),
    ];
    assert_eq!(
        pcurve_endpoints(2, 3, &trimmed_geometry),
        Some(trimmed_points)
    );
    assert_eq!(
        bind_edge_vertices(
            &BTreeMap::from([(1, loop_)]),
            &trimmed_geometry,
            &trimmed_points,
        ),
        BTreeMap::from([(3, [0, 1])])
    );
}

#[test]
fn native_vertex_identity_retains_finite_separated_lifts_with_tolerance() {
    let endpoints = [[1.0e8, 2.0e8, 3.0e8], [1.0e8 + 5.0, 2.0e8, 3.0e8]];
    let pcurves = BTreeMap::from([(
        2,
        B5Pcurve {
            object_id: 2,
            surface: 4,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [1.0, 0.0]],
            weights: None,
            parameter_range: None,
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: None,
            lifted_endpoints: Some(endpoints),
        },
    )]);
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
    let loop_ = B5Loop {
        object_id: 1,
        pcurves: vec![2],
        edges: vec![3],
        metadata: test_loop_metadata(1),
        surface: 4,
    };

    let bound = bind_native_vertices(
        &BTreeMap::from([(1, loop_.clone())]),
        &geometry,
        &BTreeMap::from([(3, [10, 11])]),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &[],
    );

    assert_eq!(bound.edges, BTreeMap::from([(3, [0, 1])]));
    assert_eq!(bound.refs, vec![10, 11]);
    assert_eq!(bound.points, endpoints);
    assert!(bound.tolerances.is_empty());

    let mismatched = bind_native_vertices(
        &BTreeMap::from([(1, loop_)]),
        &geometry,
        &BTreeMap::from([(3, [10, 11])]),
        &BTreeMap::new(),
        &BTreeMap::from([(10, endpoints[1])]),
        &[],
    );
    assert_eq!(mismatched.edges, BTreeMap::from([(3, [0, 1])]));
    assert_eq!(mismatched.refs, vec![10, 11]);
    assert_eq!(mismatched.points, [endpoints[1], endpoints[1]]);
    assert_eq!(mismatched.tolerances.len(), 1);
    assert!((mismatched.tolerances[&0] - (5.0 + 1e-9)).abs() < f64::EPSILON);
}

#[test]
fn canonical_point_uses_the_on_carrier_tolerance() {
    let points = [[0.0, 0.0, 0.0]];
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
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [10.0, 0.0]],
            weights: None,
            parameter_range: None,
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: None,
            lifted_endpoints: Some([[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]),
        },
    )]);
    let surfaces = BTreeMap::from([(
        4,
        B5Surface::Plane {
            origin: [0.0, 0.0, 0.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [0.0, 10.0],
            v_range: [-1.0, 1.0],
        },
    )]);
    let edge_parameter_incidences = BTreeMap::from([(3, [20, 21])]);
    let parameter_incidences = BTreeMap::from([
        (
            20,
            B5ParameterIncidence {
                object_id: 20,
                curves: vec![2],
                parameters: vec![0.25],
                controls: vec![1],
            },
        ),
        (
            21,
            B5ParameterIncidence {
                object_id: 21,
                curves: vec![2],
                parameters: vec![0.75],
                controls: vec![1],
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
        pcurves: vec![2],
        edges: vec![3],
        metadata: test_loop_metadata(1),
        surface: 4,
    };

    assert_eq!(
        pcurve_endpoints(2, 3, &geometry),
        Some([[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]])
    );
    assert_eq!(
        bind_edge_vertices(
            &BTreeMap::from([(1, loop_)]),
            &geometry,
            &[[2.5, 0.0, 0.0], [7.5, 0.0, 0.0]],
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
            distinct_knots: vec![2.0, 8.0],
            multiplicities: vec![2, 2],
            control_points: vec![[2.0, 0.0], [8.0, 0.0]],
            weights: None,
            parameter_range: Some([2.0, 8.0]),
            parameterization: B5PcurveParameterization::Native,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        },
    )]);
    let surfaces = BTreeMap::from([(
        4,
        B5Surface::Plane {
            origin: [0.0, 0.0, 0.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [0.0, 10.0],
            v_range: [-1.0, 1.0],
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
        Some([[2.0, 0.0, 0.0], [8.0, 0.0, 0.0]])
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
                chart_bounds: [[0.0, parameter], [0.0, chart_scale * std::f64::consts::TAU]],
                chart_shift: 0.0,
                chart_scale,
                slope: 0.0,
                phase: 0.0,
            }),
        },
    )]);
    let surfaces = BTreeMap::from([(
        4,
        B5Surface::Sphere {
            center: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 5.0,
            azimuth_range: [0.0, std::f64::consts::TAU],
            latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
            construction_radius: chart_scale,
            chart_origin: 0.0,
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
    assert_eq!(counted_references(&records[0], 0x05), Some(vec![30, 31]));
    let incidence = parameter_incidence(&records[1]).expect("parameter incidence");
    assert_eq!(incidence.curves, [2]);
    assert_eq!(incidence.parameters, [parameter]);
    assert!(
        distance_squared(
            sphere_great_circle_point(
                opaque_pcurves[&2]
                    .sphere_great_circle
                    .as_ref()
                    .expect("great circle"),
                &surfaces[&4],
                parameter,
            )
            .expect("sphere endpoint"),
            [0.0, 5.0, 0.0]
        ) < 1e-24
    );
    let coordinates = incidence_vertex_coordinates(
        &BTreeMap::from([(40, [10, 11])]),
        &BTreeMap::from([(
            10,
            B5VertexIncidenceLink {
                object_id: 10,
                incidence: 20,
                terminal_control: 0x00,
            },
        )]),
        &by_id,
        &geometry,
    );

    assert_eq!(coordinates.len(), 1);
    assert!(
        distance_squared(
            *coordinates.get(&10).expect("native vertex coordinate"),
            [0.0, 5.0, 0.0]
        ) < 1e-24
    );

    drop(by_id);
    records[2].payload[3..11].copy_from_slice(&0.0f64.to_le_bytes());
    let conflicting_by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let conflicting = incidence_vertex_coordinates(
        &BTreeMap::from([(40, [10, 11])]),
        &BTreeMap::from([(
            10,
            B5VertexIncidenceLink {
                object_id: 10,
                incidence: 20,
                terminal_control: 0x00,
            },
        )]),
        &conflicting_by_id,
        &geometry,
    );
    assert!(conflicting.is_empty());

    drop(conflicting_by_id);
    records[2].payload[3..11].copy_from_slice(&(parameter + 1.0).to_le_bytes());
    let out_of_domain_by_id = records
        .iter()
        .map(|record| (record.object_id, record))
        .collect::<HashMap<_, _>>();
    let out_of_domain = incidence_vertex_coordinates(
        &BTreeMap::from([(40, [10, 11])]),
        &BTreeMap::from([(
            10,
            B5VertexIncidenceLink {
                object_id: 10,
                incidence: 20,
                terminal_control: 0x00,
            },
        )]),
        &out_of_domain_by_id,
        &geometry,
    );
    assert!(out_of_domain.is_empty());
}

#[test]
fn conflicting_geometric_endpoints_defer_one_edge_to_native_identity() {
    let loops = BTreeMap::from([
        (
            1,
            B5Loop {
                object_id: 1,
                pcurves: vec![10],
                edges: vec![20],
                metadata: test_loop_metadata(1),
                surface: 30,
            },
        ),
        (
            2,
            B5Loop {
                object_id: 2,
                pcurves: vec![11],
                edges: vec![20],
                metadata: test_loop_metadata(1),
                surface: 31,
            },
        ),
        (
            3,
            B5Loop {
                object_id: 3,
                pcurves: vec![12],
                edges: vec![21],
                metadata: test_loop_metadata(1),
                surface: 32,
            },
        ),
    ]);
    let pcurve = |object_id, endpoints| B5Pcurve {
        object_id,
        surface: object_id + 20,
        degree: 1,
        distinct_knots: vec![0.0, 1.0],
        multiplicities: vec![2, 2],
        control_points: vec![[0.0, 0.0], [1.0, 0.0]],
        weights: None,
        parameter_range: None,
        parameterization: B5PcurveParameterization::Native,
        class_21_suffix_scalar: None,
        lifted_endpoints: Some(endpoints),
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
        bind_edge_vertices(&loops, &geometry, &points),
        BTreeMap::from([(21, [0, 2])])
    );
}
