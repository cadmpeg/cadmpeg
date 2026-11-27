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
    pub(crate) payloads: [NativeBytes; 3],
    pub(crate) pairs: Vec<Pair>,
    pub(crate) revisions: Vec<Revision>,
    pub(crate) external: Vec<External>,
}

impl Tables {
    /// The tables of metadata sections 7, 8, and 10, or the index in
    /// `meta.tables.sections` of the section that does not read and its error.
    pub(super) fn read(
        ctx: &DecodeContext<'_>,
        token: &IdentityKey,
        meta: &SegmentMeta<'_>,
    ) -> Result<Self, (usize, CodecError)> {
        let sections = SECTIONS.map(|index| &meta.tables.sections[index]);
        let counts = sections.map(|section| section.discriminator);
        let payloads = sections.map(|section| section.payload);
        let (pairs, revisions, external) =
            parse(ctx, counts, payloads).map_err(|(at, error)| (SECTIONS[at], error))?;
        let mut retained = [
            NativeBytes::default(),
            NativeBytes::default(),
            NativeBytes::default(),
        ];
        for ((target, payload), index) in retained.iter_mut().zip(payloads).zip(SECTIONS) {
            let mut bytes = ctx
                .alloc_filled(payload.remaining(), 0_u8, "retain drawing reference table")
                .map_err(|error| (index, error))?;
            bytes.copy_from_slice(payload.unread());
            *target = bytes.into();
        }
        Ok(Self {
            id: format!("inventor:drawing:reference-tables#{token}"),
            segment_token: token.clone(),
            segment_id: meta.segment_id.into(),
            counts,
            payloads: retained,
            pairs,
            revisions,
            external,
        })
    }

    pub(crate) fn valid(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if self.id != format!("inventor:drawing:reference-tables#{}", self.segment_token) {
            return Ok(false);
        }
        match parse(
            ctx,
            self.counts,
            self.payloads
                .each_ref()
                .map(|bytes| View::over_retained(bytes)),
        ) {
            Ok((pairs, revisions, external)) => {
                Ok(pairs == self.pairs && revisions == self.revisions && external == self.external)
            }
            Err((_, error @ CodecError::ResourceLimit(_))) => Err(error),
            Err(_) => Ok(false),
        }
    }

    pub(super) fn target(&self, reference: PmDcReference) -> Option<(&Pair, &External)> {
        if reference.qualified() {
            return None;
        }
        let index = usize::try_from(reference.index().checked_sub(1)?).ok()?;
        let external = self.external.get(index)?;
        if external.revision_word > u32::from(u16::MAX) {
            return None;
        }
        let revision = self
            .revisions
            .get(usize::try_from(external.revision_word).ok()?)?;
        Some((
            self.pairs.get(usize::from(revision.segment_pair_index))?,
            external,
        ))
    }
}

type Parsed = (Vec<Pair>, Vec<Revision>, Vec<External>);

/// The indices in the section list of the metadata sections 7, 8, and 10,
/// which store the segment-pair, revision, and external-reference tables.
const SECTIONS: [usize; 3] = [6, 7, 9];

fn read_entries<T: Clone>(
    ctx: &DecodeContext<'_>,
    count: u32,
    mut payload: View<'_>,
    width: usize,
    initial: T,
    mut read: impl FnMut(&mut View<'_>) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let bounded = payload.counted(u64::from(count), width).ok_or_else(|| {
        CodecError::malformed("drawing reference table count exceeds its payload")
    })?;
    if bounded.get().checked_mul(width) != Some(payload.remaining()) {
        return Err(CodecError::malformed(
            "drawing reference table has a wrong entry width",
        ));
    }
    let mut entries =
        ctx.alloc_filled(bounded.get(), initial, "retain drawing reference entries")?;
    for target in &mut entries {
        *target = read(&mut payload)?;
    }
    Ok(entries)
}

/// The three tables, or the index in `payloads` of the table that does not
/// read and its error.
fn parse(
    ctx: &DecodeContext<'_>,
    counts: [u32; 3],
    payloads: [View<'_>; 3],
) -> Result<Parsed, (usize, CodecError)> {
    let pairs = read_entries(
        ctx,
        counts[0],
        payloads[0],
        pair::LEN,
        Pair::default(),
        |view| {
            Ok(Pair {
                segment_id: reader::array(view, "drawing reference segment")?.into(),
                revision_id: reader::array(view, "drawing reference revision")?.into(),
            })
        },
    )
    .map_err(|error| (0, error))?;
    let revisions = read_entries(
        ctx,
        counts[1],
        payloads[1],
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
    .map_err(|error| (1, error))?;
    let external = read_entries(
        ctx,
        counts[2],
        payloads[2],
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
    .map_err(|error| (2, error))?;
    Ok((pairs, revisions, external))
}

/// The id of the given-up record that `record` fails its links for: for a
/// sheet, the first local view it names that `refused` holds by its segment
/// token and ordinal; for a cached view, the reference tables of its own
/// segment that `failed_tables` holds by segment token.
pub(super) fn refused_link<'r>(
    record: &super::Record,
    refused: &'r std::collections::BTreeMap<(String, u32), String>,
    failed_tables: &'r std::collections::BTreeMap<String, String>,
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
        super::records::Fields::CachedView { .. } => failed_tables.get(token).map(String::as_str),
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
    let Some((pair, external)) = source.target(view) else {
        return false;
    };
    if external.key_extension != 0 {
        return false;
    }
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
