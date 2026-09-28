// SPDX-License-Identifier: Apache-2.0
use super::admission::{admit_temporary_clones, admit_validation_candidates, invalid_owner, NativeAdmission};
use super::SldprtNative;
use cadmpeg_core::decode::DecodeContext;
use crate::records::FeatureInputLane;
use crate::resolved_features::assembly::is_supplemental_config_lane;
use crate::resolved_features::bindings::finalize_lane_bindings;

pub(super) fn admit(
    native: &SldprtNative,
    admission: NativeAdmission<'_, '_>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for lane in &native.feature_input_lanes {
        if !crate::resolved_features::names::class_declarations_match(
            &lane.native_payload,
            &lane.id,
            &lane.classes,
        ) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input class index does not match its native payload".into(),
            ));
        }
        // Every field of an object-name record, the value included, states the
        // payload bytes at `offset`. A rename is a write-side input, never an
        // edit of a stored lane, so any disagreement here is a false statement
        // about the payload and is refused.
        if !crate::resolved_features::names::object_names_structure_match(
            &lane.native_payload,
            &lane.id,
            &lane.names,
        ) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input name structure does not match its native payload".into(),
            ));
        }
        if let Some((index, actual, expected_units)) =
            crate::resolved_features::names::first_object_name_value_mismatch(
                &lane.native_payload,
                &lane.names,
            )
        {
            let characters = || std::char::decode_utf16(crate::resolved_features::names::utf16_units(expected_units));
            let (expected, _reservation) = if let Some(ctx) = admission.context() {
                let length = characters().fold(0usize, |length, character| {
                    length + character.map_or(0, char::len_utf8)
                });
                let (mut expected, reservation) = ctx.reserve_scoped_string(
                    length,
                    "decode SLDPRT native validation name",
                )?;
                expected.extend(characters().filter_map(Result::ok));
                (expected, Some(reservation))
            } else {
                (characters().filter_map(Result::ok).collect::<String>(), None)
            };
            return Err(invalid_owner(
                admission,
                format_args!(
                    "SolidWorks feature-input name value does not match its native payload: lane {} name {index} states {:?}, its payload states {:?}",
                    lane.id, actual.value, expected
                ),
            )?);
        }
        let mut entities = lane.sketch_entities.iter();
        for (index, position) in (0..lane.native_payload.len())
            .filter(|offset| {
                crate::resolved_features::markers::sketch_marker_at(&lane.native_payload, *offset)
            })
            .enumerate()
        {
            let Some(entity) = entities.next() else {
                return Err(invalid_owner(
                    admission,
                    format_args!(
                        "SolidWorks feature-input lane {} omits marker at offset {position}",
                        lane.id
                    ),
                )?);
            };
            if usize::try_from(entity.ordinal()).ok() != Some(index) {
                return Err(invalid_owner(
                    admission,
                    format_args!(
                        "SolidWorks feature-input lane expects entity ordinal {index}, found {}",
                        entity.ordinal()
                    ),
                )?);
            }
            if entity.offset() != position as u64 {
                return Err(invalid_owner(
                    admission,
                    format_args!(
                        "SolidWorks feature-input lane {} omits marker at offset {position} or has an extra or unordered entity", lane.id
                    ),
                )?);
            }
        }
        if entities.next().is_some() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks sketch entity offset is outside its native payload marker set".into(),
            ));
        }
    }
    let _expected_lanes_reservation = admit_temporary_clones(
        admission,
        native.feature_input_lanes.iter(),
        "validate SLDPRT expected lane copies",
    )?;
    if let Some(ctx) = admission.context() {
        ctx.charge_collection_items(
            u64::try_from(native.feature_input_lanes.len()).map_err(|_| {
                ctx.refuse_codec_limit("SLDPRT expected lane pairs", u64::MAX - 1, u64::MAX)
            })?,
            "validate SLDPRT expected lane pairs",
        )?;
    }
    let validation_source_bytes = native
        .feature_input_lanes
        .iter()
        .try_fold(0usize, |bytes, lane| {
            bytes.checked_add(lane.native_payload.len())
        })
        .ok_or_else(|| {
            admission.context().map_or_else(
                || {
                    cadmpeg_ir::NativeConvertError::InvalidOwner(
                        "SLDPRT lane validation byte count overflows".into(),
                    )
                },
                |ctx| {
                    ctx.refuse_codec_limit("validate SLDPRT derived lanes", u64::MAX - 1, u64::MAX)
                        .into()
                },
            )
        })?;
    let _derived_reservation = admit_validation_candidates(
        admission,
        validation_source_bytes,
        "validate SLDPRT derived lanes",
    )?;
    let expected = match admission {
        NativeAdmission::Decode(ctx) => expected_lanes_charged(ctx, native)?,
        NativeAdmission::Cadir => expected_lanes(native),
    };
    for (lane, expected_lane) in expected {
        if !crate::resolved_features::scalars::scalar_indices_match(
            &lane.scalars,
            &expected_lane.scalars,
        ) {
            let mismatch = lane
                .scalars
                .iter()
                .zip(&expected_lane.scalars)
                .find(|(actual, expected)| {
                    !crate::resolved_features::scalars::scalar_indices_match(
                        std::slice::from_ref(actual),
                        std::slice::from_ref(expected),
                    )
                });
            return match mismatch {
                Some((actual, expected)) => Err(invalid_owner(
                    admission,
                    format_args!(
                        "SolidWorks feature-input scalar index does not match its native payload: {actual:?} != {expected:?}"
                    ),
                )?),
                None => Err(invalid_owner(
                    admission,
                    format_args!(
                        "SolidWorks feature-input scalar index does not match its native payload: count {} != {}",
                        lane.scalars.len(), expected_lane.scalars.len()
                    ),
                )?),
            };
        }
        if lane.relation_bindings != expected_lane.relation_bindings {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input relation bindings do not match the native payload".into(),
            ));
        }
        if lane.relation_instances != expected_lane.relation_instances {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input relation instances do not match the native payload"
                    .into(),
            ));
        }
        if lane.references != expected_lane.references {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input reference index does not match its native payload".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn expected_lanes(native: &SldprtNative) -> Vec<(&FeatureInputLane, FeatureInputLane)> {
    let mut expected_primary_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .cloned()
        .collect::<Vec<_>>();
    let mut expected_supplemental_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| is_supplemental_config_lane(lane))
        .cloned()
        .collect::<Vec<_>>();
    for lane in expected_primary_lanes
        .iter_mut()
        .chain(&mut expected_supplemental_lanes)
    {
        let scalars = crate::resolved_features::scalars::named_scalars(
            &lane.native_payload,
            &lane.id,
            &lane.names,
        );
        rebuild_scalar_relations(lane, scalars);
    }
    expected_lane_pairs(native, expected_primary_lanes, expected_supplemental_lanes).collect()
}

