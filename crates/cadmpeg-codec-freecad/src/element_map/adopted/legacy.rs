// SPDX-License-Identifier: Apache-2.0
//! Legacy XML map rows retain their actual leaf ownership.

use super::*;

pub(super) fn elements<'source>(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'source>,
    index: usize,
    count: usize,
    symbols: super::super::parts::StringIds<'_>,
    spacing: &dyn Fn(View<'source>) -> Result<(), CodecError>,
    stage: &mut PartOutput,
) -> Result<super::super::ParsedMap, CodecError> {
    if !source.peek_text(ctx, index)?.trim().is_empty() {
        return Err(CodecError::malformed(
            "legacy XML element map has unexpected text",
        ));
    }
    xml_spacing(source, index, spacing)?;
    let children = source.children(index).collect::<Vec<_>>();
    if children.len() != count {
        return Err(CodecError::malformed(
            "legacy ElementMap count differs from direct Element children",
        ));
    }
    let mut records =
        ctx.alloc_filled(children.len(), None, "FreeCAD legacy XML map record slots")?;
    for (slot, child) in records.iter_mut().zip(children) {
        *slot = ctx
            .decode_part(
                stage,
                source.extent(child)?,
                source.spec(child, "freecad.value_element")?,
                |_stage| {
                    source.validate(child)?;
                    if source.name(child)? != "Element" {
                        return Err(CodecError::malformed("legacy map child is not Element"));
                    }
                    if source
                        .peek_attribute(ctx, child, "sid")?
                        .and_then(|value| super::super::parse_legacy_string_ids(&value).ok())
                        .is_some_and(|ids| symbols.missing(&ids))
                    {
                        return Ok(None);
                    }
                    let attributes =
                        known_attributes(ctx, source, child, &["key", "value", "sid"])?;
                    let indexed_name = attributes
                        .get("value")
                        .cloned()
                        .ok_or_else(|| CodecError::malformed("legacy Element has no value"))?;
                    super::super::split_indexed_name(&indexed_name)?;
                    let mapped_name = attributes
                        .get("key")
                        .cloned()
                        .ok_or_else(|| CodecError::malformed("legacy Element has no key"))?;
                    let string_ids = attributes
                        .get("sid")
                        .map(|value| super::super::parse_legacy_string_ids(value))
                        .transpose()?
                        .unwrap_or_default();
                    if symbols
                        .known
                        .is_some_and(|ids| string_ids.iter().any(|id| !ids.contains(id)))
                    {
                        return Err(CodecError::malformed(
                            "legacy mapped name needs an unavailable persistent string id",
                        ));
                    }
                    Ok(Some(super::super::LegacyElementRecord {
                        indexed_name,
                        mapped_name,
                        string_ids,
                    }))
                },
            )?
            .flatten();
    }
    Ok(super::super::legacy_map_payload(records, count, None)?.parsed)
}
