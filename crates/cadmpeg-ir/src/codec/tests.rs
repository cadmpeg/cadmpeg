// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};
use cadmpeg_core::target::{DefaultSource, TargetDescriptor, TargetRefusalKind, TargetToken};

use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::{DecodeTransfer, FidelityResolution, LossKind, LossNote, LossTaxonomy};
use crate::source_fidelity::{RetainedSourceRecord, SourceFidelity};
use crate::validate::validate_neutral;
use crate::CadIr;

use super::write::resolve_write_request;
use super::write::{
    CadirEncoder, Catalog, Consumption, DialectFree, EncodeInput, Encoder, EncoderBackend,
    ExportBody, ResolvedWrite, TargetRequest,
};
use super::*;

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube();
    let mut encoded = Vec::new();
    let plan = CadirEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
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
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("plan CADIR export");

    assert_eq!(plan.report().census.counts, validation_counts);
}

struct NeutralEncoder;

impl EncoderBackend for NeutralEncoder {
    const FORMAT: &'static str = "selected";
    type Target = DialectFree;
    const TARGET: DialectFree = DialectFree;

    fn plan_resolved(&self, input: EncodeInput<'_>, (): ()) -> Result<ExportBody, CodecError> {
        // The body carries no identity: whatever the backend does, the
        // wrapper stamps FORMAT.
        Ok(ExportBody::synthesized(Vec::new(), input.ir))
    }
}

#[test]
fn the_wrapper_stamps_the_backend_format_on_a_dialect_free_plan() {
    let ir = CadIr::empty(crate::units::Units::default());
    let plan = NeutralEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert_eq!(plan.report().format(), "selected");
    assert_eq!(plan.report().target(), None);
    assert_eq!(plan.report().fidelity, FidelityResolution::NotProvided);
}

#[test]
fn a_dialect_free_encoder_refuses_an_explicit_target() {
    let ir = CadIr::empty(crate::units::Units::default());
    let error = NeutralEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("any"))
        .unwrap_err();
    let CodecError::UnsupportedTarget(refusal) = error else {
        panic!("a dialect-free encoder has no explicit targets")
    };
    assert!(matches!(
        refusal.kind(),
        TargetRefusalKind::UnknownExplicit { .. }
    ));
    assert!(refusal.available().is_empty());
}

struct CatalogEncoder;

impl EncoderBackend for CatalogEncoder {
    const FORMAT: &'static str = "test";
    type Target = Catalog;
    const TARGET: Catalog = Catalog(CATALOG_WRITE_TARGETS);

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        let mut body = ExportBody::synthesized(Vec::new(), input.ir);
        body.notes
            .push(format!("resolved {}", target.target_id().as_str()));
        body.consumption = Consumption::Degraded {
            reason: "test backend never replays".to_owned(),
        };
        Ok(body)
    }
}

#[test]
fn the_wrapper_stamps_the_resolved_target_on_a_catalog_plan() {
    let ir = CadIr::empty(crate::units::Units::default());
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
        .unwrap();
    assert_eq!(plan.report().format(), "test");
    assert_eq!(plan.report().target(), Some(&DialectId::pinned("test:new")));
    assert_eq!(plan.report().notes, vec!["resolved test:new".to_owned()]);
}

#[test]
fn fidelity_resolution_is_not_provided_whenever_the_input_carries_none() {
    let ir = CadIr::empty(crate::units::Units::default());
    let plan = CatalogEncoder
        .plan(EncodeInput::new(&ir, None), TargetRequest::Explicit("new"))
        .unwrap();
    assert_eq!(plan.report().fidelity, FidelityResolution::NotProvided);
}

#[test]
fn fidelity_resolution_follows_the_backend_consumption_when_provided() {
    let ir = CadIr::empty(crate::units::Units::default());
    let fidelity = SourceFidelity::default();
    let plan = CatalogEncoder
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Explicit("new"),
        )
        .unwrap();
    assert_eq!(
        plan.report().fidelity,
        FidelityResolution::Degraded {
            reason: "test backend never replays".to_owned()
        }
    );
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

fn decoded(ir: CadIr) -> Decoded {
    Decoded {
        ir,
        body: DecodeBody::new(DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::default(),
    }
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(decoded(ir), "test")
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
    const FORMAT: &'static str = "test";

    fn detect_impl(&self, _prefix: &[u8]) -> Confidence {
        Confidence::No
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the strict gate tests do not inspect")
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, _root: View<'_>) -> Result<Decoded, CodecError> {
        let mut decoded = decoded(unit_cube());
        // Deliberately report the opposite request scope; the wrapper owns it.
        decoded.body.transfer = if ctx.container_only() {
            DecodeTransfer::full(true)
        } else {
            DecodeTransfer::ContainerOnly
        };
        decoded
            .body
            .losses
            .push(LossNote::new(reject_floor_kind(), "synthetic reject floor"));
        Ok(decoded)
    }
}

struct ForeignIdentityCodec;

impl CodecBackend for ForeignIdentityCodec {
    const FORMAT: &'static str = "selected";

    fn detect_impl(&self, _prefix: &[u8]) -> Confidence {
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
            Vec::new(),
        ))
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube();
        ir.source = Some(crate::SourceMeta::unclassified("foreign", BTreeMap::new()));
        Ok(decoded(ir))
    }
}

