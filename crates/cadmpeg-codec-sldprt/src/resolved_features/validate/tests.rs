// SPDX-License-Identifier: Apache-2.0
//! Native lane-validation findings.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
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
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .unwrap()
    .iter()
    .any(|finding| finding.message.contains("missing tree parent")));
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
    let mut expected_related = Vec::new();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let history = &mut native.feature_histories[0];
        let nested = history
            .features
            .iter()
            .find(|feature| feature.name == "Nested")
            .unwrap()
            .id
            .clone();
        expected_related = vec![
            nested.clone(),
            history.configurations[0].id.clone(),
            history
                .features
                .iter()
                .find(|feature| feature.name == "Root")
                .unwrap()
                .id
                .clone(),
        ];
        history.content = vec![
            HistoryContent::Feature(nested),
            HistoryContent::Configuration("missing-configuration".into()),
        ];
    });

    let findings = crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir(),
    )
    .unwrap();
    for (message, expected) in [
        ("references nested feature", Some(&expected_related[0])),
        ("references missing configuration", None),
        ("omits configuration", Some(&expected_related[1])),
        ("omits feature", Some(&expected_related[2])),
    ] {
        let finding = findings
            .iter()
            .find(|finding| finding.message.contains(message))
            .unwrap();
        assert_eq!(
            finding.related,
            expected.into_iter().cloned().collect::<Vec<_>>()
        );
    }
    let messages = findings
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
fn native_validation_reports_omitted_history_roots_in_record_order() {
    use crate::records::HistoryContent;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="A"/><Configuration Name="B"/><Configuration Name="C"/><Feature Name="R1" Type="Custom" id="1"/><Feature Name="R2" Type="Custom" id="2"/><Feature Name="R3" Type="Custom" id="3"/><Feature Name="R4" Type="Custom" id="4"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let mut expected = Vec::new();
    update_sldprt_native(&mut decoded.ir_mut(), |native| {
        let history = &mut native.feature_histories[0];
        expected.extend(
            history
                .configurations
                .iter()
                .map(|configuration| format!("omits configuration {}", configuration.id)),
        );
        expected.extend(
            history
                .features
                .iter()
                .filter(|feature| feature.tree_parent.is_none())
                .map(|feature| format!("omits feature {}", feature.id)),
        );
        history.content = vec![HistoryContent::Text("edited".into())];
    });
    assert!(expected.len() >= 7, "{expected:?}");

    let omitted = crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir(),
    )
    .unwrap()
    .into_iter()
    .filter_map(|finding| {
        finding
            .message
            .strip_prefix("SolidWorks history root ")
            .filter(|message| message.starts_with("omits"))
            .map(str::to_owned)
    })
    .collect::<Vec<_>>();
    assert_eq!(omitted, expected);
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
    let mut orphan_fields = orphan.fields().unwrap().clone();
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
        .unwrap()[0] = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(orphan.id()).expect("valid identity"),
        orphan_fields,
    )
    .expect("valid native identity");
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .unwrap()
    .iter()
    .any(|finding| {
        finding.message.contains("invalid owner") && finding.message.contains("missing-history")
    }));
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
    let mut native = crate::test_support::native::sldprt_native(decoded.ir());
    native.feature_input_lanes[0].relation_bindings[0].family =
        crate::records::FeatureInputRelationFamily::LineLineDistance;
    // #209 Rule W1/P: this remains malformed document content independently of
    // source positions: the edited family disagrees with its declared class.
    let mut namespace = decoded.ir().native.namespace("sldprt").unwrap().clone();
    let original = namespace.clone();
    let error = native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("relation binding")
            && error.to_string().contains("inconsistent ownership")
    );
    assert_eq!(namespace, original, "malformed store must be atomic");
    let mut wire = serde_json::to_value(original).unwrap();
    wire["feature_input_relation_bindings"][0]["family"] = serde_json::json!("line_line_distance");
    *decoded.ir_mut().native.namespace_mut("sldprt") = serde_json::from_value(wire).unwrap();
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .unwrap()
    .iter()
    .any(|finding| finding
        .message
        .contains("inconsistent relation class or scalar")));
    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("inconsistent relation class or scalar"));
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

    // #209 Rules W1/P and D1: source agreement gates patching, while the
    // canonical neutral constraint remains the winner over its native copy.
    assert!(crate::resolved_features::validate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .unwrap()
    .is_empty());
    let namespace = decoded.ir().native.namespace("sldprt").unwrap();
    let edited = crate::native::SldprtNative::load(namespace).unwrap();
    assert!(edited.feature_input_lanes[0].relation_instances[0]
        .parameter_scalar_ref()
        .is_none());
    let error = crate::native::source::load(namespace, decoded.source_fidelity()).unwrap_err();
    assert!(error
        .to_string()
        .contains("relation instances do not match the native payload"));
    let plan = SldprtCodec
        .plan(
            cadmpeg_ir::codec::write::EncodeInput::new(
                decoded.ir(),
                Some(decoded.source_fidelity()),
            ),
            cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
        )
        .unwrap();
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let expected = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| constraint.definition.clone())
        .collect::<Vec<_>>();
    assert!(
        !expected.is_empty(),
        "fixture has canonical relation semantics"
    );
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        written
            .ir()
            .model
            .sketch_constraints
            .iter()
            .map(|constraint| constraint.definition.clone())
            .collect::<Vec<_>>(),
        expected,
        "canonical relation semantics survive regeneration"
    );
}

#[test]
fn native_validation_route_refuses_nesting_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let source = sldprt_with_compact_relation_pair(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) =
        SldprtCodec.validate_native(&ctx, decoded.ir()).unwrap_err()
    else {
        panic!("native validation must propagate a resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    policy.limits.max_recursion_depth = limit.used + limit.additional - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    assert!(
        matches!(SldprtCodec.validate_native(&ctx, decoded.ir()), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::RecursionDepth && refusal.operation == limit.operation)
    );
}
