// SPDX-License-Identifier: Apache-2.0
//! Native lane-validation findings.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::history::sldprt_with_body_and_resolved_features;
use crate::test_support::history::sldprt_with_compact_relation_pair;
use crate::test_support::native::update_sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn native_validation_rejects_broken_feature_graph() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_histories[0].features[1].tree_parent =
            Some(crate::records::TreeParent::Record {
                record_id: "missing-record".into(),
                source_id: None,
            });
    });
    assert!(
        crate::resolved_features::validate::validate_native(&cadmpeg_test_support::service_decode_context(), decoded.ir()).unwrap()
            .iter()
            .any(|finding| finding.message.contains("missing tree parent"))
    );
}

#[test]
fn native_validation_rejects_broken_history_root_graph() {
    use crate::records::HistoryContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default"/><Feature Name="Root" Type="Custom" id="1"><Feature Name="Nested" Type="Custom" id="2"/></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let history = &mut native.feature_histories[0];
        let nested = history
            .features
            .iter()
            .find(|feature| feature.name == "Nested")
            .unwrap()
            .id
            .clone();
        history.content = vec![
            HistoryContent::Feature(nested),
            HistoryContent::Configuration("missing-configuration".into()),
        ];
    });

    let messages = crate::resolved_features::validate::validate_native(&cadmpeg_test_support::service_decode_context(), decoded.ir()).unwrap()
        .into_iter()
        .map(|finding| finding.message)
        .collect::<Vec<_>>();
    assert!(messages
        .iter()
        .any(|message| message.contains("references nested feature")));
    assert!(messages
        .iter()
        .any(|message| message.contains("references missing configuration")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits configuration")));
    assert!(messages
        .iter()
        .any(|message| message.contains("omits feature")));
}

#[test]
fn native_validation_rejects_orphan_history_records() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_history(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let orphan = decoded
        .ir_mut()
        .native
        .namespace_mut("sldprt")
        .arenas_mut()
        .get_mut("features")
        .unwrap()[0]
        .clone();
    let mut orphan_fields = orphan.fields();
    orphan_fields.insert(
        "parent".into(),
        serde_json::Value::String("missing-history".into()),
    );
    decoded
        .ir_mut()
        .native
        .namespace_mut("sldprt")
        .arenas_mut()
        .get_mut("features")
        .unwrap()[0] =
        cadmpeg_ir::NativeRecord::new(orphan.id(), orphan_fields).expect("valid native identity");
    assert!(
        crate::resolved_features::validate::validate_native(&cadmpeg_test_support::service_decode_context(), decoded.ir()).unwrap()
            .iter()
            .any(|finding| {
                finding.message.contains("invalid owner")
                    && finding.message.contains("missing-history")
            })
    );
}

#[test]
fn native_validation_rejects_edited_relation_binding() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body_and_resolved_features(
                &triangle_body(),
                &[0, 1],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].relation_bindings[0].family =
            crate::records::FeatureInputRelationFamily::LineLineDistance;
    });

    assert!(
        crate::resolved_features::validate::validate_native(&cadmpeg_test_support::service_decode_context(), decoded.ir()).unwrap()
            .iter()
            .any(|finding| {
                finding
                    .message
                    .contains("relation bindings do not match the native payload")
            })
    );
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("relation bindings do not match the native payload"));
}

#[test]
fn native_validation_rejects_edited_relation_instance() {
    let mut source = sldprt_with_compact_relation_pair(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        native.feature_input_lanes[0].relation_instances[0]
            .scalars
            .clear_parameter();
    });

    assert!(
        crate::resolved_features::validate::validate_native(&cadmpeg_test_support::service_decode_context(), decoded.ir()).unwrap()
            .iter()
            .any(|finding| {
                finding
                    .message
                    .contains("relation instances do not match the native payload")
            })
    );
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("relation instances do not match the native payload"));
}

fn native_validation_route_refusal(dimension: cadmpeg_core::decode::ResourceDimension) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let source = sldprt_with_compact_relation_pair(&triangle_body());
    let decoded = SldprtCodec.decode(&mut Cursor::new(&source), &DecodeOptions::default()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let selected = match dimension {
        ResourceDimension::CollectionItems => &mut policy.limits.max_collection_items,
        ResourceDimension::RetainedBytes => &mut policy.limits.max_retained_bytes,
        ResourceDimension::MaterializedBytes => &mut policy.limits.max_materialized_bytes,
        ResourceDimension::RecursionDepth => &mut policy.limits.max_recursion_depth,
        ResourceDimension::WorkUnits => &mut policy.limits.max_work_units,
        _ => panic!("unsupported validation test dimension"),
    };
    *selected = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = SldprtCodec.validate_native(&ctx, decoded.ir()).unwrap_err() else {
        panic!("native validation must propagate a resource refusal");
    };
    assert_eq!(limit.dimension, dimension);
    let selected = match dimension {
        ResourceDimension::CollectionItems => &mut policy.limits.max_collection_items,
        ResourceDimension::RetainedBytes => &mut policy.limits.max_retained_bytes,
        ResourceDimension::MaterializedBytes => &mut policy.limits.max_materialized_bytes,
        ResourceDimension::RecursionDepth => &mut policy.limits.max_recursion_depth,
        ResourceDimension::WorkUnits => &mut policy.limits.max_work_units,
        _ => panic!("unsupported validation test dimension"),
    };
    *selected = limit.used + limit.additional - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    assert!(matches!(SldprtCodec.validate_native(&ctx, decoded.ir()), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == dimension && refusal.operation == limit.operation));
}

#[test]
fn native_validation_route_refuses_collection_limit() {
    native_validation_route_refusal(cadmpeg_core::decode::ResourceDimension::CollectionItems);
}

#[test]
fn native_validation_route_refuses_retained_limit() {
    native_validation_route_refusal(cadmpeg_core::decode::ResourceDimension::RetainedBytes);
}

#[test]
fn native_validation_route_refuses_scoped_limit() {
    native_validation_route_refusal(cadmpeg_core::decode::ResourceDimension::MaterializedBytes);
}

#[test]
fn native_validation_route_refuses_nesting_limit() {
    native_validation_route_refusal(cadmpeg_core::decode::ResourceDimension::RecursionDepth);
}

#[test]
fn native_validation_route_refuses_work_limit() {
    native_validation_route_refusal(cadmpeg_core::decode::ResourceDimension::WorkUnits);
}
