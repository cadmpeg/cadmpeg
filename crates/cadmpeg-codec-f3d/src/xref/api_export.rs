// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ParseError, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::{ExternalDocument, ProductDefinition, Storage};

use super::{PropertyJson, ReferenceFieldsJson, StringProperty, XrefTable};
use crate::bytes::utf16::Utf16View;
use crate::container::ContainerScan;
use crate::layout::manifest_reference_extension_prefix as prefix;

const MIN_REFERENCE_BYTES: usize = 76 + 12 + 16 + 4 + 48;

#[derive(Debug)]
struct ReferenceError(CodecError);

impl From<ParseError> for ReferenceError {
    fn from(error: ParseError) -> Self {
        Self(error.into())
    }
}

impl From<CodecError> for ReferenceError {
    fn from(error: CodecError) -> Self {
        Self(error)
    }
}

fn require(value: bool, message: &'static str) -> Result<(), ReferenceError> {
    if value {
        Ok(())
    } else {
        Err(ReferenceError(CodecError::malformed(format_args!(
            "Manifest.dat reference extension: {message}"
        ))))
    }
}

fn text(view: &mut View<'_>) -> Result<String, ReferenceError> {
    let count = view.req_u32_le()?;
    require(
        u64::from(count)
            <= cadmpeg_core::decode::u64_from_index(crate::manifest::MAX_MANIFEST_STRING_UNITS),
        "string exceeds the manifest code-unit bound",
    )?;
    let length = view
        .counted(u64::from(count), 2)
        .ok_or_else(|| ReferenceError(CodecError::malformed("manifest string exceeds its extent")))?
        .get();
    let raw = view.req_take(length.checked_mul(2).ok_or_else(|| {
        ReferenceError(CodecError::malformed("manifest string length overflow"))
    })?)?;
    Utf16View::new(raw)
        .map(|value| value.to_string())
        .ok_or_else(|| ReferenceError(CodecError::malformed("invalid manifest reference UTF-16")))
}

pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<Option<XrefTable>, CodecError> {
    let Some(extension) = scan.manifest_reference_extension else {
        return Ok(None);
    };
    if !extension.window().starts_with(&[0, 0, 0, 0, 1]) {
        return Ok(None);
    }
    parse(ctx, extension).map(Some).map_err(|error| error.0)
}

fn parse(ctx: &DecodeContext<'_>, mut view: View<'_>) -> Result<XrefTable, ReferenceError> {
    let mut header = view.req_take_child(prefix::LEN)?;
    require(
        header.req_take(prefix::REFERENCE_FLAG)? == [0, 0, 0, 0],
        "invalid reference prologue",
    )?;
    require(
        header.req_u8()? == prefix::REFERENCE_FLAG_VALUE,
        "invalid reference flag",
    )?;
    let declared = header.req_u32_le()?;
    let count = view
        .counted(u64::from(declared), MIN_REFERENCE_BYTES)
        .ok_or_else(|| {
            ReferenceError(CodecError::malformed(
                "manifest reference count exceeds its extent",
            ))
        })?
        .get();
    require(count != 0, "empty reference run")?;
    let mut references = ctx.alloc_filled(count, None, "F3D manifest references")?;
    let mut documents = BTreeMap::new();
    let mut source_path = None;
    for (ordinal, slot) in references.iter_mut().enumerate() {
        let role = text(&mut view)?;
        require(
            crate::bytes::is_guid_hyphenated(&role),
            "invalid reference role",
        )?;
        let from = text(&mut view)?;
        if let Some(previous) = source_path.replace(from.clone()) {
            require(previous == from, "source document paths disagree")?;
        }
        let path = text(&mut view)?;
        let _cache_path = text(&mut view)?;
        for expected in [2, 4, 1, 0] {
            require(
                view.req_u32_le()? == expected,
                "invalid reference property prefix",
            )?;
        }
        let properties = view.req_u32_le()?;
        require(properties == 6, "unsupported reference property count")?;
        let mut values = BTreeMap::new();
        for _ in 0..properties {
            let key = text(&mut view)?;
            let value = text(&mut view)?;
            require(
                values.insert(key, value).is_none(),
                "duplicate reference property",
            )?;
        }
        let field = |name: &str| {
            values.get(name).ok_or_else(|| {
                ReferenceError(CodecError::malformed(format_args!(
                    "Manifest.dat reference extension lacks {name}"
                )))
            })
        };
        let lineage = field("LineageUrn")?;
        let _lineage_id = field("LineageId")?;
        let version = field("URL")?;
        let asset = field("linkedAssetGUID")?;
        require(
            crate::bytes::is_guid_hyphenated(asset),
            "invalid linked asset GUID",
        )?;
        require(
            field("linkClassification")? == "OriginalDesign",
            "unsupported reference classification",
        )?;
        let _display_name = field("toDocDisplayName")?;
        let document = ExternalDocument {
            document_id: NonBlankString::new(lineage.clone()),
            version: NonBlankString::new(version.clone()),
            ..ExternalDocument::default()
        };
        require(
            document.document_id.is_some() && document.version.is_some(),
            "empty document identity",
        )?;
        if let Some(previous) = documents.insert(path.clone(), document.clone()) {
            require(previous == document, "target identities disagree")?;
        }
        *slot = Some(
            ReferenceFieldsJson {
                from,
                relative_path: path,
                properties: vec![
                    PropertyJson::Role(StringProperty::String { value: role }),
                    PropertyJson::Data(StringProperty::String {
                        value: String::new(),
                    }),
                ],
            }
            .into_record(ordinal)?,
        );
    }
    require(view.is_empty(), "bytes follow the reference run")?;
    Ok(XrefTable {
        references: references.into_iter().flatten().collect(),
        documents,
        ..XrefTable::default()
    })
}

pub(crate) fn state_file_identity(
    model: &mut cadmpeg_ir::document::Model,
    table: Option<&XrefTable>,
    lineage_urn: Option<&str>,
) {
    let path = table
        .filter(|table| !table.documents.is_empty())
        .and_then(|table| table.references.first())
        .and_then(|reference| NonBlankString::new(reference.from.as_str()));
    let lineage = lineage_urn.and_then(NonBlankString::new);
    if path.is_none() && lineage.is_none() {
        return;
    }
    if !model
        .product_definitions
        .iter()
        .any(ProductDefinition::is_root)
    {
        crate::tree::file_root(model);
    }
    for root in model
        .product_definitions
        .iter_mut()
        .filter(|row| row.is_root())
    {
        if let Storage::File { document, .. } = &mut root.storage {
            if let Some(path) = &path {
                document.path.get_or_insert_with(|| path.clone());
            }
            if let Some(lineage) = &lineage {
                document.document_id.get_or_insert_with(|| lineage.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests;
