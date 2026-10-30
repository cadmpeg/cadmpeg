use super::{
    admitted_scalar_body, compact_inline_frame, decode, decode_explicit_local_system_slots,
    decode_feature_local_system_slots, decode_in_lane, decode_in_pcurve_lane, decode_in_row_lane,
    decode_in_surface_row_lane, decode_in_torus_row_lane,
    decode_inline_non_plane_local_system_prefix, decode_inline_surface_suffix_scalar,
    decode_model_reference_coordinate, decode_named_local_system_coordinate,
    decode_named_positive_dict_scalar, decode_plane_support_lane_variants,
    decode_plane_support_local_system, decode_plane_support_local_system_slots,
    decode_positional_cylinder_local_system_slots, decode_positional_plane_local_system_slots,
    decode_positional_torus_local_system_prefix, decode_positive_dict,
    decode_round_edge_coordinate, decode_saved_conic_local_system_prefix,
    decode_tabulated_cylinder_first_coordinate, decode_tabulated_cylinder_first_frame_coordinate,
    decode_tabulated_cylinder_frame_coordinate, decode_tabulated_cylinder_second_coordinate,
    decode_two_chart_first_coordinate, double_xar_tables, plane_support_layout,
    valid_equal_scale_orthogonal_directions, CompactFrameAxes, DoubleXarSlot,
    InlineNonPlaneLocalSystemPrefix, PlaneSupportFrameLayout, ScalarCache,
    POSITIONAL_SLOT_TABLE_WIDTH,
};
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;

mod plane_support;

const NORMAL_X_PLANE_SUPPORT_PREFIXES: [[u8; 8]; 4] = [
    [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18],
    [0x18, 0xe4, 0x10, 0xe4, 0x18, 0xe5, 0x0f, 0x18],
    [0x18, 0xe4, 0x10, 0x0f, 0x18, 0xe5, 0x10, 0x18],
    [0x18, 0xe4, 0x0f, 0x10, 0x18, 0xe5, 0x10, 0x18],
];

fn with_depth_limit<T>(
    bytes: &[u8],
    depth: u64,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth;
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("recursive fixture fits the input limit");
    f(&ctx)
}

#[test]
fn scalar_cache_deduplicates_equal_images() {
    let images = [0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x46, 0x08, 0, 0, 0, 0, 0, 0];
    let cache = ScalarCache::from_section_checked(&images);
    assert_eq!(cache.entries.len(), 1, "equal images remain deduplicated");
}

#[test]
fn double_xar_tables_parse_one_complete_table() {
    let bytes = b"double_xar\0\xf8\x02\x10\xe0";
    let tables = double_xar_tables(bytes);
    assert_eq!(tables.len(), 1);
}

#[test]
fn inline_explicit_frame_refuses_recursive_depth_before_absent_candidate() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = [0xe4];
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::decode_inline_non_plane_local_system_prefix(ctx, &bytes, &ScalarCache::default())
            .map(std::iter::Iterator::count)
    };
    assert_eq!(
        with_depth_limit(&bytes, 2, decode).expect("two recursive steps admitted"),
        0
    );
    assert_eq!(
        with_context(&bytes, decode).expect("decode inline non plane local system prefix"),
        0
    );
    let error = with_depth_limit(&bytes, 1, decode).expect_err("need minus one refuses");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("recursive resource refusal expected");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "creo inline explicit frame depth");
    assert_eq!((limit.limit, limit.used, limit.additional), (1, 1, 1));
}

#[test]
fn inline_origin_refuses_recursive_depth_before_absent_candidate() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = [0x18];
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::decode_inline_non_plane_origin_prefix(ctx, &bytes, 0, &ScalarCache::default())
            .map(std::iter::Iterator::count)
    };
    assert_eq!(
        with_depth_limit(&bytes, 2, decode).expect("two recursive steps admitted"),
        0
    );
    assert_eq!(
        with_context(&bytes, decode).expect("decode inline non plane origin prefix"),
        0
    );
    let error = with_depth_limit(&bytes, 1, decode).expect_err("need minus one refuses");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("recursive resource refusal expected");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "creo inline origin depth");
    assert_eq!((limit.limit, limit.used, limit.additional), (1, 1, 1));
}

#[test]
fn plane_support_variants_refuses_recursive_depth_before_absent_candidate() {
    use cadmpeg_core::decode::ResourceDimension;
    let bytes = [0xe4];
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::decode_plane_support_lane_variants(ctx, &bytes, &ScalarCache::default())
            .map(|(frames, _)| frames.len())
    };
    assert_eq!(
        with_depth_limit(&bytes, 2, decode).expect("two recursive steps admitted"),
        0
    );
    assert_eq!(
        with_context(&bytes, decode).expect("decode plane support lane variants"),
        0
    );
    let error = with_depth_limit(&bytes, 1, decode).expect_err("need minus one refuses");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("recursive resource refusal expected");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "creo plane support variants depth");
    assert_eq!((limit.limit, limit.used, limit.additional), (1, 1, 1));
}

#[test]
fn plane_support_lane_caps_report_their_bound() {
    use super::{PlaneSupportLaneCap, PlaneSupportLaneSearch};

    let mut full = PlaneSupportLaneSearch {
        variants: super::MAX_PLANE_SUPPORT_LANE_VARIANTS,
        ..PlaneSupportLaneSearch::default()
    };
    assert!(!super::admit_plane_support_lane_visit(
        &mut full,
        super::MAX_PLANE_SUPPORT_LANE_VARIANTS,
    ));
    assert_eq!(full.capped, Some(PlaneSupportLaneCap::Variants));
    let mut spent = PlaneSupportLaneSearch {
        visits: super::MAX_PLANE_SUPPORT_LANE_VISITS,
        ..PlaneSupportLaneSearch::default()
    };
    assert!(!super::admit_plane_support_lane_visit(
        &mut spent,
        super::MAX_PLANE_SUPPORT_LANE_VARIANTS,
    ));
    assert_eq!(spent.capped, Some(PlaneSupportLaneCap::Visits));
    for (cap, bound) in [
        (
            PlaneSupportLaneCap::Variants,
            "MAX_PLANE_SUPPORT_LANE_VARIANTS (64)",
        ),
        (
            PlaneSupportLaneCap::Visits,
            "MAX_PLANE_SUPPORT_LANE_VISITS (4096)",
        ),
    ] {
        let loss = super::plane_support_variants_capped_loss(cap);
        assert_eq!(
            loss.code,
            crate::loss::CreoLossCode::PlaneSupportVariantsCapped.kind()
        );
        assert!(loss.message.contains(bound), "{}", loss.message);
    }
}

