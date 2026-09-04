// SPDX-License-Identifier: Apache-2.0
//! Structural validation for synthesized CATIA native records.

use super::*;

pub(super) fn validate_consolidated_class61_records(
    records: &[CatiaConsolidatedClass61Record],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, record) in records.iter().enumerate() {
        let expected_id = format!("catia:consolidated:class61-record#{index}");
        let valid_payload = match &record.payload {
            CatiaConsolidatedClass61Payload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty() && tail.last() == Some(&0x03)
            }
            CatiaConsolidatedClass61Payload::Long {
                members, scalar, ..
            } => {
                scalar.is_finite()
                    && !members.is_empty()
                    && members.windows(2).all(|pair| pair[0] < pair[1])
            }
        };
        if record.id != expected_id
            || !valid_payload
            || index > 0 && records[index - 1].byte_offset >= record.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated class-0x61 record `{}` is structurally invalid",
                record.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_groups(
    groups: &[CatiaConsolidatedGroup],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, group) in groups.iter().enumerate() {
        let expected_id = format!("catia:consolidated:group#{index}");
        if group.id != expected_id
            || index > 0 && groups[index - 1].byte_offset >= group.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated group `{}` is structurally invalid",
                group.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_cone_faces(
    faces: &[CatiaConsolidatedConeFace],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let points_by_id = parameter_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<HashMap<_, _>>();
    for (index, face) in faces.iter().enumerate() {
        let mut expected_point_offset = face.byte_offset.checked_add(face.byte_len);
        let parameter_run_valid = face.parameter_points.iter().all(|id| {
            match (expected_point_offset, points_by_id.get(id.as_str())) {
                (Some(expected), Some(point)) if point.byte_offset == expected => {
                    expected_point_offset = point.byte_offset.checked_add(point.byte_len);
                    expected_point_offset.is_some()
                }
                _ => false,
            }
        });
        let frame_overhead = face
            .byte_len
            .checked_sub(u64::try_from(face.program.len()).unwrap_or(u64::MAX));
        if face.id != format!("catia:consolidated:cone-face#{index}")
            || face.program.len() < 16
            || face.program.first() != Some(&0x85)
            || !face.program.ends_with(&[0x03, 0x11])
            || !matches!(frame_overhead, Some(21..=23))
            || !face.angular_scale.is_finite()
            || face.half_angle <= 0.0
            || face.half_angle >= std::f64::consts::FRAC_PI_2
            || !parameter_run_valid
            || index > 0 && faces[index - 1].byte_offset >= face.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cone-face descriptor `{}` is structurally invalid",
                face.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_pcurves(
    pcurves: &[CatiaConsolidatedPcurve],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, pcurve) in pcurves.iter().enumerate() {
        let expected_id = format!("catia:consolidated:pcurve#{index}");
        let count = pcurve.knots.len();
        if pcurve.id != expected_id
            || pcurve.degree != 5
            || count < 2
            || pcurve.points.len() != count
            || pcurve.first_derivatives.len() != count
            || pcurve.second_derivatives.len() != count
            || !knots_strictly_increasing(&pcurve.knots)
            || pcurve.range[0] >= pcurve.range[1]
            || pcurve
                .knots
                .iter()
                .chain(pcurve.points.iter().flatten())
                .chain(pcurve.first_derivatives.iter().flatten())
                .chain(pcurve.second_derivatives.iter().flatten())
                .chain(&pcurve.range)
                .any(|value| !value.is_finite())
            || !matches!(pcurve.tail.as_slice(), [0x07] | [0x07, 0x00])
            || index > 0 && pcurves[index - 1].byte_offset >= pcurve.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated pcurve `{}` is structurally invalid",
                pcurve.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_circles(
    circles: &[CatiaConsolidatedCircle],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, circle) in circles.iter().enumerate() {
        let full_circle =
            crate::families::b2::records::circle_range_is_full_turn(circle.radius, circle.range);
        let compact_len = usize::from(u8::from(circle.layout)).checked_sub(5 * size_of::<f64>() + 9);
        let record_id_fits_layout = matches!(
            (compact_len, circle.record_id),
            (Some(1), 0..=63) | (Some(2), 0..=255) | (Some(3), 0..=65_535)
        );
        if circle.id != format!("catia:consolidated:circle#{index}")
            || !record_id_fits_layout
            || circle
                .center_pair
                .iter()
                .chain(&circle.range)
                .chain(&[circle.radius, circle.chart_shift])
                .any(|value| !value.is_finite())
            || circle.center_pair.iter().any(|value| value.abs() > 1e6)
            || circle.radius <= 0.0
            || circle.range[0] >= circle.range[1]
            || circle.full_circle != full_circle
            || index > 0 && circles[index - 1].byte_offset >= circle.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated circle `{}` is structurally invalid",
                circle.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_cones(
    cones: &[CatiaConsolidatedCone],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, cone) in cones.iter().enumerate() {
        let expected_id = format!("catia:consolidated:cone#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            cone.direction_x[1] * cone.direction_y[2] - cone.direction_x[2] * cone.direction_y[1],
            cone.direction_x[2] * cone.direction_y[0] - cone.direction_x[0] * cone.direction_y[2],
            cone.direction_x[0] * cone.direction_y[1] - cone.direction_x[1] * cone.direction_y[0],
        ];
        if cone.id != expected_id
            || cone
                .apex
                .iter()
                .chain(&cone.direction_x)
                .chain(&cone.direction_y)
                .chain(&cone.axis)
                .chain(&[
                    cone.half_angle,
                    cone.pre_angular_range_scalar,
                    cone.angular_range[0],
                    cone.angular_range[1],
                    cone.slant_range[0],
                    cone.slant_range[1],
                    cone.angular_scale,
                    cone.angular_domain[0],
                    cone.angular_domain[1],
                ])
                .any(|value| !value.is_finite())
            || [cone.direction_x, cone.direction_y, cone.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1.0e-9)
            || cross
                .iter()
                .zip(cone.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1.0e-9)
            || cone.half_angle <= 0.0
            || cone.half_angle >= std::f64::consts::FRAC_PI_2
            || !crate::analytic::periodic_angular_range_is_valid(
                cone.angular_range,
                cone.angular_domain,
            )
            || cone.slant_range[0] < 0.0
            || cone.slant_range[0] >= cone.slant_range[1]
            || cone.angular_scale <= 0.0
            || index > 0 && cones[index - 1].byte_offset >= cone.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cone `{}` is structurally invalid",
                cone.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_cylinders(
    cylinders: &[CatiaConsolidatedCylinder],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, cylinder) in cylinders.iter().enumerate() {
        let expected_id = format!("catia:consolidated:cylinder#{index}");
        let squared_length =
            |direction: [f64; 3]| direction.iter().map(|value| value * value).sum::<f64>();
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let payload_valid = match &cylinder.payload {
            CatiaConsolidatedCylinderPayload::Layout52 {
                frame_token,
                axis,
                reference_direction,
            } => {
                *frame_token == 0x1d
                    && *axis == [1.0, 0.0, 0.0]
                    && *reference_direction == [0.0, 1.0, 0.0]
                    && axis
                        .iter()
                        .chain(reference_direction)
                        .all(|value| value.is_finite())
                    && (squared_length(*axis) - 1.0).abs() <= 1.0e-9
                    && (squared_length(*reference_direction) - 1.0).abs() <= 1.0e-9
                    && dot(*axis, *reference_direction).abs() <= 1.0e-9
                    && crate::families::b2::records::circle_range_is_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
            }
            CatiaConsolidatedCylinderPayload::Layout5a {
                frame_token,
                axis,
                reference_direction,
            } => {
                matches!(*frame_token, 0x19 | 0x1c)
                    && axis[2] == 0.0
                    && *reference_direction == [-axis[1], axis[0], 0.0]
                    && axis
                        .iter()
                        .chain(reference_direction)
                        .all(|value| value.is_finite())
                    && (squared_length(*axis) - 1.0).abs() <= 1.0e-9
                    && (squared_length(*reference_direction) - 1.0).abs() <= 1.0e-9
                    && dot(*axis, *reference_direction).abs() <= 1.0e-9
                    && crate::families::b2::records::circle_range_is_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
            }
            CatiaConsolidatedCylinderPayload::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            } => {
                cylinder.payload.layout() == 0x62
                    && stored_vector
                        .iter()
                        .chain(std::iter::once(range_origin))
                        .all(|value| value.is_finite())
                    && (stored_vector[0].hypot(stored_vector[1]) - 1.0).abs() <= 1.0e-9
                    && *axis == [0.0, 1.0, 0.0]
                    && *reference_direction == [stored_vector[0], 0.0, stored_vector[1]]
                    && crate::families::b2::records::circle_range_is_within_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
                    && range_origin.to_bits()
                        == crate::families::b2::records::cylinder_range_origin(
                            cylinder.radius,
                            cylinder.u_range,
                        )
                        .to_bits()
            }
        };
        if cylinder.id != expected_id
            || cylinder
                .origin
                .iter()
                .chain(&cylinder.u_range)
                .chain(&cylinder.v_range)
                .chain(&[cylinder.radius])
                .any(|value| !value.is_finite())
            || cylinder.radius <= 0.0
            || cylinder.u_range[0] >= cylinder.u_range[1]
            || cylinder.v_range[0] >= cylinder.v_range[1]
            || !payload_valid
            || index > 0 && cylinders[index - 1].byte_offset >= cylinder.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cylinder `{}` is structurally invalid",
                cylinder.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_embedded_cylinders(
    cylinders: &[CatiaConsolidatedEmbeddedCylinder],
    groups: &[CatiaConsolidatedGroup],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let groups = groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            (
                group.id.as_str(),
                (group, groups.get(index + 1).map(|next| next.byte_offset)),
            )
        })
        .collect::<HashMap<_, _>>();
    for (index, cylinder) in cylinders.iter().enumerate() {
        let squared_length =
            |direction: [f64; 3]| direction.iter().map(|value| value * value).sum::<f64>();
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let group_valid =
            groups
                .get(cylinder.group.as_str())
                .is_some_and(|(group, next_offset)| {
                    group.group_type == 3
                        && group.byte_offset < cylinder.byte_offset
                        && next_offset.is_none_or(|next| cylinder.byte_offset < next)
                });
        if cylinder.id != format!("catia:consolidated:embedded-cylinder#{index}")
            || !group_valid
            || !cylinder
                .origin
                .iter()
                .chain(&cylinder.u_range)
                .chain(&cylinder.v_range)
                .chain(&cylinder.axis)
                .chain(&cylinder.reference_direction)
                .chain(&[cylinder.radius])
                .all(|value| value.is_finite())
            || cylinder.radius <= 0.0
            || cylinder.u_range[0] >= cylinder.u_range[1]
            || cylinder.v_range[0] >= cylinder.v_range[1]
            || !matches!(cylinder.frame_token, 0x19 | 0x1c)
            || cylinder.axis[2] != 0.0
            || cylinder.reference_direction != [-cylinder.axis[1], cylinder.axis[0], 0.0]
            || (squared_length(cylinder.axis) - 1.0).abs() > 1.0e-9
            || (squared_length(cylinder.reference_direction) - 1.0).abs() > 1.0e-9
            || dot(cylinder.axis, cylinder.reference_direction).abs() > 1.0e-9
            || !crate::families::b2::records::circle_range_is_full_turn(
                cylinder.radius,
                cylinder.u_range,
            )
            || index > 0 && cylinders[index - 1].byte_offset >= cylinder.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated embedded cylinder `{}` is structurally invalid",
                cylinder.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_parameter_points(
    points: &[CatiaConsolidatedParameterPoint],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, point) in points.iter().enumerate() {
        let payload_valid = match &point.payload {
            CatiaConsolidatedParameterPointPayload::Uv { uv } => {
                point.payload.layout() == 0x12 && uv.iter().all(|value| value.is_finite())
            }
            CatiaConsolidatedParameterPointPayload::StationUv { station, uv } => {
                point.payload.layout() == 0x1a
                    && station.is_finite()
                    && uv.iter().all(|value| value.is_finite())
            }
            CatiaConsolidatedParameterPointPayload::FiveScalars { values } => {
                point.payload.layout() == 0x2a && values.iter().all(|value| value.is_finite())
            }
        };
        let frame_overhead = point.byte_len.checked_sub(u64::from(point.payload.layout()));
        if point.id != format!("catia:consolidated:parameter-point#{index}")
            || !matches!(frame_overhead, Some(5..=7))
            || !matches!(point.prefix.as_u8(), 0x05 | 0x09 | 0x0d | 0x11)
            || !payload_valid
            || index > 0 && points[index - 1].byte_offset >= point.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated parameter point `{}` is structurally invalid",
                point.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_plane_carriers(
    carriers: &[CatiaConsolidatedPlaneCarrier],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, carrier) in carriers.iter().enumerate() {
        let (selector, scalar_count, payload_valid) = match &carrier.payload {
            CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
                point,
                direction,
                tail,
            } => (
                0xe4,
                7,
                point
                    .iter()
                    .chain(direction)
                    .chain(tail)
                    .all(|value| value.is_finite()),
            ),
            CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
                point,
                direction,
                tail,
            } => (
                0xc4,
                8,
                point
                    .iter()
                    .chain(direction)
                    .chain(tail)
                    .all(|value| value.is_finite()),
            ),
            CatiaConsolidatedPlaneCarrierPayload::PointTail { point, tail } => (
                0xec,
                6,
                point.iter().chain(tail).all(|value| value.is_finite()),
            ),
            CatiaConsolidatedPlaneCarrierPayload::ScalarLane { values, .. } => (
                carrier.payload.selector(),
                values.len(),
                !values.is_empty() && values.iter().all(|value| value.is_finite()),
            ),
        };
        let header_limit = 1u32.checked_shl(8 * u32::from(carrier.width));
        let scalar_count = u64::try_from(scalar_count).map_err(|_| {
            cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated plane carrier `{}` has too many scalars",
                carrier.id
            ))
        })?;
        let expected_len = 4 + u64::from(carrier.width) + 2 + 8 * scalar_count;
        if carrier.id != format!("catia:consolidated:plane-carrier#{index}")
            || !matches!(carrier.width, 1..=3)
            || header_limit.is_none_or(|limit| carrier.header_token >= limit)
            || !matches!(carrier.flag, 0x03 | 0x13 | 0x83)
            || matches!(
                &carrier.payload,
                CatiaConsolidatedPlaneCarrierPayload::ScalarLane { .. }
            ) && matches!(carrier.payload.selector(), 0xe4 | 0xc4 | 0xec)
            || carrier.payload.selector() != selector
            || carrier.byte_len != expected_len
            || !payload_valid
            || index > 0 && carriers[index - 1].byte_offset >= carrier.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated plane carrier `{}` is structurally invalid",
                carrier.id
            )));
        }
    }
    Ok(())
}

