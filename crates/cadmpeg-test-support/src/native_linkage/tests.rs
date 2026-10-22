// SPDX-License-Identifier: Apache-2.0
use super::audit_native_linkage;
use serde_json::json;

#[test]
fn detects_nested_value_and_map_key_backlinks_with_exact_pointers() {
    let document = json!({
        "model":{"appearances":[{"id":"neutral:appearance#1","native_ref":"native:material#7"}]},
        "native":{"codec":{"materials":[{"id":"native:material#7","nested":[{"a/b~c":"neutral:appearance#1"}],"lookup":{"neutral:appearance#1":true}}]}}
    });
    let result = audit_native_linkage(&document).unwrap();
    assert_eq!(result.neutral_records, 1);
    assert_eq!(result.native_records, 1);
    assert_eq!(result.checked_native_refs, 1);
    assert_eq!(result.findings, [
        "/native/codec/materials/0/lookup/neutral:appearance#1: native map key equals neutral id `neutral:appearance#1`",
        "/native/codec/materials/0/nested/0/a~1b~0c: native string equals neutral id `neutral:appearance#1`",
    ]);
}

#[test]
fn native_identity_precedence_requires_an_actual_row_not_an_id_prefix() {
    let mut document = json!({
        "model":{"assets":[{"id":"codec:native:asset#shared"}],"attributes":[{"id":"codec:native:attribute#only-neutral"}]},
        "native":{"codec":{"assets":[{"id":"codec:native:asset#shared","other":"codec:native:attribute#only-neutral","text":"prefix codec:native:asset#shared suffix"}]}}
    });
    let result = audit_native_linkage(&document).unwrap();
    assert_eq!(result.findings, ["/native/codec/assets/0/other: native string equals neutral id `codec:native:attribute#only-neutral`"]);
    document["native"]["codec"]["assets"][0]["other"] = json!("codec:native:asset#shared");
    assert!(audit_native_linkage(&document).unwrap().findings.is_empty());
}

#[test]
fn detects_dangling_inward_link_but_allows_authored_absence() {
    let document = json!({
        "model":{"tessellations":[{"id":"neutral:mesh#1","native_ref":"native:mesh#missing"}],"pmi":[{"id":"neutral:pmi#authored"}]},
        "native":{}
    });
    let result = audit_native_linkage(&document).unwrap();
    assert_eq!(result.neutral_records, 2);
    assert_eq!(result.checked_native_refs, 1);
    assert_eq!(result.findings, ["/model/tessellations/0/native_ref: native_ref `native:mesh#missing` has no native record"]);
}

#[test]
fn links_can_resolve_to_a_record_in_any_actual_native_arena() {
    let document = json!({
        "model":{"presentation_layers":[{"id":"neutral:layer#1","native_ref":"native:layer#2"}]},
        "native":{"codec":{"unrelated":[{"id":"native:unrelated#1"}],"layers":[{"id":"native:layer#2","name":"layer"}]}}
    });
    let result = audit_native_linkage(&document).unwrap();
    assert_eq!(result.native_records, 2);
    assert_eq!(result.checked_native_refs, 1);
    assert!(result.findings.is_empty());
}

#[test]
fn unavailable_or_unregistered_arenas_cannot_pass_as_empty_coverage() {
    assert!(
        audit_native_linkage(&json!({"model":{},"native":{"__elided":"native omitted"}}))
            .unwrap_err()
            .contains("/native/__elided")
    );
    assert!(
        audit_native_linkage(&json!({"model":{"invented_arena":[]},"native":{}}))
            .unwrap_err()
            .contains("not a registered neutral arena")
    );
    assert!(
        audit_native_linkage(&json!({"model":{"appearances":[{}]},"native":{}}))
            .unwrap_err()
            .contains("/model/appearances/0/id")
    );
    assert!(
        audit_native_linkage(&json!({"model":{},"native":{"codec":{"objects":[{}]}}}))
            .unwrap_err()
            .contains("/native/codec/objects/0/id")
    );
}

#[test]
fn live_document_audit_counts_every_actual_row_before_elision() {
    let mut document = cadmpeg_ir::CadIr::empty();
    let point = cadmpeg_ir::ids::PointId::mint("test:model:point#0").expect("point identity");
    document.model.points.push(cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
            .expect("finite point"),
        None,
    ));
    document.native.namespace_mut("codec").arenas_mut().insert(
        "objects".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("test:native:object#0").expect("native identity"),
            serde_json::Map::from_iter([("name".into(), json!("authored name"))]),
        )
        .expect("native object")],
    );
    let audit = super::assert_native_linkage(&document);
    assert_eq!(audit.neutral_records, 1);
    assert_eq!(audit.native_records, 1);
    assert!(audit.findings.is_empty());
    let mut snapshot = serde_json::to_value(&document).expect("snapshot document");
    snapshot["native"] = json!({"__elided":"native rows are not available"});
    assert!(audit_native_linkage(&snapshot).is_err());
    assert_eq!(super::assert_native_linkage(&document).native_records, 1);
}

#[test]
#[should_panic(expected = "check 8 found 1 native linkage violations")]
fn live_document_audit_refuses_a_backlink_in_an_unelided_nested_row() {
    let mut document = cadmpeg_ir::CadIr::empty();
    let point = cadmpeg_ir::ids::PointId::mint("test:model:point#0").expect("point identity");
    document.model.points.push(cadmpeg_ir::topology::Point::new(
        point,
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
            .expect("finite point"),
        None,
    ));
    document.native.namespace_mut("codec").arenas_mut().insert(
        "objects".into(),
        vec![cadmpeg_ir::NativeRecord::new(
            cadmpeg_ir::ids::Identity::new("test:native:object#0").expect("native identity"),
            serde_json::Map::from_iter([("nested".into(), json!({"target":"test:model:point#0"}))]),
        )
        .expect("native object")],
    );
    super::assert_native_linkage(&document);
}