#[test]
fn plane_support_lane_visit_cap_skips_the_frame_with_one_loss() {
    use super::PlaneSupportLaneCap;

    // The lead coordinate leaves the primary frame invalid, and each
    // following 0x18 slot branches the lane search.
    let mut body = vec![0x46, 0xe0, 0, 0, 0, 0, 0, 0];
    body.extend([0x18; 11]);
    let cache = ScalarCache::default();
    let (variants, capped) = crate::decode::with_service_context(|ctx| {
        decode_plane_support_lane_variants(ctx, &body, &cache)
    })
    .expect("the visit cap is not a decode failure");
    assert!(variants.is_empty());
    assert_eq!(capped, Some(PlaneSupportLaneCap::Visits));
    let mut losses = Vec::new();
    for _ in 0..2 {
        let decoded = crate::decode::with_service_context(|ctx| {
            decode_plane_support_local_system(ctx, &body, &cache, &mut losses)
        })
        .expect("the visit cap is not a decode failure");
        assert!(decoded.is_none());
    }
    assert_eq!(
        losses,
        [super::plane_support_variants_capped_loss(
            PlaneSupportLaneCap::Visits
        )]
    );
}

fn with_context<T>(
    bytes: &[u8],
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("the scalar fixture fits the root limit");
    f(&ctx)
}

#[test]
fn scalar_cache_records_one_unique_image() {
    let bytes = [0x46, 0x08, 1, 2, 3, 4, 5, 6];
    let cache = ScalarCache::from_section_checked(&bytes);
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.paired_byte_1(&[1, 2, 3, 4, 5, 6]), Some(0x08));
}