pub(super) fn valid_consolidated_plane_geometry(
    payload: &CatiaConsolidatedPlaneCarrierPayload,
) -> bool {
    let (point, direction, tail) = match payload {
        CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
            point,
            direction,
            tail,
        } => (*point, [direction[0], direction[1], 0.0], *tail),
        CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
            point,
            direction,
            tail,
        } => (*point, *direction, *tail),
        CatiaConsolidatedPlaneCarrierPayload::PointTail { .. } => return false,
        CatiaConsolidatedPlaneCarrierPayload::ScalarLane { .. } => return false,
    };
    let finite = point
        .iter()
        .chain(direction.iter())
        .chain(tail.iter())
        .all(|value| value.is_finite());
    let norm = direction[0].hypot(direction[1]).hypot(direction[2]);
    finite
        && (norm - 1.0).abs() <= 1.0e-9
        && direction[2].abs() <= 1.0e-9
        && tail[0] > 0.0
        && tail[1] < tail[2]
}

pub(super) fn validate_consolidated_reference_lists(
    lists: &[CatiaConsolidatedReferenceList],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, list) in lists.iter().enumerate() {
        if list.id != format!("catia:consolidated:reference-list#{index}")
            || list.references.is_empty()
            || index > 0 && lists[index - 1].byte_offset >= list.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated reference list `{}` is structurally invalid",
                list.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_revolutions(
    revolutions: &[CatiaConsolidatedRevolution],
    circles: &[CatiaConsolidatedCircle],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, revolution) in revolutions.iter().enumerate() {
        let mut profile_candidates = circles.iter().filter(|circle| {
            circle.range[0].to_bits() == revolution.profile_range[0].to_bits()
                && circle.range[1].to_bits() == revolution.profile_range[1].to_bits()
        });
        let expected_profile = profile_candidates.next().and_then(|circle| {
            profile_candidates
                .next()
                .is_none()
                .then_some(circle.id.as_str())
        });
        let expected_id = format!("catia:consolidated:revolution#{index}");
        let squared_length = |direction: [f64; 3]| {
            direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>()
        };
        let cross = [
            revolution.direction_x[1] * revolution.direction_y[2]
                - revolution.direction_x[2] * revolution.direction_y[1],
            revolution.direction_x[2] * revolution.direction_y[0]
                - revolution.direction_x[0] * revolution.direction_y[2],
            revolution.direction_x[0] * revolution.direction_y[1]
                - revolution.direction_x[1] * revolution.direction_y[0],
        ];
        if revolution.id != expected_id
            || revolution.profile_allocation_id == 0
            || revolution
                .origin
                .iter()
                .chain(&revolution.direction_x)
                .chain(&revolution.direction_y)
                .chain(&revolution.axis)
                .chain(&revolution.angular_range)
                .chain(&revolution.profile_range)
                .chain(&[revolution.angular_scale])
                .any(|value| !value.is_finite())
            || revolution.angular_scale <= 0.0
            || revolution.angular_range[0] >= revolution.angular_range[1]
            || revolution.profile_range[0] >= revolution.profile_range[1]
            || revolution.profile_circle.as_deref() != expected_profile
            || [
                revolution.direction_x,
                revolution.direction_y,
                revolution.axis,
            ]
            .into_iter()
            .any(|direction| (squared_length(direction) - 1.0).abs() > 1.0e-12)
            || cross
                .iter()
                .zip(revolution.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1.0e-12)
            || revolution.angular_range[0] / revolution.angular_scale != 0.5
            || (revolution.angular_range[1] - revolution.angular_range[0])
                / revolution.angular_scale
                != std::f64::consts::TAU
            || index > 0 && revolutions[index - 1].byte_offset >= revolution.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated revolution `{}` is structurally invalid",
                revolution.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_line_profiles(
    lines: &[CatiaConsolidatedLineProfile],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, line) in lines.iter().enumerate() {
        let squared_length = line
            .direction
            .iter()
            .map(|component| component * component)
            .sum::<f64>();
        if line.id != format!("catia:consolidated:line-profile#{index}")
            || line
                .origin
                .iter()
                .chain(&line.direction)
                .chain(&line.range)
                .any(|value| !value.is_finite())
            || (squared_length - 1.0).abs() > 1.0e-12
            || line.range[0] >= line.range[1]
            || index > 0 && lines[index - 1].byte_offset >= line.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated line profile `{}` is structurally invalid",
                line.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_spheres(
    spheres: &[CatiaConsolidatedSphere],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, sphere) in spheres.iter().enumerate() {
        let expected_id = format!("catia:consolidated:sphere#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            sphere.direction_x[1] * sphere.direction_y[2]
                - sphere.direction_x[2] * sphere.direction_y[1],
            sphere.direction_x[2] * sphere.direction_y[0]
                - sphere.direction_x[0] * sphere.direction_y[2],
            sphere.direction_x[0] * sphere.direction_y[1]
                - sphere.direction_x[1] * sphere.direction_y[0],
        ];
        if sphere.id != expected_id
            || sphere
                .center
                .iter()
                .chain(&sphere.direction_x)
                .chain(&sphere.direction_y)
                .chain(&sphere.axis)
                .chain(&sphere.azimuth_range)
                .chain(&sphere.latitude_range)
                .chain(&[sphere.radius])
                .any(|value| !value.is_finite())
            || [sphere.direction_x, sphere.direction_y, sphere.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1.0e-12)
            || dot(sphere.direction_x, sphere.direction_y).abs() > 1.0e-12
            || dot(sphere.direction_x, sphere.axis).abs() > 1.0e-12
            || dot(sphere.direction_y, sphere.axis).abs() > 1.0e-12
            || cross
                .iter()
                .zip(sphere.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1.0e-12)
            || sphere.radius <= 0.0
            || !crate::analytic::sphere_angular_ranges_are_valid(
                sphere.azimuth_range,
                sphere.latitude_range,
            )
            || index > 0 && spheres[index - 1].byte_offset >= sphere.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated sphere `{}` is structurally invalid",
                sphere.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_consolidated_tori(
    tori: &[CatiaConsolidatedTorus],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, torus) in tori.iter().enumerate() {
        let expected_id = format!("catia:consolidated:torus#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            torus.direction_x[1] * torus.direction_y[2]
                - torus.direction_x[2] * torus.direction_y[1],
            torus.direction_x[2] * torus.direction_y[0]
                - torus.direction_x[0] * torus.direction_y[2],
            torus.direction_x[0] * torus.direction_y[1]
                - torus.direction_x[1] * torus.direction_y[0],
        ];
        if torus.id != expected_id
            || torus
                .center
                .iter()
                .chain(&torus.direction_x)
                .chain(&torus.direction_y)
                .chain(&torus.axis)
                .chain(&torus.major_angular_range)
                .chain(&torus.major_angular_domain)
                .chain(&torus.minor_angular_range)
                .chain(&torus.minor_angular_domain)
                .chain(&[
                    torus.major_radius,
                    torus.minor_radius,
                    torus.major_scale,
                    torus.minor_scale,
                ])
                .any(|value| !value.is_finite())
            || [torus.direction_x, torus.direction_y, torus.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1.0e-12)
            || dot(torus.direction_x, torus.direction_y).abs() > 1.0e-12
            || dot(torus.direction_x, torus.axis).abs() > 1.0e-12
            || dot(torus.direction_y, torus.axis).abs() > 1.0e-12
            || cross
                .iter()
                .zip(torus.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1.0e-12)
            || torus.major_radius <= 0.0
            || torus.minor_radius <= 0.0
            || !crate::analytic::periodic_angular_range_is_valid(
                torus.major_angular_range,
                torus.major_angular_domain,
            )
            || !crate::analytic::periodic_angular_range_is_valid(
                torus.minor_angular_range,
                torus.minor_angular_domain,
            )
            || torus.major_scale <= 0.0
            || torus.minor_scale <= 0.0
            || index > 0 && tori[index - 1].byte_offset >= torus.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated torus `{}` is structurally invalid",
                torus.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_zero_entity_support_runs(
    runs: &[CatiaZeroEntitySupportRun],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let face_count = runs.iter().filter(|run| run.face.is_some()).count();
    let face_roster_valid = face_count == 0 || face_count == runs.len();
    let expected_loop_count = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .map(|face| face.loop_terminals.len())
        .sum::<usize>();
    let loops = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .flat_map(|face| &face.loops)
        .collect::<Vec<_>>();
    let loop_roster_valid = loops.is_empty()
        || loops.len() == expected_loop_count
            && loops
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset);
    for (index, run) in runs.iter().enumerate() {
        let support_bindings_valid = run.face.as_ref().is_none_or(|face| {
            let binding_count = face
                .loops
                .iter()
                .filter(|loop_record| !loop_record.support_record_ordinals.is_empty())
                .count();
            if binding_count == 0 {
                return true;
            }
            if binding_count != face.loops.len() {
                return false;
            }
            let mut bound = HashSet::new();
            face.loops.iter().all(|loop_record| {
                loop_record
                    .member_ids
                    .iter()
                    .zip(&loop_record.support_record_ordinals)
                    .all(|(member, record_ordinal)| {
                        let slot = loop_record.terminal_id.checked_sub(*member);
                        bound.insert(*record_ordinal)
                            && run.supports.iter().any(|support| {
                                support.record_ordinal == *record_ordinal
                                    && Some(support.face_local_slot) == slot
                            })
                    })
            }) && bound.len() == run.supports.len()
        });
        let face_valid = run.face.as_ref().is_none_or(|face| {
            let derived_terminals = face.allocations.first().and_then(|first| {
                face.allocations[1..]
                    .iter()
                    .map(|allocation| first.checked_sub(*allocation))
                    .collect::<Option<Vec<_>>>()
            });
            let expected_length = face
                .allocations
                .len()
                .checked_mul(5)
                .and_then(|length| length.checked_add(14));
            face.tag[0] == 0x5f
                && face.allocations.len() >= 2
                && !face.allocations.contains(&0)
                && !face.loop_terminals.contains(&0)
                && face.loop_terminals[1..]
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && matches!(face.terminal_control, 0x03 | 0x05)
                && expected_length == Some(usize::from(face.tag[1]) + 12)
                && derived_terminals.as_ref() == Some(&face.loop_terminals)
                && (face.loops.is_empty()
                    || face.loops.len() == face.loop_terminals.len()
                        && face
                            .loops
                            .first()
                            .is_some_and(|outer| matches!(outer.loop_class, 0x41 | 0xc1))
                        && face.loops[1..].iter().all(|inner| inner.loop_class == 0x50)
                        && face.loops.iter().zip(&face.loop_terminals).all(
                            |(loop_record, terminal)| {
                                let edge_count = loop_record.member_ids.len();
                                let reference_count = edge_count
                                    .checked_mul(2)
                                    .and_then(|count| count.checked_add(1));
                                let packed_length = edge_count
                                    .checked_mul(3)
                                    .and_then(|bits| bits.checked_add(7));
                                let expected_length = reference_count.zip(packed_length).and_then(
                                    |(reference_count, packed_length)| {
                                        reference_count
                                            .checked_mul(5)?
                                            .checked_add(16 + packed_length / 8)
                                    },
                                );
                                loop_record.tag[0] == 0x62
                                    && !loop_record.member_ids.is_empty()
                                    && loop_record.typed_references.len() == edge_count
                                    && !loop_record.typed_references.contains(&0)
                                    && (loop_record.typed_records.is_empty()
                                        || loop_record.typed_records.len() == edge_count
                                            && loop_record
                                                .typed_references
                                                .iter()
                                                .zip(&loop_record.typed_records)
                                                .all(|(ordinal, id)| {
                                                    zero_entity_record(records, *ordinal)
                                                        .is_some_and(|record| &record.id == id)
                                                }))
                                    && (loop_record.support_record_ordinals.is_empty()
                                        || loop_record.support_record_ordinals.len() == edge_count)
                                    && loop_record.forward_senses.len() == edge_count
                                    && {
                                        let endpoints = loop_record
                                            .support_record_ordinals
                                            .iter()
                                            .map(|ordinal| {
                                                run.supports
                                                    .iter()
                                                    .find(|support| {
                                                        support.record_ordinal == *ordinal
                                                    })
                                                    .and_then(|support| support.model_endpoints)
                                            })
                                            .collect::<Vec<_>>();
                                        let expected = crate::families::zero_entity::records::
                                            oriented_closed_model_endpoints(
                                                &endpoints,
                                                &loop_record.forward_senses,
                                            )
                                            .unwrap_or_default();
                                        loop_record.oriented_model_endpoints == expected
                                    }
                                    && loop_record.terminal_id == *terminal
                                    && loop_record.gap != 0
                                    && matches!(loop_record.loop_class, 0x41 | 0x50 | 0xc1)
                                    && loop_record.member_ids.iter().enumerate().all(
                                        |(member_index, member)| {
                                            u32::try_from(member_index).ok().and_then(
                                                |member_index| {
                                                    loop_record
                                                        .terminal_id
                                                        .checked_sub(loop_record.gap)?
                                                        .checked_sub(member_index)
                                                },
                                            ) == Some(*member)
                                        },
                                    )
                                    && expected_length == Some(usize::from(loop_record.tag[1]) + 12)
                                    && zero_entity_record(records, loop_record.record_ordinal)
                                        .is_some_and(|record| {
                                            record.byte_offset == loop_record.byte_offset
                                                && record.tag == loop_record.tag
                                        })
                            },
                        ))
                && zero_entity_record(records, face.record_ordinal).is_some_and(|record| {
                    record.byte_offset == face.byte_offset && record.tag == face.tag
                })
                && support_bindings_valid
                && (index == 0
                    || runs[index - 1]
                        .face
                        .as_ref()
                        .is_none_or(|previous| previous.byte_offset < face.byte_offset))
        });
        let carrier_tag =
            zero_entity_record(records, run.carrier_record_ordinal).map(|record| record.tag);
        let supports_valid = !run.supports.is_empty()
            && run
                .supports
                .iter()
                .enumerate()
                .all(|(support_index, support)| {
                    if support.face_local_slot == 0 {
                        return false;
                    }
                    let endpoints_valid = match (support.tag, support.uv_endpoints) {
                        (
                            [0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8],
                            Some(endpoints),
                        ) => endpoints.iter().flatten().all(|value| value.is_finite()),
                        ([0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8], None) => {
                            false
                        }
                        ([0x21, _], None) => true,
                        _ => false,
                    };
                    let model_endpoints_valid = support.model_endpoints.is_none_or(|endpoints| {
                        support.uv_endpoints.is_some()
                            && endpoints.iter().all(|point| {
                                [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                            })
                    });
                    let model_midpoint_valid = support.model_midpoint.is_none_or(|point| {
                        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                    });
                    let model_curve_valid =
                        validate_zero_entity_model_curve(carrier_tag, support.model_curve.as_ref());
                    let model_curve_construction_valid =
                        validate_zero_entity_model_curve_construction(
                            carrier_tag,
                            support.model_curve.as_ref(),
                            support.model_curve_construction.as_ref(),
                        );
                    let has_model_carrier =
                        support.model_curve.is_some() || support.model_curve_construction.is_some();
                    let has_pcurve = support.pcurve.is_some();
                    let model_parameters_valid =
                        support.model_parameters.is_some_and(|parameters| {
                            parameters.into_iter().all(f64::is_finite)
                                && parameters[0] != parameters[1]
                        }) == has_model_carrier;
                    let pcurve_valid = match (&support.tag, &support.pcurve) {
                        (
                            [0x21, tag @ (0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8)],
                            Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                                degree,
                                knots,
                                control_points,
                                weights,
                                periodic: false,
                            }),
                        ) => {
                            let (
                                expected_degree,
                                expected_controls,
                                expected_multiplicities,
                                rational,
                            ): (u32, usize, &[usize], bool) = match tag {
                                0x45 => (3, 12, &[4, 2, 2, 2, 2, 4], false),
                                0x71 => (1, 2, &[2, 2], false),
                                0x72 => (3, 14, &[4, 2, 2, 2, 2, 2, 4], false),
                                0x91 => (3, 4, &[4, 4], false),
                                0x99 => (2, 3, &[3, 3], true),
                                0x9f => (3, 16, &[4, 2, 2, 2, 2, 2, 2, 4], false),
                                0xd6 => (2, 5, &[3, 2, 3], false),
                                0xe8 => (3, 7, &[4, 1, 1, 1, 4], false),
                                _ => unreachable!(),
                            };
                            *degree == expected_degree
                                && control_points.len() == expected_controls
                                && knots.len() == expected_controls + expected_degree as usize + 1
                                && knots.iter().all(|knot| knot.is_finite())
                                && knots_nondecreasing(knots)
                                && knots[..=expected_degree as usize]
                                    .iter()
                                    .all(|knot| *knot == knots[0])
                                && knots[expected_controls..]
                                    .iter()
                                    .all(|knot| *knot == knots[expected_controls])
                                && knots[expected_degree as usize] < knots[expected_controls]
                                && knots
                                    .chunk_by(|left, right| left == right)
                                    .map(<[f64]>::len)
                                    .eq(expected_multiplicities.iter().copied())
                                && control_points
                                    .iter()
                                    .all(|point| point.u.is_finite() && point.v.is_finite())
                                && weights.as_ref().is_some_and(|weights| {
                                    rational
                                        && weights.len() == expected_controls
                                        && weights
                                            .iter()
                                            .all(|weight| weight.is_finite() && *weight > 0.0)
                                }) == rational
                        }
                        ([0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8], _) => false,
                        ([0x21, _], None) => true,
                        _ => false,
                    };
                    let expected_ordinal = u32::try_from(support_index)
                        .ok()
                        .and_then(|index| index.checked_add(1))
                        .and_then(|offset| run.carrier_record_ordinal.checked_add(offset));
                    support.tag[0] == 0x21
                        && zero_entity_record(records, support.record_ordinal).is_some_and(
                            |record| {
                                record.byte_offset == support.byte_offset
                                    && record.tag == support.tag
                            },
                        )
                        && support.byte_offset > run.carrier_byte_offset
                        && Some(support.record_ordinal) == expected_ordinal
                        && (support_index == 0
                            || run.supports[support_index - 1].byte_offset < support.byte_offset)
                        && endpoints_valid
                        && pcurve_valid
                        && model_curve_valid
                        && model_curve_construction_valid
                        && model_parameters_valid
                        && support.model_midpoint.is_some() == has_pcurve
                        && model_midpoint_valid
                        && model_endpoints_valid
                });
        if run.id != format!("catia:zero-entity:support-run#{index}")
            || !supports_valid
            || !face_roster_valid
            || !loop_roster_valid
            || !face_valid
            || run.carrier_record_ordinal == 0
            || zero_entity_record(records, run.carrier_record_ordinal)
                .is_none_or(|record| record.byte_offset != run.carrier_byte_offset)
            || index > 0
                && (runs[index - 1].carrier_byte_offset >= run.carrier_byte_offset
                    || runs[index - 1].carrier_record_ordinal >= run.carrier_record_ordinal)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "zero-entity support run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_zero_entity_model_curve_construction(
    carrier_tag: Option<[u8; 2]>,
    model_curve: Option<&cadmpeg_ir::geometry::CurveGeometry>,
    construction: Option<&cadmpeg_ir::geometry::ProceduralCurveDefinition>,
) -> bool {
    let finite_vector = |vector: &cadmpeg_ir::math::Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
            && vector.x.hypot(vector.y).hypot(vector.z) > 0.0
    };
    let norm = |vector: &cadmpeg_ir::math::Vector3| vector.x.hypot(vector.y).hypot(vector.z);
    let normalized_dot = |left: &cadmpeg_ir::math::Vector3, right: &cadmpeg_ir::math::Vector3| {
        (left.x * right.x + left.y * right.y + left.z * right.z) / (norm(left) * norm(right))
    };
    match (carrier_tag, model_curve, construction) {
        (
            Some([0x29, 0xb8]),
            None,
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
                angle_range,
                center,
                major,
                minor,
                pitch,
                apex_factor,
                axis,
            }),
        ) => {
            angle_range.iter().copied().all(f64::is_finite)
                && angle_range[0] < angle_range[1]
                && [center.x, center.y, center.z]
                    .into_iter()
                    .all(f64::is_finite)
                && finite_vector(major)
                && finite_vector(minor)
                && [pitch.x, pitch.y, pitch.z].into_iter().all(f64::is_finite)
                && apex_factor.is_finite()
                && finite_vector(axis)
                && (norm(axis) - 1.0).abs() <= 1.0e-9
                && (norm(major) - norm(minor)).abs() <= 1.0e-9 * norm(major).max(norm(minor))
                && normalized_dot(major, minor).abs() <= 1.0e-9
                && normalized_dot(major, axis).abs() <= 1.0e-9
                && normalized_dot(minor, axis).abs() <= 1.0e-9
                && (pitch.x == 0.0 && pitch.y == 0.0 && pitch.z == 0.0
                    || normalized_dot(pitch, axis).abs() >= 1.0 - 1.0e-9)
                && {
                    let handed_minor = cadmpeg_ir::math::Vector3::new(
                        axis.y * major.z - axis.z * major.y,
                        axis.z * major.x - axis.x * major.z,
                        axis.x * major.y - axis.y * major.x,
                    );
                    normalized_dot(&handed_minor, minor) >= 1.0 - 1.0e-9
                }
        }
        (_, Some(_), None)
        | (Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8] | [0x34, 0xc8 | 0x5e]), None, None) => {
            true
        }
        _ => false,
    }
}

