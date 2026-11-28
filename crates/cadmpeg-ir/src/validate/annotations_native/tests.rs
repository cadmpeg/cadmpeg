// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::check_annotations;
use crate::report::check::Check;
use crate::validate::validate_neutral;
use crate::{examples::unit_cube, NativeNamespace, NativeRecord};
use cadmpeg_core::serde_erase::json;
use serde_json::{Map, Value};

#[test]
fn model_entity_wins_when_native_id_collides() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let id = ir.geometry.points[0].id.as_str().to_owned();
    let mut namespace = NativeNamespace::default();
    namespace.arenas_mut().insert(
        cadmpeg_core::nonblank_literal!("records"),
        vec![NativeRecord::new(
            crate::ids::Identity::new(id.clone()).expect("valid identity"),
            Map::from_iter([("native_only".into(), Value::Bool(true))]),
        )
        .expect("valid native identity")],
    );
    ir.native.0.insert(crate::codec_id!("collision"), namespace);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let all_ids = super::BorrowedIdentities::build(|add| add(id.as_str(), ()));
    let mut builder = crate::AnnotationBuilder::new();
    builder.derived(&id, "position").unwrap();
    builder.derived(&id, "native_only").unwrap();
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
    let mut source_fidelity =
        crate::SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"test source"));
    let mut annotations = crate::AnnotationBuilder::new();
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    annotations.note("missing", &stream, 0, None);
    annotations
        .derived(ir.geometry.edges[0].id.as_str(), "not_a_serialized_field")
        .expect("nonempty exactness field");
    source_fidelity.annotations = annotations.build();
    let findings = crate::validate_neutral_with_source_fidelity(&ir, &source_fidelity, Vec::new())
        .expect("validate the neutral model")
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
    ir.native
        .namespace_mut(crate::codec_id!("f3d"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("sketch_curve_links"),
            vec![NativeRecord::new(
                crate::ids::Identity::new("native:test:link#0").expect("valid identity"),
                json::from_value(cadmpeg_core::json!({"links": ["missing"]})).unwrap(),
            )
            .expect("valid native identity")],
        );
    ir.native.finalize();
    assert!(validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn parameter_native_ref_must_resolve() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    // The document holds the native namespace that the links name.
    ir.native.namespace_mut(crate::codec_id!("native"));
    let id = crate::features::ParameterId::mint("synthetic:test:parameter#native-ref")
        .expect("identity grammar");
    ir.model
        .parameters
        .push(crate::features::parameters::DesignParameter {
            product_definition: None,
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
            dependencies: crate::features::selections::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: Some(crate::features::parameters::ParameterPmi {
                subtype: crate::features::parameters::PmiDimensionSubtype::Linear,
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
        .expect("validate the neutral model")
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks && finding.entity.as_deref() == Some(id.as_str())
        }));
    assert!(validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
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
        &cadmpeg_core::decode::test_support::service_decode_context(),
        "test",
        &[crate::NativeUnknownRecord {
            id: crate::ids::UnknownId::mint("test:model:unknown#0").expect("valid identity"),
            links: vec!["test:model:missing#0".try_into().unwrap()],
        }],
    )
    .expect("store unknown record");

    let findings = validate_neutral(&ir, Vec::new())
        .expect("validate the neutral model")
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
        cadmpeg_core::json!(null),
        cadmpeg_core::json!(true),
        cadmpeg_core::json!(3),
        cadmpeg_core::json!("test:model:body#0"),
        cadmpeg_core::json!({}),
        cadmpeg_core::json!([null]),
        cadmpeg_core::json!([false]),
        cadmpeg_core::json!([3]),
        cadmpeg_core::json!([{}]),
        cadmpeg_core::json!([[]]),
    ] {
        let mut wire = json::to_value(unit_cube().unwrap()).unwrap();
        wire["native"] = cadmpeg_core::json!({"test": {"unknowns": [{
            "id": "test:source:unknown#malformed", "links": links
        }]}});
        let ir = crate::CadIr::from_json(&wire.to_string()).unwrap();
        let findings = validate_neutral(&ir, Vec::new())
            .expect("validate the neutral model")
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
        cadmpeg_core::json!(null),
        cadmpeg_core::json!(false),
        cadmpeg_core::json!(3),
        cadmpeg_core::json!("source-specific value"),
        cadmpeg_core::json!({"target": 42}),
        cadmpeg_core::json!([null]),
        cadmpeg_core::json!([{"object": "source-object", "subelements": ["Face1"]}]),
        cadmpeg_core::json!(["source-specific value", {"row": 7}]),
    ] {
        let mut wire = json::to_value(unit_cube().unwrap()).unwrap();
        wire["native"] = cadmpeg_core::json!({"test": {"properties": [{
            "id": "test:native:property#links", "links": links
        }]}});
        let ir = crate::CadIr::from_json(&wire.to_string()).unwrap();
        let findings = validate_neutral(&ir, Vec::new())
            .expect("validate the neutral model")
            .findings;
        assert!(
            findings
                .iter()
                .all(|finding| finding.check != Check::NativeLinks),
            "{findings:?}"
        );
        assert_eq!(json::to_value(&ir).unwrap()["native"], wire["native"]);
    }
}

#[test]
fn native_annotation_paths_report_only_unresolvable_paths() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let id = "test:native:record#payload";
    let mut ir = crate::CadIr::empty();
    ir.native
        .namespace_mut(crate::codec_id!("test"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![NativeRecord::new(
                crate::ids::Identity::new(id).unwrap(),
                Map::from_iter([
                    (
                        "payload".into(),
                        Value::Array((0..3).map(Value::from).collect()),
                    ),
                    (
                        "nested".into(),
                        cadmpeg_core::json!({"null": null, "rows": [{"value": true}]}),
                    ),
                ]),
            )
            .unwrap()],
        );
    let mut builder = crate::AnnotationBuilder::new();
    for path in [
        "id",
        "payload.2",
        "nested.null",
        "nested.rows.0.value",
        "payload.3",
        "missing",
        "id.child",
    ] {
        builder.derived(id, path).unwrap();
    }
    let annotations = builder.build();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let all_ids = super::BorrowedIdentities::build(|add| add(id, ()));
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
    for path in ["payload.3", "missing", "id.child"] {
        assert!(findings
            .iter()
            .any(|finding| finding.message.contains(&format!("`{path}`"))));
    }
}

#[test]
fn many_native_annotation_paths_parse_their_record_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    // Scanning the record per path would read 2^28 members.
    const FIELDS: usize = 1 << 14;
    let id = "test:native:record#wide";
    let mut ir = crate::CadIr::empty();
    ir.native
        .namespace_mut(crate::codec_id!("test"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![NativeRecord::new(
                crate::ids::Identity::new(id).unwrap(),
                (0..FIELDS)
                    .map(|index| (format!("field{index:05}"), Value::from(index)))
                    .collect(),
            )
            .unwrap()],
        );
    let mut builder = crate::AnnotationBuilder::new();
    for index in 0..FIELDS {
        builder.derived(id, &format!("field{index:05}")).unwrap();
    }
    builder.derived(id, "field99999").unwrap();
    let annotations = builder.build();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let all_ids = super::BorrowedIdentities::build(|add| add(id, ()));
    let mut findings = Vec::new();
    check_annotations(
        &ctx,
        crate::native::view::NativeView::new(&ir, None),
        &annotations,
        &all_ids,
        &mut findings,
    )
    .unwrap();
    let messages: Vec<_> = findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect();
    assert_eq!(
        messages,
        ["exactness field path `field99999` does not resolve"]
    );
}

#[test]
fn annotations_on_many_model_points_resolve_without_findings() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut ir = crate::CadIr::empty();
    let prototype = unit_cube().unwrap().geometry.points.remove(0);
    let mut builder = crate::AnnotationBuilder::new();
    for ordinal in 0..2_000 {
        let mut point = prototype.clone();
        point.id = crate::ids::PointId::mint(format!("test:model:point#{ordinal}")).unwrap();
        builder.derived(point.id.as_str(), "position.x").unwrap();
        ir.geometry.points.push(point);
    }
    let annotations = builder.build();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let all_ids = super::BorrowedIdentities::build(|add| {
        for point in &ir.geometry.points {
            add(point.id.as_str(), ());
        }
    });
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

#[test]
fn a_native_record_links_no_neutral_record_while_a_stub_may() {
    let cube = unit_cube().unwrap();
    let face = cube.geometry.faces[0].id.as_str().to_owned();
    let native_links = |links: serde_json::Value| {
        let mut wire = json::to_value(&cube).unwrap();
        wire["native"] = cadmpeg_core::json!({"test": {
            "properties": [
                {"id": "test:native:property#a", "links": links},
                {"id": "test:native:property#b", "links": []}
            ],
            "unknowns": [{"id": "test:source:unknown#stub", "links": [face]}]
        }});
        let ir = crate::CadIr::from_json(&wire.to_string()).unwrap();
        validate_neutral(&ir, Vec::new())
            .expect("validate the neutral model")
            .findings
            .into_iter()
            .filter(|finding| finding.check == Check::NativeLinks)
            .collect::<Vec<_>>()
    };

    let findings = native_links(cadmpeg_core::json!(["test:native:property#b"]));
    assert!(findings.is_empty(), "{findings:?}");

    let findings = native_links(cadmpeg_core::json!([face]));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("test:native:property#a")
    );
    assert!(
        findings[0].message.contains("names a neutral record"),
        "{findings:?}"
    );
}

