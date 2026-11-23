// SPDX-License-Identifier: Apache-2.0
//! NIST FTC specimens, decoded once per test binary.

use std::io::Cursor;
use std::path::Path;
use std::sync::LazyLock;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};

use crate::SldprtCodec;

static FTC_06: LazyLock<DecodeResult> =
    LazyLock::new(|| decode("nist_ftc_06_asme1_rd_sw1802.SLDPRT"));
static FTC_08: LazyLock<DecodeResult> =
    LazyLock::new(|| decode("nist_ftc_08_asme1_rc_sw1802.SLDPRT"));

/// The decoded NIST FTC-06 specimen.
pub(crate) fn ftc_06() -> &'static DecodeResult {
    &FTC_06
}

/// The decoded NIST FTC-08 specimen.
pub(crate) fn ftc_08() -> &'static DecodeResult {
    &FTC_08
}

fn decode(name: &str) -> DecodeResult {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/sldprt")
        .join(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
}