pub(super) fn validate_zero_entity_model_curve(
    carrier_tag: Option<[u8; 2]>,
    curve: Option<&cadmpeg_ir::geometry::CurveGeometry>,
) -> bool {
    use cadmpeg_ir::geometry::CurveGeometry;

    let finite_point = |point: &cadmpeg_ir::math::Point3| {
        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
    };
    let finite_vector = |vector: &cadmpeg_ir::math::Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
            && vector.x.hypot(vector.y).hypot(vector.z) > 0.0
    };
    match (carrier_tag, curve) {
        (Some([0x27, 0x6a] | [0x34, 0xc8 | 0x5e]), Some(CurveGeometry::Nurbs(curve))) => {
            let Ok(degree) = usize::try_from(curve.degree) else {
                return false;
            };
            curve.control_points.len() > degree
                && curve.knots.len() == curve.control_points.len() + degree + 1
                && curve.knots.iter().all(|knot| knot.is_finite())
                && knots_nondecreasing(&curve.knots)
                && curve.control_points.iter().all(finite_point)
                && curve.weights.as_ref().is_none_or(|weights| {
                    weights.len() == curve.control_points.len()
                        && weights
                            .iter()
                            .all(|weight| weight.is_finite() && *weight > 0.0)
                })
                && !curve.periodic
        }
        (Some([0x28, 0x8a] | [0x29, 0xb8]), Some(CurveGeometry::Line { origin, direction })) => {
            finite_point(origin) && finite_vector(direction)
        }
        (
            Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8]),
            Some(CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            }),
        ) => {
            finite_point(center)
                && finite_vector(axis)
                && finite_vector(ref_direction)
                && radius.is_finite()
                && *radius > 0.0
        }
        (Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8] | [0x34, 0xc8 | 0x5e]), None) => true,
        _ => false,
    }
}

