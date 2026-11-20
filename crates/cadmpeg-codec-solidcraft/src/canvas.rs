// SPDX-License-Identifier: Apache-2.0
//! Canvases.
//!
//! A canvas is a reference image placed on a plane. Its image decodes to an
//! embedded asset: in a container the bytes of the `image_blob` member, in
//! plain JSON the base64 `data`. Where the image sits (plane, centre, width,
//! angle, opacity, flip, lock, component) has no neutral record and stays in
//! the retained `canvases` section.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent, AssetData, AssetId};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{IdentityKey, UnknownId};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use crate::loss::SolidcraftLossCode;
use crate::package::Package;
use crate::points::base64;

/// The media type of a canvas format.
fn media_type(format: &str) -> Option<&'static str> {
    match format {
        "png" => Some("image/png"),
        "jpeg" => Some("image/jpeg"),
        _ => None,
    }
}

/// The image bytes of the canvas `record`.
fn image<'a>(
    ctx: &DecodeContext<'a>,
    package: Option<&Package<'a>>,
    record: &Value,
) -> Result<Option<Vec<u8>>, CodecError> {
    if let Some(name) = record.get("image_blob").and_then(Value::as_str) {
        let Some(package) = package.filter(|package| package.contains(name)) else {
            return Ok(None);
        };
        return Ok(Some(package.member(ctx, name)?.window().to_vec()));
    }
    Ok(record.get("data").and_then(Value::as_str).and_then(base64))
}

/// The canvas images of `document`, as embedded assets.
pub(crate) fn canvases<'a>(
    ctx: &DecodeContext<'a>,
    package: Option<&Package<'a>>,
    document: &serde_json::Map<String, Value>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let Some(records) = document.get("canvases").and_then(Value::as_array) else {
        return Ok(losses);
    };
    let namespace = cadmpeg_ir::identity_namespace!("solidcraft", "design", "canvas");
    let section = UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("solidcraft", "native", "section"),
        IdentityKey::encode_segment("canvases"),
    )
    .into_string();
    for (index, record) in records.iter().enumerate() {
        let key = record
            .get("id")
            .and_then(Value::as_u64)
            .map_or_else(|| index.to_string(), |id| id.to_string());
        let name = record
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let media = record
            .get("format")
            .and_then(Value::as_str)
            .and_then(media_type);
        let bytes = image(ctx, package, record)?.and_then(AssetData::new);
        let (Some(media), Some(data)) = (media, bytes) else {
            losses.push(SolidcraftLossCode::DocumentSectionUndecoded.note(format!(
                "the image of canvas {key} does not decode, so it stays in the retained section"
            )));
            continue;
        };
        ctx.charge_entities(u64_from_index(data.as_bytes().len()), "decode canvas image")?;
        let asset = Asset::try_new(
            AssetId::compose(&namespace, IdentityKey::encode_segment(&key)),
            name.filter(|name| !name.trim().is_empty()),
            Some(media.to_owned()),
            AssetContent::Embedded { data },
            Some(section.clone()),
        );
        match asset {
            Ok(asset) => ir.model.assets.push(asset),
            Err(error) => losses.push(
                SolidcraftLossCode::DocumentSectionUndecoded
                    .note(format!("canvas {key} has no neutral asset: {error}")),
            ),
        }
    }
    Ok(losses)
}
