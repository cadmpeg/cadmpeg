// SPDX-License-Identifier: Apache-2.0
//! Siemens JT integer packet decoding used by embedded NX display models.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteBinary32;
use serde::{Deserialize, Serialize};

/// Ordered finite endpoints of one JT binary32 quantization interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f32; 2]", into = "[f32; 2]")]
pub(crate) struct QuantizedRange([FiniteBinary32; 2]);

impl QuantizedRange {
    pub(crate) const ZERO: Self = Self([FiniteBinary32::ZERO; 2]);

    pub(crate) fn new(minimum: f32, maximum: f32) -> Option<Self> {
        let minimum = FiniteBinary32::new(minimum)?;
        let maximum = FiniteBinary32::new(maximum)?;
        (minimum.get() <= maximum.get()).then_some(Self([minimum, maximum]))
    }

    fn get(self) -> [f32; 2] {
        self.0.map(FiniteBinary32::get)
    }
}

impl TryFrom<[f32; 2]> for QuantizedRange {
    type Error = &'static str;

    fn try_from([minimum, maximum]: [f32; 2]) -> Result<Self, Self::Error> {
        Self::new(minimum, maximum).ok_or("quantized range: finite ordered endpoints required")
    }
}

impl From<QuantizedRange> for [f32; 2] {
    fn from(value: QuantizedRange) -> Self {
        value.get()
    }
}

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
    View::u32_le_at(bytes, offset)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Predictor {
    Lag1,
    Null,
}

pub(crate) struct DecodedVertexArray<T> {
    pub(crate) values: Vec<T>,
    pub(crate) hash: u32,
    pub(crate) byte_len: usize,
}

/// Scratch lane whose storage admission survives borrowed consumption.
struct ScratchLane<'ctx, T> {
    values: Vec<T>,
    reservation: ScopedReservation<'ctx>,
}

impl<T> ScratchLane<'_, T> {
    fn into_retained(self) -> Result<Vec<T>, CodecError> {
        self.reservation.commit()?;
        Ok(self.values)
    }
}

impl<T> std::ops::Deref for ScratchLane<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<'lane, T> IntoIterator for &'lane ScratchLane<'_, T> {
    type Item = &'lane T;
    type IntoIter = std::slice::Iter<'lane, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

/// Reconstruct JT primal integers from predictor residuals.
pub(crate) fn unpack_predictor_residuals(
    ctx: &DecodeContext<'_>,
    residuals: &[i32],
    predictor: Predictor,
) -> Result<Vec<i32>, CodecError> {
    unpack_predictor_scratch(ctx, residuals, predictor)?.into_retained()
}

fn unpack_predictor_scratch<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    residuals: &[i32],
    predictor: Predictor,
) -> Result<ScratchLane<'ctx, i32>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(residuals.len()),
        "unpack JT predictor residuals",
    )?;
    let (mut values, reservation) = ctx.temporary_vec(residuals.len(), "nx JT decoded vector")?;
    if predictor == Predictor::Null {
        values.extend_from_slice(residuals);
        return Ok(ScratchLane {
            values,
            reservation,
        });
    }
    for (index, &residual) in residuals.iter().enumerate() {
        if index < 4 {
            values.push(residual);
            continue;
        }
        // wrapping-exception: JT Int32 predictor and packet integer addition is modulo 2^32
        values.push(residual.wrapping_add(values[index - 1]));
    }
    Ok(ScratchLane {
        values,
        reservation,
    })
}

fn lossless_coordinate_component<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    exponents: &[i32],
    mantissae: &[i32],
) -> Result<Option<ScratchLane<'ctx, FiniteBinary32>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        if exponents.len() != mantissae.len() {
            return None;
        }
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(exponents.len()),
            "form JT lossless component"
        ));
        let (mut values, reservation) =
            propagate_resource!(ctx.temporary_vec(exponents.len(), "nx JT decoded vector"));
        for (&exponent, &mantissa) in exponents.iter().zip(mantissae) {
            let exponent = exponent.cast_unsigned() & 0x1ff;
            let mantissa = mantissa.cast_unsigned() & 0x7f_ffff;
            let value = f32::from_bits((exponent << 23) | mantissa);
            values.push(FiniteBinary32::new(value)?);
        }
        Some(Ok(ScratchLane {
            values,
            reservation,
        }))
    })();
    decoded.transpose()
}

/// One of the six sextants of the Deering normal encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sextant {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
}

