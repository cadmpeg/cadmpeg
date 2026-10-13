// SPDX-License-Identifier: Apache-2.0
//! Surface producer indexes follow ownership and payload agreement.

use crate::native::{surface_selection_disagrees_with_payload, validate_lane_selections, SldprtNative};
use crate::records::{FeatureInputSurfaceSelection, FeatureInputSurfaceSelectionKind};
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::NativeConvertError;

const INDEX: &str = "index SLDPRT component path producers";

#[test]
fn native_surface_source_index_waits_for_payload_agreement() {
    let (mut lane, mut record) = super::surface_validation_fixture();
    lane.edge_selections.clear();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, INDEX, None);
    let mut index = None;
    record.components[0].local_id = Some(13);
    assert!(surface_selection_disagrees_with_payload(&ctx, &lane, &record, &[], &mut index).unwrap());
    assert!(index.is_none());
    drop(index);
    lane.surface_selections = vec![record];
    assert!(matches!(validate_lane_selections(&ctx, &lane, &[]), Err(NativeConvertError::InvalidOwner(message))
        if message.contains("surface selection") && message.contains("disagrees with its payload")));
    ctx.finish_session().unwrap();
}

#[test]
fn native_surface_source_index_is_built_once_for_valid_readers() {
    let (mut lane, mut record) = super::surface_validation_fixture();
    let (native, _) = owned_surface_fixture();
    let features = &native.feature_histories[0].features;
    let feature = features.iter().find(|feature| feature.source_value() == Some(7)).unwrap();
    assert_eq!(features.iter().filter(|feature| feature.source_value() == Some(7)).count(), 1);
    let old_signature = record.components[0].type_signature;
    let signature_offset = lane.native_payload.windows(old_signature.len())
        .position(|candidate| candidate == old_signature).unwrap();
    record.components[0].type_signature[4..8].copy_from_slice(&7u32.to_le_bytes());
    lane.native_payload[signature_offset..signature_offset + old_signature.len()]
        .copy_from_slice(&record.components[0].type_signature);
    record.producer_feature_refs = vec![feature.id.clone()];
    record.terminal_feature_ref = Some(feature.id.clone());
    crate::test_support::work_refusal_at(INDEX, |ctx| {
        surface_selection_disagrees_with_payload(ctx, &lane, &record, features, &mut None)
            .map_err(cadmpeg_core::CodecError::from)
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = None;
    assert!(!surface_selection_disagrees_with_payload(&ctx, &lane, &record, features, &mut index).unwrap());
    assert!(index.is_some());
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, INDEX, None);
    assert!(!surface_selection_disagrees_with_payload(&ctx, &lane, &record, features, &mut index).unwrap());
    drop(index);
    ctx.finish_session().unwrap();
}

fn owned_surface_fixture() -> (SldprtNative, FeatureInputSurfaceSelection) {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &crate::test_support::history::resolved_features_payload(&[0, 1]),
    ));
    let decoded = crate::SldprtCodec.decode(
        &mut std::io::Cursor::new(source),
        &DecodeOptions::default(),
    ).unwrap();
    let native = crate::test_support::native::sldprt_native(decoded.ir());
    let lane = &native.feature_input_lanes[0];
    let (_, template) = super::surface_validation_fixture();
    let record = FeatureInputSurfaceSelection {
        parent: lane.id.clone(),
        object_name_ref: lane.names[0].id.clone(),
        feature_ref: native.feature_histories[0].features[0].id.clone(),
        offset: u64::MAX,
        kind: FeatureInputSurfaceSelectionKind::Component,
        ..template
    };
    (native, record)
}

#[test]
fn native_surface_store_rejections_skip_the_source_index() {
    for invalid_owner in [false, true] {
        let (mut native, mut record) = owned_surface_fixture();
        if invalid_owner {
            record.parent = "wrong-lane".into();
        }
        let record_id = record.id.clone();
        native.feature_input_lanes[0].surface_selections = vec![record];
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, INDEX, None);
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        assert!(matches!(native.store(&ctx, &mut namespace), Err(NativeConvertError::InvalidOwner(message))
            if message.contains(&record_id) && message.contains("surface selection")));
        assert!(namespace.arenas().is_empty());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn native_surface_load_payload_rejections_skip_the_source_index() {
    let (native, record) = owned_surface_fixture();
    let service = cadmpeg_test_support::service_decode_context();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&service, &mut namespace).unwrap();
    namespace.set_arena(&service, "feature_input_surface_selections", std::slice::from_ref(&record)).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, INDEX, None);
    assert!(matches!(SldprtNative::load_charged(&ctx, &namespace), Err(NativeConvertError::InvalidOwner(message))
        if message.contains(&record.id) && message.contains("disagrees with its payload")));
    ctx.finish_session().unwrap();
}
