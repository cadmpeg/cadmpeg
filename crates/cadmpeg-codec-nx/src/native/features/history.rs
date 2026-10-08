//! Feature-history sections and their operation records, decoded once.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::container::entry_ref::EntryRef;
use crate::container::Container;
use crate::native::segments::{segment_om_links, SegmentOmLink};
use crate::om::operation_record::OperationRecord;
use crate::om::UnlabeledOperationRecord;

/// The canonical feature-history sections of one container.
///
/// Extractors read this shared view instead of re-deriving links, matching
/// sections and re-parsing operation headers for every record family.
pub(in crate::native) struct FeatureHistory<'c, 'a, 's> {
    container: &'c Container<'a>,
    sections: Vec<FeatureHistorySection<'c>>,
    _storage: ScopedReservation<'s>,
}

/// One linked feature-history section.
pub(in crate::native) struct FeatureHistorySection<'c> {
    /// The segment-index link that points at the section.
    pub(in crate::native) link: SegmentOmLink,
    /// Position of the link among canonical feature-history links.
    pub(in crate::native) ordinal: usize,
    /// Zero-padded `ordinal` used in record identities.
    pub(in crate::native) key: String,
    /// The directory entry that frames the section.
    pub(in crate::native) entry: EntryRef<'c>,
    pub(in crate::native) section: crate::om::Section<'c>,
    /// File offset of the directory entry that frames the section.
    pub(in crate::native) entry_offset: u64,
    /// Labelled operation records with their header ordinals.
    pub(in crate::native) records: Vec<(usize, OperationRecord<'c>)>,
}

impl<'c, 'a, 's> FeatureHistory<'c, 'a, 's> {
    /// Match canonical feature-history links to framed sections.
    ///
    /// A link whose offset names no section is skipped; its ordinal still
    /// counts toward later section keys. When two sections start at the same
    /// file offset, the first in container order is used.
    pub(in crate::native) fn new(
        ctx: &'s DecodeContext<'_>,
        container: &'c Container<'a>,
    ) -> Result<Self, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "NX feature history")?;
        let links = storage.with_storage(|| {
            super::canonical_feature_history_links(ctx, segment_om_links(ctx, container)?)
        })?;
        let framed = container.om_sections(ctx)?;
        let mut index_storage = ctx.reserve_scoped(0, "NX feature history section index")?;
        let mut starts = Vec::new();
        let mut slots = Vec::new();
        for (index, (entry, section)) in ctx
            .admit_iter(framed, "index NX feature history sections")?
            .enumerate()
        {
            let entry_offset = entry.file_span().map_or(0, |(offset, _)| offset);
            if let Some(start) = entry_offset.checked_add(u64_from_index(section.offset)) {
                ctx.push_scoped_vec(
                    &mut index_storage,
                    &mut starts,
                    (start, index),
                    "NX feature history section starts",
                )?;
            }
            ctx.push_scoped_vec(
                &mut index_storage,
                &mut slots,
                Some((entry, entry_offset, section)),
                "NX feature history section slots",
            )?;
        }
        ctx.stable_sort_by_key(
            &mut starts,
            |&(start, _)| start,
            Ord::cmp,
            "sort NX feature history sections",
        )?;
        let mut sections = Vec::new();
        for (ordinal, link) in ctx
            .admit_iter(links, "visit NX feature history links")?
            .enumerate()
        {
            let target = link.location.section_offset();
            let at = ctx.partition_point(
                &starts,
                |&(start, _)| Ok(start < target),
                "match NX feature history section",
            )?;
            let Some(&(start, index)) = starts.get(at) else {
                continue;
            };
            if start != target {
                continue;
            }
            let Some((entry, entry_offset, section)) = slots.get_mut(index).and_then(Option::take)
            else {
                continue;
            };
            let key = ctx.format_scoped_text(
                &mut storage,
                format_args!("{ordinal:010}"),
                "NX feature history section key",
            )?;
            let records =
                storage.with_storage(|| section.operation_records_with_label_ordinals(ctx))?;
            ctx.push_scoped_vec(
                &mut storage,
                &mut sections,
                FeatureHistorySection {
                    link,
                    ordinal,
                    key,
                    entry,
                    section,
                    entry_offset,
                    records,
                },
                "NX feature history sections",
            )?;
        }
        Ok(Self {
            container,
            sections,
            _storage: storage,
        })
    }

    pub(in crate::native) fn container(&self) -> &'c Container<'a> {
        self.container
    }

    pub(in crate::native) fn sections(&self) -> &[FeatureHistorySection<'c>] {
        &self.sections
    }
}

impl<'c> FeatureHistorySection<'c> {
    /// Operation records without a complete label frame, held under the
    /// returned scoped reservation.
    pub(in crate::native) fn unlabeled_records<'ctx>(
        &self,
        ctx: &'ctx DecodeContext<'_>,
    ) -> Result<
        (
            Vec<(usize, UnlabeledOperationRecord<'c>)>,
            ScopedReservation<'ctx>,
        ),
        CodecError,
    > {
        let mut storage = ctx.reserve_scoped(0, "NX unlabeled operation records")?;
        let records =
            storage.with_storage(|| self.section.unlabeled_operation_records_with_ordinals(ctx))?;
        Ok((records, storage))
    }
}