impl Sextant {
    fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            2 => Some(Self::Two),
            3 => Some(Self::Three),
            4 => Some(Self::Four),
            5 => Some(Self::Five),
            _ => None,
        }
    }

    fn from_hue_sixth(hue: f32) -> Option<Self> {
        Some(
            match cadmpeg_core::convert::truncate_f64_to_u8(f64::from(hue))? {
                0 => Self::Zero,
                1 => Self::One,
                2 => Self::Two,
                3 => Self::Three,
                4 => Self::Four,
                _ => Self::Five,
            },
        )
    }

    fn is_odd(self) -> bool {
        matches!(self, Self::One | Self::Three | Self::Five)
    }
}

/// One of the eight octants of the Deering normal encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Octant(u32);

impl Octant {
    fn new(octant: i32) -> Option<Self> {
        u32::try_from(octant)
            .ok()
            .filter(|value| *value < 8)
            .map(Self)
    }
}

/// The bit width of a Deering angle code, between one and thirteen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NormalBits(u8);

impl NormalBits {
    fn new(bits: u8) -> Option<Self> {
        (1..=13).contains(&bits).then_some(Self(bits))
    }
}

/// A Deering angle code with the shift that widens it to a thirteen-bit index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NormalCode {
    value: u32,
    shift: u8,
}

impl NormalCode {
    fn new(code: i32, bits: NormalBits) -> Option<Self> {
        let value = u32::try_from(code)
            .ok()
            .filter(|value| *value < (1_u32 << bits.0))?;
        Some(Self {
            value,
            shift: 13 - bits.0,
        })
    }

    fn index(self, parity: u32) -> u32 {
        (self.value + parity) << self.shift
    }
}

fn deering_normal(
    sextant: Sextant,
    octant: Octant,
    theta: NormalCode,
    psi: NormalCode,
) -> Option<[FiniteBinary32; 3]> {
    let theta_index = theta.index(u32::from(sextant.is_odd()));
    let psi_index = psi.index(0);
    let table_size = f64::from(1_u32 << 13);
    let maximum_psi = 0.615_479_709_f64;
    let theta_angle = (maximum_psi * (table_size - f64::from(theta_index)) / table_size)
        .tan()
        .asin();
    let psi_angle = maximum_psi * f64::from(psi_index) / table_size;
    let x = cadmpeg_core::convert::f32_from_f64(psi_angle.cos() * theta_angle.cos())?;
    let y = cadmpeg_core::convert::f32_from_f64(psi_angle.sin())?;
    let z = cadmpeg_core::convert::f32_from_f64(psi_angle.cos() * theta_angle.sin())?;
    let mut result = match sextant {
        Sextant::Zero => [x, y, z],
        Sextant::One => [z, y, x],
        Sextant::Two => [y, z, x],
        Sextant::Three => [y, x, z],
        Sextant::Four => [z, x, y],
        Sextant::Five => [x, z, y],
    };
    for (component, bit) in [4, 2, 1].into_iter().enumerate() {
        if octant.0 & bit == 0 {
            result[component] = -result[component];
        }
    }
    Some([
        FiniteBinary32::new(result[0])?,
        FiniteBinary32::new(result[1])?,
        FiniteBinary32::new(result[2])?,
    ])
}

fn finish_decode<T>(ctx: &DecodeContext<'_>, value: Option<T>) -> Result<Option<T>, CodecError> {
    ctx.charge_work(0, "complete JT packet decode")?;
    Ok(value)
}

/// Decode one JT compressed normal array and its trailing hash.
pub(crate) fn decode_vertex_normals(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 3]>>, CodecError> {
    finish_decode(
        ctx,
        decode_vertex_normals_inner(ctx, bytes, expected_count, expected_bits)?,
    )
}

fn decode_vertex_normals_inner(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 3]>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        let count = usize::try_from(read_u32(bytes, 0)?).ok()?;
        if count != expected_count || *bytes.get(4)? != 3 || *bytes.get(5)? != expected_bits {
            return None;
        }
        let mut cursor = 6usize;
        let normals = if expected_bits == 0 {
            let (mut components, _components_reservation) =
                propagate_resource!(ctx.temporary_vec(3, "nx JT decoded vector"));
            for _ in 0..3 {
                let (exponents, exponent_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(exponent_len)?;
                let (mantissae, mantissa_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(mantissa_len)?;
                if exponents.len() != count || mantissae.len() != count {
                    return None;
                }
                components.push(propagate_resource!(lossless_coordinate_component(
                    ctx, &exponents, &mantissae
                ))?);
            }
            let mut normals =
                propagate_resource!(ctx.collection_vec(count, "nx JT decoded vector"));
            for ((x, y), z) in components[0].iter().zip(&components[1]).zip(&components[2]) {
                normals.push([*x, *y, *z]);
            }
            normals
        } else {
            let (mut codes, _codes_reservation) =
                propagate_resource!(ctx.temporary_vec(4, "nx JT decoded vector"));
            for _ in 0..4 {
                let (values, byte_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(byte_len)?;
                if values.len() != count {
                    return None;
                }
                codes.push(values);
            }
            let bits = NormalBits::new(expected_bits)?;
            let mut normals =
                propagate_resource!(ctx.collection_vec(count, "nx JT decoded vector"));
            for (((sextant, octant), theta), psi) in
                codes[0].iter().zip(&codes[1]).zip(&codes[2]).zip(&codes[3])
            {
                normals.push(deering_normal(
                    Sextant::from_index(*sextant)?,
                    Octant::new(*octant)?,
                    NormalCode::new(*theta, bits)?,
                    NormalCode::new(*psi, bits)?,
                )?);
            }
            normals
        };
        let hash = read_u32(bytes, cursor)?;
        cursor = cursor.checked_add(4)?;
        Some(Ok(DecodedVertexArray {
            values: normals,
            hash,
            byte_len: cursor,
        }))
    })();
    decoded.transpose()
}

