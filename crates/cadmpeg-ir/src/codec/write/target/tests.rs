// SPDX-License-Identifier: Apache-2.0
use super::resolve_write_request;
use super::TargetRequest;
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::CadIr;
use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::target::TargetCatalog;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn an_empty_native_catalog_has_no_format_identity_request() {
    let ir = CadIr::empty();
    let error = resolve_write_request(&ir, TargetRequest::Inherit, TargetCatalog::empty("cadir"))
        .unwrap_err();

    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("an empty native catalog must refuse without inventing an identity request")
    };
    assert_eq!(
        serde_json::to_value(&refusal).expect("serialize refusal")["refusal"],
        serde_json::json!({"kind": "no_default", "source": {"kind": "no_source"}})
    );
    assert!(refusal.available().is_empty());
}
const CATALOG_WRITE_CATALOG: TargetCatalog = TargetCatalog::new(CATALOG_WRITE_TARGETS, Some(1));

fn catalog_write_ir(source: Option<(&str, Option<&'static str>)>) -> CadIr {
    let mut ir = CadIr::empty();
    ir.source = source.map(|(format, dialect)| match dialect {
        Some(id) => crate::document::SourceMeta::classified(
            DialectLayers::of(DialectMatch::admitted(
                DialectId::parse(id).expect("the test dialect id is valid"),
            )),
            BTreeMap::new(),
        ),
        None => serde_json::from_value(serde_json::json!({
            "identity": {"classification": "unclassified", "format": format},
            "attributes": {},
        }))
        .unwrap(),
    });
    ir
}

#[test]
fn write_request_resolves_an_explicit_on_catalog_target() {
    let ir = catalog_write_ir(None);
    let resolved =
        resolve_write_request(&ir, TargetRequest::Explicit("old"), CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_refuses_an_unknown_explicit_target_with_the_catalog() {
    let ir = catalog_write_ir(None);
    let error = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:missing"),
        CATALOG_WRITE_CATALOG,
    )
    .unwrap_err();
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("expected an unsupported target");
    };
    assert_eq!(
        serde_json::to_value(&refusal).expect("serialize refusal")["refusal"],
        serde_json::json!({"kind": "unknown_explicit", "requested": "test:missing"})
    );
    assert_eq!(
        refusal
            .available()
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        ["test:old", "test:new"]
    );
}

#[test]
fn write_request_inherit_with_a_cross_format_source_uses_the_default() {
    let ir = catalog_write_ir(Some(("other", Some("other:only"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:new");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_refuses_a_same_format_unrecorded_source() {
    let ir = catalog_write_ir(Some(("test", None)));
    let error =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap_err();

    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("an unrecorded same-format source must produce a target refusal")
    };
    assert_eq!(
        serde_json::to_value(&refusal).expect("serialize refusal")["refusal"]["kind"],
        "unrecorded_source"
    );
    assert_eq!(refusal.format(), "test");
}

#[test]
fn write_request_explicit_over_an_unrecorded_source_has_no_recorded_relation() {
    let ir = catalog_write_ir(Some(("test", None)));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:old"),
        CATALOG_WRITE_CATALOG,
    )
    .unwrap();

    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(!resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_preserves_a_same_format_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_preserves_a_same_format_off_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(resolved.entry(), None);
    assert_eq!(resolved.target_id().as_str(), "test:future");
    assert!(resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn catalog_write_explicit_difference_returns_the_displaced_dialect() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:new"),
        CATALOG_WRITE_CATALOG,
    )
    .unwrap();
    let entry = resolved.entry().expect("expected a catalog target");
    let displaced = resolved
        .displaced_source()
        .expect("the explicit target displaces the recorded source");
    assert_eq!(entry.id.as_str(), "test:new");
    assert_eq!(displaced, &cadmpeg_core::dialect_id!("test:old"));
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(
        resolved.displacement_message().as_deref(),
        Some(
        "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved"
        )
    );
}
