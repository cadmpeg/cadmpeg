// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy, View};

fn failed(ctx: &DecodeContext<'_>, source: View<'_>) -> FinalizedFailureRegistry {
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(
        &mut output,
        source,
        PartSpec::new(
            PartLevel::parse("step.simple_entity_instance").unwrap(),
            vec![],
        ),
        |_| Err::<(), _>(CodecError::malformed("actual invalid field")),
    )
    .unwrap();
    output
        .into_parts(ctx)
        .unwrap()
        .into_parts_with_failure_registry(ctx)
        .unwrap()
        .2
}

#[test]
fn late_dependents_update_only_the_original_located_failure() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=BAD;", &arena, &DecodePolicy::service()).unwrap();
    let mut output = PartOutput::new(CodecFormat::Step);
    ctx.decode_part(
        &mut output,
        source,
        PartSpec::new(
            PartLevel::parse("step.simple_entity_instance").unwrap(),
            vec![],
        ),
        |_| Err::<(), _>(CodecError::malformed("actual invalid field")),
    )
    .unwrap();
    let ((model, native, fidelity, mut losses), dropped, mut registry) = output
        .into_parts(&ctx)
        .unwrap()
        .into_parts_with_failure_registry(&ctx)
        .unwrap();
    assert!(dropped.is_empty());
    let part = fidelity.parts().keys().next().unwrap();
    let home = registry.home(part.as_str()).unwrap();
    assert!(registry.home("invented").is_none());
    let unchanged_native = serde_json::to_value(&native).unwrap();
    let unchanged_fidelity = serde_json::to_value(&fidelity).unwrap();
    let original_code = losses[0].code.clone();
    let original_source = losses[0].provenance.clone();
    registry
        .sponsor_unformed(&ctx, &home, ["step:model:feature#lost".to_owned()])
        .unwrap();
    registry
        .sponsor_unformed(&ctx, &home, ["step:model:feature#lost".to_owned()])
        .unwrap();
    assert!(registry
        .home_for_dropped_record("step:model:feature#lost")
        .is_some());
    registry.apply(&ctx, &fidelity, &mut losses).unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, original_code);
    assert_eq!(losses[0].provenance, original_source);
    assert!(losses[0]
        .message
        .ends_with("dropped=[\"step:model:feature#lost\"]"));
    assert_eq!(serde_json::to_value(&native).unwrap(), unchanged_native);
    assert_eq!(serde_json::to_value(&fidelity).unwrap(), unchanged_fidelity);
    assert!(model.features.is_empty());
    assert!(ctx.take_record_captures().unwrap().is_empty());
    assert!(registry
        .sponsor_unformed(&ctx, &home, ["step:model:feature#sealed".into()])
        .is_err());
}

#[test]
fn failure_handles_reject_other_registries_sessions_and_completed_batches() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=BAD;", &arena, &DecodePolicy::service()).unwrap();
    let mut first = failed(&ctx, source);
    let home = first.handle(0).unwrap();
    let mut second = failed(&ctx, source);
    assert!(second
        .sponsor_unformed(&ctx, &home, ["step:model:feature#foreign".into()])
        .is_err());
    let other_arena = DecodeArena::new();
    let (other_ctx, _) =
        DecodeContext::from_root_bytes(b"#2=BAD;", &other_arena, &DecodePolicy::service()).unwrap();
    assert!(first
        .sponsor_unformed(&other_ctx, &home, ["step:model:feature#foreign".into()])
        .is_err());
    let (mut in_batch, _) = ctx.record_batch(|| Ok(failed(&ctx, source))).unwrap();
    let stale = in_batch.handle(0).unwrap();
    assert!(in_batch
        .sponsor_unformed(&ctx, &stale, ["step:model:feature#stale".into()])
        .is_err());
    drop(first);
    assert!(second
        .sponsor_unformed(&ctx, &home, ["step:model:feature#expired".into()])
        .is_err());
}

