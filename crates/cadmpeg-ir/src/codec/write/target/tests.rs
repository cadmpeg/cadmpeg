// SPDX-License-Identifier: Apache-2.0
use super::{resolve_write_request, Catalog};
use super::{
    TargetRequest, TargetResolution, WrittenRow, REQUEST_DISPLACED, SOURCE_DIALECT_DISPLACED,
};
use crate::codec::write::test_support::CATALOG_WRITE_TARGETS;
use crate::CadIr;
use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

const CATALOG_WRITE_CATALOG: Catalog = Catalog::new(CATALOG_WRITE_TARGETS, Some(1));

fn catalog_write_ir(source: Option<(&str, Option<&'static str>)>) -> CadIr {
    let mut ir = CadIr::empty();
    ir.source = source.map(|(format, dialect)| match dialect {
        Some(id) => crate::document::SourceMeta::classified(
            DialectLayers::of(DialectMatch::admitted(
                DialectId::parse(id).expect("the test dialect id is valid"),
            )),
            BTreeMap::new(),
        ),
        None => cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
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
    let resolved =
        resolve_write_request(&ir, TargetRequest::Explicit("old"), CATALOG_WRITE_CATALOG).unwrap();
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
            CATALOG_WRITE_CATALOG.with_replay_rows(REPLAY),
        )
        .unwrap();
        let default = CATALOG_WRITE_CATALOG.rows.default().unwrap().1;
        assert_eq!(resolved.target_id(), &default.id);
        let (target, losses) = charged(&resolved, true, None);
        assert_eq!(target, default.id.as_str());
        let [(code, message)] = losses.as_slice() else {
            panic!("a replay-only row is written displaced: {losses:?}")
        };
        assert_eq!(code, REQUEST_DISPLACED);
        assert!(
            message.contains(token) && message.contains(default.id.as_str()),
            "{message}"
        );
    }
    let on_catalog = resolve_write_request(
        &ir,
        TargetRequest::Explicit("old"),
        CATALOG_WRITE_CATALOG.with_replay_rows(REPLAY),
    )
    .unwrap();
    assert_eq!(charged(&on_catalog, true, None).1, vec![]);
    assert!(resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:missing"),
        CATALOG_WRITE_CATALOG.with_replay_rows(REPLAY),
    )
    .is_err());
}

