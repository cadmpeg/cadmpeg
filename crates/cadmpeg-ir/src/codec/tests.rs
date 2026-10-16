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
use cadmpeg_core::decode::{DecodeContext, DecodeDepth, DecodeMode, InspectOptions, View};
use cadmpeg_core::CodecError;

fn decoded(ir: CadIr) -> Decoded {
    Decoded {
        ir,
        body: DecodeBody::new(crate::report::decode::DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::default(),
    }
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(decoded(ir), FormatId::new("test"), DecodeDepth::Full).unwrap()
}

fn with_parameter(mut ir: CadIr) -> CadIr {
    ir.model.parameters.push(crate::features::DesignParameter {
        id: crate::features::ParameterId::mint("synthetic:test:parameter#depth").unwrap(),
        owner: None,
        ordinal: 0,
        name: "D1".into(),
        expression: "1mm".into(),
        display: None,
        value: None,
        dependencies: crate::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    ir
}

struct RejectFloorCodec;

fn reject_floor_kind() -> LossKind {
    LossKind::shared(LossTaxonomy::TopologyNotTransferred)
}

impl CodecBackend for RejectFloorCodec {
    const FORMAT: FormatId = FormatId::new("test");

    fn detect_impl(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        Ok(Confidence::No)
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the strict gate tests do not inspect")
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, _root: View<'_>) -> Result<Decoded, CodecError> {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        if ctx.container_only() {
            decoded.body.transfer = crate::report::decode::DecodeTransfer::ContainerOnly {};
        }
        decoded
            .body
            .losses
            .push(LossNote::new(reject_floor_kind(), "synthetic reject floor"));
        Ok(decoded)
    }
}

/// Delivers geometry depth whatever the request.
struct GeometryOnlyCodec;

impl CodecBackend for GeometryOnlyCodec {
    const FORMAT: FormatId = FormatId::new("test");

    fn detect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _prefix: View<'_>,
    ) -> Result<Confidence, CodecError> {
        Ok(Confidence::No)
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the depth tests do not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        decoded.body.transfer =
            crate::report::decode::DecodeTransfer::at(DecodeDepth::Geometry, true);
        Ok(decoded)
    }
}

#[test]
fn the_sealed_decode_flags_and_strictly_refuses_a_shallower_delivery() {
    let options = DecodeOptions {
        depth: DecodeDepth::Full,
        ..DecodeOptions::default()
    };
    let result = GeometryOnlyCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &options)
        .unwrap();
    assert_eq!(result.ir().decode_depth(), DecodeDepth::Geometry);
    assert_eq!(depth_losses(&result).len(), 1);

    let error = GeometryOnlyCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &strict_options(DecodeDepth::Full),
        )
        .unwrap_err();
    let DecodeFailure::StrictRejected { rejection } = error else {
        panic!("expected a strict refusal, got {error:?}")
    };
    assert_eq!(
        rejection.loss().code.to_string(),
        "decode/depth.not-delivered"
    );

    let admitted = GeometryOnlyCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &strict_options(DecodeDepth::Geometry),
        )
        .unwrap();
    assert!(depth_losses(&admitted).is_empty());
}

struct ForeignIdentityCodec;

struct CyclicModelCodec;

impl CodecBackend for CyclicModelCodec {
    const FORMAT: FormatId = FormatId::new("test");

    fn detect_impl(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
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

    fn detect_impl(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
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
    let (ctx, root) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::default())
            .expect("root");
    assert_eq!(
        ForeignIdentityCodec.detect(&ctx, root).expect("detect"),
        Confidence::No
    );
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

fn strict_options(depth: DecodeDepth) -> DecodeOptions {
    let mut options = DecodeOptions {
        depth,
        ..DecodeOptions::default()
    };
    options.policy.mode = DecodeMode::Strict;
    options
}

#[test]
fn the_strict_gate_refuses_a_full_decode_on_a_reject_floor_loss() {
    let error = RejectFloorCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &strict_options(DecodeDepth::Full),
        )
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
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &strict_options(DecodeDepth::Container),
        )
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

