// SPDX-License-Identifier: Apache-2.0
//! A CADIR write and read of a decoded document, for crate tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};
use cadmpeg_ir::document::CadIr;

/// The `arena.omitted` messages of a CADIR write of `ir`, and the document
/// that reading the written bytes gives.
pub(crate) fn round_trip(ir: &CadIr) -> (Vec<String>, CadIr) {
    let plan = CadirEncoder.plan(EncodeInput::new(ir, None)).unwrap();
    let omitted = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "arena.omitted")
        .map(|loss| loss.message.clone())
        .collect();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let (read, _, _) = CadIr::read(
        bytes.as_slice(),
        &cadmpeg_ir::document::read::ReadOptions::default(),
    )
    .unwrap()
    .without_sidecar();
    (omitted, read)
}
