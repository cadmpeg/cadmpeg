// SPDX-License-Identifier: Apache-2.0
//! Unit tests for JT codec primitives.

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::scalar::FiniteBinary32;

fn decode_int32_cdp2(bytes: &[u8], depth: u8) -> Option<(Vec<i32>, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| super::decode_int32_cdp2(ctx, bytes, depth).expect("service decode budget"),
    )
}

#[test]
fn jt_int32_cdp2_refuses_counted_vector_at_caller_limit() {
    let packet = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    assert_eq!(
        decode_int32_cdp2(&packet, 0),
        Some((vec![1, -1], packet.len()))
    );

    crate::test_support::with_decode_context_over(
        &packet,
        |policy| {
            policy.limits.max_collection_items = 1;
        },
        |ctx| {
            let error = super::decode_int32_cdp2(ctx, &packet, 0)
                .expect_err("two decoded integers exceed one collection item");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                        && limit.operation == "nx JT decoded vector"
            ));
        },
    );
}

#[test]
fn jt_int32_cdp2_refuses_retained_vector_at_caller_limit() {
    let packet = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];

    crate::test_support::with_decode_context_over(
        &packet,
        |policy| {
            policy.limits.max_retained_bytes =
                cadmpeg_core::decode::u64_from_index(2 * std::mem::size_of::<i32>()) - 1;
        },
        |ctx| {
            let error = super::decode_int32_cdp2(ctx, &packet, 0)
                .expect_err("two decoded integers exceed retained storage");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        && limit.operation == "nx JT decoded vector"
            ));
        },
    );
}

#[test]
fn jt_int32_cdp2_refuses_nesting_at_caller_limit() {
    let packet = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];

    crate::test_support::with_decode_context_over(
        &packet,
        |policy| {
            policy.limits.max_recursion_depth = 0;
        },
        |ctx| {
            let error = super::decode_int32_cdp2(ctx, &packet, 0)
                .expect_err("packet nesting exceeds zero levels");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
                        && limit.operation == "decode JT integer packet"
            ));
        },
    );
}

#[test]
fn jt_int32_cdp2_refuses_symbol_work_at_caller_limit() {
    let packet = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];

    let error = crate::test_support::resource_refusal_at(
        &packet,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "decode JT bitlength symbols",
        |ctx| super::decode_int32_cdp2(ctx, &packet, 0),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "decode JT bitlength symbols"
                && limit.additional == 2
    ));
}

fn frame_int32_cdp2(bytes: &[u8], depth: u8) -> Option<(u32, u8, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| super::frame_int32_cdp2(ctx, bytes, depth).expect("service decode budget"),
    )
}

fn decode_vertex_coordinates(
    bytes: &[u8],
    count: usize,
    ranges: [super::QuantizedRange; 3],
    bits: [u8; 3],
) -> Option<(Vec<[FiniteBinary32; 3]>, u32, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            super::decode_vertex_coordinates(ctx, bytes, count, ranges, bits)
                .expect("service decode budget")
                .map(|array| (array.values, array.hash, array.byte_len))
        },
    )
}

fn decode_vertex_texture_coordinates(
    bytes: &[u8],
    count: usize,
    bits: u8,
) -> Option<(Vec<Vec<FiniteBinary32>>, u32, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            super::decode_vertex_texture_coordinates(ctx, bytes, count, bits)
                .expect("service decode budget")
                .map(|array| (array.values, array.hash, array.byte_len))
        },
    )
}

fn decode_vertex_colors(
    bytes: &[u8],
    count: usize,
    bits: u8,
) -> Option<(Vec<[FiniteBinary32; 4]>, u32, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            super::decode_vertex_colors(ctx, bytes, count, bits)
                .expect("service decode budget")
                .map(|array| (array.values, array.hash, array.byte_len))
        },
    )
}

fn decode_vertex_flags(bytes: &[u8], count: usize) -> Option<(Vec<u32>, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| super::decode_vertex_flags(ctx, bytes, count).expect("service decode budget"),
    )
}

fn parse_probability_context(bytes: &[u8]) -> Option<(Vec<super::ProbabilityEntry>, usize)> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            super::parse_probability_context(ctx, bytes)
                .expect("service decode budget")
                .map(|(entries, length, _reservation)| (entries, length))
        },
    )
}

