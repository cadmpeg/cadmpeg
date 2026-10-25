// SPDX-License-Identifier: Apache-2.0
//! Original material strings supply named attributes on their current asset.
use super::{GuiMaterial, GuiSources};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::attributes::{AttributeMapEntry, AttributeTarget, AttributeValue, SourceAttribute};

pub(super) struct Request<'a> {
    pub property: &'a str,
    pub name: &'a str,
    pub entry: &'a str,
    pub version: u32,
    pub index: usize,
    pub material: &'a GuiMaterial,
    pub appearance: &'a Appearance,
}
pub(super) fn string(ctx: &DecodeContext<'_>, mut source: View<'_>) -> Result<String, CodecError> {
    let count = usize::try_from(source.req_u32_le()?).map_err(CodecError::malformed)?;
    let value = std::str::from_utf8(source.req_take(count)?)
        .map_err(|_| CodecError::malformed("material source string is not UTF-8"))?;
    if value.contains('\0') {
        return Err(CodecError::malformed("material source string contains NUL"));
    }
    ctx.retain_bytes(u64_from_index(value.len()), "material attribute string")?;
    Ok(value.to_owned())
}
fn fields<'a>(request: &Request<'a>) -> [(&'static str, Option<&'a String>); 3] {
    [
        ("image", request.material.image.as_ref()),
        ("image_path", request.material.image_path.as_ref()),
        (
            "uuid",
            request
                .material
                .uuid
                .as_ref()
                .filter(|_| request.appearance.asset_guid.is_none()),
        ),
    ]
}
fn identity(request: &Request<'_>) -> Result<cadmpeg_ir::ids::AttributeId, CodecError> {
    cadmpeg_ir::ids::AttributeId::mint(crate::native::native_child_id(
        "attribute",
        request.appearance.id.as_str(),
        "material-strings",
    ))
    .map_err(CodecError::malformed)
}
fn construct(
    request: &Request<'_>,
    id: cadmpeg_ir::ids::AttributeId,
    name: String,
    values: Vec<AttributeMapEntry>,
) -> Result<SourceAttribute, CodecError> {
    let arena = cadmpeg_ir::document::ArenaName::ALL
        .iter()
        .copied()
        .find(|arena| arena.as_str() == "appearances")
        .ok_or_else(|| CodecError::malformed("appearance arena is not registered"))?;
    Ok(SourceAttribute {
        id,
        native_ref: Some(request.entry.to_owned()),
        target: AttributeTarget::Record {
            arena,
            id: request.appearance.id.as_str().to_owned(),
        },
        name,
        values: vec![AttributeValue::Map(values)],
    })
}

/// Reconstruct a source-less attribute from the current typed material fields.
pub(super) fn from_material(request: Request<'_>) -> Result<Option<SourceAttribute>, CodecError> {
    let values = fields(&request)
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| AttributeMapEntry {
                key: key.to_owned(),
                value: value.clone(),
            })
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(None);
    }
    construct(
        &request,
        identity(&request)?,
        request.name.to_owned(),
        values,
    )
    .map(Some)
}

/// Each decoded attribute obtains its own name and strings from original Views.
pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    sources: &GuiSources<'_, '_>,
    request: Request<'_>,
) -> Result<Option<SourceAttribute>, CodecError> {
    let selected = fields(&request);
    let count = selected.iter().filter(|(_, value)| value.is_some()).count();
    if count == 0 {
        return Ok(None);
    }
    let names = sources
        .gui_entry_interpretations
        .and_then(|memo| memo.material_name(request.property))
        .or_else(|| {
            sources
                .entry_interpretations
                .and_then(|memo| memo.material_name(request.property))
        })
        .ok_or_else(|| {
            CodecError::malformed("material attribute has no original property-name source")
        })?;
    let id = identity(&request)?;
    ctx.record(id.as_str(), names.primary(), || {
        let name = names.read(ctx)?;
        ctx.retain_bytes(
            u64_from_index(
                count
                    .checked_mul(std::mem::size_of::<AttributeMapEntry>())
                    .ok_or_else(|| CodecError::malformed("material attribute storage overflows"))?,
            ),
            "material attribute map",
        )?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| CodecError::malformed("material attribute allocation failed"))?;
        for (field, (key, held)) in selected.into_iter().enumerate() {
            if held.is_none() {
                continue;
            }
            let view = sources
                .gui_entry_interpretations
                .map(|memo| {
                    memo.material_string(ctx, request.entry, request.version, request.index, field)
                })
                .transpose()?
                .flatten()
                .or(sources
                    .entry_interpretations
                    .map(|memo| {
                        memo.material_string(
                            ctx,
                            request.entry,
                            request.version,
                            request.index,
                            field,
                        )
                    })
                    .transpose()?
                    .flatten())
                .ok_or_else(|| {
                    CodecError::malformed("material attribute has no original string source")
                })?;
            ctx.retain_bytes(u64_from_index(key.len()), "material attribute key")?;
            values.push(AttributeMapEntry {
                key: key.to_owned(),
                value: string(ctx, view)?,
            });
        }
        construct(&request, id.clone(), name, values).map(Some)
    })
}