/// The unit cube with one evaluation whose delta holds a copy of its first
/// vertex, and the evaluation's id.
fn cube_with_vertex_state() -> (crate::CadIr, crate::design::ids::EvaluationId) {
    use crate::design::evaluation::{
        Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
    };
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let id = crate::design::ids::EvaluationId::mint("test:design:evaluation#1").unwrap();
    let mut records = crate::document::Topology::default();
    records.vertices.push(ir.geometry.vertices[0].clone());
    ir.design.evaluations.push(Evaluation {
        id: id.clone(),
        scope: crate::ids::ProductDefinitionId::mint("test:model:definition#1").unwrap(),
        configuration: None,
        of: crate::design::refs::Evaluated::Tool(
            crate::features::FeatureId::mint("test:design:feature#1").unwrap(),
        ),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base: StateBase::Empty,
            delta: StateDelta {
                records,
                ..StateDelta::default()
            },
        }),
        native_ref: None,
    });
    (ir, id)
}

fn annotation_findings(
    ir: &crate::CadIr,
    annotations: crate::Annotations,
) -> Vec<crate::report::check::Finding> {
    let mut source_fidelity =
        crate::SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(b"test source"));
    source_fidelity.annotations = annotations;
    crate::validate_neutral_with_source_fidelity(ir, &source_fidelity, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Annotations)
        .collect()
}

