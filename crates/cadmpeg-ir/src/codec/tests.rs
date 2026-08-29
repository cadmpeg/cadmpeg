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
    CadirEncoder
        .plan(
            crate::codec::EncodeInput::new(&ir, None),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
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

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(
        ir,
        DecodeReport {
            dialects: None,
            format: "test".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            losses: Vec::new(),
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        },
        SourceFidelity::default(),
    )
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
        Ok(DecodeResult::new(ir, report, fidelity))
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
    let mut report = DecodeReport {
        dialects: Some(
            DialectLayers::new(
                dialect_layer("test", "test:only"),
                vec![dialect_layer("acis", "acis:save-format-217")],
            )
            .unwrap(),
        ),
        format: "test".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
    };
    let result = DecodeResult::new(ir.clone(), report.clone(), SourceFidelity::default());

    assert_eq!(result.report().dialects.as_ref().unwrap().iter().count(), 2);

    report.dialects = None;
    let unclassified = DecodeResult::new(ir, report, SourceFidelity::default());
    assert!(unclassified.report().dialects.is_none());
}

#[test]
fn a_decode_result_projects_source_mirrors_from_the_primary_layer() {
    let mut ir = unit_cube();
    ir.source = Some(crate::SourceMeta {
        format: "test".into(),
        dialect: Some(DialectMatch {
            format: "test".into(),
            dialect: Some(DialectId::pinned("test:wrong")),
            declared: BTreeMap::from([("wrong".into(), "value".into())]),
            instance: None,
            admission: Admission::Admitted,
        }),
        ..Default::default()
    });
    let mut primary = dialect_layer("test", "test:only");
    primary.declared = BTreeMap::from([("version".into(), "only".into())]);
    let report = DecodeReport {
        dialects: Some(DialectLayers::new(primary.clone(), Vec::new()).unwrap()),
        format: "test".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
    };

    let result = DecodeResult::new(ir, report, SourceFidelity::default());
    let source = result
        .ir()
        .source
        .as_ref()
        .expect("source metadata remains");
    assert_eq!(source.dialect, Some(primary));
}

fn dialect_layer(format: &str, id: &'static str) -> DialectMatch {
    DialectMatch {
        format: format.into(),
        dialect: Some(DialectId::pinned(id)),
        declared: BTreeMap::new(),
        instance: None,
        admission: Admission::Admitted,
    }
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
    assert_eq!(requested.as_deref(), Some("cadir:nonesuch"));
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
    ir.source = source.map(|(format, dialect)| crate::document::SourceMeta {
        format: format.to_owned(),
        dialect: dialect.map(|id| {
            DialectMatch::layer(
                format,
                DialectId::pinned(id),
                BTreeMap::new(),
                Admission::Admitted,
            )
        }),
        ..Default::default()
    });
    ir
}

fn resolve_test_catalog<'a>(
    ir: &'a CadIr,
    request: TargetRequest<'a>,
) -> Result<(&'static str, Option<DialectId>), CodecError> {
    resolve_catalog_write(
        ir,
        request,
        "test",
        CATALOG_WRITE_TARGETS,
        "the test writer cannot synthesize the source row",
    )
    .map(|(entry, displaced)| (entry.id, displaced))
}

#[test]
fn catalog_write_resolves_an_explicit_on_catalog_target() {
    let ir = catalog_write_ir(None);
    assert_eq!(
        resolve_test_catalog(&ir, TargetRequest::Explicit("old")).unwrap(),
        ("test:old", None)
    );
}

#[test]
fn catalog_write_refuses_an_explicit_off_catalog_target_with_the_catalog() {
    let ir = catalog_write_ir(None);
    let error = resolve_test_catalog(&ir, TargetRequest::Explicit("test:missing")).unwrap_err();
    let CodecError::UnsupportedTarget { available, .. } = error else {
        panic!("expected an unsupported target");
    };
    assert_eq!(available, "test:old, test:new");
}

#[test]
fn catalog_write_inherit_without_a_source_uses_the_default() {
    let ir = catalog_write_ir(None);
    assert_eq!(
        resolve_test_catalog(&ir, TargetRequest::Inherit).unwrap(),
        ("test:new", None)
    );
}

#[test]
fn catalog_write_inherit_uses_a_same_format_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    assert_eq!(
        resolve_test_catalog(&ir, TargetRequest::Inherit).unwrap(),
        ("test:old", None)
    );
}

#[test]
fn catalog_write_inherit_refuses_a_same_format_off_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let error = resolve_test_catalog(&ir, TargetRequest::Inherit).unwrap_err();
    let CodecError::UnsupportedTarget {
        requested,
        reason,
        available,
        ..
    } = error
    else {
        panic!("expected an unsupported target");
    };
    assert_eq!(requested.as_deref(), Some("test:future"));
    assert_eq!(reason, "the test writer cannot synthesize the source row");
    assert_eq!(available, "test:old, test:new");
}

#[test]
fn catalog_write_explicit_difference_returns_the_displaced_dialect() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let (target, displaced) =
        resolve_test_catalog(&ir, TargetRequest::Explicit("test:new")).unwrap();
    assert_eq!(target, "test:new");
    assert_eq!(displaced.as_ref(), Some(&DialectId::pinned("test:old")));
    assert_eq!(
        source_dialect_displaced_message(
            displaced.as_ref().expect("the source dialect differs"),
            &DialectId::pinned(target),
        ),
        "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved"
    );
}
