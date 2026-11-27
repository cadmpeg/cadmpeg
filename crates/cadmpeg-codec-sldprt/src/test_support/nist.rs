// SPDX-License-Identifier: Apache-2.0
//! NIST FTC specimens, decoded once per test binary, and the write session of
//! each, which decodes its retained source once per test binary.

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::LazyLock;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::SourceFidelity;

use crate::{SldprtCodec, SldprtWriteSession};

/// One specimen: its bytes, its decode, and the session that
/// writes its edits.
struct Specimen {
    bytes: LazyLock<Vec<u8>>,
    decoded: LazyLock<DecodeResult>,
    session: LazyLock<SldprtWriteSession<'static>>,
}

static FTC_06: Specimen = Specimen {
    bytes: LazyLock::new(|| read("nist_ftc_06_asme1_rd_sw1802.SLDPRT")),
    decoded: LazyLock::new(|| decode(&FTC_06.bytes)),
    session: LazyLock::new(|| SldprtWriteSession::new(Some(FTC_06.decoded.source_fidelity()))),
};

static FTC_08: Specimen = Specimen {
    bytes: LazyLock::new(|| read("nist_ftc_08_asme1_rc_sw1802.SLDPRT")),
    decoded: LazyLock::new(|| decode(&FTC_08.bytes)),
    session: LazyLock::new(|| SldprtWriteSession::new(Some(FTC_08.decoded.source_fidelity()))),
};

/// The decoded NIST FTC-06 specimen.
pub(crate) fn ftc_06() -> &'static DecodeResult {
    &FTC_06.decoded
}

/// The decoded NIST FTC-08 specimen.
pub(crate) fn ftc_08() -> &'static DecodeResult {
    &FTC_08.decoded
}

/// The specimen whose file is `image`, found without decoding the others.
fn specimen(image: &[u8]) -> Option<&'static Specimen> {
    [&FTC_06, &FTC_08]
        .into_iter()
        .find(|specimen| specimen.bytes.as_slice() == image)
}

/// The decoded specimen whose file is `image`.
pub(crate) fn decoded(image: &[u8]) -> Option<&'static DecodeResult> {
    specimen(image).map(|specimen| &*specimen.decoded)
}

/// The decoded specimen whose file `fidelity` retains as its source image.
pub(crate) fn decoded_source(fidelity: &SourceFidelity) -> Option<&'static DecodeResult> {
    decoded(source_image(fidelity)?)
}

/// The write session of the specimen whose file `fidelity` retains as its
/// source image.
pub(crate) fn session(fidelity: &SourceFidelity) -> Option<&'static SldprtWriteSession<'static>> {
    specimen(source_image(fidelity)?).map(|specimen| &*specimen.session)
}

fn source_image(fidelity: &SourceFidelity) -> Option<&[u8]> {
    fidelity.retained_record(crate::SOURCE_IMAGE_ID)?.data()
}

fn read(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/nist-pmi/sldprt")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn decode(bytes: &[u8]) -> DecodeResult {
    SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode a NIST specimen: {error}"))
}