/// Decode one JT compressed texture-coordinate array and its trailing hash.
pub(crate) fn decode_vertex_texture_coordinates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<Vec<FiniteBinary32>>>, CodecError> {
    finish_decode(
        ctx,
        decode_vertex_texture_coordinates_inner(ctx, bytes, expected_count, expected_bits)?,
    )
}

fn decode_vertex_texture_coordinates_inner(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<Vec<FiniteBinary32>>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
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
        let (mut components, _components_reservation) =
            propagate_resource!(ctx.temporary_vec(component_count, "nx JT decoded vector"));
        if expected_bits == 0 {
            for _ in 0..component_count {
                let (exponents, exponent_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(exponent_len)?;
                let (mantissae, mantissa_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(mantissa_len)?;
                if exponents.len() != count || mantissae.len() != count {
                    return None;
                }
                components.push(propagate_resource!(lossless_coordinate_component(
                    ctx, &exponents, &mantissae
                ))?);
            }
        } else {
            let (mut ranges, _ranges_reservation) =
                propagate_resource!(ctx.temporary_vec(component_count, "nx JT decoded vector"));
            for _ in 0..component_count {
                let minimum = View::f32_le_at(bytes, cursor)?;
                let maximum = View::f32_le_at(bytes, cursor + 4)?;
                let bits = *bytes.get(cursor + 8)?;
                if bits != expected_bits {
                    return None;
                }
                ranges.push(QuantizedRange::new(minimum, maximum)?);
                cursor = cursor.checked_add(9)?;
            }
            for range in ranges {
                let (residuals, byte_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(byte_len)?;
                if residuals.len() != count {
                    return None;
                }
                let (mut component, reservation) =
                    propagate_resource!(ctx.temporary_vec(count, "nx JT decoded vector"));
                for code in
                    propagate_resource!(unpack_predictor_scratch(ctx, &residuals, Predictor::Lag1))
                        .iter()
                        .copied()
                {
                    component.push(dequantize_uniform(
                        u32::try_from(code).ok()?,
                        range,
                        expected_bits,
                    )?);
                }
                components.push(ScratchLane {
                    values: component,
                    reservation,
                });
            }
        }
        let hash = read_u32(bytes, cursor)?;
        cursor = cursor.checked_add(4)?;
        let mut values = propagate_resource!(ctx.collection_vec(count, "nx JT decoded vector"));
        for index in 0..count {
            let mut value =
                propagate_resource!(ctx.collection_vec(component_count, "nx JT decoded vector"));
            for component in 0..component_count {
                value.push(components.get(component)?.get(index).copied()?);
            }
            values.push(value);
        }
        Some(Ok(DecodedVertexArray {
            values,
            hash,
            byte_len: cursor,
        }))
    })();
    decoded.transpose()
}

/// Decode one JT compressed color array as RGBA values and its trailing hash.
pub(crate) fn decode_vertex_colors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 4]>>, CodecError> {
    finish_decode(
        ctx,
        decode_vertex_colors_inner(ctx, bytes, expected_count, expected_bits)?,
    )
}

