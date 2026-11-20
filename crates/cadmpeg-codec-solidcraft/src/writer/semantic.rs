// SPDX-License-Identifier: Apache-2.0
//! The edited-design write: the retained document patched with the typed
//! edits of the neutral model, then packed again.
//!
//! The writer knows one edit family, a parameter's expression. It decodes the
//! retained source image again for its baseline, sets each edited
//! parameter's baseline expression back in a copy of the edited model, and
//! digests that copy: only when the digest equals the decode baseline did
//! nothing else change, and every other edit is refused by name. A model
//! cache is keyed to the document it was computed for, so a patched container
//! is written without its `cache/`: `SolidCraft` rebuilds it from the history.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    ArenaCoverage, Consumption, EncodeInput, ExportBody, PatchConsumption, WritePath,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;
use cadmpeg_ir::report::export::{CensusBasis, EntityCensus};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, System, ZipWriter};

use crate::loss::SolidcraftLossCode;
use crate::package::{Package, DOCUMENT_MEMBER, MARKER_MEMBER};

/// The member prefix of the model cache.
const CACHE_PREFIX: &str = "cache/";

fn refuse(reason: impl Into<String>) -> CodecError {
    CodecError::NotImplemented(reason.into())
}

/// One edited parameter: its name and the expression to write.
struct Edit {
    name: String,
    expression: String,
}

/// The decode baseline of `image`.
fn baseline(image: &[u8]) -> Result<CadIr, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(image, &arena, &policy)?;
    let decoded = crate::decode::decode(&ctx, root)?;
    ctx.finish_session()?;
    Ok(decoded.ir)
}

/// The parameters whose expression differs from the baseline, proved to be
/// the only change since decode.
fn edits(ir: &CadIr, image: &[u8]) -> Result<Vec<Edit>, CodecError> {
    let before = baseline(image)?;
    let original: BTreeMap<_, _> = before
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.as_str(), parameter))
        .collect();
    let mut edits = Vec::new();
    let mut reverted = ir.clone();
    for parameter in &mut reverted.model.parameters {
        let Some(base) = original.get(parameter.id.as_str()) else {
            return Err(refuse(format!(
                "parameter {} is new: this writer edits the expressions of existing parameters only",
                parameter.id
            )));
        };
        if parameter.expression != base.expression {
            edits.push(Edit {
                name: parameter.name.clone(),
                expression: parameter.expression.clone(),
            });
            parameter.expression.clone_from(&base.expression);
            parameter.value.clone_from(&base.value);
            parameter.dependencies.clone_from(&base.dependencies);
        }
    }
    let expected = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
        .ok_or_else(|| refuse("the document carries no SolidCraft write baseline"))?;
    if super::document_local_sha256(&reverted)? != *expected {
        return Err(refuse(
            "the document changed beyond parameter expressions since decode, and this writer \
             has no serializer for those edits",
        ));
    }
    Ok(edits)
}

/// Set each edit in the `params` records of `document`.
fn patch(document: &mut Value, edits: &[Edit]) -> Result<(), CodecError> {
    let params = document
        .get_mut("params")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| refuse("the retained document has no params list"))?;
    for edit in edits {
        let record = params
            .iter_mut()
            .find(|record| record.get("name").and_then(Value::as_str) == Some(&edit.name))
            .ok_or_else(|| {
                refuse(format!(
                    "parameter {} is not a record of the document's params list, so its \
                     expression has no field to write",
                    edit.name
                ))
            })?;
        record["expr"] = Value::String(edit.expression.clone());
    }
    Ok(())
}

fn options(method: CompressionMethod) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(method)
        .system(System::Unix)
}

fn json_error(error: &serde_json::Error) -> CodecError {
    CodecError::malformed(format_args!("SolidCraft document: {error}"))
}

fn zip_error(error: &zip::result::ZipError) -> CodecError {
    CodecError::malformed(format_args!("SolidCraft container: {error}"))
}

/// Write the edited design.
pub(super) fn write(input: EncodeInput<'_>, image: &[u8]) -> Result<ExportBody, CodecError> {
    let edits = edits(input.ir, image)?;
    let mut losses = Vec::new();
    let bytes = if image.starts_with(b"PK") {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(image, &arena, &policy)?;
        let package = Package::open(&ctx, root)?;
        let mut document: Value =
            serde_json::from_slice(package.member(&ctx, DOCUMENT_MEMBER)?.window())
                .map_err(|error| json_error(&error))?;
        patch(&mut document, &edits)?;
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let mut dropped = 0_usize;
        let mut put = |name: &str, method, bytes: &[u8]| -> Result<(), CodecError> {
            zip.start_file(name, options(method))
                .map_err(|error| zip_error(&error))?;
            zip.write_all(bytes)?;
            Ok(())
        };
        put(
            MARKER_MEMBER,
            CompressionMethod::Stored,
            package.member(&ctx, MARKER_MEMBER)?.window(),
        )?;
        put(
            DOCUMENT_MEMBER,
            CompressionMethod::Deflated,
            &serde_json::to_vec(&document).map_err(|error| json_error(&error))?,
        )?;
        for name in package.roles().keys() {
            if name == MARKER_MEMBER || name == DOCUMENT_MEMBER {
                continue;
            }
            if name.starts_with(CACHE_PREFIX) {
                dropped += 1;
                continue;
            }
            put(
                name,
                CompressionMethod::Stored,
                package.member(&ctx, name)?.window(),
            )?;
        }
        if dropped > 0 {
            losses.push(SolidcraftLossCode::ModelCacheStale.note(format!(
                "{dropped} model cache members were not written: the cache is keyed to the \
                 unedited document, and SolidCraft rebuilds it from the history"
            )));
        }
        let out = zip
            .finish()
            .map_err(|error| zip_error(&error))?
            .into_inner();
        ctx.finish_session()?;
        out
    } else {
        let mut document: Value =
            serde_json::from_slice(image).map_err(|error| json_error(&error))?;
        patch(&mut document, &edits)?;
        let mut out = serde_json::to_vec_pretty(&document).map_err(|error| json_error(&error))?;
        if image.ends_with(b"\n") {
            out.push(b'\n');
        }
        out
    };
    Ok(ExportBody {
        bytes,
        census: EntityCensus {
            basis: CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path: WritePath::Patched {
            consumption: PatchConsumption::Independent(Consumption::NotConsumed),
        },
        coverage: ArenaCoverage::Complete,
        losses,
        notes: vec![format!(
            "{} parameter expressions were patched in the retained document",
            edits.len()
        )],
    })
}