fn expected_lanes_charged<'a>(
    ctx: &DecodeContext<'_>,
    native: &'a SldprtNative,
) -> Result<Vec<(&'a FeatureInputLane, FeatureInputLane)>, cadmpeg_ir::NativeConvertError> {
    let primary_count = native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .count();
    let mut expected_primary_lanes = Vec::new();
    ctx.reserve_precharged_vec(
        &mut expected_primary_lanes,
        primary_count,
        "validate SLDPRT expected primary lanes",
    )?;
    expected_primary_lanes.extend(
        native
            .feature_input_lanes
            .iter()
            .filter(|lane| !is_supplemental_config_lane(lane))
            .cloned(),
    );
    let supplemental_count = native.feature_input_lanes.len() - primary_count;
    let mut expected_supplemental_lanes = Vec::new();
    ctx.reserve_precharged_vec(
        &mut expected_supplemental_lanes,
        supplemental_count,
        "validate SLDPRT expected supplemental lanes",
    )?;
    expected_supplemental_lanes.extend(
        native
            .feature_input_lanes
            .iter()
            .filter(|lane| is_supplemental_config_lane(lane))
            .cloned(),
    );
    for lane in expected_primary_lanes
        .iter_mut()
        .chain(&mut expected_supplemental_lanes)
    {
        let scalars = crate::resolved_features::scalars::named_scalars_charged(
            ctx,
            &lane.native_payload,
            &lane.id,
            &lane.names,
        )?;
        rebuild_scalar_relations(lane, scalars);
    }
    let mut expected = Vec::new();
    ctx.reserve_precharged_vec(
        &mut expected,
        native.feature_input_lanes.len(),
        "validate SLDPRT expected lane pairs",
    )?;
    expected.extend(expected_lane_pairs(
        native,
        expected_primary_lanes,
        expected_supplemental_lanes,
    ));
    Ok(expected)
}

fn expected_lane_pairs<'a>(
    native: &'a SldprtNative,
    mut expected_primary_lanes: Vec<FeatureInputLane>,
    mut expected_supplemental_lanes: Vec<FeatureInputLane>,
) -> impl Iterator<Item = (&'a FeatureInputLane, FeatureInputLane)> + 'a {
    crate::resolved_features::bindings::bind_scalar_operands(
        &native.feature_histories,
        &mut expected_primary_lanes,
    );
    crate::resolved_features::bindings::bind_scalar_operands(
        &native.feature_histories,
        &mut expected_supplemental_lanes,
    );
    for (expected_lane, actual_lane) in expected_supplemental_lanes.iter_mut().zip(
        native
            .feature_input_lanes
            .iter()
            .filter(|lane| is_supplemental_config_lane(lane)),
    ) {
        // Detached supplemental objects acquire owners before later projection
        // can replace an unresolved sketch definition. The final model does not
        // retain that intermediate state. Treat the stored owner partition as
        // derived provenance, then re-derive every byte-backed local link from it.
        for (expected, actual) in expected_lane
            .sketch_entities
            .iter_mut()
            .zip(&actual_lane.sketch_entities)
        {
            expected.feature_ref.clone_from(&actual.feature_ref);
            expected.links = None;
        }
        for (expected, actual) in expected_lane
            .references
            .iter_mut()
            .zip(&actual_lane.references)
        {
            expected.feature_ref.clone_from(&actual.feature_ref);
        }
        for (expected, actual) in expected_lane.scalars.iter_mut().zip(&actual_lane.scalars) {
            expected.feature_ref.clone_from(&actual.feature_ref);
        }
        finalize_lane_bindings(&native.feature_histories, expected_lane);
    }
    native
        .feature_input_lanes
        .iter()
        .filter(|lane| !is_supplemental_config_lane(lane))
        .zip(expected_primary_lanes)
        .chain(
            native
                .feature_input_lanes
                .iter()
                .filter(|lane| is_supplemental_config_lane(lane))
                .zip(expected_supplemental_lanes),
        )
}

fn rebuild_scalar_relations(
    lane: &mut FeatureInputLane,
    scalars: Vec<crate::records::FeatureInputScalar>,
) {
    lane.scalars = scalars;
    lane.relation_bindings = crate::resolved_features::markers::relation_bindings(
        &lane.id,
        &lane.classes,
        &lane.scalars,
    );
    lane.references =
        crate::resolved_features::markers::reference_cells(&lane.scalars, &lane.classes);
}
