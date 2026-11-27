// SPDX-License-Identifier: Apache-2.0
//! Synthetic bundle builders and reader assertion adapters.
use crate::{reader::Model, OnshapeCodec};
use cadmpeg_core::serde_erase::json;
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
fn encode_step(ir: &cadmpeg_ir::CadIr) -> Vec<u8> {
    let plan = cadmpeg_codec_step::StepCodec::default()
        .plan(EncodeInput::new(ir, None).with_target(TargetRequest::Explicit("step:ap214")))
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    bytes
}
pub(crate) fn step() -> &'static [u8] {
    static STEP: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    STEP.get_or_init(|| encode_step(&cadmpeg_ir::examples::unit_cube().unwrap()))
}
/// A STEP export whose sole solid states no name.
fn unnamed_step() -> &'static [u8] {
    static STEP: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    STEP.get_or_init(|| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for body in &mut ir.geometry.bodies {
            body.name = None;
        }
        encode_step(&ir)
    })
}
pub(crate) fn fixture(name: &str) -> Vec<u8> {
    let members = fixtures::members(name);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, value) in members.as_object().unwrap() {
        zip.start_file(path, options).unwrap();
        zip.write_all(&json::to_vec(value).unwrap()).unwrap();
    }
    for path in members.as_object().unwrap().keys() {
        if let Some(folder) = path.strip_suffix("/features.json") {
            zip.start_file(format!("{folder}/export.step"), options)
                .unwrap();
            zip.write_all(if name == "derived" {
                unnamed_step()
            } else {
                step()
            })
            .unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}
/// How a report states a codec code.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Charge {
    /// A note of the code.
    Note,
    /// A give-up of this cause whose text carries the code.
    GivenUp(cadmpeg_ir::unknown::part::Cause),
    /// A note or a give-up of either cause: for an assertion that no loss
    /// states the code.
    Any,
}
/// Whether a loss of kind `kind` and text `message` states the code `code`
/// as `charge`.
pub(crate) fn states(code: &str, charge: Charge, kind: &str, message: &str) -> bool {
    use cadmpeg_ir::unknown::part::Cause;
    let given_up =
        |cause: Cause| cause.kind().to_string() == kind && message.contains(&format!("{code}: "));
    match charge {
        Charge::Note => kind == code,
        Charge::GivenUp(cause) => given_up(cause),
        Charge::Any => kind == code || given_up(Cause::Unreadable) || given_up(Cause::Undecoded),
    }
}
#[derive(Debug)]
pub(crate) struct Loss {
    pub(crate) code: Option<String>,
    pub(crate) detail: String,
}
impl Loss {
    /// Whether the loss states `code` as `charge`.
    pub(crate) fn states(&self, code: &str, charge: Charge) -> bool {
        self.code
            .as_deref()
            .is_some_and(|kind| states(code, charge, kind, &self.detail))
    }
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
