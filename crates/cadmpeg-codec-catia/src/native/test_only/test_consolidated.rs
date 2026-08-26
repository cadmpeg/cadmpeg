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

pub(super) fn validate_consolidated_class5b5c_records(
    records: &[CatiaConsolidatedClass5b5cRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, record) in records.iter().enumerate() {
        let expected_len = 4u64
            .checked_add(u64::from(record.width))
            .and_then(|len| len.checked_add(u64::try_from(record.payload.len()).ok()?));
        let source_order_valid = index == 0
            || (
                records[index - 1].source_index,
                records[index - 1].source_offset,
            ) < (record.source_index, record.source_offset);
        if record.id != format!("catia:consolidated:class5b5c-record#{index}")
            || !matches!(record.width, 1..=3)
            || !matches!(record.flag, 0x03 | 0x13 | 0x83)
            || !matches!(record.class, 0x5b | 0x5c)
            || expected_len != Some(record.byte_len)
            || !source_order_valid
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated class-0x5b/0x5c record `{}` is structurally invalid",
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
        let compact_len = usize::from(circle.layout).checked_sub(5 * size_of::<f64>() + 9);
        let record_id_fits_layout = matches!(
            (compact_len, circle.record_id),
            (Some(1), 0..=63) | (Some(2), 0..=255) | (Some(3), 0..=65_535)
        );
        if circle.id != format!("catia:consolidated:circle#{index}")
            || !(0x32..=0x34).contains(&circle.layout)
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
                    cone.reference_radius,
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
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-9)
            || cross
                .iter()
                .zip(cone.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-9)
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
            CatiaConsolidatedCylinderPayload::Resolved {
                frame_token,
                axis,
                reference_direction,
            } => {
                let frame_matches_layout = match cylinder.layout {
                    0x52 => {
                        *frame_token == 0x1d
                            && *axis == [1.0, 0.0, 0.0]
                            && *reference_direction == [0.0, 1.0, 0.0]
                    }
                    0x5a => {
                        matches!(*frame_token, 0x19 | 0x1c)
                            && axis[2] == 0.0
                            && *reference_direction == [-axis[1], axis[0], 0.0]
                    }
                    _ => false,
                };
                frame_matches_layout
                    && axis
                        .iter()
                        .chain(reference_direction)
                        .all(|value| value.is_finite())
                    && (squared_length(*axis) - 1.0).abs() <= 1e-9
                    && (squared_length(*reference_direction) - 1.0).abs() <= 1e-9
                    && dot(*axis, *reference_direction).abs() <= 1e-9
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
                cylinder.layout == 0x62
                    && stored_vector
                        .iter()
                        .chain(std::iter::once(range_origin))
                        .all(|value| value.is_finite())
                    && (stored_vector[0].hypot(stored_vector[1]) - 1.0).abs() <= 1e-9
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
            || (squared_length(cylinder.axis) - 1.0).abs() > 1e-9
            || (squared_length(cylinder.reference_direction) - 1.0).abs() > 1e-9
            || dot(cylinder.axis, cylinder.reference_direction).abs() > 1e-9
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
        let payload_valid = point.payload.is_valid_for_layout(point.layout);
        let frame_overhead = point.byte_len.checked_sub(u64::from(point.layout));
        if point.id != format!("catia:consolidated:parameter-point#{index}")
            || !matches!(frame_overhead, Some(5..=7))
            || !matches!(point.prefix, 0x05 | 0x09 | 0x0d | 0x11)
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
            CatiaConsolidatedPlaneCarrierPayload::ScalarLane { values } => (
                carrier.selector,
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
            ) && matches!(carrier.selector, 0xe4 | 0xc4 | 0xec)
            || carrier.selector != selector
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
        && (norm - 1.0).abs() <= 1e-9
        && direction[2].abs() <= 1e-9
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
            || !matches!(revolution.reference_token, 0x08 | 0x0a)
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
            .any(|direction| (squared_length(direction) - 1.0).abs() > 1e-12)
            || cross
                .iter()
                .zip(revolution.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
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
            || (squared_length - 1.0).abs() > 1e-12
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
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-12)
            || dot(sphere.direction_x, sphere.direction_y).abs() > 1e-12
            || dot(sphere.direction_x, sphere.axis).abs() > 1e-12
            || dot(sphere.direction_y, sphere.axis).abs() > 1e-12
            || cross
                .iter()
                .zip(sphere.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
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
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-12)
            || dot(torus.direction_x, torus.direction_y).abs() > 1e-12
            || dot(torus.direction_x, torus.axis).abs() > 1e-12
            || dot(torus.direction_y, torus.axis).abs() > 1e-12
            || cross
                .iter()
                .zip(torus.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
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