fn feature(id: &str, dependencies: Vec<crate::features::FeatureId>) -> crate::features::Feature {
    use crate::features::*;
    Feature {
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: dependencies.try_into().unwrap(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
    }
}

#[test]
fn late_model_closure_removes_required_consumers_and_preserves_independent_peers() {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=BAD;", &arena, &DecodePolicy::service()).unwrap();
    let mut registry = failed(&ctx, source);
    let home = registry.handle(0).unwrap();
    registry
        .sponsor_unformed(&ctx, &home, ["step:model:feature#lost".into()])
        .unwrap();
    let mut model = crate::document::Model::default();
    model.features.push(feature(
        "step:model:feature#consumer",
        vec![crate::features::FeatureId::mint("step:model:feature#lost").unwrap()],
    ));
    model
        .features
        .push(feature("step:model:feature#peer", vec![]));
    model.parameters.push(crate::features::DesignParameter {
        id: crate::features::ParameterId::mint("step:model:parameter#required").unwrap(),
        owner: Some(crate::features::FeatureId::mint("step:model:feature#lost").unwrap()),
        ordinal: 0,
        name: "length".into(),
        expression: crate::features::ParameterValue::Length(
            crate::scalar::Length::new(1.0).unwrap(),
        )
        .literal_expression(),
        driving: true,
        display: None,
        value: None,
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    });
    registry.prune_late_model(&ctx, &mut model).unwrap();
    assert_eq!(model.features.len(), 2);
    assert_eq!(model.features[0].id.as_str(), "step:model:feature#consumer");
    assert!(model.features[0].dependencies.is_empty());
    assert_eq!(model.features[1].id.as_str(), "step:model:feature#peer");
    assert!(model.parameters.is_empty());
    assert!(registry
        .home_for_dropped_record("step:model:parameter#required")
        .is_some());
    assert!(registry
        .home_for_dropped_record("step:model:feature#consumer")
        .is_none());
    assert!(registry
        .home_for_dropped_record("step:model:feature#peer")
        .is_none());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn late_model_drops_retire_own_fragments_without_retiring_same_id_in_another_batch(
) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, source) =
        DecodeContext::from_root_bytes(b"#2=BAD;LRP", &arena, &DecodePolicy::service())?;
    let mut registry = failed(&ctx, source.child(0, 7).expect("failed source"));
    let home = registry.handle(0).expect("actual failed home");
    registry.sponsor_unformed(&ctx, &home, ["step:model:feature#lost".into()])?;
    let lost = "step:model:parameter#required";
    let (earlier_value, earlier_batch) = ctx.record_batch(|| {
        ctx.record(
            lost,
            source.child(7, 8).expect("other batch source"),
            || Ok(source.read_slice(7..8).expect("other batch field")[0]),
        )
    })?;
    assert_eq!(earlier_value, b'L');
    let ((name, ordinal), owner) =
        ctx.record_owner(lost, source.child(7, 9).expect("parameter source"), || {
            let name =
                char::from(source.read_slice(7..8).expect("actual parameter name")[0]).to_string();
            Ok((name, 0u32))
        })?;
    let ordinal =
        ctx.record_fragment(&owner, source.child(8, 9).expect("ordinal source"), || {
            Ok(ordinal + u32::from(source.read_slice(8..9).expect("actual ordinal")[0]))
        })?;
    let mut model = crate::document::Model::default();
    model.parameters.push(crate::features::DesignParameter {
        id: crate::features::ParameterId::mint(lost).expect("parameter identity"),
        owner: Some(
            crate::features::FeatureId::mint("step:model:feature#lost").expect("feature identity"),
        ),
        ordinal,
        name,
        expression: crate::features::ParameterValue::Length(
            crate::scalar::Length::new(1.0).expect("finite length"),
        )
        .literal_expression(),
        driving: true,
        display: None,
        value: None,
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    });
    let peer_id = "step:model:feature#peer";
    let peer = ctx.record(peer_id, source.child(9, 10).expect("peer source"), || {
        let mut peer = feature(peer_id, vec![]);
        peer.name =
            Some(char::from(source.read_slice(9..10).expect("actual peer name")[0]).to_string());
        Ok(peer)
    })?;
    model.features.push(peer);
    registry.prune_late_model(&ctx, &mut model)?;
    assert!(model.parameters.is_empty());
    assert_eq!(model.features.len(), 1);
    assert_eq!(model.features[0].name.as_deref(), Some("P"));
    assert!(registry.home_for_dropped_record(lost).is_some());
    let captures = ctx.take_record_captures()?;
    assert_eq!(captures.len(), 2);
    let earlier = captures
        .iter()
        .find(|capture| capture.batch_id() == Some(earlier_batch))
        .expect("same-ID producer in its original batch survives");
    assert_eq!(earlier.id(), lost);
    assert_eq!(
        earlier.reads()["file"],
        std::iter::once(7..8).collect::<Vec<_>>()
    );
    let peer = captures
        .iter()
        .find(|capture| capture.batch_id().is_none())
        .expect("independent current-batch peer survives");
    assert_eq!(peer.id(), peer_id);
    assert_eq!(
        peer.reads()["file"],
        std::iter::once(9..10).collect::<Vec<_>>()
    );
    Ok(())
}