#[test]
fn scalar_cache_checked_preserves_first_image_order_and_tail_conflicts() {
    let bytes = [
        0x46, 0x08, 1, 2, 3, 4, 5, 6, 0x46, 0x09, 1, 2, 3, 4, 5, 6, 0x46, 0x08, 1, 2, 3, 4, 5, 6,
    ];
    let checked = ScalarCache::from_section_checked(&bytes);
    let original = ScalarCache::from_section(&bytes);
    assert_eq!(checked.entries, original.entries);
    assert_eq!(
        checked.paired_byte_1_by_tail,
        original.paired_byte_1_by_tail
    );
    assert_eq!(checked.entries.len(), 2);
    assert_eq!(checked.paired_byte_1(&[1, 2, 3, 4, 5, 6]), None);
}
/// Every arm of the surface-row lane reads the bytes it reports: a decode
/// that states a value advances the cursor and stops inside the body. The
/// slot readers rest on that, so no caller tests the token for emptiness.
#[test]
fn a_surface_row_lane_decode_advances_inside_the_body() {
    // The second byte selects several arms, so it varies over the bytes
    // those arms name as well as over ordinary payload.
    const SECOND: [u8; 12] = [
        0x00, 0x01, 0x0e, 0x73, 0x92, 0xa0, 0xbb, 0xda, 0xe0, 0xe3, 0xf1, 0xf8,
    ];
    let cache = ScalarCache::from_section(&[
        0x46, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x46, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
        0x17,
    ]);
    for opener in 0..=u8::MAX {
        for second in SECOND {
            for length in 1..12usize {
                let mut body = vec![opener];
                if length > 1 {
                    body.push(second);
                }
                body.extend(
                    (2..length).map(|index| u8::try_from(index).expect("fixture value fits u8")),
                );
                for offset in 0..body.len() {
                    for decode in [
                        decode_in_surface_row_lane,
                        decode_in_row_lane,
                        decode_in_lane,
                        decode_in_pcurve_lane,
                        decode_in_torus_row_lane,
                    ] {
                        if let Some((_, next)) = decode(&body, offset, &cache) {
                            assert!(
                                next > offset && next <= body.len(),
                                "opener {opener:#04x} second {second:#04x} \
                                 offset {offset} states {next} over {body:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

fn positive_subunit_coordinate(value: f64) -> [u8; 8] {
    let bytes = value.to_be_bytes();
    assert_eq!(bytes[0], 0x3f);
    [
        0x41, bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]
}

#[test]
fn round_edge_positive_dict_uses_the_extended_prefix_lattice() {
    let low = [0x4b, 0, 0, 0, 0, 0, 0];
    let high = [0xa3, 0, 0, 0, 0, 0, 0];
    let cache = ScalarCache::default();
    let (low_value, low_end) =
        decode_round_edge_coordinate(&low, 0, &cache).expect("low extended positive-DICT prefix");
    let (high_value, high_end) =
        decode_round_edge_coordinate(&high, 0, &cache).expect("high extended positive-DICT prefix");
    assert_eq!(low_end, low.len());
    assert_eq!(high_end, high.len());
    assert_eq!(
        low_value,
        f64::from_be_bytes([0x3f, 0xc0, 0, 0, 0, 0, 0, 0])
    );
    assert_eq!(
        high_value,
        f64::from_be_bytes([0x40, 0x18, 0, 0, 0, 0, 0, 0])
    );
}

#[test]
fn first_directrix_coordinate_uses_the_complete_positive_dict_lattice() {
    let encoded = [0x51, 0x20, 0xbb, 0xca, 0xab, 0x7c, 0x66];

    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&encoded, 0, &ScalarCache::default()),
        Some((
            f64::from_be_bytes([0x3f, 0xc6, 0x20, 0xbb, 0xca, 0xab, 0x7c, 0x66]),
            encoded.len()
        ))
    );
}

#[test]
fn two_chart_coordinates_use_their_bounded_lane_extensions() {
    let first = [0x32, 0, 0, 0, 0, 0, 0, 0];
    let second = [0x56, 0, 0, 0, 0, 0, 0];
    let cache = ScalarCache::default();

    let (first_value, first_end) =
        decode_two_chart_first_coordinate(&first, 0, &cache).expect("first coordinate");
    let (second_value, repeat, second_end) = super::decode_stateful_scalar_run(
        &second,
        0,
        None,
        &cache,
        super::StatefulScalarKind::Pcurve,
    )
    .expect("second coordinate");
    assert_eq!(repeat, 1);
    assert_eq!(first_end, first.len());
    assert_eq!(second_end, second.len());
    assert!((first_value - f64::from_be_bytes([0x3f, 0, 0, 0, 0, 0, 0, 0])).abs() <= f64::EPSILON);
    assert!(
        (second_value - f64::from_be_bytes([0x3f, 0xcb, 0, 0, 0, 0, 0, 0])).abs() <= f64::EPSILON
    );
}

#[test]
fn two_chart_first_coordinate_uses_the_signed_coordinate_lane() {
    let cache = ScalarCache::default();
    let positive = [0x46, 0x28, 0x9c, 0x2c, 0x6a, 0x48, 0x72, 0xb4];
    let negative = [0x2d, 0x28, 0x9c, 0xc2, 0x75, 0x62, 0x07, 0x2d];

    for encoded in [positive, negative] {
        assert_eq!(
            decode_two_chart_first_coordinate(&encoded, 0, &cache),
            decode_tabulated_cylinder_first_coordinate(&encoded, 0, &cache)
        );
        assert_ne!(
            decode_two_chart_first_coordinate(&encoded, 0, &cache),
            decode_in_pcurve_lane(&encoded, 0, &cache)
        );
    }
}

#[test]
fn positional_plane_origin_x_prefers_row_then_signed_first_coordinate_lanes() {
    let cache = ScalarCache::from_section(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    let body = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x4a, 0x08, 0, 0, 0, 0, 0, 0x18, 0x00, 0x0f,
    ];

    assert_eq!(
        decode_positional_plane_local_system_slots(&body, &cache).map(FiniteVector::get),
        Some([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -3.0, 3.0, 0.0])
    );
    assert!(decode_explicit_local_system_slots(&body, &cache).is_none());

    let fixed = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x18, 0x00, 0x0f,
    ];
    assert_eq!(
        decode_positional_plane_local_system_slots(&fixed, &cache).map(|slots| slots.get()[9]),
        Some(3.0)
    );

    let dict_origin = [
        0x18, 0xe4, 0x10, 0x18, 0x0f, 0x18, 0x0f, 0x18, 0xe4, 0x9f, 0x77, 0xa7, 0x70, 0x76, 0xc8,
        0xb8, 0x2d, 0x1e, 0, 0, 0, 0, 0, 0x65, 0xb9, 0x11, 0x9e, 0xed, 0x48, 0x6f, 0x9e,
    ];
    assert_eq!(
        decode_positional_plane_local_system_slots(&dict_origin, &cache).map(|slots| {
            let slots = slots.get();
            [slots[9], slots[10], slots[11]]
        }),
        Some([
            f64::from_be_bytes([0x40, 0x14, 0x77, 0xa7, 0x70, 0x76, 0xc8, 0xb8]),
            f64::from_be_bytes([0xc0, 0x1e, 0, 0, 0, 0, 0, 0x65]),
            f64::from_be_bytes([0xbf, 0x11, 0x9e, 0xed, 0x48, 0x6f, 0x9e, 0]),
        ])
    );
}

#[test]
fn cache_zero_prefix_recognizes_every_short_float_opener() {
    let cache = ScalarCache::default();
    for token in [
        [0x29, 0xe8, 0x00],
        [0x2a, 0xfa, 0x00],
        [0x2e, 0x00, 0x00],
        [0x2f, 0x05, 0x00],
        [0x42, 0xe8, 0x00],
        [0x43, 0xfa, 0x00],
        [0x47, 0x00, 0x00],
        [0x48, 0x05, 0x00],
    ] {
        let body = [0x18, token[0], token[1], token[2]];
        assert_eq!(decode_in_lane(&body, 0, &cache), Some((0.0, 1)));
        assert_eq!(
            decode_in_lane(&body, 1, &cache).map(|(_, end)| end),
            Some(4)
        );
    }
}

#[test]
fn rank_two_image_is_shared_by_explicit_support_slot_lanes() {
    let body = [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6];
    let expected = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let cache = ScalarCache::default();

    assert_eq!(
        decode_positional_plane_local_system_slots(&body, &cache).map(FiniteVector::get),
        Some(expected)
    );
    assert_eq!(
        decode_explicit_local_system_slots(&body, &cache).map(FiniteVector::get),
        Some(expected)
    );
    assert_eq!(
        decode_feature_local_system_slots(&body, &cache).map(FiniteVector::get),
        Some(expected)
    );
    assert_eq!(
        decode_positional_cylinder_local_system_slots(&body, &cache).map(FiniteVector::get),
        Some(expected)
    );
    assert_eq!(
        decode_plane_support_local_system_slots(&body, &cache),
        Some(expected)
    );
}

#[test]
fn inline_non_plane_compact_images_name_the_axis_coordinate() {
    let cases = [
        (
            vec![0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f],
            2,
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        ),
        (
            vec![0x18, 0x10, 0x18, 0x10, 0x18, 0xe6, 0x10],
            2,
            [
                0.0, -1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0,
            ],
        ),
        (
            vec![0x0f, 0x18, 0xe6, 0x0f, 0x18, 0x10, 0x18],
            1,
            [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            vec![0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4],
            0,
            [0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
    ];
    let cache = ScalarCache::default();
    for (body, axis, expected) in cases {
        let frame = crate::decode::with_service_context(|ctx| {
            decode_inline_non_plane_local_system_prefix(ctx, &body, &cache)
        })
        .expect("decode the inline non-plane local system prefix")
        .into_iter()
        .find_map(|prefix| match prefix {
            InlineNonPlaneLocalSystemPrefix::Compact(frame)
                if frame.values.get()[6 + axis] != 0.0 =>
            {
                Some(frame)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("compact inline local-system image {body:02x?}"));
        assert_eq!(frame.cursor, body.len());
        assert_eq!(frame.values, expected);
    }
}

#[test]
fn every_compact_axis_pair_names_two_distinct_model_axes() {
    let pairs = [
        CompactFrameAxes::XY,
        CompactFrameAxes::XZ,
        CompactFrameAxes::YX,
        CompactFrameAxes::YZ,
        CompactFrameAxes::ZX,
        CompactFrameAxes::ZY,
    ];
    for axes in pairs {
        assert_ne!(axes.axis(), axes.reference(), "{axes:?}");
    }
    let mut seen = std::collections::BTreeSet::new();
    for axes in pairs {
        assert!(seen.insert((axes.axis().index(), axes.reference().index())));
    }
    assert_eq!(seen.len(), 6);
}

#[test]
fn compact_plane_rotation_expands_repeats_and_preserves_all_axis_signs() {
    for u_axis in 0..3 {
        for v_axis in 0..3 {
            if u_axis == v_axis {
                continue;
            }
            for u_sign in [-1.0, 1.0] {
                for v_sign in [-1.0, 1.0] {
                    let mut u = [0.0; 3];
                    let mut v = [0.0; 3];
                    u[u_axis] = u_sign;
                    v[v_axis] = v_sign;
                    let n = crate::vecmath::cross(u, v);
                    let matrix: [f64; 9] = std::array::from_fn(|i| [u, v, n][i % 3][i / 3]);
                    let mut body = Vec::new();
                    let mut cursor = 0;
                    while cursor < matrix.len() {
                        let value = matrix[cursor];
                        body.push(if value == 1.0 {
                            0x0f
                        } else if value == -1.0 {
                            0x10
                        } else {
                            0x18
                        });
                        let repeats = matrix[cursor + 1..]
                            .iter()
                            .take(3)
                            .take_while(|next| **next == value)
                            .count();
                        if repeats > 0 {
                            body.push(0xe3 + u8::try_from(repeats).expect("at most three repeats"));
                        }
                        cursor += repeats + 1;
                    }
                    body.extend([0x2f, 0x00, 0x00, 0x2f, 0x10, 0x00, 0x2f, 0x20, 0x00]);
                    let expected = [
                        u[0], u[1], u[2], 0.0, 0.0, 0.0, v[0], v[1], v[2], 2.0, 4.0, 8.0,
                    ];
                    let decoded =
                        super::decode_compact_plane_rotation(&body, &ScalarCache::default())
                            .expect("compact rotation")
                            .expect("complete finite origin");
                    assert_eq!(decoded.get(), expected);
                    body.pop();
                    assert!(
                        super::decode_compact_plane_rotation(&body, &ScalarCache::default())
                            .expect("compact rotation")
                            .is_err()
                    );
                }
            }
        }
    }
}

#[test]
fn compact_plane_rotation_origins_share_signed_literals_and_repeat_state() {
    let mut body = vec![0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f];
    body.extend([
        0x46, 0x10, 0, 0, 0, 0, 0, 0, 0x2d, 0x10, 0, 0, 0, 0, 0, 0, 0xe4,
    ]);
    let cache = ScalarCache::default();
    let slots = super::decode_compact_plane_rotation(&body, &cache)
        .expect("compact rotation")
        .expect("complete origin")
        .get();
    assert_eq!(&slots[9..], &[-4.0, 4.0, 4.0]);
    body.truncate(7);
    body.push(0xe6);
    let slots = super::decode_compact_plane_rotation(&body, &cache)
        .expect("compact rotation")
        .expect("repeat across direction and origin")
        .get();
    assert_eq!(&slots[9..], &[1.0; 3]);
    assert!(super::axis_unit_matrix_body(&body, &cache));
    assert_eq!(
        &super::axis_unit_matrix_slots(&body, &cache)
            .expect("same matrix gate")
            .as_raw()[9..],
        &[1.0; 3]
    );
}

#[test]
fn compact_frames_carry_a_right_handed_orthonormal_basis() {
    for axes in [
        CompactFrameAxes::XY,
        CompactFrameAxes::XZ,
        CompactFrameAxes::YX,
        CompactFrameAxes::YZ,
        CompactFrameAxes::ZX,
        CompactFrameAxes::ZY,
    ] {
        for reference_sign in [1.0, -1.0] {
            for axis_sign in [1.0, -1.0] {
                let values = compact_inline_frame(axes, reference_sign, axis_sign);
                let axis = axes.axis().index();
                let reference = axes.reference().index();
                assert_eq!(values[reference], reference_sign);
                assert_eq!(values[6 + axis], axis_sign);
                let second: [f64; 3] = match values[3..6].try_into() {
                    Ok(second) => second,
                    Err(error) => panic!("compact frame has no second direction: {error}"),
                };
                assert_eq!(
                    second.iter().map(|value| value * value).sum::<f64>(),
                    1.0,
                    "{axes:?} second direction is not a unit vector"
                );
                assert_eq!(second[axis], 0.0);
                assert_eq!(second[reference], 0.0);
                assert_eq!(values[9..12], [0.0; 3]);
            }
        }
    }
}

#[test]
fn compact_x_plane_support_preserves_parameter_and_normal_signs() {
    let mut body = vec![0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4];
    body.extend([0x18, 0x18, 0x18]);

    assert_eq!(
        crate::decode::with_service_context(|ctx| decode_plane_support_local_system(
            ctx,
            &body,
            &ScalarCache::default(),
            &mut Vec::new(),
        ))
        .expect("decode the plane support local system")
        .map(|(values, layout)| (values.get(), layout)),
        Some((
            [0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            PlaneSupportFrameLayout::SupportTriples,
        ))
    );

    body[2] = 0x10;
    body[6] = 0x0f;
    assert_eq!(
        decode_plane_support_local_system_slots(&body, &ScalarCache::default()),
        Some([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0])
    );
}

#[test]
fn inline_non_plane_explicit_frames_expand_the_four_slot_fill() {
    let body = [
        0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x2f,
        0x10, 0x00,
    ];
    let expected = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0];
    let cache = ScalarCache::default();

    assert!(crate::decode::with_service_context(|ctx| {
        decode_inline_non_plane_local_system_prefix(ctx, &body, &cache)
    })
    .expect("decode the inline non-plane local system prefix")
    .into_iter()
    .any(|prefix| matches!(
        prefix,
        InlineNonPlaneLocalSystemPrefix::Explicit(frame)
            if frame.cursor == body.len() && frame.values == expected
    )));
}

#[test]
fn inline_non_plane_explicit_frames_accept_the_reflected_four_slot_fill() {
    let body = [
        0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x18, 0xe5, 0x10, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x2f,
        0x10, 0x00,
    ];
    let expected = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, 2.0, 2.0, 4.0];
    let cache = ScalarCache::default();

    assert!(crate::decode::with_service_context(|ctx| {
        decode_inline_non_plane_local_system_prefix(ctx, &body, &cache)
    })
    .expect("decode the inline non-plane local system prefix")
    .into_iter()
    .any(|prefix| matches!(
        prefix,
        InlineNonPlaneLocalSystemPrefix::Explicit(frame)
            if frame.cursor == body.len() && frame.values == expected
    )));
}

#[test]
fn inline_surface_suffix_uses_exact_zero_and_unit_tokens() {
    let cache = ScalarCache::default();
    assert_eq!(
        decode_inline_surface_suffix_scalar(&[0x0e], 0, &cache),
        Some((0.5, 1))
    );
    assert_eq!(
        decode_inline_surface_suffix_scalar(&[0x0f], 0, &cache),
        Some((1.0, 1))
    );
    assert_eq!(
        decode_inline_surface_suffix_scalar(&[0x18], 0, &cache),
        Some((0.0, 1))
    );
}

#[test]
fn positive_dict_arithmetic_covers_unlisted_inline_prefixes() {
    for prefix in [0x78_u8, 0x7a_u8] {
        let bytes = [prefix, 0, 0, 0, 0, 0, 0];
        let byte_1 = prefix.wrapping_add(0x75);
        assert_eq!(
            decode_positive_dict(&bytes, 0),
            Some((f64::from_be_bytes([0x3f, byte_1, 0, 0, 0, 0, 0, 0]), 7))
        );
    }
}

#[test]
fn inline_non_plane_negative_lanes_follow_their_prefix_arithmetic() {
    let cache = ScalarCache::default();
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&[0xd5, 0, 0, 0, 0, 0, 0], 0, &cache),
        Some((f64::from_be_bytes([0xc0, 0x03, 0, 0, 0, 0, 0, 0]), 7))
    );
    for (prefix, byte_1) in [(0xc0, 0xed), (0xc1, 0xee), (0xc2, 0xef)] {
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&[prefix, 0, 0, 0, 0, 0, 0], 0, &cache,),
            Some((f64::from_be_bytes([0xbf, byte_1, 0, 0, 0, 0, 0, 0]), 7))
        );
    }
}

#[test]
fn decodes_model_reference_wrapped_ieee_coordinate() {
    let data = [0xed, 0x3b, 0xbc, 0xea, 0x89, 0x1b, 0xc2, 0xbd, 0x60];
    let cache = ScalarCache::default();
    assert_eq!(
        decode_model_reference_coordinate(&data, 0, &cache),
        Some((
            f64::from_be_bytes(data[1..].try_into().expect("required invariant")),
            9
        ))
    );
    assert_eq!(
        decode_model_reference_coordinate(&data[..8], 0, &cache),
        None
    );
}

#[test]
fn decodes_model_reference_positive_ieee_coordinate() {
    let data = [0x32, 0xb3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfa];
    let cache = ScalarCache::default();
    assert_eq!(
        decode_model_reference_coordinate(&data, 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xb3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfa]),
            8
        ))
    );
    assert_eq!(
        decode_model_reference_coordinate(&data[..7], 0, &cache),
        None
    );
}

#[test]
fn decodes_model_reference_low_positive_ieee_coordinate() {
    let data = [0x19, 0xc3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfd];
    let cache = ScalarCache::default();
    assert_eq!(
        decode_model_reference_coordinate(&data, 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xc3, 0xa2, 0x70, 0xe5, 0xa0, 0x3f, 0xfd]),
            8
        ))
    );
}

