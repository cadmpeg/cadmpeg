// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch};

use crate::codec::{CadirEncoder, Encoder};
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::{LossKind, LossNote, LossTaxonomy, TransferLedger};
use crate::source_fidelity::RetainedSourceRecord;
use crate::validate::validate_neutral;
use crate::CadIr;

use super::*;

const NO_ALIASES: &[&str] = &[];

fn target(id: &'static str, aliases: &'static [&'static str], default: bool) -> TargetDescriptor {
    TargetDescriptor {
        id,
        label: id,
        aliases,
        default,
    }
}

#[test]
#[should_panic(expected = "at most one entry may be the default")]
fn a_target_catalog_rejects_multiple_defaults() {
    let targets = [
        target("test:first", NO_ALIASES, true),
        target("test:second", NO_ALIASES, true),
    ];
    assert_valid_target_catalog(&targets);
}

#[test]
#[should_panic(expected = "duplicate id")]
fn a_target_catalog_rejects_duplicate_ids() {
    let targets = [
        target("test:same", NO_ALIASES, false),
        target("test:same", NO_ALIASES, false),
    ];
    assert_valid_target_catalog(&targets);
}

#[test]
#[should_panic(expected = "duplicate alias")]
fn a_target_catalog_rejects_duplicate_aliases() {
    let targets = [
        target("test:first", &["same"], false),
        target("test:second", &["same"], false),
    ];
    assert_valid_target_catalog(&targets);
}

#[test]
#[should_panic(expected = "is also an id")]
fn a_target_catalog_rejects_an_alias_that_is_an_id() {
    let targets = [
        target("test:first", &["test:second"], false),
        target("test:second", NO_ALIASES, false),
    ];
    assert_valid_target_catalog(&targets);
}

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube();
    let mut encoded = Vec::new();
    let plan = CadirEncoder
        .plan(
            crate::codec::EncodeInput::new(&ir, None),
            TargetRequest::Inherit,
        )
        .expect("empty-catalog inheritance resolves to CADIR identity");
    assert_eq!(plan.report().target(), None);
    plan.write_to(&mut encoded).unwrap();
    let mut canonical = ir.to_canonical_json().unwrap();
    canonical.push('\n');
    assert_eq!(encoded, canonical.as_bytes());
}

#[test]
fn cadir_encoder_census_matches_validation_counts() {
    let ir = directed_subd_sum();
    let validation_counts = validate_neutral(&ir, Vec::new()).entity_counts;
    let plan = CadirEncoder
        .plan(
            crate::codec::EncodeInput::new(&ir, None),
            TargetRequest::Inherit,
        )
        .expect("plan CADIR export");

    assert_eq!(plan.report().census.counts, validation_counts);
}

#[test]
fn an_empty_native_catalog_has_no_format_identity_request() {
    let ir = CadIr::empty(crate::units::Units::default());
    let error = resolve_write_request(&ir, TargetRequest::Inherit, "cadir", &[]).unwrap_err();

    let CodecError::UnsupportedTarget {
        requested,
        available,
        ..
    } = error
    else {
        panic!("an empty native catalog must refuse without inventing an identity request")
    };
    assert_eq!(requested, None);
    assert_eq!(available, "none");
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(
        ir,
        DecodeReport::unclassified(
            "test",
            false,
            true,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            TransferLedger::default(),
        ),
        SourceFidelity::default(),
    )
    .expect("the test source and report formats agree")
}

fn retained_record(id: &str, offset: u64) -> RetainedSourceRecord {
    RetainedSourceRecord {
        id: id.into(),
        stream: "test".into(),
        offset,
        byte_len: 0,
        sha256: String::new(),
        data: None,
    }
}

#[test]
fn decode_result_edit_guards_restore_finalization() {
    let mut result = decode_result(unit_cube());
    {
        let mut ir = result.ir_mut();
        ir.model.points.reverse();
    }
    assert!(result
        .ir()
        .model
        .points
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));

    {
        let mut fidelity = result.source_fidelity_mut();
        fidelity
            .retained_records
            .extend([retained_record("b", 2), retained_record("a", 1)]);
    }
    assert_eq!(
        result
            .source_fidelity()
            .retained_records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}

#[test]
fn decode_result_edit_guard_finalizes_during_unwind() {
    let mut result = decode_result(unit_cube());
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut ir = result.ir_mut();
        ir.model.points.reverse();
        panic!("abort edit");
    }));
    assert!(unwind.is_err());
    assert!(result
        .ir()
        .model
        .points
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));
}

struct RejectFloorCodec;

