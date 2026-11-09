// SPDX-License-Identifier: Apache-2.0
//! A sheet body with several regions is one shell-based surface model.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

fn decode(bytes: &[u8]) -> CadIr {
    StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the surface model")
        .into_parts()
        .0
}

#[test]
fn a_sheet_body_with_two_regions_round_trips_as_one_surface_model() {
    let source = decode(include_bytes!(
        "../reader/topology/tests/data/tp14_two_shell_surface_model.p21"
    ));
    assert_eq!(source.model.bodies.len(), 1);
    assert_eq!(source.model.bodies[0].regions.len(), 2);
    let mut bytes = Vec::new();
    write_step(
        &source,
        &mut bytes,
        StepSchema::Ap242Edition3,
        &StepWriteOptions::default(),
    )
    .expect("write the surface model");
    assert_eq!(
        std::str::from_utf8(&bytes)
            .expect("STEP text")
            .matches("SHELL_BASED_SURFACE_MODEL(")
            .count(),
        1
    );
    let written = decode(&bytes);
    assert_eq!(written.model.bodies.len(), 1);
    assert_eq!(written.model.bodies[0].regions.len(), 2);
    assert_eq!(written.model.faces.len(), 2);
}
