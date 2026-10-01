// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

use crate::examples::unit_cube;
use crate::report::loss::{LossKind, LossNote, LossTaxonomy};
use crate::source_fidelity::SourceFidelity;
use crate::CadIr;

use super::{
    Codec, CodecBackend, Confidence, DecodeBody, DecodeFailure, DecodeOptions, DecodeResult,
    Decoded, FormatId,
};
use crate::ContainerSummary;
use cadmpeg_core::decode::{DecodeContext, DecodeMode, InspectOptions, View};
use cadmpeg_core::CodecError;

fn decoded(ir: CadIr) -> Decoded {
    Decoded {
        ir,
        body: DecodeBody::new(crate::report::decode::DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::default(),
    }
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(decoded(ir), FormatId::new("test"), false)
}

struct RejectFloorCodec;

fn reject_floor_kind() -> LossKind {
    LossKind::shared(LossTaxonomy::TopologyNotTransferred)
}

impl CodecBackend for RejectFloorCodec {
    const FORMAT: FormatId = FormatId::new("test");

    fn detect_impl(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, _prefix: cadmpeg_core::decode::View<'_>) -> Result<Confidence, cadmpeg_core::CodecError> {
        let _ctx = ctx;
        Ok(Confidence::No)
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
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        decoded
            .body
            .losses
            .push(LossNote::new(reject_floor_kind(), "synthetic reject floor"));
        Ok(decoded)
    }
}

struct ForeignIdentityCodec;

struct CyclicModelCodec;

impl CodecBackend for CyclicModelCodec {
    const FORMAT: FormatId = FormatId::new("test");

    fn detect_impl(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, _prefix: cadmpeg_core::decode::View<'_>) -> Result<Confidence, cadmpeg_core::CodecError> {
        let _ctx = ctx;
        Ok(Confidence::No)
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the cycle admission test does not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        Ok(decoded(
            crate::test_support::evaluation_cycles::cyclic_model().0,
        ))
    }
}

impl CodecBackend for ForeignIdentityCodec {
    const FORMAT: FormatId = FormatId::new("selected");

    fn detect_impl(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, _prefix: cadmpeg_core::decode::View<'_>) -> Result<Confidence, cadmpeg_core::CodecError> {
        let _ctx = ctx;
        Ok(Confidence::No)
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        Ok(serde_json::from_value(serde_json::json!({
            "identity": {"classification": "unclassified", "format": "foreign"},
            "container_kind": "flat",
            "entries": [],
            "notes": []
        }))
        .expect("foreign inspection fixture"))
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        ir.source = Some(crate::SourceMeta::classified(
            DialectLayers::of(DialectMatch::admitted(cadmpeg_core::dialect_id!(
                "foreign:test"
            ))),
            BTreeMap::new(),
        ));
        Ok(decoded(ir))
    }
}

#[test]
fn the_sealed_wrapper_reports_the_backend_format() {
    assert_eq!(Codec::id(&ForeignIdentityCodec), FormatId::new("selected"));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::default()).expect("root");
    assert_eq!(ForeignIdentityCodec.detect(&ctx, root).expect("detect"), Confidence::No);
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

#[test]
fn sealed_decode_refuses_a_cyclic_model_as_malformed() {
    let (_, curve, surface) = crate::test_support::evaluation_cycles::cyclic_model();
    let error = CyclicModelCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &DecodeOptions::default(),
        )
        .expect_err("a cyclic model cannot leave the codec");
    let DecodeFailure::Codec(CodecError::Malformed(message)) = error else {
        panic!("expected a malformed cycle refusal, got {error:?}")
    };
    assert_eq!(
        message,
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}")
    );
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
        DecodeFailure::StrictRejected { rejection } => {
            assert_eq!(rejection.loss().code, reject_floor_kind());
            assert_eq!(rejection.report().losses.len(), 1);
            assert_eq!(rejection.report().losses[0].code, reject_floor_kind());
            assert_eq!(
                rejection.report().losses[0].message,
                "synthetic reject floor"
            );
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let primary = dialect_layer("test:only").with_declared(BTreeMap::from([(
        cadmpeg_core::nonblank_literal!("version"),
        "only".into(),
    )]));
    let layers = DialectLayers::of(primary.clone())
        .with(dialect_layer("acis:save-format-217").with_instance("body.sab"))
        .expect("the test dialect layers have distinct keys");
    ir.source = Some(crate::SourceMeta::classified(
        layers.clone(),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("attribute"),
            "retained".into(),
        )]),
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
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.source = Some(
        serde_json::from_value(serde_json::json!({
            "identity": {"classification": "unclassified", "format": "test"},
            "attributes": {},
        }))
        .unwrap(),
    );

    let result = decode_result(ir);

    assert_eq!(result.report().format(), "test");
    assert!(result.report().dialects().is_none());
}

