// SPDX-License-Identifier: Apache-2.0
//! Synthetic layers and helpers for crate tests.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};

use crate::layout::usdc_bootstrap as bootstrap;
use crate::UsdCodec;

/// The 88-byte USDC bootstrap header of version `version` (spec §9).
pub(crate) fn usdc_header(version: [u8; 3]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 88];
    bytes[bootstrap::IDENTIFIER..bootstrap::MAJOR].copy_from_slice(&bootstrap::IDENTIFIER_VALUE);
    bytes[bootstrap::MAJOR] = version[0];
    bytes[bootstrap::MINOR] = version[1];
    bytes[bootstrap::PATCH] = version[2];
    bytes
}

/// Decodes `text` as a layer.
pub(crate) fn decode(text: &str) -> DecodeResult {
    UsdCodec
        .decode(&mut Cursor::new(text.as_bytes()), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode failed: {error}\n{text}"))
}

/// Companion layers by path, each anchored at the directory of the input.
#[derive(Debug)]
pub(crate) struct Files(pub(crate) Vec<(&'static str, &'static [u8])>);

impl cadmpeg_core::decode::Companions for Files {
    fn open(
        &self,
        _anchor: Option<&str>,
        path: &str,
        _max_bytes: u64,
    ) -> Result<cadmpeg_core::decode::Companion, String> {
        let name = path.strip_prefix("./").unwrap_or(path);
        self.0
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(candidate, bytes)| cadmpeg_core::decode::Companion {
                id: (*candidate).to_owned(),
                bytes: bytes.to_vec(),
            })
            .ok_or_else(|| format!("{name}: not found"))
    }

    fn locate(&self, _anchor: Option<&str>, path: &str) -> Result<String, String> {
        let name = path.strip_prefix("./").unwrap_or(path);
        self.0
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(candidate, _)| (*candidate).to_owned())
            .ok_or_else(|| format!("{name}: not found"))
    }
}

/// Decodes `text` with the companion files of `files`.
pub(crate) fn decode_with(
    text: &str,
    files: &dyn cadmpeg_core::decode::Companions,
) -> DecodeResult {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) =
        DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy).expect("context");
    let ctx = ctx.with_companions(files);
    let result = UsdCodec
        .decode_with_context(&ctx, root)
        .unwrap_or_else(|failure| panic!("decode failed: {failure:?}\n{text}"));
    ctx.finish_session().expect("session");
    result
}

/// A written layer and its report.
pub(crate) type Written = (String, cadmpeg_ir::report::export::ExportReport);

/// Writes `ir` as a USDA layer of the target `request` resolves; for a
/// package target, the text is its root layer.
pub(crate) fn try_encode(
    ir: &cadmpeg_ir::CadIr,
    request: TargetRequest<'_>,
) -> Result<Written, cadmpeg_core::CodecError> {
    let plan = UsdCodec.plan(EncodeInput::new(ir, None), request)?;
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes)?;
    if bytes.starts_with(b"PXR-USDC") {
        // A crate layer is written from the text layer of the same model.
        let text = try_encode(ir, TargetRequest::Explicit("usda-1.0"))?.0;
        return Ok((text, report));
    }
    if crate::usdz::is_archive(&bytes) {
        bytes = with_root(&bytes, |ctx, root| {
            crate::usdz::open(ctx, root)
                .expect("open the written package")
                .root
                .window()
                .to_vec()
        });
    }
    Ok((
        String::from_utf8(bytes).expect("USDA text is UTF-8"),
        report,
    ))
}

/// The bytes that `ir` writes as for the target `request` resolves.
pub(crate) fn encode_bytes(ir: &cadmpeg_ir::CadIr, request: TargetRequest<'_>) -> Vec<u8> {
    let plan = UsdCodec
        .plan(EncodeInput::new(ir, None), request)
        .expect("plan the write");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("write");
    bytes
}

/// Decodes `bytes`: a layer or a package.
pub(crate) fn decode_bytes(bytes: &[u8]) -> DecodeResult {
    UsdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("decode failed: {error}"))
}

/// Writes `ir` as a USDA 1.0 layer.
pub(crate) fn encode(ir: &cadmpeg_ir::CadIr) -> Written {
    try_encode(ir, TargetRequest::Explicit("usda-1.0")).expect("write the layer")
}

/// Runs `run` with a decode context and the root view over `bytes`, and
/// finishes the session.
pub(crate) fn with_root<T>(
    bytes: &[u8],
    run: impl for<'a> FnOnce(&DecodeContext<'a>, cadmpeg_core::decode::View<'a>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("context");
    let value = run(&ctx, root);
    ctx.finish_session().expect("session");
    value
}

/// Runs `run` with a decode context over `bytes` and finishes the session.
pub(crate) fn with_context<T>(bytes: &[u8], run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("context");
    let value = run(&ctx);
    ctx.finish_session().expect("session");
    value
}

/// A layer with one prim body, `body`, under `def Xform "A"`.
pub(crate) fn prim_layer(body: &str) -> String {
    format!("#usda 1.0\ndef Xform \"A\"\n{{\n{body}\n}}\n")
}

/// A compressed buffer of one LZ4 block of literals (spec §9.3).
pub(crate) fn usdc_literal_buffer(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0_u8];
    let length = bytes.len();
    if length < 15 {
        out.push(u8::try_from(length).expect("length") << 4);
    } else {
        out.push(0xF0);
        let mut rest = length - 15;
        while rest >= 255 {
            out.push(255);
            rest -= 255;
        }
        out.push(u8::try_from(rest).expect("length"));
    }
    out.extend_from_slice(bytes);
    out
}

/// A compressed 32-bit integer sequence with its byte size (spec §9.4),
/// with every difference coded in 32 bits.
pub(crate) fn usdc_compressed_ints(values: &[i32]) -> Vec<u8> {
    let mut coded = 0_i32.to_le_bytes().to_vec();
    coded.resize(coded.len() + values.len().div_ceil(4), 0xFF);
    let mut previous = 0_i32;
    for value in values {
        coded.extend_from_slice(&(value - previous).to_le_bytes());
        previous = *value;
    }
    let buffer = usdc_literal_buffer(&coded);
    let mut out = u64::try_from(buffer.len())
        .expect("size")
        .to_le_bytes()
        .to_vec();
    out.extend_from_slice(&buffer);
    out
}
