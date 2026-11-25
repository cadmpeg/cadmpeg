// SPDX-License-Identifier: Apache-2.0
use super::resolve_write_request;
use super::{
    TargetRequest, TargetResolution, WrittenRow, REQUEST_DISPLACED, SOURCE_DIALECT_DISPLACED,
};
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::CadIr;
use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::target::TargetCatalog;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[test]
fn an_empty_native_catalog_has_no_format_identity_request() {
    let ir = CadIr::empty();
    let error = resolve_write_request(
        &ir,
        TargetRequest::Inherit,
        TargetCatalog::empty("cadir"),
        &[],
    )
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
            "identity": {"kind": "unclassified", "format": format},
            "attributes": {},
        }))
        .unwrap(),
    });
    ir
}

#[test]
fn write_request_resolves_an_explicit_on_catalog_target() {
    let ir = catalog_write_ir(None);
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("old"),
        CATALOG_WRITE_CATALOG,
        &[],
    )
    .unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:old");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn an_explicit_replay_row_is_written_as_the_default_and_flagged() {
    const REPLAY: &[DialectId] = &[cadmpeg_core::dialect_id!("test:archive")];
    let ir = catalog_write_ir(None);
    for token in ["test:archive", "archive"] {
        let resolved = resolve_write_request(
            &ir,
            TargetRequest::Explicit(token),
            CATALOG_WRITE_CATALOG,
            REPLAY,
        )
        .unwrap();
        let default = CATALOG_WRITE_CATALOG.default().unwrap().1;
        assert_eq!(resolved.target_id(), &default.id);
        let message = resolved
            .request_displacement_message()
            .expect("a replay-only row is written displaced");
        assert!(
            message.contains(token) && message.contains(default.id.as_str()),
            "{message}"
        );
    }
    let on_catalog = resolve_write_request(
        &ir,
        TargetRequest::Explicit("old"),
        CATALOG_WRITE_CATALOG,
        REPLAY,
    )
    .unwrap();
    assert_eq!(on_catalog.request_displacement_message(), None);
    assert!(resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:missing"),
        CATALOG_WRITE_CATALOG,
        REPLAY
    )
    .is_err());
}

#[test]
fn write_request_refuses_an_unknown_explicit_target_with_the_catalog() {
    let ir = catalog_write_ir(None);
    let error = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:missing"),
        CATALOG_WRITE_CATALOG,
        &[],
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
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG, &[]).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:new");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

/// The codes of the losses `report_target` charges, and the report target.
fn charged(
    resolved: &impl TargetResolution,
    synthesized: bool,
    row: Option<WrittenRow>,
) -> (String, Vec<(String, String)>) {
    let (target, losses) = resolved.report_target("test", synthesized, row);
    (
        target
            .expect("a catalog write names its target")
            .to_string(),
        losses
            .into_iter()
            .map(|loss| {
                assert_eq!(loss.severity, crate::report::Severity::Warning);
                assert_eq!(loss.code.namespace(), "test");
                (loss.code.local_code().to_owned(), loss.message)
            })
            .collect(),
    )
}

#[test]
fn write_request_inherit_writes_a_same_format_unrecorded_source_as_the_default() {
    let ir = catalog_write_ir(Some(("test", None)));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG, &[]).unwrap();
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:new");
    assert!(!resolved.preserves_source());
    assert!(!resolved.source_preservation_eligible());

    let (target, losses) = charged(&resolved, true, None);
    assert_eq!(target, "test:new");
    assert_eq!(
        losses,
        [(
            SOURCE_DIALECT_DISPLACED.to_owned(),
            "the test source records no dialect; the file is written as test:new".to_owned()
        )]
    );
}

#[test]
fn write_request_inherit_with_an_unrecorded_source_and_no_default_names_both() {
    let ir = catalog_write_ir(Some(("test", None)));
    let error = resolve_write_request(
        &ir,
        TargetRequest::Inherit,
        TargetCatalog::new(CATALOG_WRITE_TARGETS, None),
        &[],
    )
    .unwrap_err();
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("a catalog without a default has nothing to write")
    };
    assert_eq!(
        serde_json::to_value(&refusal).expect("serialize refusal")["refusal"],
        serde_json::json!({"kind": "no_default", "source": {"kind": "unrecorded_dialect"}})
    );
}

#[test]
fn a_preserved_source_synthesized_is_written_as_the_default_and_charged() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG, &[]).unwrap();
    assert_eq!(
        resolved
            .synthesized_row()
            .map(|(_, entry)| entry.id.as_str()),
        Some("test:new")
    );

    assert_eq!(
        charged(&resolved, false, None),
        ("test:future".into(), vec![])
    );
    let (target, losses) = charged(&resolved, true, None);
    assert_eq!(target, "test:new");
    assert_eq!(
        losses,
        [(
            SOURCE_DIALECT_DISPLACED.to_owned(),
            "source dialect test:future was displaced by target dialect test:new; the source \
             dialect identity is not preserved"
                .to_owned()
        )]
    );
}

#[test]
fn a_row_the_writer_states_is_the_report_target_and_names_the_displacement() {
    let row = |target: &str| {
        Some(WrittenRow {
            target: DialectId::parse(target).unwrap(),
            reason: "the stored envelope decides the row".into(),
        })
    };
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let explicit = resolve_write_request(
        &ir,
        TargetRequest::Explicit("new"),
        CATALOG_WRITE_CATALOG,
        &[],
    )
    .unwrap();
    let (target, losses) = charged(&explicit, true, row("test:old"));
    assert_eq!(target, "test:old");
    assert_eq!(
        losses,
        [(
            REQUEST_DISPLACED.to_owned(),
            "requested target new (test:new) is a row the writer cannot reach from this \
             document; the file is written as test:old; the stored envelope decides the row"
                .to_owned()
        )]
    );

    let inherited =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG, &[]).unwrap();
    let (target, losses) = charged(&inherited, true, row("test:new"));
    assert_eq!(target, "test:new");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].0, SOURCE_DIALECT_DISPLACED);
    assert!(
        losses[0].1.contains("test:old")
            && losses[0].1.contains("test:new")
            && losses[0].1.ends_with("the stored envelope decides the row"),
        "{}",
        losses[0].1
    );

    assert_eq!(
        charged(&inherited, true, row("test:old")),
        ("test:old".into(), vec![])
    );
}

#[test]
fn write_request_explicit_over_an_unrecorded_source_has_no_recorded_relation() {
    let ir = catalog_write_ir(Some(("test", None)));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:old"),
        CATALOG_WRITE_CATALOG,
        &[],
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
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG, &[]).unwrap();
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
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG, &[]).unwrap();
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
        &[],
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
        charged(&resolved, true, None),
        (
            "test:new".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved".to_owned()
            )]
        )
    );
}