fn decode_vertex_colors_inner(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 4]>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
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
            let (mut components, _components_reservation) =
                propagate_resource!(ctx.temporary_vec(component_count, "nx JT decoded vector"));
            for _ in 0..component_count {
                let (exponents, exponent_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(exponent_len)?;
                let (mantissae, mantissa_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(mantissa_len)?;
                if exponents.len() != count || mantissae.len() != count {
                    return None;
                }
                let exponents =
                    propagate_resource!(unpack_predictor_scratch(ctx, &exponents, Predictor::Lag1));
                let mantissae =
                    propagate_resource!(unpack_predictor_scratch(ctx, &mantissae, Predictor::Lag1));
                components.push(propagate_resource!(lossless_coordinate_component(
                    ctx, &exponents, &mantissae
                ))?);
            }
            let mut colors = propagate_resource!(ctx.collection_vec(count, "nx JT decoded vector"));
            for index in 0..count {
                colors.push([
                    *components.first()?.get(index)?,
                    *components.get(1)?.get(index)?,
                    *components.get(2)?.get(index)?,
                    components
                        .get(3)
                        .and_then(|component| component.get(index))
                        .copied()
                        .unwrap_or(FiniteBinary32::ONE),
                ]);
            }
            colors
        } else {
            let hsv = match *bytes.get(cursor)? {
                0 => false,
                1 => true,
                _ => return None,
            };
            cursor = cursor.checked_add(1)?;
            let (mut ranges, _ranges_reservation) =
                propagate_resource!(ctx.temporary_vec(4, "nx JT decoded vector"));
            let (mut component_bits, _bits_reservation) =
                propagate_resource!(ctx.temporary_vec(4, "nx JT decoded vector"));
            if hsv {
                for range in [[0.0, 6.0], [0.0, 1.0], [0.0, 1.0], [0.0, 1.0]] {
                    let bits = *bytes.get(cursor)?;
                    if bits == 0 || bits > 8 {
                        return None;
                    }
                    ranges.push(QuantizedRange::new(range[0], range[1])?);
                    component_bits.push(bits);
                    cursor = cursor.checked_add(1)?;
                }
            } else {
                for _ in 0..4 {
                    let minimum = View::f32_le_at(bytes, cursor)?;
                    let maximum = View::f32_le_at(bytes, cursor + 4)?;
                    let bits = *bytes.get(cursor + 8)?;
                    if bits == 0 || bits > 8 {
                        return None;
                    }
                    ranges.push(QuantizedRange::new(minimum, maximum)?);
                    component_bits.push(bits);
                    cursor = cursor.checked_add(9)?;
                }
            }
            let (mut components, _components_reservation) =
                propagate_resource!(ctx.temporary_vec(4, "nx JT decoded vector"));
            for component in 0..4 {
                let (residuals, byte_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(byte_len)?;
                if residuals.len() != count {
                    return None;
                }
                let (mut values, reservation) =
                    propagate_resource!(ctx.temporary_vec(count, "nx JT decoded vector"));
                for code in
                    propagate_resource!(unpack_predictor_scratch(ctx, &residuals, Predictor::Lag1))
                        .iter()
                        .copied()
                {
                    values.push(dequantize_uniform(
                        u32::try_from(code).ok()?,
                        *ranges.get(component)?,
                        *component_bits.get(component)?,
                    )?);
                }
                components.push(ScratchLane {
                    values,
                    reservation,
                });
            }
            let mut colors = propagate_resource!(ctx.collection_vec(count, "nx JT decoded vector"));
            for index in 0..count {
                let first = *components.first()?.get(index)?;
                let second = *components.get(1)?.get(index)?;
                let third = *components.get(2)?.get(index)?;
                let alpha = *components.get(3)?.get(index)?;
                if hsv {
                    let [red, green, blue] = hsv_to_rgb(first, second, third)?;
                    colors.push([red, green, blue, alpha]);
                } else {
                    colors.push([first, second, third, alpha]);
                }
            }
            colors
        };
        let hash = read_u32(bytes, cursor)?;
        cursor = cursor.checked_add(4)?;
        Some(Ok(DecodedVertexArray {
            values: colors,
            hash,
            byte_len: cursor,
        }))
    })();
    decoded.transpose()
}

fn hsv_to_rgb(
    hue: FiniteBinary32,
    saturation: FiniteBinary32,
    value: FiniteBinary32,
) -> Option<[FiniteBinary32; 3]> {
    let hue = hue.get().rem_euclid(6.0);
    let chroma = value.get() * saturation.get();
    let intermediate = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let minimum = value.get() - chroma;
    let [red, green, blue] = match Sextant::from_hue_sixth(hue)? {
        Sextant::Zero => [chroma, intermediate, 0.0],
        Sextant::One => [intermediate, chroma, 0.0],
        Sextant::Two => [0.0, chroma, intermediate],
        Sextant::Three => [0.0, intermediate, chroma],
        Sextant::Four => [intermediate, 0.0, chroma],
        Sextant::Five => [chroma, 0.0, intermediate],
    };
    Some([
        FiniteBinary32::new(red + minimum)?,
        FiniteBinary32::new(green + minimum)?,
        FiniteBinary32::new(blue + minimum)?,
    ])
}

/// Decode one JT compressed vertex-flag array.
pub(crate) fn decode_vertex_flags(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
) -> Result<Option<(Vec<u32>, usize)>, CodecError> {
    finish_decode(ctx, decode_vertex_flags_inner(ctx, bytes, expected_count)?)
}

