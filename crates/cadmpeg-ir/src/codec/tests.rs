// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::wire;
use cadmpeg_core::serde_erase::json;

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
    crate::report::loss::NamespacedLossKind::new(
        crate::loss_namespace!("test"),
        "synthetic.reject-floor",
        LossTaxonomy::TopologyNotTransferred,
    )
    .with_strict_floor(Some(crate::report::Severity::Warning))
    .into()
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
fn the_sealed_decode_flags_a_shallower_delivery_that_a_strict_decode_admits() {
    let options = DecodeOptions {
        depth: DecodeDepth::Full,
        ..DecodeOptions::default()
    };
    let result = GeometryOnlyCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &options)
        .unwrap();
    assert_eq!(result.ir().scope(), ContentScope::geometry());
    assert_eq!(depth_losses(&result).len(), 2);

    let strict = GeometryOnlyCodec
        .decode(
            &mut Cursor::new(vec![1u8, 2, 3, 4]),
            &strict_options(DecodeDepth::Full),
        )
        .unwrap();
    let losses = depth_losses(&strict);
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.severity == crate::report::Severity::Error
            && loss.code.strict_floor().is_none()));

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
        Ok(json::from_value(serde_json::json!({
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
        json::from_value(serde_json::json!({
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
            assert!(!strict_options(requested).strictness.refuses(loss, false));
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

/// A decode that holds records of a class its scope excludes keeps them and
/// the scope; the breach is a part 10 finding (§2.1).
#[test]
fn the_wrapper_keeps_records_of_a_class_the_scope_excludes() {
    use crate::document::{ArenaName, ScopeViolation};

    let ir = with_parameter(unit_cube().expect("valid unit cube fixture"));
    for (depth, arena) in [
        (DecodeDepth::Container, "bodies"),
        (DecodeDepth::Geometry, "parameters"),
    ] {
        let mut decoded = decoded(ir.clone());
        decoded.body.transfer = crate::report::decode::DecodeTransfer::at(depth, true);
        let result = DecodeResult::new(
            decoded,
            FormatId::new("test"),
            depth,
            &DecodePolicy::default(),
        )
        .expect("a breach of the scope does not refuse the decode");
        assert_eq!(result.ir().scope(), depth.scope());
        assert_eq!(result.ir().model.parameters.len(), 1);
        let violations = result.ir().scope_violations();
        assert!(
            violations.contains(&ScopeViolation::Populated {
                arena: ArenaName::parse(arena).unwrap(),
            }),
            "{violations:?}"
        );
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
fn a_pinned_decode_refuses_a_given_up_repeated_id_and_an_unpinned_decode_admits_it() {
    let id_repeated = super::loss::DecodeLossCode::IdRepeated.note("").code;
    let error = RepeatedIdentityCodec { differ: true }
        .decode(
            &mut Cursor::new(Vec::new()),
            &strict_options(DecodeDepth::Full),
        )
        .unwrap_err();
    assert_eq!(refused_code(error), id_repeated);

    let result = RepeatedIdentityCodec { differ: true }
        .decode(
            &mut Cursor::new(Vec::new()),
            &DecodeOptions {
                depth: DecodeDepth::Full,
                ..DecodeOptions::default()
            },
        )
        .expect("an unpinned decode admits a given-up id");
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == id_repeated));
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
    ir.set_scope(scope);
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

/// A value outside every record that passes a bound is stated as the value
/// it takes when unstated, with one `record.unreadable` Error that names its
/// field, so the decode returns a document within the bounds (§2.5, §3.3)
/// and digests it.
#[test]
fn a_value_past_the_bound_outside_every_record_is_unstated_with_its_loss() {
    let result = DeepContextCodec
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("a value past the bound does not refuse the decode");
    assert!(result.ir().repro.is_none());
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    assert_eq!(
        result.source_fidelity().content_digest,
        Some(crate::hash::content_digest(&ctx, result.ir()).unwrap())
    );
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::unknown::part::record_unreadable_kind())
        .collect::<Vec<_>>();
    let [loss] = unreadable.as_slice() else {
        panic!("one loss for the value outside every record: {unreadable:?}");
    };
    assert_eq!(loss.severity, crate::report::Severity::Error);
    assert!(loss.message.contains("(repro)"), "{}", loss.message);
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
        let text = json::to_string(result.ir()).unwrap();
        assert!(CadIr::from_json(&text).is_ok());
    }
}

#[test]
fn a_pinned_decode_refuses_a_record_given_up_past_the_document_bound() {
    let error = PastTheBoundCodec
        .decode(
            &mut Cursor::new(Vec::new()),
            &strict_options(DecodeDepth::Full),
        )
        .unwrap_err();
    assert_eq!(
        refused_code(error),
        crate::unknown::part::record_unreadable_kind()
    );
}

/// A decoder that states, beside the unit cube, [`Self::RECORDS`] native
/// records past the document's depth bound, each holding the whole input as
/// its innermost string. Their retained text passes the base retained
/// allowance while the input stays small.
struct LargePastTheBoundCodec;

impl LargePastTheBoundCodec {
    const RECORDS: usize = 10;
    const INPUT_BYTES: usize = 2 << 20;

    fn id(index: usize) -> String {
        format!("test:native:large#{index}")
    }
}

impl CodecBackend for LargePastTheBoundCodec {
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
        panic!("the retained allowance test does not inspect")
    }

    fn decode_impl(&self, _ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        let text = std::str::from_utf8(root.window()).expect("ASCII input");
        let past = crate::bounds::MAX_DOCUMENT_DEPTH - 5 + 1;
        let records = (0..Self::RECORDS)
            .map(|index| {
                let field = (0..past).fold(serde_json::Value::from(text), |inner, _| {
                    serde_json::Value::Array(vec![inner])
                });
                crate::NativeRecord::new(
                    crate::ids::Identity::new(Self::id(index)).expect("valid native identity"),
                    [("field".to_owned(), field)].into_iter().collect(),
                )
                .expect("valid native record")
            })
            .collect();
        ir.native
            .namespace_mut(crate::design::names::CodecId::new("test").expect("a codec id"))
            .set_arena_records("large", records)
            .expect("a valid arena");
        Ok(decoded(ir))
    }
}

/// The retained-text allowance of the wrapper's give-ups is the decode's: it
/// grows with the input, and every record past the bound is given up and
/// reported when their text passes the base allowance.
#[test]
fn the_sealed_wrapper_gives_up_records_whose_text_passes_the_base_retained_allowance() {
    let input = vec![b'a'; LargePastTheBoundCodec::INPUT_BYTES];
    let result = LargePastTheBoundCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .expect("records past the bound do not refuse the file");
    assert!(result
        .ir()
        .native
        .namespace("test")
        .expect("the test namespace")
        .arenas()
        .get("large")
        .is_none_or(|arena| arena.iter().next().is_none()));
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::unknown::part::record_unreadable_kind())
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), LargePastTheBoundCodec::RECORDS);
    for index in 0..LargePastTheBoundCodec::RECORDS {
        let id = LargePastTheBoundCodec::id(index);
        let naming = unreadable
            .iter()
            .filter(|loss| loss.message.contains(&format!("/{id}\"")))
            .count();
        assert_eq!(naming, 1, "one loss names {id}");
    }
    let retained: u64 = result
        .source_fidelity()
        .retained_records()
        .values()
        .map(crate::source_fidelity::RetainedSourceRecord::byte_len)
        .sum();
    assert!(retained > 16 << 20, "{retained} retained bytes");
}

/// `MAX_DOCUMENT_DEPTH` placements of the unit u axis: a pcurve that nests
/// past the document bound wherever a record holds it.
fn deep_pcurve() -> crate::geometry::pcurve::PcurveGeometry {
    use crate::geometry::pcurve::{LinePcurve, PcurveGeometry, PlacedPcurve};
    (0..crate::bounds::MAX_DOCUMENT_DEPTH).fold(
        PcurveGeometry::Line(LinePcurve::U_AXIS),
        |basis, _| {
            PcurveGeometry::Transformed(
                PlacedPcurve::try_new(Box::new(basis), crate::transform::Transform2::identity())
                    .unwrap(),
            )
        },
    )
}

/// `MAX_DOCUMENT_DEPTH` placements of a unit line: a solved curve that nests
/// past the document bound wherever a record holds it.
fn deep_line() -> crate::geometry::SolvedCurveGeometry {
    use crate::geometry::analytic::LineCurve;
    use crate::geometry::{PlacedCurve, SolvedCurveGeometry};
    use crate::math::{Point3, Vector3};
    let line = LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap();
    (0..crate::bounds::MAX_DOCUMENT_DEPTH).fold(SolvedCurveGeometry::Line(line), |basis, _| {
        SolvedCurveGeometry::Transformed(
            PlacedCurve::try_new(Box::new(basis), crate::transform::Transform::identity()).unwrap(),
        )
    })
}

/// Adds curve `test:model:curve#<key>` and the intersection construction
/// `test:model:construction#<key>` it owns. The construction's support
/// pcurve is `pcurve`, and the curve's solved cache is `cache`.
fn add_procedural_curve(
    ir: &mut CadIr,
    key: &str,
    pcurve: Option<crate::geometry::pcurve::PcurveGeometry>,
    cache: Option<crate::geometry::SolvedCurveGeometry>,
) {
    use crate::geometry::intersection_curves::{IntcurveSupportContext, IntcurveSupportSide};
    use crate::geometry::procedural_curves::{ProceduralCurveDefinition, SupportPcurve};
    use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
    use crate::ids::{CurveId, ProceduralCurveId};
    let construction = ProceduralCurveId::mint(format!("test:model:construction#{key}")).unwrap();
    let side = |pcurve: Option<crate::geometry::pcurve::PcurveGeometry>| IntcurveSupportSide {
        surface: None,
        pcurve: pcurve.map(SupportPcurve::from),
    };
    let context = IntcurveSupportContext::try_new(
        [side(pcurve), side(None)],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag: false,
            cache: None,
        },
    ));
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint(format!("test:model:curve#{key}")).unwrap(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache,
        },
        source_object: None,
        native_ref: None,
    });
}

