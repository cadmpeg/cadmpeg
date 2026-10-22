// SPDX-License-Identifier: Apache-2.0
//! Parse-local source fields for assembly operand paths.

use crate::records::feature::assembly::{DesignAssemblyOperandPath, DesignAssemblyOperandPathLink};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

pub(super) struct ParsedOperandPath<'a> {
    pub(super) content: DesignAssemblyOperandPath,
    pub(super) source: PathSourceDraft<'a>,
}

pub(super) struct PathLinkSources<'a> {
    locator_reference: View<'a>,
    locator: View<'a>,
    scope_reference: View<'a>,
    wrapper_reference: View<'a>,
    wrapper: View<'a>,
    path_reference: View<'a>,
}

impl<'a> PathLinkSources<'a> {
    pub(super) fn new(
        source: View<'a>,
        locator_reference: u64,
        locator: usize,
        scope_reference: u64,
        wrapper_reference: u64,
        wrapper: usize,
        path_reference: u64,
    ) -> Option<Self> {
        Some(Self {
            locator_reference: field(source, usize::try_from(locator_reference).ok()?, 4)?,
            locator: header(source, locator, 4)?,
            scope_reference: field(source, usize::try_from(scope_reference).ok()?, 4)?,
            wrapper_reference: field(source, usize::try_from(wrapper_reference).ok()?, 4)?,
            wrapper: header(source, wrapper, 4)?,
            path_reference: field(source, usize::try_from(path_reference).ok()?, 4)?,
        })
    }
}

pub(super) struct PathSourceDraft<'a> {
    bulk: View<'a>,
    origins: Vec<View<'a>>,
    header: View<'a>,
    link: Option<PathLinkSources<'a>>,
    occurrences: Vec<View<'a>>,
    identities: Vec<View<'a>>,
    pairs: Vec<View<'a>>,
}

impl<'a> PathSourceDraft<'a> {
    pub(super) fn new(source: View<'a>, start: usize) -> Option<Self> {
        Some(Self {
            bulk: source,
            origins: Vec::from([source]),
            header: header(source, start, 8)?,
            link: None,
            occurrences: Vec::new(),
            identities: Vec::new(),
            pairs: Vec::new(),
        })
    }

    pub(super) fn set_link(&mut self, source: PathLinkSources<'a>) {
        self.link = Some(source);
    }

    pub(super) fn occurrence(&mut self, count_at: usize, end: usize) -> Option<()> {
        self.occurrences.push(text(self.bulk, count_at, end)?);
        Some(())
    }

    pub(super) fn identity(&mut self, count_at: usize, end: usize) -> Option<()> {
        self.identities.push(text(self.bulk, count_at, end)?);
        Some(())
    }

    pub(super) fn pair(&mut self, count_at: usize, end: usize) -> Option<()> {
        self.pairs.push(text(self.bulk, count_at, end)?);
        Some(())
    }

    pub(super) fn append(&mut self, continuation: Self) -> Option<()> {
        if !std::ptr::eq(self.bulk.window(), continuation.bulk.window())
            || self.bulk.start() != continuation.bulk.start()
            || self.bulk.end() != continuation.bulk.end()
        {
            return None;
        }
        self.origins.extend(continuation.origins);
        self.occurrences.extend(continuation.occurrences);
        self.identities.extend(continuation.identities);
        self.pairs.extend(continuation.pairs);
        Some(())
    }

    pub(super) fn read(
        &self,
        ctx: &DecodeContext<'_>,
        path: &DesignAssemblyOperandPath,
        scope_index: u32,
        prefix: &str,
        pointers: &mut BTreeMap<String, u64>,
    ) -> Result<(), CodecError> {
        for origin in &self.origins {
            ctx.capture_part_extent(*origin)?;
        }
        read_header(self.header, path.class_tag().as_str(), path.record_index)?;
        self.point(pointers, prefix, "byte_offset", self.header, false)?;
        if let Some(link) = &self.link {
            self.read_link(
                path.link(),
                path.record_index,
                scope_index,
                prefix,
                pointers,
                link,
            )?;
        }
        for (field, sources, values) in [
            (
                "occurrence_guid_offsets",
                self.occurrences.as_slice(),
                path.occurrence_guids()
                    .iter()
                    .map(|key| key.value.as_str())
                    .collect::<Vec<_>>(),
            ),
            (
                "identity_guid_offsets",
                self.identities.as_slice(),
                path.identity_guids()
                    .iter()
                    .map(|key| key.value.as_str())
                    .collect::<Vec<_>>(),
            ),
            (
                "paired_occurrence_key_offsets",
                self.pairs.as_slice(),
                path.paired_occurrence_keys()
                    .iter()
                    .map(|key| key.value.as_str())
                    .collect::<Vec<_>>(),
            ),
        ] {
            if sources.len() != values.len() {
                return Err(CodecError::malformed(
                    "assembly path source field count differs from content",
                ));
            }
            for (ordinal, (&source, value)) in sources.iter().zip(values).enumerate() {
                read_text(ctx, source, value)?;
                self.point(
                    pointers,
                    prefix,
                    &format!("{field}.{ordinal}"),
                    source,
                    true,
                )?;
            }
        }
        Ok(())
    }

