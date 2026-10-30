use super::*;

#[test]
fn plane_support_layout_keeps_an_invalid_generic_frame_unresolved() {
    let cache = ScalarCache::from_section(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    let mut body = vec![0x46, 0x08, 0, 0, 0, 0, 0, 0];
    body.extend([0x10; 10]);
    body.push(0x18);

    assert_eq!(
        crate::decode::with_service_context(|ctx| decode_plane_support_local_system(
            ctx,
            &body,
            &cache,
            &mut Vec::new(),
        ))
        .expect("decode the plane support local system")
        .map(|(_, layout)| layout),
        Some(PlaneSupportFrameLayout::SupportTriples)
    );
    assert_eq!(
        crate::decode::with_service_context(|ctx| decode_plane_support_local_system(
            ctx,
            &[0x10; 12],
            &ScalarCache::default(),
            &mut Vec::new(),
        ))
        .expect("decode the plane support local system")
        .map(|(_, layout)| layout),
        Some(PlaneSupportFrameLayout::SupportTriples)
    );
    assert_eq!(
        plane_support_layout(
            &[
                1.0, 0.0, 0.0, // first matrix row
                0.0, 0.0, 1.0, // second matrix row
                0.0, 0.0, 0.0, // third matrix row
                0.0, 0.0, 0.0,
            ],
            true,
        ),
        PlaneSupportFrameLayout::MatrixColumns
    );
    assert_eq!(
        plane_support_layout(
            &[
                0.6, 0.0, 0.8, // direct parameter direction
                0.0, 0.0, 0.0, // direct zero rank
                0.8, 0.0, -0.6, // direct plane normal
                0.0, 0.0, 0.0,
            ],
            true,
        ),
        PlaneSupportFrameLayout::DirectNormalTriples
    );
}

#[test]
fn complete_local_system_rejects_a_nonfinite_slot() {
    let cache = ScalarCache {
        entries: vec![1.0, f64::NAN],
        paired_byte_1_by_tail: BTreeMap::new(),
    };
    let mut body = Vec::new();
    for _ in 0..9 {
        body.extend_from_slice(&[0x18, 0x00]);
    }
    for _ in 0..3 {
        body.extend_from_slice(&[0x18, 0x01]);
    }

    assert!(decode_feature_local_system_slots(&body, &cache).is_none());
}

#[test]
fn positional_torus_prefix_holds_the_admitted_finite_frame() {
    let mut body = Vec::new();
    for _ in 0..12 {
        body.extend_from_slice(&[0x18, 0x00]);
    }
    let finite = ScalarCache {
        entries: vec![1.0],
        paired_byte_1_by_tail: BTreeMap::new(),
    };
    let (frame, cursor) = decode_positional_torus_local_system_prefix(&body, &finite)
        .expect("the twelve finite coordinates are admitted");
    assert_eq!(frame.get(), [1.0; 12]);
    assert_eq!(cursor, body.len());

    let nonfinite = ScalarCache {
        entries: vec![f64::NAN],
        paired_byte_1_by_tail: BTreeMap::new(),
    };
    assert!(decode_positional_torus_local_system_prefix(&body, &nonfinite).is_none());
}

#[test]
fn saved_conic_local_system_expands_its_planar_normal() {
    let body = [
        0xf9, 4, 3, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x0f, 0x0f, 0x0f,
    ];

    assert_eq!(
        decode_saved_conic_local_system_prefix(&body, &ScalarCache::default()),
        Some((
            cadmpeg_ir::units::FiniteVector::new([
                1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0
            ])
            .expect("finite local system"),
            body.len()
        ))
    );
}

#[test]
fn positional_plane_origin_yz_fall_back_to_the_second_coordinate_lane() {
    let body = [
        0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f, 0x9f, 0x77, 0xa7, 0x70, 0x76, 0xc8, 0xb8, 0x2d,
        0x1e, 0, 0, 0, 0, 0, 0x65, 0xad, 0x53, 0xd5, 0xa1, 0x38, 0xce, 0xd8,
    ];

    assert_eq!(
        decode_positional_plane_local_system_slots(&body, &ScalarCache::default()).map(|slots| {
            let slots = slots.get();
            [slots[9], slots[10], slots[11]]
        }),
        Some([
            f64::from_be_bytes([0x40, 0x14, 0x77, 0xa7, 0x70, 0x76, 0xc8, 0xb8]),
            f64::from_be_bytes([0xc0, 0x1e, 0, 0, 0, 0, 0, 0x65]),
            f64::from_be_bytes([0xbf, 0xd9, 0x53, 0xd5, 0xa1, 0x38, 0xce, 0xd8]),
        ])
    );
}

#[test]
fn plane_support_origin_uses_positive_compact_half() {
    let body = [
        0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4, 0x0e, 0x18, 0xe4,
    ];
    let cache = ScalarCache::default();

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &cache),
        Some([0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.0])
    );
    assert_eq!(
        decode_positional_plane_local_system_slots(&body, &cache).map(|slots| slots.get()[9]),
        Some(-0.5)
    );
}

