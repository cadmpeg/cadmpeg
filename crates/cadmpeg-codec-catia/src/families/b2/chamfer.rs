// SPDX-License-Identifier: Apache-2.0
//! Compact class-`3b` chamfer records and contextual contact bounds.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::records::{b2_parameter_points_from_records, B2ParameterPoint};
use crate::families::chamfer::{ChamferContactOrder, ChamferParameters, ChamferResultKind};
use crate::wire::bytes::read_f64_array;
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{records_are_contiguous, ConsolidatedFamily, ConsolidatedRecord};

const EPS_CHAMFER_CONTACT_BOUNDS: f64 = 1.0e-12;

pub(crate) struct B2Chamfer {
    pub(crate) pos: usize,
    pub(crate) end: usize,
    pub(crate) support_surfaces: [CompactUnsigned; 2],
    pub(crate) support_pcurves: [CompactUnsigned; 2],
    pub(crate) parameters: ChamferParameters,
}

pub(crate) fn records(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<B2Chamfer>, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "catia_b2_chamfer_scan",
    )?;
    let mut records = Vec::new();
    let Some(last) = bytes.len().checked_sub(5) else {
        return Ok(records);
    };
    for pos in 0..last {
        let Some(width) = bytes[pos]
            .checked_sub(0xb1)
            .filter(|width| (1..=3).contains(width))
        else {
            continue;
        };
        if !matches!(bytes.get(pos + 1), Some(0x03 | 0x13 | 0x83))
            || bytes.get(pos + 2) != Some(&0x3b)
        {
            continue;
        }
        let start = pos + 4 + usize::from(width);
        let end = start + usize::from(bytes[pos + 3]);
        let Some(payload) = bytes.get(start..end) else {
            continue;
        };
        let token = bytes[pos + 4..start]
            .iter()
            .enumerate()
            .fold(0u32, |value, (shift, byte)| {
                value | (u32::from(*byte) << (8 * shift))
            });
        if token != 5 || payload.get(..2) != Some(&[0x85, 0x05]) {
            continue;
        }
        let mut at = 2;
        let Some(decoded) = (|| {
            let support_surfaces = [
                CompactUnsigned::read(payload, &mut at)?,
                CompactUnsigned::read(payload, &mut at)?,
            ];
            let support_pcurves = [
                CompactUnsigned::read(payload, &mut at)?,
                CompactUnsigned::read(payload, &mut at)?,
            ];
            if support_surfaces
                .iter()
                .any(|reference| reference.byte_len() == 1)
                || (support_pcurves[0].byte_len() == 1) != (support_pcurves[1].byte_len() == 1)
                || payload.len().checked_sub(at)? != 22
            {
                return None;
            }
            let controls = payload.get(at..at + 6)?.try_into().ok()?;
            let parameters =
                ChamferParameters::decode(controls, read_f64_array::<2>(payload, at + 6)?)?;
            Some((support_surfaces, support_pcurves, parameters))
        })() else {
            continue;
        };
        ctx.push_vec(
            &mut records,
            B2Chamfer {
                pos,
                end,
                support_surfaces: decoded.0,
                support_pcurves: decoded.1,
                parameters: decoded.2,
            },
            "catia_b2_chamfer_records",
        )?;
    }
    Ok(records)
}

fn direct_line(point: &B2ParameterPoint) -> Option<(u8, [f64; 3])> {
    let control = point.control;
    if control >= 0x20 || !matches!(control & 3, 1 | 2) {
        return None;
    }
    let (values, count) = point.payload.scalars();
    let mut at = 0;
    let tuple = [0x10, 0x04, 0x08].map(|mask| {
        if control & mask != 0 {
            0.0
        } else {
            let value = values[at];
            at += 1;
            value
        }
    });
    (at == count).then_some((control & 3, tuple))
}

fn endpoints(point: &B2ParameterPoint) -> Option<[[f64; 2]; 2]> {
    let (values, _) = point.payload.scalars();
    let ends = match point.control {
        0x08 => [-1.0, 1.0].map(|sign| {
            [
                (sign * values[0]).mul_add(values[3], values[1]),
                (sign * values[0]).mul_add(values[4], values[2]),
            ]
        }),
        0x48 => [
            [values[1], values[2] + values[0]],
            [values[1], values[2] - values[0]],
        ],
        0x88 => [
            [values[1] + values[0], values[2]],
            [values[1] - values[0], values[2]],
        ],
        _ => {
            let (axis, [a, b, c]) = direct_line(point)?;
            if axis == 1 {
                [[a, b], [a, c]]
            } else {
                [[b, a], [c, a]]
            }
        }
    };
    ends.iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(ends)
}

