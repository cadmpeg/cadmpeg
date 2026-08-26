// SPDX-License-Identifier: Apache-2.0
//! Unit tests for JT codec primitives.

#![allow(clippy::unwrap_used)]

#[test]
fn jt_int32_cdp2_decodes_empty_and_bitlength_packets() {
    assert_eq!(
        super::decode_int32_cdp2(&[0, 0, 0, 0], 0),
        Some((vec![], 4))
    );

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
        packet.extend_from_slice(&(bits.len() as u32).to_le_bytes());
        packet.extend(code_words);
        packet
    };
    let field = |bits: &mut Vec<u8>, value: u32, width: u8| {
        bits.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
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
        super::decode_int32_cdp2(&packet, 0),
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
        super::decode_int32_cdp2(&packet, 0),
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
        super::decode_int32_cdp2(&packet, 0),
        Some((vec![7, 7, 7], packet.len()))
    );

    packet.truncate(packet.len() - 4);
    assert!(super::decode_int32_cdp2(&packet, 0).is_none());
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

    assert!(super::parse_probability_context(&context).is_none());
}

#[test]
fn jt_int32_cdp2_decodes_unsplit_and_split_chopper_packets() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let low_bits = [2, 0, 0, 0, 1, 17, 0, 0, 0, 0x00, 0x80, 0x12, 0x04];
    let mut unsplit = vec![2, 0, 0, 0, 4, 0];
    unsplit.extend_from_slice(&nested);
    assert_eq!(
        super::decode_int32_cdp2(&unsplit, 0),
        Some((vec![1, -1], unsplit.len()))
    );

    let mut split = vec![2, 0, 0, 0, 4, 2];
    split.extend_from_slice(&10_i32.to_le_bytes());
    split.push(4);
    split.extend_from_slice(&nested);
    split.extend_from_slice(&low_bits);
    assert_eq!(
        super::decode_int32_cdp2(&split, 0),
        Some((vec![15, 7], split.len()))
    );
}

#[test]
fn jt_int32_cdp2_frames_zero_chop_nested_packet() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let mut packet = vec![2, 0, 0, 0, 4, 0];
    packet.extend_from_slice(&nested);
    assert_eq!(
        super::frame_int32_cdp2(&packet, 0),
        Some((2, 4, packet.len()))
    );

    packet[6] = 3;
    assert!(super::frame_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_int32_cdp2_rejects_an_oversized_declared_count_before_allocation() {
    let mut packet = u32::MAX.to_le_bytes().to_vec();
    packet.extend_from_slice(&[1, 0, 0, 0, 0]);
    assert!(super::decode_int32_cdp2(&packet, 0).is_none());
    assert!(super::frame_int32_cdp2(&packet, 0).is_none());
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
    assert!(super::decode_arithmetic(&[], 0, super::MAX_ARITHMETIC_VALUES, &entries,).is_none());
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

    assert!(super::decode_arithmetic(&code_word, 16, 1, &entries).is_none());
}

#[test]
fn jt_predictors_reconstruct_primal_integers() {
    use super::{unpack_predictor_residuals, Predictor};

    let primers = [10, 20, 30, 40];
    let residuals = [10, 20, 30, 40, 5, -2];
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag1),
        [10, 20, 30, 40, 45, 43]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag2),
        [10, 20, 30, 40, 35, 38]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Stride1),
        [10, 20, 30, 40, 55, 68]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Stride2),
        [10, 20, 30, 40, 55, 58]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::StripIndex),
        [10, 20, 30, 40, 37, 40]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Ramp),
        [10, 20, 30, 40, 9, 3]
    );
    assert_eq!(
        unpack_predictor_residuals(&[10, 20, 30, 40, 0x2d ^ 0x28], Predictor::Xor1),
        [10, 20, 30, 40, 45]
    );
    assert_eq!(
        unpack_predictor_residuals(&[10, 20, 30, 40, 0x23 ^ 0x1e], Predictor::Xor2),
        [10, 20, 30, 40, 35]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Null),
        residuals
    );
    assert_eq!(primers, residuals[..4]);
}

