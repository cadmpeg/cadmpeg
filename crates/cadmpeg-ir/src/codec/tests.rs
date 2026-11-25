// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::wire;

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::dialect::{DialectId, DialectLayers, DialectMatch};

use crate::examples::unit_cube;
use crate::report::loss::{LossKind, LossNote, LossTaxonomy};
use crate::report::strictness::{code_name, Strictness};
use crate::report::Severity;
use crate::source_fidelity::SourceFidelity;
use crate::CadIr;

use super::{
    Codec, CodecBackend, Confidence, DecodeBody, DecodeFailure, DecodeOptions, DecodeResult,
    Decoded, FormatId,
};
use crate::ContainerSummary;
use cadmpeg_core::decode::{
    ContentScope, DecodeContext, DecodeDepth, DecodePolicy, InspectOptions, View,
};
use cadmpeg_core::CodecError;

fn decoded(ir: CadIr) -> Decoded {
    Decoded {
        ir,
        body: DecodeBody::new(crate::report::decode::DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(
            b"test source",
        )),
    }
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(
        decoded(ir),
        FormatId::new("test"),
        DecodeDepth::Full,
        &DecodePolicy::default(),
    )
    .unwrap()
}

fn with_parameter(mut ir: CadIr) -> CadIr {
    ir.model
        .parameters
        .push(crate::features::parameters::DesignParameter {
            product_definition: None,
            id: crate::features::ParameterId::mint("synthetic:test:parameter#depth").unwrap(),
            owner: None,
            ordinal: 0,
            name: "D1".into(),
            expression: "1mm".into(),
            display: None,
            value: None,
            dependencies: crate::features::selections::DistinctMembers::default(),
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
        let mut decoded = if ctx.container_only() {
            let mut decoded = decoded(CadIr::empty());
            decoded.body.transfer = crate::report::decode::DecodeTransfer::ContainerOnly {};
            decoded
        } else {
            decoded(unit_cube().expect("valid unit cube fixture"))
        };
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
    assert_eq!(result.ir().scope(), ContentScope::geometry());
    assert_eq!(depth_losses(&result).len(), 2);

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
        panic!("the cyclic decode test does not inspect")
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
            "identity": {"kind": "unclassified", "format": "foreign"},
            "container_kind": "flat",
            "entries": [],
            "losses": [],
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
fn the_decode_wrapper_normalizes_a_spelling_the_decoder_left() {
    let mut ir = unit_cube().unwrap();
    let dimension = crate::pmi::PmiDimension {
        modifiers: [crate::pmi::DimensionModifier::Other(
            cadmpeg_core::nonblank_literal!("Statistical"),
        )]
        .into(),
        ..crate::pmi::PmiDimension::new(crate::pmi::PmiDimensionKind::Size)
    };
    ir.model.pmi.push(crate::pmi::PmiAnnotation::new(
        crate::ids::PmiId::try_from("test:codec:pmi#size".to_owned()).unwrap(),
        Vec::new(),
        crate::pmi::PmiDefinition::Dimension(dimension),
    ));
    let result = decode_result(ir);
    let crate::pmi::PmiDefinition::Dimension(dimension) = &result.ir().model.pmi[0].definition
    else {
        panic!("a dimension");
    };
    assert_eq!(
        dimension.modifiers,
        [crate::pmi::DimensionModifier::Statistical].into()
    );
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
fn sealed_decode_keeps_a_cyclic_model_and_validation_reports_the_cycle() {
    let (_, curve, surface) = crate::test_support::evaluation_cycles::cyclic_model();
    let result = CyclicModelCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &DecodeOptions::default(),
        )
        .expect("a cycle between records does not refuse the decode");
    assert_eq!(result.ir().geometry.curves.len(), 1);
    assert_eq!(result.ir().geometry.surfaces.len(), 2);
    let report = crate::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the decoded model");
    let expected =
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}");
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.entity.as_deref() == Some(curve.as_str())
            && finding.message == expected));
}

struct DesignProjectionFailureCodec;

impl CodecBackend for DesignProjectionFailureCodec {
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
        panic!("the design projection test does not inspect")
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, _root: View<'_>) -> Result<Decoded, CodecError> {
        let mut decoded = decoded(with_parameter(
            unit_cube().expect("valid unit cube fixture"),
        ));
        crate::design::projection::project_layer(
            ctx,
            "test",
            &mut decoded.ir,
            &mut decoded.body.losses,
            |ir, _| {
                ir.design.scopes.push(crate::design::scope::DesignScope {
                    definition: crate::ids::ProductDefinitionId::mint(
                        "test:model:product-definition#root",
                    )
                    .unwrap(),
                    roots: Vec::new(),
                    active_configuration: None,
                    active_arrangements: std::collections::BTreeSet::new(),
                    occurrences: BTreeMap::new(),
                    current: Vec::new(),
                    unproduced: std::collections::BTreeSet::new(),
                });
                Err(CodecError::malformed("design record has no frame"))
            },
        )?;
        Ok(decoded)
    }
}