fn close(left: [f64; 2], right: [f64; 2]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(a, b)| (a - b).abs() <= EPS_CHAMFER_CONTACT_BOUNDS * a.abs().max(b.abs()).max(1.0))
}

/// The first two contacts, plus only independently closed connector sides.
pub(crate) fn parameter_positions(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    inventory: &[ConsolidatedRecord],
    construction: &B2Chamfer,
) -> Result<Vec<usize>, CodecError> {
    let mut output = Vec::new();
    let Some(index) = inventory
        .iter()
        .position(|record| record.byte_offset() == construction.pos)
    else {
        return Ok(output);
    };
    if index == 0 {
        return Ok(output);
    }
    let Some(window) = inventory.get(index - 1..index + 3) else {
        return Ok(output);
    };
    let result_class = match construction.parameters.result_kind {
        ChamferResultKind::Cone => 0x29,
        ChamferResultKind::Plane => 0x27,
    };
    if !records_are_contiguous(window)
        || window
            .iter()
            .any(|record| record.family() != ConsolidatedFamily::B)
        || window[0].class() != result_class
        || window[1].class() != 0x3b
        || window[2].class() != 0x18
        || window[3].class() != 0x18
    {
        return Ok(output);
    }
    let mut contacts = b2_parameter_points_from_records(bytes, &window[2..]);
    let (Some(first), Some(second)) = (contacts.next(), contacts.next()) else {
        return Ok(output);
    };
    if [first.operand.value(), second.operand.value()] != [1, 2] {
        return Ok(output);
    }
    let (Some((axis, a)), Some((other_axis, b))) = (direct_line(&first), direct_line(&second))
    else {
        return Ok(output);
    };
    if axis != other_axis
        || a[1..] != b[1..]
        || a[1] >= a[2]
        || match construction.parameters.contact_order {
            ChamferContactOrder::LowerUpper => a[0] >= b[0],
            ChamferContactOrder::UpperLower => a[0] <= b[0],
        }
    {
        return Ok(output);
    }
    ctx.push_vec(&mut output, first.pos, "catia_chamfer_contact_positions")?;
    ctx.push_vec(&mut output, second.pos, "catia_chamfer_contact_positions")?;
    let Some(connectors) = inventory.get(index + 3..index + 5) else {
        return Ok(output);
    };
    if !records_are_contiguous(connectors)
        || connectors
            .iter()
            .any(|record| record.family() != ConsolidatedFamily::B || record.class() != 0x18)
    {
        return Ok(output);
    }
    let Some(full) = inventory.get(index - 1..index + 5) else {
        return Ok(output);
    };
    if !records_are_contiguous(full) {
        return Ok(output);
    }
    let mut parsed = b2_parameter_points_from_records(bytes, connectors);
    let (Some(third), Some(fourth)) = (parsed.next(), parsed.next()) else {
        return Ok(output);
    };
    if [third.operand.value(), fourth.operand.value()] != [3, 4] {
        return Ok(output);
    }
    let lo = a[0].min(b[0]);
    let hi = a[0].max(b[0]);
    for (point, bound) in [&third, &fourth].into_iter().zip([a[1], a[2]]) {
        let Some(actual) = endpoints(point) else {
            return Ok(output);
        };
        let expected = if axis == 1 {
            [[lo, bound], [hi, bound]]
        } else {
            [[bound, lo], [bound, hi]]
        };
        if !(close(actual[0], expected[0]) && close(actual[1], expected[1])
            || close(actual[0], expected[1]) && close(actual[1], expected[0]))
        {
            return Ok(output);
        }
    }
    ctx.push_vec(&mut output, third.pos, "catia_chamfer_connector_positions")?;
    ctx.push_vec(&mut output, fourth.pos, "catia_chamfer_connector_positions")?;
    Ok(output)
}

#[cfg(test)]
mod tests;