fn decode_vertex_flags_inner(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
) -> Result<Option<(Vec<u32>, usize)>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        let count = usize::try_from(read_u32(bytes, 0)?).ok()?;
        if count != expected_count {
            return None;
        }
        let (values, byte_len) =
            propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(4..)?, 0))?;
        if values.len() != count {
            return None;
        }
        let mut flags = propagate_resource!(ctx.collection_vec(count, "nx JT decoded vector"));
        for value in values.iter().copied() {
            flags.push(u32::try_from(value).ok().filter(|value| *value <= 1)?);
        }
        Some(Ok((flags, 4usize.checked_add(byte_len)?)))
    })();
    decoded.transpose()
}

fn dequantize_uniform(code: u32, range: QuantizedRange, bits: u8) -> Option<FiniteBinary32> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let range = range.get();
    let maximum_code = if bits == 32 {
        u32::MAX
    } else {
        (1_u32 << bits) - 1
    };
    if code > maximum_code {
        return None;
    }
    let step = (f64::from(range[1]) - f64::from(range[0])) / f64::from(maximum_code);
    let value =
        cadmpeg_core::convert::f32_from_f64(f64::from(range[0]) + (f64::from(code) - 0.5) * step)?;
    FiniteBinary32::new(value)
}

/// Decode the component vectors and hash of one JT vertex-coordinate array.
pub(crate) fn decode_vertex_coordinates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    vertex_count: usize,
    ranges: [QuantizedRange; 3],
    quantization_bits: [u8; 3],
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 3]>>, CodecError> {
    finish_decode(
        ctx,
        decode_vertex_coordinates_inner(ctx, bytes, vertex_count, ranges, quantization_bits)?,
    )
}

fn decode_vertex_coordinates_inner(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    vertex_count: usize,
    ranges: [QuantizedRange; 3],
    quantization_bits: [u8; 3],
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 3]>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        let mut cursor = 0usize;
        let (mut components, _components_reservation) =
            propagate_resource!(ctx.temporary_vec(3, "nx JT decoded vector"));
        for component in 0..3 {
            if quantization_bits[component] == 0 {
                let (exponent_residuals, exponent_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(exponent_len)?;
                let (mantissa_residuals, mantissa_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(mantissa_len)?;
                if exponent_residuals.len() != vertex_count
                    || mantissa_residuals.len() != vertex_count
                {
                    return None;
                }
                components.push(propagate_resource!(lossless_coordinate_component(
                    ctx,
                    &propagate_resource!(unpack_predictor_scratch(
                        ctx,
                        &exponent_residuals,
                        Predictor::Lag1
                    )),
                    &propagate_resource!(unpack_predictor_scratch(
                        ctx,
                        &mantissa_residuals,
                        Predictor::Lag1
                    )),
                ))?);
            } else {
                let (residuals, byte_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(cursor..)?, 0))?;
                cursor = cursor.checked_add(byte_len)?;
                if residuals.len() != vertex_count {
                    return None;
                }
                let (mut values, reservation) =
                    propagate_resource!(ctx.temporary_vec(vertex_count, "nx JT decoded vector"));
                for code in
                    propagate_resource!(unpack_predictor_scratch(ctx, &residuals, Predictor::Lag1))
                        .iter()
                        .copied()
                {
                    values.push(dequantize_uniform(
                        u32::try_from(code).ok()?,
                        ranges[component],
                        quantization_bits[component],
                    )?);
                }
                components.push(ScratchLane {
                    values,
                    reservation,
                });
            }
        }
        let coordinate_hash = read_u32(bytes, cursor)?;
        cursor = cursor.checked_add(4)?;
        let mut points =
            propagate_resource!(ctx.collection_vec(vertex_count, "nx JT decoded vector"));
        for index in 0..vertex_count {
            points.push([
                *components.first()?.get(index)?,
                *components.get(1)?.get(index)?,
                *components.get(2)?.get(index)?,
            ]);
        }
        Some(Ok(DecodedVertexArray {
            values: points,
            hash: coordinate_hash,
            byte_len: cursor,
        }))
    })();
    decoded.transpose()
}

