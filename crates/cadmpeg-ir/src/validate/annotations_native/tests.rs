// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::check_annotations;
use crate::report::check::Check;
use crate::validate::validate_neutral;
use crate::{examples::unit_cube, NativeNamespace, NativeRecord};
use serde_json::{Map, Value};

#[test]
fn model_entity_wins_when_native_id_collides() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let id = ir.model.points[0].id.as_str().to_owned();
    let mut namespace = NativeNamespace::default();
    namespace.arenas_mut().insert(
        "records".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new(id.clone()).expect("valid identity"),
            Map::from_iter([("native_only".into(), Value::Bool(true))]),
        )
        .expect("valid native identity")],
    );
    ir.native.0.insert("collision".into(), namespace);
    let ctx = cadmpeg_test_support::service_decode_context();
    let all_ids = super::BorrowedIdentities::build(&ctx, |add| add(id.as_str(), ())).unwrap();
    let mut builder = crate::AnnotationBuilder::new();
    builder
        .derived(
            &cadmpeg_test_support::service_decode_context(),
            &id,
            "position",
        )
        .unwrap();
    builder
        .derived(
            &cadmpeg_test_support::service_decode_context(),
            &id,
            "native_only",
        )
        .unwrap();
    let mut findings = Vec::new();
    check_annotations(
        &ctx,
        crate::native::view::NativeView::new(&ir, None),
        &builder.build(),
        &all_ids,
        &mut findings,
    )
    .unwrap();
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("`position`")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("`native_only`")));
}

#[test]
fn annotation_keys_and_field_paths_are_checked() {
    let ir = unit_cube().expect("valid unit cube fixture");
    let mut source_fidelity = crate::SourceFidelity::default();
    let mut annotations = crate::AnnotationBuilder::new();
    let stream = crate::annotations::StreamHandle::new(
        &cadmpeg_test_support::service_decode_context(),
        crate::stream_name!("test:source"),
        "fixture stream handle",
    )
    .unwrap();
    annotations
        .note(
            &cadmpeg_test_support::service_decode_context(),
            "missing",
            &stream,
            0,
            None,
        )
        .unwrap();
    annotations
        .derived(
            &cadmpeg_test_support::service_decode_context(),
            ir.model.edges[0].id.as_str(),
            "not_a_serialized_field",
        )
        .expect("nonempty exactness field");
    source_fidelity.annotations = annotations.build();
    let findings = crate::validate_neutral_with_source_fidelity(&ir, &source_fidelity, Vec::new())
        .expect("resource allocation did not fail")
        .findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Error
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn native_topology_link_must_resolve() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.native.namespace_mut("f3d").arenas_mut().insert(
        "sketch_curve_links".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new("native:test:link#0").expect("valid identity"),
            serde_json::from_value(serde_json::json!({"links": ["missing"]})).unwrap(),
        )
        .expect("valid native identity")],
    );
    ir.native
        .finalize(&cadmpeg_test_support::service_decode_context())
        .expect("fixture ordering is admitted");
    assert!(validate_neutral(&ir, Vec::new())
        .expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn parameter_native_ref_must_resolve() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let id = crate::features::ParameterId::mint("synthetic:test:parameter#native-ref")
        .expect("identity grammar");
    ir.model.parameters.push(crate::features::DesignParameter {
        id: id.clone(),
        owner: Some(
            crate::features::FeatureId::mint("synthetic:test:feature#missing")
                .expect("identity grammar"),
        ),
        ordinal: 0,
        name: "D1".into(),
        expression: "1mm".into(),
        display: None,
        value: None,
        dependencies: crate::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: Some(crate::features::ParameterPmi {
            subtype: crate::features::PmiDimensionSubtype::Linear,
            precision: 2,
            display_text: None,
            basic: false,
            inspection: false,
            reference_only: false,
            native_ref: "native:pmi-missing#0".into(),
        }),
        native_ref: Some("native:missing#0".into()),
    });
    assert!(validate_neutral(&ir, Vec::new())
        .expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks && finding.entity.as_deref() == Some(id.as_str())
        }));
    assert!(validate_neutral(&ir, Vec::new())
        .expect("resource allocation did not fail")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks
                && finding.message.contains("PMI native_ref")
                && finding.entity.as_deref() == Some(id.as_str())
        }));
}

#[test]
fn unresolved_unknown_record_link_is_reported_once() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.set_native_unknowns(
        &cadmpeg_test_support::service_decode_context(),
        "test",
        &[crate::NativeUnknownRecord {
            id: crate::ids::UnknownId::mint("test:model:unknown#0").expect("valid identity"),
            links: vec!["test:model:missing#0".try_into().unwrap()],
        }],
    )
    .expect("store unknown record");

    let findings = validate_neutral(&ir, Vec::new())
        .expect("resource allocation did not fail")
        .findings;
    let reported = findings
        .iter()
        .filter(|finding| {
            finding.check == Check::NativeLinks && finding.message.contains("test:model:missing#0")
        })
        .collect::<Vec<_>>();
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].entity.as_deref(), Some("test:model:unknown#0"));
}

