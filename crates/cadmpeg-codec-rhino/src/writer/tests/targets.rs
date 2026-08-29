// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::units::Units;

use crate::RhinoEncoder;

/// An explicit target this writer does not produce is refused by `plan` itself,
/// with the catalog in the message.
///
/// The check runs before any synthesis, so an empty document is enough: what is
/// under test is that the request reaches the encoder at all. A `plan` that
/// dropped the guard would write a Rhino archive and report success for a
/// dialect nobody asked for.
#[test]
fn plan_refuses_an_explicit_target_outside_the_catalog() {
    let ir = CadIr::empty(Units::default());
    let encoder = RhinoEncoder;
    let error = Encoder::plan(
        &encoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("rhino:nonesuch"),
    )
    .err()
    .expect("an id outside the catalog is refused");

    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "rhino");
    assert_eq!(requested.as_deref(), Some("rhino:nonesuch"));
    for target in Encoder::targets(&encoder) {
        assert!(available.contains(target.id), "{available}");
    }
}

/// An empty document that a Rhino decode of `dialect` would have produced.
fn source_in(dialect: &'static str) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(cadmpeg_ir::document::SourceMeta {
        format: "rhino".into(),
        dialect: Some(cadmpeg_core::dialect::DialectId::pinned(dialect)),
        ..cadmpeg_ir::document::SourceMeta::default()
    });
    ir
}

/// The resolved target `plan` reports for one request.
fn resolved(ir: &CadIr, encoder: RhinoEncoder, request: TargetRequest<'_>) -> String {
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
        resolved(&ir, RhinoEncoder, TargetRequest::Inherit),
        "rhino:archive-50"
    );
}

/// An explicit target overrides the source: the flag is the escape from
/// preservation, and it says which archive version to write.
#[test]
fn an_explicit_target_wins_over_the_source_archive_version() {
    let ir = source_in("rhino:archive-50");
    assert_eq!(
        resolved(
            &ir,
            RhinoEncoder,
            TargetRequest::Explicit("rhino:archive-70"),
        ),
        "rhino:archive-70"
    );
}

/// A cross-format conversion has nothing to inherit, so the application layer
/// asks for the catalog default outright, and `plan` writes exactly that —
/// never the version the encoder happens to carry.
#[test]
fn a_cross_format_request_resolves_to_the_catalog_default() {
    let ir = CadIr::empty(Units::default());
    let default = cadmpeg_ir::codec::default_target(crate::dialect::TARGETS)
        .expect("the Rhino catalog has a default");
    assert_eq!(
        resolved(&ir, RhinoEncoder, TargetRequest::Explicit(default.id),),
        "rhino:archive-80"
    );
}

/// With no Rhino source there is nothing to inherit, so `Inherit` falls back to
/// the catalog default — never to encoder state, which no longer exists.
///
/// This is the cross-format shape reached through `Inherit` instead of through
/// the application layer's `Explicit(catalog default)`. It is a legitimate
/// library-caller path, and it changes no existing file's identity, because
/// there is no same-format source whose identity could change.
#[test]
fn inherit_falls_back_to_the_catalog_default_with_nothing_to_inherit() {
    let ir = CadIr::empty(Units::default());
    assert_eq!(
        resolved(&ir, RhinoEncoder, TargetRequest::Inherit),
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
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(cadmpeg_ir::document::SourceMeta {
        format: "rhino".into(),
        dialect: None,
        ..cadmpeg_ir::document::SourceMeta::default()
    });
    let error = Encoder::plan(
        &RhinoEncoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .err()
    .expect("a source with no recorded dialect is refused");

    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "rhino");
    assert_eq!(*requested, None);
    assert!(available.contains("rhino:archive-80"), "{available}");
}

/// An explicit target that is not the source archive version charges a loss.
#[test]
fn a_dialect_changing_explicit_write_charges_displacement_by_name() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoEncoder,
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
        &RhinoEncoder,
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
    let encoder = RhinoEncoder;
    let error = Encoder::plan(
        &encoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .err()
    .expect("a source dialect outside the catalog is refused");

    let CodecError::UnsupportedTarget {
        format,
        requested,
        available,
        ..
    } = &error
    else {
        panic!("expected a target refusal, got {error}");
    };
    assert_eq!(format, "rhino");
    assert_eq!(requested.as_deref(), Some("rhino:archive-3"));
    for target in Encoder::targets(&encoder) {
        assert!(available.contains(target.id), "{available}");
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

    let mut ir = CadIr::empty(Units::default());
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
            &RhinoEncoder,
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(version.target()),
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
            .dialects
            .as_ref()
            .map(cadmpeg_core::dialect::DialectLayers::primary)
            .and_then(|entry| entry.dialect.clone())
            .unwrap_or_else(|| panic!("{version:?} output must classify a host dialect"));
        assert_eq!(
            classified, claimed,
            "{version:?}: the report claims {claimed} but the bytes are {classified}"
        );
    }
}