/// Adds surface `test:model:surface#<key>` and the taper construction
/// `test:model:construction#<key>` it owns, on the first surface and curve
/// of `ir`. The construction's reference pcurve is `pcurve`.
fn add_procedural_surface(
    ir: &mut CadIr,
    key: &str,
    pcurve: Option<crate::geometry::pcurve::PcurveGeometry>,
) {
    use crate::geometry::legacy_cache::CacheContract;
    use crate::geometry::procedural_surfaces::{ProceduralSurfaceDefinition, TaperSurfaceKind};
    use crate::geometry::surface_payloads::TaperSurfaceConstruction;
    use crate::geometry::{ProceduralSurface, Surface, SurfaceGeometry};
    use crate::ids::{ProceduralSurfaceId, SurfaceId};
    let construction = ProceduralSurfaceId::mint(format!("test:model:construction#{key}")).unwrap();
    let taper = TaperSurfaceConstruction::try_new(
        ir.geometry.surfaces[0].id.clone(),
        ir.geometry.curves[0].id.clone(),
        pcurve,
        0.0,
        TaperSurfaceKind::Standard {},
        CacheContract::legacy(),
    )
    .unwrap();
    ir.geometry.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Taper(taper),
        None,
    ));
    ir.geometry.surfaces.push(Surface {
        id: SurfaceId::mint(format!("test:model:surface#{key}")).unwrap(),
        geometry: SurfaceGeometry::Procedural {
            construction,
            cache: None,
        },
        source_object: None,
        native_ref: None,
    });
}

