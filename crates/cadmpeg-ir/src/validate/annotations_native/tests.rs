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
    let all_ids = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
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
        None,
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
fn annotation_paths_preserve_utf8_and_empty_components() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let value = serde_value::to_value(serde_json::json!({
        "": {"": true, "β": true},
        "é": {"": true, "β": true}
    }))
    .unwrap();
    for (path, resolves) in [
        ("", true),
        (".", true),
        (".β", true),
        ("é.", true),
        ("é.β", true),
        ("é..β", false),
        ("β", false),
        ("é.γ", false),
    ] {
        assert_eq!(
            super::field_path_resolves(&ctx, &value, path).unwrap(),
            resolves,
            "{path}"
        );
    }
    ctx.finish_session().unwrap();
}

#[test]
fn annotation_path_scan_preserves_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let value = serde_value::Value::Bool(true);
    let Err(CodecError::ResourceLimit(limit)) = super::field_path_resolves(&ctx, &value, "é.β")
    else {
        panic!("path scan must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "annotation field path scan");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn source_annotation_paths_preserve_index_grammar_and_bounds() {
    let record = crate::unknown::UnknownRecord::retained(
        "test:native:record#paths".try_into().unwrap(),
        0,
        Vec::new(),
        vec![
            "test:model:point#first".into(),
            "test:model:point#second".into(),
        ],
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    for (path, expected) in [
        ("id", true),
        ("links", true),
        ("links.0", true),
        ("links.1", true),
        ("links.2", false),
        ("links.+0", true),
        ("links.00", true),
        ("links.", false),
        ("links.-1", false),
        ("links. 0", false),
        ("links.β", false),
        ("links.foo", false),
        ("links.0.extra", false),
        ("link.0", false),
    ] {
        assert_eq!(
            super::source_field_path_resolves(&ctx, &record, path).unwrap(),
            expected,
            "{path}"
        );
    }
    let empty = crate::unknown::UnknownRecord::retained(
        "test:native:record#empty".try_into().unwrap(),
        0,
        Vec::new(),
        Vec::new(),
    );
    assert!(super::source_field_path_resolves(&ctx, &empty, "id").unwrap());
    assert!(!super::source_field_path_resolves(&ctx, &empty, "links").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn source_annotation_path_refusal_preserves_original_error() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let record = crate::unknown::UnknownRecord::retained(
        "test:native:record#paths".try_into().unwrap(),
        0,
        Vec::new(),
        vec!["test:model:point#first".into()],
    );
    // Zero refuses the prefix scan; six admits the prefix and refuses suffix parsing.
    for max_work_units in [0, 6] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = max_work_units;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            super::source_field_path_resolves(&ctx, &record, "links.0")
        else {
            panic!("source path must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "annotation source field path scan");
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}