#[test]
fn compact_plane_origins_repeat_the_preceding_scalar_and_keep_signed_units() {
    let prefix = [0x0f, 0x18, 0xe6, 0x0f, 0x18, 0x10, 0x18];
    for (origin, expected) in [
        (vec![0xe6], [0.0, 0.0, 0.0]),
        (vec![0xe5, 0x0f], [0.0, 0.0, 1.0]),
        (vec![0x0f, 0xe5], [1.0, 1.0, 1.0]),
        (vec![0xe4, 0x10, 0x0e], [0.0, -1.0, 0.5]),
    ] {
        let body = [prefix.as_slice(), &origin].concat();
        let slots = decode_plane_support_local_system_slots(&body, &ScalarCache::default())
            .expect("complete compact frame");
        assert_eq!(&slots[9..], &expected);
    }
    for (unit, expected) in [(0x0f, 1.0), (0x10, -1.0)] {
        let body = [0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, unit, 0xe4, 0x18, 0x18];
        let slots = decode_plane_support_local_system_slots(&body, &ScalarCache::default())
            .expect("complete repeated origin");
        assert_eq!(&slots[9..], &[expected, 0.0, 0.0]);
    }
    for origin in [vec![0xe5], vec![0x18, 0xe6], vec![0xe6, 0x18]] {
        let body = [prefix.as_slice(), &origin].concat();
        assert!(decode_plane_support_local_system_slots(&body, &ScalarCache::default()).is_none());
    }
}

#[test]
fn diagonal_compact_plane_support_names_the_z_normal() {
    let body = [0x0f, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x10, 0x18, 0x18, 0x18];

    let (slots, layout) = crate::decode::with_service_context(|ctx| {
        decode_plane_support_local_system(ctx, &body, &ScalarCache::default(), &mut Vec::new())
    })
    .expect("decode the plane support local system")
    .expect("complete diagonal support image");
    assert_eq!(layout, PlaneSupportFrameLayout::DirectNormalTriples);
    assert_eq!(
        slots,
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0]
    );
}

#[test]
fn plane_support_directions_use_component_coordinate_lanes() {
    let body = [
        0x4e, 0xf0, 0, 0, 0, 0, 0,    // first direction x = 1
        0x18, // first direction y = 0
        0x4c, 0xf0, 0, 0, 0, 0, 0, // first direction z = 1
        0x10, 0x10, 0x10, // zero rank marker
        0x10, 0x10, 0x4c, 0xf0, 0, 0, 0, 0, 0, // second direction
        0x10, 0x10, 0x18, // origin
    ];

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0])
    );
    assert!(decode_positional_plane_local_system_slots(&body, &ScalarCache::default()).is_none());
}

#[test]
fn positional_pcurve_frame_has_bounded_repeats_and_signed_coordinates() {
    let body = [
        0x2d, 0x10, 0, 0, 0, 0, 0, 0, 0xe4, 0x18, 0x0f, 0x46, 0x20, 0, 0, 0, 0, 0, 0, 0x10, 0xe5,
    ];
    let frame =
        super::decode_pcurve_scalar_frame(&body, &ScalarCache::default()).expect("complete pcurve");
    assert_eq!(frame.values, [4.0, 4.0, 0.0, 1.0, -8.0, -1.0, -1.0, -1.0]);
    assert_eq!(frame.token_count, 7);
    for invalid in [
        &body[..body.len() - 1],
        &[body.as_slice(), &[0x18]].concat(),
    ] {
        assert!(super::decode_pcurve_scalar_frame(invalid, &ScalarCache::default()).is_none());
    }
}