#[test]
fn the_sealed_wrapper_reports_the_backend_format() {
    assert_eq!(Codec::id(&ForeignIdentityCodec), "selected");
    assert_eq!(ForeignIdentityCodec.detect(&[]), Confidence::No);
}

#[test]
fn sealed_inspect_rejects_a_backend_that_reports_another_format() {
    let error = ForeignIdentityCodec
        .inspect(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &InspectOptions::default(),
        )
        .expect_err("the sealed wrapper owns inspect format identity");

    let CodecError::WrongFormat(message) = error else {
        panic!("expected a wrong-format refusal, got {error:?}")
    };
    assert_eq!(
        message,
        "codec \"selected\" inspected a \"foreign\" container"
    );
}

#[test]
fn sealed_decode_rejects_a_document_authored_for_another_format() {
    let error = ForeignIdentityCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &DecodeOptions::default(),
        )
        .expect_err("the sealed wrapper owns decode format identity");

    let DecodeFailure::Codec(CodecError::WrongFormat(message)) = error else {
        panic!("expected a wrong-format refusal, got {error:?}")
    };
    assert_eq!(message, "codec \"selected\" decoded a \"foreign\" document");
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
fn a_decode_result_stamps_every_source_dialect_layer_onto_the_report() {
    let mut ir = unit_cube();
    let primary = dialect_layer("test:only")
        .with_declared(BTreeMap::from([("version".into(), "only".into())]));
    let layers = DialectLayers::of(primary.clone())
        .with(dialect_layer("acis:save-format-217").with_instance("body.sab"));
    ir.source = Some(crate::SourceMeta::classified(
        layers.clone(),
        BTreeMap::from([("attribute".into(), "retained".into())]),
    ));

    let result = decode_result(ir);

    assert_eq!(result.report().format(), "test");
    assert_eq!(result.report().dialects(), Some(&layers));
    let source = result
        .ir()
        .source
        .as_ref()
        .expect("source metadata remains");
    assert_eq!(source.dialect(), Some(&primary));
    assert_eq!(source.dialects(), result.report().dialects());
    assert_eq!(source.attributes["attribute"], "retained");
}

#[test]
fn a_decode_result_with_unclassified_source_yields_an_unclassified_report() {
    let mut ir = unit_cube();
    ir.source = Some(crate::SourceMeta::unclassified("test", BTreeMap::new()));

    let result = decode_result(ir);

    assert_eq!(result.report().format(), "test");
    assert!(result.report().dialects().is_none());
}

#[test]
fn a_decode_result_without_source_metadata_reports_the_codec_format() {
    let mut ir = unit_cube();
    ir.source = None;

    let result = DecodeResult::new(decoded(ir), "test");

    assert_eq!(result.report().format(), "test");
    assert!(result.report().dialects().is_none());
    assert!(result.ir().source.is_none());
}

#[test]
fn a_decode_result_keeps_the_body_it_was_given() {
    let mut body = DecodeBody::new(DecodeTransfer::ContainerOnly);
    body.notes.push("kept".into());
    body.coverage.insert("entities".into(), 3);
    let result = DecodeResult::new(
        Decoded {
            ir: unit_cube(),
            body,
            source_fidelity: SourceFidelity::default(),
        },
        "test",
    );

    assert!(result.report().container_only());
    assert_eq!(result.report().notes, ["kept"]);
    assert_eq!(result.report().coverage["entities"], 3);
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
            DialectLayers::of(DialectMatch::admitted(DialectId::pinned(id))),
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
    assert_eq!(resolved.entry().unwrap().id.as_str(), "test:new");
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
        TargetRefusalKind::UnrecordedSource
    ));
    assert_eq!(refusal.format(), "test");
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
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
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
        resolve_write_request(&ir, TargetRequest::Inherit, "test", CATALOG_WRITE_TARGETS).unwrap();
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
        "test",
        CATALOG_WRITE_TARGETS,
    )
    .unwrap();
    let entry = resolved.entry().expect("expected a catalog target");
    let displaced = resolved
        .displaced_source()
        .expect("the explicit target displaces the recorded source");
    assert_eq!(entry.id.as_str(), "test:new");
    assert_eq!(displaced, &DialectId::pinned("test:old"));
    assert!(!resolved.source_preservation_eligible());
    assert_eq!(
        resolved.displacement_message().as_deref(),
        Some(
        "source dialect test:old was displaced by target dialect test:new; the source dialect identity is not preserved"
        )
    );
}