#[test]
fn sealed_decode_keeps_the_geometry_when_the_design_projection_fails() {
    let result = DesignProjectionFailureCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &DecodeOptions::default(),
        )
        .expect("a design projection error does not refuse the decode");
    let cube = with_parameter(unit_cube().unwrap());
    assert_eq!(result.ir().geometry.bodies, cube.geometry.bodies);
    assert_eq!(result.ir().geometry.faces.len(), cube.geometry.faces.len());
    assert_eq!(result.ir().model.parameters, cube.model.parameters);
    assert!(result.ir().design.scopes.is_empty());
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::unknown::part::record_unreadable_kind())
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0]
        .message
        .starts_with("test v11 design layer is not stated: "));
}

fn strict_options(depth: DecodeDepth) -> DecodeOptions {
    let mut options = DecodeOptions {
        depth,
        ..DecodeOptions::default()
    };
    options.strictness.pinned = true;
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

fn decode_with(depth: DecodeDepth, strictness: Strictness) -> Result<DecodeResult, DecodeFailure> {
    RejectFloorCodec.decode(
        &mut Cursor::new(vec![1u8, 2, 3, 4]),
        &DecodeOptions {
            depth,
            strictness,
            ..DecodeOptions::default()
        },
    )
}

fn refused_code(failure: DecodeFailure) -> LossKind {
    match failure {
        DecodeFailure::StrictRejected { rejection } => rejection.loss().code.clone(),
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn the_default_strictness_admits_a_reject_floor_loss() {
    for depth in [DecodeDepth::Container, DecodeDepth::Full] {
        let result = decode_with(depth, Strictness::default()).unwrap();
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == reject_floor_kind()));
    }
}

#[test]
fn a_severity_floor_refuses_a_full_and_a_container_only_decode() {
    let severity = LossNote::new(reject_floor_kind(), "probe").severity;
    for depth in [DecodeDepth::Container, DecodeDepth::Full] {
        let strictness = Strictness {
            floor: Some(severity),
            ..Strictness::default()
        };
        assert_eq!(
            refused_code(decode_with(depth, strictness).unwrap_err()),
            reject_floor_kind()
        );
    }
}

#[test]
fn a_severity_floor_above_every_loss_admits_the_decode() {
    let severity = LossNote::new(reject_floor_kind(), "probe").severity;
    assert!(severity < Severity::Blocking);
    let strictness = Strictness {
        floor: Some(Severity::Blocking),
        ..Strictness::default()
    };
    decode_with(DecodeDepth::Full, strictness).unwrap();
}

#[test]
fn a_refused_code_refuses_a_full_and_a_container_only_decode() {
    for depth in [DecodeDepth::Container, DecodeDepth::Full] {
        let strictness = Strictness::default().with_code(&code_name(&reject_floor_kind()));
        assert_eq!(
            refused_code(decode_with(depth, strictness).unwrap_err()),
            reject_floor_kind()
        );
    }
}

#[test]
fn a_code_the_report_does_not_hold_admits_the_decode() {
    let strictness = Strictness::default().with_code("export:output.empty");
    decode_with(DecodeDepth::Full, strictness).unwrap();
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
            "identity": {"kind": "unclassified", "format": "test"},
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
            ir: CadIr::empty(),
            body,
            source_fidelity: SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(
                b"test source",
            )),
        },
        FormatId::new("test"),
        DecodeDepth::Container,
        &DecodePolicy::default(),
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
        let mut decoded = decoded(if transfer.container_only() {
            CadIr::empty()
        } else {
            unit_cube().expect("valid unit cube fixture")
        });
        decoded.body.transfer = transfer;
        let result = DecodeResult::new(
            decoded,
            FormatId::new("test"),
            transfer.depth(),
            &DecodePolicy::default(),
        )
        .unwrap();
        assert_eq!(result.ir().scope(), transfer.depth().scope());
        assert_eq!(result.report().depth(), transfer.depth());
        assert_eq!(
            result.report().geometry_transferred(),
            transfer.geometry_transferred()
        );
        assert!(depth_losses(&result).is_empty());
    }
}