/// Bound one complete JT Int32 Compressed Data Packet Mk. 2 without interpreting its symbols.
pub(crate) fn frame_int32_cdp2(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(u32, u8, usize)>, CodecError> {
    finish_decode(ctx, frame_int32_cdp2_inner(ctx, bytes, depth)?)
}

fn frame_int32_cdp2_inner(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(u32, u8, usize)>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        if depth > 3 {
            return None;
        }
        let _depth = propagate_resource!(ctx.enter_nested("frame JT integer packet"));
        let value_count = read_u32(bytes, 0)?;
        if usize::try_from(value_count).ok()? > MAX_ARITHMETIC_VALUES {
            return None;
        }
        if value_count == 0 {
            return Some(Ok((0, 0, 4)));
        }
        let &codec = bytes.get(4)?;
        if codec == 4 {
            let &chop_bits = bytes.get(5)?;
            if chop_bits == 0 {
                let (nested_count, _, nested_len) =
                    propagate_resource!(frame_int32_cdp2_inner(ctx, bytes.get(6..)?, depth + 1))?;
                return (nested_count == value_count).then_some(Ok((
                    value_count,
                    codec,
                    6 + nested_len,
                )));
            }
            let &span_bits = bytes.get(10)?;
            if chop_bits > span_bits || span_bits > 32 {
                return None;
            }
            let (msb_count, _, msb_len) =
                propagate_resource!(frame_int32_cdp2_inner(ctx, bytes.get(11..)?, depth + 1))?;
            let (lsb_count, _, lsb_len) = propagate_resource!(frame_int32_cdp2_inner(
                ctx,
                bytes.get(11 + msb_len..)?,
                depth + 1
            ))?;
            return (msb_count == value_count && lsb_count == value_count)
                .then_some((value_count, codec, 11 + msb_len + lsb_len))
                .map(Ok);
        }
        if !matches!(codec, 1 | 3) {
            return None;
        }
        let code_bit_len = usize::try_from(read_u32(bytes, 5)?).ok()?;
        let code_byte_len = code_bit_len.div_ceil(32).checked_mul(4)?;
        let mut cursor = 9_usize.checked_add(code_byte_len)?;
        bytes.get(..cursor)?;
        if codec == 1 {
            return Some(Ok((value_count, codec, cursor)));
        }
        let (entries, context_len, _entries_reservation) =
            propagate_resource!(parse_probability_context(ctx, bytes.get(cursor..)?))?;
        cursor = cursor.checked_add(context_len)?;
        let code_words = bytes.get(9..9 + code_byte_len)?;
        let symbols = propagate_resource!(decode_arithmetic(
            ctx,
            code_words,
            code_bit_len,
            usize::try_from(value_count).ok()?,
            &entries,
        ))?;
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(symbols.len()),
            "count JT escape symbols"
        ));
        let escape_count = symbols.iter().filter(|value| value.is_none()).count();
        let (out_of_band_count, _, out_of_band_len) =
            propagate_resource!(frame_int32_cdp2_inner(ctx, bytes.get(cursor..)?, depth + 1))?;
        if usize::try_from(out_of_band_count).ok()? != escape_count {
            return None;
        }
        cursor = cursor.checked_add(out_of_band_len)?;
        Some(Ok((value_count, codec, cursor)))
    })();
    decoded.transpose()
}

fn parse_probability_context<'a>(
    ctx: &'a DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<(Vec<ProbabilityEntry>, usize, ScopedReservation<'a>)>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        let entry_count = usize::from(View::u16_be_at(bytes, 0)?);
        let mut bits = MsbBitReader::new(bytes.get(2..)?);
        let symbol_bits = u8::try_from(bits.read(6)?).ok()?;
        let occurrence_bits = u8::try_from(bits.read(6)?).ok()?;
        let value_bits = u8::try_from(bits.read(6)?).ok()?;
        let minimum = bits.read(32)?.cast_signed();
        if symbol_bits > 32 || occurrence_bits > 32 || value_bits > 32 {
            return None;
        }
        let entry_bits = usize::from(symbol_bits)
            .checked_add(usize::from(occurrence_bits))?
            .checked_add(usize::from(value_bits))?;
        let available_bits = bytes
            .get(2..)?
            .len()
            .checked_mul(8)?
            .checked_sub(bits.bit)?;
        // Prove the declared table fits in the bounded context before allocating
        // its entries. An all-zero-width context cannot produce a nonzero
        // arithmetic frequency table and is therefore malformed.
        (entry_bits > 0 && entry_count <= available_bits / entry_bits).then_some(())?;
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(entry_count),
            "parse JT probability context",
        ));
        let (mut entries, reservation) =
            propagate_resource!(ctx.temporary_vec(entry_count, "nx JT decoded vector"));
        for _ in 0..entry_count {
            let symbol = i32::try_from(i64::from(bits.read(symbol_bits)?).checked_sub(2)?).ok()?;
            let occurrence_count = bits.read(occurrence_bits)?;
            // wrapping-exception: JT Int32 predictor and packet integer addition is modulo 2^32
            let value = (bits.read(value_bits)?.cast_signed()).wrapping_add(minimum);
            entries.push(ProbabilityEntry {
                symbol,
                occurrence_count,
                value,
            });
        }
        let bit_bytes = bits.finish_zero_padding()?;
        Some(Ok((entries, 2 + bit_bytes, reservation)))
    })();
    decoded.transpose()
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
            value = (value << 1) | u32::from(self.next()?);
        }
        Some(value)
    }

    fn read_signed(&mut self, count: u8) -> Option<i32> {
        let raw = self.read(count)?;
        Some(match count {
            0 => 0,
            32 => raw.cast_signed(),
            _ => (raw << (32 - count)).cast_signed() >> (32 - count),
        })
    }

    fn next(&mut self) -> Option<u16> {
        if self.bit >= self.bit_len {
            return None;
        }
        let word_index = self.bit / 32;
        let bit_index = self.bit % 32;
        let offset = word_index * 4;
        let word = View::u32_le_at(self.words, offset)?;
        self.bit += 1;
        u16::try_from((word >> (31 - bit_index)) & 1).ok()
    }
}