#[test]
fn a_key_qualified_by_an_evaluation_resolves_to_its_delta_record() {
    let (ir, evaluation) = cube_with_vertex_state();
    let vertex = &ir.geometry.vertices[0].id;
    let key = crate::annotations::AnnotationKey::in_delta(&evaluation, vertex);
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    let mut annotations = crate::AnnotationBuilder::new();
    annotations.note(&key, &stream, 0, None);
    annotations.derived(&key, "point").unwrap();
    annotations.derived(vertex, "point").unwrap();
    let findings = annotation_findings(&ir, annotations.build());
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_qualified_key_outside_every_delta_is_reported_like_a_dangling_key() {
    let (ir, evaluation) = cube_with_vertex_state();
    let vertex = &ir.geometry.vertices[0].id;
    let other = crate::design::ids::EvaluationId::mint("test:design:evaluation#2").unwrap();
    let keys = [
        crate::annotations::AnnotationKey::in_delta(&other, vertex).to_string(),
        crate::annotations::AnnotationKey::in_delta(&evaluation, &ir.geometry.edges[0].id)
            .to_string(),
        format!("{}  {vertex}", evaluation.as_str()),
        format!(" {vertex}"),
        format!("{} ", evaluation.as_str()),
    ];
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    let mut annotations = crate::AnnotationBuilder::new();
    for key in &keys {
        annotations.note(key, &stream, 0, None);
        annotations.exactness(key, crate::provenance::Exactness::Inferred);
    }
    let findings = annotation_findings(&ir, annotations.build());
    for key in &keys {
        for rule in [
            "annotations_native.provenance-key-does-not-resolve-entity",
            "annotations_native.exactness-key-does-not-resolve-entity",
        ] {
            assert!(
                findings.iter().any(|finding| {
                    finding.severity == crate::report::Severity::Error
                        && finding.entity.as_deref() == Some(key.as_str())
                        && finding.rule.as_str() == rule
                }),
                "{key}: {rule}: {findings:?}"
            );
        }
    }
    assert_eq!(findings.len(), 2 * keys.len(), "{findings:?}");
}

#[test]
fn a_field_path_of_a_delta_record_is_checked_against_that_record() {
    let (ir, evaluation) = cube_with_vertex_state();
    let key = crate::annotations::AnnotationKey::in_delta(&evaluation, &ir.geometry.vertices[0].id);
    let mut annotations = crate::AnnotationBuilder::new();
    annotations.derived(&key, "not_a_serialized_field").unwrap();
    let findings = annotation_findings(&ir, annotations.build());
    assert!(
        matches!(findings.as_slice(), [finding]
            if finding.severity == crate::report::Severity::Warning
                && finding.message.contains("`not_a_serialized_field`")),
        "{findings:?}"
    );
}

#[test]
fn keys_of_design_records_resolve() {
    let ir = crate::validate::design::fixture::document();
    let named = [
        "test:design:feature#extrude",
        "test:design:evaluation#after-extrude",
    ];
    let absent = "test:design:feature#absent";
    let stream = crate::annotations::StreamHandle::new(crate::stream_name!("test:source"));
    let mut annotations = crate::AnnotationBuilder::new();
    for key in named.into_iter().chain([absent]) {
        annotations.note(key, &stream, 0, None);
        annotations.exactness(key, crate::provenance::Exactness::Inferred);
    }
    let findings = annotation_findings(&ir, annotations.build());
    for rule in [
        "annotations_native.provenance-key-does-not-resolve-entity",
        "annotations_native.exactness-key-does-not-resolve-entity",
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding.severity == crate::report::Severity::Error
                    && finding.entity.as_deref() == Some(absent)
                    && finding.rule.as_str() == rule
            }),
            "{rule}: {findings:?}"
        );
    }
    assert_eq!(findings.len(), 2, "{findings:?}");
}