/// A catalog that declares no default resolves an explicit replay row to its
/// newest row. A file the writer states as the requested row charges nothing;
/// a file of any other row charges `target.request-displaced`.
#[test]
fn an_explicit_replay_row_of_a_catalog_with_no_default_takes_the_newest_row() {
    const REPLAY: &[DialectId] = &[cadmpeg_core::dialect_id!("test:archive")];
    let catalog = Catalog::new(CATALOG_WRITE_TARGETS, None).with_replay_rows(REPLAY);
    let ir = catalog_write_ir(None);
    let archive = || {
        Some(WrittenRow {
            target: cadmpeg_core::dialect_id!("test:archive"),
            reason: "the stored envelope decides the row".into(),
        })
    };
    for token in ["test:archive", "archive"] {
        let resolved = resolve_write_request(&ir, TargetRequest::Explicit(token), catalog)
            .expect("a replay row resolves without a catalog default");
        assert_eq!(resolved.target_id().as_str(), "test:new");
        assert_eq!(
            charged(&resolved, false, archive()),
            ("test:archive".into(), vec![])
        );
        let (target, losses) = charged(&resolved, false, None);
        assert_eq!(target, "test:new");
        let [(code, message)] = losses.as_slice() else {
            panic!("a file of another row is displaced: {losses:?}")
        };
        assert_eq!(code, REQUEST_DISPLACED);
        assert!(
            message.contains(token) && message.contains("test:new"),
            "{message}"
        );
    }
    assert!(resolve_write_request(&ir, TargetRequest::Explicit("test:missing"), catalog).is_err());
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
        cadmpeg_core::serde_erase::json::to_value(&refusal).expect("serialize refusal")["refusal"],
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

/// The codes of the losses `report_target` charges, and the report target.
fn charged(
    resolved: &impl TargetResolution,
    synthesized: bool,
    row: Option<WrittenRow>,
) -> (String, Vec<(String, String)>) {
    let (target, losses) = resolved.report_target(crate::loss_namespace!("test"), synthesized, row);
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
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
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

/// A catalog that declares no default resolves an `Inherit` request with no
/// recorded dialect to its newest row, and the writer writes a file.
#[test]
fn write_request_inherit_with_no_default_takes_the_newest_row() {
    let catalog = Catalog::new(CATALOG_WRITE_TARGETS, None);
    let unrecorded = catalog_write_ir(Some(("test", None)));
    let resolved = resolve_write_request(&unrecorded, TargetRequest::Inherit, catalog)
        .expect("a catalog without a default still resolves");
    assert_eq!(resolved.target_id().as_str(), "test:new");
    let (target, losses) = charged(&resolved, true, None);
    assert_eq!(target, "test:new");
    assert_eq!(
        losses,
        [(
            SOURCE_DIALECT_DISPLACED.to_owned(),
            "the test source records no dialect; the file is written as test:new".to_owned()
        )]
    );

    for ir in [
        catalog_write_ir(None),
        catalog_write_ir(Some(("other", Some("other:only")))),
    ] {
        let resolved = resolve_write_request(&ir, TargetRequest::Inherit, catalog)
            .expect("a catalog without a default still resolves");
        let (target, losses) = charged(&resolved, true, None);
        assert_eq!(target, "test:new");
        assert_eq!(
            losses
                .iter()
                .map(|(code, _)| code.as_str())
                .collect::<Vec<_>>(),
            [SOURCE_DIALECT_DISPLACED]
        );
    }
}

/// An `Inherit` request names no row, so it charges only
/// `target.source-dialect-displaced`: for a source that records no dialect,
/// a source of another format, and no source, each naming the written row,
/// and the resolved row where the writer states another.
#[test]
fn an_inherit_write_of_another_row_charges_only_the_source_code() {
    let row = Some(WrittenRow {
        target: cadmpeg_core::dialect_id!("test:old"),
        reason: "the stored envelope decides the row".into(),
    });
    let unrecorded = catalog_write_ir(Some(("test", None)));
    let resolved =
        resolve_write_request(&unrecorded, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(
        charged(&resolved, true, row.clone()),
        (
            "test:old".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "the test source records no dialect; the file is written as test:old; the \
                 stored envelope decides the row"
                    .to_owned()
            )]
        )
    );

    let foreign = catalog_write_ir(Some(("other", Some("other:only"))));
    let resolved =
        resolve_write_request(&foreign, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(
        charged(&resolved, true, row.clone()),
        (
            "test:old".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "the document's source is other, which records no test dialect; Inherit \
                 resolved to test:new, and the file is written as test:old; the stored \
                 envelope decides the row"
                    .to_owned()
            )]
        )
    );

    let sourceless = catalog_write_ir(None);
    let resolved =
        resolve_write_request(&sourceless, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(
        charged(&resolved, true, row),
        (
            "test:old".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "the document records no source; Inherit resolved to test:new, and the file \
                 is written as test:old; the stored envelope decides the row"
                    .to_owned()
            )]
        )
    );
    assert_eq!(
        charged(&resolved, true, None),
        (
            "test:new".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "the document records no source; the file is written as test:new".to_owned()
            )]
        ),
        "an Inherit write with no source resolves to the catalog default and charges it"
    );

    let resolved =
        resolve_write_request(&foreign, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(
        charged(&resolved, true, None),
        (
            "test:new".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "the document's source is other, which records no test dialect; the file is \
                 written as test:new"
                    .to_owned()
            )]
        ),
        "an Inherit write from another format resolves to the catalog default and charges it"
    );
}

/// §3.2 pins a strict floor only where it states one with the code, and it
/// states none for either `target.*` code.
#[test]
fn the_target_codes_pin_no_strict_floor() {
    for code in [SOURCE_DIALECT_DISPLACED, REQUEST_DISPLACED] {
        assert_eq!(
            super::target_loss_kind(crate::loss_namespace!("test"), code).strict_floor(),
            None
        );
    }
}