#[test]
fn decodes_counted_double_xar_dictionary() {
    let mut data = b"prefix double_xar\0".to_vec();
    data.extend_from_slice(&[
        0xf8, 0x07, 0x10, 0xe5, 0x07, 0x23, 0x11, 0x2e, 0x0b, 0xe8, 0x26, 0xd6, 0x95, 0x46, 0x08,
        0, 0, 0, 0, 0, 0, 0x0b, 0xe0,
    ]);
    let tables = double_xar_tables(&data);
    let [table] = tables.as_slice() else {
        panic!("complete dictionary");
    };
    assert_eq!(table.entries.len(), 7);
    assert_eq!(
        table
            .entries
            .iter()
            .flat_map(DoubleXarSlot::raw)
            .copied()
            .collect::<Vec<_>>(),
        data[b"prefix double_xar\0".len() + 2..],
    );
    assert_eq!(table.entries[0].value(), Some(1.0));
    assert_eq!(table.entries[1].kind(), "recursive_placeholder_1");
    assert_eq!(table.entries[2].value(), Some(0.0));
    assert_eq!(table.entries[3].kind(), "recursive_placeholder_3");
    assert_eq!(table.entries[4].value(), Some(3.0));
    assert_eq!(table.entries[5].value(), Some(0.0));
    assert_eq!(table.entries[6].kind(), "terminal_null");
}