    let result = decode_result(ir);

    assert_eq!(result.report().format(), "test");
    assert!(result.report().dialects().is_none());
    assert!(result.ir().source.is_none());
}

#[test]
fn a_decode_result_keeps_the_body_it_was_given() {
    let mut body = DecodeBody::new(crate::report::decode::DecodeTransfer::ContainerOnly {});
    body.notes.push("kept".into());
    body.coverage
        .record(crate::report::decode::CoverageKey::new("entities"), 3);
    let result = DecodeResult::new(
        Decoded {
            ir: unit_cube().expect("valid unit cube fixture"),
            body,
            source_fidelity: SourceFidelity::default(),
        },
        FormatId::new("test"),
        DecodeDepth::Container,
    )
    .unwrap();

    assert!(result.report().container_only());
    assert!(result.report().losses.is_empty());
    assert_eq!(result.report().notes, ["kept"]);
    assert_eq!(wire::coverage(result.report())["entities"], 3);
}

fn dialect_layer(id: &'static str) -> DialectMatch {
    DialectMatch::admitted(DialectId::parse(id).expect("the test dialect id is valid"))
}

fn depth_losses(result: &DecodeResult) -> Vec<&LossNote> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/depth.not-delivered")
        .collect()
}

#[test]
fn the_document_records_the_depth_the_backend_reported() {
    use crate::report::decode::DecodeTransfer;

    for transfer in [
        DecodeTransfer::ContainerOnly {},
        DecodeTransfer::at(DecodeDepth::Geometry, false),
        DecodeTransfer::at(DecodeDepth::Geometry, true),
        DecodeTransfer::full(false),
        DecodeTransfer::full(true),
    ] {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        decoded.body.transfer = transfer;
        let result =
            DecodeResult::new(decoded, FormatId::new("test"), DecodeDepth::Container).unwrap();
        assert_eq!(result.ir().decode_depth(), transfer.depth());
        assert_eq!(result.report().depth(), transfer.depth());
        assert_eq!(
            result.report().geometry_transferred(),
            transfer.geometry_transferred()
        );
        assert!(depth_losses(&result).is_empty());
    }
}

#[test]
fn the_wrapper_flags_a_depth_shallower_than_requested() {
    for (delivered, requested) in [
        (DecodeDepth::Container, DecodeDepth::Geometry),
        (DecodeDepth::Container, DecodeDepth::Full),
        (DecodeDepth::Geometry, DecodeDepth::Full),
    ] {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        decoded.body.transfer = crate::report::decode::DecodeTransfer::at(delivered, true);
        let result = DecodeResult::new(decoded, FormatId::new("test"), requested).unwrap();

        assert_eq!(result.ir().decode_depth(), delivered);
        let [loss] = depth_losses(&result)[..] else {
            panic!("expected one depth loss: {:?}", result.report().losses);
        };
        assert_eq!(loss.severity, crate::report::Severity::Error);
        assert_eq!(
            loss.strict_consequence(),
            crate::report::loss::StrictConsequence::Reject
        );
        assert!(loss
            .message
            .contains(&format!("requested {requested} depth")));
        assert!(loss
            .message
            .contains(&format!("delivered {delivered} depth")));
    }
}

#[test]
fn the_wrapper_refuses_design_records_below_full_depth() {
    let ir = with_parameter(unit_cube().expect("valid unit cube fixture"));
    for depth in [DecodeDepth::Container, DecodeDepth::Geometry] {
        let mut decoded = decoded(ir.clone());
        decoded.body.transfer = crate::report::decode::DecodeTransfer::at(depth, true);
        let error = DecodeResult::new(decoded, FormatId::new("test"), depth).unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)), "{error}");
        assert!(
            error.to_string().contains("design arena `parameters`"),
            "{error}"
        );
    }

    let result = decode_result(ir);
    assert_eq!(result.ir().decode_depth(), DecodeDepth::Full);
    assert_eq!(result.ir().model.parameters.len(), 1);
}
