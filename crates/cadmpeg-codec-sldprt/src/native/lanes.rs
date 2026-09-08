// SPDX-License-Identifier: Apache-2.0
use super::SldprtNative;
use crate::records::FeatureInputLane;
use crate::resolved_features::assembly::is_supplemental_config_lane;
use crate::resolved_features::bindings::finalize_lane_bindings;

pub(super) fn admit(native: &SldprtNative) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for lane in &native.feature_input_lanes {
        let expected_classes =
            crate::resolved_features::names::class_declarations(&lane.native_payload, &lane.id);
        if lane.classes != expected_classes {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input class index does not match its native payload".into(),
            ));
        }
        let expected_names =
            crate::resolved_features::names::object_names(&lane.native_payload, &lane.id);
        if lane.names.len() != expected_names.len()
            || lane
                .names
                .iter()
                .zip(&expected_names)
                .any(|(actual, expected)| {
                    actual.id != expected.id
                        || actual.parent != expected.parent
                        || actual.ordinal != expected.ordinal
                        || actual.offset != expected.offset
                })
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "SolidWorks feature-input name structure does not match its native payload".into(),
            ));
        }
        let expected_offsets = (0..lane.native_payload.len())
            .filter(|offset| {
                crate::resolved_features::markers::sketch_marker_at(&lane.native_payload, *offset)
            })
            .map(|offset| offset as u64)
            .collect::<std::collections::HashSet<_>>();
        let actual_offsets = lane
            .sketch_entities
            .iter()
            .map(|entity| entity.offset)
            .collect::<std::collections::HashSet<_>>();
        if let Some(offset) = expected_offsets.difference(&actual_offsets).next() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "SolidWorks feature-input lane {} omits marker at offset {offset}",
                lane.id
            )));
        }
    }
    for (lane, expected_lane) in expected_lanes(native) {
        if !crate::resolved_features::scalars::scalar_indices_match(
            &lane.scalars,
            &expected_lane.scalars,
        ) {
            let detail = lane
                .scalars
                .iter()
                .zip(&expected_lane.scalars)
                .find(|(actual, expected)| {
                    !crate::resolved_features::scalars::scalar_indices_match(
                        std::slice::from_ref(actual),
                        std::slice::from_ref(expected),
                    )
                })
                .map_or_else(
                    || {
                        format!(
                            "count {} != {}",
                            lane.scalars.len(),
                            expected_lane.scalars.len()
                        )
                    },
                    |(actual, expected)| format!("{actual:?} != {expected:?}"),
                );
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "SolidWorks feature-input scalar index does not match its native payload: {detail}"
            )));
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
        lane.scalars = crate::resolved_features::scalars::named_scalars(
            &lane.native_payload,
            &lane.id,
            &lane.names,
        );
        lane.relation_bindings = crate::resolved_features::markers::relation_bindings(
            &lane.id,
            &lane.classes,
            &lane.scalars,
        );
        lane.references =
            crate::resolved_features::markers::reference_cells(&lane.scalars, &lane.classes);
    }
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
        .collect()
}