#[test]
fn the_wrapper_flags_each_requested_class_left_undelivered() {
    use cadmpeg_core::decode::ArenaClass::{Design, Model, Process};
    for (delivered, requested, missing) in [
        (DecodeDepth::Container, DecodeDepth::Geometry, &[Model][..]),
        (
            DecodeDepth::Container,
            DecodeDepth::Full,
            &[Model, Design, Process],
        ),
        (DecodeDepth::Geometry, DecodeDepth::Design, &[Design]),
        (DecodeDepth::Geometry, DecodeDepth::Full, &[Design, Process]),
        (DecodeDepth::Design, DecodeDepth::Full, &[Process]),
    ] {
        let mut decoded = decoded(if delivered == DecodeDepth::Container {
            CadIr::empty()
        } else {
            unit_cube().expect("valid unit cube fixture")
        });
        decoded.body.transfer = crate::report::decode::DecodeTransfer::at(delivered, true);
        let result = DecodeResult::new(
            decoded,
            FormatId::new("test"),
            requested,
            &DecodePolicy::default(),
        )
        .unwrap();

        assert_eq!(result.ir().scope(), delivered.scope());
        let losses = depth_losses(&result);
        assert_eq!(losses.len(), missing.len(), "{losses:?}");
        for (loss, class) in losses.into_iter().zip(missing) {
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
                .contains(&format!("did not deliver the {class} class")));
        }
    }
}

/// A decoder that states the source holds design records beyond the depth it
/// delivers charges `design` once, and the wrapper adds no second charge.
#[test]
fn a_class_the_source_holds_beyond_the_delivered_depth_is_charged_once() {
    use cadmpeg_core::decode::ArenaClass::{Design, Process};
    for (delivered, requested, missing) in [
        (DecodeDepth::Geometry, DecodeDepth::Geometry, &[Design][..]),
        (DecodeDepth::Geometry, DecodeDepth::Design, &[Design]),
        (DecodeDepth::Geometry, DecodeDepth::Full, &[Design, Process]),
        (DecodeDepth::Design, DecodeDepth::Design, &[]),
        (DecodeDepth::Full, DecodeDepth::Full, &[]),
    ] {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        decoded.body.transfer = crate::report::decode::DecodeTransfer::at(delivered, true);
        decoded.body.source_holds(Design);
        decoded.body.source_holds(Design);
        let result = DecodeResult::new(
            decoded,
            FormatId::new("test"),
            requested,
            &DecodePolicy::default(),
        )
        .unwrap();

        assert_eq!(result.ir().scope(), delivered.scope());
        let losses = depth_losses(&result);
        assert_eq!(losses.len(), missing.len(), "{losses:?}");
        for class in missing {
            assert_eq!(
                losses
                    .iter()
                    .filter(|loss| loss.message.contains(&format!(" {class} ")))
                    .count(),
                1,
                "{class}: {losses:?}"
            );
        }
        let held = losses.iter().find(|loss| {
            loss.message
                .contains("the source holds design records that a geometry-depth decode")
        });
        assert_eq!(held.is_some(), !missing.is_empty(), "{losses:?}");
        if let Some(loss) = held {
            assert_eq!(loss.severity, crate::report::Severity::Error);
        }
    }
}

#[test]
fn the_wrapper_records_only_the_classes_a_request_names_when_the_rest_are_empty() {
    for requested in [DecodeDepth::Geometry, DecodeDepth::Design] {
        let mut decoded = decoded(unit_cube().expect("valid unit cube fixture"));
        decoded.body.transfer = crate::report::decode::DecodeTransfer::full(true);
        let result = DecodeResult::new(
            decoded,
            FormatId::new("test"),
            requested,
            &DecodePolicy::default(),
        )
        .unwrap();
        assert_eq!(result.ir().scope(), requested.scope());
        assert!(depth_losses(&result).is_empty());
    }
}

#[test]
fn the_wrapper_refuses_records_of_a_class_the_scope_excludes() {
    let ir = with_parameter(unit_cube().expect("valid unit cube fixture"));
    for (depth, expected) in [
        (DecodeDepth::Container, "model arena `bodies`"),
        (DecodeDepth::Geometry, "design arena `parameters`"),
    ] {
        let mut decoded = decoded(ir.clone());
        decoded.body.transfer = crate::report::decode::DecodeTransfer::at(depth, true);
        let error = DecodeResult::new(
            decoded,
            FormatId::new("test"),
            depth,
            &DecodePolicy::default(),
        )
        .unwrap_err();
        assert!(matches!(error, CodecError::Malformed(_)), "{error}");
        assert!(error.to_string().contains(expected), "{error}");
    }

    let mut decoded = decoded(ir.clone());
    decoded.body.transfer = crate::report::decode::DecodeTransfer::full(true);
    let result = DecodeResult::new(
        decoded,
        FormatId::new("test"),
        DecodeDepth::Geometry,
        &DecodePolicy::default(),
    )
    .unwrap();
    assert_eq!(result.ir().scope(), ContentScope::design());
    assert_eq!(result.ir().model.parameters.len(), 1);

    let result = decode_result(ir);
    assert_eq!(result.ir().scope(), ContentScope::full());
    assert_eq!(result.ir().model.parameters.len(), 1);
}