fn reject_floor_kind() -> LossKind {
    LossKind::shared(LossTaxonomy::TopologyNotTransferred)
}

impl CodecBackend for RejectFloorCodec {
    fn id(&self) -> &'static str {
        "reject-floor"
    }

    fn detect(&self, _prefix: &[u8]) -> Confidence {
        Confidence::No
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the strict gate tests do not inspect")
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let (ir, mut report, fidelity) = decode_result(unit_cube()).into_parts();
        // Deliberately lie in both directions; the wrapper owns this field.
        report.container_only = !ctx.container_only();
        report
            .losses
            .push(LossNote::new(reject_floor_kind(), "synthetic reject floor"));
        Ok(DecodeResult::new(ir, report, fidelity)?)
    }
}

fn strict_options(container_only: bool) -> DecodeOptions {
    let mut options = DecodeOptions {
        container_only,
        ..DecodeOptions::default()
    };
    options.policy.mode = DecodeMode::Strict;
    options
}

#[test]
fn the_strict_gate_refuses_a_full_decode_on_a_reject_floor_loss() {
    let error = RejectFloorCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &strict_options(false))
        .unwrap_err();

    match error {
        CodecError::StrictRefusal { loss_code, .. } => {
            assert_eq!(loss_code, reject_floor_kind().to_string());
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn a_container_only_strict_decode_keeps_its_losses_and_is_admitted() {
    let result = RejectFloorCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &strict_options(true))
        .unwrap();

    assert!(result.report().container_only);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == reject_floor_kind())
            .count(),
        1
    );
}

#[test]
fn a_decode_result_accepts_dialects_with_one_primary_layer() {
    let mut ir = unit_cube();
    ir.source = None;
    let report = DecodeReport::classified(
        DialectLayers::new(
            dialect_layer("test:only"),
            vec![dialect_layer("acis:save-format-217")],
        )
        .unwrap(),
        false,
        true,
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );
    let result = DecodeResult::new(ir.clone(), report.clone(), SourceFidelity::default())
        .expect("the test source and report formats agree");

    assert_eq!(result.report().dialects().unwrap().iter().count(), 2);

    let unclassified = DecodeResult::new(
        ir,
        DecodeReport::unclassified(
            "test",
            report.container_only,
            report.geometry_transferred,
            report.coverage,
            report.losses,
            report.notes,
            report.transfer_ledger,
        ),
        SourceFidelity::default(),
    )
    .expect("the test source and report formats agree");
    assert!(unclassified.report().dialects().is_none());
}

#[test]
fn a_decode_result_projects_source_mirrors_from_the_primary_layer() {
    let mut ir = unit_cube();
    ir.source = Some(crate::SourceMeta::classified(
        DialectMatch::new(DialectId::pinned("test:wrong"), Admission::Admitted)
            .expect("the known test dialect is classified")
            .with_declared(BTreeMap::from([("wrong".into(), "value".into())])),
        BTreeMap::new(),
    ));
    let primary = dialect_layer("test:only")
        .with_declared(BTreeMap::from([("version".into(), "only".into())]));
    let report = DecodeReport::classified(
        DialectLayers::new(primary.clone(), Vec::new()).unwrap(),
        false,
        true,
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let result = DecodeResult::new(ir, report, SourceFidelity::default())
        .expect("the test source and report formats agree");
    let source = result
        .ir()
        .source
        .as_ref()
        .expect("source metadata remains");
    assert_eq!(source.dialect(), Some(&primary));
}

#[test]
fn a_decode_result_rejects_a_source_and_report_format_mismatch_before_stamping() {
    let mut ir = unit_cube();
    let original = DialectMatch::new(DialectId::pinned("step:ap242e3"), Admission::Admitted)
        .expect("the known STEP dialect is classified");
    ir.source = Some(crate::SourceMeta::classified(
        original.clone(),
        BTreeMap::new(),
    ));
    let report = DecodeReport::classified(
        DialectLayers::of(
            DialectMatch::new(DialectId::pinned("rhino:archive-80"), Admission::Admitted)
                .expect("the known Rhino dialect is classified"),
        ),
        false,
        true,
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let error = DecodeResult::new(ir, report, SourceFidelity::default())
        .expect_err("a decode result must not overwrite a foreign source classification");

    assert_eq!(
        error.to_string(),
        "decode source format \"step\" does not match report primary format \"rhino\""
    );
}

fn dialect_layer(id: &'static str) -> DialectMatch {
    DialectMatch::new(DialectId::pinned(id), Admission::Admitted)
        .expect("the known test dialect is classified")
}

/// An explicit target is refused by `plan` itself, with the catalog in the
/// message.
///
/// The neutral encoder has no catalog at all, so every explicit id is outside
/// it: CADIR is the neutral document, and its version is data about cadmpeg,
/// never a dialect. A `plan` that dropped the guard would answer a dialect
/// request by writing a document that has none.
#[test]
fn plan_refuses_an_explicit_target_outside_the_catalog() {
    let ir = crate::CadIr::empty(crate::units::Units::default());
    let error = Encoder::plan(
        &CadirEncoder,
        crate::codec::EncodeInput::new(&ir, None),
        crate::codec::TargetRequest::Explicit("cadir:nonesuch"),
    )
    .err()
    .expect("an id outside the catalog is refused");

    let cadmpeg_core::CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "cadir");
    assert_eq!(
        requested.as_ref().map(cadmpeg_core::TargetToken::as_str),
        Some("cadir:nonesuch")
    );
    assert_eq!(available, "none");
    assert!(Encoder::targets(&CadirEncoder).is_empty());
}

const CATALOG_WRITE_TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: "test:old",
        label: "Old test dialect",
        aliases: &["old"],
        default: false,
    },
    TargetDescriptor {
        id: "test:new",
        label: "New test dialect",
        aliases: &["new"],
        default: true,
    },
];