#[test]
fn withholds_incomplete_double_xar_dictionary() {
    let first = b"double_xar\0\xf8\x02\x10";
    assert!(double_xar_tables(first).is_empty());
    let second = b"double_xar\0\xf8\x02\x10\x0b";
    assert!(double_xar_tables(second).is_empty());
}

#[test]
fn decodes_defined_ieee_forms() {
    assert_eq!(decode(&[0xe4], 0), Some((1.0, 1)));
    assert_eq!(decode(&[0x0d], 0), Some((-1.0, 1)));
    assert_eq!(decode(&[0x46, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((3.0, 8)));
    assert_eq!(decode(&[0x6a, 0x08, 0, 0, 0, 0, 0], 0), Some((3.0, 7)));
    assert_eq!(
        decode(&[0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x2c], 0),
        Some((
            f64::from_be_bytes([0x3f, 0xd3, 0x33, 0x33, 0x33, 0x33, 0x33, 0x2c]),
            7
        ))
    );
    assert_eq!(decode(&[0x2d, 0x08, 0, 0, 0, 0, 0, 0], 0), Some((-3.0, 8)));
    assert_eq!(
        decode(&[0xde, 0x5c, 0xfa, 0x99, 0x80, 0x36, 0x84], 0),
        Some((
            f64::from_be_bytes([0xbf, 0x5c, 0xfa, 0x99, 0x80, 0x36, 0x84, 0]),
            7
        ))
    );
}

#[test]
fn torus_row_lane_decodes_seven_byte_negative_coordinates() {
    let data = [0x2d, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6];
    let cache = ScalarCache::default();

    assert_eq!(decode_in_torus_row_lane(&data, 0, &cache), Some((-7.0, 7)));
    assert_ne!(
        decode_in_surface_row_lane(&data, 0, &cache),
        Some((-7.0, 7))
    );
}

#[test]
fn signed_directrix_lanes_keep_their_distinct_lower_boundary_and_generic_alias() {
    let cache = ScalarCache::default();
    let lower = [0xa4, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
    assert!(decode_tabulated_cylinder_first_coordinate(&lower, 0, &cache).is_none());
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&lower, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xcf, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]),
            7
        ))
    );
    let aliased = [0xb0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
    assert_eq!(
        super::decode(&aliased, 0),
        Some((
            f64::from_be_bytes([0xbf, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&aliased, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xdc, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]),
            7
        ))
    );
    for length in 1..7 {
        assert!(decode_tabulated_cylinder_second_coordinate(&lower[..length], 0, &cache).is_none());
        assert!(
            decode_tabulated_cylinder_first_coordinate(&aliased[..length], 0, &cache).is_none()
        );
    }
}

