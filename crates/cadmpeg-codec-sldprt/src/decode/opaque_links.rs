//! Opaque geometry link staging and transfer to retained source records.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::unknown::UnknownRecord;
use std::collections::{btree_map::Entry, BTreeMap};

struct OpaqueLinkGroup<'ctx> {
    links: Vec<String>,
    storage: ScopedReservation<'ctx>,
}

/// Holds only scoped map and vector storage. Link text belongs to the output.
pub(super) struct OpaqueGeometryLinks<'source, 'ctx> {
    ctx: &'ctx DecodeContext<'ctx>,
    groups: BTreeMap<&'source str, OpaqueLinkGroup<'ctx>>,
    map_storage: ScopedReservation<'ctx>,
}

impl<'source, 'ctx> OpaqueGeometryLinks<'source, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            groups: BTreeMap::new(),
            map_storage: ctx.reserve_scoped(0, "SLDPRT opaque geometry index workspace")?,
        })
    }

    pub(super) fn push(&mut self, record: &'source str, entity: &str) -> Result<(), CodecError> {
        const RECORD: &str = "index SLDPRT opaque geometry record";
        const LINK: &str = "index SLDPRT opaque geometry link";
        let entry = self.map_storage.with_storage(|| {
            self.ctx.entry_btree_map(&mut self.groups, record, RECORD)
        })?;
        let group = match entry {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(OpaqueLinkGroup {
                links: Vec::new(),
                storage: self.ctx.reserve_scoped(0, LINK)?,
            }),
        };
        let link = self.ctx.copy_retained_text(entity, "retain SLDPRT opaque geometry link")?;
        group.storage.with_storage(|| self.ctx.push_vec(&mut group.links, link, LINK))
    }

    /// Consume all staging before source-image preservation. A missing target
    /// is malformed; successful transfer keeps entity text in its source record.
    pub(super) fn attach(self, unknowns: &mut [UnknownRecord]) -> Result<(), CodecError> {
        const OPERATION: &str = "append SLDPRT opaque geometry links";
        if self.groups.is_empty() {
            return Ok(());
        }
        let (unknown_positions, unknown_position_storage) = self.ctx.collect_scoped_string_map(
            unknowns.len(),
            unknowns.iter().enumerate().rev()
                .map(|(position, record)| (record.id().as_str(), position)),
            "index SLDPRT opaque geometry records",
        )?;
        let (mut linked_positions, mut position_storage) = self.ctx.scoped_vector_storage(
            0, "SLDPRT opaque geometry link positions",
        )?;
        for (record_id, group) in self.ctx.admit_iter(self.groups, OPERATION)? {
            let Some(&position) = self.ctx.get_hash_map(&unknown_positions, record_id, OPERATION)?
            else {
                return Err(CodecError::Malformed(self.ctx.format_retained(
                    format_args!("opaque geometry record {record_id} was not retained"),
                    "format SLDPRT opaque geometry error",
                )?));
            };
            position_storage.with_storage(|| {
                self.ctx.push_vec(&mut linked_positions, (position, group),
                    "SLDPRT opaque geometry link positions")
            })?;
        }
        drop((unknown_positions, unknown_position_storage, self.map_storage));
        for (position, group) in self.ctx.admit_iter(linked_positions, OPERATION)? {
            self.ctx.extend_vec(unknowns[position].links_mut(), group.links, OPERATION)?;
            drop(group.storage);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
