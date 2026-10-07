//! Marker lookups shared by the geometry queries for one sketch.

use super::sketch_marker_prefix_at;
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

struct OwnerGeometry<'a> {
    points: Vec<&'a SketchInputEntity>,
}

/// Located markers by owner and object index, with point positions in payload order.
pub(in crate::resolved_features) struct MarkerGeometryIndex<'a, 'ctx> {
    owners: HashMap<Option<&'a str>, OwnerGeometry<'a>>,
    objects: HashMap<(Option<&'a str>, Option<u32>), Vec<&'a SketchInputEntity>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> MarkerGeometryIndex<'a, 'ctx> {
    pub(in crate::resolved_features) fn new(
        ctx: &'ctx DecodeContext<'_>,
        markers: &[&'a SketchInputEntity],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index SLDPRT marker geometry";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut owners = HashMap::new();
        let mut objects = HashMap::new();
        let mut owner_order = Vec::new();
        for &marker in ctx.admit_iter(markers, OPERATION)? {
            if marker.coordinates_m.is_none() { continue; }
            let owner = marker.feature_ref.as_deref();
            storage.with_storage(|| {
                if !ctx.contains_key_hash_map(&owners, &owner, OPERATION)? {
                    ctx.insert_hash_map(&mut owners, owner, OwnerGeometry {
                        points: Vec::new(),
                    }, OPERATION)?;
                    ctx.push_vec(&mut owner_order, owner, OPERATION)?;
                }
                let Some(group) = ctx.get_mut_hash_map(&mut owners, &owner, OPERATION)? else { return Ok(()); };
                if matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint) {
                    ctx.push_vec(&mut group.points, marker, OPERATION)?;
                }
                if matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint | SketchInputKind::LineOrCircle | SketchInputKind::Arc) {
                    ctx.push_hash_group(&mut objects, (owner, marker.object_index()), marker, OPERATION, OPERATION)?;
                }
                Ok::<(), CodecError>(())
            })?;
        }
        for owner in ctx.admit_iter(owner_order, OPERATION)? {
            if let Some(group) = ctx.get_mut_hash_map(&mut owners, &owner, OPERATION)? {
                ctx.sort_unstable_by_key(&mut group.points, |marker| marker.offset(), Ord::cmp, OPERATION)?;
            }
        }
        Ok(Self { owners, objects, _storage: storage })
    }

    pub(in crate::resolved_features) fn points<'query>(&'query self, ctx: &DecodeContext<'_>, curve: &'query SketchInputEntity) -> Result<&'query [&'a SketchInputEntity], CodecError> {
        Ok(ctx.get_hash_map(&self.owners, &curve.feature_ref.as_deref(), "lookup SLDPRT profile points")?
            .map_or(&[][..], |owner| owner.points.as_slice()))
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
                if ctx.equal(marker.id(), curve.id(), OPERATION)? { return Ok(false); }
            } else if !matches!(marker.kind(), SketchInputKind::Point | SketchInputKind::ConstrainedPoint) { return Ok(false); }
            if selected.is_some() { return Ok(true); }
            selected = Some(marker);
            Ok(false)
        }, OPERATION)?;
        Ok(selected.filter(|_| !ambiguous))
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
