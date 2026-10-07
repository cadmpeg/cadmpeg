//! Marker lookups shared by the geometry queries for one sketch.

use super::sketch_marker_prefix_at;
use super::arc_centers::ArcCenterIndex;
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

struct OwnerGeometry<'a, 'ctx> {
    all: Vec<&'a SketchInputEntity>,
    all_roster: std::cell::OnceCell<Vec<&'a SketchInputEntity>>,
    located_roster: std::cell::OnceCell<Vec<&'a SketchInputEntity>>,
    geometry_roster: std::cell::OnceCell<Vec<&'a SketchInputEntity>>,
    markers: Vec<&'a SketchInputEntity>,
    native_arc_centers: std::cell::OnceCell<ArcCenterIndex<'a, 'ctx>>,
    profile_arc_centers: std::cell::OnceCell<ArcCenterIndex<'a, 'ctx>>,
    points: Vec<&'a SketchInputEntity>,
}

/// Located markers by owner and object index, with point positions in payload order.
pub(in crate::resolved_features) struct MarkerGeometryIndex<'a, 'ctx> {
    ctx: &'ctx DecodeContext<'ctx>,
    roster_storage: std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    owners: HashMap<Option<&'a str>, OwnerGeometry<'a, 'ctx>>,
    objects: HashMap<(Option<&'a str>, Option<u32>), Vec<&'a SketchInputEntity>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> MarkerGeometryIndex<'a, 'ctx> {
    pub(in crate::resolved_features) fn new(
        ctx: &'ctx DecodeContext<'ctx>,
        markers: &[&'a SketchInputEntity],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT marker geometry";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut owners = HashMap::new();
        let mut objects = HashMap::new();
        let mut order_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut owner_order = Vec::new();
        for &marker in ctx.admit_iter(markers, OPERATION)? {
            let owner = marker.feature_ref.as_deref();
            storage.with_storage(|| {
                if !ctx.contains_key_hash_map(&owners, &owner, OPERATION)? {
                    ctx.insert_hash_map(&mut owners, owner, OwnerGeometry {
                        points: Vec::new(), markers: Vec::new(), all: Vec::new(),
                        all_roster: std::cell::OnceCell::new(), located_roster: std::cell::OnceCell::new(), geometry_roster: std::cell::OnceCell::new(),
                        native_arc_centers: std::cell::OnceCell::new(),
                        profile_arc_centers: std::cell::OnceCell::new(),
                    }, OPERATION)?;
                    order_storage.with_storage(|| ctx.push_vec(&mut owner_order, owner, OPERATION))?;
                }
                let Some(group) = ctx.get_mut_hash_map(&mut owners, &owner, OPERATION)? else { return Ok(()); };
                ctx.push_vec(&mut group.all, marker, OPERATION)?;
                if marker.coordinates_m.is_none() { return Ok(()); }
                ctx.push_vec(&mut group.markers, marker, OPERATION)?;
                if matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint) {
                    ctx.push_vec(&mut group.points, marker, OPERATION)?;
                }
                ctx.push_hash_group(&mut objects, (owner, marker.object_index()), marker, OPERATION, OPERATION)?;
                Ok::<(), CodecError>(())
            })?;
        }
        for owner in ctx.admit_iter(owner_order, OPERATION)? {
            if let Some(group) = ctx.get_mut_hash_map(&mut owners, &owner, OPERATION)? {
                ctx.sort_unstable_by_key(&mut group.points, |marker| marker.offset(), Ord::cmp, OPERATION)?;
            }
        }
        Ok(Self { ctx, owners, objects, _storage: storage, roster_storage: std::cell::RefCell::new(ctx.reserve_scoped(0, "index SLDPRT owner marker rosters")?) })
    }

    pub(in crate::resolved_features) fn endpoint(
        &self,
        ctx: &DecodeContext<'_>,
        curve: &SketchInputEntity,
        index: u32,
        broad: bool,
    ) -> Result<Option<&'a SketchInputEntity>, CodecError> {
        const OPERATION: &str = "lookup SLDPRT inline line endpoint";
        let markers = ctx.get_hash_map(&self.objects, &(curve.feature_ref.as_deref(), Some(index)), OPERATION)?
            .map_or(&[][..], Vec::as_slice);
        let mut selected = None;
        let ambiguous = ctx.any_by(markers, |&marker| {
            if broad {
                if !matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint | SketchInputKind::LineOrCircle | SketchInputKind::Arc) { return Ok(false); }
                if ctx.equal(marker.id(), curve.id(), OPERATION)? { return Ok(false); }
            } else if !matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint) { return Ok(false); }
            if selected.is_some() { return Ok(true); }
            selected = Some(marker);
            Ok(false)
        }, OPERATION)?;
        Ok(selected.filter(|_| !ambiguous))
    }

    pub(in crate::resolved_features) fn object_markers<'query>(
        &'query self, ctx: &DecodeContext<'_>, curve: &'query SketchInputEntity, index: Option<u32>,
    ) -> Result<&'query [&'a SketchInputEntity], CodecError> {
        Ok(ctx.get_hash_map(&self.objects, &(curve.feature_ref.as_deref(), index), "lookup SLDPRT curve object markers")?
            .map_or(&[][..], Vec::as_slice))
    }

    pub(in crate::resolved_features) fn arc_centers<'query>(
        &'query self, curve: &'query SketchInputEntity, profile: bool, tolerance: f64,
    ) -> Result<Option<&'query ArcCenterIndex<'a, 'ctx>>, CodecError> {
        const OPERATION: &str = "index SLDPRT arc center positions";
        let Some(owner) = self.ctx.get_hash_map(&self.owners, &curve.feature_ref.as_deref(), OPERATION)? else { return Ok(None); };
        let cache = if profile { &owner.profile_arc_centers } else { &owner.native_arc_centers };
        if let Some(index) = cache.get() { return Ok(Some(index)); }
        let index = ArcCenterIndex::from_markers(self.ctx, &owner.markers,
            if profile { 1000.0 } else { 1.0 }, !profile, tolerance)?;
        Ok(Some(cache.get_or_init(|| index)))
    }


}