#[test]
fn tabulated_first_coordinate_preserves_the_negative_subunit_boundary() {
    let cache = ScalarCache::default();
    for (head, ieee_head) in [
        (0xae, 0xbfda_u16),
        (0xaf, 0xbfdb),
        (0xb0, 0xbfdc),
        (0xb1, 0xbfdd),
        (0xb2, 0xbfdf),
    ] {
        let token = [head, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
        let [hi, lo] = ieee_head.to_be_bytes();
        let expected = f64::from_be_bytes([hi, lo, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
        assert_eq!(
            decode_tabulated_cylinder_first_coordinate(&token, 0, &cache),
            Some((expected, 7))
        );
        assert_eq!(
            decode_tabulated_cylinder_first_frame_coordinate(&token, 0, &cache),
            Some((expected, 7))
        );
    }
}

#[test]
fn tabulated_directrix_control_polygon_matches_its_independent_boundary() {
    let cache = ScalarCache::default();
    let bodies = [
        [
            0xb0, 0xdf, 0x28, 0x30, 0, 0, 0, 0x7d, 0x11, 0x39, 0x79, 0x13, 0x66, 0x08,
        ],
        [
            0xb0, 0xdf, 0x28, 0x30, 0, 0, 0, 0x7b, 0xe4, 0xe8, 0x23, 0x13, 0x66, 0x08,
        ],
        [
            0xb0, 0x7b, 0x0c, 0x30, 0, 0, 0, 0x7b, 0x1c, 0xb1, 0xeb, 0x13, 0x66, 0x08,
        ],
        [
            0xaf, 0x9e, 0xf9, 0x4c, 0x0e, 0xbd, 0xf0, 0x7a, 0x76, 0x12, 0xf2, 0xe4, 0x6b, 0xbf,
        ],
    ];
    let boundary = [
        [-0.451_120_421_290_4, 1.129_205_201_103],
        [-0.451_120_421_290_4, 1.055_885_445_609],
        [-0.445_010_229_945_2, 1.007_005_613_58],
        [-0.431_577_991_747_8, 0.983_163_332_366],
    ];
    for (body, expected) in bodies.iter().zip(boundary) {
        let (x, next) = decode_tabulated_cylinder_first_coordinate(body, 0, &cache)
            .expect("first pole coordinate");
        let (z, end) = decode_tabulated_cylinder_second_coordinate(body, next, &cache)
            .expect("second pole coordinate");
        assert_eq!(end, body.len());
        assert!((x - expected[0]).abs() < 1e-12);
        assert!((z - expected[1]).abs() < 1e-12);
    }
}

#[test]
fn decodes_tabulated_cylinder_coordinate_lanes() {
    let cache = ScalarCache::default();
    let first_eight = [0x46, 0x13, 0x77, 0x9f, 0x89, 0x00, 0x00, 0x00];
    let first = [0x4a, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00];
    let first_positive_dict = [0x96, 0x02, 0xf4, 0x7a, 0, 0, 0];
    let first_negative_dict = [0xd7, 0xd4, 0x8d, 0x46, 0, 0, 0];
    let first_negative_subunit = [0xc8, 0xd6, 0xa3, 0x0c, 0, 0, 0];
    let first_negative_large = [0xde, 0xbe, 0x21, 0xc3, 0, 0, 0];
    let first_negative_reserved_gap = [0xdd, 0x9f, 0xe4, 0x46, 0, 0, 0];
    let first_negative_subunit_gap = [0xa7, 0x6b, 0x7c, 0x32, 0x0d, 0x03, 0xd0];
    let first_positive_seven = [0x54, 0xad, 0xf7, 0xa0, 0, 0, 0];
    let first_positive_eight = [0x41, 0xb9, 0x9d, 0x5b, 0x81, 0x25, 0x62, 0xc0];
    let first_negative_low = [0xb2, 0x05, 0xe8, 0xa6, 0, 0, 0];
    let second = [0x7f, 0x24, 0x57, 0x89, 0x13, 0x66, 0x08];
    let second_positive_low = [0x69, 0x91, 0x22, 0x33, 0x44, 0x55, 0x66];
    let second_positive_lower_dict = [0x5c, 0x47, 0x59, 0x45, 0x2d, 0x97, 0x90];
    let second_negative_fixed = [0x45, 0xa7, 0x21, 0x45, 0x78, 0x5e, 0x04];
    let second_negative = [0xc7, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let second_negative_large = [0xdd, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_eight, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x13, 0x77, 0x9f, 0x89, 0, 0, 0]),
            8
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_positive_dict, 0, &cache),
        Some((
            f64::from_be_bytes([0x40, 0x0b, 0x02, 0xf4, 0x7a, 0, 0, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_negative_dict, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x05, 0xd4, 0x8d, 0x46, 0, 0, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_negative_subunit, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xf5, 0xd6, 0xa3, 0x0c, 0, 0, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_negative_large, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x10, 0xbe, 0x21, 0xc3, 0, 0, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_negative_reserved_gap, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x0c, 0x9f, 0xe4, 0x46, 0, 0, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_negative_subunit_gap, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xd3, 0x6b, 0x7c, 0x32, 0x0d, 0x03, 0xd0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_positive_seven, 0, &cache),
        Some((f64::from_be_bytes([0x3f, 0xad, 0xf7, 0xa0, 0, 0, 0, 0]), 7))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_positive_eight, 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xb9, 0x9d, 0x5b, 0x81, 0x25, 0x62, 0xc0]),
            8
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_coordinate(&first_negative_low, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xdf, 0x05, 0xe8, 0xa6, 0, 0, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&second, 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xf4, 0x24, 0x57, 0x89, 0x13, 0x66, 0x08]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&second_positive_low, 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xde, 0x91, 0x22, 0x33, 0x44, 0x55, 0x66]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&second_positive_lower_dict, 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xd1, 0x47, 0x59, 0x45, 0x2d, 0x97, 0x90]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&second_negative_fixed, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xa7, 0x21, 0x45, 0x78, 0x5e, 0x04, 0]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&second_negative, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xf4, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]),
            7
        ))
    );
    assert_eq!(
        decode_tabulated_cylinder_second_coordinate(&second_negative_large, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x0c, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]),
            7
        ))
    );
}

#[test]
fn surface_row_lane_decodes_large_positive_dict_forms() {
    let cache = ScalarCache::default();
    assert_eq!(
        decode_in_surface_row_lane(&[0xd1, 0xf1, 0x60, 0x5a, 0xa4, 0xd9, 0x00], 0, &cache),
        Some((
            f64::from_be_bytes([0x3f, 0xff, 0xf1, 0x60, 0x5a, 0xa4, 0xd9, 0x00]),
            7
        ))
    );
    assert_eq!(
        decode_in_surface_row_lane(&[0xd3, 0x65, 0x1a, 0x84, 0x5c, 0xa9, 0xf0], 0, &cache),
        Some((
            f64::from_be_bytes([0x40, 0x01, 0x65, 0x1a, 0x84, 0x5c, 0xa9, 0xf0]),
            7
        ))
    );
    assert_eq!(
        decode_in_surface_row_lane(&[0xde, 0xee, 0xa1, 0x55, 0x61, 0x88, 0x28], 0, &cache),
        Some((
            f64::from_be_bytes([0x40, 0x10, 0xee, 0xa1, 0x55, 0x61, 0x88, 0x28]),
            7
        ))
    );
    assert_eq!(
        decode_in_surface_row_lane(&[0xdf, 0x19, 0x4c, 0x93, 0x0f, 0x96, 0xe8], 0, &cache),
        Some((
            f64::from_be_bytes([0x40, 0x11, 0x19, 0x4c, 0x93, 0x0f, 0x96, 0xe8]),
            7
        ))
    );
}

#[test]
fn tabulated_cylinder_frame_decodes_positive_4a() {
    let cache = ScalarCache::default();
    assert_eq!(
        decode_tabulated_cylinder_frame_coordinate(
            &[0x4a, 0x13, 0x1f, 0x1c, 0x0b, 0, 0],
            0,
            &cache
        ),
        Some((
            f64::from_be_bytes([0x40, 0x13, 0x1f, 0x1c, 0x0b, 0, 0, 0]),
            7
        ))
    );
}

#[test]
fn tabulated_cylinder_first_frame_uses_the_first_coordinate_sign() {
    let cache = ScalarCache::default();
    let fixed = [0x46, 0x12, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        decode_tabulated_cylinder_first_frame_coordinate(&fixed, 0, &cache),
        Some((f64::from_be_bytes([0xc0, 0x12, 0, 0, 0, 0, 0, 0]), 8))
    );
    assert_eq!(
        decode_tabulated_cylinder_first_frame_coordinate(&[0x4a, 0x13, 0, 0, 0, 0, 0], 0, &cache,),
        Some((f64::from_be_bytes([0x40, 0x13, 0, 0, 0, 0, 0, 0]), 7))
    );
}