fn decode_arithmetic(
    bytes: &[u8],
    bits: usize,
    count: usize,
    entries: &[super::ProbabilityEntry],
) -> Option<Vec<Option<i32>>> {
    crate::test_support::with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            super::decode_arithmetic(ctx, bytes, bits, count, entries)
                .expect("service decode budget")
                .map(|symbols| symbols.values)
        },
    )
}

fn unpack_predictor_residuals(residuals: &[i32], predictor: super::Predictor) -> Vec<i32> {
    crate::test_support::with_decode_context_over(
        &[],
        |_| {},
        |ctx| {
            super::unpack_predictor_residuals(ctx, residuals, predictor)
                .expect("service predictor budget")
        },
    )
}

const EPS_JT_NORMAL_RECONSTRUCTION: f32 = 1.0e-6;
const EPS_JT_HSV_RECONSTRUCTION: f32 = 1.0e-6;

fn range(minimum: f32, maximum: f32) -> super::QuantizedRange {
    super::QuantizedRange::new(minimum, maximum).expect("test quantization range is ordered")
}

#[test]
fn jt_int32_cdp2_decodes_empty_and_bitlength_packets() {
    assert_eq!(decode_int32_cdp2(&[0, 0, 0, 0], 0), Some((vec![], 4)));

    let encode_packet = |bits: &[u8], value_count: u32| {
        let mut code_words = Vec::new();
        for chunk in bits.chunks(32) {
            let mut word = 0u32;
            for bit in chunk {
                word = (word << 1) | u32::from(*bit);
            }
            word <<= 32 - chunk.len();
            code_words.extend_from_slice(&word.to_le_bytes());
        }
        let mut packet = value_count.to_le_bytes().to_vec();
        packet.push(1);
        packet.extend_from_slice(
            &(u32::try_from(bits.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        packet.extend(code_words);
        packet
    };
    let field = |bits: &mut Vec<u8>, value: u32, width: u8| {
        bits.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };

    // Fixed-width mode: range [-1, 1], followed by codes for 1 and -1.
    let mut bits = vec![0];
    field(&mut bits, 2, 6);
    field(&mut bits, 2, 6);
    field(&mut bits, 0b11, 2);
    field(&mut bits, 0b01, 2);
    field(&mut bits, 2, 2);
    field(&mut bits, 0, 2);
    let packet = encode_packet(&bits, 2);
    assert_eq!(
        decode_int32_cdp2(&packet, 0),
        Some((vec![1, -1], packet.len()))
    );

    // Variable-width mode: mean 10, one two-bit run containing +1 and -1.
    let mut bits = vec![1];
    field(&mut bits, 10, 32);
    field(&mut bits, 3, 3);
    field(&mut bits, 3, 3);
    field(&mut bits, 2, 3);
    field(&mut bits, 2, 3);
    field(&mut bits, 1, 2);
    field(&mut bits, 3, 2);
    let packet = encode_packet(&bits, 2);
    assert_eq!(
        decode_int32_cdp2(&packet, 0),
        Some((vec![11, 9], packet.len()))
    );
}

#[test]
fn jt_int32_cdp2_decodes_arithmetic_context_with_zero_frequency_entry() {
    let mut context_bits = Vec::<bool>::new();
    let mut push = |value: u32, width: u8| {
        for shift in (0..width).rev() {
            context_bits.push((value >> shift) & 1 != 0);
        }
    };
    push(2, 6);
    push(1, 6);
    push(1, 6);
    push(7, 32);
    push(0, 2);
    push(0, 1);
    push(0, 1);
    push(1, 2);
    push(1, 1);
    push(0, 1);
    let mut context = vec![0, 2];
    for chunk in context_bits.chunks(8) {
        let mut byte = 0u8;
        for bit in chunk {
            byte = (byte << 1) | u8::from(*bit);
        }
        byte <<= 8 - chunk.len();
        context.push(byte);
    }
    let mut packet = Vec::new();
    packet.extend_from_slice(&3_u32.to_le_bytes());
    packet.push(3);
    packet.extend_from_slice(&16_u32.to_le_bytes());
    packet.extend_from_slice(&0_u32.to_le_bytes());
    packet.extend_from_slice(&context);
    packet.extend_from_slice(&0_u32.to_le_bytes());
    assert_eq!(
        decode_int32_cdp2(&packet, 0),
        Some((vec![7, 7, 7], packet.len()))
    );

    packet.truncate(packet.len() - 4);
    assert!(decode_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_int32_cdp2_refuses_scoped_probability_table_at_caller_limit() {
    let mut context_bits = Vec::<bool>::new();
    let mut push = |value: u32, width: u8| {
        for shift in (0..width).rev() {
            context_bits.push((value >> shift) & 1 != 0);
        }
    };
    push(2, 6);
    push(1, 6);
    push(1, 6);
    push(7, 32);
    push(0, 2);
    push(0, 1);
    push(0, 1);
    push(1, 2);
    push(1, 1);
    push(0, 1);
    let mut context = vec![0, 2];
    for chunk in context_bits.chunks(8) {
        let mut byte = 0u8;
        for bit in chunk {
            byte = (byte << 1) | u8::from(*bit);
        }
        byte <<= 8 - chunk.len();
        context.push(byte);
    }
    let mut packet = Vec::new();
    packet.extend_from_slice(&3_u32.to_le_bytes());
    packet.push(3);
    packet.extend_from_slice(&16_u32.to_le_bytes());
    packet.extend_from_slice(&0_u32.to_le_bytes());
    packet.extend_from_slice(&context);
    packet.extend_from_slice(&0_u32.to_le_bytes());

    crate::test_support::with_decode_context_over(
        &packet,
        |policy| {
            policy.limits.max_materialized_bytes = 0;
        },
        |ctx| {
            let error = super::decode_int32_cdp2(ctx, &packet, 0)
                .expect_err("probability table needs scoped storage");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                        && limit.operation == "nx JT decoded vector"
            ));
        },
    );
}

#[test]
fn jt_arithmetic_context_rejects_count_without_serialized_entry_span() {
    let mut context_bits = Vec::<bool>::new();
    let mut push = |value: u32, width: u8| {
        for shift in (0..width).rev() {
            context_bits.push((value >> shift) & 1 != 0);
        }
    };
    push(0, 6);
    push(1, 6);
    push(0, 6);
    push(0, 32);

    let mut context = vec![0xff, 0xff];
    for chunk in context_bits.chunks(8) {
        let mut byte = 0u8;
        for bit in chunk {
            byte = (byte << 1) | u8::from(*bit);
        }
        byte <<= 8 - chunk.len();
        context.push(byte);
    }

    assert!(parse_probability_context(&context).is_none());
}

#[test]
fn jt_int32_cdp2_decodes_unsplit_and_split_chopper_packets() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let low_bits = [2, 0, 0, 0, 1, 17, 0, 0, 0, 0x00, 0x80, 0x12, 0x04];
    let mut unsplit = vec![2, 0, 0, 0, 4, 0];
    unsplit.extend_from_slice(&nested);
    assert_eq!(
        decode_int32_cdp2(&unsplit, 0),
        Some((vec![1, -1], unsplit.len()))
    );

    let mut split = vec![2, 0, 0, 0, 4, 2];
    split.extend_from_slice(&10_i32.to_le_bytes());
    split.push(4);
    split.extend_from_slice(&nested);
    split.extend_from_slice(&low_bits);
    assert_eq!(
        decode_int32_cdp2(&split, 0),
        Some((vec![15, 7], split.len()))
    );
}

#[test]
fn jt_int32_cdp2_frames_zero_chop_nested_packet() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let mut packet = vec![2, 0, 0, 0, 4, 0];
    packet.extend_from_slice(&nested);
    assert_eq!(frame_int32_cdp2(&packet, 0), Some((2, 4, packet.len())));

    packet[6] = 3;
    assert!(frame_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_int32_cdp2_rejects_an_oversized_declared_count_before_allocation() {
    let mut packet = u32::MAX.to_le_bytes().to_vec();
    packet.extend_from_slice(&[1, 0, 0, 0, 0]);
    assert!(decode_int32_cdp2(&packet, 0).is_none());
    assert!(frame_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_arithmetic_decode_bounds_table_lookup_work() {
    let entries = vec![
        super::ProbabilityEntry {
            symbol: 0,
            occurrence_count: 1,
            value: 0,
        };
        65
    ];
    crate::test_support::with_decode_context_over(
        &[],
        |_| {},
        |ctx| {
            let error =
                super::decode_arithmetic(ctx, &[], 0, super::MAX_ARITHMETIC_VALUES, &entries)
                    .err()
                    .expect("local work limit");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "decode JT arithmetic symbols")
            );
        },
    );
}

#[test]
fn jt_arithmetic_decode_rejects_normalization_past_declared_bits() {
    let entries = vec![
        super::ProbabilityEntry {
            symbol: 0,
            occurrence_count: 1,
            value: 0,
        },
        super::ProbabilityEntry {
            symbol: 1,
            occurrence_count: 1,
            value: 1,
        },
        super::ProbabilityEntry {
            symbol: 2,
            occurrence_count: 1,
            value: 2,
        },
    ];
    let code_word = 0x5555_0000_u32.to_le_bytes();

    assert!(decode_arithmetic(&code_word, 16, 1, &entries).is_none());
}

#[test]
fn jt_predictors_reconstruct_primal_integers() {
    use super::Predictor;

    let primers = [10, 20, 30, 40];
    let residuals = [10, 20, 30, 40, 5, -2];
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag1),
        [10, 20, 30, 40, 45, 43]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Null),
        residuals
    );
    assert_eq!(primers, residuals[..4]);
}