pub(super) fn validate_zero_entity_endpoint_pair_candidates(
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
    runs: &[CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let expected = zero_entity_endpoint_pair_candidates(derived_zero_entity_endpoint_pairs(runs));
    if endpoint_pairs != expected {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity endpoint-pair candidates disagree with their radial support occurrences"
                .to_string(),
        ));
    }
    Ok(())
}

pub(super) fn derived_zero_entity_endpoint_pairs(
    runs: &[CatiaZeroEntitySupportRun],
) -> Vec<crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate> {
    let mut occurrences = Vec::new();
    for run in runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let midpoints = run
            .supports
            .iter()
            .filter_map(|support| Some((support.record_ordinal, support.model_midpoint?)))
            .collect::<std::collections::HashMap<_, _>>();
        for loop_record in &face.loops {
            for (support_record_ordinal, model_endpoints) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .zip(loop_record.oriented_model_endpoints.iter().copied())
            {
                let Some(model_midpoint) = midpoints.get(&support_record_ordinal).copied() else {
                    continue;
                };
                occurrences.push(
                    crate::families::zero_entity::topology::ZeroEntityOrientedOccurrence {
                        face_record_ordinal: face.record_ordinal,
                        support_record_ordinal,
                        model_endpoints,
                        model_midpoint,
                    },
                );
            }
        }
    }
    crate::families::zero_entity::topology::endpoint_pair_candidates(&occurrences)
}