#[test]
fn section_cache_uses_unique_raw_tokens_in_first_appearance_order() {
    let first = [0x46, 0x08, 0, 0, 0, 0, 0, 0];
    let second = [0x46, 0x10, 0, 0, 0, 0, 0, 0];
    let mut section = vec![0xaa];
    section.extend_from_slice(&first);
    section.extend_from_slice(&first);
    section.extend_from_slice(&second);
    let cache = ScalarCache::from_section(&section);

    assert_eq!(decode_in_lane(&[0x18, 0], 0, &cache), Some((3.0, 2)));
    assert_eq!(decode_in_lane(&[0x18, 1], 0, &cache), Some((4.0, 2)));
}

#[test]
fn lane_zero_does_not_consume_the_following_scalar_opener() {
    let cache = ScalarCache::default();
    assert_eq!(decode_in_lane(&[0x18, 0xe4], 0, &cache), Some((0.0, 1)));
    assert_eq!(decode_in_lane(&[0x18, 0xe4], 1, &cache), Some((1.0, 2)));
    assert_eq!(decode_in_lane(&[0x18, 0x0d], 0, &cache), Some((0.0, 1)));
    assert_eq!(
        decode_in_lane(&[0x18, 0x67, 0, 0, 0, 0, 0, 0], 0, &cache),
        Some((0.0, 1))
    );
    assert_eq!(
        decode_in_lane(&[0x18, 0x67, 0, 0, 0, 0, 0, 0], 1, &cache),
        Some((f64::from_be_bytes([0x3f, 0, 0, 0, 0, 0, 0, 0]), 8))
    );
    assert_eq!(decode_in_row_lane(&[0x18, 0x0e], 0, &cache), Some((0.0, 1)));
    assert_eq!(decode_in_lane(&[0x18, 0x18, 0], 0, &cache), Some((0.0, 1)));
}

#[test]
fn cache_indices_use_only_the_enclosing_lane_opener_set() {
    let mut section = Vec::new();
    for index in 0..=116_u8 {
        let encoded = if index < 0x46 { index } else { index + 1 };
        section.extend_from_slice(&[0x46, 0x08, encoded, 0, 0, 0, 0, 0]);
    }
    let cache = ScalarCache::from_section(&section);

    assert_eq!(
        decode_in_lane(&[0x18, 0x74], 0, &cache),
        Some((f64::from_be_bytes([0x40, 0x08, 0x75, 0, 0, 0, 0, 0]), 2))
    );
    assert_eq!(
        decode_in_lane(&[0x18, 0x0e], 0, &cache),
        Some((f64::from_be_bytes([0x40, 0x08, 0x0e, 0, 0, 0, 0, 0]), 2))
    );
    assert_eq!(decode_in_row_lane(&[0x18, 0x0e], 0, &cache), Some((0.0, 1)));
}

#[test]
fn lane_zero_does_not_consume_the_following_named_record() {
    let cache = ScalarCache::default();
    assert_eq!(decode_in_lane(&[0x18, 0xe0], 0, &cache), Some((0.0, 1)));
}

#[test]
fn paired_negative_lane_uses_matching_positive_cache_tail() {
    let cache = ScalarCache::from_section(&[0x46, 0x08, 1, 2, 3, 4, 5, 6]);
    let expected = f64::from_be_bytes([0xc0, 0x08, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        decode_in_lane(&[0xa3, 1, 2, 3, 4, 5, 6], 0, &cache),
        Some((expected, 7))
    );
}

#[test]
fn decodes_saved_spline_tangent_dict_forms() {
    let cache = ScalarCache::default();
    let negative = [0xb3, 0, 0, 0, 0, 0, 0];
    let positive = [0x76, 0xb6, 0x7a, 0xe8, 0x58, 0x4c, 0x9a];

    assert_eq!(decode_in_lane(&negative, 0, &cache), Some((-0.5, 7)));
    let (value, end) = decode_in_lane(&positive, 0, &cache).expect("positive tangent");
    assert_eq!(end, 7);
    assert!((value - 3.0_f64.sqrt() / 2.0).abs() < 3e-15);
}

#[test]
fn paired_positive_lane_uses_matching_cache_exponent() {
    let cache = ScalarCache::from_section(&[0x46, 0x13, 1, 2, 3, 4, 5, 6]);
    let expected = f64::from_be_bytes([0x40, 0x13, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
        Some((expected, 7))
    );
    assert_eq!(
        decode_in_lane(&[0x18, 0x9e, 1, 2, 3, 4, 5, 6], 0, &cache),
        Some((0.0, 1))
    );
}

#[test]
fn paired_cache_tail_collision_withholds_value() {
    let cache =
        ScalarCache::from_section(&[0x46, 0x08, 1, 2, 3, 4, 5, 6, 0x46, 0x13, 1, 2, 3, 4, 5, 6]);
    assert_eq!(decode_in_lane(&[0x9e, 1, 2, 3, 4, 5, 6], 0, &cache), None);
    assert_eq!(decode_in_lane(&[0xa3, 1, 2, 3, 4, 5, 6], 0, &cache), None);
}

#[test]
fn row_lane_uses_seven_byte_0x71_without_consuming_the_next_scalar() {
    let cache = ScalarCache::default();
    let data = [0x71, 0xf0, 0, 0, 0, 0, 0, 0xe4];
    assert_eq!(decode_in_row_lane(&data, 0, &cache), Some((1.0, 7)));
    assert_eq!(decode_in_row_lane(&data, 7, &cache), Some((1.0, 8)));
    assert_eq!(
        decode_in_lane(&data, 0, &cache).map(|(_, end)| end),
        Some(8)
    );
}

#[test]
fn pcurve_lane_falls_back_to_positive_dict_after_generic_forms() {
    let cache = ScalarCache::default();
    let positive_dict = [0x98, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        decode_in_pcurve_lane(&positive_dict, 0, &cache),
        Some((f64::from_be_bytes([0x40, 0x0d, 1, 2, 3, 4, 5, 6]), 7))
    );

    let generic = [0x86, 1, 2, 3, 4, 5, 6];
    assert_eq!(
        decode_in_pcurve_lane(&generic, 0, &cache),
        Some((f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 0]), 7))
    );
}

#[test]
fn surface_row_lane_decodes_negative_a0_dict_form() {
    let cache = ScalarCache::default();
    let data = [0xa0, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0x5c, 0xe4];
    assert_eq!(
        decode_in_surface_row_lane(&data, 0, &cache),
        Some((
            f64::from_be_bytes([0xc0, 0x15, 0x5c, 0x28, 0xf5, 0xc2, 0x8f, 0x5c]),
            7
        ))
    );
    assert_eq!(decode_in_surface_row_lane(&data, 7, &cache), Some((1.0, 8)));
}

#[test]
fn surface_row_lane_decodes_negative_a7_dict_form() {
    let cache = ScalarCache::default();
    let data = [0xa7, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0xe4];
    assert_eq!(
        decode_in_surface_row_lane(&data, 0, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xd3, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80]),
            7
        ))
    );
    assert_eq!(decode_in_surface_row_lane(&data, 7, &cache), Some((1.0, 8)));
}

