use super::*;

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

fn validate_zero_entity_model_curve_construction(
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

fn validate_zero_entity_model_curve(
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

fn derived_zero_entity_endpoint_pairs(
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