/// A replay-only request written as a row the writer states names the
/// request and the written row.
#[test]
fn a_displaced_request_written_as_a_stated_row_names_the_written_row() {
    const REPLAY: &[DialectId] = &[cadmpeg_core::dialect_id!("test:archive")];
    let ir = catalog_write_ir(None);
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("archive"),
        CATALOG_WRITE_CATALOG.with_replay_rows(REPLAY),
    )
    .unwrap();
    let row = WrittenRow {
        target: cadmpeg_core::dialect_id!("test:old"),
        reason: "the stored envelope decides the row".into(),
    };
    assert_eq!(
        charged(&resolved, true, Some(row)),
        (
            "test:old".into(),
            vec![(
                REQUEST_DISPLACED.to_owned(),
                "requested target archive is a row the writer reaches only by replaying a \
                 source of that dialect; the file is written as test:old; the stored envelope \
                 decides the row"
                    .to_owned()
            )]
        )
    );
}

#[test]
fn a_preserved_source_synthesized_is_written_as_the_default_and_charged() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
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
    let explicit =
        resolve_write_request(&ir, TargetRequest::Explicit("new"), CATALOG_WRITE_CATALOG).unwrap();
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
        resolve_write_request(&ir, TargetRequest::Inherit, CATALOG_WRITE_CATALOG).unwrap();
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

/// A request that names no target takes the catalog default without a loss
/// where the source is of another format or absent, and inherits where it is
/// of this format: a recorded catalog dialect writes as itself, a row the
/// writer states in its place is charged, and a source that records no
/// dialect is charged.
#[test]
fn an_unnamed_request_inherits_only_from_a_source_of_this_format() {
    for ir in [
        catalog_write_ir(None),
        catalog_write_ir(Some(("other", Some("other:only")))),
    ] {
        let resolved = resolve_write_request(&ir, TargetRequest::Unnamed, CATALOG_WRITE_CATALOG)
            .expect("an unnamed request resolves");
        assert_eq!(charged(&resolved, true, None), ("test:new".into(), vec![]));
    }

    let recorded = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved =
        resolve_write_request(&recorded, TargetRequest::Unnamed, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(charged(&resolved, false, None), ("test:old".into(), vec![]));

    let unrecorded = catalog_write_ir(Some(("test", None)));
    let resolved =
        resolve_write_request(&unrecorded, TargetRequest::Unnamed, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(
        charged(&resolved, true, None),
        (
            "test:new".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "the test source records no dialect; the file is written as test:new".to_owned()
            )]
        )
    );

    let recorded = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved =
        resolve_write_request(&recorded, TargetRequest::Unnamed, CATALOG_WRITE_CATALOG).unwrap();
    assert_eq!(
        charged(
            &resolved,
            false,
            Some(stated_row(cadmpeg_core::dialect_id!("test:new")))
        ),
        (
            "test:new".into(),
            vec![(
                SOURCE_DIALECT_DISPLACED.to_owned(),
                "source dialect test:old was displaced by target dialect test:new; the source \
                 dialect identity is not preserved; the stored envelope decides the row"
                    .to_owned()
            )]
        )
    );
}

/// A request that names no target, on a document whose source is of another
/// format or absent, charges nothing whatever row the writer states: no source
/// dialect exists to lose.
#[test]
fn an_unnamed_request_without_a_same_format_source_charges_no_stated_row() {
    for ir in [
        catalog_write_ir(None),
        catalog_write_ir(Some(("other", Some("other:only")))),
    ] {
        let resolved =
            resolve_write_request(&ir, TargetRequest::Unnamed, CATALOG_WRITE_CATALOG).unwrap();
        assert_eq!(
            charged(
                &resolved,
                true,
                Some(stated_row(cadmpeg_core::dialect_id!("test:old")))
            ),
            ("test:old".into(), vec![])
        );
    }
}

fn stated_row(target: DialectId) -> WrittenRow {
    WrittenRow {
        target,
        reason: "the stored envelope decides the row".into(),
    }
}