#[test]
fn named_local_system_decodes_negative_5d_dict_form() {
    let cache = ScalarCache::default();
    let data = [0x5d, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2, 0xe4];
    assert_eq!(
        decode_named_local_system_coordinate(&data, 0, 4, &cache),
        Some((
            f64::from_be_bytes([0xbf, 0xd2, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2]),
            7
        ))
    );
    assert_eq!(
        decode_named_local_system_coordinate(&data, 7, 5, &cache),
        Some((1.0, 8))
    );
}

#[test]
fn surface_row_lane_decodes_signed_i48_form() {
    let cache = ScalarCache::default();
    assert_eq!(
        decode_in_surface_row_lane(&[0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe8], 0, &cache),
        Some((-24.0, 7))
    );
    assert_eq!(
        decode_in_surface_row_lane(&[0x92, 0x00, 0x00, 0x00, 0x00, 0x01, 0x23], 0, &cache),
        Some((291.0, 7))
    );
    assert_eq!(
        decode_in_surface_row_lane(&[0xda, 0x00, 0x00, 0x00, 0x00, 0x00, 0x15], 0, &cache),
        Some((21.0, 7))
    );
    assert_eq!(
        decode_in_surface_row_lane(&[0x92, 0xff, 0xff], 0, &cache),
        None
    );
}

#[test]
fn surface_row_zero_does_not_consume_a_surface_only_opener() {
    let cache = ScalarCache::default();

    for opener in [0x73, 0x92, 0xa0, 0xbb, 0xda] {
        assert_eq!(
            decode_in_surface_row_lane(&[0x18, opener, 0, 0, 0, 0, 0, 0], 0, &cache),
            Some((0.0, 1))
        );
    }
}

#[test]
fn positive_dict_lane_decodes_cone_half_angles() {
    let forty_five_degrees = [0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23];
    let eighty_degrees = [0x81, 0x57, 0x18, 0x4a, 0xe7, 0x44, 0x8d];
    let other_angle = [0xb7, 0x5e, 0x8a, 0x1c, 0xf2, 0x17, 0x1e];

    assert_eq!(
        decode_positive_dict(&forty_five_degrees, 0),
        Some((
            f64::from_be_bytes([0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23]),
            7
        ))
    );
    assert_eq!(
        decode_positive_dict(&eighty_degrees, 0),
        Some((
            f64::from_be_bytes([0x3f, 0xf6, 0x57, 0x18, 0x4a, 0xe7, 0x44, 0x8d]),
            7
        ))
    );
    assert_eq!(
        decode_positive_dict(&other_angle, 0),
        Some((
            f64::from_be_bytes([0x3f, 0xe4, 0x5e, 0x8a, 0x1c, 0xf2, 0x17, 0x1e]),
            7
        ))
    );
}

#[test]
fn named_positive_dict_lane_precedes_generic_scalar_forms() {
    let cache = ScalarCache::default();
    let subunit = [0x71, 0, 0, 0, 0, 0, 0];
    let named_dict = [0xa3, 0, 0, 0, 0, 0, 0];

    assert_eq!(
        decode_named_positive_dict_scalar(&subunit, 0, &cache),
        Some((f64::from_be_bytes([0x3f, 0xe6, 0, 0, 0, 0, 0, 0]), 7))
    );
    assert_eq!(
        decode_named_positive_dict_scalar(&named_dict, 0, &cache),
        Some((f64::from_be_bytes([0x40, 0x18, 0, 0, 0, 0, 0, 0]), 7))
    );
}

#[test]
fn row_lane_decodes_negative_half_literal() {
    let cache = ScalarCache::default();

    assert_eq!(
        decode_in_row_lane(&[0x0e, 0x18], 0, &cache),
        Some((-0.5, 1))
    );
}

#[test]
fn a_scalar_body_whose_values_start_past_the_payload_is_refused() {
    let payload = [0x8au8, 0x02, 0x03, 0x10, 0x11, 0x12];

    // One byte past the payload states no bytes at all. The record is
    // refused rather than read as "zero bytes remain".
    assert_eq!(
        admitted_scalar_body(&payload, 2, payload.len() + 1, 1),
        None
    );
    assert_eq!(admitted_scalar_body(&payload, 2, usize::MAX, 1), None);

    // The values start at the end of the payload: no bytes remain, and only
    // a slot count the fixed positional table covers is admitted.
    assert_eq!(
        admitted_scalar_body(&payload, 2, payload.len(), 0),
        Some(&payload[payload.len()..])
    );
    assert_eq!(
        admitted_scalar_body(&payload, 2, payload.len(), POSITIONAL_SLOT_TABLE_WIDTH),
        Some(&payload[payload.len()..])
    );
    assert_eq!(
        admitted_scalar_body(&payload, 2, payload.len(), POSITIONAL_SLOT_TABLE_WIDTH + 1),
        None
    );
}

#[test]
fn a_scalar_body_admits_one_slot_per_remaining_byte() {
    let payload = [0x8au8, 0x02, 0x03, 0x10, 0x11, 0x12];
    let remaining = payload.len() - 3;

    assert_eq!(
        admitted_scalar_body(&payload, 2, 3, remaining),
        Some(&payload[3..])
    );
    // Above the remaining byte count and above the fixed positional table:
    // the record states more slots than its own bytes can carry.
    assert_eq!(
        admitted_scalar_body(&payload, 2, 3, POSITIONAL_SLOT_TABLE_WIDTH + 1),
        None
    );

    // A shape neither compact integer stated is not a scalar body.
    assert_eq!(admitted_scalar_body(&payload, 1, 3, 1), None);
    assert_eq!(admitted_scalar_body(&payload, 2, 2, 1), None);
}

#[test]
fn stateful_plane_matrix_uses_columns_and_refuses_reflection_or_trailing_slots() {
    let body = [
        0x18, 0x10, 0x18, 0x0f, 0x18, 0xe6, 0x0f, 0x2f, 0x10, 0, 0x0e, 0x48, 0, 0,
    ];
    let parsed = crate::decode::with_service_context(|ctx| {
        decode_plane_support_local_system(ctx, &body, &ScalarCache::default(), &mut Vec::new())
    })
    .expect("bounded matrix")
    .expect("complete right-handed matrix");
    assert_eq!(parsed.1, PlaneSupportFrameLayout::RotationColumns);
    assert_eq!(&parsed.0.get()[9..], &[4.0, 0.5, -2.0]);
    assert_eq!(
        super::rotation_plane_axes(parsed.0.as_raw()),
        Some(([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]))
    );
    let mut reflected = body.to_vec();
    reflected[6] = 0x10;
    for invalid in [
        reflected,
        [body.as_slice(), &[0x18]].concat(),
        vec![0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x10, 0x18, 0x18, 0x18],
    ] {
        assert!(crate::decode::with_service_context(|ctx| {
            decode_plane_support_local_system(
                ctx,
                &invalid,
                &ScalarCache::default(),
                &mut Vec::new(),
            )
        })
        .expect("bounded invalid matrix")
        .is_none());
    }
}