pub(super) fn validate_zero_entity_endpoint_locus_candidates(
    endpoint_loci: &[CatiaZeroEntityEndpointLocusCandidate],
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
    runs: &[CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let derived_pairs = derived_zero_entity_endpoint_pairs(runs);
    let expected = zero_entity_endpoint_locus_candidates(
        crate::families::zero_entity::topology::endpoint_locus_candidates(&derived_pairs),
        endpoint_pairs,
    );
    if endpoint_loci != expected {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity endpoint-locus candidates disagree with their endpoint-pair endpoints"
                .to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_zero_entity_records(
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let valid = records.iter().enumerate().all(|(index, record)| {
        let ordinal = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1));
        Some(record.record_ordinal) == ordinal
            && record.id == format!("catia:zero-entity:record#{}", record.record_ordinal)
            && record.logical_end > record.byte_offset
            && (index == 0 || records[index - 1].logical_end <= record.byte_offset)
    });
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity record namespace is structurally invalid".to_string(),
        ))
    }
}

pub(super) fn validate_zero_entity_ownership_roots(
    roots: &[CatiaZeroEntityOwnershipRoot],
    support_runs: &[CatiaZeroEntitySupportRun],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let bound_face_count = support_runs.iter().filter(|run| run.face.is_some()).count();
    let valid = roots.len() <= 1
        && roots.iter().all(|root| {
            root.id == "catia:zero-entity:ownership-root#0"
                && root.face_slots.len() == bound_face_count
                && root
                    .face_slots
                    .iter()
                    .copied()
                    .eq((1..=u32::try_from(bound_face_count).unwrap_or(0)).rev())
                && [
                    (
                        root.face_roster_record_ordinal,
                        root.face_roster_byte_offset,
                        [0x61, 0x42],
                    ),
                    (
                        root.shell_record_ordinal,
                        root.shell_byte_offset,
                        [0x60, 0x06],
                    ),
                    (
                        root.body_record_ordinal,
                        root.body_byte_offset,
                        [0x65, 0x08],
                    ),
                ]
                .into_iter()
                .all(|(ordinal, byte_offset, tag)| {
                    zero_entity_record(records, ordinal).is_some_and(|record| {
                        record.byte_offset == byte_offset && record.tag == tag
                    })
                })
                && root.shell_record_ordinal == root.face_roster_record_ordinal.saturating_add(1)
                && root.body_record_ordinal == root.shell_record_ordinal.saturating_add(1)
        });
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity ownership root is structurally invalid".to_string(),
        ))
    }
}