#[test]
fn jt_predictors_use_wrapping_i32_arithmetic() {
    use super::Predictor;

    assert_eq!(
        unpack_predictor_residuals(&[0, 0, 0, i32::MAX, 1], Predictor::Lag1),
        [0, 0, 0, i32::MAX, i32::MIN]
    );
}

#[test]
fn jt_uniform_dequantization_uses_the_full_unsigned_code_range() {
    assert_eq!(
        super::dequantize_uniform(0, range(10.0, 20.0), 2).map(FiniteBinary32::get),
        Some(8.333_333)
    );
    assert_eq!(
        super::dequantize_uniform(3, range(10.0, 20.0), 2).map(FiniteBinary32::get),
        Some(18.333_334)
    );
    assert_eq!(super::dequantize_uniform(4, range(10.0, 20.0), 2), None);
    assert_eq!(
        super::dequantize_uniform(u32::MAX, range(4.0, 4.0), 32).map(FiniteBinary32::get),
        Some(4.0)
    );
}

#[test]
fn jt_quantized_coordinates_reject_negative_codes_at_thirty_two_bits() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };
    push(0, 1);
    push(6, 6);
    push(6, 6);
    push(0b11_1111, 6);
    push(0, 6);
    for _ in 0..4 {
        push(0, 1);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(
        &(u32::try_from(code.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = Vec::new();
    for _ in 0..3 {
        array.extend_from_slice(&packet);
    }
    array.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    assert!(decode_vertex_coordinates(&array, 4, [range(4.0, 4.0); 3], [32; 3]).is_none());
}

#[test]
fn jt_hsv_colors_with_a_wrapped_hue_stay_in_the_sextant_table() {
    let color = super::hsv_to_rgb(
        FiniteBinary32::new(-1.0e-45).unwrap(),
        FiniteBinary32::ONE,
        FiniteBinary32::ONE,
    )
    .expect("finite hsv color")
    .map(FiniteBinary32::get);
    assert!(color.iter().all(|value| value.is_finite()));
    assert!((color[0] - 1.0).abs() < EPS_JT_HSV_RECONSTRUCTION);
    assert!(color[1].abs() < EPS_JT_HSV_RECONSTRUCTION);
    assert!(color[2].abs() < EPS_JT_HSV_RECONSTRUCTION);
}

#[test]
fn jt_quantized_coordinate_array_decodes_three_lag1_code_vectors() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(
        &(u32::try_from(code.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = Vec::new();
    for _ in 0..3 {
        array.extend_from_slice(&packet);
    }
    array.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    let (points, hash, consumed) =
        decode_vertex_coordinates(&array, 4, [range(10.0, 20.0); 3], [2; 3])
            .expect("complete quantized coordinate array");
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, array.len());
    assert_eq!(points[0].map(FiniteBinary32::get), [8.333_333; 3]);
    assert_eq!(points[3].map(FiniteBinary32::get), [18.333_334; 3]);
}

#[test]
fn jt_coordinate_array_refuses_scoped_component_storage() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(
        &(u32::try_from(code.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = Vec::new();
    for _ in 0..3 {
        array.extend_from_slice(&packet);
    }
    array.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    crate::test_support::with_decode_context_over(
        &array,
        |policy| {
            policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
                3 * std::mem::size_of::<Vec<FiniteBinary32>>(),
            ) - 1;
        },
        |ctx| {
            let error =
                super::decode_vertex_coordinates(ctx, &array, 4, [range(10.0, 20.0); 3], [2; 3])
                    .err()
                    .expect("the component vector exceeds scoped storage");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                        && limit.operation == "nx JT decoded vector"
            ));
        },
    );
}

#[test]
fn jt_deering_normal_applies_sextant_octant_and_code_bounds() {
    let bits = super::NormalBits::new(13).expect("thirteen-bit codes");
    let normal = super::deering_normal(
        super::Sextant::from_index(1).expect("sextant one"),
        super::Octant::new(7).expect("octant seven"),
        super::NormalCode::new(8191, bits).expect("theta code"),
        super::NormalCode::new(0, bits).expect("psi code"),
    )
    .unwrap();
    assert!(normal[0].get().abs() < 1e-3);
    assert!(normal[1].get().abs() < EPS_JT_NORMAL_RECONSTRUCTION);
    assert!((normal[2].get() - 1.0).abs() < EPS_JT_NORMAL_RECONSTRUCTION);
    let coarse = super::NormalBits::new(6).expect("six-bit codes");
    let odd_sextant = super::deering_normal(
        super::Sextant::from_index(1).expect("sextant one"),
        super::Octant::new(7).expect("octant seven"),
        super::NormalCode::new(3, coarse).expect("theta code"),
        super::NormalCode::new(0, coarse).expect("psi code"),
    )
    .expect("finite normal");
    assert_eq!(
        odd_sextant.map(FiniteBinary32::get),
        [0.650_906_5, 0.0, 0.759_157_9]
    );
    assert!(super::Sextant::from_index(6).is_none());
    assert!(super::Sextant::from_index(-1).is_none());
    assert!(super::Octant::new(8).is_none());
    assert!(super::Octant::new(-1).is_none());
    assert!(super::NormalCode::new(8192, bits).is_none());
    assert!(super::NormalCode::new(-1, bits).is_none());
    assert!(super::NormalBits::new(0).is_none());
    assert!(super::NormalBits::new(14).is_none());
}

#[test]
fn jt_quantized_texture_coordinates_decode_component_major_lag1_codes() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(
        &(u32::try_from(code.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    packet.extend_from_slice(&word.to_le_bytes());

    let mut array = 4_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&[2, 2]);
    for _ in 0..2 {
        array.extend_from_slice(&0_f32.to_le_bytes());
        array.extend_from_slice(&3_f32.to_le_bytes());
        array.push(2);
    }
    array.extend_from_slice(&packet);
    array.extend_from_slice(&packet);
    array.extend_from_slice(&0x8765_4321_u32.to_le_bytes());

    let (values, hash, consumed) = decode_vertex_texture_coordinates(&array, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, array.len());
    assert_eq!(
        values[0]
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        vec![-0.5, -0.5]
    );
    assert_eq!(
        values[3]
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        vec![2.5, 2.5]
    );
}

#[test]
fn jt_quantized_colors_decode_rgb_and_hsv_quantizers() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(
        &(u32::try_from(code.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    packet.extend_from_slice(&word.to_le_bytes());

    let mut rgb = 4_u32.to_le_bytes().to_vec();
    rgb.extend_from_slice(&[3, 2, 0]);
    for _ in 0..4 {
        rgb.extend_from_slice(&0_f32.to_le_bytes());
        rgb.extend_from_slice(&3_f32.to_le_bytes());
        rgb.push(2);
    }
    for _ in 0..4 {
        rgb.extend_from_slice(&packet);
    }
    rgb.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    let (colors, hash, consumed) = decode_vertex_colors(&rgb, 4, 2).unwrap();
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, rgb.len());
    assert_eq!(colors[0].map(FiniteBinary32::get), [-0.5; 4]);
    assert_eq!(colors[3].map(FiniteBinary32::get), [2.5; 4]);

    let mut hsv = 4_u32.to_le_bytes().to_vec();
    hsv.extend_from_slice(&[4, 2, 1, 2, 2, 2, 2]);
    for _ in 0..4 {
        hsv.extend_from_slice(&packet);
    }
    hsv.extend_from_slice(&0x8765_4321_u32.to_le_bytes());
    let (colors, hash, consumed) = decode_vertex_colors(&hsv, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, hsv.len());
    assert!(colors
        .iter()
        .flatten()
        .all(|component| component.get().is_finite()));
    assert!((colors[1][0].get() - 1.0 / 6.0).abs() < EPS_JT_HSV_RECONSTRUCTION);
    assert!((colors[1][1].get() - 1.0 / 6.0).abs() < EPS_JT_HSV_RECONSTRUCTION);
    assert!((colors[1][2].get() - 5.0 / 36.0).abs() < EPS_JT_HSV_RECONSTRUCTION);
    assert!((colors[1][3].get() - 1.0 / 6.0).abs() < EPS_JT_HSV_RECONSTRUCTION);
}

#[test]
fn jt_vertex_flags_require_a_complete_binary_value_packet() {
    let mut bits = vec![0];
    let mut field = |value: u32, width: u8| {
        bits.extend(
            (0..width)
                .rev()
                .map(|shift| u8::try_from((value >> shift) & 1).expect("fixture value fits u8")),
        );
    };
    field(1, 6);
    field(2, 6);
    field(0, 1);
    field(1, 2);
    field(0, 1);
    field(1, 1);
    field(0, 1);
    let mut word = 0u32;
    for bit in &bits {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - bits.len();
    let mut packet = 3_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(
        &(u32::try_from(bits.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = 3_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&packet);

    assert_eq!(
        decode_vertex_flags(&array, 3),
        Some((vec![0, 1, 0], array.len()))
    );
    assert!(decode_vertex_flags(&array, 2).is_none());
    let last = array.len() - 1;
    array[last] |= 1;
    assert!(decode_vertex_flags(&array, 3).is_none());
}

#[test]
fn dequantization_refuses_value_below_binary32_range_before_rounding() {
    let range = super::QuantizedRange::new(-f32::MAX, (-f32::MAX).next_up())
        .expect("finite ordered quantization range");
    assert!(super::dequantize_uniform(0, range, 2).is_none());
}

#[test]
fn jt_probability_context_biases_unsigned_symbols_before_range_admission() {
    for (raw, expected) in [
        (0x8000_0000_u32, Some(i32::MAX - 1)),
        (0x8000_0001, Some(i32::MAX)),
        (0x8000_0002, None),
        (u32::MAX, None),
    ] {
        let (context, packet) = single_symbol_probability_packet(raw);
        let parsed = parse_probability_context(&context);
        assert_eq!(
            parsed.as_ref().map(|(entries, _)| entries[0].symbol),
            expected
        );
        assert_eq!(
            decode_int32_cdp2(&packet, 0),
            expected.map(|_| (vec![0], packet.len()))
        );
        assert_eq!(
            frame_int32_cdp2(&packet, 0),
            expected.map(|_| (1, 3, packet.len()))
        );
    }
}

#[test]
fn jt_integer_scratch_requires_retained_commit() {
    let packet = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    crate::test_support::with_decode_context_over(
        &packet,
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let (lane, length) = super::decode_int32_cdp2_inner(ctx, &packet, 0)
                .unwrap()
                .unwrap();
            assert_eq!(&*lane, &[1, -1]);
            assert_eq!(length, packet.len());
            assert!(
                matches!(lane.into_retained(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
            );
        },
    );
}

#[test]
fn jt_lossless_component_uses_scoped_storage() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let lane = super::lossless_coordinate_component(ctx, &[0, 0], &[0, 0])
                .unwrap()
                .unwrap();
            assert_eq!(lane.len(), 2);
            assert!(lane.iter().all(|value| value.get() == 0.0));
        },
    );
}

#[test]
fn jt_arithmetic_local_work_refusal_is_structured() {
    let entries = vec![
        super::ProbabilityEntry {
            symbol: 0,
            occurrence_count: 1,
            value: 0
        };
        100
    ];
    crate::test_support::with_decode_context_over(
        &[],
        |_| {},
        |ctx| {
            let error = super::decode_arithmetic(ctx, &[], 0, 700_000, &entries)
                .err()
                .unwrap();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == "decode JT arithmetic symbols" && limit.limit == 64_000_000)
            );
        },
    );
}

#[test]
fn jt_component_owner_keeps_scratch_reservation_live() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_materialized_bytes = 8,
        |ctx| {
            let component = super::lossless_coordinate_component(ctx, &[0, 0], &[0, 0])
                .unwrap()
                .unwrap();
            assert_eq!(component.len(), 2);
            assert!(ctx
                .reserve_scoped(1, "overlapping JT component copy")
                .is_err());
            assert_eq!(component[0].get(), 0.0);
        },
    );
}

fn single_symbol_probability_packet(raw: u32) -> (Vec<u8>, Vec<u8>) {
    let mut bits = Vec::new();
    for (value, width) in [(32_u32, 6), (1, 6), (0, 6), (0, 32), (raw, 32), (1, 1)] {
        bits.extend((0..width).rev().map(|shift| (value >> shift) & 1 != 0));
    }
    let mut context = vec![0, 1];
    for chunk in bits.chunks(8) {
        let mut byte = 0_u8;
        for bit in chunk {
            byte = (byte << 1) | u8::from(*bit);
        }
        context.push(byte << (8 - chunk.len()));
    }
    let mut packet = vec![1, 0, 0, 0, 3, 16, 0, 0, 0, 0, 0, 0, 0];
    packet.extend_from_slice(&context);
    packet.extend([0; 4]);
    (context, packet)
}

#[test]
fn jt_arithmetic_output_copy_refuses_work_before_formation() {
    let (_, packet) = single_symbol_probability_packet(2);
    let error = crate::test_support::resource_refusal_at(
        &packet,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "form JT arithmetic values",
        |ctx| super::decode_int32_cdp2(ctx, &packet, 0),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits && limit.operation == "form JT arithmetic values")
    );
}

#[test]
fn jt_variable_bitlength_delta_cycles_refuse_code_work() {
    let mut bits = vec![1_u8];
    bits.extend([0; 32]);
    bits.extend([0, 1, 0, 0, 0, 1]); // delta width 2, run width 1
    for _ in 0..128 {
        bits.extend([0, 1, 0, 1, 1, 0]); // +1, +1, -2
    }
    bits.extend([0, 0, 1]); // zero delta, run one
    let bit_len = u32::try_from(bits.len()).unwrap();
    let mut packet = 1_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in bits.chunks(32) {
        let mut word = 0_u32;
        for bit in chunk {
            word = (word << 1) | u32::from(*bit);
        }
        word <<= 32 - chunk.len();
        packet.extend_from_slice(&word.to_le_bytes());
    }
    crate::test_support::with_decode_context_over(
        &packet,
        // The final two-bit delta follows the header and all 128 six-bit cycles.
        |policy| policy.limits.max_work_units = u64::from(bit_len) - 2,
        |ctx| {
            let error = super::decode_int32_cdp2(ctx, &packet, 0).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == "decode JT bit field"
                    && limit.additional == 2)
            );
        },
    );
    assert_eq!(decode_int32_cdp2(&packet, 0), Some((vec![0], packet.len())));
}

