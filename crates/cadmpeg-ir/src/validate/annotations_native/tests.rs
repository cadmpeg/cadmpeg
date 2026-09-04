// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::annotated_entity_json;
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::{examples::unit_cube, NativeNamespace, NativeRecord};
use serde_json::{Map, Value};
use std::collections::HashSet;

#[test]
fn model_entity_wins_when_native_id_collides() {
    let mut ir = unit_cube();
    let id = ir.model.points[0].id.0.clone();
    let mut namespace = NativeNamespace::new(std::num::NonZeroU32::MIN);
    namespace.arenas.insert(
        "records".into(),
        vec![NativeRecord::new(
            id.clone(),
            Map::from_iter([("native_only".into(), Value::Bool(true))]),
        )],
    );
    ir.native.0.insert("collision".into(), namespace);
    let entities = annotated_entity_json(&ir, &HashSet::from([id.as_str()]));
    assert!(entities[&id].get("position").is_some());
    assert!(entities[&id].get("native_only").is_none());
}

#[test]
fn annotation_keys_and_field_paths_are_checked() {
    let ir = unit_cube();
    let mut source_fidelity = crate::SourceFidelity::default();
    let mut annotations = crate::AnnotationBuilder::new();
    let stream = annotations.stream("test:source");
    annotations.note("missing", stream, 0);
    annotations.derived(&ir.model.edges[0].id.0, "not_a_serialized_field");
    source_fidelity.annotations = annotations.build();
    let findings =
        crate::validate_neutral_with_source_fidelity(&ir, &source_fidelity, Vec::new()).findings;
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Error
    }));
    assert!(findings.iter().any(|finding| {
        finding.check == Check::Annotations && finding.severity == crate::report::Severity::Warning
    }));
}

#[test]
fn native_topology_link_must_resolve() {
    let mut ir = unit_cube();
    ir.native
        .namespace_mut("f3d", std::num::NonZeroU32::MIN)
        .arenas
        .insert(
            "sketch_curve_links".into(),
            vec![NativeRecord::new(
                "native:link#0",
                serde_json::from_value(serde_json::json!({"links": ["missing"]})).unwrap(),
            )],
        );
    ir.native.finalize();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::NativeLinks));
}

#[test]
fn parameter_native_ref_must_resolve() {
    let mut ir = unit_cube();
    let id = crate::features::ParameterId("synthetic:test:parameter#native-ref".into());
    ir.model.parameters.push(crate::features::DesignParameter {
        id: id.clone(),
        owner: Some(crate::features::FeatureId(
            "synthetic:test:feature#missing".into(),
        )),
        ordinal: 0,
        name: "D1".into(),
        expression: "1mm".into(),
        display: None,
        value: None,
        dependencies: Vec::new(),
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
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks && finding.entity.as_deref() == Some(id.0.as_str())
        }));
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::NativeLinks
                && finding.message.contains("PMI native_ref")
                && finding.entity.as_deref() == Some(id.0.as_str())
        }));
}

#[test]
fn unresolved_unknown_record_link_is_reported_once() {
    let mut ir = unit_cube();
    ir.set_native_unknowns(
        "test",
        &[crate::NativeUnknownRecord {
            id: crate::ids::UnknownId("test:unknown#0".into()),
            links: vec!["test:missing#0".into()],
        }],
    )
    .expect("store unknown record");

    let findings = validate_neutral(&ir, Vec::new()).findings;
    let reported = findings
        .iter()
        .filter(|finding| {
            finding.check == Check::NativeLinks && finding.message.contains("test:missing#0")
        })
        .collect::<Vec<_>>();
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].entity.as_deref(), Some("test:unknown#0"));
}