/// Upper bound on values a single arithmetic-coded lane may declare.
const MAX_ARITHMETIC_VALUES: usize = 1_000_000;
/// Upper bound on arithmetic decoder table lookups for one lane.
const MAX_ARITHMETIC_WORK: usize = 64_000_000;

fn decode_arithmetic<'a>(
    ctx: &'a DecodeContext<'_>,
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
    entries: &[ProbabilityEntry],
) -> Result<Option<ScratchLane<'a, Option<i32>>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        // Arithmetic symbols can consume zero code bits, so the stream length puts
        // no floor under `value_count`; an absolute cap bounds the allocation and
        // the per-value decode work instead.
        if value_count > MAX_ARITHMETIC_VALUES {
            return None;
        }
        let Some(work) = entries.len().checked_mul(value_count) else {
            return Some(Err(ctx.refuse_codec_limit(
                "decode JT arithmetic symbols",
                u64::MAX - 1,
                u64::MAX,
            )));
        };
        let budget = ctx.work_budget(cadmpeg_core::decode::u64_from_index(MAX_ARITHMETIC_WORK));
        if !budget.charge_by(work) {
            let error = match ctx.resource_refusal() {
                Some(limit) => CodecError::ResourceLimit(limit),
                None => ctx.refuse_codec_limit(
                    "decode JT arithmetic symbols",
                    cadmpeg_core::decode::u64_from_index(MAX_ARITHMETIC_WORK),
                    cadmpeg_core::decode::u64_from_index(work),
                ),
            };
            return Some(Err(error));
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
            code = (code << 1) | bits.next()?;
        }
        let mut low = 0u16;
        let mut high = u16::MAX;
        let (mut values, reservation) =
            propagate_resource!(ctx.temporary_vec(value_count, "nx JT decoded vector"));
        for _ in 0..value_count {
            let range = u32::from(high.checked_sub(low)?) + 1;
            let scaled = ((u32::from(code.checked_sub(low)?) + 1) * total - 1) / range;
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
            high = low.checked_add(u16::try_from((range * entry_high) / total - 1).ok()?)?;
            low = low.checked_add(u16::try_from((range * cumulative) / total).ok()?)?;
            loop {
                if ((high ^ low) & 0x8000) == 0 {
                } else if low & 0x4000 != 0 && high & 0x4000 == 0 {
                    code ^= 0x4000;
                    low &= 0x3fff;
                    high |= 0x4000;
                } else {
                    break;
                }
                // wrapping-exception: JT arithmetic normalization uses a sixteen-bit code register
                low = low.wrapping_shl(1);
                // wrapping-exception: JT arithmetic normalization uses a sixteen-bit code register
                high = high.wrapping_shl(1) | 1;
                // wrapping-exception: JT arithmetic normalization uses a sixteen-bit code register
                code = code.wrapping_shl(1) | bits.next()?;
            }
            values.push(if entry.symbol == -2 {
                None
            } else {
                Some(entry.value)
            });
        }
        Some(Ok(ScratchLane {
            values,
            reservation,
        }))
    })();
    decoded.transpose()
}

fn decode_bitlength<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
) -> Result<Option<ScratchLane<'ctx, i32>>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        let mut bits = CodeBits {
            words: code_words,
            bit_len: code_bit_len,
            bit: 0,
        };
        let value_count = cadmpeg_core::decode::bounded_len(
            cadmpeg_core::decode::u64_from_index(value_count),
            1,
            MAX_ARITHMETIC_VALUES,
        )?;
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(value_count),
            "decode JT bitlength symbols",
        ));
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(code_bit_len),
            "decode JT bitlength code bits",
        ));
        let (mut values, reservation) =
            propagate_resource!(ctx.temporary_vec(value_count, "nx JT decoded vector"));
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
                u8::try_from(u32::BITS - span.leading_zeros()).ok()?
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
                    // wrapping-exception: JT Int32 predictor and packet integer addition is modulo 2^32
                    values.push(mean.wrapping_add(bits.read_signed(u8::try_from(width).ok()?)?));
                }
            }
        }
        (bits.bit == code_bit_len).then_some(Ok(ScratchLane {
            values,
            reservation,
        }))
    })();
    decoded.transpose()
}