pub(super) fn validate_zero_entity_topology_records(
    edge_strides: &[CatiaZeroEntityEdgeStride],
    oriented_use_pairs: &[CatiaZeroEntityOrientedUsePair],
    vertex_incidences: &[CatiaZeroEntityVertexIncidence],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let edge_strides_valid = edge_strides.iter().enumerate().all(|(index, record)| {
        record.id == format!("catia:zero-entity:edge-stride#{index}")
            && record.record_ordinal != 0
            && !record.allocations.contains(&0)
            && record.allocations[0].checked_sub(1) == Some(record.allocations[3])
            && record.allocations[0].checked_sub(2) == Some(record.allocations[4])
            && record.topology_refs
                == [
                    record.allocations[0],
                    record.allocations[3],
                    record.allocations[4],
                ]
            && record.surface_support_refs == [record.allocations[1], record.allocations[2]]
            && zero_entity_record(records, record.record_ordinal).is_some_and(|source| {
                source.byte_offset == record.byte_offset && source.tag == [0x5e, 0x1a]
            })
            && (index == 0
                || edge_strides[index - 1].byte_offset < record.byte_offset
                    && edge_strides[index - 1].record_ordinal < record.record_ordinal)
    });
    let pairs_valid = oriented_use_pairs.iter().enumerate().all(|(index, pair)| {
        pair.id == format!("catia:zero-entity:oriented-use-pair#{index}")
            && pair.header_record_ordinal != 0
            && zero_entity_record(records, pair.header_record_ordinal).is_some_and(|source| {
                source.byte_offset == pair.header_byte_offset && source.tag == [0x25, 0x69]
            })
            && (index == 0
                || oriented_use_pairs[index - 1].header_byte_offset < pair.header_byte_offset
                    && oriented_use_pairs[index - 1].header_record_ordinal
                        < pair.header_record_ordinal)
            && pair.uses.iter().enumerate().all(|(use_index, use_)| {
                let side = use_index as u32 + 1;
                use_.side == side
                    && !use_.allocations.contains(&0)
                    && zero_entity_record(records, use_.record_ordinal).is_some_and(|source| {
                        source.byte_offset == use_.byte_offset && source.tag == [0x06, 0x38]
                    })
                    && use_.byte_offset > pair.header_byte_offset
                    && (use_index == 0 || pair.uses[use_index - 1].byte_offset < use_.byte_offset)
                    && use_.record_ordinal == pair.header_record_ordinal.saturating_add(side)
                    && use_.allocations
                        == [
                            pair.base_columns[0].saturating_add(side),
                            pair.base_columns[1].saturating_add(side),
                        ]
            })
    });
    let incidences_valid = vertex_incidences.iter().enumerate().all(|(index, record)| {
        let expected_count = match record.tag {
            [0x05, 0x0b] => 2,
            [0x05, 0x10] => 3,
            [0x05, 0x15] => 4,
            _ => return false,
        };
        record.id == format!("catia:zero-entity:vertex-incidence#{index}")
            && record.record_ordinal != 0
            && !record.allocations.contains(&0)
            && zero_entity_record(records, record.record_ordinal).is_some_and(|source| {
                source.byte_offset == record.byte_offset && source.tag == record.tag
            })
            && record.allocations.len() == expected_count
            && record.vertex_record.as_deref()
                == zero_entity_vertex_owner(records, record.record_ordinal)
                    .map(|owner| owner.id.as_str())
            && (index == 0
                || vertex_incidences[index - 1].byte_offset < record.byte_offset
                    && vertex_incidences[index - 1].record_ordinal < record.record_ordinal)
    });
    if edge_strides_valid && pairs_valid && incidences_valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity topology records are structurally invalid".to_string(),
        ))
    }
}

pub(super) fn validate_consolidated_owner_packets(
    packets: &[CatiaConsolidatedOwnerPacket],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, packet) in packets.iter().enumerate() {
        let valid_link = packet.allocation_link.is_none_or(|link| {
            link.byte_offset.checked_add(link.byte_len) == Some(packet.byte_offset)
                && link.target.checked_add(1) == packet.payload.final_reference()
        });
        let valid_payload = match &packet.payload {
            CatiaOwnerPacketPayload::FixedNine { numeric_tail, .. } => {
                numeric_tail.header[0] == 0x84
                    && matches!(numeric_tail.header[1], 0x41 | 0xc1)
                    && numeric_tail.header[4] == 0x0d
                    && numeric_tail.lower.iter().all(|value| value.is_finite())
                    && numeric_tail.upper.iter().all(|value| value.is_finite())
                    && numeric_tail.lower[0] < numeric_tail.upper[0]
                    && numeric_tail.lower[1] < numeric_tail.upper[1]
                    && numeric_tail.bounds.iter().all(|bounds| {
                        bounds[0].is_finite() && bounds[1].is_finite() && bounds[0] < bounds[1]
                    })
            }
            CatiaOwnerPacketPayload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty()
            }
        };
        if packet.id != format!("catia:consolidated:owner-packet#{:010}", packet.byte_offset)
            || !valid_payload
            || !valid_link
            || index > 0 && packets[index - 1].byte_offset >= packet.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated owner packet `{}` is structurally invalid",
                packet.id
            )));
        }
    }
    Ok(())
}

