// SPDX-License-Identifier: Apache-2.0
//! Original XML fields select the source of an independent appearance producer.
use crate::native::entry_contents::EntryGrammar;
use crate::xml_framing::{XmlAttributeSource, XmlSource};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) struct Control<'source> {
    provider: XmlAttributeSource<'source>,
    property: XmlAttributeSource<'source>,
    kind: XmlAttributeSource<'source>,
    file: XmlAttributeSource<'source>,
    version: Option<XmlAttributeSource<'source>>,
}
pub(crate) struct Selection {
    pub(crate) provider: String,
    pub(crate) property: String,
    pub(crate) entry: String,
    pub(crate) version: u32,
}
fn required<'source>(
    ctx: &DecodeContext<'_>,
    xml: &XmlSource<'source>,
    index: usize,
    key: &str,
) -> Result<XmlAttributeSource<'source>, CodecError> {
    xml.attribute_source(ctx, index, key)?
        .ok_or_else(|| CodecError::malformed("appearance source control is absent"))
}
fn selected<'source>(
    ctx: &DecodeContext<'_>,
    xml: &XmlSource<'source>,
    index: usize,
    key: &str,
) -> Result<Option<XmlAttributeSource<'source>>, CodecError> {
    for name in xml.attribute_names(index)?.into_iter().rev() {
        if name != "xmlns" && !name.starts_with("xmlns:") && name.rsplit(':').next() == Some(key) {
            return xml.attribute_source(ctx, index, name);
        }
    }
    Ok(None)
}
impl<'source> Control<'source> {
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        xml: &XmlSource<'source>,
        value: usize,
    ) -> Result<Option<Self>, CodecError> {
        let mut property = None;
        let mut parent = xml.elements()[value].parent();
        while let Some(index) = parent {
            match xml.name(index)? {
                "Property" | "_Property" => property = Some(index),
                "ViewProvider" => {
                    let property = property
                        .ok_or_else(|| CodecError::malformed("appearance value has no property"))?;
                    return Ok(Some(Self {
                        provider: required(ctx, xml, index, "name")?,
                        property: required(ctx, xml, property, "name")?,
                        kind: required(ctx, xml, property, "type")?,
                        file: selected(ctx, xml, value, "file")?.ok_or_else(|| {
                            CodecError::malformed("appearance value has no source member")
                        })?,
                        version: selected(ctx, xml, value, "version")?,
                    }));
                }
                _ => {}
            }
            parent = xml.elements()[index].parent();
        }
        Ok(None)
    }
    pub(crate) fn read(
        &self,
        ctx: &DecodeContext<'_>,
        grammar: EntryGrammar,
    ) -> Result<Selection, CodecError> {
        let provider = self.provider.read(ctx)?;
        let property = self.property.read(ctx)?;
        let kind = self.kind.read(ctx)?;
        let file = self.file.read(ctx)?;
        let version = match grammar {
            EntryGrammar::Material(expected) => {
                let version = self
                    .version
                    .as_ref()
                    .map(|source| {
                        source
                            .read(ctx)?
                            .parse::<u32>()
                            .map_err(CodecError::malformed)
                    })
                    .transpose()?
                    .unwrap_or(0);
                if kind != "App::PropertyMaterialList"
                    || property != "ShapeAppearance"
                    || version != expected
                {
                    return Err(CodecError::malformed(
                        "appearance material control does not match its source interpretation",
                    ));
                }
                version
            }
            EntryGrammar::Color if kind == "App::PropertyColorList" => 0,
            _ => {
                return Err(CodecError::malformed(
                    "appearance control does not select a color or material list",
                ))
            }
        };
        Ok(Selection {
            provider,
            property,
            entry: crate::native::native_id("entry", &file),
            version,
        })
    }
}
pub(crate) fn count(mut source: View<'_>, grammar: EntryGrammar) -> Result<usize, CodecError> {
    let first = source.req_u32_le()?;
    let count = if matches!(grammar, EntryGrammar::Material(0 | 1)) && first & 0x8000_0000 != 0 {
        source.req_u32_le()?
    } else {
        first
    };
    usize::try_from(count).map_err(CodecError::malformed)
}
pub(crate) fn row(
    source: View<'_>,
    grammar: EntryGrammar,
    index: usize,
) -> Result<View<'_>, CodecError> {
    let header = if matches!(grammar, EntryGrammar::Material(0 | 1))
        && View::u32_le_at(source.window(), 0).is_some_and(|word| word & 0x8000_0000 != 0)
    {
        8
    } else {
        4
    };
    let width = if grammar == EntryGrammar::Color {
        4
    } else {
        24
    };
    let start = index
        .checked_mul(width)
        .and_then(|offset| offset.checked_add(header))
        .and_then(|offset| source.start().checked_add(offset))
        .ok_or_else(|| CodecError::malformed("appearance row offset overflows"))?;
    let end = start
        .checked_add(width)
        .ok_or_else(|| CodecError::malformed("appearance row end overflows"))?;
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("appearance row escapes its original entry"))
}