    fn read_link(
        &self,
        link: &DesignAssemblyOperandPathLink,
        path_index: u32,
        scope_index: u32,
        prefix: &str,
        pointers: &mut BTreeMap<String, u64>,
        sources: &PathLinkSources<'_>,
    ) -> Result<(), CodecError> {
        for (field, mut source, expected) in [
            (
                "locator_reference_offset",
                sources.locator_reference,
                link.locator_record_index,
            ),
            (
                "locator_scope_reference_offset",
                sources.scope_reference,
                scope_index,
            ),
            (
                "wrapper_reference_offset",
                sources.wrapper_reference,
                link.wrapper_record_index,
            ),
            ("path_reference_offset", sources.path_reference, path_index),
        ] {
            let actual = source.u32_le().ok_or_else(|| {
                CodecError::malformed("assembly path source reference is truncated")
            })?;
            if actual != expected {
                return Err(CodecError::malformed(
                    "assembly path source reference differs from content",
                ));
            }
            self.point(pointers, prefix, &format!("link.{field}"), source, false)?;
        }
        for (field, source, tag, index) in [
            (
                "locator_byte_offset",
                sources.locator,
                link.locator_class_tag.as_str(),
                link.locator_record_index,
            ),
            (
                "wrapper_byte_offset",
                sources.wrapper,
                link.wrapper_class_tag.as_str(),
                link.wrapper_record_index,
            ),
        ] {
            read_link_header(source, tag, index)?;
            self.point(pointers, prefix, &format!("link.{field}"), source, false)?;
        }
        Ok(())
    }

    fn point(
        &self,
        pointers: &mut BTreeMap<String, u64>,
        prefix: &str,
        field: &str,
        source: View<'_>,
        after_count: bool,
    ) -> Result<(), CodecError> {
        let position = source
            .start()
            .checked_sub(self.bulk.start())
            .and_then(|position| position.checked_add(if after_count { 4 } else { 0 }))
            .ok_or_else(|| CodecError::malformed("assembly path source position overflows"))?;
        if pointers
            .insert(format!("{prefix}.{field}"), u64_from_index(position))
            .is_some()
        {
            return Err(CodecError::malformed(
                "assembly path source field is duplicated",
            ));
        }
        Ok(())
    }
}

fn field(source: View<'_>, relative: usize, len: usize) -> Option<View<'_>> {
    let start = source.start().checked_add(relative)?;
    source.child(start, start.checked_add(len)?)
}

fn header(source: View<'_>, relative: usize, index_bytes: usize) -> Option<View<'_>> {
    let len = usize::try_from(View::u32_le_at(source.window(), relative)?).ok()?;
    field(
        source,
        relative,
        4usize.checked_add(len)?.checked_add(index_bytes)?,
    )
}

fn text(source: View<'_>, count_at: usize, end: usize) -> Option<View<'_>> {
    field(source, count_at, end.checked_sub(count_at)?)
}

fn read_header(mut source: View<'_>, tag: &str, index: u32) -> Result<(), CodecError> {
    let len = source
        .u32_le()
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| CodecError::malformed("assembly path class length is truncated"))?;
    let actual = source
        .take(len)
        .ok_or_else(|| CodecError::malformed("assembly path class is truncated"))?;
    if actual != tag.as_bytes() || source.u64_le() != Some(u64::from(index)) || !source.is_empty() {
        return Err(CodecError::malformed(
            "assembly path header differs from content",
        ));
    }
    Ok(())
}

fn read_link_header(mut source: View<'_>, tag: &str, index: u32) -> Result<(), CodecError> {
    let len = source
        .u32_le()
        .and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| CodecError::malformed("assembly link class length is truncated"))?;
    let actual = source
        .take(len)
        .ok_or_else(|| CodecError::malformed("assembly link class is truncated"))?;
    if actual != tag.as_bytes() || source.u32_le() != Some(index) || !source.is_empty() {
        return Err(CodecError::malformed(
            "assembly link header differs from content",
        ));
    }
    Ok(())
}

fn read_text(
    ctx: &DecodeContext<'_>,
    mut source: View<'_>,
    expected: &str,
) -> Result<(), CodecError> {
    let count = source
        .u32_le()
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| CodecError::malformed("assembly path key count is truncated"))?;
    let bytes = source
        .read_remaining()
        .ok_or_else(|| CodecError::malformed("assembly path key is truncated"))?;
    if count.checked_mul(2) != Some(bytes.len()) {
        return Err(CodecError::malformed(
            "assembly path key count differs from its source extent",
        ));
    }
    let actual = ctx.utf16le_text(bytes, count, false, "read assembly operand path key")?;
    if actual != expected {
        return Err(CodecError::malformed(
            "assembly path key differs from content",
        ));
    }
    Ok(())
}
