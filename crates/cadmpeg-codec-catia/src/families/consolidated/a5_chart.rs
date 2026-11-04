// SPDX-License-Identifier: Apache-2.0
//! Source-adjacent parameter boundaries of consolidated A5 surfaces.

use crate::wire::bytes::read_f64_array;
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{
    records_are_contiguous, ConsolidatedFamily, ConsolidatedRecord, WidthCodedToken,
};
use cadmpeg_ir::scalar::FiniteReal;

const AXIS_DIRECTION_TOLERANCE: f64 = 1.0e-12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChartAxis {
    U,
    V,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChartSide {
    pub(crate) pos: usize,
    pub(crate) axis: ChartAxis,
    pub(crate) fixed: f64,
    pub(crate) interval: [f64; 2],
    pub(crate) reverse_interval: Option<[f64; 2]>,
}

#[derive(Debug, Clone)]
pub(crate) struct A5ChartBoundary {
    pub(crate) carrier_pos: usize,
    pub(crate) source_index: usize,
    pub(crate) bounds: [[f64; 2]; 2],
    pub(crate) sides: [ChartSide; 4],
}

pub(crate) fn a5_chart_boundaries(
    data: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<A5ChartBoundary> {
    records
        .windows(5)
        .filter_map(|window| {
            let [carrier, a, b, c, d] = window else {
                return None;
            };
            if carrier.family() != ConsolidatedFamily::A
                || carrier.class() != 0x34
                || !records_are_contiguous(window)
            {
                return None;
            }
            let sides = [a, b, c, d];
            let mut parsed = Vec::new();
            let mut operand = 0u32;
            for record in sides {
                if record.family() != ConsolidatedFamily::B || record.class() != 0x18 {
                    return None;
                }
                let increment = WidthCodedToken::new(record.width(), record.header_token())
                    .ok()?
                    .compact_operand()?
                    .value();
                if increment == 0 {
                    return None;
                }
                operand = operand.checked_add(increment)?;
                parsed.push(chart_side(data, record, operand)?);
            }
            let sides: [ChartSide; 4] = parsed.try_into().ok()?;
            let bounds = chart_bounds(&sides)?;
            Some(A5ChartBoundary {
                carrier_pos: carrier.byte_offset(),
                source_index: carrier.source_index(),
                bounds,
                sides,
            })
        })
        .collect()
}

fn chart_side(data: &[u8], record: &ConsolidatedRecord, expected: u32) -> Option<ChartSide> {
    let bytes = data.get(record.payload()?)?;
    let mut at = 0;
    if CompactUnsigned::read(bytes, &mut at)?.value() != expected {
        return None;
    }
    let control = *bytes.get(at)?;
    at += 1;
    let scalar_count = bytes.len().checked_sub(at)? / 8;
    if at.checked_add(scalar_count.checked_mul(8)?)? != bytes.len() {
        return None;
    }
    let (axis, fixed, interval, reverse_interval) =
        if control < 0x20 && matches!(control & 3, 1 | 2) {
            let mut tuple = [0.0; 3];
            for (index, bit) in [0x10, 0x04, 0x08].into_iter().enumerate() {
                if control & bit == 0 {
                    tuple[index] = crate::wire::bytes::f64_le(bytes, at)?.get();
                    at += 8;
                }
            }
            if at != bytes.len() {
                return None;
            }
            (
                if control & 3 == 1 {
                    ChartAxis::U
                } else {
                    ChartAxis::V
                },
                tuple[0],
                [tuple[1], tuple[2]],
                None,
            )
        } else {
            let (range, origin, direction) = match (control, scalar_count) {
                (0x48 | 0x88, 3) => {
                    let [t, u, v] = read_f64_array::<3>(bytes, at)?.map(FiniteReal::get);
                    if t >= 0.0 {
                        return None;
                    }
                    let direction = if control == 0x48 {
                        [0.0, 1.0]
                    } else {
                        [1.0, 0.0]
                    };
                    ([t, -t], [u, v], direction)
                }
                (0x40 | 0x80, 4) => {
                    let [first, last, u, v] = read_f64_array::<4>(bytes, at)?.map(FiniteReal::get);
                    let direction = if control == 0x40 {
                        [0.0, 1.0]
                    } else {
                        [1.0, 0.0]
                    };
                    ([first, last], [u, v], direction)
                }
                (0x08, 5) => {
                    let [t, u, v, du, dv] = read_f64_array::<5>(bytes, at)?.map(FiniteReal::get);
                    if t >= 0.0 {
                        return None;
                    }
                    ([t, -t], [u, v], [du, dv])
                }
                (0x00, 6) => {
                    let [first, last, u, v, du, dv] =
                        read_f64_array::<6>(bytes, at)?.map(FiniteReal::get);
                    ([first, last], [u, v], [du, dv])
                }
                _ => return None,
            };
            if range[0] >= range[1] {
                return None;
            }
            let (axis, fixed, varying, sign) = if direction[0].abs() <= AXIS_DIRECTION_TOLERANCE
                && (direction[1].abs() - 1.0).abs() <= AXIS_DIRECTION_TOLERANCE
            {
                (ChartAxis::U, origin[0], origin[1], direction[1])
            } else if direction[1].abs() <= AXIS_DIRECTION_TOLERANCE
                && (direction[0].abs() - 1.0).abs() <= AXIS_DIRECTION_TOLERANCE
            {
                (ChartAxis::V, origin[1], origin[0], direction[0])
            } else {
                return None;
            };
            let ends = [varying + sign * range[0], varying + sign * range[1]];
            let reverse = if matches!(control, 0x40 | 0x80) {
                let values = [varying - range[0], varying - range[1]];
                Some([values[0].min(values[1]), values[0].max(values[1])])
            } else {
                None
            };
            (
                axis,
                fixed,
                [ends[0].min(ends[1]), ends[0].max(ends[1])],
                reverse,
            )
        };
    (interval[0].is_finite()
        && interval[1].is_finite()
        && interval[0] < interval[1]
        && reverse_interval.is_none_or(|r| r[0].is_finite() && r[1].is_finite()))
    .then_some(ChartSide {
        pos: record.byte_offset(),
        axis,
        fixed,
        interval,
        reverse_interval,
    })
}

fn chart_bounds(sides: &[ChartSide; 4]) -> Option<[[f64; 2]; 2]> {
    let mut fixed = [Vec::new(), Vec::new()];
    for side in sides {
        fixed[usize::from(side.axis == ChartAxis::V)].push(side.fixed);
    }
    let [[u0, u1], [v0, v1]] = [fixed[0].as_slice(), fixed[1].as_slice()] else {
        return None;
    };
    let bounds = [[u0.min(*u1), u0.max(*u1)], [v0.min(*v1), v0.max(*v1)]];
    if bounds.iter().any(|[lower, upper]| lower >= upper) {
        return None;
    }
    for side in sides {
        let [lower, upper] = bounds[usize::from(side.axis == ChartAxis::U)];
        let overlaps = |interval: [f64; 2]| interval[1] >= lower && interval[0] <= upper;
        if !overlaps(side.interval) && !side.reverse_interval.is_some_and(overlaps) {
            return None;
        }
    }
    Some(bounds)
}

#[cfg(test)]
mod tests {
    use super::a5_chart_boundaries;
    use crate::test_support::test_a5a8::a5_surface_stream;
    use crate::wire::records::{consolidated_records, consolidated_records_in_ranges};

    fn side(operand: u8, control: u8, scalars: &[f64]) -> Vec<u8> {
        let mut payload = vec![operand * 4 + 1, control];
        for value in scalars {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let mut frame = vec![
            0xb2,
            3,
            0x18,
            u8::try_from(payload.len()).expect("short side"),
            9,
        ];
        frame.extend(payload);
        frame
    }

    fn chart() -> (Vec<u8>, usize) {
        let mut bytes = a5_surface_stream();
        let split = bytes.len();
        bytes.extend(side(2, 0x15, &[1.0]));
        bytes.extend(side(4, 0x05, &[1.0, 1.0]));
        bytes.extend(side(6, 0x80, &[-0.5, 0.0, 0.5, 0.0]));
        bytes.extend(side(8, 0x00, &[-0.5, 0.5, 0.5, 1.0, -1.0, 0.0]));
        (bytes, split)
    }

    #[test]
    fn adjacent_sides_define_chart_with_a_partial_boundary() {
        let (bytes, _) = chart();
        let records = consolidated_records(&bytes);
        let charts = a5_chart_boundaries(&bytes, &records);
        let [chart] = charts.as_slice() else {
            panic!("one source-adjacent chart");
        };
        assert_eq!(chart.carrier_pos, 0);
        assert_eq!(chart.source_index, 0);
        assert!(chart.sides.windows(2).all(|pair| pair[0].pos < pair[1].pos));
        for [lower, upper] in chart.bounds {
            assert!(lower.abs() < 1.0e-12);
            assert!((upper - 1.0).abs() < 1.0e-12);
        }
        assert!((chart.sides[2].interval[1] - 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn side_operands_follow_cumulative_header_values() {
        let (mut bytes, split) = chart();
        let records = consolidated_records(&bytes);
        for (record, (increment, operand)) in
            records[1..].iter().zip([(2, 2), (7, 9), (2, 11), (7, 18)])
        {
            let start = record.byte_offset();
            bytes[start + 4] = increment * 4 + 1;
            bytes[start + 5] = operand * 4 + 1;
        }
        let records = consolidated_records(&bytes);
        assert_eq!(a5_chart_boundaries(&bytes, &records).len(), 1);
        bytes[split + 4] = 1;
        assert!(a5_chart_boundaries(&bytes, &consolidated_records(&bytes)).is_empty());
    }

    #[test]
    fn a_chart_cannot_cross_a_source_boundary_or_a_frame_gap() {
        let (bytes, split) = chart();
        let records = consolidated_records_in_ranges(&bytes, [0..split, split..bytes.len()]);
        assert!(a5_chart_boundaries(&bytes, &records).is_empty());
        let mut gap = bytes;
        gap.insert(split, 0xff);
        assert!(a5_chart_boundaries(&gap, &consolidated_records(&gap)).is_empty());
    }

    #[test]
    fn chart_rejects_wrong_operand_and_diagonal_direction() {
        let (mut bytes, split) = chart();
        bytes[split + 5] = 13;
        assert!(a5_chart_boundaries(&bytes, &consolidated_records(&bytes)).is_empty());
        let mut bytes = a5_surface_stream();
        bytes.extend(side(2, 0x15, &[1.0]));
        bytes.extend(side(4, 0x05, &[1.0, 1.0]));
        bytes.extend(side(6, 0x08, &[-0.5, 0.5, 0.0, 0.6, 0.8]));
        bytes.extend(side(8, 0x06, &[1.0, 1.0]));
        assert!(a5_chart_boundaries(&bytes, &consolidated_records(&bytes)).is_empty());
    }
}
