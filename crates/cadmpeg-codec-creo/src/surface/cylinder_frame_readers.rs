// SPDX-License-Identifier: Apache-2.0
//! Per-layout readers for positional cylinder frames.

use cadmpeg_ir::scalar::PositiveLength;

use super::{
    type24_round_edge_separator_end, type24_round_edge_shell_end, PositionalCylinderFrame,
    EPS_CYLINDER_GEOMETRY_MIN, EPS_CYLINDER_GEOMETRY_RELATIVE, EPS_SUPPORT_ORTHOGONALITY,
    EPS_SURFACE_AGREEMENT, EPS_SURFACE_NONZERO,
};
use crate::psb;
use crate::scalar;
use crate::vecmath::local_system_lanes;

pub(super) fn decode_positional_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let candidates = positional_cylinder_frame_candidates(body, cache);
    unique_positional_cylinder_frame(&candidates)
}

fn positional_cylinder_frame_candidates(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Vec<PositionalCylinderFrame> {
    [
        decode_compact_y_axis_cylinder_frame(body, cache),
        decode_complete_directrix_interval_cylinder_frame(body, cache),
        decode_selector_corner_interval_cylinder_frame(body, cache),
        decode_local_system_cylinder_frame(body, cache),
        decode_zero_support_cylinder_frame(body, cache),
        decode_signed_zero_support_cylinder_frame(body, cache),
        decode_signed_axis_aligned_cylinder_frame(body, cache),
        decode_signed_axial_radial_cylinder_frame(body, cache),
        decode_signed_radial_envelope_cylinder_frame(body, cache),
        decode_xz_axis_y_radial_cylinder_frame(body, cache),
        decode_symmetric_revolution_cylinder_frame(body, cache),
        decode_axial_endpoint_radial_sample_cylinder_frame(body, cache),
        decode_precise_center_edge_cylinder_frame(body, cache),
        decode_precise_held_center_cylinder_frame(body, cache),
        decode_compound_local_system_cylinder_frame(body, cache),
        decode_local_system_suffix_cylinder_frame(body, cache),
        decode_referenced_planar_envelope_cylinder_frame(body, cache),
        decode_held_axis_cylinder_frame(body, cache),
        decode_axial_radial_cylinder_frame(body, cache),
        decode_compact_axis_aligned_cylinder_frame(body, cache),
        decode_directrix_lane_axis_aligned_cylinder_frame(body, cache),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub(super) fn decode_selector_corner_interval_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let selector = |offset: usize| match body.get(offset..) {
        Some([value @ 0x11..=0x14, ..]) => Some((*value, offset + 1)),
        Some([0x00, value @ 0x11, 0x13, ..] | [0x00, value @ 0x13, 0x1a, ..]) => {
            Some((*value, offset + 3))
        }
        _ => None,
    };
    let (first_selector, first_parameter_start) = selector(0)?;
    let (first_parameter, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, first_parameter_start, cache)?;
    let (second_selector, second_parameter_start) = selector(next)?;
    let (second_parameter, mut cursor) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, second_parameter_start, cache)?;
    let mut corners = [[None; 3]; 2];
    for corner in &mut corners {
        for coordinate in corner {
            if matches!(body.get(cursor), Some(0x92 | 0xda)) {
                body.get(cursor..cursor + 7)?;
                cursor += 7;
                continue;
            }
            let (value, next) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
            value.is_finite().then_some(())?;
            *coordinate = Some(value);
            cursor = next;
        }
    }
    if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (_, next) = psb::reference_id(body, cursor + 1).ok()?;
        cursor = next;
    }
    (cursor == body.len()).then_some(())?;

    let parameter_span = (second_parameter - first_parameter).abs();
    let mut spans =
        std::array::from_fn::<_, 3, _>(|axis| Some(corners[1][axis]? - corners[0][axis]?));
    let scale = corners
        .iter()
        .flatten()
        .filter_map(|value| *value)
        .chain([first_parameter, second_parameter])
        .map(f64::abs)
        .fold(1.0, f64::max);
    (parameter_span > EPS_CYLINDER_GEOMETRY_MIN * scale).then_some(())?;
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale;
    let axial_axes = (0..3)
        .filter(|axis| spans[*axis].is_some_and(|span| close(span.abs(), parameter_span)))
        .collect::<Vec<_>>();
    let [axis_index] = axial_axes.as_slice() else {
        return None;
    };
    let [first_radial, second_radial]: [usize; 2] = (0..3)
        .filter(|axis| axis != axis_index)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    match (spans[first_radial], spans[second_radial]) {
        (Some(first), Some(second)) => close(first.abs(), second.abs()).then_some(())?,
        (Some(known), None) => spans[second_radial] = Some(known),
        (None, Some(known)) => spans[first_radial] = Some(known),
        (None, None) => return None,
    }
    for radial_axis in [first_radial, second_radial] {
        match corners.map(|corner| corner[radial_axis]) {
            [None, Some(second)] => corners[0][radial_axis] = Some(second - spans[radial_axis]?),
            [Some(first), None] => corners[1][radial_axis] = Some(first + spans[radial_axis]?),
            [Some(_), Some(_)] => {}
            [None, None] => return None,
        }
    }
    let complete_corner = |[x, y, z]: [Option<f64>; 3]| Some([x?, y?, z?]);
    let corners = [complete_corner(corners[0])?, complete_corner(corners[1])?];
    let radius = f64::midpoint(spans[first_radial]?.abs(), spans[second_radial]?.abs());
    (radius > EPS_CYLINDER_GEOMETRY_MIN * scale).then_some(())?;

    let axial_candidates = [
        (
            corners[0][*axis_index] - first_parameter,
            corners[1][*axis_index] - second_parameter,
            1.0,
        ),
        (
            corners[0][*axis_index] + first_parameter,
            corners[1][*axis_index] + second_parameter,
            -1.0,
        ),
    ]
    .into_iter()
    .filter(|(first, second, _)| close(*first, *second))
    .collect::<Vec<_>>();
    let [(first_axial, second_axial, axis_sign)] = axial_candidates.as_slice() else {
        return None;
    };
    let transverse_maxima = match [first_selector, second_selector] {
        [0x12, 0x11] => [true, true],
        [0x11, 0x14] => [true, false],
        [0x14, 0x13] => [false, false],
        [0x13, 0x12] => [false, true],
        _ => return None,
    };
    let mut origin = [0.0; 3];
    origin[*axis_index] = f64::midpoint(*first_axial, *second_axial);
    for (radial_axis, take_maximum) in [first_radial, second_radial]
        .into_iter()
        .zip(transverse_maxima)
    {
        origin[radial_axis] = if take_maximum {
            corners[0][radial_axis].max(corners[1][radial_axis])
        } else {
            corners[0][radial_axis].min(corners[1][radial_axis])
        };
    }
    let mut axis = [0.0; 3];
    axis[*axis_index] = *axis_sign;
    let mut ref_direction = [0.0; 3];
    ref_direction[first_radial] = 1.0;
    PositionalCylinderFrame::new(origin, axis, ref_direction, radius, Some(parameter_span))
}