#[test]
fn rotation_plane_origins_use_signed_coordinate_forms() {
    const EPS_ROTATION_TEST: f64 = 1e-12;
    let mut body = Vec::new();
    body.extend_from_slice(&positive_subunit_coordinate(0.6));
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(0.8));
    let middle = body.len() + 1;
    body.extend_from_slice(&[0x18, 0x0f, 0x18]);
    let mut negative = positive_subunit_coordinate(0.8);
    negative[0] = 0x45;
    body.extend_from_slice(&negative[..7]);
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(0.6));
    body.extend_from_slice(&[0x2d, 0x10, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x8b, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x46, 0x20, 0, 0, 0, 0, 0, 0]);
    let decoded = crate::decode::with_service_context(|ctx| {
        decode_plane_support_local_system(ctx, &body, &ScalarCache::default(), &mut Vec::new())
    })
    .expect("bounded frame")
    .expect("complete rotation frame");
    assert_eq!(decoded.1, PlaneSupportFrameLayout::RotationColumns);
    assert_eq!(&decoded.0.get()[9..], &[4.0, 2.0, -8.0]);
    let (u, n) = super::rotation_plane_axes(decoded.0.as_raw()).expect("rotation axes");
    for (actual, expected) in u.into_iter().chain(n).zip([0.6, 0.0, -0.8, 0.8, 0.0, 0.6]) {
        assert!((actual - expected).abs() <= EPS_ROTATION_TEST);
    }
    let mut reflected = body.clone();
    reflected[middle] = 0x10;
    for invalid in [reflected, [body.as_slice(), &[0x0f]].concat()] {
        assert!(crate::decode::with_service_context(|ctx| {
            decode_plane_support_local_system(
                ctx,
                &invalid,
                &ScalarCache::default(),
                &mut Vec::new(),
            )
        })
        .expect("bounded frame")
        .is_none());
    }
}

#[test]
fn plane_support_frame_selects_an_alternate_coordinate_lane_by_orthogonality() {
    const EPS_PLANE_SUPPORT_LANE_TEST: f64 = 1e-12;

    let cache = ScalarCache::default();
    let alternate = f64::from_be_bytes([0xbf, 0xdd, 0, 0, 0, 0, 0, 0]);
    let first_z = (1.0 - alternate * alternate).sqrt();
    let mut body = Vec::new();
    body.extend_from_slice(&positive_subunit_coordinate(first_z));
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(-alternate));
    body.extend_from_slice(&[0x0f, 0x0f, 0x0f]);
    body.extend_from_slice(&[
        0xb1, 0, 0, 0, 0, 0, 0, // second-coordinate lane supplies `alternate`
    ]);
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(first_z));
    body.extend_from_slice(&[0x18, 0x18, 0x18]);

    let (slots, layout) = crate::decode::with_service_context(|ctx| {
        decode_plane_support_local_system(ctx, &body, &cache, &mut Vec::new())
    })
    .expect("decode the plane support local system")
    .expect("lane-ambiguous frame has one valid orthogonal interpretation");
    let slots = slots.get();
    assert_eq!(layout, PlaneSupportFrameLayout::DirectNormalTriples);
    assert!((slots[6] - alternate).abs() <= EPS_PLANE_SUPPORT_LANE_TEST);
    assert!(valid_equal_scale_orthogonal_directions(
        [slots[0], slots[1], slots[2]],
        [slots[6], slots[7], slots[8]],
    ));
}