/// The decode result of `ir` after the wrapper's digest walk in a decode
/// under the service policy, the stock policy with the lowest recursion
/// ceiling, with its `record.unreadable` losses.
fn given_up(ir: CadIr) -> (DecodeResult, Vec<LossNote>) {
    let mut result = decode_result(ir);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    result
        .record_content_digest(&ctx, FormatId::new("test"))
        .expect("a record past the bound does not refuse the decode");
    let unreadable = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::unknown::part::record_unreadable_kind())
        .cloned()
        .collect();
    (result, unreadable)
}

/// Every construction has exactly one owning carrier and every carrier's
/// construction is in the document, every reference resolves, the document
/// reads back from its JSON, and its digest is the one the wrapper recorded.
fn assert_resolves(result: &DecodeResult) {
    let ir = result.ir();
    let constructions = ir
        .geometry
        .procedural_curves
        .iter()
        .map(|procedural| procedural.id.as_str())
        .chain(
            ir.geometry
                .procedural_surfaces
                .iter()
                .map(|procedural| procedural.id.as_str()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let mut owned = ir
        .geometry
        .curves
        .iter()
        .filter_map(|curve| curve.geometry.procedural_construction())
        .map(crate::ids::ProceduralCurveId::as_str)
        .chain(
            ir.geometry
                .surfaces
                .iter()
                .filter_map(|surface| surface.geometry.procedural_construction())
                .map(crate::ids::ProceduralSurfaceId::as_str),
        )
        .collect::<Vec<_>>();
    owned.sort_unstable();
    assert_eq!(owned, constructions.into_iter().collect::<Vec<_>>());
    let unresolved = crate::validate::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.check == crate::report::check::Check::ReferentialIntegrity)
        .collect::<Vec<_>>();
    assert!(unresolved.is_empty(), "{unresolved:?}");
    let text = json::to_string(ir).unwrap();
    assert!(CadIr::from_json(&text).is_ok());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::desktop()).unwrap();
    assert_eq!(
        result.source_fidelity().content_digest,
        Some(crate::hash::content_digest(&ctx, ir).unwrap())
    );
}