#[test]
fn jt_predictors_use_wrapping_i32_arithmetic() {
    use super::{unpack_predictor_residuals, Predictor};

    assert_eq!(
        unpack_predictor_residuals(&[0, 0, 0, i32::MAX, 1], Predictor::Lag1),
        [0, 0, 0, i32::MAX, i32::MIN]
    );
}

#[test]
fn jt_uniform_dequantization_uses_the_full_unsigned_code_range() {
    assert_eq!(
        super::dequantize_uniform(0, [10.0, 20.0], 2),
        Some(8.333_333)
    );
    assert_eq!(
        super::dequantize_uniform(3, [10.0, 20.0], 2),
        Some(18.333_334)
    );
    assert_eq!(super::dequantize_uniform(4, [10.0, 20.0], 2), None);
    assert_eq!(super::dequantize_uniform(-1, [4.0, 4.0], 32), Some(4.0));
}

#[test]
fn jt_quantized_coordinate_array_decodes_three_lag1_code_vectors() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
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
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = Vec::new();
    for _ in 0..3 {
        array.extend_from_slice(&packet);
    }
    array.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    let (points, hash, consumed) =
        super::decode_vertex_coordinates(&array, 4, [[10.0, 20.0]; 3], [2; 3])
            .expect("complete quantized coordinate array");
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, array.len());
    assert_eq!(points[0], [8.333_333; 3]);
    assert_eq!(points[3], [18.333_334; 3]);
}

#[test]
fn jt_deering_normal_applies_sextant_octant_and_code_bounds() {
    let normal = super::deering_normal(1, 7, 8191, 0, 13).unwrap();
    assert!(normal[0].abs() < 1e-3);
    assert!(normal[1].abs() < 1.0e-6);
    assert!((normal[2] - 1.0).abs() < 1.0e-6);
    assert!(super::deering_normal(6, 7, 0, 0, 13).is_none());
    assert!(super::deering_normal(0, 8, 0, 0, 13).is_none());
    assert!(super::deering_normal(0, 7, 8192, 0, 13).is_none());
}

#[test]
fn jt_quantized_texture_coordinates_decode_component_major_lag1_codes() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
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
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
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

    let (values, hash, consumed) = super::decode_vertex_texture_coordinates(&array, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, array.len());
    assert_eq!(values[0], vec![-0.5, -0.5]);
    assert_eq!(values[3], vec![2.5, 2.5]);
}

#[test]
fn jt_quantized_colors_decode_rgb_and_hsv_quantizers() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
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
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
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
    let (colors, hash, consumed) = super::decode_vertex_colors(&rgb, 4, 2).unwrap();
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, rgb.len());
    assert_eq!(colors[0], [-0.5; 4]);
    assert_eq!(colors[3], [2.5; 4]);

    let mut hsv = 4_u32.to_le_bytes().to_vec();
    hsv.extend_from_slice(&[4, 2, 1, 2, 2, 2, 2]);
    for _ in 0..4 {
        hsv.extend_from_slice(&packet);
    }
    hsv.extend_from_slice(&0x8765_4321_u32.to_le_bytes());
    let (colors, hash, consumed) = super::decode_vertex_colors(&hsv, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, hsv.len());
    assert!(colors
        .iter()
        .flatten()
        .all(|component| component.is_finite()));
    assert!((colors[1][0] - 1.0 / 6.0).abs() < 1.0e-6);
    assert!((colors[1][1] - 1.0 / 6.0).abs() < 1.0e-6);
    assert!((colors[1][2] - 5.0 / 36.0).abs() < 1.0e-6);
    assert!((colors[1][3] - 1.0 / 6.0).abs() < 1.0e-6);
}

#[test]
fn jt_vertex_flags_require_a_complete_binary_value_packet() {
    let mut bits = vec![0];
    let mut field = |value: u32, width: u8| {
        bits.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
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
    packet.extend_from_slice(&(bits.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = 3_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&packet);

    assert_eq!(
        super::decode_vertex_flags(&array, 3),
        Some((vec![0, 1, 0], array.len()))
    );
    assert!(super::decode_vertex_flags(&array, 2).is_none());
    let last = array.len() - 1;
    array[last] |= 1;
    assert!(super::decode_vertex_flags(&array, 3).is_none());
}