#[test]
fn msb_bit_range_refusal_precedes_read() {
    let bytes = [0xa5];
    crate::test_support::with_decode_context_over(&bytes, |policy| policy.limits.max_work_units = 3, |ctx| {
        let mut bits = super::MsbBitReader::new(&bytes);
        let error = bits.read(ctx, 4).unwrap_err();
        assert!(matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "decode JT bit field" && limit.additional == 4));
        assert_eq!(bits.bit, 0);
        assert_eq!(ctx.resource_refusal(), match error { cadmpeg_core::CodecError::ResourceLimit(limit) => Some(limit), _ => unreachable!() });
    });
}

#[test]
fn code_bit_range_refusal_precedes_read() {
    let bytes = [0, 0, 0, 0];
    crate::test_support::with_decode_context_over(&bytes, |policy| policy.limits.max_work_units = 3, |ctx| {
        let mut bits = super::CodeBits { words: &bytes, bit_len: 32, bit: 0 };
        let error = bits.read(ctx, 4).unwrap_err();
        assert!(matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "decode JT bit field" && limit.additional == 4));
        assert_eq!(bits.bit, 0);
        assert_eq!(ctx.resource_refusal(), match error { cadmpeg_core::CodecError::ResourceLimit(limit) => Some(limit), _ => unreachable!() });
    });
}

#[test]
fn signed_code_bit_range_refusal_propagates() {
    let bytes = [0xff; 4];
    crate::test_support::with_decode_context_over(&bytes, |policy| policy.limits.max_work_units = 3, |ctx| {
        let mut bits = super::CodeBits { words: &bytes, bit_len: 32, bit: 0 };
        let error = bits.read_signed(ctx, 4).unwrap_err();
        assert!(matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "decode JT bit field" && limit.additional == 4));
        assert_eq!(bits.bit, 0);
    });
}
