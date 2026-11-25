// SPDX-License-Identifier: Apache-2.0
//! Synthetic bundle builders and reader assertion adapters.
use crate::{reader::Model, OnshapeCodec};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
use std::io::{Cursor, Write};
use std::path::Path;
mod fixtures;
pub(crate) mod projection;
pub(crate) mod regions;

pub(crate) fn codec() -> OnshapeCodec {
    OnshapeCodec::new(|ctx, view| cadmpeg_codec_step::StepCodec::default().decode_impl(ctx, view))
}
fn step() -> &'static [u8] {
    static STEP: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    STEP.get_or_init(|| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let plan = cadmpeg_codec_step::StepCodec::default()
            .plan(
                EncodeInput::new(&ir, None),
                TargetRequest::Explicit("step:ap214"),
            )
            .unwrap();
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        bytes
    })
}
pub(crate) fn fixture(name: &str) -> Vec<u8> {
    let members = fixtures::members(name);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, value) in members.as_object().unwrap() {
        zip.start_file(path, options).unwrap();
        zip.write_all(&cadmpeg_core::serde_erase::json::to_vec(value).unwrap())
            .unwrap();
    }
    zip.start_file("partstudios/studio/export.step", options)
        .unwrap();
    zip.write_all(step()).unwrap();
    zip.finish().unwrap().into_inner()
}
#[derive(Debug)]
pub(crate) struct Loss {
    pub(crate) code: Option<String>,
    pub(crate) detail: String,
}
#[derive(Debug, Default)]
pub(crate) struct Report {
    losses: Vec<Loss>,
}
impl Report {
    pub(crate) fn losses(&self) -> &[Loss] {
        &self.losses
    }
}
pub(crate) fn read_bytes(
    _path: &Path,
    bytes: &[u8],
    report: &mut Report,
) -> Result<Model, cadmpeg_core::CodecError> {
    let result = codec()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .map_err(|e| cadmpeg_core::CodecError::malformed(e.to_string()))?;
    let (mut ir, decoded_report, source_fidelity) = result.into_parts();
    report.losses = decoded_report
        .losses
        .iter()
        .map(|n| Loss {
            code: Some(n.code.to_string()),
            detail: n.message.clone(),
        })
        .collect();
    let source_step = ir.model.assets.iter().find_map(|a| {
        if a.name
            .as_ref()
            .is_some_and(|n| n.as_str().ends_with("export.step"))
        {
            if let cadmpeg_ir::assets::AssetContent::Embedded { data } = &a.content {
                return Some(String::from_utf8(data.as_bytes().to_vec()).unwrap());
            }
        }
        None
    });
    ir.model.assets.clear();
    let transfer = cadmpeg_ir::report::decode::DecodeTransfer::full(!ir.geometry.bodies.is_empty());
    Ok(Model {
        ir,
        body: cadmpeg_ir::codec::DecodeBody::new(transfer),
        source_fidelity,
        source_step,
    })
}
pub(crate) mod assembly;
pub(crate) mod review;
