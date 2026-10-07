//! Marker lookups shared by the geometry queries for one sketch.

use crate::resolved_features::markers::{sketch_marker_prefix_at, compact_legacy_coordinate_roster_coordinates, compact_legacy_code_two_profile_point_coordinates, compact_legacy_embedded_geometry_coordinates};
use crate::resolved_features::relation_loci::same_dimension_length;
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
    coordinate_order: std::cell::OnceCell<Vec<(f64, &'a SketchInputEntity)>>,
    embedded_roster: std::cell::RefCell<EmbeddedRoster<'a>>,
}

/// Marker sources by object index and located markers by owner, with point positions in payload order.
pub(in crate::resolved_features) struct MarkerGeometryIndex<'a, 'payload, 'ctx> {
    ctx: &'ctx DecodeContext<'ctx>,
    prefixes: std::rc::Rc<MarkerPrefixIndex<'payload, 'ctx>>,
    roster_storage: std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    owners: HashMap<Option<&'a str>, OwnerGeometry<'a, 'ctx>>,
    owner_order: Vec<Option<&'a str>>,
    _owner_order_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    object_sources: std::cell::OnceCell<HashMap<Option<u32>, Vec<&'a SketchInputEntity>>>,
    objects: HashMap<(Option<&'a str>, Option<u32>), Vec<&'a SketchInputEntity>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'payload, 'ctx> MarkerGeometryIndex<'a, 'payload, 'ctx> {
    pub(in crate::resolved_features) fn new(
        ctx: &'ctx DecodeContext<'ctx>,
        markers: &[&'a SketchInputEntity],
        prefixes: std::rc::Rc<MarkerPrefixIndex<'payload, 'ctx>>,
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
                        coordinate_order: std::cell::OnceCell::new(), embedded_roster: std::cell::RefCell::new(EmbeddedRoster::default()),
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
        for &owner in ctx.admit_iter(&owner_order, OPERATION)? {
            if let Some(group) = ctx.get_mut_hash_map(&mut owners, &owner, OPERATION)? {
                ctx.sort_unstable_by_key(&mut group.points, |marker| marker.offset(), Ord::cmp, OPERATION)?;
            }
        }
        Ok(Self { ctx, prefixes, owners, objects, owner_order, _owner_order_storage: order_storage, object_sources: std::cell::OnceCell::new(), _storage: storage, roster_storage: std::cell::RefCell::new(ctx.reserve_scoped(0, "index SLDPRT owner marker rosters")?) })
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

    /// All sources, including unlocated records and records of other owners.
    pub(in crate::resolved_features) fn object_sources(
        &self, ctx: &DecodeContext<'_>, index: Option<u32>,
    ) -> Result<&[&'a SketchInputEntity], CodecError> {
        const OPERATION: &str = "index SLDPRT object marker sources";
        let sources = match self.object_sources.get() {
            Some(sources) => sources,
            None => {
                let mut sources = HashMap::new();
                self.roster_storage.borrow_mut().with_storage(|| {
                    for owner in ctx.admit_iter(&self.owner_order, OPERATION)? {
                        let Some(group) = ctx.get_hash_map(&self.owners, owner, OPERATION)? else { continue; };
                        for &marker in ctx.admit_iter(&group.all, OPERATION)? {
                            ctx.push_hash_group(&mut sources, marker.object_index(), marker, OPERATION, OPERATION)?;
                        }
                    }
                    Ok::<_, CodecError>(())
                })?;
                self.object_sources.get_or_init(|| sources)
            }
        };
        Ok(ctx.get_hash_map(sources, &index, "lookup SLDPRT object marker sources")?.map_or(&[], Vec::as_slice))
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
    coordinates: std::cell::OnceCell<Vec<LegacyCoordinateRecord>>,
    _node_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    storage: std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'payload, 'ctx> MarkerPrefixIndex<'payload, 'ctx> {
    pub(in crate::resolved_features) fn new(ctx: &'ctx DecodeContext<'_>, payload: &'payload [u8]) -> Result<std::rc::Rc<Self>, CodecError> {
        // Rc stores one index and two reference counts in its allocation.
        let node_storage = ctx.reserve_scoped(u64_from_index(std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>()), "allocate SLDPRT marker prefix index")?;
        Ok(std::rc::Rc::new(Self { payload, offsets: std::cell::OnceCell::new(), coordinates: std::cell::OnceCell::new(), _node_storage: node_storage,
            storage: std::cell::RefCell::new(ctx.reserve_scoped(0, "index SLDPRT sketch marker prefixes")?) }))
    }

    fn offsets(&self, ctx: &DecodeContext<'_>) -> Result<&Vec<usize>, CodecError> {
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
        Ok(offsets)
    }

    pub(in crate::resolved_features) fn nth(&self, ctx: &DecodeContext<'_>, start: usize, end: usize, ordinal: usize) -> Result<Option<u64>, CodecError> {
        let offsets = self.offsets(ctx)?;
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

impl<'a, 'payload, 'ctx> MarkerGeometryIndex<'a, 'payload, 'ctx> {
    pub(in crate::resolved_features) fn roster<'query>(
        &'query self,
        curve: &'query SketchInputEntity,
        kind: MarkerRoster,
    ) -> Result<&'query [&'a SketchInputEntity], CodecError> {
        const OPERATION: &str = "index SLDPRT owner marker rosters";
        let ctx = self.ctx;
        let Some(owner) = ctx.get_hash_map(&self.owners, &curve.feature_ref.as_deref(), "lookup SLDPRT owner marker rosters")? else { return Ok(&[]); };
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

/// Coordinate records in payload order, with prefix counts for each legacy layout.
struct LegacyCoordinateRecord {
    offset: usize,
    coordinates: [f64; 2],
    code_two_count: usize,
    embedded_count: usize,
}

#[derive(Default)]
struct EmbeddedRoster<'a> {
    markers: Vec<&'a SketchInputEntity>,
    failed: bool,
}

impl<'payload, 'ctx> MarkerPrefixIndex<'payload, 'ctx> {
    fn coordinate_records(&self, ctx: &DecodeContext<'_>) -> Result<&[LegacyCoordinateRecord], CodecError> {
        const OPERATION: &str = "index SLDPRT legacy coordinate records";
        if let Some(records) = self.coordinates.get() { return Ok(records); }
        let offsets = self.offsets(ctx)?;
        let mut records = Vec::new();
        let mut code_two_count = 0;
        let mut embedded_count = 0;
        self.storage.borrow_mut().with_storage(|| -> Result<_, CodecError> {
            for &offset in ctx.admit_iter(offsets, OPERATION)? {
                let Some(coordinates) = compact_legacy_coordinate_roster_coordinates(self.payload, offset) else { continue; };
                code_two_count += usize::from(compact_legacy_code_two_profile_point_coordinates(self.payload, offset).is_some());
                embedded_count += usize::from(compact_legacy_embedded_geometry_coordinates(self.payload, offset).is_some());
                ctx.push_vec(&mut records, LegacyCoordinateRecord { offset, coordinates: coordinates.get(), code_two_count, embedded_count }, OPERATION)?;
            }
            Ok(())
        })?;
        Ok(self.coordinates.get_or_init(|| records))
    }
}

impl<'a, 'payload, 'ctx> MarkerGeometryIndex<'a, 'payload, 'ctx> {
    /// Resolves each legacy coordinate once per owner and extends the cached prefix as needed.
    pub(in crate::resolved_features) fn embedded_roster<'query>(
        &'query self,
        curve: &'query SketchInputEntity,
    ) -> Result<Option<std::cell::Ref<'query, [&'a SketchInputEntity]>>, CodecError> {
        const OPERATION: &str = "resolve SLDPRT embedded coordinate roster";
        // Ten times the relative acceptance tolerance includes every accepted U coordinate.
        const COORDINATE_SEARCH_MARGIN: f64 = 1.0e-8;
        let ctx = self.ctx;
        let source = self.roster(curve, MarkerRoster::Geometry)?;
        let Some(first) = source.first().and_then(|marker| usize::try_from(marker.offset()).ok()) else { return Ok(None); };
        let Some(owner_offset) = usize::try_from(curve.offset()).ok().filter(|offset| *offset >= first) else { return Ok(None); };
        let last = owner_offset.saturating_sub(crate::resolved_features::LEGACY_SKETCH_MARKER.len());
        if last < first { return Ok(None); }
        let records = self.prefixes.coordinate_records(ctx)?;
        let start = ctx.partition_point(records, |record| Ok(record.offset < first), "lookup SLDPRT embedded coordinate roster")?;
        let end = ctx.partition_point(records, |record| Ok(record.offset <= last), "lookup SLDPRT embedded coordinate roster")?;
        if start == end { return Ok(None); }
        let before = start.checked_sub(1).map(|index| &records[index]);
        let tail = &records[end - 1];
        if tail.code_two_count == before.map_or(0, |record| record.code_two_count)
            || tail.embedded_count == before.map_or(0, |record| record.embedded_count) { return Ok(None); }
        let Some(owner) = ctx.get_hash_map(&self.owners, &curve.feature_ref.as_deref(), "lookup SLDPRT embedded coordinate roster")? else { return Ok(None); };
        let positions = match owner.coordinate_order.get() {
            Some(positions) => positions,
            None => {
                let mut positions = Vec::new();
                self.roster_storage.borrow_mut().with_storage(|| -> Result<_, CodecError> {
                    for &marker in ctx.admit_iter(source, OPERATION)? {
                        let Some(coordinates) = marker.coordinates_m else { continue; };
                        ctx.push_vec(&mut positions, (coordinates[0], marker), OPERATION)?;
                    }
                    ctx.sort_unstable_by_key(&mut positions, |point| point.0, f64::total_cmp, OPERATION)
                })?;
                owner.coordinate_order.get_or_init(|| positions)
            }
        };
        let count = end - start;
        let mut cache = owner.embedded_roster.borrow_mut();
        if cache.failed && count > cache.markers.len() { return Ok(None); }
        let mut pending = start + cache.markers.len()..end;
        while !pending.is_empty() {
            let Some(index) = ctx.next_charged(&mut pending, OPERATION)? else { break; };
            let coordinates = records[index].coordinates;
            let margin = COORDINATE_SEARCH_MARGIN * coordinates[0].abs().max(1.0);
            let lower = ctx.partition_point(positions, |point| Ok(point.0 < coordinates[0] - margin), OPERATION)?;
            let upper = ctx.partition_point(positions, |point| Ok(point.0 <= coordinates[0] + margin), OPERATION)?;
            let mut selected = None;
            let ambiguous = ctx.any_by(&positions[lower..upper], |&(_, marker)| {
                let Some(candidate) = marker.coordinates_m else { return Ok(false); };
                if !same_dimension_length(candidate[0], coordinates[0]) || !same_dimension_length(candidate[1], coordinates[1]) { return Ok(false); }
                Ok(selected.replace(marker).is_some())
            }, OPERATION)?;
            let Some(marker) = selected.filter(|_| !ambiguous) else { cache.failed = true; return Ok(None); };
            self.roster_storage.borrow_mut().with_storage(|| ctx.push_vec(&mut cache.markers, marker, OPERATION))?;
        }
        drop(cache);
        Ok(Some(std::cell::Ref::map(owner.embedded_roster.borrow(), |cache| &cache.markers[..count])))
    }
}

#[cfg(test)]
mod tests;