#[test]
fn a_field_path_of_a_design_record_is_checked_against_that_record() {
    let ir = crate::validate::design::fixture::document();
    let mut annotations = crate::AnnotationBuilder::new();
    annotations
        .derived("test:design:feature#extrude", "definition")
        .unwrap();
    annotations
        .derived("test:design:feature#fillet", "not_a_serialized_field")
        .unwrap();
    let findings = annotation_findings(&ir, annotations.build());
    assert!(
        matches!(findings.as_slice(), [finding]
            if finding.severity == crate::report::Severity::Warning
                && finding.entity.as_deref() == Some("test:design:feature#fillet")
                && finding.rule.as_str() == "annotations_native.exactness-field-path-does-not-resolve"
                && finding.message.contains("`not_a_serialized_field`")),
        "{findings:?}"
    );
}

/// A design record and the model record it restates share an id, and the
/// CADIR wire carries the design record alone: a field path names a field of
/// the design record.
#[test]
fn a_design_record_takes_no_field_path_from_the_model_record_it_restates() {
    use crate::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    let mut ir = crate::validate::design::fixture::document();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:design:feature#fillet").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ),
        native_ref: None,
    });
    let mut annotations = crate::AnnotationBuilder::new();
    for field in ["definition", "ordinal"] {
        annotations
            .derived("test:design:feature#fillet", field)
            .unwrap();
    }
    let findings = annotation_findings(&ir, annotations.build());
    assert!(
        matches!(findings.as_slice(), [finding]
            if finding.severity == crate::report::Severity::Warning
                && finding.entity.as_deref() == Some("test:design:feature#fillet")
                && finding.message.contains("`ordinal`")),
        "{findings:?}"
    );
}

/// A design scope has the id of its product definition, and the CADIR wire
/// carries both: a field path at that id names a field of either record.
#[test]
fn a_field_path_at_a_definition_id_names_a_field_of_the_definition_or_its_scope() {
    let ir = crate::validate::design::fixture::document();
    let definition = "test:design:definition#part";
    let mut annotations = crate::AnnotationBuilder::new();
    for field in ["active_arrangements", "kind", "not_a_serialized_field"] {
        annotations.derived(definition, field).unwrap();
    }
    let findings = annotation_findings(&ir, annotations.build());
    assert!(
        matches!(findings.as_slice(), [finding]
            if finding.severity == crate::report::Severity::Warning
                && finding.entity.as_deref() == Some(definition)
                && finding.rule.as_str() == "annotations_native.exactness-field-path-does-not-resolve"
                && finding.message.contains("`not_a_serialized_field`")),
        "{findings:?}"
    );
}