/// Decode one complete JT Int32 Compressed Data Packet Mk. 2.
pub(crate) fn decode_int32_cdp2(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(Vec<i32>, usize)>, CodecError> {
    let decoded = decode_int32_cdp2_inner(ctx, bytes, depth)?
        .map(|(values, length)| -> Result<_, CodecError> { Ok((values.into_retained()?, length)) })
        .transpose()?;
    finish_decode(ctx, decoded)
}

fn decode_int32_cdp2_inner<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(ScratchLane<'ctx, i32>, usize)>, CodecError> {
    let decoded: Option<Result<_, CodecError>> = (|| {
        if depth > 3 {
            return None;
        }
        let _depth = propagate_resource!(ctx.enter_nested("decode JT integer packet"));
        let value_count = usize::try_from(read_u32(bytes, 0)?).ok()?;
        if value_count > MAX_ARITHMETIC_VALUES {
            return None;
        }
        if value_count == 0 {
            return Some(Ok((
                ScratchLane {
                    values: Vec::new(),
                    reservation: propagate_resource!(ctx.reserve_scoped(0, "nx JT decoded vector")),
                },
                4,
            )));
        }
        let &codec = bytes.get(4)?;
        if codec == 4 {
            let &chop_bits = bytes.get(5)?;
            if chop_bits == 0 {
                let (values, nested_len) =
                    propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(6..)?, depth + 1))?;
                return (values.len() == value_count).then_some(Ok((values, 6 + nested_len)));
            }
            let bias = read_u32(bytes, 6)?.cast_signed();
            let &span_bits = bytes.get(10)?;
            if chop_bits > span_bits || span_bits > 32 {
                return None;
            }
            let (msb, msb_len) =
                propagate_resource!(decode_int32_cdp2_inner(ctx, bytes.get(11..)?, depth + 1))?;
            let (lsb, lsb_len) = propagate_resource!(decode_int32_cdp2_inner(
                ctx,
                bytes.get(11 + msb_len..)?,
                depth + 1
            ))?;
            if msb.len() != value_count || lsb.len() != value_count {
                return None;
            }
            let shift = span_bits - chop_bits;
            let low_mask = if shift == 32 {
                u32::MAX
            } else {
                (1_u32 << shift) - 1
            };
            propagate_resource!(ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(value_count),
                "validate JT low symbols"
            ));
            if lsb.iter().any(|value| {
                u32::try_from(*value)
                    .ok()
                    .is_none_or(|value| value > low_mask)
            }) {
                return None;
            }
            let (mut values, reservation) =
                propagate_resource!(ctx.temporary_vec(value_count, "nx JT decoded vector"));
            propagate_resource!(ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(value_count),
                "combine JT chopped symbols"
            ));
            for (high, low) in msb.iter().copied().zip(lsb.iter().copied()) {
                let high = high.checked_shl(u32::from(shift))?;
                // wrapping-exception: JT chopped Int32 symbols add bias modulo 2^32
                values.push((low | high).wrapping_add(bias));
            }
            return Some(Ok((
                ScratchLane {
                    values,
                    reservation,
                },
                11 + msb_len + lsb_len,
            )));
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
            let values =
                propagate_resource!(decode_bitlength(ctx, code_words, code_bit_len, value_count))?;
            return Some(Ok((values, cursor)));
        }
        let (entries, context_len, _entries_reservation) =
            propagate_resource!(parse_probability_context(ctx, bytes.get(cursor..)?))?;
        cursor += context_len;
        let symbols = propagate_resource!(decode_arithmetic(
            ctx,
            code_words,
            code_bit_len,
            value_count,
            &entries
        ))?;
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(symbols.len()),
            "count JT escape symbols"
        ));
        let escape_count = symbols.iter().filter(|value| value.is_none()).count();
        let (out_of_band, oob_len) = propagate_resource!(decode_int32_cdp2_inner(
            ctx,
            bytes.get(cursor..)?,
            depth + 1
        ))?;
        if out_of_band.len() != escape_count {
            return None;
        }
        cursor += oob_len;
        let mut out_of_band = out_of_band.iter().copied();
        let (mut values, reservation) =
            propagate_resource!(ctx.temporary_vec(value_count, "nx JT decoded vector"));
        propagate_resource!(ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(value_count),
            "form JT arithmetic values"
        ));
        for value in symbols.iter().copied() {
            values.push(value.or_else(|| out_of_band.next())?);
        }
        Some(Ok((
            ScratchLane {
                values,
                reservation,
            },
            cursor,
        )))
    })();
    decoded.transpose()
}

#[cfg(test)]
mod tests;
