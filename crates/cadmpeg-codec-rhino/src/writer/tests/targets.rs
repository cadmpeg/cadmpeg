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
        .target
        .as_ref()
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
        resolved(&ir, RhinoEncoder, TargetRequest::Explicit(default),),
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

/// An explicit target that is not the source's archive version charges the
/// fidelity, with a reason naming both dialects.
///
/// The write is not "fidelity was never offered": the source declared archive
/// 50 and the output declares archive 70, so an identity the file carried is
/// gone and the report says which one it was and what replaced it.
#[test]
fn a_dialect_changing_explicit_write_is_degraded_by_name() {
    let ir = source_in("rhino:archive-50");
    let plan = Encoder::plan(
        &RhinoEncoder,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("rhino:archive-70"),
    )
    .expect("archive 70 is in the catalog");
    let cadmpeg_ir::FidelityResolution::Degraded { reason } = &plan.report().fidelity else {
        panic!(
            "a dialect-changing write must be degraded, got {:?}",
            plan.report().fidelity
        );
    };
    assert!(reason.contains("rhino:archive-50"), "{reason}");
    assert!(reason.contains("rhino:archive-70"), "{reason}");
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
/// `--rhino-target` is the escape.
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