/// Every marker prefix in payload order, including prefixes inside incomplete records.
pub(in crate::resolved_features) struct MarkerPrefixIndex<'payload, 'ctx> {
    payload: &'payload [u8],
    offsets: std::cell::OnceCell<Vec<usize>>,
    storage: std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'payload, 'ctx> MarkerPrefixIndex<'payload, 'ctx> {
    pub(in crate::resolved_features) fn new(ctx: &'ctx DecodeContext<'_>, payload: &'payload [u8]) -> Result<Self, CodecError> {
        Ok(Self { payload, offsets: std::cell::OnceCell::new(),
            storage: std::cell::RefCell::new(ctx.reserve_scoped(0, "index SLDPRT sketch marker prefixes")?) })
    }

    pub(in crate::resolved_features) fn nth(&self, ctx: &DecodeContext<'_>, start: usize, end: usize, ordinal: usize) -> Result<Option<u64>, CodecError> {
        const OPERATION: &str = "index SLDPRT sketch marker prefixes";
        let offsets = match self.offsets.get() {
            Some(offsets) => offsets,
            None => {
                let mut offsets = Vec::new();
                self.storage.borrow_mut().with_storage(|| {
                    for (offset, _) in ctx.admit_iter(self.payload, OPERATION)?.enumerate() {
                        if sketch_marker_prefix_at(self.payload, offset) {
                            ctx.push_vec(&mut offsets, offset, OPERATION)?;
                        }
                    }
                    Ok::<(), CodecError>(())
                })?;
                self.offsets.get_or_init(|| offsets)
            }
        };
        let first = ctx.partition_point(offsets, |offset| Ok(*offset < start), "lookup SLDPRT sketch marker prefix")?;
        Ok(first.checked_add(ordinal).and_then(|index| offsets.get(index)).copied()
            .filter(|offset| *offset <= end).map(u64_from_index))
    }
}

/// An owner's offset roster includes either every marker or a coordinate subset.
#[derive(Clone, Copy)]
pub(in crate::resolved_features) enum MarkerRoster {
    All,
    Located,
    Geometry,
    Points,
}

impl<'a, 'ctx> MarkerGeometryIndex<'a, 'ctx> {
    pub(in crate::resolved_features) fn roster<'query>(
        &'query self,
        curve: &'query SketchInputEntity,
        kind: MarkerRoster,
    ) -> Result<&'query [&'a SketchInputEntity], CodecError> {
        const OPERATION: &str = "index SLDPRT owner marker rosters";
        let ctx = self.ctx;
        let Some(owner) = ctx.get_hash_map(&self.owners, &curve.feature_ref.as_deref(), OPERATION)? else { return Ok(&[]); };
        let (cache, source) = match kind {
            MarkerRoster::All => (&owner.all_roster, &owner.all),
            MarkerRoster::Located => (&owner.located_roster, &owner.markers),
            MarkerRoster::Geometry => (&owner.geometry_roster, &owner.markers),
            MarkerRoster::Points => return Ok(&owner.points),
        };
        if let Some(roster) = cache.get() { return Ok(roster); }
        let mut roster = Vec::new();
        self.roster_storage.borrow_mut().with_storage(|| {
            for &marker in ctx.admit_iter(source, OPERATION)? {
                if matches!(kind, MarkerRoster::Geometry) && !matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint | SketchInputKind::LineOrCircle | SketchInputKind::Arc) { continue; }
                ctx.push_vec(&mut roster, marker, OPERATION)?;
            }
            ctx.sort_unstable_by_key(&mut roster, |marker| marker.offset(), Ord::cmp, OPERATION)
        })?;
        Ok(cache.get_or_init(|| roster))
    }
}

#[cfg(test)]
mod tests;