#[test]
fn plane_support_frame_does_not_reinterpret_a_missing_primary_parse() {
    let cache = ScalarCache::default();
    let alternate =
        decode_tabulated_cylinder_second_coordinate(&[0xa4, 0, 0, 0, 0, 0, 0], 0, &cache)
            .expect("synthetic alternate-lane scalar")
            .0;
    let first = (1.0 - alternate * alternate).sqrt();
    let mut body = Vec::new();
    body.extend_from_slice(&positive_subunit_coordinate(first));
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(-alternate));
    body.extend_from_slice(&[0x0f, 0x0f, 0x0f]);
    body.extend_from_slice(&[0xa4, 0, 0, 0, 0, 0, 0]);
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(first));
    body.extend_from_slice(&[0x18, 0x18, 0x18]);

    assert_eq!(
        crate::decode::with_service_context(|ctx| decode_plane_support_lane_variants(
            ctx, &body, &cache,
        ))
        .expect("decode the plane support lane variants")
        .0
        .len(),
        1
    );
    assert!(
        crate::decode::with_service_context(|ctx| decode_plane_support_local_system(
            ctx,
            &body,
            &cache,
            &mut Vec::new(),
        ))
        .expect("decode the plane support local system")
        .is_none()
    );
}

#[test]
fn plane_support_prefix_constructs_orthogonal_directions() {
    let body = [
        0x18, 0x0f, 0x18, 0x18, 0x18, 0xe4, 0xe4, 0x18, 0x18, 0x18, 0x18, 0x18,
    ];

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.0, 0.0, 0.0, 0.0])
    );

    let other_axis = [
        0x18, 0x0f, 0x18, 0xe4, 0x18, 0x18, 0xe4, 0x18, 0xe4, 0x18, 0x18, 0x18,
    ];
    let slots = decode_plane_support_local_system_slots(&other_axis, &ScalarCache::default())
        .expect("complete orthogonal-copy frame");
    assert_eq!(slots[0..3], [1.0, 0.0, 0.0]);
    assert_eq!(slots[3..6], [0.0, 0.0, 0.0]);
    assert_eq!(slots[6..9], [0.0, 0.0, -1.0]);
}

#[test]
fn compact_axis_plane_support_decodes_rank_and_origin() {
    let body = [
        0x18, 0x0f, 0x18, 0xe5, 0x0f, 0xe4, 0x18, 0xe4, 0x18, 0x18, 0x18,
    ];

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0])
    );
}

#[test]
fn normal_y_plane_support_has_a_signed_direct_parameter_normal_frame() {
    for (a, b, c, parameter, normal) in [
        (0x0f, 0x0f, 0x10, 1.0, 1.0),
        (0x10, 0x0f, 0x0f, -1.0, 1.0),
        (0x0f, 0x10, 0x0f, 1.0, -1.0),
        (0x10, 0x10, 0x10, -1.0, -1.0),
    ] {
        let prefix = [a, 0x18, 0xe6, b, 0x18, c, 0x18];
        let origin = [0x2f, 0x02, 0x00, 0x2a, 0xe8, 0x00, 0x2f, 0x26, 0x00];
        let body = [prefix.as_slice(), &origin].concat();
        crate::decode::with_service_context(|ctx| {
            let (slots, layout) = decode_plane_support_local_system(
                ctx,
                &body,
                &ScalarCache::default(),
                &mut Vec::new(),
            )?
            .expect("complete frame");
            assert_eq!(layout, PlaneSupportFrameLayout::DirectNormalTriples);
            assert_eq!(
                slots.get(),
                [parameter, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, normal, 0.0, 2.25, 0.75, 11.0,]
            );
            for invalid in [
                &body[..body.len() - 1],
                &[body.as_slice(), &[0x18]].concat(),
            ] {
                assert!(decode_plane_support_local_system(
                    ctx,
                    invalid,
                    &ScalarCache::default(),
                    &mut Vec::new(),
                )?
                .is_none());
            }
            Ok::<(), cadmpeg_core::CodecError>(())
        })
        .expect("bounded frame decode");
    }
}

#[test]
fn compact_x_plane_support_preserves_both_support_signs() {
    let origin = [0x2f, 0x02, 0x00, 0x2a, 0xe8, 0x00, 0x2f, 0x26, 0x00];
    for (prefix, (first_y, second_z)) in NORMAL_X_PLANE_SUPPORT_PREFIXES.into_iter().zip([
        (1.0, 1.0),
        (-1.0, 1.0),
        (1.0, -1.0),
        (-1.0, -1.0),
    ]) {
        let body = [prefix.as_slice(), &origin].concat();
        assert_eq!(
            decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
            Some([0.0, first_y, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, second_z, 2.25, 0.75, 11.0])
        );
    }
}

