// SPDX-License-Identifier: Apache-2.0
//! Lazily built indexes used by FreeCAD design transfer.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FeatureId;

use crate::native::{ObjectRecord, PropertyRecord};

/// Object lookup whose storage is admitted only when a caller needs a lookup.
pub(super) struct ObjectIndex<'ctx, 'arena, 'objects> {
    ctx: &'ctx DecodeContext<'arena>,
    objects: &'objects [ObjectRecord],
    entries: RefCell<
        Option<(
            BTreeMap<&'objects str, &'objects ObjectRecord>,
            ScopedReservation<'ctx>,
        )>,
    >,
}

impl<'ctx, 'arena, 'objects> ObjectIndex<'ctx, 'arena, 'objects> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'arena>,
        objects: &'objects [ObjectRecord],
    ) -> Result<Self, CodecError> {
        ctx.charge_work(0, "fcstd design object index initialization")?;
        Ok(Self {
            ctx,
            objects,
            entries: RefCell::new(None),
        })
    }

    pub(super) fn get(
        &self,
        key: &str,
        operation: &'static str,
    ) -> Result<Option<&'objects ObjectRecord>, CodecError> {
        let mut cache = self.entries.borrow_mut();
        let (entries, _storage) = match &mut *cache {
            Some(entries) => entries,
            slot @ None => slot.insert(self.ctx.collect_scoped_btree_map(
                self.objects
                    .iter()
                    .rev()
                    .map(|object| (object.id().as_str(), object)),
                "fcstd design object index",
            )?),
        };
        self.ctx
            .get_btree_map(entries, key, operation)
            .map(|object| object.copied())
    }
}

/// Body predecessor lookup whose full scan runs only for an implicit seed query.
pub(super) struct BodyPredecessors<'ctx, 'arena, 'data> {
    ctx: &'ctx DecodeContext<'arena>,
    objects: &'data [ObjectRecord],
    features: &'data HashMap<&'data str, FeatureId>,
    properties_by_owner: &'data BTreeMap<&'data str, Vec<&'data PropertyRecord>>,
    entries: RefCell<
        Option<(
            BTreeMap<&'data str, &'data FeatureId>,
            ScopedReservation<'ctx>,
        )>,
    >,
}

impl<'ctx, 'arena, 'data> BodyPredecessors<'ctx, 'arena, 'data> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'arena>,
        objects: &'data [ObjectRecord],
        features: &'data HashMap<&'data str, FeatureId>,
        properties_by_owner: &'data BTreeMap<&'data str, Vec<&'data PropertyRecord>>,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(0, "fcstd body predecessor index initialization")?;
        Ok(Self {
            ctx,
            objects,
            features,
            properties_by_owner,
            entries: RefCell::new(None),
        })
    }

    fn build(
        &self,
    ) -> Result<
        (
            BTreeMap<&'data str, &'data FeatureId>,
            ScopedReservation<'ctx>,
        ),
        CodecError,
    > {
        self.ctx
            .with_scoped_storage("fcstd body predecessor storage", || {
                let mut predecessors = BTreeMap::new();
                let mut objects = self.objects.iter();
                while objects.len() > 0 {
                    let Some(object) = self
                        .ctx
                        .next_charged(&mut objects, "fcstd body predecessor objects")?
                    else {
                        break;
                    };
                    let Some(owned) = self.ctx.get_btree_map(
                        self.properties_by_owner,
                        object.id().as_str(),
                        "fcstd body predecessor properties",
                    )?
                    else {
                        continue;
                    };
                    let Some(members) = super::body_membership_property(self.ctx, owned)? else {
                        continue;
                    };
                    let mut previous = None;
                    let (mut seen_storage, mut seen);
                    (seen, seen_storage) = self.ctx.with_scoped_storage(
                        "fcstd body predecessor seen storage",
                        || Ok::<_, CodecError>(BTreeSet::new()),
                    )?;
                    let mut links = members.links().iter();
                    while links.len() > 0 {
                        let Some(link) = self
                            .ctx
                            .next_charged(&mut links, "fcstd body predecessor members")?
                        else {
                            break;
                        };
                        let Some(member) = link
                            .as_ref()
                            .and_then(crate::native::LinkTarget::object)
                        else {
                            continue;
                        };
                        if seen_storage.with_storage(|| {
                            self.ctx.insert_btree_set(
                                &mut seen,
                                member,
                                "fcstd body predecessor first member",
                            )
                        })? {
                            if let Some(previous) = previous {
                                if !self.ctx.contains_key_btree_map(
                                    &predecessors,
                                    member,
                                    "fcstd body predecessor first body",
                                )? {
                                    self.ctx.insert_btree_map(
                                        &mut predecessors,
                                        member,
                                        previous,
                                        "fcstd body predecessor index",
                                    )?;
                                }
                            }
                        }
                        if let Some(feature) = self.ctx.get_hash_map(
                            self.features,
                            member,
                            "fcstd body predecessor feature",
                        )? {
                            previous = Some(feature);
                        }
                    }
                }
                Ok::<_, CodecError>(predecessors)
            })
    }

    pub(super) fn get(
        &self,
        key: &str,
        operation: &'static str,
    ) -> Result<Option<&'data FeatureId>, CodecError> {
        let mut cache = self.entries.borrow_mut();
        let (entries, _storage) = match &mut *cache {
            Some(entries) => entries,
            slot @ None => slot.insert(self.build()?),
        };
        self.ctx
            .get_btree_map(entries, key, operation)
            .map(|feature| feature.copied())
    }
}

#[cfg(test)]
mod tests;
