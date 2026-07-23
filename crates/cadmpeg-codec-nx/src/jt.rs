// SPDX-License-Identifier: Apache-2.0
//! Siemens JT integer packet decoding used by embedded NX display models.

#[derive(Debug, Clone, Copy)]
struct ProbabilityEntry {
    symbol: i32,
    occurrence_count: u32,
    value: i32,
}

struct MsbBitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> MsbBitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn read(&mut self, count: u8) -> Option<u32> {
        if count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.bytes.get(self.bit / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - self.bit % 8)) & 1);
            self.bit += 1;
        }
        Some(value)
    }

    fn finish_zero_padding(self) -> Option<usize> {
        let byte_len = self.bit.div_ceil(8);
        if !self.bit.is_multiple_of(8) {
            let used = self.bit % 8;
            let last = *self.bytes.get(byte_len - 1)?;
            if last & ((1 << (8 - used)) - 1) != 0 {
                return None;
            }
        }
        Some(byte_len)
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset.checked_add(4)?)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Predictor {
    Lag1,
    Lag2,
    Stride1,
    Stride2,
    StripIndex,
    Ramp,
    Xor1,
    Xor2,
    Null,
}

/// Reconstruct JT primal integers from predictor residuals.
pub(crate) fn unpack_predictor_residuals(residuals: &[i32], predictor: Predictor) -> Vec<i32> {
    if predictor == Predictor::Null {
        return residuals.to_vec();
    }

    let mut values = Vec::with_capacity(residuals.len());
    for (index, &residual) in residuals.iter().enumerate() {
        if index < 4 {
            values.push(residual);
            continue;
        }
        let v1 = values[index - 1];
        let v2 = values[index - 2];
        let v4 = values[index - 4];
        let predicted = match predictor {
            Predictor::Lag1 | Predictor::Xor1 => v1,
            Predictor::Lag2 | Predictor::Xor2 => v2,
            Predictor::Stride1 => v1.wrapping_add(v1.wrapping_sub(v2)),
            Predictor::Stride2 => v2.wrapping_add(v2.wrapping_sub(v4)),
            Predictor::StripIndex => {
                let stride = v2.wrapping_sub(v4);
                if (-7..=7).contains(&stride) {
                    v2.wrapping_add(stride)
                } else {
                    v2.wrapping_add(2)
                }
            }
            Predictor::Ramp => index as i32,
            Predictor::Null => unreachable!(),
        };
        values.push(if matches!(predictor, Predictor::Xor1 | Predictor::Xor2) {
            residual ^ predicted
        } else {
            residual.wrapping_add(predicted)
        });
    }
    values
}

fn lossless_coordinate_component(exponents: &[i32], mantissae: &[i32]) -> Option<Vec<f32>> {
    if exponents.len() != mantissae.len() {
        return None;
    }
    exponents
        .iter()
        .zip(mantissae)
        .map(|(&exponent, &mantissa)| {
            let exponent = exponent as u32 & 0x1ff;
            let mantissa = mantissa as u32 & 0x7f_ffff;
            let value = f32::from_bits((exponent << 23) | mantissa);
            value.is_finite().then_some(value)
        })
        .collect()
}