#[test]
fn complete_document_native_link_validation_reports_malformed_shapes() {
    for links in [
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!(3),
        serde_json::json!("test:model:body#0"),
        serde_json::json!({}),
        serde_json::json!([null]),
        serde_json::json!([false]),
        serde_json::json!([3]),
        serde_json::json!([{}]),
        serde_json::json!([[]]),
    ] {
        let mut wire = serde_json::to_value(unit_cube().unwrap()).unwrap();
        wire["native"] = serde_json::json!({"test": {"unknowns": [{
            "id": "test:source:unknown#malformed", "links": links
        }]}});
        let ir = crate::CadIr::from_json(&wire.to_string()).unwrap();
        let findings = validate_neutral(&ir, Vec::new())
            .expect("resource allocation did not fail")
            .findings;
        let found = findings
            .iter()
            .filter(|finding| {
                finding.check == Check::NativeLinks
                    && finding.entity.as_deref() == Some("test:source:unknown#malformed")
            })
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 1, "{findings:?}");
    }
}

#[test]
fn codec_owned_link_payloads_do_not_inherit_unknown_record_shape() {
    for links in [
        serde_json::json!(null),
        serde_json::json!(false),
        serde_json::json!(3),
        serde_json::json!("source-specific value"),
        serde_json::json!({"target": 42}),
        serde_json::json!([null]),
        serde_json::json!([{"object": "source-object", "subelements": ["Face1"]}]),
        serde_json::json!(["source-specific value", {"row": 7}]),
    ] {
        let mut wire = serde_json::to_value(unit_cube().unwrap()).unwrap();
        wire["native"] = serde_json::json!({"test": {"properties": [{
            "id": "test:native:property#links", "links": links
        }]}});
        let ir = crate::CadIr::from_json(&wire.to_string()).unwrap();
        let findings = validate_neutral(&ir, Vec::new())
            .expect("resource allocation did not fail")
            .findings;
        assert!(
            findings
                .iter()
                .all(|finding| finding.check != Check::NativeLinks),
            "{findings:?}"
        );
        assert_eq!(serde_json::to_value(&ir).unwrap()["native"], wire["native"]);
    }
}

#[test]
fn native_annotation_paths_borrow_payloads_without_projecting_unrelated_data() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let id = "test:native:record#payload";
    let mut ir = crate::CadIr::empty();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "records".into(),
        vec![NativeRecord::new(
            crate::ids::Identity::new(id).unwrap(),
            Map::from_iter([
                (
                    "payload".into(),
                    Value::Array((0..10_000).map(Value::from).collect()),
                ),
                (
                    "nested".into(),
                    serde_json::json!({"null": null, "rows": [{"value": true}]}),
                ),
            ]),
        )
        .unwrap()],
    );
    let preparation = cadmpeg_test_support::service_decode_context();
    let mut builder = crate::AnnotationBuilder::new();
    for path in [
        "id",
        "payload.9999",
        "nested.null",
        "nested.rows.0.value",
        "payload.10000",
        "missing",
        "id.child",
    ] {
        builder.derived(&preparation, id, path).unwrap();
    }
    let annotations = builder.build();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let all_ids = super::BorrowedIdentities::build(&ctx, |add| add(id, ())).unwrap();
    let mut findings = Vec::new();
    check_annotations(
        &ctx,
        crate::native::view::NativeView::new(&ir, None),
        &annotations,
        &all_ids,
        &mut findings,
    )
    .unwrap();
    assert_eq!(findings.len(), 3);
    for path in ["payload.10000", "missing", "id.child"] {
        assert!(findings
            .iter()
            .any(|finding| finding.message.contains(&format!("`{path}`"))));
    }
}

#[test]
fn annotated_model_entities_use_indexed_identity_lookups() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut ir = crate::CadIr::empty();
    let prototype = unit_cube().unwrap().model.points.remove(0);
    let preparation = cadmpeg_test_support::service_decode_context();
    let mut builder = crate::AnnotationBuilder::new();
    for ordinal in 0..2_000 {
        let mut point = prototype.clone();
        point.id = crate::ids::PointId::mint(format!("test:model:point#{ordinal}")).unwrap();
        builder
            .derived(&preparation, point.id.as_str(), "position.x")
            .unwrap();
        ir.model.points.push(point);
    }
    let annotations = builder.build();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Admit indexed sorting and projection work while excluding repeated
    // full-population identity scans for each annotation.
    policy.limits.max_work_units = 20_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let all_ids = super::BorrowedIdentities::build(&ctx, |add| {
        for point in &ir.model.points {
            add(point.id.as_str(), ())?;
        }
        Ok(())
    })
    .unwrap();
    let mut findings = Vec::new();
    check_annotations(
        &ctx,
        crate::native::view::NativeView::new(&ir, None),
        &annotations,
        &all_ids,
        &mut findings,
    )
    .unwrap();
    assert!(findings.is_empty());
}