pub(super) fn decode_type24_axial_interval_corner_candidates(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<Vec<PositionalCylinderFrame>> {
    let start = type24_round_edge_shell_end(body, cache)?;
    let (first_parameter, mut cursor) = scalar::decode_round_edge_coordinate(body, start, cache)?;
    cursor = type24_round_edge_separator_end(body, cursor, cache)?;
    let (second_parameter, next) = scalar::decode_round_edge_coordinate(body, cursor, cache)?;
    cursor = next;

    let mut corners = [[0.0; 3]; 2];
    for corner in &mut corners {
        for (coordinate_index, coordinate) in corner.iter_mut().enumerate() {
            let (value, next) = if coordinate_index == 0 {
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?
            } else {
                scalar::decode_tabulated_cylinder_second_coordinate(body, cursor, cache)?
            };
            value.is_finite().then_some(())?;
            *coordinate = value;
            cursor = next;
        }
    }
    if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
        let (_, next) = psb::reference_id(body, cursor + 1).ok()?;
        cursor = next;
    }
    if body.get(cursor) == Some(&psb::token::COMPOUND_CLOSE) {
        cursor += 1;
    }
    (cursor == body.len()).then_some(())?;

    let parameter_span = (second_parameter - first_parameter).abs();
    let spans = std::array::from_fn::<_, 3, _>(|axis| corners[1][axis] - corners[0][axis]);
    let scale = corners
        .iter()
        .flatten()
        .chain([first_parameter, second_parameter].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (parameter_span > EPS_CYLINDER_GEOMETRY_MIN * scale).then_some(())?;
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale;
    let axial_axes = (0..3)
        .filter(|axis| close(spans[*axis].abs(), parameter_span))
        .collect::<Vec<_>>();
    let [axis_index] = axial_axes.as_slice() else {
        return None;
    };
    let radial_axes: [usize; 2] = (0..3)
        .filter(|axis| axis != axis_index)
        .collect::<Vec<_>>()
        .try_into()
        .ok()?;
    let [first_radial, second_radial] = radial_axes;
    close(spans[first_radial].abs(), spans[second_radial].abs()).then_some(())?;
    let radius = f64::midpoint(spans[first_radial].abs(), spans[second_radial].abs());
    (radius > EPS_CYLINDER_GEOMETRY_MIN * scale).then_some(())?;

    let axial_candidates = [
        (
            corners[0][*axis_index] - first_parameter,
            corners[1][*axis_index] - second_parameter,
            1.0,
        ),
        (
            corners[0][*axis_index] + first_parameter,
            corners[1][*axis_index] + second_parameter,
            -1.0,
        ),
    ]
    .into_iter()
    .filter(|(first, second, _)| close(*first, *second))
    .collect::<Vec<_>>();
    let [(first_axial, second_axial, axis_sign)] = axial_candidates.as_slice() else {
        return None;
    };

    let mut frames = Vec::new();
    for radial_maxima in [[true, true], [true, false], [false, false], [false, true]] {
        let mut origin = [0.0; 3];
        origin[*axis_index] = f64::midpoint(*first_axial, *second_axial);
        for (radial_axis, take_maximum) in radial_axes.into_iter().zip(radial_maxima) {
            origin[radial_axis] = if take_maximum {
                corners[0][radial_axis].max(corners[1][radial_axis])
            } else {
                corners[0][radial_axis].min(corners[1][radial_axis])
            };
        }
        let mut axis = [0.0; 3];
        axis[*axis_index] = *axis_sign;
        let mut ref_direction = [0.0; 3];
        ref_direction[first_radial] = 1.0;
        frames.push(PositionalCylinderFrame::new(
            origin,
            axis,
            ref_direction,
            radius,
            Some(parameter_span),
        )?);
    }
    Some(frames)
}

pub(super) fn decode_complete_directrix_interval_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let mut cursor = if body.starts_with(&[0x18, 0xe4, 0x11]) {
        3
    } else if body.starts_with(&[0x18, 0xe4, 0x00, 0x11, 0x07])
        || body.starts_with(&[0x00, 0x11, 0x07, 0x18, 0x13])
    {
        5
    } else {
        return None;
    };
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (body.get(cursor..cursor + 3) == Some(&[0xf7, 0x17, psb::token::COMPOUND_CLOSE]))
        .then_some(())?;

    let [axial_low, radial_low, transverse_low, signed_length, radial_high, transverse_center, axial_high] =
        values;
    let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale;
    (signed_length != 0.0).then_some(())?;
    close(axial_high - axial_low, signed_length).then_some(())?;
    let signed_radius = 0.5 * (radial_high - radial_low);
    (signed_radius != 0.0).then_some(())?;
    close(transverse_center - transverse_low, signed_radius.abs()).then_some(())?;

    PositionalCylinderFrame::new(
        [
            f64::midpoint(radial_low, radial_high),
            transverse_center,
            axial_low,
        ],
        [0.0, 0.0, signed_length.signum()],
        [signed_radius.signum(), 0.0, 0.0],
        signed_radius.abs(),
        Some(signed_length.abs()),
    )
}

pub(super) fn unique_positional_cylinder_frame(
    candidates: &[PositionalCylinderFrame],
) -> Option<PositionalCylinderFrame> {
    let first = candidates.first().copied()?;
    candidates
        .iter()
        .all(|candidate| positional_cylinder_frames_agree(first, *candidate))
        .then_some(first)
}

pub(crate) fn positional_cylinder_frames_agree(
    first: PositionalCylinderFrame,
    second: PositionalCylinderFrame,
) -> bool {
    let scale = first
        .frame()
        .origin()
        .into_iter()
        .chain(second.frame().origin())
        .chain([first.radius.get(), second.radius.get()])
        .chain(first.length.map(PositiveLength::get))
        .chain(second.length.map(PositiveLength::get))
        .map(f64::abs)
        .fold(1.0, f64::max);
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale;
    first
        .frame()
        .origin()
        .into_iter()
        .zip(second.frame().origin())
        .all(|(left, right)| close(left, right))
        && first
            .frame()
            .axis()
            .into_iter()
            .zip(second.frame().axis())
            .all(|(left, right)| close(left, right))
        && first
            .frame()
            .ref_direction()
            .into_iter()
            .zip(second.frame().ref_direction())
            .all(|(left, right)| close(left, right))
        && close(first.radius.get(), second.radius.get())
        && match (first.length, second.length) {
            (Some(left), Some(right)) => close(left.get(), right.get()),
            (None, None) => true,
            _ => false,
        }
}