/// The record a `record.unreadable` loss gives up, and the ids its
/// `dropped=[…]` names.
fn given_up_record(message: &str) -> (String, std::collections::BTreeSet<String>) {
    let record = message
        .split_once(": record ")
        .and_then(|(_, tail)| tail.split_once(" of arena "))
        .map(|(record, _)| record.to_owned())
        .unwrap_or_default();
    let dropped = message
        .split_once("dropped=[")
        .and_then(|(_, tail)| tail.split_once(']'))
        .map(|(ids, _)| {
            ids.split(", ")
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    (record, dropped)
}

/// The ids of the curves of `result`.
fn curve_ids(result: &DecodeResult) -> Vec<&str> {
    result
        .ir()
        .geometry
        .curves
        .iter()
        .map(|curve| curve.id.as_str())
        .collect()
}

/// A carrier past the bound is given up with the construction it owns.
#[test]
fn a_carrier_past_the_document_bound_is_given_up_with_its_construction() {
    let mut ir = unit_cube().unwrap();
    add_procedural_curve(&mut ir, "deep", None, Some(deep_line()));
    add_procedural_curve(&mut ir, "kept", None, None);
    let (result, unreadable) = given_up(ir);
    let [loss] = unreadable.as_slice() else {
        panic!("one loss for the one record: {unreadable:?}");
    };
    assert!(
        loss.message.contains("of arena geometry.curves"),
        "{}",
        loss.message
    );
    assert_eq!(
        given_up_record(&loss.message),
        (
            "test:model:curve#deep".to_owned(),
            ["test:model:construction#deep", "test:model:curve#deep"]
                .map(str::to_owned)
                .into()
        )
    );
    assert!(!curve_ids(&result).contains(&"test:model:curve#deep"));
    assert!(curve_ids(&result).contains(&"test:model:curve#kept"));
    assert_resolves(&result);
}

/// A construction past the bound is given up with the carrier that owns it,
/// so no carrier names a construction the document does not hold.
#[test]
fn a_construction_past_the_document_bound_is_given_up_with_its_carrier() {
    let mut ir = unit_cube().unwrap();
    add_procedural_curve(&mut ir, "deep", Some(deep_pcurve()), None);
    add_procedural_curve(&mut ir, "kept", None, None);
    let (result, unreadable) = given_up(ir);
    let [loss] = unreadable.as_slice() else {
        panic!("one loss for the one record: {unreadable:?}");
    };
    assert!(
        loss.message.contains("of arena geometry.procedural_curves"),
        "{}",
        loss.message
    );
    assert_eq!(
        given_up_record(&loss.message),
        (
            "test:model:construction#deep".to_owned(),
            ["test:model:construction#deep", "test:model:curve#deep"]
                .map(str::to_owned)
                .into()
        )
    );
    assert!(!curve_ids(&result).contains(&"test:model:curve#deep"));
    assert!(curve_ids(&result).contains(&"test:model:curve#kept"));
    assert_resolves(&result);
}

/// A surface construction past the bound is given up with the surface that
/// owns it, so no surface names a construction the document does not hold.
#[test]
fn a_surface_construction_past_the_document_bound_is_given_up_with_its_surface() {
    let mut ir = unit_cube().unwrap();
    add_procedural_surface(&mut ir, "deep", Some(deep_pcurve()));
    add_procedural_surface(&mut ir, "kept", None);
    let (result, unreadable) = given_up(ir);
    let [loss] = unreadable.as_slice() else {
        panic!("one loss for the one record: {unreadable:?}");
    };
    assert!(
        loss.message
            .contains("of arena geometry.procedural_surfaces"),
        "{}",
        loss.message
    );
    assert_eq!(
        given_up_record(&loss.message),
        (
            "test:model:construction#deep".to_owned(),
            ["test:model:construction#deep", "test:model:surface#deep"]
                .map(str::to_owned)
                .into()
        )
    );
    let surfaces = result
        .ir()
        .geometry
        .surfaces
        .iter()
        .map(|surface| surface.id.as_str())
        .collect::<Vec<_>>();
    assert!(!surfaces.contains(&"test:model:surface#deep"));
    assert!(surfaces.contains(&"test:model:surface#kept"));
    assert_resolves(&result);
}

/// An edge whose carrier is given up names it no longer: the give-up names
/// the edge among the records it drops and marks, and every reference of the
/// document resolves.
#[test]
fn an_edge_on_a_carrier_past_the_document_bound_names_no_given_up_curve() {
    let mut ir = unit_cube().unwrap();
    add_procedural_curve(&mut ir, "deep", None, Some(deep_line()));
    let deep = crate::ids::CurveId::mint("test:model:curve#deep").unwrap();
    ir.geometry.edges[0].carrier =
        crate::topology::EdgeCarrier::new(Some(deep.clone()), None).unwrap();
    let edge = ir.geometry.edges[0].id.clone();
    let (result, unreadable) = given_up(ir);
    let [loss] = unreadable.as_slice() else {
        panic!("one loss for the one record: {unreadable:?}");
    };
    assert!(loss.message.contains(edge.as_str()), "{}", loss.message);
    assert!(!curve_ids(&result).contains(&deep.as_str()));
    assert!(result
        .ir()
        .geometry
        .edges
        .iter()
        .all(|edge| edge.carrier.curve() != Some(&deep)));
    assert_resolves(&result);
}

/// A give-up that drops another record moves the places after it, and each
/// record past the bound is given up at its own place. The deep
/// construction `#a` is given up first and drops `curve#a`, which moves the
/// deep `curve#b` one place: a stale place would remove `curve#c`.
#[test]
fn records_past_the_bound_in_one_walk_are_each_given_up_at_their_place() {
    let mut ir = unit_cube().unwrap();
    add_procedural_curve(&mut ir, "a", Some(deep_pcurve()), None);
    add_procedural_curve(&mut ir, "b", None, Some(deep_line()));
    add_procedural_curve(&mut ir, "c", None, None);
    let (result, unreadable) = given_up(ir);
    let mut named = unreadable
        .iter()
        .map(|loss| given_up_record(&loss.message))
        .collect::<Vec<_>>();
    named.sort();
    let ids = |ids: [&str; 2]| ids.map(str::to_owned).into();
    assert_eq!(
        named,
        [
            (
                "test:model:construction#a".to_owned(),
                ids(["test:model:construction#a", "test:model:curve#a"])
            ),
            (
                "test:model:curve#b".to_owned(),
                ids(["test:model:construction#b", "test:model:curve#b"])
            ),
        ]
    );
    assert!(curve_ids(&result).contains(&"test:model:curve#c"));
    assert!(result
        .ir()
        .geometry
        .procedural_curves
        .iter()
        .any(|procedural| procedural.id.as_str() == "test:model:construction#c"));
    assert_resolves(&result);
}

/// A decoder that gives up the cube's first point, keeps the vertex that
/// needs it, and drops two source records that need it in turn, and two
/// source records whose need it never gives up.
struct GivenUpPointCodec;

impl CodecBackend for GivenUpPointCodec {
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
        panic!("the given-up point test does not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        use crate::unknown::part::{dropped_with, GivenUpRecord};
        let mut ir = unit_cube().expect("valid unit cube fixture");
        let point = ir.geometry.points.remove(0).id;
        let mut decoded = decoded(ir);
        let instance = |key: u32| {
            crate::ids::Identity::new(format!("test:file:instance#{key}")).expect("an id")
        };
        decoded.body.losses.extend([
            dropped_with(instance(10), instance(9)),
            dropped_with(instance(9), point.clone()),
            GivenUpRecord::new(point, "the point's coordinates do not read").loss(),
            dropped_with(instance(6), instance(7)),
            dropped_with(instance(7), instance(8)),
        ]);
        Ok(decoded)
    }
}

#[test]
fn the_sealed_wrapper_names_each_record_dropped_with_a_given_up_record_in_its_one_loss() {
    let result = GivenUpPointCodec
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("a given-up point does not refuse the file");
    let cube = unit_cube().expect("valid unit cube fixture");
    let point = cube.geometry.points[0].id.as_str();
    let vertex = cube.geometry.vertices[0].id.as_str();
    assert!(!result
        .ir()
        .geometry
        .vertices
        .iter()
        .any(|kept| kept.id.as_str() == vertex));
    let unreadable = crate::unknown::part::record_unreadable_kind();
    let charges = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == unreadable)
        .collect::<Vec<_>>();
    let [given_up, unneeded] = charges.as_slice() else {
        panic!("one loss for the point and one for the record whose need is kept: {charges:?}");
    };
    assert!(given_up
        .message
        .starts_with(&format!("record {point} given up: ")));
    let (_, dropped) = crate::unknown::part::charged(given_up).expect("a charge");
    for id in ["test:file:instance#10", "test:file:instance#9", vertex] {
        assert!(dropped.iter().any(|dropped| dropped == id), "{given_up:?}");
        assert!(given_up.message.contains(id), "{given_up:?}");
    }
    assert_eq!(
        unneeded.message,
        "record test:file:instance#7 given up: it needs test:file:instance#8, which no give-up \
         of the decode names; dropped=[test:file:instance#6]"
    );
    let with = super::loss::DecodeLossCode::RecordDroppedWith.kind();
    assert!(!result.report().losses.iter().any(|loss| loss.code == with));
}

/// A decoder that keeps the cube with the edits of `.0` and charges the id
/// `.1` given up.
struct ChargesLabelCodec(fn(&mut CadIr), &'static str);

impl CodecBackend for ChargesLabelCodec {
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
        panic!("the charged-label test does not inspect")
    }

    fn decode_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<Decoded, CodecError> {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        (self.0)(&mut ir);
        let mut decoded = decoded(ir);
        decoded.body.losses.push(
            crate::unknown::part::GivenUpRecord::new(
                crate::ids::Identity::new(self.1).expect("an id"),
                "the record does not read",
            )
            .loss(),
        );
        Ok(decoded)
    }
}

fn decode_charging(codec: &ChargesLabelCodec) -> DecodeResult {
    codec
        .decode(&mut Cursor::new(Vec::new()), &DecodeOptions::default())
        .expect("a given-up record does not refuse the file")
}

/// A kept native record whose label field states the id of the record a
/// decoder gave up, as a structural issue states the scope it rejects,
/// keeps its place.
#[test]
fn a_native_record_whose_label_equals_a_charged_id_is_kept() {
    fn issue(ir: &mut CadIr) {
        let record = crate::NativeRecord::new(
            crate::ids::Identity::new("test:native:issue#1").unwrap(),
            [("scope".to_owned(), serde_json::json!("test:file:scope#3"))]
                .into_iter()
                .collect(),
        )
        .unwrap();
        ir.native
            .namespace_mut(crate::design::names::CodecId::new("test").unwrap())
            .set_arena_records("issues", vec![record])
            .unwrap();
    }
    let result = decode_charging(&ChargesLabelCodec(issue, "test:file:scope#3"));
    let issues = result
        .ir()
        .native
        .namespace("test")
        .and_then(|namespace| namespace.arena_as::<serde_json::Value>("issues").ok())
        .unwrap_or_default();
    assert_eq!(
        issues,
        [serde_json::json!({"id": "test:native:issue#1", "scope": "test:file:scope#3"})]
    );
}

/// A kept surface whose source object states the id of the record a decoder
/// gave up keeps its place, and so do the faces on it.
#[test]
fn a_surface_whose_source_object_equals_a_charged_id_is_kept() {
    fn labelled(ir: &mut CadIr) {
        ir.geometry.surfaces[0].source_object = Some(
            json::from_value(
                serde_json::json!({"format": "test", "object_id": "test:file:surface#7"}),
            )
            .unwrap(),
        );
    }
    let cube = unit_cube().unwrap();
    let result = decode_charging(&ChargesLabelCodec(labelled, "test:file:surface#7"));
    let ir = result.ir();
    assert_eq!(ir.geometry.surfaces.len(), cube.geometry.surfaces.len());
    assert_eq!(ir.geometry.faces.len(), cube.geometry.faces.len());
    assert_eq!(
        ir.geometry.surfaces[0]
            .source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("test:file:surface#7")
    );
}
