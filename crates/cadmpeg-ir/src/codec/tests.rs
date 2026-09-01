// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::target::{DefaultSource, TargetDescriptor, TargetRefusalKind, TargetToken};

use crate::codec::{
    CadirEncoder, Encoder, EncoderBackend, EncoderTargetDomain, ResolvedEncoderTarget,
};
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::{DecodeTransfer, LossKind, LossNote, LossTaxonomy, TransferLedger};
use crate::source_fidelity::RetainedSourceRecord;
use crate::validate::validate_neutral;
use crate::CadIr;

use super::*;

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

struct ForeignIdentityEncoder;

impl EncoderBackend for ForeignIdentityEncoder {
    const FORMAT: &'static str = "selected";
    const TARGET_DOMAIN: EncoderTargetDomain = EncoderTargetDomain::DialectFree;

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError> {
        let ResolvedEncoderTarget::DialectFree = target else {
            panic!("the sealed wrapper must honor the backend target domain")
        };
        CadirEncoder.plan_resolved(input, ResolvedEncoderTarget::DialectFree)
    }
}

#[test]
fn sealed_plan_rejects_a_backend_that_reports_another_format() {
    let ir = CadIr::empty(crate::units::Units::default());
    let Err(error) =
        ForeignIdentityEncoder.plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
    else {
        panic!("the sealed wrapper must own plan format identity")
    };

    let CodecError::ContractViolation {
        codec,
        operation,
        expected,
        reported,
    } = error
    else {
        panic!("expected an encoder contract violation")
    };
    assert_eq!(codec, "selected");
    assert_eq!(operation, "plan");
    assert_eq!(expected, "selected");
    assert_eq!(reported, "cadir");
}

struct WrongTargetEncoder;

impl EncoderBackend for WrongTargetEncoder {
    const FORMAT: &'static str = "test";
    const TARGET_DOMAIN: EncoderTargetDomain = EncoderTargetDomain::Catalog(CATALOG_WRITE_TARGETS);

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedEncoderTarget,
    ) -> Result<ExportPlan, CodecError> {
        let ResolvedEncoderTarget::Native(target) = target else {
            panic!("the sealed wrapper must resolve a catalog target")
        };
        assert_eq!(target.dialect().as_str(), "test:new");
        Ok(ExportPlan::buffered(
            crate::report::ExportReport::native(
                DialectId::pinned("test:old"),
                crate::report::EntityCensus {
                    basis: crate::report::CensusBasis::IrArenas,
                    counts: input.ir.census(),
                },
                crate::report::FidelityResolution::NotProvided,
                crate::report::WritePath::Synthesized,
                Vec::new(),
                Vec::new(),
            ),
            Vec::new(),
        ))
    }
}

#[test]
fn sealed_plan_rejects_a_backend_that_reports_another_target_in_its_format() {
    let ir = CadIr::empty(crate::units::Units::default());
    let Err(error) =
        WrongTargetEncoder.plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
    else {
        panic!("the sealed wrapper must bind the exact resolved target")
    };

    let CodecError::ContractViolation {
        codec,
        operation,
        expected,
        reported,
    } = error
    else {
        panic!("expected an encoder target contract violation")
    };
    assert_eq!(codec, "test");
    assert_eq!(operation, "plan target");
    assert_eq!(expected, "test:new");
    assert_eq!(reported, "test:old");
}

#[test]
fn an_empty_native_catalog_has_no_format_identity_request() {
    let ir = CadIr::empty(crate::units::Units::default());
    let error = resolve_write_request(&ir, TargetRequest::Inherit, "cadir", &[]).unwrap_err();

    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("an empty native catalog must refuse without inventing an identity request")
    };
    let TargetRefusalKind::NoDefault { source, .. } = refusal.kind() else {
        panic!("a source-free inherit request must report a missing default")
    };
    assert_eq!(source, &DefaultSource::NoSource);
    assert!(refusal.available().is_empty());
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(
        ir,
        DecodeReport::unclassified(
            "test",
            DecodeTransfer::full(true),
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
        "test"
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
        // Deliberately report the opposite request scope; the wrapper owns it.
        report.stamp_request_scope(!ctx.container_only());
        report
            .losses
            .push(LossNote::new(reject_floor_kind(), "synthetic reject floor"));
        Ok(DecodeResult::new(ir, report, fidelity)?)
    }
}

