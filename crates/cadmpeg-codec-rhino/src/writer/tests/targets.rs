// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::CadIr;

use crate::RhinoCodec;

/// An empty document that a Rhino decode of `dialect` would have produced.
fn source_in(dialect: &'static str) -> CadIr {
    let mut ir = CadIr::empty();
    ir.source = Some(cadmpeg_ir::document::SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect::DialectId::pinned(dialect),
        )),
        std::collections::BTreeMap::new(),
    ));
    ir
}

/// The resolved target `plan` reports for one request.
fn resolved(ir: &CadIr, encoder: RhinoCodec, request: TargetRequest<'_>) -> String {
    Encoder::plan(&encoder, EncodeInput::new(ir, None), request)
        .expect("the request resolves")
        .report()
        .target()
        .expect("a Rhino write always names its archive version")
        .as_str()
        .to_owned()
}

/// `convert old.3dm -o new.3dm` with no target flag keeps the archive version
/// the file already is.
///
/// The encoder carries no version at all, and the source is archive 50, so the
/// answer can only come from the source. This is the defect the resolution
/// closes — the round trip used to hand a Rhino 5 user a file their own Rhino
/// cannot open.
#[test]
fn inherit_resolves_to_the_source_archive_version() {
    let ir = source_in("rhino:archive-50");
    assert_eq!(
        resolved(&ir, RhinoCodec, TargetRequest::Inherit),
        "rhino:archive-50"
    );
}

/// An explicit target overrides the source: the flag is the escape from
/// preservation, and it says which archive version to write.
#[test]
fn an_explicit_target_wins_over_the_source_archive_version() {
    let ir = source_in("rhino:archive-50");
    assert_eq!(
        resolved(&ir, RhinoCodec, TargetRequest::Explicit("rhino:archive-70"),),
        "rhino:archive-70"
    );
}

/// With no Rhino source there is nothing to inherit, so `Inherit` falls back to
/// the catalog default — never to encoder state, which no longer exists.
///
/// This is the cross-format request the application and library callers send.
/// It changes no existing file's identity because there is no same-format
/// source whose identity could change.
#[test]
fn inherit_falls_back_to_the_catalog_default_with_nothing_to_inherit() {
    let ir = CadIr::empty();
    assert_eq!(
        resolved(&ir, RhinoCodec, TargetRequest::Inherit),
        "rhino:archive-80"
    );
}

/// A Rhino source that records no dialect refuses `Inherit`, uniformly with
/// every other encoder.
///
/// There is nothing to preserve, so the identity default cannot know what it
/// would be preserving, and any catalog row it picked could change what the
/// file is. The refusal quotes no dialect id, because none exists: the
/// `requested` field is `None`, not the bare format id it used to carry in one
/// codec.
#[test]
fn inherit_refuses_a_source_that_records_no_dialect() {
    let mut ir = CadIr::empty();
    ir.source = Some(
        serde_json::from_value(serde_json::json!({
            "format": "rhino",
            "attributes": {},
        }))
        .unwrap(),
    );
    let error = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect_err("a source with no recorded dialect is refused");

    let CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "rhino");
    assert_eq!(refusal.requested(), None);
    assert!(
        refusal
            .available()
            .iter()
            .any(|target| target.id.as_str() == "rhino:archive-80"),
        "{:?}",
        refusal.available()
    );
}

/// An explicit target that is not the source archive version charges a loss.
#[test]
fn a_dialect_changing_explicit_write_charges_displacement_by_name() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("rhino:archive-70"),
    )
    .expect("archive 70 is in the catalog");
    assert_eq!(
        plan.report().fidelity,
        cadmpeg_ir::FidelityResolution::NotProvided
    );
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == crate::loss::RhinoLossCode::SourceDialectDisplaced.kind())
        .expect("dialect displacement is charged");
    assert!(loss.message.contains("rhino:archive-50"));
    assert!(loss.message.contains("rhino:archive-70"));
}

/// An explicit target that is the source's own archive version changes nothing,
/// so it is not degraded.
#[test]
fn an_explicit_write_at_the_source_dialect_is_not_degraded() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("rhino:archive-50"),
    )
    .expect("archive 50 is in the catalog");
    assert_eq!(
        plan.report().fidelity,
        cadmpeg_ir::FidelityResolution::NotProvided
    );
}

/// A source archive version outside the synthesis catalog is refused, not
/// quietly rewritten as the catalog default.
///
/// Archives 2, 3, 4 and 90 decode and have no writer, and 3DM has no byte-replay
/// path that could preserve them, so preservation is impossible and the honest
/// answer is a refusal naming the source dialect and every target. An explicit
/// `--to rhino:<archive>` is the escape.
#[test]
fn inherit_refuses_a_source_archive_version_outside_the_catalog() {
    let ir = source_in("rhino:archive-3");
    let encoder = RhinoCodec;
    let error = Encoder::plan(
        &encoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .expect_err("a source dialect outside the catalog is refused");

    let CodecError::UnsupportedTarget(refusal) = &error else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(refusal.format(), "rhino");
    assert_eq!(refusal.requested(), Some("rhino:archive-3"));
    for target in Encoder::targets(&encoder) {
        assert!(
            refusal
                .available()
                .iter()
                .any(|available| available == target),
            "{:?}",
            refusal.available()
        );
    }
}

/// The §8.3 honesty invariant on the synthesis path: re-decoding the output
/// classifies the host layer into exactly the dialect the report named.
///
/// The assertion is against the bytes, not against the report twice. `target`
/// is a claim about what was written, and the only thing that can check a claim
/// about bytes is reading them back through the classifier the codec uses on
/// any other input. For 3DM the whole claim rests on the archive version word
/// in the file header, and writing a fixed word there makes this test fail.
#[test]
fn every_synthesized_target_re_decodes_as_the_dialect_the_report_named() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    let mut ir = CadIr::empty();
    ir.model.points.push(cadmpeg_ir::topology::Point {
        id: cadmpeg_ir::ids::PointId("cadir:model:point#honesty".into()),
        source_object: None,
        position: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
    });

    for version in [
        crate::RhinoArchiveVersion::V5,
        crate::RhinoArchiveVersion::V6,
        crate::RhinoArchiveVersion::V7,
        crate::RhinoArchiveVersion::V8,
    ] {
        let plan = Encoder::plan(
            &RhinoCodec,
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(version.descriptor().id.as_str()),
        )
        .unwrap_or_else(|error| panic!("{version:?} is a catalog row, got {error}"));
        let claimed = plan
            .report()
            .target()
            .cloned()
            .expect("a Rhino write always names its archive version");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("the plan writes");

        let decoded = crate::RhinoCodec
            .decode(
                &mut std::io::Cursor::new(written),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{version:?} output must decode, got {error}"));
        let classified = decoded
            .report()
            .dialects()
            .unwrap_or_else(|| panic!("{version:?} output must classify a host dialect"))
            .primary()
            .dialect()
            .clone();
        assert_eq!(
            classified, claimed,
            "{version:?}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}
