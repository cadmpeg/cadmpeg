// SPDX-License-Identifier: Apache-2.0
//! A CADIR write and read of a decoded document, for crate tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;

/// The document read from the CADIR write of `ir`.
pub(crate) fn round_trip(ir: &CadIr) -> CadIr {
    let plan = CadirEncoder.plan(EncodeInput::new(ir, None)).unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let (read, _, _) = CadIr::read(
        bytes.as_slice(),
        &cadmpeg_ir::document::read::ReadOptions::default(),
    )
    .unwrap()
    .without_sidecar();
    read
}
