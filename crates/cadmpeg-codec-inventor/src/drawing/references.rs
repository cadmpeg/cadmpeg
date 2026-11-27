// SPDX-License-Identifier: Apache-2.0
//! Drawing references through the segment, revision, and external-entry tables.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::native::bytes::NativeBytes;
use serde::{Deserialize, Serialize};

use crate::layout::{
    meta_external_reference as external, meta_external_revision as revision,
    meta_segment_pair as pair,
};
use crate::pmdc::PmDcReference;
use crate::reader;
use crate::records::{reference_section_count, MetaSection, REFERENCE_SECTIONS};
use crate::rse::SegmentMeta;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pair {
    pub(crate) segment_id: NativeBytes<[u8; 16]>,
    revision_id: NativeBytes<[u8; 16]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Revision {
    #[serde(rename = "revision_id")]
    identifier: NativeBytes<[u8; 16]>,
    segment_pair_index: u16,
    reference_count: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct External {
    revision_word: u32,
    pub(crate) key_extension: u16,
    pub(crate) header_id: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Tables {
    pub(crate) id: String,
    #[serde(with = "super::token_wire")]
    pub(crate) segment_token: IdentityKey,
    pub(crate) segment_id: NativeBytes<[u8; 16]>,
    pub(crate) counts: [u32; 3],
    pub(crate) payloads: [Option<NativeBytes>; 3],
    pub(crate) pairs: Vec<Pair>,
    pub(crate) revisions: Vec<Revision>,
    pub(crate) external: Vec<External>,
}

impl Tables {
    /// Reads each reference table at its own metadata section boundary.
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        token: &IdentityKey,
        meta: &SegmentMeta<'_>,
        mut give_up: impl FnMut(&MetaSection<'_>, CodecError) -> Result<(), CodecError>,
    ) -> Result<Self, CodecError> {
        let [pairs, revisions, external] =
            REFERENCE_SECTIONS.map(|(index, _)| &meta.tables.sections[index]);
        let (pair_rows, pair_payload) = read_part(ctx, pairs, read_pairs, &mut give_up)?;
        let (revision_rows, revision_payload) =
            read_part(ctx, revisions, read_revisions, &mut give_up)?;
        let (external_rows, external_payload) =
            read_part(ctx, external, read_external, &mut give_up)?;
        Ok(Self {
            id: format!("inventor:drawing:reference-tables#{token}"),
            segment_token: token.clone(),
            segment_id: meta.segment_id.into(),
            counts: [
                pairs.discriminator,
                revisions.discriminator,
                external.discriminator,
            ],
            payloads: [pair_payload, revision_payload, external_payload],
            pairs: pair_rows,
            revisions: revision_rows,
            external: external_rows,
        })
    }

    pub(crate) fn valid(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if self.id != format!("inventor:drawing:reference-tables#{}", self.segment_token) {
            return Ok(false);
        }
        let [pairs, revisions, external] = self.payloads.each_ref();
        Ok(
            table_matches(ctx, self.counts[0], pairs.as_ref(), &self.pairs, read_pairs)?
                && table_matches(
                    ctx,
                    self.counts[1],
                    revisions.as_ref(),
                    &self.revisions,
                    read_revisions,
                )?
                && table_matches(
                    ctx,
                    self.counts[2],
                    external.as_ref(),
                    &self.external,
                    read_external,
                )?,
        )
    }

    /// The section needed by this reference, or an invalid stored join.
    fn resolve(&self, reference: PmDcReference) -> Result<(&Pair, &External), LinkFailure> {
        if reference.qualified() {
            return Err(LinkFailure::Invalid);
        }
        let index = reference
            .index()
            .checked_sub(1)
            .ok_or(LinkFailure::Invalid)?;
        let external = self.entry(2, &self.external, index)?;
        if external.revision_word > u32::from(u16::MAX) || external.key_extension != 0 {
            return Err(LinkFailure::Invalid);
        }
        let revision = self.entry(1, &self.revisions, external.revision_word)?;
        let pair = self.entry(0, &self.pairs, u32::from(revision.segment_pair_index))?;
        Ok((pair, external))
    }

    fn entry<'r, T>(
        &self,
        section: usize,
        rows: &'r [T],
        index: u32,
    ) -> Result<&'r T, LinkFailure> {
        if index >= self.counts[section] {
            return Err(LinkFailure::Invalid);
        }
        if self.payloads[section].is_none() {
            return Err(LinkFailure::Unread([7, 8, 10][section]));
        }
        usize::try_from(index)
            .ok()
            .and_then(|at| rows.get(at))
            .ok_or(LinkFailure::Invalid)
    }
}

#[derive(Debug)]
enum LinkFailure {
    Unread(u8),
    Invalid,
}

/// Open, parse, and retain the payload of one independently framed table.
fn read_part<T>(
    ctx: &DecodeContext<'_>,
    section: &MetaSection<'_>,
    read: impl FnOnce(&DecodeContext<'_>, u32, View<'_>) -> Result<Vec<T>, CodecError>,
    give_up: &mut impl FnMut(&MetaSection<'_>, CodecError) -> Result<(), CodecError>,
) -> Result<(Vec<T>, Option<NativeBytes>), CodecError> {
    let decode = || {
        let rows = read(ctx, section.discriminator, section.payload)?;
        let mut bytes = ctx.alloc_filled(
            section.payload.remaining(),
            0_u8,
            "retain drawing reference table",
        )?;
        bytes.copy_from_slice(section.payload.unread());
        Ok((rows, Some(bytes.into())))
    };
    match ctx.run_part(decode)? {
        Ok(table) => Ok(table),
        Err(error) => {
            give_up(section, error)?;
            Ok((Vec::new(), None))
        }
    }
}

fn table_matches<T: PartialEq>(
    ctx: &DecodeContext<'_>,
    count: u32,
    payload: Option<&NativeBytes>,
    rows: &[T],
    read: impl FnOnce(&DecodeContext<'_>, u32, View<'_>) -> Result<Vec<T>, CodecError>,
) -> Result<bool, CodecError> {
    let Some(payload) = payload else {
        return Ok(rows.is_empty());
    };
    match read(ctx, count, View::over_retained(payload)) {
        Ok(parsed) => Ok(parsed == rows),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(false),
    }
}

fn read_entries<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: u32,
    mut payload: View<'_>,
    width: usize,
    initial: T,
    mut read: impl FnMut(&mut View<'_>) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let count = reference_section_count(count, payload, width)?;
    let mut entries = ctx.alloc_filled(count, initial, "retain drawing reference entries")?;
    for target in &mut entries {
        *target = read(&mut payload)?;
    }
    Ok(entries)
}

fn read_pairs(
    ctx: &DecodeContext<'_>,
    count: u32,
    payload: View<'_>,
) -> Result<Vec<Pair>, CodecError> {
    read_entries(ctx, count, payload, pair::LEN, Pair::default(), |view| {
        Ok(Pair {
            segment_id: reader::array(view, "drawing reference segment")?.into(),
            revision_id: reader::array(view, "drawing reference revision")?.into(),
        })
    })
}

fn read_revisions(
    ctx: &DecodeContext<'_>,
    count: u32,
    payload: View<'_>,
) -> Result<Vec<Revision>, CodecError> {
    read_entries(
        ctx,
        count,
        payload,
        revision::LEN,
        Revision::default(),
        |view| {
            Ok(Revision {
                identifier: reader::array(view, "drawing revision identifier")?.into(),
                segment_pair_index: reader::u16(view, "drawing revision segment index")?,
                reference_count: reader::u16(view, "drawing revision reference count")?,
            })
        },
    )
}

fn read_external(
    ctx: &DecodeContext<'_>,
    count: u32,
    payload: View<'_>,
) -> Result<Vec<External>, CodecError> {
    read_entries(
        ctx,
        count,
        payload,
        external::LEN,
        External::default(),
        |view| {
            Ok(External {
                revision_word: reader::u32(view, "drawing external revision index")?,
                key_extension: reader::u16(view, "drawing external key extension")?,
                header_id: reader::u16(view, "drawing external header identifier")?,
            })
        },
    )
}

/// The id of the given-up record that `record` fails its links for: for a
/// sheet, the first local view it names that `refused` holds by its segment
/// token and ordinal; for a cached view, the reference tables of its own
/// segment whose required section gave up.
pub(super) fn refused_link<'r>(
    record: &super::Record,
    refused: &'r std::collections::BTreeMap<(String, u32), String>,
    tables: &[Tables],
    failed_sections: &'r std::collections::BTreeMap<(String, u8), String>,
) -> Option<&'r str> {
    let token = record.segment_token.as_str();
    match &record.fields {
        super::records::Fields::Sheet {
            views,
            auxiliary_lists,
            ..
        } => views
            .references()
            .iter()
            .chain(auxiliary_lists[2].references())
            .filter(|reference| reference.qualified())
            .filter_map(|reference| reference.record_ordinal())
            .find_map(|ordinal| refused.get(&(token.to_owned(), ordinal)))
            .map(String::as_str),
        super::records::Fields::CachedView { view, .. } => {
            let table = tables
                .iter()
                .find(|table| table.segment_token.as_str() == token)?;
            let Err(LinkFailure::Unread(section)) = table.resolve(*view) else {
                return None;
            };
            failed_sections
                .get(&(token.to_owned(), section))
                .map(String::as_str)
        }
        super::records::Fields::View { .. } | super::records::Fields::ModelReference { .. } => None,
    }
}