struct ForeignIdentityCodec;

impl CodecBackend for ForeignIdentityCodec {
    fn id(&self) -> &'static str {
        "selected"
    }

    fn detect(&self, _prefix: &[u8]) -> Confidence {
        Confidence::No
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        Ok(ContainerSummary::unclassified(
            "foreign",
            "test",
            Vec::new(),
            Vec::new(),
        ))
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        Ok(decode_result(unit_cube()))
    }
}

#[test]
fn sealed_inspect_rejects_a_backend_that_reports_another_format() {
    let error = ForeignIdentityCodec
        .inspect(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &InspectOptions::default(),
        )
        .expect_err("the sealed wrapper owns inspect format identity");

    let CodecError::ContractViolation {
        codec,
        operation,
        expected,
        reported,
    } = error
    else {
        panic!("expected a codec contract violation")
    };
    assert_eq!(codec, "selected");
    assert_eq!(operation, "inspect");
    assert_eq!(expected, "selected");
    assert_eq!(reported, "foreign");
}

#[test]
fn sealed_decode_rejects_a_consistent_foreign_result() {
    let error = ForeignIdentityCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &DecodeOptions::default(),
        )
        .expect_err("the sealed wrapper owns decode format identity");

    let DecodeFailure::Codec(CodecError::ContractViolation {
        codec,
        operation,
        expected,
        reported,
    }) = error
    else {
        panic!("expected a codec contract violation")
    };
    assert_eq!(codec, "selected");
    assert_eq!(operation, "decode");
    assert_eq!(expected, "selected");
    assert_eq!(reported, "test");
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
        DecodeFailure::StrictRejected {
            loss_code, report, ..
        } => {
            assert_eq!(loss_code, reject_floor_kind().to_string());
            assert_eq!(report.losses.len(), 1);
            assert_eq!(report.losses[0].code, reject_floor_kind());
            assert_eq!(report.losses[0].message, "synthetic reject floor");
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn a_container_only_strict_decode_keeps_its_losses_and_is_admitted() {
    let result = RejectFloorCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &strict_options(true))
        .unwrap();

    assert!(result.report().container_only());
    assert!(!result.report().geometry_transferred());
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
    ir.source = Some(crate::SourceMeta::unclassified("test", BTreeMap::new()));
    let report = DecodeReport::classified(
        DialectLayers::new(
            dialect_layer("test:only"),
            vec![dialect_layer("acis:save-format-217")],
        )
        .unwrap(),
        DecodeTransfer::full(true),
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
            report.transfer(),
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
    ir.source = Some(crate::SourceMeta::unclassified(
        "test",
        BTreeMap::from([("attribute".into(), "retained".into())]),
    ));
    let primary = dialect_layer("test:only")
        .with_declared(BTreeMap::from([("version".into(), "only".into())]));
    let report = DecodeReport::classified(
        DialectLayers::new(primary.clone(), Vec::new()).unwrap(),
        DecodeTransfer::full(true),
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
    assert_eq!(source.attributes["attribute"], "retained");
}

#[test]
fn a_decode_result_rejects_a_classified_report_without_source_metadata() {
    let mut ir = unit_cube();
    ir.source = None;
    let report = DecodeReport::classified(
        DialectLayers::of(dialect_layer("test:only")),
        DecodeTransfer::full(true),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let error = DecodeResult::new(ir, report, SourceFidelity::default())
        .expect_err("classified reports require a source block for write inheritance");
    assert_eq!(
        error.to_string(),
        "classified decode report for \"test\" requires source metadata"
    );
}

#[test]
fn a_decode_result_rejects_same_format_dialect_disagreement() {
    let mut ir = unit_cube();
    ir.source = Some(crate::SourceMeta::classified(
        dialect_layer("test:wrong"),
        BTreeMap::new(),
    ));
    let report = DecodeReport::classified(
        DialectLayers::of(dialect_layer("test:only")),
        DecodeTransfer::full(true),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let error = DecodeResult::new(ir, report, SourceFidelity::default())
        .expect_err("same-format dialect disagreement must not be overwritten");
    assert_eq!(
        error.to_string(),
        "decode source dialect metadata (dialect test:wrong, admission Admitted, instance None, declared {}) disagrees with report primary dialect metadata (dialect test:only, admission Admitted, instance None, declared {})"
    );
}

#[test]
fn a_decode_result_explains_same_id_admission_disagreement() {
    let mut ir = unit_cube();
    ir.source = Some(crate::SourceMeta::classified(
        DialectMatch::admitted(DialectId::pinned("test:only")),
        BTreeMap::new(),
    ));
    let report = DecodeReport::classified(
        DialectLayers::of(DialectMatch::residual(DialectId::pinned("test:only"))),
        DecodeTransfer::full(true),
        BTreeMap::new(),
        Vec::new(),
        Vec::new(),
        TransferLedger::default(),
    );

    let error = DecodeResult::new(ir, report, SourceFidelity::default())
        .expect_err("admission disagreement must not be overwritten");
    let rendered = error.to_string();
    assert!(rendered.contains("dialect test:only, admission Admitted,"));
    assert!(rendered.contains("dialect test:only, admission AdmittedUnverified { using: None },"));
}

#[test]
fn a_decode_result_rejects_a_source_and_report_format_mismatch_before_stamping() {
    let mut ir = unit_cube();
    let original = DialectMatch::admitted(DialectId::pinned("step:ap242e3"));
    ir.source = Some(crate::SourceMeta::classified(
        original.clone(),
        BTreeMap::new(),
    ));
    let report = DecodeReport::classified(
        DialectLayers::of(DialectMatch::admitted(DialectId::pinned(
            "rhino:archive-80",
        ))),
        DecodeTransfer::full(true),
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
    DialectMatch::admitted(DialectId::pinned(id))
}

const CATALOG_WRITE_TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: DialectId::pinned("test:old"),
        aliases: &["old"],
        default: false,
    },
    TargetDescriptor {
        id: DialectId::pinned("test:new"),
        aliases: &["new"],
        default: true,
    },
];

fn catalog_write_ir(source: Option<(&str, Option<&'static str>)>) -> CadIr {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.source = source.map(|(format, dialect)| match dialect {
        Some(id) => crate::document::SourceMeta::classified(
            DialectMatch::admitted(DialectId::pinned(id)),
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
    assert_eq!(resolved.catalog_entry().unwrap().id.as_str(), "test:old");
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
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap_err();
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("expected an unsupported target");
    };
    let TargetRefusalKind::UnknownExplicit { requested, .. } = refusal.kind() else {
        panic!("the explicit token is outside the catalog")
    };
    assert_eq!(requested, &TargetToken::new("test:missing"));
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
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
    assert_eq!(resolved.catalog_entry().unwrap().id.as_str(), "test:new");
    assert!(!resolved.preserves_source());
    assert!(!resolved.has_same_format_source());
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_refuses_a_same_format_unrecorded_source() {
    let ir = catalog_write_ir(Some(("test", None)));
    let error = resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS)
        .unwrap_err();

    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("an unrecorded same-format source must produce a target refusal")
    };
    assert!(matches!(
        refusal.kind(),
        TargetRefusalKind::UnrecordedSource { format } if format == "test"
    ));
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

    assert_eq!(resolved.catalog_entry().unwrap().id.as_str(), "test:old");
    assert!(!resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_preserves_a_same_format_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:old"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
    assert_eq!(resolved.catalog_entry().unwrap().id.as_str(), "test:old");
    assert!(resolved.preserves_source());
    assert!(resolved.has_same_format_source());
    assert!(resolved.source_preservation_eligible());
    assert_eq!(resolved.displaced_source(), None);
}

#[test]
fn write_request_inherit_preserves_a_same_format_off_catalog_source() {
    let ir = catalog_write_ir(Some(("test", Some("test:future"))));
    let resolved =
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
    assert_eq!(resolved.catalog_entry(), None);
    assert_eq!(resolved.dialect().as_str(), "test:future");
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
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap();
    let entry = resolved.catalog_entry().expect("expected a catalog target");
    let displaced = resolved
        .displaced_source()
        .expect("the explicit target displaces the recorded source");
    assert_eq!(entry.id.as_str(), "test:new");
    assert_eq!(displaced, &DialectId::pinned("test:old"));
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(
        source_dialect_displaced_message(
            displaced,
            &entry.id,
        ),
        "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved"
    );
}