pub(crate) fn deering_normal(
    sextant: i32,
    octant: i32,
    theta: i32,
    psi: i32,
    bits: u8,
) -> Option<[f32; 3]> {
    if bits == 0 || bits > 13 {
        return None;
    }
    let sextant = u32::try_from(sextant).ok().filter(|value| *value < 6)?;
    let octant = u32::try_from(octant).ok().filter(|value| *value < 8)?;
    let code_limit = 1_u32 << bits;
    let theta = u32::try_from(theta)
        .ok()
        .filter(|value| *value < code_limit)?;
    let psi = u32::try_from(psi)
        .ok()
        .filter(|value| *value < code_limit)?;
    let shift = 13 - bits;
    let theta_index = (theta + (sextant & 1)) << shift;
    let psi_index = psi << shift;
    let table_size = f64::from(1_u32 << 13);
    let maximum_psi = 0.615_479_709_f64;
    let theta_angle = (maximum_psi * (table_size - f64::from(theta_index)) / table_size)
        .tan()
        .asin();
    let psi_angle = maximum_psi * f64::from(psi_index) / table_size;
    let x = (psi_angle.cos() * theta_angle.cos()) as f32;
    let y = psi_angle.sin() as f32;
    let z = (psi_angle.cos() * theta_angle.sin()) as f32;
    let mut result = match sextant {
        0 => [x, y, z],
        1 => [z, y, x],
        2 => [y, z, x],
        3 => [y, x, z],
        4 => [z, x, y],
        5 => [x, z, y],
        _ => unreachable!(),
    };
    for (component, bit) in [4, 2, 1].into_iter().enumerate() {
        if octant & bit == 0 {
            result[component] = -result[component];
        }
    }
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

/// Decode one JT compressed normal array and its trailing hash.
pub(crate) fn decode_vertex_normals(
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Option<(Vec<[f32; 3]>, u32, usize)> {
    let count = usize::try_from(read_u32(bytes, 0)?).ok()?;
    if count != expected_count || *bytes.get(4)? != 3 || *bytes.get(5)? != expected_bits {
        return None;
    }
    let mut cursor = 6usize;
    let normals = if expected_bits == 0 {
        let mut components = Vec::with_capacity(3);
        for _ in 0..3 {
            let (exponents, exponent_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(exponent_len)?;
            let (mantissae, mantissa_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(mantissa_len)?;
            if exponents.len() != count || mantissae.len() != count {
                return None;
            }
            components.push(lossless_coordinate_component(&exponents, &mantissae)?);
        }
        (0..count)
            .map(|index| {
                [
                    components[0][index],
                    components[1][index],
                    components[2][index],
                ]
            })
            .collect::<Vec<_>>()
    } else {
        let mut codes = Vec::with_capacity(4);
        for _ in 0..4 {
            let (values, byte_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(byte_len)?;
            if values.len() != count {
                return None;
            }
            codes.push(values);
        }
        (0..count)
            .map(|index| {
                deering_normal(
                    codes[0][index],
                    codes[1][index],
                    codes[2][index],
                    codes[3][index],
                    expected_bits,
                )
            })
            .collect::<Option<Vec<_>>>()?
    };
    let hash = read_u32(bytes, cursor)?;
    cursor = cursor.checked_add(4)?;
    Some((normals, hash, cursor))
}

/// Decode one JT compressed texture-coordinate array and its trailing hash.
pub(crate) fn decode_vertex_texture_coordinates(
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Option<(Vec<Vec<f32>>, u32, usize)> {
    let count = usize::try_from(read_u32(bytes, 0)?).ok()?;
    let component_count = usize::from(*bytes.get(4)?);
    if count != expected_count
        || !(1..=4).contains(&component_count)
        || *bytes.get(5)? != expected_bits
        || expected_bits > 24
    {
        return None;
    }
    let mut cursor = 6usize;
    let mut components = Vec::with_capacity(component_count);
    if expected_bits == 0 {
        for _ in 0..component_count {
            let (exponents, exponent_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(exponent_len)?;
            let (mantissae, mantissa_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(mantissa_len)?;
            if exponents.len() != count || mantissae.len() != count {
                return None;
            }
            components.push(lossless_coordinate_component(&exponents, &mantissae)?);
        }
    } else {
        let mut ranges = Vec::with_capacity(component_count);
        for _ in 0..component_count {
            let minimum = f32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?);
            let maximum = f32::from_le_bytes(bytes.get(cursor + 4..cursor + 8)?.try_into().ok()?);
            let bits = *bytes.get(cursor + 8)?;
            if bits != expected_bits
                || !minimum.is_finite()
                || !maximum.is_finite()
                || minimum > maximum
            {
                return None;
            }
            ranges.push([minimum, maximum]);
            cursor = cursor.checked_add(9)?;
        }
        for range in ranges {
            let (residuals, byte_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(byte_len)?;
            if residuals.len() != count {
                return None;
            }
            components.push(
                unpack_predictor_residuals(&residuals, Predictor::Lag1)
                    .into_iter()
                    .map(|code| dequantize_uniform(code, range, expected_bits))
                    .collect::<Option<Vec<_>>>()?,
            );
        }
    }
    let hash = read_u32(bytes, cursor)?;
    cursor = cursor.checked_add(4)?;
    let values = (0..count)
        .map(|index| {
            (0..component_count)
                .map(|component| components.get(component)?.get(index).copied())
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    Some((values, hash, cursor))
}

/// Decode one JT compressed color array as RGBA values and its trailing hash.
pub(crate) fn decode_vertex_colors(
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Option<(Vec<[f32; 4]>, u32, usize)> {
    let count = usize::try_from(read_u32(bytes, 0)?).ok()?;
    let component_count = usize::from(*bytes.get(4)?);
    if count != expected_count
        || !matches!(component_count, 3 | 4)
        || *bytes.get(5)? != expected_bits
        || expected_bits > 8
    {
        return None;
    }
    let mut cursor = 6usize;
    let colors = if expected_bits == 0 {
        let mut components = Vec::with_capacity(component_count);
        for _ in 0..component_count {
            let (exponents, exponent_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(exponent_len)?;
            let (mantissae, mantissa_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(mantissa_len)?;
            if exponents.len() != count || mantissae.len() != count {
                return None;
            }
            let exponents = unpack_predictor_residuals(&exponents, Predictor::Lag1);
            let mantissae = unpack_predictor_residuals(&mantissae, Predictor::Lag1);
            components.push(lossless_coordinate_component(&exponents, &mantissae)?);
        }
        (0..count)
            .map(|index| {
                Some([
                    *components.first()?.get(index)?,
                    *components.get(1)?.get(index)?,
                    *components.get(2)?.get(index)?,
                    components
                        .get(3)
                        .and_then(|component| component.get(index))
                        .copied()
                        .unwrap_or(1.0),
                ])
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        let hsv = match *bytes.get(cursor)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        cursor = cursor.checked_add(1)?;
        let mut ranges = Vec::with_capacity(4);
        let mut component_bits = Vec::with_capacity(4);
        if hsv {
            for range in [[0.0, 6.0], [0.0, 1.0], [0.0, 1.0], [0.0, 1.0]] {
                let bits = *bytes.get(cursor)?;
                if bits == 0 || bits > 8 {
                    return None;
                }
                ranges.push(range);
                component_bits.push(bits);
                cursor = cursor.checked_add(1)?;
            }
        } else {
            for _ in 0..4 {
                let minimum = f32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?);
                let maximum =
                    f32::from_le_bytes(bytes.get(cursor + 4..cursor + 8)?.try_into().ok()?);
                let bits = *bytes.get(cursor + 8)?;
                if bits == 0
                    || bits > 8
                    || !minimum.is_finite()
                    || !maximum.is_finite()
                    || minimum > maximum
                {
                    return None;
                }
                ranges.push([minimum, maximum]);
                component_bits.push(bits);
                cursor = cursor.checked_add(9)?;
            }
        }
        let mut components = Vec::with_capacity(4);
        for component in 0..4 {
            let (residuals, byte_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(byte_len)?;
            if residuals.len() != count {
                return None;
            }
            components.push(
                unpack_predictor_residuals(&residuals, Predictor::Lag1)
                    .into_iter()
                    .map(|code| {
                        dequantize_uniform(code, ranges[component], component_bits[component])
                    })
                    .collect::<Option<Vec<_>>>()?,
            );
        }
        (0..count)
            .map(|index| {
                let first = components[0][index];
                let second = components[1][index];
                let third = components[2][index];
                let alpha = components[3][index];
                if hsv {
                    let [red, green, blue] = hsv_to_rgb(first, second, third)?;
                    Some([red, green, blue, alpha])
                } else {
                    Some([first, second, third, alpha])
                }
            })
            .collect::<Option<Vec<_>>>()?
    };
    let hash = read_u32(bytes, cursor)?;
    cursor = cursor.checked_add(4)?;
    Some((colors, hash, cursor))
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> Option<[f32; 3]> {
    if !hue.is_finite() || !saturation.is_finite() || !value.is_finite() {
        return None;
    }
    let hue = hue.rem_euclid(6.0);
    let chroma = value * saturation;
    let intermediate = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let minimum = value - chroma;
    let [red, green, blue] = match hue as u8 {
        0 => [chroma, intermediate, 0.0],
        1 => [intermediate, chroma, 0.0],
        2 => [0.0, chroma, intermediate],
        3 => [0.0, intermediate, chroma],
        4 => [intermediate, 0.0, chroma],
        5 => [chroma, 0.0, intermediate],
        _ => unreachable!(),
    };
    let result = [red + minimum, green + minimum, blue + minimum];
    result
        .iter()
        .all(|component| component.is_finite())
        .then_some(result)
}

/// Decode one JT compressed vertex-flag array.
pub(crate) fn decode_vertex_flags(
    bytes: &[u8],
    expected_count: usize,
) -> Option<(Vec<u32>, usize)> {
    let count = usize::try_from(read_u32(bytes, 0)?).ok()?;
    if count != expected_count {
        return None;
    }
    let (values, byte_len) = decode_int32_cdp2(bytes.get(4..)?, 0)?;
    if values.len() != count {
        return None;
    }
    let values = values
        .into_iter()
        .map(|value| u32::try_from(value).ok().filter(|value| *value <= 1))
        .collect::<Option<Vec<_>>>()?;
    Some((values, 4usize.checked_add(byte_len)?))
}

pub(crate) fn dequantize_uniform(code: i32, range: [f32; 2], bits: u8) -> Option<f32> {
    if bits == 0
        || bits > 32
        || !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] > range[1]
    {
        return None;
    }
    let maximum_code = if bits == 32 {
        u32::MAX
    } else {
        (1_u32 << bits) - 1
    };
    let code = code as u32;
    if code > maximum_code {
        return None;
    }
    let step = (f64::from(range[1]) - f64::from(range[0])) / f64::from(maximum_code);
    let value = (f64::from(range[0]) + (f64::from(code) - 0.5) * step) as f32;
    value.is_finite().then_some(value)
}

/// Decode the component vectors and hash of one JT vertex-coordinate array.
pub(crate) fn decode_vertex_coordinates(
    bytes: &[u8],
    vertex_count: usize,
    ranges: [[f32; 2]; 3],
    quantization_bits: [u8; 3],
) -> Option<(Vec<[f32; 3]>, u32, usize)> {
    let mut cursor = 0usize;
    let mut components = Vec::with_capacity(3);
    for component in 0..3 {
        if quantization_bits[component] == 0 {
            let (exponent_residuals, exponent_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(exponent_len)?;
            let (mantissa_residuals, mantissa_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(mantissa_len)?;
            if exponent_residuals.len() != vertex_count || mantissa_residuals.len() != vertex_count
            {
                return None;
            }
            components.push(lossless_coordinate_component(
                &unpack_predictor_residuals(&exponent_residuals, Predictor::Lag1),
                &unpack_predictor_residuals(&mantissa_residuals, Predictor::Lag1),
            )?);
        } else {
            let (residuals, byte_len) = decode_int32_cdp2(bytes.get(cursor..)?, 0)?;
            cursor = cursor.checked_add(byte_len)?;
            if residuals.len() != vertex_count {
                return None;
            }
            components.push(
                unpack_predictor_residuals(&residuals, Predictor::Lag1)
                    .into_iter()
                    .map(|code| {
                        dequantize_uniform(code, ranges[component], quantization_bits[component])
                    })
                    .collect::<Option<Vec<_>>>()?,
            );
        }
    }
    let coordinate_hash = read_u32(bytes, cursor)?;
    cursor = cursor.checked_add(4)?;
    let points = (0..vertex_count)
        .map(|index| {
            [
                components[0][index],
                components[1][index],
                components[2][index],
            ]
        })
        .collect();
    Some((points, coordinate_hash, cursor))
}

/// Bound one complete JT Int32 Compressed Data Packet Mk. 2 without interpreting its symbols.
pub(crate) fn frame_int32_cdp2(bytes: &[u8], depth: u8) -> Option<(u32, u8, usize)> {
    if depth > 3 {
        return None;
    }
    let value_count = read_u32(bytes, 0)?;
    if value_count == 0 {
        return Some((0, 0, 4));
    }
    let &codec = bytes.get(4)?;
    if codec == 4 {
        let &chop_bits = bytes.get(5)?;
        if chop_bits == 0 {
            let (nested_count, _, nested_len) = frame_int32_cdp2(bytes.get(6..)?, depth + 1)?;
            return (nested_count == value_count).then_some((value_count, codec, 6 + nested_len));
        }
        let &span_bits = bytes.get(10)?;
        if chop_bits > span_bits || span_bits > 32 {
            return None;
        }
        let (msb_count, _, msb_len) = frame_int32_cdp2(bytes.get(11..)?, depth + 1)?;
        let (lsb_count, _, lsb_len) = frame_int32_cdp2(bytes.get(11 + msb_len..)?, depth + 1)?;
        return (msb_count == value_count && lsb_count == value_count).then_some((
            value_count,
            codec,
            11 + msb_len + lsb_len,
        ));
    }
    if !matches!(codec, 1 | 3) {
        return None;
    }
    let code_bit_len = usize::try_from(read_u32(bytes, 5)?).ok()?;
    let code_byte_len = code_bit_len.div_ceil(32).checked_mul(4)?;
    let mut cursor = 9_usize.checked_add(code_byte_len)?;
    bytes.get(..cursor)?;
    if codec == 1 {
        return Some((value_count, codec, cursor));
    }
    let (entries, context_len) = parse_probability_context(bytes.get(cursor..)?)?;
    cursor = cursor.checked_add(context_len)?;
    let code_words = bytes.get(9..9 + code_byte_len)?;
    let symbols = decode_arithmetic(
        code_words,
        code_bit_len,
        usize::try_from(value_count).ok()?,
        &entries,
    )?;
    let escape_count = symbols.iter().filter(|value| value.is_none()).count();
    let (out_of_band_count, _, out_of_band_len) =
        frame_int32_cdp2(bytes.get(cursor..)?, depth + 1)?;
    if usize::try_from(out_of_band_count).ok()? != escape_count {
        return None;
    }
    cursor = cursor.checked_add(out_of_band_len)?;
    Some((value_count, codec, cursor))
}

fn parse_probability_context(bytes: &[u8]) -> Option<(Vec<ProbabilityEntry>, usize)> {
    let entry_count = usize::from(u16::from_be_bytes(bytes.get(..2)?.try_into().ok()?));
    let mut bits = MsbBitReader::new(bytes.get(2..)?);
    let symbol_bits = u8::try_from(bits.read(6)?).ok()?;
    let occurrence_bits = u8::try_from(bits.read(6)?).ok()?;
    let value_bits = u8::try_from(bits.read(6)?).ok()?;
    let minimum = bits.read(32)? as i32;
    if symbol_bits > 32 || occurrence_bits > 32 || value_bits > 32 {
        return None;
    }
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let symbol = bits.read(symbol_bits)? as i32 - 2;
        let occurrence_count = bits.read(occurrence_bits)?;
        let value = (bits.read(value_bits)? as i32).wrapping_add(minimum);
        entries.push(ProbabilityEntry {
            symbol,
            occurrence_count,
            value,
        });
    }
    let bit_bytes = bits.finish_zero_padding()?;
    Some((entries, 2 + bit_bytes))
}

struct CodeBits<'a> {
    words: &'a [u8],
    bit_len: usize,
    bit: usize,
}

impl CodeBits<'_> {
    fn read(&mut self, count: u8) -> Option<u32> {
        let end = self.bit.checked_add(usize::from(count))?;
        if end > self.bit_len {
            return None;
        }
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.next());
        }
        Some(value)
    }

    fn read_signed(&mut self, count: u8) -> Option<i32> {
        let raw = self.read(count)?;
        Some(match count {
            0 => 0,
            32 => raw as i32,
            _ => ((raw << (32 - count)) as i32) >> (32 - count),
        })
    }

    fn next(&mut self) -> u16 {
        if self.bit >= self.bit_len {
            return 0;
        }
        let word_index = self.bit / 32;
        let bit_index = self.bit % 32;
        let offset = word_index * 4;
        let word = self
            .words
            .get(offset..offset + 4)
            .and_then(|value| value.try_into().ok())
            .map_or(0, u32::from_le_bytes);
        self.bit += 1;
        ((word >> (31 - bit_index)) & 1) as u16
    }
}

/// Upper bound on values a single arithmetic-coded lane may declare.
const MAX_ARITHMETIC_VALUES: usize = 1_000_000;

fn decode_arithmetic(
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
    entries: &[ProbabilityEntry],
) -> Option<Vec<Option<i32>>> {
    // Arithmetic symbols can consume zero code bits, so the stream length puts
    // no floor under `value_count`; an absolute cap bounds the allocation and
    // the per-value decode work instead.
    if value_count > MAX_ARITHMETIC_VALUES {
        return None;
    }
    let total: u32 = entries
        .iter()
        .try_fold(0u32, |sum, entry| sum.checked_add(entry.occurrence_count))?;
    if total == 0 || total > u32::from(u16::MAX) {
        return None;
    }
    let mut bits = CodeBits {
        words: code_words,
        bit_len: code_bit_len,
        bit: 0,
    };
    let mut code = 0u16;
    for _ in 0..16 {
        code = (code << 1) | bits.next();
    }
    let mut low = 0u16;
    let mut high = u16::MAX;
    let mut values = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let range = u32::from(high.wrapping_sub(low)) + 1;
        let scaled = ((u32::from(code.wrapping_sub(low)) + 1) * total - 1) / range;
        let mut cumulative = 0u32;
        let entry = entries.iter().find(|entry| {
            let end = cumulative + entry.occurrence_count;
            let contains = scaled >= cumulative && scaled < end;
            if !contains {
                cumulative = end;
            }
            contains
        })?;
        let entry_high = cumulative + entry.occurrence_count;
        high = low.wrapping_add(((range * entry_high) / total - 1) as u16);
        low = low.wrapping_add(((range * cumulative) / total) as u16);
        loop {
            if ((high ^ low) & 0x8000) == 0 {
            } else if low & 0x4000 != 0 && high & 0x4000 == 0 {
                code ^= 0x4000;
                low &= 0x3fff;
                high |= 0x4000;
            } else {
                break;
            }
            low = low.wrapping_shl(1);
            high = high.wrapping_shl(1) | 1;
            code = code.wrapping_shl(1) | bits.next();
        }
        values.push(if entry.symbol == -2 {
            None
        } else {
            Some(entry.value)
        });
    }
    Some(values)
}

fn decode_bitlength(
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
) -> Option<Vec<i32>> {
    let mut bits = CodeBits {
        words: code_words,
        bit_len: code_bit_len,
        bit: 0,
    };
    let value_count =
        cadmpeg_ir::wire::cursor::bounded_len(value_count as u64, 1, MAX_ARITHMETIC_VALUES)?;
    let mut values = Vec::with_capacity(value_count);
    if bits.read(1)? == 0 {
        let minimum_bits = u8::try_from(bits.read(6)?).ok()?;
        let maximum_bits = u8::try_from(bits.read(6)?).ok()?;
        if minimum_bits > 32 || maximum_bits > 32 {
            return None;
        }
        let minimum = bits.read_signed(minimum_bits)?;
        let maximum = bits.read_signed(maximum_bits)?;
        if maximum < minimum {
            return None;
        }
        let span = u32::try_from(i64::from(maximum) - i64::from(minimum)).ok()?;
        let width = if span == 0 {
            0
        } else {
            (u32::BITS - span.leading_zeros()) as u8
        };
        for _ in 0..value_count {
            let code = bits.read(width)?;
            let value = i64::from(minimum) + i64::from(code);
            if value > i64::from(maximum) {
                return None;
            }
            values.push(i32::try_from(value).ok()?);
        }
    } else {
        let mean = bits.read_signed(32)?;
        let delta_bits = u8::try_from(bits.read(3)?).ok()?;
        let run_bits = u8::try_from(bits.read(3)?).ok()?;
        if delta_bits == 0 || run_bits == 0 {
            return None;
        }
        let minimum_delta = -(1_i32 << (delta_bits - 1));
        let maximum_delta = (1_i32 << (delta_bits - 1)) - 1;
        let mut width = 0i32;
        while values.len() < value_count {
            loop {
                let delta = bits.read_signed(delta_bits)?;
                width = width.checked_add(delta)?;
                if !(0..=32).contains(&width) {
                    return None;
                }
                if delta != minimum_delta && delta != maximum_delta {
                    break;
                }
            }
            let run = usize::try_from(bits.read(run_bits)?).ok()?;
            if run == 0 || values.len().checked_add(run)? > value_count {
                return None;
            }
            for _ in 0..run {
                values.push(mean.wrapping_add(bits.read_signed(width as u8)?));
            }
        }
    }
    (bits.bit == code_bit_len).then_some(values)
}

/// Decode one complete JT Int32 Compressed Data Packet Mk. 2.
pub(crate) fn decode_int32_cdp2(bytes: &[u8], depth: u8) -> Option<(Vec<i32>, usize)> {
    if depth > 3 {
        return None;
    }
    let value_count = usize::try_from(read_u32(bytes, 0)?).ok()?;
    if value_count == 0 {
        return Some((Vec::new(), 4));
    }
    let &codec = bytes.get(4)?;
    if codec == 4 {
        let &chop_bits = bytes.get(5)?;
        if chop_bits == 0 {
            let (values, nested_len) = decode_int32_cdp2(bytes.get(6..)?, depth + 1)?;
            return (values.len() == value_count).then_some((values, 6 + nested_len));
        }
        let bias = read_u32(bytes, 6)? as i32;
        let &span_bits = bytes.get(10)?;
        if chop_bits > span_bits || span_bits > 32 {
            return None;
        }
        let (msb, msb_len) = decode_int32_cdp2(bytes.get(11..)?, depth + 1)?;
        let (lsb, lsb_len) = decode_int32_cdp2(bytes.get(11 + msb_len..)?, depth + 1)?;
        if msb.len() != value_count || lsb.len() != value_count {
            return None;
        }
        let shift = span_bits - chop_bits;
        let low_mask = if shift == 32 {
            u32::MAX
        } else {
            (1_u32 << shift) - 1
        };
        if lsb
            .iter()
            .any(|value| *value < 0 || (*value as u32) > low_mask)
        {
            return None;
        }
        let values = msb
            .into_iter()
            .zip(lsb)
            .map(|(high, low)| (low | high.wrapping_shl(u32::from(shift))).wrapping_add(bias))
            .collect();
        return Some((values, 11 + msb_len + lsb_len));
    }
    if !matches!(codec, 1 | 3) {
        return None;
    }
    let code_bit_len = usize::try_from(read_u32(bytes, 5)?).ok()?;
    let word_count = code_bit_len.div_ceil(32);
    let code_byte_len = word_count.checked_mul(4)?;
    let code_words = bytes.get(9..9 + code_byte_len)?;
    let mut cursor = 9 + code_byte_len;
    if codec == 1 {
        let values = decode_bitlength(code_words, code_bit_len, value_count)?;
        return Some((values, cursor));
    }
    let (entries, context_len) = parse_probability_context(bytes.get(cursor..)?)?;
    cursor += context_len;
    let symbols = decode_arithmetic(code_words, code_bit_len, value_count, &entries)?;
    let escape_count = symbols.iter().filter(|value| value.is_none()).count();
    let (out_of_band, oob_len) = decode_int32_cdp2(bytes.get(cursor..)?, depth + 1)?;
    if out_of_band.len() != escape_count {
        return None;
    }
    cursor += oob_len;
    let mut out_of_band = out_of_band.into_iter();
    let values = symbols
        .into_iter()
        .map(|value| value.or_else(|| out_of_band.next()))
        .collect::<Option<Vec<_>>>()?;
    Some((values, cursor))
}