pub(super) fn record_links_are_valid(
    record: &super::Record,
    tables: &[Tables],
    records: &[super::Record],
) -> bool {
    if let super::records::Fields::Sheet {
        views,
        auxiliary_lists,
        ..
    } = &record.fields
    {
        let local_view = |reference: &PmDcReference, type_id: &str| {
            reference.qualified()
                && reference.record_ordinal().is_some_and(|ordinal| {
                    records
                        .iter()
                        .filter(|candidate| {
                            candidate.segment_token == record.segment_token
                                && candidate.ordinal == ordinal
                                && candidate.type_id.as_str() == type_id
                        })
                        .count()
                        == 1
                })
        };
        return views
            .references()
            .iter()
            .all(|reference| local_view(reference, "5942cb22d111c557600037bd861c3cb0"))
            && auxiliary_lists[2]
                .references()
                .iter()
                .all(|reference| local_view(reference, "c1965962d4114e8e600095873fa6fbb0"));
    }
    let super::records::Fields::CachedView { view, .. } = record.fields else {
        return true;
    };
    let mut sources = tables
        .iter()
        .filter(|table| table.segment_token == record.segment_token);
    let Some(source) = sources.next() else {
        return false;
    };
    if sources.next().is_some() {
        return false;
    }
    let Ok((pair, external)) = source.resolve(view) else {
        return false;
    };
    let mut targets = tables
        .iter()
        .filter(|table| table.segment_id == pair.segment_id);
    let Some(target) = targets.next() else {
        return false;
    };
    if targets.next().is_some() {
        return false;
    }
    records
        .iter()
        .filter(|candidate| {
            candidate.segment_token == target.segment_token
                && candidate.definition_header() == Some((0, external.header_id))
        })
        .count()
        == 1
}

#[cfg(test)]
mod tests;
