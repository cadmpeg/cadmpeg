// SPDX-License-Identifier: Apache-2.0
//! Admission against available source fidelity, independent of document admission.

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::UnknownId;

use super::SldprtNative;
use crate::records::FeatureInputLane;

/// Load semantic records and check their source statements when fidelity is present.
pub(crate) fn load(
    namespace: &cadmpeg_ir::NativeNamespace,
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> Result<SldprtNative, CodecError> {
    let mut native = SldprtNative::load(namespace)?;
    attach(&mut native, |id| {
        fidelity.retained_record(id.as_str())?.data()
    })?;
    validate(&native)?;
    Ok(native)
}

/// Rehydrate transient source positions without changing document-owned content.
pub(crate) fn attach<'a>(
    native: &mut SldprtNative,
    bytes_for: impl Fn(&UnknownId) -> Option<&'a [u8]>,
) -> Result<(), CodecError> {
    for lane in &mut native.feature_input_lanes {
        let id = crate::records::lane_payload::retained_id(&lane.id)?;
        let Some(bytes) = bytes_for(&id) else {
            continue;
        };
        lane.native_payload.attach(bytes)?;
        let expected = expected_lane(&native.feature_histories, lane)?;
        macro_rules! positions {
            ($($field:ident),* $(,)?) => {$(
                for record in &mut lane.$field {
                    record.offset = expected.$field.iter()
                        .find(|source| source.id == record.id)
                        .and_then(|source| source.offset);
                }
            )*};
        }
        positions!(
            classes,
            names,
            scalars,
            references,
            relation_bindings,
            relation_instances,
            body_selections,
            edge_selections,
            surface_selections,
            generated_surface_identities,
        );
        for scalar in &mut lane.scalars {
            for operand in &mut scalar.operands {
                operand.offset = expected
                    .scalars
                    .iter()
                    .flat_map(|source| &source.operands)
                    .find(|source| source.reference_ref == operand.reference_ref)
                    .and_then(|source| source.offset);
            }
        }
        for relation in &mut lane.relation_instances {
            for operand in &mut relation.operands {
                operand.offset = expected
                    .references
                    .iter()
                    .find(|source| source.id == operand.reference_ref)
                    .and_then(|source| source.offset);
            }
        }
        for entity in &mut lane.sketch_entities {
            let offset = expected
                .sketch_entities
                .iter()
                .find(|source| source.id() == entity.id())
                .and_then(crate::records::SketchInputEntity::offset);
            entity.attach_source_position(offset);
        }
    }
    Ok(())
}

fn expected_lane(
    histories: &[crate::records::FeatureHistory],
    lane: &FeatureInputLane,
) -> Result<FeatureInputLane, CodecError> {
    crate::resolved_features::write_prepare::generated_native_lane(
        histories,
        &lane.id,
        lane.configuration.clone(),
        lane.native_payload.to_vec(),
    )
}

/// Refuse false source statements at the fidelity boundary, never at document load.
pub(crate) fn validate(native: &SldprtNative) -> Result<(), CodecError> {
    for lane in &native.feature_input_lanes {
        if lane.native_payload.is_empty() {
            continue;
        }
        let expected = expected_lane(&native.feature_histories, lane)?;
        let classes_match = lane.classes.len() == expected.classes.len()
            && lane
                .classes
                .iter()
                .zip(&expected.classes)
                .all(|(actual, source)| {
                    actual.id == source.id
                        && actual.parent == source.parent
                        && actual.ordinal == source.ordinal
                        && actual.offset == source.offset
                        && actual.name == source.name
                });
        if !classes_match {
            return mismatch("class index");
        }
        let names_match = lane.names.len() == expected.names.len()
            && lane
                .names
                .iter()
                .zip(&expected.names)
                .all(|(actual, source)| {
                    actual.id == source.id
                        && actual.parent == source.parent
                        && actual.ordinal == source.ordinal
                        && actual.offset == source.offset
                        && actual.object_id == source.object_id
                });
        if !names_match {
            return mismatch("name structure");
        }
        if let Some((actual, source)) = lane
            .names
            .iter()
            .zip(&expected.names)
            .find(|(actual, source)| actual.value != source.value)
        {
            return Err(CodecError::malformed(format_args!(
                "SolidWorks feature-input name value does not match its native payload: lane {} name {} states {:?}, its payload states {:?}",
                lane.id, actual.ordinal, actual.value, source.value,
            )));
        }
        for (entity, source) in lane.sketch_entities.iter().zip(&expected.sketch_entities) {
            if entity.ordinal() != source.ordinal() {
                return Err(CodecError::malformed(format_args!(
                    "SolidWorks feature-input lane expects entity ordinal {}, found {}",
                    source.ordinal(),
                    entity.ordinal(),
                )));
            }
        }
        for entity in &lane.sketch_entities {
            entity
                .validate_against_payload(&lane.native_payload)
                .map_err(CodecError::malformed)?;
        }
        if lane.sketch_entities.len() != expected.sketch_entities.len() {
            return Err(CodecError::malformed(
                "SolidWorks feature-input lane omits marker content",
            ));
        }
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    for (lane, expected) in super::lanes::expected_lanes(&ctx, native)? {
        if lane.native_payload.is_empty() {
            continue;
        }
        if !crate::resolved_features::scalars::scalar_indices_match(
            &lane.scalars,
            &expected.scalars,
        ) {
            return mismatch("scalar index");
        }
        if lane.relation_bindings != expected.relation_bindings {
            return mismatch("relation bindings");
        }
        if lane.relation_instances != expected.relation_instances {
            return mismatch("relation instances");
        }
        if lane.references != expected.references {
            return mismatch("reference index");
        }
    }
    let history_lanes = native
        .feature_input_lanes
        .iter()
        .filter(|lane| {
            !lane.native_payload.is_empty()
                && !crate::resolved_features::assembly::is_supplemental_config_lane(lane)
        })
        .collect::<Vec<_>>();
    if !history_lanes.is_empty() {
        let expected_classes = crate::resolved_features::classes::history_classes(
            &native.feature_histories,
            &history_lanes,
        );
        if native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .zip(expected_classes)
            .any(|(feature, expected)| feature.input_class.as_deref() != expected)
        {
            return Err(CodecError::malformed(
                "history feature classes do not match the feature-input index",
            ));
        }
    }
    ctx.finish_session()?;
    Ok(())
}

fn mismatch<T>(family: &str) -> Result<T, CodecError> {
    Err(CodecError::malformed(format_args!(
        "SolidWorks feature-input {family} does not match its native payload"
    )))
}
