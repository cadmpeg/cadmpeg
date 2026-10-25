// SPDX-License-Identifier: Apache-2.0
//! Source fields retained by scanner-owned property records.

use crate::native::DynamicPropertyMeta;
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(super) fn dynamic(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    peek: &BTreeMap<String, String>,
) -> Result<Option<DynamicPropertyMeta>, CodecError> {
    let Some(group) = source.attribute(ctx, index, "group")? else {
        return Ok(None);
    };
    let integer = if peek
        .get("attr")
        .is_some_and(|text| text.parse::<i64>().is_ok())
    {
        source
            .attribute(ctx, index, "attr")?
            .and_then(|text| text.parse().ok())
    } else {
        None
    };
    let boolean = |name| -> Result<Option<bool>, CodecError> {
        if peek
            .get(name)
            .and_then(|text| crate::native::parse_bool(text))
            .is_none()
        {
            return Ok(None);
        }
        Ok(source
            .attribute(ctx, index, name)?
            .as_deref()
            .and_then(crate::native::parse_bool))
    };
    Ok(Some(DynamicPropertyMeta {
        group,
        documentation: source.attribute(ctx, index, "doc")?,
        attributes: integer,
        read_only: boolean("ro")?,
        hidden: boolean("hide")?,
    }))
}

pub(crate) fn value_attributes(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    opaque: bool,
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut attributes = BTreeMap::new();
    for key in source.attribute_names(index)?.into_iter().rev() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        if attributes.contains_key(local)
            || opaque && !matches!(local, "file" | "File" | "name" | "Name")
        {
            continue;
        }
        if let Some(value) = source.attribute(ctx, index, key)? {
            attributes.insert(local.to_owned(), value);
        }
    }
    Ok(attributes)
}

/// Reads the same last local-name winner as the retained value dictionary.
pub(crate) fn value_attribute(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
    local: &str,
) -> Result<Option<String>, CodecError> {
    let name = source
        .attribute_names(index)?
        .into_iter()
        .rev()
        .find(|key| {
            *key != "xmlns" && !key.starts_with("xmlns:") && key.rsplit(':').next() == Some(local)
        });
    name.map_or(Ok(None), |name| source.attribute(ctx, index, name))
}

/// Selects the same source fields as `value_attributes` without retaining them.
/// Dependency admission uses this navigation before deciding which values survive.
pub(crate) fn peek_value_attributes(
    ctx: &DecodeContext<'_>,
    source: &XmlSource<'_>,
    index: usize,
) -> Result<BTreeMap<String, String>, CodecError> {
    let peek = source.peek_attributes(ctx, index)?;
    let mut attributes = BTreeMap::new();
    for key in source.attribute_names(index)?.into_iter().rev() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        if attributes.contains_key(local) {
            continue;
        }
        if let Some(value) = peek.get(key) {
            attributes.insert(local.to_owned(), value.clone());
        }
    }
    Ok(attributes)
}

#[cfg(test)]
mod tests;