#[test]
fn normal_x_plane_support_requires_a_complete_bounded_origin() {
    for prefix in NORMAL_X_PLANE_SUPPORT_PREFIXES {
        let mut truncated = prefix.to_vec();
        truncated.extend([0x18, 0x18]);
        assert!(
            decode_plane_support_local_system_slots(&truncated, &ScalarCache::default()).is_none()
        );

        let mut trailing = prefix.to_vec();
        trailing.extend([0x18, 0x18, 0x18, 0x18]);
        assert!(
            decode_plane_support_local_system_slots(&trailing, &ScalarCache::default()).is_none()
        );
    }
}

#[test]
fn trailing_rank_plane_support_constructs_orthogonal_directions() {
    let body = [
        0xe4, 0x18, 0x18, 0xe4, 0x18, 0xe4, 0x18, 0x0f, 0x18, 0x18, 0x18, 0x18,
    ];

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0])
    );
}

#[test]
fn reflected_plane_support_component_constructs_orthogonal_directions() {
    let mut body = Vec::new();
    body.extend_from_slice(&positive_subunit_coordinate(0.6));
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(0.8));
    body.extend_from_slice(&[0x0f, 0x0f, 0x0f]);
    body.extend_from_slice(&positive_subunit_coordinate(0.8));
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(0.6));
    body.extend_from_slice(&[0x18, 0x18, 0x18]);

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([0.6, 0.0, 0.8, 0.0, 0.0, 0.0, 0.8, 0.0, -0.6, 0.0, 0.0, 0.0])
    );
}

#[test]
fn trailing_rank_reflected_plane_support_constructs_orthogonal_directions() {
    let mut body = vec![0x18];
    body.extend_from_slice(&positive_subunit_coordinate(0.6));
    body.extend_from_slice(&positive_subunit_coordinate(0.8));
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(0.8));
    body.extend_from_slice(&positive_subunit_coordinate(0.6));
    body.extend_from_slice(&[0x18, 0x18, 0xe4, 0x18, 0x18, 0x18]);

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([0.0, 0.6, 0.8, 0.0, 0.0, 0.0, 0.0, 0.8, -0.6, 0.0, 0.0, 0.0])
    );
}

#[test]
fn plane_support_slot_eight_decodes_compact_negated_component() {
    let first_x = f64::from_be_bytes([0x3f, 0xc2, 0, 0, 0, 0, 0, 0]);
    let first_z = (1.0 - first_x * first_x).sqrt();
    let first_x_bytes = positive_subunit_coordinate(first_x);
    let mut body = Vec::new();
    body.extend_from_slice(&first_x_bytes);
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(first_z));
    body.extend_from_slice(&[0x0f, 0x0f, 0x0f]);
    body.extend_from_slice(&positive_subunit_coordinate(first_z));
    body.push(0x18);
    body.push(0x50);
    body.extend_from_slice(&first_x_bytes[2..]);
    body.extend_from_slice(&[0x18, 0x18, 0x18]);

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([first_x, 0.0, first_z, 0.0, 0.0, 0.0, first_z, 0.0, -first_x, 0.0, 0.0, 0.0])
    );
}

#[test]
fn plane_support_slot_six_decodes_paired_positive_component() {
    let first_x = 0.75;
    let first_z = f64::from_be_bytes([0xbf, 0xcf, 0, 0, 0, 0, 0, 0]);
    let second_x = -first_z;
    let mut body = Vec::new();
    body.extend_from_slice(&positive_subunit_coordinate(first_x));
    body.push(0x18);
    body.extend_from_slice(&[0xa4, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x0f, 0x0f, 0x0f]);
    body.extend_from_slice(&[0x4e, 0, 0, 0, 0, 0, 0]);
    body.push(0x18);
    body.extend_from_slice(&positive_subunit_coordinate(first_x));
    body.extend_from_slice(&[0x18, 0x18, 0x18]);

    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([first_x, 0.0, first_z, 0.0, 0.0, 0.0, second_x, 0.0, first_x, 0.0, 0.0, 0.0])
    );
}