pub(super) struct ConsolidatedSupportArenas<'a> {
    pub(super) circles: &'a [CatiaConsolidatedCircle],
    pub(super) cones: &'a [CatiaConsolidatedCone],
    pub(super) cylinders: &'a [CatiaConsolidatedCylinder],
    pub(super) embedded_cylinders: &'a [CatiaConsolidatedEmbeddedCylinder],
    pub(super) groups: &'a [CatiaConsolidatedGroup],
    pub(super) planes: &'a [CatiaConsolidatedPlaneCarrier],
    pub(super) spheres: &'a [CatiaConsolidatedSphere],
    pub(super) tori: &'a [CatiaConsolidatedTorus],
}

pub(super) fn validate_consolidated_edge_runs(
    runs: &[CatiaConsolidatedEdgeRun],
    pcurves: &[CatiaConsolidatedPcurve],
    supports: &ConsolidatedSupportArenas<'_>,
    nodes: &[CatiaConsolidatedEdgeNode],
    vertex_identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let pcurves = pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let circles = supports
        .circles
        .iter()
        .map(|circle| (circle.id.as_str(), circle))
        .collect::<HashMap<_, _>>();
    let circle_offsets = circles
        .values()
        .map(|circle| circle.byte_offset)
        .collect::<HashSet<_>>();
    let cone_offsets = supports
        .cones
        .iter()
        .map(|cone| cone.byte_offset)
        .collect::<HashSet<_>>();
    let sphere_offsets = supports
        .spheres
        .iter()
        .map(|sphere| sphere.byte_offset)
        .collect::<HashSet<_>>();
    let torus_offsets = supports
        .tori
        .iter()
        .map(|torus| torus.byte_offset)
        .collect::<HashSet<_>>();
    let cylinder_offsets = supports
        .cylinders
        .iter()
        .map(|cylinder| cylinder.byte_offset)
        .collect::<HashSet<_>>();
    let group_offsets = supports
        .groups
        .iter()
        .map(|group| (group.id.as_str(), group.byte_offset))
        .collect::<HashMap<_, _>>();
    let embedded_cylinder_offsets = supports
        .embedded_cylinders
        .iter()
        .filter_map(|cylinder| {
            Some((
                cylinder.byte_offset,
                *group_offsets.get(cylinder.group.as_str())?,
            ))
        })
        .collect::<HashSet<_>>();
    let plane_offsets = supports
        .planes
        .iter()
        .filter(|plane| valid_consolidated_plane_geometry(&plane.payload))
        .map(|plane| plane.byte_offset)
        .collect::<HashSet<_>>();
    let mut run_nodes = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let token_limit = 1u32.checked_shl(u32::from(node.width) * 8);
        let uses_valid = node.uses.as_ref().is_none_or(|uses| {
            node.curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses.references == [[first, second], [second, node.curve_ref]]
                })

                && node.parameter_selectors == [2, 1]
        });
        let definition_valid = node.definition.as_ref().is_none_or(|definition| {
            let token_limit = 1u32.checked_shl(u32::from(u8::from(definition.width)) * 8);
            let expected_data =
                crate::families::consolidated::records::consolidated_edge_definition_data(
                    definition.class,
                    &definition.payload,
                );
            node.uses.is_some()
                && matches!(definition.class, 0x23..=0x25)
                && token_limit.is_some_and(|limit| definition.header_token < limit)
                && !definition.payload.is_empty()
                && definition.byte_offset < node.byte_offset
                && definition.data == expected_data
        });
        let analytic_circle_valid = node.analytic_circle.as_ref().is_none_or(|binding| {
            let definition = node.definition.as_ref();
            let circle = circles.get(binding.circle.as_str());
            node.uses.is_some()
                && definition.is_some_and(|definition| {
                    definition.class == 0x23
                        && matches!(
                            definition.data,
                            Some(ConsolidatedEdgeDefinitionData::Scalar {
                                ref values,
                                ..
                            }) if values.len() == 8
                        )
                        && circle.is_some_and(|circle| {
                            binding.descriptor.byte_offset < circle.byte_offset
                                && circle.byte_offset < definition.byte_offset
                        })
                })
                && 1u32
                    .checked_shl(u32::from(u8::from(binding.descriptor.width)) * 8)
                    .is_some_and(|limit| binding.descriptor.header_token < limit)
                && !binding.descriptor.payload.is_empty()
        });
        let class25_descriptor_valid = node.class25_descriptor.as_ref().is_none_or(|descriptor| {
            node.uses.is_some()
                && node.definition.as_ref().is_some_and(|definition| {
                    definition.class == 0x25
                        && matches!(
                            definition.data,
                            Some(
                                ConsolidatedEdgeDefinitionData::Scalar25 { .. }
                                    | ConsolidatedEdgeDefinitionData::SegmentedScalar25 { .. }
                            )
                        )
                        && descriptor.byte_offset < definition.byte_offset
                })
                && matches!(descriptor.control, 0x02 | 0x0a)
                && matches!(descriptor.values.len(), 2 | 3)
                && descriptor.values.iter().all(|value| value.is_finite())
        });
        if node.id != format!("catia:consolidated:edge-node#{index}")
            || !matches!(node.width, 1..=3)
            || !matches!(node.flag, 0x03 | 0x13 | 0x83)
            || token_limit.is_some_and(|limit| node.header_token >= limit)
            || !uses_valid
            || !definition_valid
            || !analytic_circle_valid
            || !class25_descriptor_valid
            || index > 0 && nodes[index - 1].byte_offset >= node.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` is structurally invalid",
                node.id
            )));
        }
    }
    for (index, run) in runs.iter().enumerate() {
        let expected_id = format!("catia:consolidated:edge-run#{index}");
        let pcurve_offsets = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.byte_offset));
        let pcurve_ranges = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.range));
        let Some(node) = nodes_by_id.get(run.node.as_str()) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` references missing node `{}`",
                run.id, run.node
            )));
        };
        if !run_nodes.insert(run.node.as_str()) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` belongs to multiple runs",
                run.node
            )));
        }
        let loci_valid = run.shared_loci.as_ref().map_or_else(
            || run.endpoint_loci.is_none(),
            |loci| {
                loci.len() >= 2
                    && loci.iter().flatten().all(|value| value.is_finite())
                    && run.endpoint_loci
                        == loci
                            .first()
                            .copied()
                            .zip(loci.last().copied())
                            .map(|(first, last)| [first, last])
            },
        );
        let bindings_valid = run
            .support_bindings
            .iter()
            .flatten()
            .all(|binding| match binding {
                CatiaConsolidatedSupportBinding::Cylinder { byte_offset } => {
                    cylinder_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::EmbeddedCylinder {
                    byte_offset,
                    wrapper_byte_offset,
                } => embedded_cylinder_offsets.contains(&(*byte_offset, *wrapper_byte_offset)),
                CatiaConsolidatedSupportBinding::Circle { byte_offset } => {
                    circle_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Cone { byte_offset } => {
                    cone_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Sphere { byte_offset } => {
                    sphere_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Torus { byte_offset } => {
                    torus_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Plane { byte_offset } => {
                    plane_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::NurbsCarrier { offset, .. } => offset.is_finite(),
            });
        if run.id != expected_id
            || pcurve_offsets[0] != Some(run.byte_offset)
            || pcurve_offsets[1].is_none()
            || pcurve_offsets[0] >= pcurve_offsets[1]
            || pcurve_offsets[1].is_some_and(|offset| offset >= node.byte_offset)
            || pcurve_ranges != [Some(run.parameter_range), Some(run.parameter_range)]
            || run.parameter_range[0] >= run.parameter_range[1]
            || !run.parameter_range.iter().all(|value| value.is_finite())
            || !run.tolerance.is_finite()
            || run.tolerance < 0.0
            || node.uses.is_none()
            || !matches!(node.tail, 0x01 | 0x21)
            || !bindings_valid
            || !loci_valid
            || index > 0 && runs[index - 1].byte_offset >= run.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    let mut expected_nodes = nodes.to_vec();
    let expected_identities = consolidated_vertex_identities(&mut expected_nodes);
    if expected_nodes != nodes || expected_identities != vertex_identities {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "consolidated vertex identities disagree with edge incidence".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn validate_native_links(
    aliases: &[CatiaAliasRow],
    catalogs: &[CatiaCatalog],
    graphs: &[CatiaObjectGraph],
    segments: &[CatiaFinjplSegment],
    value_blocks: &[CatiaValueBlock],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for catalog in catalogs {
        let count_width = if catalog.declared_count <= 0x50 { 1 } else { 2 };
        let Some(mut expected_offset) = catalog.byte_offset.checked_add(6 + count_width) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an overflowing extent",
                catalog.id
            )));
        };
        let catalog_end = catalog.byte_offset.checked_add(catalog.byte_len);
        if catalog.id != format!("catia:outer:catalog#{:010}", catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an invalid source identity",
                catalog.id
            )));
        }
        for (index, entry) in catalog.entries.iter().enumerate() {
            let next_offset = catalog
                .entries
                .get(index + 1)
                .map(|next| next.byte_offset)
                .or(catalog_end);
            let encoded_len = next_offset.and_then(|next| next.checked_sub(entry.byte_offset));
            let value_len = u64::try_from(entry.value.len()).ok();
            if entry.byte_offset != expected_offset
                || entry.id != format!("catia:outer:catalog-entry#{:010}", entry.byte_offset)
                || !encoded_len.zip(value_len).is_some_and(|(encoded, value)| {
                    matches!(encoded.checked_sub(value), Some(1 | 5))
                })
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog entry `{}` has an invalid source extent",
                    entry.id
                )));
            }
            expected_offset = next_offset.expect("validated catalog end");
        }
        if Some(expected_offset) != catalog_end {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` entries do not cover its frame",
                catalog.id
            )));
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let parsed = container::finjpl_segments(&segment.data, 0, segment.data.len());
        let expected_id = format!("catia:outer:finjpl#{index}");
        if segment.id != expected_id
            || u64::try_from(segment.data.len()).ok() != Some(segment.byte_len)
            || segment.byte_offset.checked_add(segment.byte_len).is_none()
            || !matches!(parsed.as_slice(), [parsed]
                if parsed.range == (0..segment.data.len())
                    && parsed.type_word == segment.type_word
                    && finjpl_family(parsed.kind) == segment.family
                    && parsed.name == segment.name)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "FINJPL segment `{}` has an invalid retained view",
                segment.id
            )));
        }
    }
    if segments
        .windows(2)
        .any(|pair| pair[0].byte_offset.checked_add(pair[0].byte_len) != Some(pair[1].byte_offset))
    {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "CATIA FINJPL segment extents are not contiguous".to_string(),
        ));
    }
    for block in value_blocks {
        if block.id != format!("catia:outer:value-block#{:010}", block.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid source identity",
                block.id
            )));
        }
        let Some(catalog) = catalogs.iter().find(|catalog| catalog.id == block.catalog) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` references missing catalog `{}`",
                block.id, block.catalog
            )));
        };
        if block.byte_offset.checked_add(block.byte_len) != Some(catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` is not adjacent to catalog `{}`",
                block.id, block.catalog
            )));
        }
        let payload_len = u64::try_from(block.payload.len()).ok();
        if block.declared_len.checked_add(1) != Some(block.byte_len)
            || payload_len.and_then(|len| len.checked_add(6)) != Some(block.declared_len)
            || value_block::tokenize(&block.payload) != block.fields
            || value_schema_selections(&block.id, block.byte_offset, &block.fields, catalog)
                != block.schema_selections
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid derived view",
                block.id
            )));
        }
        let mut adjacent_graphs = graphs.iter().filter(|graph| {
            graph.byte_offset.checked_add(graph.byte_len) == Some(block.byte_offset)
        });
        let adjacent_graph = adjacent_graphs.next();
        if adjacent_graphs.next().is_some()
            || block.object_graph.as_deref() != adjacent_graph.map(|graph| graph.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid adjacent graph link",
                block.id
            )));
        }
    }
    for graph in graphs {
        let Some(graph_end) = graph.byte_offset.checked_add(graph.byte_len) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an overflowing extent",
                graph.id
            )));
        };
        let mut expected_record_offset = graph.byte_offset.checked_add(6);
        if graph.id != format!("catia:outer:object-graph#{:010}", graph.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid source identity",
                graph.id
            )));
        }
        if graph.finjpl_segment.as_deref()
            != containing_finjpl_segment(graph.byte_offset, graph.byte_len, segments)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid FINJPL segment link",
                graph.id
            )));
        }
        for record in &graph.records {
            if Some(record.byte_offset) != expected_record_offset
                || record.id != format!("catia:outer:object-record#{:010}", record.byte_offset)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid source extent",
                    record.id
                )));
            }
            expected_record_offset = record.byte_offset.checked_add(record.byte_len);
        }
        if expected_record_offset != Some(graph_end) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` records do not cover its frame",
                graph.id
            )));
        }
        let mut candidates = catalogs
            .iter()
            .filter(|catalog| catalog.byte_offset == graph_end)
            .chain(
                value_blocks
                    .iter()
                    .filter(|block| block.byte_offset == graph_end)
                    .filter_map(|block| {
                        catalogs.iter().find(|catalog| catalog.id == block.catalog)
                    }),
            );
        let catalog = candidates.next();
        if candidates.next().is_some()
            || graph.catalog_byte_offset != catalog.map(|catalog| catalog.byte_offset)
            || graph.catalog.as_deref() != catalog.map(|catalog| catalog.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid schema-catalog link",
                graph.id
            )));
        }
        for record in &graph.records {
            let expected_class = catalog.and_then(|catalog| {
                usize::try_from(record.class_ref()?).ok().and_then(|ordinal| {
                    catalog
                        .entries
                        .get(ordinal)
                        .map(|entry| (entry.id.as_str(), entry.value.as_str()))
                })
            });
            if record.class_entry() != expected_class.map(|(entry, _)| entry)
                || record.class_name() != expected_class.map(|(_, value)| value)
                || record.repeated_reference_schema_selection
                    != repeated_reference_schema_selection(
                        record.repeated_reference_suffix.as_ref(),
                        catalog,
                    )
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid schema class",
                    record.id
                )));
            }
        }
    }
    let mut primary_graphs = graphs.iter().filter(|graph| {
        graph
            .outer_container
            .as_ref()
            .is_some_and(|container| container.class_name == "CATPrtCont")
    });
    let primary_graph = match (primary_graphs.next(), primary_graphs.next()) {
        (Some(graph), None) => Some(graph),
        _ => None,
    };
    for alias in aliases {
        if alias.id != format!("catia:outer:alias-row#{:010}", alias.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has an invalid source identity",
                alias.id
            )));
        }
        let expected = usize::from(alias.entity_record_ordinal)
            .checked_sub(1)
            .and_then(|index| {
                let graph = primary_graph?;
                let record = graph.records.get(index)?;
                Some((
                    graph.id.as_str(),
                    record.id.as_str(),
                    record.design_object.as_deref(),
                ))
            });
        let valid = expected.map_or_else(
            || {
                alias.object_graph.is_none()
                    && alias.object_record.is_none()
                    && alias.design_object.is_none()
            },
            |(graph, record, object)| {
                alias.object_graph.as_deref() == Some(graph)
                    && alias.object_record.as_deref() == Some(record)
                    && alias.design_object.as_deref() == object
            },
        );
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has invalid graph, record, or design-object links",
                alias.id
            )));
        }
        if let Some(group) = &alias.group {
            if group.target_slot != (u32::from(alias.f1[2]) | ((alias.f2 & 0x00ff_ffff) << 8))
                || !object_graph::is_alias_group_storage_prefix(&group.storage_prefix)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "alias row `{}` has invalid group storage",
                    alias.id
                )));
            }
        }
    }
    Ok(())
}
