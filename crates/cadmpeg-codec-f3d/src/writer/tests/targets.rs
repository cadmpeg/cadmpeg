// SPDX-License-Identifier: Apache-2.0
//! The write-target request reaching this encoder's `plan`.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::units::Units;

use crate::F3dCodec;

/// An explicit target this writer does not produce is refused by `plan` itself,
/// with the catalog in the message.
///
/// The check runs before any synthesis, so an empty document is enough: what is
/// under test is that the request reaches the encoder at all. This writer has a
/// single dialect, which is exactly why the refusal must exist — every other id
/// is a claim it cannot honour.
#[test]
fn plan_refuses_an_explicit_target_outside_the_catalog() {
    let ir = CadIr::empty(Units::default());
    let error = Encoder::plan(
        &F3dCodec,
        EncodeInput::new(&ir, None),
        TargetRequest::Explicit("f3d:nonesuch"),
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
    assert_eq!(format, "f3d");
    assert_eq!(requested, "f3d:nonesuch");
    for target in Encoder::targets(&F3dCodec) {
        assert!(available.contains(target.id), "{available}");
    }
}
