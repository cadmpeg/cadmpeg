// SPDX-License-Identifier: Apache-2.0
//! End-to-end checks that native output is authored from document state.
#![allow(clippy::unwrap_used)]
#![cfg(any(
    feature = "fcstd",
    feature = "rhino",
    feature = "sldprt",
    feature = "f3d"
))]

#[cfg(feature = "fcstd")]
mod freecad;
#[cfg(feature = "fcstd")]
mod mixed_design;
#[cfg(any(feature = "fcstd", feature = "sldprt", feature = "f3d"))]
mod one_fact;
#[cfg(feature = "rhino")]
mod rhino;

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::ExportReport;
use cadmpeg_ir::{CadIr, SourceFidelity};
use cadmpeg_test_support::EditableDecodeResult;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn decode(codec: &dyn Codec, bytes: &[u8]) -> EditableDecodeResult {
    codec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode native document")
        .into()
}

fn write_decode(
    codec: &dyn Codec,
    encoder: &dyn Encoder,
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    target: Option<&str>,
) -> (EditableDecodeResult, ExportReport) {
    let plan = encoder
        .plan(
            EncodeInput::new(ir, fidelity),
            target.map_or(TargetRequest::Inherit, TargetRequest::Explicit),
        )
        .expect("plan native output from document state");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write native output");
    (decode(codec, &bytes), report)
}
