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
    if predictor == Predictor::Null {
        let (values, reservation) = ctx.copy_temporary_slice(residuals, "JT predictor values")?;
        return Ok(ScratchLane {
            values,
            reservation,
        });
    }
    let (mut values, reservation) = ctx.temporary_vec(residuals.len(), "JT predictor values")?;
    let mut visits = residuals.iter().enumerate();
    while let Some((index, &residual)) =
        ctx.next_charged(&mut visits, "unpack JT predictor residuals")?
    {
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
    if exponents.len() != mantissae.len() {
        return Ok(None);
    }
    let (mut values, reservation) = ctx.temporary_vec(exponents.len(), "JT lossless component")?;
    let mut visits = exponents.iter().zip(mantissae.iter());
    while let Some((&exponent, &mantissa)) =
        ctx.next_charged(&mut visits, "form JT lossless exponents")?
    {
        let exponent = exponent.cast_unsigned() & 0x1ff;
        let mantissa = mantissa.cast_unsigned() & 0x7f_ffff;
        let value = f32::from_bits((exponent << 23) | mantissa);
        values.push(match FiniteBinary32::new(value) {
            Some(value) => value,
            None => return Ok(None),
        });
    }
    Ok(Some(ScratchLane {
        values,
        reservation,
    }))
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

/// Decode one JT compressed normal array and its trailing hash.
pub(crate) fn decode_vertex_normals(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 3]>>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, 0).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if count != expected_count
        || *(match bytes.get(4) {
            Some(value) => value,
            None => return Ok(None),
        }) != 3
        || *(match bytes.get(5) {
            Some(value) => value,
            None => return Ok(None),
        }) != expected_bits
    {
        return Ok(None);
    }
    let mut cursor = 6usize;
    let normals = if expected_bits == 0 {
        let (mut components, _components_reservation) = ctx.temporary_vec(3, "JT normal array")?;
        for _ in 0..3 {
            let Some((exponents, exponent_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(exponent_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            let Some((mantissae, mantissa_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(mantissa_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if exponents.len() != count || mantissae.len() != count {
                return Ok(None);
            }
            components.push(
                match lossless_coordinate_component(ctx, &exponents, &mantissae)? {
                    Some(value) => value,
                    None => return Ok(None),
                },
            );
        }
        let mut normals = ctx.collection_vec(count, "JT normal array")?;
        let mut visits = (components[0].values.iter())
            .zip(components[1].values.iter())
            .zip(components[2].values.iter());
        while let Some(((x, y), z)) = ctx.next_charged(&mut visits, "form JT normal x values")? {
            normals.push([*x, *y, *z]);
        }
        normals
    } else {
        let (mut codes, _codes_reservation) = ctx.temporary_vec(4, "JT normal array")?;
        for _ in 0..4 {
            let Some((values, byte_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(byte_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if values.len() != count {
                return Ok(None);
            }
            codes.push(values);
        }
        let Some(bits) = NormalBits::new(expected_bits) else {
            return Ok(None);
        };
        let mut normals = ctx.collection_vec(count, "JT normal array")?;
        let mut visits = (codes[0].values.iter())
            .zip(codes[1].values.iter())
            .zip(codes[2].values.iter())
            .zip(codes[3].values.iter());
        while let Some((((sextant, octant), theta), psi)) =
            ctx.next_charged(&mut visits, "form JT normal sextants")?
        {
            normals.push(
                match deering_normal(
                    match Sextant::from_index(*sextant) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                    match Octant::new(*octant) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                    match NormalCode::new(*theta, bits) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                    match NormalCode::new(*psi, bits) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                ) {
                    Some(value) => value,
                    None => return Ok(None),
                },
            );
        }
        normals
    };
    let Some(hash) = View::u32_le_at(bytes, cursor) else {
        return Ok(None);
    };
    let Some(next_cursor) = cursor.checked_add(4) else {
        return Ok(None);
    };
    cursor = next_cursor;
    Ok(Some(DecodedVertexArray {
        values: normals,
        hash,
        byte_len: cursor,
    }))
}

/// Decode one JT compressed texture-coordinate array and its trailing hash.
pub(crate) fn decode_vertex_texture_coordinates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<Vec<FiniteBinary32>>>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, 0).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    let component_count = usize::from(
        *(match bytes.get(4) {
            Some(value) => value,
            None => return Ok(None),
        }),
    );
    if count != expected_count
        || !(1..=4).contains(&component_count)
        || *(match bytes.get(5) {
            Some(value) => value,
            None => return Ok(None),
        }) != expected_bits
        || expected_bits > 24
    {
        return Ok(None);
    }
    let mut cursor = 6usize;
    let (mut components, _components_reservation) =
        ctx.temporary_vec(component_count, "JT texture array")?;
    if expected_bits == 0 {
        for _ in 0..component_count {
            let Some((exponents, exponent_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(exponent_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            let Some((mantissae, mantissa_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(mantissa_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if exponents.len() != count || mantissae.len() != count {
                return Ok(None);
            }
            components.push(
                match lossless_coordinate_component(ctx, &exponents, &mantissae)? {
                    Some(value) => value,
                    None => return Ok(None),
                },
            );
        }
    } else {
        let (mut ranges, _ranges_reservation) =
            ctx.temporary_vec(component_count, "JT texture array")?;
        for _ in 0..component_count {
            let Some(minimum) = View::f32_le_at(bytes, cursor) else {
                return Ok(None);
            };
            let Some(maximum) = View::f32_le_at(bytes, cursor + 4) else {
                return Ok(None);
            };
            let bits = *(match bytes.get(cursor + 8) {
                Some(value) => value,
                None => return Ok(None),
            });
            if bits != expected_bits {
                return Ok(None);
            }
            ranges.push(match QuantizedRange::new(minimum, maximum) {
                Some(value) => value,
                None => return Ok(None),
            });
            let Some(next_cursor) = cursor.checked_add(9) else {
                return Ok(None);
            };
            cursor = next_cursor;
        }
        for range in ranges.iter().copied() {
            let Some((residuals, byte_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(byte_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if residuals.len() != count {
                return Ok(None);
            }
            let (mut component, reservation) = ctx.temporary_vec(count, "JT texture array")?;
            let predicted = unpack_predictor_scratch(ctx, &residuals, Predictor::Lag1)?;
            let mut visits = (predicted.values.iter()).copied();
            while let Some(code) = ctx.next_charged(&mut visits, "JT predicted value traversal")? {
                component.push(
                    match dequantize_uniform(
                        match u32::try_from(code).ok() {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                        range,
                        expected_bits,
                    ) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                );
            }
            components.push(ScratchLane {
                values: component,
                reservation,
            });
        }
    }
    let Some(hash) = View::u32_le_at(bytes, cursor) else {
        return Ok(None);
    };
    let Some(next_cursor) = cursor.checked_add(4) else {
        return Ok(None);
    };
    cursor = next_cursor;
    let mut values = ctx.collection_vec(count, "JT texture array")?;
    let mut visits = 0..count;
    while let Some(index) = ctx.next_charged(
        &mut visits,
        "NX decode vertex texture coordinates inner range traversal",
    )? {
        let mut value = ctx.collection_vec(component_count, "JT texture array")?;
        for component in 0..component_count {
            value.push(
                match components
                    .get(component)
                    .and_then(|component| component.get(index))
                    .copied()
                {
                    Some(value) => value,
                    None => return Ok(None),
                },
            );
        }
        values.push(value);
    }
    Ok(Some(DecodedVertexArray {
        values,
        hash,
        byte_len: cursor,
    }))
}

/// Decode one JT compressed color array as RGBA values and its trailing hash.
pub(crate) fn decode_vertex_colors(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected_count: usize,
    expected_bits: u8,
) -> Result<Option<DecodedVertexArray<[FiniteBinary32; 4]>>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, 0).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    let component_count = usize::from(
        *(match bytes.get(4) {
            Some(value) => value,
            None => return Ok(None),
        }),
    );
    if count != expected_count
        || !matches!(component_count, 3 | 4)
        || *(match bytes.get(5) {
            Some(value) => value,
            None => return Ok(None),
        }) != expected_bits
        || expected_bits > 8
    {
        return Ok(None);
    }
    let mut cursor = 6usize;
    let colors = if expected_bits == 0 {
        let (mut components, _components_reservation) =
            ctx.temporary_vec(component_count, "JT color array")?;
        for _ in 0..component_count {
            let Some((exponents, exponent_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(exponent_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            let Some((mantissae, mantissa_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(mantissa_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if exponents.len() != count || mantissae.len() != count {
                return Ok(None);
            }
            let exponents = unpack_predictor_scratch(ctx, &exponents, Predictor::Lag1)?;
            let mantissae = unpack_predictor_scratch(ctx, &mantissae, Predictor::Lag1)?;
            components.push(
                match lossless_coordinate_component(ctx, &exponents, &mantissae)? {
                    Some(value) => value,
                    None => return Ok(None),
                },
            );
        }
        let mut colors = ctx.collection_vec(count, "JT color array")?;
        let mut visits = 0..count;
        while let Some(index) =
            ctx.next_charged(&mut visits, "NX decode vertex colors inner range traversal")?
        {
            colors.push([
                *(match components
                    .first()
                    .and_then(|component| component.get(index))
                {
                    Some(value) => value,
                    None => return Ok(None),
                }),
                *(match components.get(1).and_then(|component| component.get(index)) {
                    Some(value) => value,
                    None => return Ok(None),
                }),
                *(match components.get(2).and_then(|component| component.get(index)) {
                    Some(value) => value,
                    None => return Ok(None),
                }),
                components
                    .get(3)
                    .and_then(|component| component.get(index))
                    .copied()
                    .unwrap_or(FiniteBinary32::ONE),
            ]);
        }
        colors
    } else {
        let hsv = match *(match bytes.get(cursor) {
            Some(value) => value,
            None => return Ok(None),
        }) {
            0 => false,
            1 => true,
            _ => return Ok(None),
        };
        let Some(next_cursor) = cursor.checked_add(1) else {
            return Ok(None);
        };
        cursor = next_cursor;
        let (mut ranges, _ranges_reservation) = ctx.temporary_vec(4, "JT color array")?;
        let (mut component_bits, _bits_reservation) = ctx.temporary_vec(4, "JT color array")?;
        if hsv {
            for range in [[0.0, 6.0], [0.0, 1.0], [0.0, 1.0], [0.0, 1.0]] {
                let bits = *(match bytes.get(cursor) {
                    Some(value) => value,
                    None => return Ok(None),
                });
                if bits == 0 || bits > 8 {
                    return Ok(None);
                }
                ranges.push(match QuantizedRange::new(range[0], range[1]) {
                    Some(value) => value,
                    None => return Ok(None),
                });
                component_bits.push(bits);
                let Some(next_cursor) = cursor.checked_add(1) else {
                    return Ok(None);
                };
                cursor = next_cursor;
            }
        } else {
            for _ in 0..4 {
                let Some(minimum) = View::f32_le_at(bytes, cursor) else {
                    return Ok(None);
                };
                let Some(maximum) = View::f32_le_at(bytes, cursor + 4) else {
                    return Ok(None);
                };
                let bits = *(match bytes.get(cursor + 8) {
                    Some(value) => value,
                    None => return Ok(None),
                });
                if bits == 0 || bits > 8 {
                    return Ok(None);
                }
                ranges.push(match QuantizedRange::new(minimum, maximum) {
                    Some(value) => value,
                    None => return Ok(None),
                });
                component_bits.push(bits);
                let Some(next_cursor) = cursor.checked_add(9) else {
                    return Ok(None);
                };
                cursor = next_cursor;
            }
        }
        let (mut components, _components_reservation) = ctx.temporary_vec(4, "JT color array")?;
        for component in 0..4 {
            let Some((residuals, byte_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(byte_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if residuals.len() != count {
                return Ok(None);
            }
            let (mut values, reservation) = ctx.temporary_vec(count, "JT color array")?;
            let predicted = unpack_predictor_scratch(ctx, &residuals, Predictor::Lag1)?;
            let mut visits = (predicted.values.iter()).copied();
            while let Some(code) = ctx.next_charged(&mut visits, "JT predicted value traversal")? {
                values.push(
                    match dequantize_uniform(
                        match u32::try_from(code).ok() {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                        *(match ranges.get(component) {
                            Some(value) => value,
                            None => return Ok(None),
                        }),
                        *(match component_bits.get(component) {
                            Some(value) => value,
                            None => return Ok(None),
                        }),
                    ) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                );
            }
            components.push(ScratchLane {
                values,
                reservation,
            });
        }
        let mut colors = ctx.collection_vec(count, "JT color array")?;
        let mut visits = 0..count;
        while let Some(index) =
            ctx.next_charged(&mut visits, "NX decode vertex colors inner range traversal")?
        {
            let first = *(match components
                .first()
                .and_then(|component| component.get(index))
            {
                Some(value) => value,
                None => return Ok(None),
            });
            let second = *(match components.get(1).and_then(|component| component.get(index)) {
                Some(value) => value,
                None => return Ok(None),
            });
            let third = *(match components.get(2).and_then(|component| component.get(index)) {
                Some(value) => value,
                None => return Ok(None),
            });
            let alpha = *(match components.get(3).and_then(|component| component.get(index)) {
                Some(value) => value,
                None => return Ok(None),
            });
            if hsv {
                let Some([red, green, blue]) = hsv_to_rgb(first, second, third) else {
                    return Ok(None);
                };
                colors.push([red, green, blue, alpha]);
            } else {
                colors.push([first, second, third, alpha]);
            }
        }
        colors
    };
    let Some(hash) = View::u32_le_at(bytes, cursor) else {
        return Ok(None);
    };
    let Some(next_cursor) = cursor.checked_add(4) else {
        return Ok(None);
    };
    cursor = next_cursor;
    Ok(Some(DecodedVertexArray {
        values: colors,
        hash,
        byte_len: cursor,
    }))
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
    let Some(count) = View::u32_le_at(bytes, 0).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if count != expected_count {
        return Ok(None);
    }
    let Some((values, byte_len)) = decode_int32_cdp2_inner(
        ctx,
        match bytes.get(4..) {
            Some(value) => value,
            None => return Ok(None),
        },
        0,
    )?
    else {
        return Ok(None);
    };
    if values.len() != count {
        return Ok(None);
    }
    let mut flags = ctx.collection_vec(count, "JT flag array")?;
    let mut visits = (values.values.iter()).copied();
    while let Some(value) = ctx.next_charged(&mut visits, "JT flag value traversal")? {
        flags.push(
            match u32::try_from(value).ok().filter(|value| *value <= 1) {
                Some(value) => value,
                None => return Ok(None),
            },
        );
    }
    Ok(Some((
        flags,
        match 4usize.checked_add(byte_len) {
            Some(value) => value,
            None => return Ok(None),
        },
    )))
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
    let mut cursor = 0usize;
    let (mut components, _components_reservation) = ctx.temporary_vec(3, "JT coordinate array")?;
    for component in 0..3 {
        if quantization_bits[component] == 0 {
            let Some((exponent_residuals, exponent_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(exponent_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            let Some((mantissa_residuals, mantissa_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(mantissa_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if exponent_residuals.len() != vertex_count || mantissa_residuals.len() != vertex_count
            {
                return Ok(None);
            }
            components.push(
                match lossless_coordinate_component(
                    ctx,
                    &(unpack_predictor_scratch(ctx, &exponent_residuals, Predictor::Lag1)?),
                    &(unpack_predictor_scratch(ctx, &mantissa_residuals, Predictor::Lag1)?),
                )? {
                    Some(value) => value,
                    None => return Ok(None),
                },
            );
        } else {
            let Some((residuals, byte_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(cursor..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                0,
            )?
            else {
                return Ok(None);
            };
            let Some(next_cursor) = cursor.checked_add(byte_len) else {
                return Ok(None);
            };
            cursor = next_cursor;
            if residuals.len() != vertex_count {
                return Ok(None);
            }
            let (mut values, reservation) =
                ctx.temporary_vec(vertex_count, "JT coordinate array")?;
            let predicted = unpack_predictor_scratch(ctx, &residuals, Predictor::Lag1)?;
            let mut visits = (predicted.values.iter()).copied();
            while let Some(code) = ctx.next_charged(&mut visits, "JT predicted value traversal")? {
                values.push(
                    match dequantize_uniform(
                        match u32::try_from(code).ok() {
                            Some(value) => value,
                            None => return Ok(None),
                        },
                        ranges[component],
                        quantization_bits[component],
                    ) {
                        Some(value) => value,
                        None => return Ok(None),
                    },
                );
            }
            components.push(ScratchLane {
                values,
                reservation,
            });
        }
    }
    let Some(coordinate_hash) = View::u32_le_at(bytes, cursor) else {
        return Ok(None);
    };
    let Some(next_cursor) = cursor.checked_add(4) else {
        return Ok(None);
    };
    cursor = next_cursor;
    let mut points = ctx.collection_vec(vertex_count, "JT coordinate array")?;
    let mut visits = 0..vertex_count;
    while let Some(index) = ctx.next_charged(
        &mut visits,
        "NX decode vertex coordinates inner range traversal",
    )? {
        points.push([
            *(match components
                .first()
                .and_then(|component| component.get(index))
            {
                Some(value) => value,
                None => return Ok(None),
            }),
            *(match components.get(1).and_then(|component| component.get(index)) {
                Some(value) => value,
                None => return Ok(None),
            }),
            *(match components.get(2).and_then(|component| component.get(index)) {
                Some(value) => value,
                None => return Ok(None),
            }),
        ]);
    }
    Ok(Some(DecodedVertexArray {
        values: points,
        hash: coordinate_hash,
        byte_len: cursor,
    }))
}

/// Bound one complete JT Int32 Compressed Data Packet Mk. 2 without interpreting its symbols.
pub(crate) fn frame_int32_cdp2(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(u32, u8, usize)>, CodecError> {
    if depth > 3 {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("frame JT integer packet")?;
    let Some(value_count) = View::u32_le_at(bytes, 0) else {
        return Ok(None);
    };
    if match usize::try_from(value_count).ok() {
        Some(value) => value,
        None => return Ok(None),
    } > MAX_ARITHMETIC_VALUES
    {
        return Ok(None);
    }
    if value_count == 0 {
        return Ok(Some((0, 0, 4)));
    }
    let Some(&codec) = bytes.get(4) else {
        return Ok(None);
    };
    if codec == 4 {
        let Some(&chop_bits) = bytes.get(5) else {
            return Ok(None);
        };
        if chop_bits == 0 {
            let Some((nested_count, _, nested_len)) = frame_int32_cdp2(
                ctx,
                match bytes.get(6..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                depth + 1,
            )?
            else {
                return Ok(None);
            };
            return Ok((nested_count == value_count).then_some((
                value_count,
                codec,
                6 + nested_len,
            )));
        }
        let Some(&span_bits) = bytes.get(10) else {
            return Ok(None);
        };
        if chop_bits > span_bits || span_bits > 32 {
            return Ok(None);
        }
        let Some((msb_count, _, msb_len)) = frame_int32_cdp2(
            ctx,
            match bytes.get(11..) {
                Some(value) => value,
                None => return Ok(None),
            },
            depth + 1,
        )?
        else {
            return Ok(None);
        };
        let Some((lsb_count, _, lsb_len)) = frame_int32_cdp2(
            ctx,
            match bytes.get(11 + msb_len..) {
                Some(value) => value,
                None => return Ok(None),
            },
            depth + 1,
        )?
        else {
            return Ok(None);
        };
        return Ok(
            (msb_count == value_count && lsb_count == value_count).then_some((
                value_count,
                codec,
                11 + msb_len + lsb_len,
            )),
        );
    }
    if !matches!(codec, 1 | 3) {
        return Ok(None);
    }
    let Some(code_bit_len) = usize::try_from(match View::u32_le_at(bytes, 5) {
        Some(value) => value,
        None => return Ok(None),
    })
    .ok() else {
        return Ok(None);
    };
    let Some(code_byte_len) = code_bit_len.div_ceil(32).checked_mul(4) else {
        return Ok(None);
    };
    let Some(mut cursor) = 9_usize.checked_add(code_byte_len) else {
        return Ok(None);
    };
    match bytes.get(..cursor) {
        Some(value) => value,
        None => return Ok(None),
    };
    if codec == 1 {
        return Ok(Some((value_count, codec, cursor)));
    }
    let Some((entries, context_len)) = parse_probability_context(
        ctx,
        match bytes.get(cursor..) {
            Some(value) => value,
            None => return Ok(None),
        },
    )?
    else {
        return Ok(None);
    };
    let Some(next_cursor) = cursor.checked_add(context_len) else {
        return Ok(None);
    };
    cursor = next_cursor;
    let Some(code_words) = bytes.get(9..9 + code_byte_len) else {
        return Ok(None);
    };
    let Some(symbols) = decode_arithmetic(
        ctx,
        code_words,
        code_bit_len,
        match usize::try_from(value_count).ok() {
            Some(value) => value,
            None => return Ok(None),
        },
        &entries.values,
    )?
    else {
        return Ok(None);
    };
    let escape_count = (ctx.admit_iter(&symbols.values, "count JT escape symbols")?)
        .filter(|value| value.is_none())
        .count();
    let Some((out_of_band_count, _, out_of_band_len)) = frame_int32_cdp2(
        ctx,
        match bytes.get(cursor..) {
            Some(value) => value,
            None => return Ok(None),
        },
        depth + 1,
    )?
    else {
        return Ok(None);
    };
    if match usize::try_from(out_of_band_count).ok() {
        Some(value) => value,
        None => return Ok(None),
    } != escape_count
    {
        return Ok(None);
    }
    let Some(next_cursor) = cursor.checked_add(out_of_band_len) else {
        return Ok(None);
    };
    cursor = next_cursor;
    Ok(Some((value_count, codec, cursor)))
}

fn parse_probability_context<'a>(
    ctx: &'a DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<(ScratchLane<'a, ProbabilityEntry>, usize)>, CodecError> {
    let entry_count = usize::from(match View::u16_be_at(bytes, 0) {
        Some(value) => value,
        None => return Ok(None),
    });
    let mut bits = MsbBitReader::new(match bytes.get(2..) {
        Some(value) => value,
        None => return Ok(None),
    });
    let Some(symbol_bits) = u8::try_from(match bits.read(6) {
        Some(value) => value,
        None => return Ok(None),
    })
    .ok() else {
        return Ok(None);
    };
    let Some(occurrence_bits) = u8::try_from(match bits.read(6) {
        Some(value) => value,
        None => return Ok(None),
    })
    .ok() else {
        return Ok(None);
    };
    let Some(value_bits) = u8::try_from(match bits.read(6) {
        Some(value) => value,
        None => return Ok(None),
    })
    .ok() else {
        return Ok(None);
    };
    let minimum = (match bits.read(32) {
        Some(value) => value,
        None => return Ok(None),
    })
    .cast_signed();
    if symbol_bits > 32 || occurrence_bits > 32 || value_bits > 32 {
        return Ok(None);
    }
    let Some(entry_bits) =
        (match usize::from(symbol_bits).checked_add(usize::from(occurrence_bits)) {
            Some(value) => value,
            None => return Ok(None),
        })
        .checked_add(usize::from(value_bits))
    else {
        return Ok(None);
    };
    let Some(available_bits) = (match match bytes.get(2..) {
        Some(value) => value,
        None => return Ok(None),
    }
    .len()
    .checked_mul(8)
    {
        Some(value) => value,
        None => return Ok(None),
    })
    .checked_sub(bits.bit) else {
        return Ok(None);
    };
    // Prove the declared table fits in the bounded context before allocating
    // its entries. An all-zero-width context cannot produce a nonzero
    // arithmetic frequency table and is therefore malformed.
    if entry_bits == 0 || entry_count > available_bits / entry_bits {
        return Ok(None);
    }
    let (mut entries, reservation) = ctx.temporary_vec(entry_count, "JT probability entries")?;
    let mut visits = 0..entry_count;
    while ctx
        .next_charged(&mut visits, "parse JT probability context")?
        .is_some()
    {
        let Some(symbol) = i32::try_from(
            match i64::from(match bits.read(symbol_bits) {
                Some(value) => value,
                None => return Ok(None),
            })
            .checked_sub(2)
            {
                Some(value) => value,
                None => return Ok(None),
            },
        )
        .ok() else {
            return Ok(None);
        };
        let Some(occurrence_count) = bits.read(occurrence_bits) else {
            return Ok(None);
        };
        let Some(raw_value) = bits.read(value_bits) else {
            return Ok(None);
        };
        // wrapping-exception: JT Int32 predictor and packet integer addition is modulo 2^32
        let value = raw_value.cast_signed().wrapping_add(minimum);
        entries.push(ProbabilityEntry {
            symbol,
            occurrence_count,
            value,
        });
    }
    let Some(bit_bytes) = bits.finish_zero_padding() else {
        return Ok(None);
    };
    Ok(Some((
        ScratchLane {
            values: entries,
            reservation,
        },
        2 + bit_bytes,
    )))
}

struct CodeBits<'a> {
    words: &'a [u8],
    bit_len: usize,
    bit: usize,
}

impl CodeBits<'_> {
    fn read(&mut self, count: u8) -> Option<u32> {
        if count > 32 {
            return None;
        }
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
    // Arithmetic symbols can consume zero code bits, so the stream length puts
    // no floor under `value_count`; an absolute cap bounds the allocation and
    // the per-value decode work instead.
    if value_count > MAX_ARITHMETIC_VALUES {
        return Ok(None);
    }
    let Some(work) = entries.len().checked_mul(value_count) else {
        return Err(ctx.refuse_codec_limit("decode JT arithmetic symbols", u64::MAX - 1, u64::MAX));
    };
    if work > MAX_ARITHMETIC_WORK {
        return Err(ctx.refuse_codec_limit(
            "decode JT arithmetic symbols",
            cadmpeg_core::decode::u64_from_index(MAX_ARITHMETIC_WORK),
            cadmpeg_core::decode::u64_from_index(work),
        ));
    }
    let mut total = 0u32;
    let mut entries_to_sum = entries.iter();
    while let Some(entry) =
        ctx.next_charged(&mut entries_to_sum, "JT probability total traversal")?
    {
        let Some(sum) = total.checked_add(entry.occurrence_count) else {
            return Ok(None);
        };
        total = sum;
    }
    if total == 0 || total > u32::from(u16::MAX) {
        return Ok(None);
    }
    let mut bits = CodeBits {
        words: code_words,
        bit_len: code_bit_len,
        bit: 0,
    };
    let mut code = 0u16;
    for _ in 0..16 {
        code = (code << 1)
            | (match bits.next() {
                Some(value) => value,
                None => return Ok(None),
            });
    }
    let mut low = 0u16;
    let mut high = u16::MAX;
    let (mut values, reservation) = ctx.temporary_vec(value_count, "JT arithmetic symbols")?;
    let mut visits = 0..value_count;
    while ctx
        .next_charged(&mut visits, "NX decode arithmetic range traversal")?
        .is_some()
    {
        let range = u32::from(match high.checked_sub(low) {
            Some(value) => value,
            None => return Ok(None),
        }) + 1;
        let scaled = ((u32::from(match code.checked_sub(low) {
            Some(value) => value,
            None => return Ok(None),
        }) + 1)
            * total
            - 1)
            / range;
        let mut cumulative = 0u32;
        let Some(entry) = ctx.find_by(
            entries,
            |entry| {
                let end = cumulative + entry.occurrence_count;
                let contains = scaled >= cumulative && scaled < end;
                if !contains {
                    cumulative = end;
                }
                Ok(contains)
            },
            "JT probability entry search",
        )?
        else {
            return Ok(None);
        };
        let entry_high = cumulative + entry.occurrence_count;
        let Some(next_high) =
            low.checked_add(match u16::try_from((range * entry_high) / total - 1).ok() {
                Some(value) => value,
                None => return Ok(None),
            })
        else {
            return Ok(None);
        };
        high = next_high;
        let Some(next_low) =
            low.checked_add(match u16::try_from((range * cumulative) / total).ok() {
                Some(value) => value,
                None => return Ok(None),
            })
        else {
            return Ok(None);
        };
        low = next_low;
        loop {
            ctx.charge_work(1, "normalize JT arithmetic code")?;
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
            let Some(next_bit) = bits.next() else {
                return Ok(None);
            };
            // wrapping-exception: JT arithmetic normalization uses a sixteen-bit code register
            code = code.wrapping_shl(1) | next_bit;
        }
        values.push(if entry.symbol == -2 {
            None
        } else {
            Some(entry.value)
        });
    }
    Ok(Some(ScratchLane {
        values,
        reservation,
    }))
}

fn decode_bitlength<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    code_words: &[u8],
    code_bit_len: usize,
    value_count: usize,
) -> Result<Option<ScratchLane<'ctx, i32>>, CodecError> {
    let mut bits = CodeBits {
        words: code_words,
        bit_len: code_bit_len,
        bit: 0,
    };
    let Some(value_count) = cadmpeg_core::decode::bounded_len(
        cadmpeg_core::decode::u64_from_index(value_count),
        1,
        MAX_ARITHMETIC_VALUES,
    ) else {
        return Ok(None);
    };
    let (mut values, reservation) = ctx.temporary_vec(value_count, "JT bitlength values")?;
    if match bits.read(1) {
        Some(value) => value,
        None => return Ok(None),
    } == 0
    {
        let Some(minimum_bits) = u8::try_from(match bits.read(6) {
            Some(value) => value,
            None => return Ok(None),
        })
        .ok() else {
            return Ok(None);
        };
        let Some(maximum_bits) = u8::try_from(match bits.read(6) {
            Some(value) => value,
            None => return Ok(None),
        })
        .ok() else {
            return Ok(None);
        };
        if minimum_bits > 32 || maximum_bits > 32 {
            return Ok(None);
        }
        let Some(minimum) = bits.read_signed(minimum_bits) else {
            return Ok(None);
        };
        let Some(maximum) = bits.read_signed(maximum_bits) else {
            return Ok(None);
        };
        if maximum < minimum {
            return Ok(None);
        }
        let Some(span) = u32::try_from(i64::from(maximum) - i64::from(minimum)).ok() else {
            return Ok(None);
        };
        let width = if span == 0 {
            0
        } else {
            match u8::try_from(u32::BITS - span.leading_zeros()).ok() {
                Some(value) => value,
                None => return Ok(None),
            }
        };
        let mut visits = 0..value_count;
        while ctx
            .next_charged(&mut visits, "decode JT bitlength symbols")?
            .is_some()
        {
            let Some(code) = bits.read(width) else {
                return Ok(None);
            };
            let value = i64::from(minimum) + i64::from(code);
            if value > i64::from(maximum) {
                return Ok(None);
            }
            values.push(match i32::try_from(value).ok() {
                Some(value) => value,
                None => return Ok(None),
            });
        }
    } else {
        let Some(mean) = bits.read_signed(32) else {
            return Ok(None);
        };
        let Some(delta_bits) = u8::try_from(match bits.read(3) {
            Some(value) => value,
            None => return Ok(None),
        })
        .ok() else {
            return Ok(None);
        };
        let Some(run_bits) = u8::try_from(match bits.read(3) {
            Some(value) => value,
            None => return Ok(None),
        })
        .ok() else {
            return Ok(None);
        };
        if delta_bits == 0 || run_bits == 0 {
            return Ok(None);
        }
        let minimum_delta = -(1_i32 << (delta_bits - 1));
        let maximum_delta = (1_i32 << (delta_bits - 1)) - 1;
        let mut width = 0i32;
        while values.len() < value_count {
            ctx.charge_work(1, "read JT bitlength runs")?;
            loop {
                ctx.charge_work(1, "read JT bitlength deltas")?;
                let Some(delta) = bits.read_signed(delta_bits) else {
                    return Ok(None);
                };
                let Some(next_width) = width.checked_add(delta) else {
                    return Ok(None);
                };
                width = next_width;
                if !(0..=32).contains(&width) {
                    return Ok(None);
                }
                if delta != minimum_delta && delta != maximum_delta {
                    break;
                }
            }
            let Some(run) = usize::try_from(match bits.read(run_bits) {
                Some(value) => value,
                None => return Ok(None),
            })
            .ok() else {
                return Ok(None);
            };
            if run == 0
                || (match values.len().checked_add(run) {
                    Some(value) => value,
                    None => return Ok(None),
                }) > value_count
            {
                return Ok(None);
            }
            let mut visits = 0..run;
            while ctx
                .next_charged(&mut visits, "decode JT bitlength symbols")?
                .is_some()
            {
                let Ok(symbol_width) = u8::try_from(width) else {
                    return Ok(None);
                };
                let Some(residual) = bits.read_signed(symbol_width) else {
                    return Ok(None);
                };
                // wrapping-exception: JT Int32 predictor and packet integer addition is modulo 2^32
                values.push(mean.wrapping_add(residual));
            }
        }
    }
    Ok((bits.bit == code_bit_len).then_some(ScratchLane {
        values,
        reservation,
    }))
}

/// Decode one complete JT Int32 Compressed Data Packet Mk. 2.
pub(crate) fn decode_int32_cdp2(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(Vec<i32>, usize)>, CodecError> {
    let Some((values, length)) = decode_int32_cdp2_inner(ctx, bytes, depth)? else {
        return Ok(None);
    };
    Ok(Some((values.into_retained()?, length)))
}

fn decode_int32_cdp2_inner<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    depth: u8,
) -> Result<Option<(ScratchLane<'ctx, i32>, usize)>, CodecError> {
    if depth > 3 {
        return Ok(None);
    }
    let _depth = ctx.enter_nested("decode JT integer packet")?;
    let Some(value_count) = View::u32_le_at(bytes, 0).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if value_count > MAX_ARITHMETIC_VALUES {
        return Ok(None);
    }
    if value_count == 0 {
        return Ok(Some((
            ScratchLane {
                values: Vec::new(),
                reservation: ctx.reserve_scoped(0, "JT packet symbols")?,
            },
            4,
        )));
    }
    let Some(&codec) = bytes.get(4) else {
        return Ok(None);
    };
    if codec == 4 {
        let Some(&chop_bits) = bytes.get(5) else {
            return Ok(None);
        };
        if chop_bits == 0 {
            let Some((values, nested_len)) = decode_int32_cdp2_inner(
                ctx,
                match bytes.get(6..) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                depth + 1,
            )?
            else {
                return Ok(None);
            };
            return Ok((values.len() == value_count).then_some((values, 6 + nested_len)));
        }
        let bias = (match View::u32_le_at(bytes, 6) {
            Some(value) => value,
            None => return Ok(None),
        })
        .cast_signed();
        let Some(&span_bits) = bytes.get(10) else {
            return Ok(None);
        };
        if chop_bits > span_bits || span_bits > 32 {
            return Ok(None);
        }
        let Some((msb, msb_len)) = decode_int32_cdp2_inner(
            ctx,
            match bytes.get(11..) {
                Some(value) => value,
                None => return Ok(None),
            },
            depth + 1,
        )?
        else {
            return Ok(None);
        };
        let Some((lsb, lsb_len)) = decode_int32_cdp2_inner(
            ctx,
            match bytes.get(11 + msb_len..) {
                Some(value) => value,
                None => return Ok(None),
            },
            depth + 1,
        )?
        else {
            return Ok(None);
        };
        if msb.len() != value_count || lsb.len() != value_count {
            return Ok(None);
        }
        let shift = span_bits - chop_bits;
        let low_mask = if shift == 32 {
            u32::MAX
        } else {
            (1_u32 << shift) - 1
        };
        if ctx.any_by(
            &lsb.values,
            |value| {
                Ok(u32::try_from(*value)
                    .ok()
                    .is_none_or(|value| value > low_mask))
            },
            "validate JT low symbols",
        )? {
            return Ok(None);
        }
        let (mut values, reservation) = ctx.temporary_vec(value_count, "JT packet symbols")?;
        let mut visits = (msb.values.iter())
            .copied()
            .zip((lsb.values.iter()).copied());
        while let Some((high, low)) = ctx.next_charged(&mut visits, "combine JT high symbols")? {
            let Some(high) = high.checked_shl(u32::from(shift)) else {
                return Ok(None);
            };
            // wrapping-exception: JT chopped Int32 symbols add bias modulo 2^32
            values.push((low | high).wrapping_add(bias));
        }
        return Ok(Some((
            ScratchLane {
                values,
                reservation,
            },
            11 + msb_len + lsb_len,
        )));
    }
    if !matches!(codec, 1 | 3) {
        return Ok(None);
    }
    let Some(code_bit_len) = usize::try_from(match View::u32_le_at(bytes, 5) {
        Some(value) => value,
        None => return Ok(None),
    })
    .ok() else {
        return Ok(None);
    };
    let word_count = code_bit_len.div_ceil(32);
    let Some(code_byte_len) = word_count.checked_mul(4) else {
        return Ok(None);
    };
    let Some(code_words) = bytes.get(9..9 + code_byte_len) else {
        return Ok(None);
    };
    let mut cursor = 9 + code_byte_len;
    if codec == 1 {
        let Some(values) = decode_bitlength(ctx, code_words, code_bit_len, value_count)? else {
            return Ok(None);
        };
        return Ok(Some((values, cursor)));
    }
    let Some((entries, context_len)) = parse_probability_context(
        ctx,
        match bytes.get(cursor..) {
            Some(value) => value,
            None => return Ok(None),
        },
    )?
    else {
        return Ok(None);
    };
    cursor += context_len;
    let Some(symbols) =
        decode_arithmetic(ctx, code_words, code_bit_len, value_count, &entries.values)?
    else {
        return Ok(None);
    };
    let escape_count = (ctx.admit_iter(&symbols.values, "count JT escape symbols")?)
        .filter(|value| value.is_none())
        .count();
    let Some((out_of_band, oob_len)) = decode_int32_cdp2_inner(
        ctx,
        match bytes.get(cursor..) {
            Some(value) => value,
            None => return Ok(None),
        },
        depth + 1,
    )?
    else {
        return Ok(None);
    };
    if out_of_band.len() != escape_count {
        return Ok(None);
    }
    cursor += oob_len;
    let mut out_of_band = out_of_band.iter().copied();
    let (mut values, reservation) = ctx.temporary_vec(value_count, "JT packet symbols")?;
    let mut visits = (symbols.values.iter()).copied();
    while let Some(value) = ctx.next_charged(&mut visits, "form JT arithmetic values")? {
        values.push(match value.or_else(|| out_of_band.next()) {
            Some(value) => value,
            None => return Ok(None),
        });
    }
    Ok(Some((
        ScratchLane {
            values,
            reservation,
        },
        cursor,
    )))
}

#[cfg(test)]
mod tests;