#[test]
fn a_decode_result_without_source_metadata_reports_the_codec_format() {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    ir.source = None;

    let result = DecodeResult::new(decoded(ir), FormatId::new("test"), false);

    assert_eq!(result.report().format(), "test");
    assert!(result.report().dialects().is_none());
    assert!(result.ir().source.is_none());
}

#[test]
fn a_decode_result_keeps_the_body_it_was_given() {
    let mut body = DecodeBody::new(crate::report::decode::DecodeTransfer::full(false));
    body.notes.push("kept".into());
    crate::test_support::with_service_decode_context(|ctx| {
        body.coverage
            .record(ctx, crate::report::decode::CoverageKey::new("entities"), 3)
    })
    .expect("coverage entry");
    let result = DecodeResult::new(
        Decoded {
            ir: unit_cube().expect("valid unit cube fixture"),
            body,
            source_fidelity: SourceFidelity::default(),
        },
        FormatId::new("test"),
        true,
    );

    assert!(result.report().container_only());
    assert_eq!(result.report().notes, ["kept"]);
    assert_eq!(wire::coverage(result.report())["entities"], 3);
}

fn dialect_layer(id: &'static str) -> DialectMatch {
    DialectMatch::admitted(DialectId::parse(id).expect("the test dialect id is valid"))
}

#[test]
fn wrapper_stamps_request_scope_for_each_backend_transfer() {
    for transfer in [
        crate::report::decode::DecodeTransfer::ContainerOnly {},
        crate::report::decode::DecodeTransfer::full(false),
        crate::report::decode::DecodeTransfer::full(true),
    ] {
        for container_only in [false, true] {
            let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
            decoded.body.transfer = transfer;
            let result = DecodeResult::new(decoded, FormatId::new("test"), container_only);
            assert_eq!(result.report().container_only(), container_only);
            assert_eq!(
                result.report().geometry_transferred(),
                !container_only && transfer.geometry_transferred()
            );
        }
    }
}

struct SharedBudgetCodec;

impl CodecBackend for SharedBudgetCodec {
    const FORMAT: FormatId = FormatId::new("test");
    fn detect_impl(&self, ctx: &DecodeContext<'_>, _: View<'_>) -> Result<Confidence, CodecError> {
        ctx.charge_work(1, "test detection")?;
        Ok(Confidence::High)
    }
    fn inspect_impl(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<ContainerSummary, CodecError> {
        panic!("shared decode test does not inspect")
    }
    fn decode_impl(&self, ctx: &DecodeContext<'_>, _: View<'_>) -> Result<Decoded, CodecError> {
        ctx.charge_work(1, "test decode")?;
        Ok(decoded(CadIr::empty()))
    }
}

#[test]
fn detection_and_decode_draw_from_one_work_budget() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut options = DecodeOptions::default();
    options.policy.limits.max_work_units = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(&[], &arena, &options.policy).expect("root");
    assert_eq!(SharedBudgetCodec.detect(&ctx, root).expect("detect"), Confidence::High);
    let error = SharedBudgetCodec.decode_with_context(&ctx, root, &options).expect_err("decode must keep detection charge");
    let DecodeFailure::Codec(CodecError::ResourceLimit(limit)) = error else { panic!("typed work refusal"); };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 1);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(fused)) if fused == limit));
}