fn catalog_write_ir(source: Option<(&str, Option<&'static str>)>) -> CadIr {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.source = source.map(|(format, dialect)| match dialect {
        Some(id) => crate::document::SourceMeta::classified(
            DialectMatch::layer(DialectId::pinned(id), BTreeMap::new(), Admission::Admitted)
                .expect("the known test dialect is classified"),
            BTreeMap::new(),
        ),
        None => crate::document::SourceMeta::unclassified(format, BTreeMap::new()),
    });
    ir
}

#[test]
fn write_request_resolves_an_explicit_on_catalog_target() {
    let ir = catalog_write_ir(None);
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("old"),
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap();
    assert!(matches!(
        resolved,
        WriteRequest::Catalog {
            entry,
            source: SourceRelation::None,
        } if entry.id == "test:old"
    ));
}

#[test]
fn write_request_refuses_an_unknown_explicit_target_with_the_catalog() {
    let ir = catalog_write_ir(None);
    let error = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:missing"),
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap_err();
    let CodecError::UnsupportedTarget { available, .. } = error else {
        panic!("expected an unsupported target");
    };
    assert_eq!(available, "test:old, test:new");
}

#[test]
fn write_request_inherit_with_a_cross_format_source_uses_the_default() {
    let ir = catalog_write_ir(Some(("other", Some("other:only"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
    assert!(matches!(
        resolved,
        WriteRequest::Catalog {
            entry,
            source: SourceRelation::None,
        } if entry.id == "test:new"
    ));
}

#[test]
fn write_request_inherit_refuses_a_same_format_unrecorded_source() {
    let ir = catalog_write_ir(Some(("test", None)));
    let error = resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS)
        .unwrap_err();

    let CodecError::UnsupportedTarget { requested, .. } = error else {
        panic!("an unrecorded same-format source must produce a target refusal")
    };
    assert_eq!(requested, None);
}

#[test]
fn write_request_explicit_over_an_unrecorded_source_has_no_recorded_relation() {
    let ir = catalog_write_ir(Some(("test", None)));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:old"),
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap();

    assert!(matches!(
        resolved,
        WriteRequest::Catalog {
            entry,
            source: SourceRelation::None,
        } if entry.id == "test:old"
    ));
}

#[test]
fn write_request_inherit_preserves_a_same_format_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
    assert!(matches!(
        resolved,
        WriteRequest::Catalog {
            entry,
            source: SourceRelation::Preserve,
        } if entry.id == "test:old"
    ));
}

#[test]
fn write_request_inherit_preserves_a_same_format_off_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
    assert!(matches!(
        resolved,
        WriteRequest::OffCatalog { dialect } if dialect.as_str() == "test:future"
    ));
}

#[test]
fn catalog_write_explicit_difference_returns_the_displaced_dialect() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved = resolve_write_request(
        &ir,
        TargetRequest::Explicit("test:new"),
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap();
    let WriteRequest::Catalog {
        entry,
        source: SourceRelation::Displaced(displaced),
    } = resolved
    else {
        panic!("expected a catalog target");
    };
    assert_eq!(entry.id, "test:new");
    assert_eq!(displaced, DialectId::pinned("test:old"));
    assert_eq!(
        source_dialect_displaced_message(
            &displaced,
            &DialectId::pinned(entry.id),
        ),
        "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved"
    );
}