/// A decoder that emits the cube's first point three times; with `differ`
/// the later copies hold another `native_ref`.
struct RepeatedIdentityCodec {
    differ: bool,
}

impl CodecBackend for RepeatedIdentityCodec {
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
        panic!("the repeated identity test does not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        for copy in ["second", "third"] {
            let mut point = ir.geometry.points[0].clone();
            if self.differ {
                point.native_ref = Some(copy.into());
            }
            ir.geometry.points.push(point);
        }
        Ok(decoded(ir))
    }
}

#[test]
fn the_sealed_wrapper_states_no_copy_of_a_repeated_id_whose_copies_differ_and_charges_one_loss() {
    let result = RepeatedIdentityCodec { differ: true }
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("a decoder's repeated id does not refuse the file");
    let cube = unit_cube().expect("valid unit cube fixture");
    assert_eq!(result.ir().geometry.points, cube.geometry.points[1..]);
    let repeated = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == super::loss::DecodeLossCode::IdRepeated.note("").code)
        .collect::<Vec<_>>();
    let [loss] = repeated.as_slice() else {
        panic!("one loss for the one repeated id: {repeated:?}");
    };
    assert_eq!(loss.severity, crate::report::Severity::Error);
    assert!(
        loss.message.contains(cube.geometry.points[0].id.as_str())
            && loss.message.contains("3 times"),
        "{}",
        loss.message
    );
    let [stub]: [_; 1] = result
        .ir()
        .native_unknowns("test")
        .expect("the test stubs")
        .try_into()
        .expect("one stub for the one repeated id");
    assert!(stub.links.is_empty());
    assert!(
        loss.message
            .contains(&format!("record={}", stub.id.as_str()))
            && loss.message.contains(cube.geometry.vertices[0].id.as_str()),
        "{}",
        loss.message
    );
    assert!(result
        .source_fidelity()
        .retained_record(stub.id.as_str())
        .is_some());
    assert!(!result
        .ir()
        .geometry
        .vertices
        .iter()
        .any(|vertex| vertex.point == cube.geometry.points[0].id));
}

#[test]
fn the_sealed_wrapper_states_one_record_for_equal_copies_of_an_id_and_charges_nothing() {
    let result = RepeatedIdentityCodec { differ: false }
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("a decoder's repeated id does not refuse the file");
    let cube = unit_cube().expect("valid unit cube fixture");
    assert_eq!(result.ir().geometry.points, cube.geometry.points);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == super::loss::DecodeLossCode::IdRepeated.note("").code));
}

#[test]
fn the_sealed_wrapper_records_the_digest_of_the_document_it_returns() {
    let result = RepeatedIdentityCodec { differ: false }
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("the unit cube decodes");
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    assert_eq!(
        result.source_fidelity().content_digest,
        Some(crate::hash::content_digest(&ctx, result.ir()).unwrap())
    );
    assert!(!result.source_fidelity().edited(result.ir()).unwrap());

    let (mut ir, _, fidelity) = result.into_parts();
    let mut scope = ContentScope::full();
    scope.process = false;
    ir.set_scope(scope).unwrap();
    assert!(
        fidelity.edited(&ir).unwrap(),
        "a change of scope alone is an edit"
    );
}

/// A decoder that states the unit cube with repro metadata whose context
/// nests past the levels the digest walk admits, outside every record.
struct DeepContextCodec;

impl CodecBackend for DeepContextCodec {
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
        panic!("the digest test does not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        let finding = crate::report::check::Finding {
            rule: crate::rule_name!("test.unresolved-link"),
            check: crate::report::check::Check::NativeLinks,
            severity: crate::report::Severity::Error,
            entity: None,
            related: Vec::new(),
            message: "unresolved link".into(),
        };
        let levels = usize::try_from(crate::bounds::DIGEST_WALK_DEPTH).unwrap();
        ir.repro = Some(Box::new(crate::repro::BundleMetadata {
            signature: crate::repro::Signature::new(
                &finding,
                crate::codec_id!("cadir"),
                crate::repro::Stage::Check,
            ),
            roots: Vec::new(),
            context: (0..levels).fold(crate::attributes::AttributeJson::Null, |inner, _| {
                crate::attributes::AttributeJson::Array(vec![inner])
            }),
            manifest: Vec::new(),
            incomplete_reason: None,
        }));
        Ok(decoded(ir))
    }
}

