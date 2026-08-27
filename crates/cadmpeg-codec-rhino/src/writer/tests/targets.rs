// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::units::Units;

use crate::{RhinoArchiveVersion, RhinoEncoder};

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
    let encoder = RhinoEncoder::new(RhinoArchiveVersion::V8);
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
    assert_eq!(requested, "rhino:nonesuch");
    for target in Encoder::targets(&encoder) {
        assert!(available.contains(target.id), "{available}");
    }
}
