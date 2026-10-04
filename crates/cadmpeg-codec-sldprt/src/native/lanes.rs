// SPDX-License-Identifier: Apache-2.0
use super::SldprtNative;
use crate::records::charged_clone::CloneCharged;
use crate::records::FeatureInputLane;
use crate::resolved_features::assembly::is_supplemental_config_lane;
use crate::resolved_features::bindings::finalize_lane_bindings;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};

pub(super) fn admit(
    native: &SldprtNative,
    ctx: &DecodeContext<'_>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for lane in &native.feature_input_lanes {
        if !crate::resolved_features::names::class_declarations_match(
            ctx,
            &lane.native_payload,
            &lane.id,
            &lane.classes,
        )? {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input class index does not match its native payload".into(),
            ));
        }
        // Every field of an object-name record, the value included, states the
        // payload bytes at `offset`. A rename is a write-side input, never an
        // edit of a stored lane, so any disagreement here is a false statement
        // about the payload and is refused.
        if !crate::resolved_features::names::object_names_structure_match(
            ctx,
            &lane.native_payload,
            &lane.id,
            &lane.names,
        )? {
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
            let characters = || {
                std::char::decode_utf16(crate::resolved_features::names::utf16_units(
                    expected_units,
                ))
            };
            let length = characters().fold(0usize, |length, character| {
                length + character.map_or(0, char::len_utf8)
            });
            let (mut expected, _reservation) =
                ctx.scoped_string(length, "decode SLDPRT native validation name")?;
            expected.extend(characters().filter_map(Result::ok));
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(ctx.format_retained(format_args!(
                    "SolidWorks feature-input name value does not match its native payload: lane {} name {index} states {:?}, its payload states {:?}",
                    lane.id, actual.value, expected
                ), "format SLDPRT native validation error")?));
        }
        let mut entities = lane.sketch_entities.iter();
        for (index, position) in (0..lane.native_payload.len())
            .filter(|offset| {
                crate::resolved_features::markers::sketch_marker_at(&lane.native_payload, *offset)
            })
            .enumerate()
        {
            let Some(entity) = entities.next() else {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                            "SolidWorks feature-input lane {} omits marker at offset {position}",
                            lane.id
                        ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            };
            if usize::try_from(entity.ordinal()).ok() != Some(index) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    ctx.format_retained(
                        format_args!(
                        "SolidWorks feature-input lane expects entity ordinal {index}, found {}",
                        entity.ordinal()
                    ),
                        "format SLDPRT native validation error",
                    )?,
                ));
            }
            if entity.offset() != u64_from_index(position) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(ctx.format_retained(format_args!(
                        "SolidWorks feature-input lane {} omits marker at offset {position} or has an extra or unordered entity", lane.id
                    ), "format SLDPRT native validation error")?));
            }
        }
        if entities.next().is_some() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks sketch entity offset is outside its native payload marker set".into(),
            ));
        }
    }
    let ExpectedLanes {
        pairs: expected,
        storage: _expected_reservation,
    } = expected_lanes_charged(ctx, native)?;
    for (lane, expected_lane) in expected {
        if !crate::resolved_features::scalars::scalar_indices_match(
            &lane.scalars,
            &expected_lane.scalars,
        ) {
            let mismatch =
                lane.scalars
                    .iter()
                    .zip(&expected_lane.scalars)
                    .find(|(actual, expected)| {
                        !crate::resolved_features::scalars::scalar_indices_match(
                            std::slice::from_ref(actual),
                            std::slice::from_ref(expected),
                        )
                    });
            return match mismatch {
                Some((actual, expected)) => Err(cadmpeg_ir::NativeConvertError::InvalidOwner(ctx.format_retained(format_args!(
                        "SolidWorks feature-input scalar index does not match its native payload: {actual:?} != {expected:?}"
                    ), "format SLDPRT native validation error")?)),
                None => Err(cadmpeg_ir::NativeConvertError::InvalidOwner(ctx.format_retained(format_args!(
                        "SolidWorks feature-input scalar index does not match its native payload: count {} != {}",
                        lane.scalars.len(), expected_lane.scalars.len()
                    ), "format SLDPRT native validation error")?)),
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

pub(crate) struct ExpectedLanes<'a, 'ctx> {
    pub(crate) pairs: Vec<(&'a FeatureInputLane, FeatureInputLane)>,
    pub(crate) storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(crate) fn expected_lanes_charged<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    native: &'a SldprtNative,
) -> Result<ExpectedLanes<'a, 'ctx>, cadmpeg_ir::NativeConvertError> {
    let (pairs, storage) =
        ctx.with_scoped_storage("validate SLDPRT expected lane copies", || {
            let primary_count = native
                .feature_input_lanes
                .iter()
                .filter(|lane| !is_supplemental_config_lane(lane))
                .count();
            let mut expected_primary_lanes = Vec::new();
            ctx.reserve_vec(
                &mut expected_primary_lanes,
                primary_count,
                "validate SLDPRT expected primary lanes",
            )?;
            for lane in native
                .feature_input_lanes
                .iter()
                .filter(|lane| !is_supplemental_config_lane(lane))
            {
                expected_primary_lanes
                    .push(lane.clone_charged(ctx, "validate SLDPRT expected primary lane copies")?);
            }
            let supplemental_count = native.feature_input_lanes.len() - primary_count;
            let mut expected_supplemental_lanes = Vec::new();
            ctx.reserve_vec(
                &mut expected_supplemental_lanes,
                supplemental_count,
                "validate SLDPRT expected supplemental lanes",
            )?;
            for lane in native
                .feature_input_lanes
                .iter()
                .filter(|lane| is_supplemental_config_lane(lane))
            {
                expected_supplemental_lanes.push(
                    lane.clone_charged(ctx, "validate SLDPRT expected supplemental lane copies")?,
                );
            }
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
                rebuild_scalar_relations_charged(ctx, lane, scalars)?;
            }
            let mut expected = Vec::new();
            ctx.reserve_vec(
                &mut expected,
                native.feature_input_lanes.len(),
                "validate SLDPRT expected lane pairs",
            )?;
            expected.extend(expected_lane_pairs_impl(
                native,
                expected_primary_lanes,
                expected_supplemental_lanes,
                ctx,
            )?);
            Ok::<_, cadmpeg_ir::NativeConvertError>(expected)
        })?;
    Ok(ExpectedLanes { pairs, storage })
}

fn copy_feature_ref(
    ctx: &DecodeContext<'_>,
    feature: Option<&str>,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    feature
        .map(|feature| {
            let mut copy = String::new();
            ctx.try_reserve_retained_text(
                &mut copy,
                feature.len(),
                "retain SLDPRT native lane owner",
            )?;
            copy.push_str(feature);
            Ok(copy)
        })
        .transpose()
}

fn expected_lane_pairs_impl<'a>(
    native: &'a SldprtNative,
    mut expected_primary_lanes: Vec<FeatureInputLane>,
    mut expected_supplemental_lanes: Vec<FeatureInputLane>,
    ctx: &DecodeContext<'_>,
) -> Result<
    impl Iterator<Item = (&'a FeatureInputLane, FeatureInputLane)> + 'a,
    cadmpeg_ir::NativeConvertError,
> {
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &native.feature_histories,
        &mut expected_primary_lanes,
    )?;
    crate::resolved_features::bindings::bind_scalar_operands(
        ctx,
        &native.feature_histories,
        &mut expected_supplemental_lanes,
    )?;
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
            expected.feature_ref = copy_feature_ref(ctx, actual.feature_ref.as_deref())?;
            expected.links = None;
        }
        for (expected, actual) in expected_lane
            .references
            .iter_mut()
            .zip(&actual_lane.references)
        {
            expected.feature_ref = copy_feature_ref(ctx, actual.feature_ref.as_deref())?;
        }
        for (expected, actual) in expected_lane.scalars.iter_mut().zip(&actual_lane.scalars) {
            expected.feature_ref = copy_feature_ref(ctx, actual.feature_ref.as_deref())?;
        }
        finalize_lane_bindings(ctx, &native.feature_histories, expected_lane)?;
    }
    Ok(native
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
        ))
}

fn rebuild_scalar_relations_charged(
    ctx: &DecodeContext<'_>,
    lane: &mut FeatureInputLane,
    scalars: Vec<crate::records::FeatureInputScalar>,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    lane.scalars = scalars;
    lane.relation_bindings = crate::resolved_features::markers::relation_bindings_charged(
        ctx,
        &lane.id,
        &lane.classes,
        &lane.scalars,
    )?;
    lane.references = crate::resolved_features::markers::reference_cells_charged(
        ctx,
        &lane.scalars,
        &lane.classes,
    )?;
    Ok(())
}