fn decode_xz_axis_y_radial_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.get(..3) == Some(&[0x20, 0x10, 0x00])).then_some(())?;
    let mut values = [0.0; 9];
    let mut cursor = 3;
    for (index, value) in values.iter_mut().enumerate() {
        if index == 5 && body.get(cursor..cursor + 3) == Some(&[0x34, 0xf0, 0x00]) {
            cursor += 3;
            continue;
        }
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (cursor == body.len()).then_some(())?;

    let [first_axial, auxiliary, second_axial, x0, y0, z0, x1, y1, z1] = values;
    let axis_vector = [x1 - x0, 0.0, z1 - z0];
    let length = axis_vector
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let radius = 0.5 * (y1 - y0).abs();
    let scale = values
        .into_iter()
        .map(f64::abs)
        .fold(length.max(radius).max(1.0), f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    (length.is_finite()
        && length > EPS_SURFACE_NONZERO * scale
        && radius.is_finite()
        && radius > EPS_SURFACE_NONZERO * scale
        && auxiliary.abs() < length
        && close(second_axial - first_axial, z1 - z0))
    .then_some(())?;

    PositionalCylinderFrame::new(
        [x0, f64::midpoint(y0, y1), z0],
        axis_vector.map(|value| value / length),
        [0.0, (y1 - y0).signum(), 0.0],
        radius,
        Some(length),
    )
}

fn decode_axial_endpoint_radial_sample_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let (first_leading, first_end) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, 0, cache)?;
    (first_end == 7 && body.get(first_end) == Some(&0x18)).then_some(())?;
    let (second_leading, second_end) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, first_end + 1, cache)?;
    (second_end == 15 && body.get(second_end) == Some(&0x0e)).then_some(())?;
    let (radial_x, mut cursor) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, second_end + 1, cache)?;
    let (axial_start, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (auxiliary_radial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radius, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (axial_end, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (radial_z, next) = scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    (body.get(next..) == Some(&[0xf7, 0x19])).then_some(())?;

    let values = [
        first_leading,
        second_leading,
        radial_x,
        axial_start,
        auxiliary_radial,
        radius,
        axial_end,
        radial_z,
    ];
    values.into_iter().all(f64::is_finite).then_some(())?;
    let scale = values.into_iter().map(f64::abs).fold(1.0, f64::max);
    let tolerance = EPS_SURFACE_AGREEMENT * scale;
    let length = (axial_end - axial_start).abs();
    (radius > EPS_SURFACE_NONZERO * scale
        && length > EPS_SURFACE_NONZERO * scale
        && radial_x.abs() > EPS_SURFACE_NONZERO * scale
        && auxiliary_radial.abs() <= radius + tolerance
        && (radial_x.hypot(radial_z) - radius).abs() <= EPS_SURFACE_AGREEMENT * radius)
        .then_some(())?;

    PositionalCylinderFrame::new(
        [0.0, axial_start, 0.0],
        [0.0, (axial_end - axial_start).signum(), 0.0],
        [-radial_x.signum(), 0.0, 0.0],
        radius,
        Some(length),
    )
}

fn decode_symmetric_revolution_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let mut cursor = 1;
    let (first_axial, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let separator = match body.first()? {
        0x15 => 0x18,
        0x17 => 0x15,
        _ => return None,
    };
    (body.get(cursor) == Some(&separator)).then_some(())?;
    cursor += 1;
    let (second_axial, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (radial_low, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_opposite, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    if body[0] == 0x15 {
        (body.get(cursor) == Some(&0x18)).then_some(())?;
        cursor += 1;
    } else {
        let (repeated_radial_low, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        cursor = next;
        let scale = radial_low.abs().max(repeated_radial_low.abs()).max(1.0);
        ((radial_low - repeated_radial_low).abs() <= EPS_SURFACE_AGREEMENT * scale).then_some(())?;
    }
    let (radial_high, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (first_opposite, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    if body[0] == 0x15 {
        let (repeated_radial_high, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        cursor = next;
        let scale = radial_high.abs().max(repeated_radial_high.abs()).max(1.0);
        ((radial_high - repeated_radial_high).abs() <= EPS_SURFACE_AGREEMENT * scale)
            .then_some(())?;
    } else {
        let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
        cursor = next;
    }
    (body.get(cursor..) == Some(&[0xf7, 0x19])).then_some(())?;

    let values = [
        first_axial,
        second_axial,
        radial_low,
        radial_high,
        second_opposite,
        first_opposite,
    ];
    values.into_iter().all(f64::is_finite).then_some(())?;
    let scale = values.into_iter().map(f64::abs).fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    let axial_midpoint = f64::midpoint(first_axial, first_opposite);
    close(axial_midpoint, f64::midpoint(second_axial, second_opposite)).then_some(())?;
    let radius = 0.5 * (radial_high - radial_low).abs();
    (radius > EPS_SURFACE_NONZERO * scale
        && close(f64::midpoint(radial_low, radial_high), 0.0)
        && (first_axial - first_opposite).abs() > EPS_SURFACE_NONZERO * scale
        && (second_axial - axial_midpoint).abs() > (first_axial - axial_midpoint).abs())
    .then_some(())?;

    PositionalCylinderFrame::new(
        [0.0, axial_midpoint, 0.0],
        [0.0, (first_axial - first_opposite).signum(), 0.0],
        [(radial_low - radial_high).signum(), 0.0, 0.0],
        radius,
        Some((first_opposite - first_axial).abs()),
    )
}

fn decode_compact_y_axis_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let decode_values = |start: usize, count: usize| {
        let mut cursor = start;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let (value, next) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
            value.is_finite().then_some(())?;
            values.push(value);
            cursor = next;
        }
        Some((values, cursor))
    };
    let (
        axial_start,
        axial_end,
        transverse_center,
        transverse_edge,
        radial_low,
        radial_high,
        repeated_start,
        repeated_end,
    ) = match body.first()? {
        0x14 => {
            let (values, end) = decode_values(1, 9)?;
            (end == body.len()).then_some(())?;
            let [axial_start, _, axial_end, transverse_center, repeated_start, radial_low, transverse_edge, repeated_end, radial_high] =
                values.as_slice()
            else {
                return None;
            };
            (
                *axial_start,
                *axial_end,
                *transverse_center,
                *transverse_edge,
                *radial_low,
                *radial_high,
                *repeated_start,
                *repeated_end,
            )
        }
        0x12 => {
            let (leading, marker) = decode_values(1, 1)?;
            (body.get(marker) == Some(&0x14)).then_some(())?;
            let (trailing, end) = decode_values(marker + 1, 7)?;
            (end == body.len()).then_some(())?;
            let [axial_end, transverse_edge, repeated_start, radial_low, transverse_center, repeated_end, radial_high] =
                trailing.as_slice()
            else {
                return None;
            };
            (
                leading[0],
                *axial_end,
                *transverse_center,
                *transverse_edge,
                *radial_low,
                *radial_high,
                *repeated_start,
                *repeated_end,
            )
        }
        _ => return None,
    };
    let scale = [
        axial_start,
        axial_end,
        transverse_center,
        transverse_edge,
        radial_low,
        radial_high,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    close(axial_start, repeated_start).then_some(())?;
    close(axial_end, repeated_end).then_some(())?;
    let radius = 0.5 * (radial_high - radial_low).abs();
    (radius > EPS_SURFACE_NONZERO * scale).then_some(())?;
    close((transverse_edge - transverse_center).abs(), radius).then_some(())?;
    let length = (axial_end - axial_start).abs();
    (length > EPS_SURFACE_NONZERO * scale).then_some(())?;
    PositionalCylinderFrame::new(
        [
            transverse_center,
            axial_start,
            f64::midpoint(radial_low, radial_high),
        ],
        [0.0, (axial_end - axial_start).signum(), 0.0],
        [(transverse_edge - transverse_center).signum(), 0.0, 0.0],
        radius,
        Some(length),
    )
}

fn decode_referenced_planar_envelope_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let (length, next) = scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (first_radial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (first_axial, next) =
        if body.get(cursor) == Some(&0x18) && matches!(body.get(cursor + 1), Some(0x19 | 0x32)) {
            (0.0, cursor + 1)
        } else {
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?
        };
    cursor = next;
    matches!(body.get(cursor), Some(0x19 | 0x32)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_radial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_axial, next) = if body.get(cursor) == Some(&0x18) {
        (0.0, cursor + 1)
    } else {
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?
    };
    cursor = next;
    let (radius, next) = scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    let reversed = if next == body.len() {
        false
    } else if matches!(body.get(next..), Some([0xf7, 0x17 | 0x19])) {
        true
    } else {
        return None;
    };

    let values = [
        length,
        first_radial,
        first_axial,
        second_radial,
        second_axial,
        radius,
    ];
    values.iter().all(|value| value.is_finite()).then_some(())?;
    let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    close((second_axial - first_axial).abs(), length).then_some(())?;
    close((second_radial - first_radial).abs(), 2.0 * radius).then_some(())?;

    let radial_midpoint = f64::midpoint(first_radial, second_radial);
    let orientation = if reversed { -1.0 } else { 1.0 };
    let axial_sign = orientation * (second_axial - first_axial).signum();
    let radial_sign = orientation * (second_radial - first_radial).signum();
    PositionalCylinderFrame::new(
        [radial_midpoint, second_axial, 0.0],
        [0.0, axial_sign, 0.0],
        [radial_sign, 0.0, 0.0],
        radius,
        Some(length),
    )
}

pub(super) fn decode_held_axis_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let decode = |cursor| {
        let (value, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        value.is_finite().then_some(())?;
        Some((value, next))
    };
    let (held, next) = decode(cursor)?;
    cursor = next;
    let (first_radial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x10)).then_some(())?;
    cursor += 1;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let (second_radial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x19)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor..) == Some(&[0xf7, 0x17])).then_some(())?;

    let scale = [held, first_radial, first_axial, second_radial, second_axial]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    ((second_axial - first_axial).abs() <= EPS_SURFACE_AGREEMENT * scale).then_some(())?;
    let radius = 0.5 * (second_radial - first_radial).abs();
    PositionalCylinderFrame::new(
        [
            f64::midpoint(first_radial, second_radial),
            held,
            second_axial,
        ],
        [0.0, 0.0, 1.0],
        [(second_radial - first_radial).signum(), 0.0, 0.0],
        radius,
        None,
    )
}

fn decode_axial_radial_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let decode = |cursor| {
        let (value, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (length, mut cursor) = decode(3)?;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let origin_at_first = body.get(cursor) == Some(&0x10);
    if origin_at_first {
        cursor += 1;
    }
    let (radial_sample, next) = decode(cursor)?;
    cursor = next;
    if !origin_at_first {
        (body.get(cursor) == Some(&0x10)).then_some(())?;
        cursor += 1;
    }
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x19)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radial_center, next) = decode(cursor)?;
    (body.get(next..) == Some(&[0xf7, 0x17])).then_some(())?;

    axial_radial_cylinder_frame(
        length,
        first_axial,
        radial_sample,
        second_axial,
        radial_center,
        origin_at_first,
    )
}

fn axial_radial_cylinder_frame(
    length: f64,
    first_axial: f64,
    radial_sample: f64,
    second_axial: f64,
    radial_center: f64,
    origin_at_first: bool,
) -> Option<PositionalCylinderFrame> {
    let scale = [
        length,
        first_axial,
        radial_sample,
        second_axial,
        radial_center,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    (((second_axial - first_axial).abs() - length).abs() <= EPS_SURFACE_AGREEMENT * scale)
        .then_some(())?;
    let radius = (radial_sample - radial_center).abs();
    let (origin_x, axis_x) = if origin_at_first {
        (first_axial, (second_axial - first_axial).signum())
    } else {
        (second_axial, (first_axial - second_axial).signum())
    };
    PositionalCylinderFrame::new(
        [origin_x, 0.0, radial_center],
        [axis_x, 0.0, 0.0],
        [0.0, 0.0, -(radial_sample - radial_center).signum()],
        radius,
        Some(length),
    )
}

fn decode_local_system_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut envelope = [0.0; 6];
    for value in &mut envelope {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let (radius_start, radius) = unique_terminal_positive_scalar(body, cursor)?;
    let frames = (cursor..radius_start)
        .filter_map(|start| {
            scalar::decode_positional_plane_local_system_slots(
                body.get(start..radius_start)?,
                cache,
            )
            .map(cadmpeg_ir::units::FiniteVector::get)
        })
        .collect::<Vec<_>>();
    let [slots] = frames.as_slice() else {
        return None;
    };
    let length = envelope[0];
    let scale = envelope
        .iter()
        .chain(slots.iter())
        .chain([radius, length].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |first: f64, second: f64| (first - second).abs() <= EPS_SURFACE_AGREEMENT * scale;
    let axis_indices = (0..2)
        .filter(|index| close((envelope[1 + index] - envelope[4 + index]).abs(), length))
        .collect::<Vec<_>>();
    let [axis_index] = axis_indices.as_slice() else {
        return None;
    };
    let radial_index = 1 - axis_index;
    close(
        (envelope[1 + radial_index] - envelope[4 + radial_index]).abs(),
        2.0 * radius,
    )
    .then_some(())?;
    let [support, .., origin] = local_system_lanes(*slots);
    let first_axial = envelope[1 + axis_index];
    let second_axial = envelope[4 + axis_index];
    let origin_at_first = close(origin[*axis_index], first_axial);
    let origin_at_second = close(origin[*axis_index], second_axial);
    (origin_at_first ^ origin_at_second).then_some(())?;
    let sign = if origin_at_first {
        (second_axial - first_axial).signum()
    } else {
        (first_axial - second_axial).signum()
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = sign;
    let magnitude = support
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (magnitude.is_finite() && magnitude > 0.0).then_some(())?;
    (support[*axis_index].abs() <= EPS_SURFACE_AGREEMENT * magnitude).then_some(())?;
    let ref_direction = support.map(|value| sign * value / magnitude);
    PositionalCylinderFrame::new(origin, axis, ref_direction, radius, Some(length))
}

fn decode_zero_support_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    const ZERO_SUPPORT: &[u8] = &[0x0f, 0x18, 0xe6, 0x10, 0x18, 0x0f, 0x18];
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut envelope = [0.0; 6];
    for value in &mut envelope {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let (origin, radius) =
        decode_zero_support_cylinder_origin_radius(body, cursor, ZERO_SUPPORT, cache)?;
    let length = envelope[0];
    let scale = envelope
        .iter()
        .chain(origin.iter())
        .chain([radius, length].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |first: f64, second: f64| (first - second).abs() <= EPS_SURFACE_AGREEMENT * scale;
    let axes = (0..2)
        .filter_map(|axis_index| {
            let radial_index = 1 - axis_index;
            (close(
                (envelope[1 + axis_index] - envelope[4 + axis_index]).abs(),
                length,
            ) && close(
                (envelope[1 + radial_index] - envelope[4 + radial_index]).abs(),
                2.0 * radius,
            ) && close(
                origin[radial_index],
                f64::midpoint(envelope[1 + radial_index], envelope[4 + radial_index]),
            ))
            .then_some((axis_index, radial_index))
        })
        .collect::<Vec<_>>();
    let [(axis_index, radial_index)] = axes.as_slice() else {
        return None;
    };
    let first_axial = envelope[1 + axis_index];
    let second_axial = envelope[4 + axis_index];
    let origin_at_first = close(origin[*axis_index], first_axial);
    let origin_at_second = close(origin[*axis_index], second_axial);
    (origin_at_first ^ origin_at_second).then_some(())?;
    let other_axial = if origin_at_first {
        second_axial
    } else {
        first_axial
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = (other_axial - origin[*axis_index]).signum();
    let mut ref_direction = [0.0; 3];
    ref_direction[*radial_index] = (envelope[4 + radial_index] - origin[*radial_index]).signum();
    PositionalCylinderFrame::new(origin, axis, ref_direction, radius, Some(length))
}

fn decode_signed_zero_support_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    const ZERO_SUPPORT: &[u8] = &[0x10, 0x18, 0xe6, 0x0f, 0x18, 0x0f, 0x18];
    (body.first() == Some(&0x11)).then_some(())?;
    let (signed_length, mut cursor) = scalar::decode_in_surface_row_lane(body, 1, cache)?;
    (signed_length.is_finite() && signed_length != 0.0 && body.get(cursor) == Some(&0x13))
        .then_some(())?;
    cursor += 1;
    let mut stored = [0.0; 6];
    for value in &mut stored {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let (origin, radius) =
        decode_zero_support_cylinder_origin_radius(body, cursor, ZERO_SUPPORT, cache)?;

    let first = [stored[1], stored[2], stored[0]];
    let second = [stored[4], stored[5], stored[3]];
    let spans = std::array::from_fn::<_, 3, _>(|index| (second[index] - first[index]).abs());
    let scale = first
        .iter()
        .chain(second.iter())
        .chain(origin.iter())
        .chain([signed_length, radius].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    let candidates = (0..3)
        .filter_map(|axis_index| {
            let radial = (0..3)
                .filter(|index| *index != axis_index)
                .collect::<Vec<_>>();
            let [first_radial, second_radial] = radial.as_slice() else {
                return None;
            };
            let (diameter_index, radius_index) = match (
                close(spans[*first_radial], 2.0 * radius),
                close(spans[*second_radial], radius),
            ) {
                (true, true) => (*first_radial, *second_radial),
                _ if close(spans[*second_radial], 2.0 * radius)
                    && close(spans[*first_radial], radius) =>
                {
                    (*second_radial, *first_radial)
                }
                _ => return None,
            };
            close(spans[axis_index], signed_length.abs()).then_some((
                axis_index,
                diameter_index,
                radius_index,
            ))
        })
        .collect::<Vec<_>>();
    let [(axis_index, diameter_index, radius_index)] = candidates.as_slice() else {
        return None;
    };
    close(
        origin[*diameter_index],
        f64::midpoint(first[*diameter_index], second[*diameter_index]),
    )
    .then_some(())?;
    let radius_origin_at_first = close(origin[*radius_index], first[*radius_index]);
    let radius_origin_at_second = close(origin[*radius_index], second[*radius_index]);
    (radius_origin_at_first ^ radius_origin_at_second).then_some(())?;
    let axis_origin_at_first = close(origin[*axis_index], first[*axis_index]);
    let axis_origin_at_second = close(origin[*axis_index], second[*axis_index]);
    (axis_origin_at_first ^ axis_origin_at_second).then_some(())?;

    let other_axis = if axis_origin_at_first {
        second[*axis_index]
    } else {
        first[*axis_index]
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = (other_axis - origin[*axis_index]).signum();
    let mut ref_direction = [0.0; 3];
    ref_direction[*diameter_index] = if signed_length.is_sign_negative() {
        -(second[*diameter_index] - first[*diameter_index]).signum()
    } else {
        (second[*diameter_index] - first[*diameter_index]).signum()
    };
    PositionalCylinderFrame::new(
        origin,
        axis,
        ref_direction,
        radius,
        Some(signed_length.abs()),
    )
}

fn decode_signed_axis_aligned_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x11)).then_some(())?;
    let (signed_length, mut cursor) = scalar::decode_in_surface_row_lane(body, 1, cache)?;
    (signed_length.is_finite() && signed_length != 0.0 && body.get(cursor) == Some(&0x13))
        .then_some(())?;
    cursor += 1;
    let (auxiliary, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    auxiliary.is_finite().then_some(())?;
    cursor = next;
    let mut corner_pair = [[0.0; 3]; 2];
    for coordinate in corner_pair.iter_mut().flatten() {
        let (value, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        value.is_finite().then_some(())?;
        *coordinate = value;
        cursor = next;
    }
    let orientation = if cursor == body.len() {
        AxisAlignedCornerOrientation::SecondToFirst
    } else if body.get(cursor..) == Some(&[0xf7, 0x17]) {
        AxisAlignedCornerOrientation::FirstToSecond
    } else {
        return None;
    };
    let corners = AxisAlignedCorners::new(corner_pair[0], corner_pair[1], Some(signed_length));
    // The auxiliary lane stays under the extent in magnitude, so it raises no tolerance scale.
    (auxiliary.abs() < signed_length.abs()).then_some(())?;
    // The lane states the axial extent, so this lane's axis is the one span that witnesses it and
    // a second witness leaves the axis unstated.
    let mut witnesses = crate::decode::axis::Axis::ALL
        .into_iter()
        .filter(|axis| corners.close(corners.spans[axis.index()], signed_length.abs()));
    let model_axis = witnesses.next()?;
    witnesses.next().is_none().then_some(())?;
    let axes = corners.axes(model_axis)?;
    (corners.spans[axes.radius] > EPS_SURFACE_NONZERO * corners.scale).then_some(())?;
    corners.frame(axes, orientation, signed_length.abs())
}

fn decode_signed_axial_radial_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x11)).then_some(())?;
    let decode = |offset| {
        let (value, next) = scalar::decode_in_surface_row_lane(body, offset, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (signed_length, mut cursor) = decode(1)?;
    (signed_length != 0.0 && body.get(cursor) == Some(&0x13)).then_some(())?;
    cursor += 1;
    let (auxiliary, next) = decode(cursor)?;
    cursor = next;
    (auxiliary.abs() < signed_length.abs()).then_some(())?;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let (radial_sample, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    cursor += 1;
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x19)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radial_center, next) = decode(cursor)?;
    (body.get(next..) == Some(&[0xf7, 0x17])).then_some(())?;

    axial_radial_cylinder_frame(
        signed_length.abs(),
        first_axial,
        radial_sample,
        second_axial,
        radial_center,
        false,
    )
}

fn decode_signed_radial_envelope_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x11)).then_some(())?;
    let (leading, mut cursor) = scalar::decode_in_surface_row_lane(body, 1, cache)?;
    (leading.is_finite() && body.get(cursor) == Some(&0x13)).then_some(())?;
    cursor += 1;
    let mut values = [0.0; 7];
    let mut terminal_zero = false;
    for (index, value) in values.iter_mut().enumerate() {
        if index == 6 && body.get(cursor) == Some(&0x18) && cursor + 1 == body.len() {
            *value = 0.0;
            cursor += 1;
            terminal_zero = true;
            continue;
        }
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let reversed_trailer = if cursor == body.len() {
        false
    } else if body.get(cursor..) == Some(&[0xf7, 0x19]) {
        true
    } else {
        return None;
    };
    let terminal_zero_negative_form = terminal_zero
        && !reversed_trailer
        && leading.is_sign_negative()
        && values[0].abs() < leading.abs();
    let (signed_length, auxiliary, reversed) = if reversed_trailer || terminal_zero_negative_form {
        (leading, values[0], true)
    } else {
        (values[0], leading, false)
    };
    (signed_length.is_finite()
        && signed_length != 0.0
        && reversed == signed_length.is_sign_negative()
        && auxiliary.abs() < signed_length.abs())
    .then_some(())?;

    let first_radial = [values[1], values[2]];
    let second_radial = [values[4], values[5]];
    let radial_spans =
        std::array::from_fn::<_, 2, _>(|index| (second_radial[index] - first_radial[index]).abs());
    let scale = values
        .iter()
        .chain([leading, signed_length].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale;
    let (diameter_index, radius_index) = match (
        close(radial_spans[0], 2.0 * radial_spans[1]),
        close(radial_spans[1], 2.0 * radial_spans[0]),
    ) {
        (true, false) => (0, 1),
        (false, true) => (1, 0),
        _ => return None,
    };
    let radius = radial_spans[radius_index];
    (radius > EPS_CYLINDER_GEOMETRY_MIN * scale).then_some(())?;

    let axial_end = values[6];
    let axial_start = axial_end - signed_length.abs();
    let axial_sample = values[3];
    (axial_sample >= axial_start - EPS_CYLINDER_GEOMETRY_RELATIVE * scale
        && axial_sample <= axial_end + EPS_CYLINDER_GEOMETRY_RELATIVE * scale)
        .then_some(())?;
    let mut origin = [0.0; 3];
    origin[diameter_index] =
        f64::midpoint(first_radial[diameter_index], second_radial[diameter_index]);
    origin[radius_index] = second_radial[radius_index];
    origin[2] = if reversed { axial_end } else { axial_start };
    let mut axis = [0.0; 3];
    axis[2] = if reversed { -1.0 } else { 1.0 };
    let mut ref_direction = [0.0; 3];
    ref_direction[diameter_index] =
        axis[2] * (second_radial[diameter_index] - first_radial[diameter_index]).signum();
    PositionalCylinderFrame::new(
        origin,
        axis,
        ref_direction,
        radius,
        Some(signed_length.abs()),
    )
}

fn decode_precise_center_edge_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x18)).then_some(())?;
    let (control, mut cursor) = scalar::decode_in_surface_row_lane(body, 2, cache)?;
    (control.is_finite() && cursor == 9).then_some(())?;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (body.get(cursor..) == Some(&[0xf7, 0x19])).then_some(())?;

    let signed_length = values[0];
    (signed_length.is_finite() && signed_length != 0.0).then_some(())?;
    let first = [values[1], values[2], values[3]];
    let second = [values[4], values[5], values[6]];
    let spans = std::array::from_fn::<_, 3, _>(|index| (second[index] - first[index]).abs());
    let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
    let close =
        |left: f64, right: f64| (left - right).abs() <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale;
    let candidates = [(0, 1, 2), (0, 2, 1), (1, 2, 0)]
        .into_iter()
        .filter_map(|(first_radial, second_radial, axis_index)| {
            (close(spans[first_radial], spans[second_radial])
                && spans[first_radial] > EPS_CYLINDER_GEOMETRY_MIN * scale
                && spans[axis_index] > spans[first_radial])
                .then_some((first_radial, second_radial, axis_index))
        })
        .collect::<Vec<_>>();
    let [(first_radial, second_radial, axis_index)] = candidates.as_slice() else {
        return None;
    };
    let radius = spans[*first_radial];
    let origin_axial = second[*axis_index] + signed_length;
    let lower = origin_axial.min(second[*axis_index]);
    let upper = origin_axial.max(second[*axis_index]);
    (first[*axis_index] >= lower - EPS_CYLINDER_GEOMETRY_RELATIVE * scale
        && first[*axis_index] <= upper + EPS_CYLINDER_GEOMETRY_RELATIVE * scale)
        .then_some(())?;

    let mut origin = first;
    origin[*axis_index] = origin_axial;
    let mut axis = [0.0; 3];
    axis[*axis_index] = -signed_length.signum();
    let reference_index = (*first_radial).max(*second_radial);
    let mut ref_direction = [0.0; 3];
    ref_direction[reference_index] = (second[reference_index] - first[reference_index]).signum();
    PositionalCylinderFrame::new(
        origin,
        axis,
        ref_direction,
        radius,
        Some(signed_length.abs()),
    )
}

fn decode_precise_held_center_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x18)).then_some(())?;
    let (control, mut cursor) = scalar::decode_in_surface_row_lane(body, 3, cache)?;
    (control.is_finite() && cursor == 10).then_some(())?;
    let decode = |cursor| {
        let (value, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (signed_length, next) = decode(cursor)?;
    cursor = next;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let (held_center, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    let (first_radius, next) = decode(cursor)?;
    cursor = next;
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    let (radial_edge, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    let (second_radius, next) = decode(cursor)?;
    (body.get(next..) == Some(&[0xf7, 0x19])).then_some(())?;

    let scale = [
        signed_length,
        first_axial,
        held_center,
        first_radius,
        second_axial,
        radial_edge,
        second_radius,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    (signed_length != 0.0
        && first_radius > EPS_SURFACE_NONZERO * scale
        && close(first_radius, second_radius)
        && close((radial_edge - held_center).abs(), first_radius))
    .then_some(())?;
    let origin_axial = first_axial - signed_length;
    let lower = first_axial.min(origin_axial);
    let upper = first_axial.max(origin_axial);
    (second_axial >= lower - EPS_SURFACE_AGREEMENT * scale
        && second_axial <= upper + EPS_SURFACE_AGREEMENT * scale
        && (second_axial - origin_axial).abs() <= first_radius + EPS_SURFACE_AGREEMENT * scale)
        .then_some(())?;
    PositionalCylinderFrame::new(
        [origin_axial, held_center, held_center],
        [signed_length.signum(), 0.0, 0.0],
        [0.0, 0.0, (radial_edge - held_center).signum()],
        first_radius,
        Some(signed_length.abs()),
    )
}

pub(super) fn decode_local_system_suffix_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let (radius_start, radius) = unique_terminal_positive_scalar(body, 1)?;
    let frames = (0..radius_start)
        .filter_map(|start| {
            scalar::decode_positional_cylinder_local_system_slots(
                body.get(start..radius_start)?,
                cache,
            )
            .map(cadmpeg_ir::units::FiniteVector::get)
        })
        .filter(|slots| {
            let first = [slots[0], slots[1], slots[2]];
            let second = [slots[3], slots[4], slots[5]];
            let first_magnitude = first.iter().map(|value| value * value).sum::<f64>().sqrt();
            let second_magnitude = second.iter().map(|value| value * value).sum::<f64>().sqrt();
            let scale = first_magnitude.max(second_magnitude).max(1.0);
            first_magnitude > 0.0
                && (first_magnitude - second_magnitude).abs() <= EPS_SURFACE_AGREEMENT * scale
                && first
                    .iter()
                    .zip(second)
                    .map(|(left, right)| left * right)
                    .sum::<f64>()
                    .abs()
                    <= EPS_SUPPORT_ORTHOGONALITY * scale
        })
        .collect::<Vec<_>>();
    let [slots] = frames.as_slice() else {
        return None;
    };
    cylinder_frame_from_local_system(slots, radius)
}

pub(super) fn decode_compound_local_system_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let radius_frames = (1..body.len())
        .filter_map(|radius_start| {
            let (radius, radius_end) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, radius_start, cache)
                    .or_else(|| scalar::decode(body, radius_start))?;
            (radius.is_finite()
                && radius > 0.0
                && body.get(radius_end) == Some(&psb::token::COMPOUND_CLOSE))
            .then_some((radius_start, radius))
        })
        .collect::<Vec<_>>();
    let candidates = (1..body.len())
        .filter(|start| body[start - 1] == psb::token::COMPOUND_CLOSE)
        .flat_map(|start| {
            radius_frames
                .iter()
                .filter(move |(radius_start, _)| *radius_start > start)
                .filter_map(move |(radius_start, radius)| {
                    let slots = scalar::decode_positional_cylinder_local_system_slots(
                        body.get(start..*radius_start)?,
                        cache,
                    )?
                    .get();
                    Some((slots, *radius))
                })
        })
        .collect::<Vec<_>>();
    let [(slots, radius)] = candidates.as_slice() else {
        return None;
    };
    cylinder_frame_from_local_system(slots, *radius)
}

fn cylinder_frame_from_local_system(
    slots: &[f64; 12],
    radius: f64,
) -> Option<PositionalCylinderFrame> {
    let normalize = |vector: [f64; 3]| {
        let magnitude = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
        (magnitude.is_finite() && magnitude > 0.0)
            .then(|| (vector.map(|value| value / magnitude), magnitude))
    };
    let [stored_first, stored_second, _, origin] = local_system_lanes(*slots);
    let (first, first_magnitude) = normalize(stored_first)?;
    let (second, second_magnitude) = normalize(stored_second)?;
    let scale = first_magnitude.max(second_magnitude).max(1.0);
    ((first_magnitude - second_magnitude).abs() <= EPS_SURFACE_AGREEMENT * scale).then_some(())?;
    (first
        .iter()
        .zip(second)
        .map(|(left, right)| left * right)
        .sum::<f64>()
        .abs()
        <= EPS_SUPPORT_ORTHOGONALITY)
        .then_some(())?;
    let (axis, _) = normalize([
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ])?;
    PositionalCylinderFrame::new(origin, axis, first, radius, None)
}

fn decode_zero_support_cylinder_origin_radius(
    body: &[u8],
    start: usize,
    zero_support: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<([f64; 3], f64)> {
    let (radius_start, radius) = unique_terminal_positive_scalar(body, start)?;
    let origins = (start + zero_support.len()..radius_start)
        .filter_map(|origin_start| {
            (body.get(origin_start - zero_support.len()..origin_start) == Some(zero_support)).then(
                || decode_positional_cylinder_origin(body, origin_start, radius_start, cache),
            )?
        })
        .collect::<Vec<_>>();
    let [origin] = origins.as_slice() else {
        return None;
    };
    Some((*origin, radius))
}

pub(super) fn unique_terminal_positive_scalar(body: &[u8], start: usize) -> Option<(usize, f64)> {
    let candidates = (start..body.len())
        .filter_map(|offset| {
            let (value, end) = scalar::decode(body, offset)?;
            (end == body.len() && value.is_finite() && value > 0.0).then_some((offset, value))
        })
        .collect::<Vec<_>>();
    let [(offset, value)] = candidates.as_slice() else {
        return None;
    };
    Some((*offset, *value))
}

fn decode_positional_cylinder_origin(
    body: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<[f64; 3]> {
    let mut cursor = start;
    let mut origin = [0.0; 3];
    for (index, value) in origin.iter_mut().enumerate() {
        if body.get(cursor) == Some(&0x18) && cursor + 1 == end {
            *value = 0.0;
            cursor += 1;
            continue;
        }
        let row = scalar::decode_in_row_lane(body, cursor, cache);
        let (decoded, next) = match index {
            0 => row.or_else(|| {
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)
            })?,
            _ => row.or_else(|| {
                scalar::decode_tabulated_cylinder_second_coordinate(body, cursor, cache)
            })?,
        };
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (cursor == end).then_some(origin)
}

fn decode_compact_axis_aligned_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (cursor == body.len()).then_some(())?;
    axis_aligned_cylinder_from_corners(
        [values[1], values[2], values[3]],
        [values[4], values[5], values[6]],
        Some(PositiveLength::new(values[0])?),
        AxisAlignedCornerOrientation::SecondToFirst,
    )
}

pub(super) fn decode_directrix_lane_axis_aligned_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let orientation = if cursor == body.len() {
        AxisAlignedCornerOrientation::SecondToFirst
    } else if matches!(body.get(cursor..), Some([0xf7, 0x17 | 0x19])) {
        AxisAlignedCornerOrientation::FirstToSecond
    } else {
        return None;
    };
    // The lane's first scalar is positive. The frame's axial length is the axial corner span over
    // `values[1..7]`, which that scalar does not always equal, so the frame carries it nowhere and
    // no later statement reads `values[0]`.
    (values[0] > 0.0).then_some(())?;
    axis_aligned_cylinder_from_corners(
        [values[1], values[2], values[3]],
        [values[4], values[5], values[6]],
        None,
        orientation,
    )
}

/// The corner the cylinder axis runs from, out of the two corners a body states.
#[derive(Clone, Copy)]
enum AxisAlignedCornerOrientation {
    FirstToSecond,
    SecondToFirst,
}

/// The model axes of one corner-pair cylinder: the axis the cylinder runs along, the perpendicular
/// axis whose corner span is the diameter, and the perpendicular axis whose span is the radius.
#[derive(Clone, Copy)]
struct AxisAlignedCylinderAxes {
    axis: usize,
    diameter: usize,
    radius: usize,
}

/// Two axis-aligned corners of a cylinder body, with the corner spans and the magnitude scale that
/// every step of the corner-to-cylinder derivation reads.
///
/// The derivation takes a corner pair to a cylinder in three steps: an axis whose perpendicular
/// spans hold a diameter and a radius, an agreement tolerance that scales with the body's
/// magnitudes, and a frame whose origin sits on the diameter midpoint and whose directions run
/// from one corner to the other. A reader states which axis its lane selects and which extent the
/// frame carries; the steps below are the same for every reader.
#[derive(Clone, Copy)]
struct AxisAlignedCorners {
    first: [f64; 3],
    second: [f64; 3],
    /// Corner-to-corner extent per model axis.
    spans: [f64; 3],
    /// The largest magnitude the body states, at least one. Every tolerance scales with it.
    scale: f64,
}

impl AxisAlignedCorners {
    /// Takes the corner pair. `extent` is an axial extent the body states outside the corners; its
    /// magnitude joins the scale.
    fn new(first: [f64; 3], second: [f64; 3], extent: Option<f64>) -> Self {
        Self {
            first,
            second,
            spans: std::array::from_fn(|index| (second[index] - first[index]).abs()),
            scale: first
                .iter()
                .chain(second.iter())
                .copied()
                .chain(extent)
                .map(f64::abs)
                .fold(1.0, f64::max),
        }
    }
    /// States that two extents agree within the body's scaled agreement tolerance.
    fn close(&self, left: f64, right: f64) -> bool {
        (left - right).abs() <= EPS_SURFACE_AGREEMENT * self.scale
    }
    /// Splits the two axes perpendicular to `axis` into the diameter axis and the radius axis,
    /// which the corners state as one span twice the other.
    fn axes(&self, axis: crate::decode::axis::Axis) -> Option<AxisAlignedCylinderAxes> {
        let [left, right] = axis.complement().map(crate::decode::axis::Axis::index);
        let (diameter, radius) = match (
            self.close(self.spans[left], 2.0 * self.spans[right]),
            self.close(self.spans[right], 2.0 * self.spans[left]),
        ) {
            (true, false) => (left, right),
            (false, true) => (right, left),
            _ => return None,
        };
        Some(AxisAlignedCylinderAxes {
            axis: axis.index(),
            diameter,
            radius,
        })
    }
    /// Admits the cylinder the corners state on `axes`, running along the axis as `orientation`
    /// states, with `length` as the frame's axial extent and the radius span as the radius.
    fn frame(
        &self,
        axes: AxisAlignedCylinderAxes,
        orientation: AxisAlignedCornerOrientation,
        length: f64,
    ) -> Option<PositionalCylinderFrame> {
        let (from, to) = match orientation {
            AxisAlignedCornerOrientation::FirstToSecond => (self.first, self.second),
            AxisAlignedCornerOrientation::SecondToFirst => (self.second, self.first),
        };
        let mut origin = self.second;
        origin[axes.axis] = from[axes.axis];
        origin[axes.diameter] =
            f64::midpoint(self.first[axes.diameter], self.second[axes.diameter]);
        let mut axis = [0.0; 3];
        axis[axes.axis] = (to[axes.axis] - from[axes.axis]).signum();
        let mut ref_direction = [0.0; 3];
        ref_direction[axes.diameter] = (to[axes.diameter] - from[axes.diameter]).signum();
        PositionalCylinderFrame::new(
            origin,
            axis,
            ref_direction,
            self.spans[axes.radius],
            Some(length),
        )
    }
}

/// Admits the cylinder frame that two axis-aligned corners describe.
///
/// `stored_length` is a body's own axial extent, which the candidate filter uses as a witness of
/// the axial span within `EPS_SURFACE_AGREEMENT`. Its type states the extent's sign and finiteness,
/// so a caller reading one admits it once and this function states no refusal of its own. A body
/// whose lane carries no extent, or carries one the frame does not witness, passes `None`. The
/// frame carries the axial corner span, which the extent only witnesses.
fn axis_aligned_cylinder_from_corners(
    first: [f64; 3],
    second: [f64; 3],
    stored_length: Option<PositiveLength>,
    orientation: AxisAlignedCornerOrientation,
) -> Option<PositionalCylinderFrame> {
    let corners = AxisAlignedCorners::new(first, second, stored_length.map(PositiveLength::get));
    let mut candidates = crate::decode::axis::Axis::ALL
        .into_iter()
        .filter_map(|axis| {
            let axes = corners.axes(axis)?;
            stored_length
                .is_none_or(|length| corners.close(corners.spans[axes.axis], length.get()))
                .then_some(axes)
        });
    let axes = candidates.next()?;
    candidates.next().is_none().then_some(())?;
    corners.frame(axes, orientation, corners.spans[axes.axis])
}

#[cfg(test)]
mod tests;