#[test]
fn a_document_without_canonical_json_keeps_no_content_digest_and_reads_as_edited() {
    let result = DeepContextCodec
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("a digest the walk refuses does not refuse the decode");
    assert_eq!(result.source_fidelity().content_digest, None);
    assert!(result.source_fidelity().edited(result.ir()).unwrap());
    let unavailable = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/content-digest.unavailable")
        .collect::<Vec<_>>();
    let [loss] = unavailable.as_slice() else {
        panic!("one loss for the missing digest: {unavailable:?}");
    };
    assert_eq!(loss.severity, crate::report::Severity::Warning);
    let outside = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::unknown::part::record_unreadable_kind())
        .collect::<Vec<_>>();
    let [loss] = outside.as_slice() else {
        panic!("one loss for the depth outside every record: {outside:?}");
    };
    assert!(
        loss.message.contains("outside every record"),
        "{}",
        loss.message
    );
}

/// A decoder that states, beside the unit cube, two native records: one at
/// the document's depth bound and one a level past it. A native record sits
/// at the document's fifth level, so its field nests at most 123 levels.
struct PastTheBoundCodec;

impl PastTheBoundCodec {
    const WITHIN: &'static str = "test:native:within#1";
    const PAST: &'static str = "test:native:past#1";
}

impl CodecBackend for PastTheBoundCodec {
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
        panic!("the depth bound test does not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        let record = |id: &str, depth: usize| {
            let field = (0..depth).fold(serde_json::Value::Null, |inner, _| {
                serde_json::Value::Array(vec![inner])
            });
            crate::NativeRecord::new(
                crate::ids::Identity::new(id).expect("valid native identity"),
                [("field".to_owned(), field)].into_iter().collect(),
            )
            .expect("valid native record")
        };
        let within = crate::bounds::MAX_DOCUMENT_DEPTH - 5;
        ir.native
            .namespace_mut(crate::design::names::CodecId::new("test").expect("a codec id"))
            .set_arena_records(
                "deep",
                vec![record(Self::WITHIN, within), record(Self::PAST, within + 1)],
            )
            .expect("a valid arena");
        Ok(decoded(ir))
    }
}

/// The wrapper digests a document at the bound and gives up the record past
/// it under a policy whose recursion ceiling is the bound's, too.
#[test]
fn the_sealed_wrapper_gives_up_a_record_past_the_document_bound() {
    for policy in [DecodePolicy::desktop(), DecodePolicy::service()] {
        let options = DecodeOptions {
            policy,
            ..DecodeOptions::default()
        };
        let result = PastTheBoundCodec
            .decode(&mut Cursor::new(Vec::new()), &options)
            .expect("a record past the bound does not refuse the file");
        let arena = result
            .ir()
            .native
            .namespace("test")
            .expect("the test namespace")
            .arenas()
            .get("deep")
            .expect("the deep arena");
        let kept = arena
            .iter()
            .map(crate::NativeRecord::id)
            .collect::<Vec<_>>();
        assert_eq!(kept, [PastTheBoundCodec::WITHIN]);
        let unreadable = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == crate::unknown::part::record_unreadable_kind())
            .collect::<Vec<_>>();
        let [loss] = unreadable.as_slice() else {
            panic!("one loss for the one record: {unreadable:?}");
        };
        assert_eq!(loss.severity, crate::report::Severity::Error);
        assert!(
            loss.message.contains(PastTheBoundCodec::PAST)
                && loss.message.contains("native.test.deep"),
            "{}",
            loss.message
        );
        let [stub]: [_; 1] = result
            .ir()
            .native_unknowns("test")
            .expect("the test stubs")
            .try_into()
            .expect("one stub for the one record");
        assert!(
            loss.message
                .contains(&format!("record={}", stub.id.as_str())),
            "{}",
            loss.message
        );
        let retained = result
            .source_fidelity()
            .retained_record(stub.id.as_str())
            .expect("the record's retained text");
        assert_eq!(
            retained.stream(),
            format!("native.test.deep/{}", PastTheBoundCodec::PAST)
        );
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).unwrap();
        assert_eq!(
            result.source_fidelity().content_digest,
            Some(crate::hash::content_digest(&ctx, result.ir()).unwrap())
        );
        let text = serde_json::to_string(result.ir()).unwrap();
        assert!(CadIr::from_json(&text).is_ok());
    }
}
