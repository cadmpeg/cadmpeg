// SPDX-License-Identifier: Apache-2.0
//! Immutable source markers and lazy owner-specific coordinate rosters.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::records::{SketchInputEntity, SketchInputKind};

const OPERATION: &str = "resolve SLDPRT coordinate endpoint roster";
type OwnerRows<'a> = HashMap<Option<&'a str>, Vec<&'a SketchInputEntity>>;

fn has_coordinates(marker: &SketchInputEntity) -> bool {
    marker.coordinates_m.is_some()
        && matches!(
            marker.kind(),
            SketchInputKind::Point
                | SketchInputKind::ConstrainedPoint
                | SketchInputKind::LineOrCircle
                | SketchInputKind::Arc
        )
}

/// Owns one immutable marker inventory. Each grammar is indexed on first use.
pub(in crate::resolved_features) struct CoordinateRosters<'markers, 'a, 'ctx> {
    markers: Cow<'markers, [&'a SketchInputEntity]>,
    ctx: &'ctx DecodeContext<'ctx>,
    storage: RefCell<ScopedReservation<'ctx>>,
    rows: RefCell<[Option<OwnerRows<'a>>; 2]>,
}
impl<'markers, 'a, 'ctx> CoordinateRosters<'markers, 'a, 'ctx> {
    pub(in crate::resolved_features) fn new(
        ctx: &'ctx DecodeContext<'ctx>,
        markers: Vec<&'a SketchInputEntity>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            storage: RefCell::new(ctx.reserve_scoped(0, OPERATION)?),
            markers: Cow::Owned(markers),
            rows: RefCell::new([None, None]),
        })
    }
    pub(in crate::resolved_features) fn borrowed(
        ctx: &'ctx DecodeContext<'ctx>,
        markers: &'markers [&'a SketchInputEntity],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            ctx,
            storage: RefCell::new(ctx.reserve_scoped(0, OPERATION)?),
            markers: Cow::Borrowed(markers),
            rows: RefCell::new([None, None]),
        })
    }
}
impl<'a> std::ops::Deref for CoordinateRosters<'_, 'a, '_> {
    type Target = [&'a SketchInputEntity];
    fn deref(&self) -> &Self::Target {
        &self.markers
    }
}
impl<'a, 'r> IntoIterator for &'r CoordinateRosters<'_, 'a, '_> {
    type Item = &'r &'a SketchInputEntity;
    type IntoIter = std::slice::Iter<'r, &'a SketchInputEntity>;
    fn into_iter(self) -> Self::IntoIter {
        self.markers.iter()
    }
}

/// Supplies immutable source markers and an owner/grammar-specific lookup.
/// Plain slices serve isolated queries; an inventory retains each built index.
pub(in crate::resolved_features) trait CoordinateRosterSource<'a> {
    fn markers(&self) -> &[&'a SketchInputEntity];
    fn with_roster<T>(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &SketchInputEntity,
        complete: bool,
        read: impl FnOnce(&[&'a SketchInputEntity]) -> T,
    ) -> Result<T, CodecError>;
}
impl<'a, S: AsRef<[&'a SketchInputEntity]> + ?Sized> CoordinateRosterSource<'a> for S {
    fn markers(&self) -> &[&'a SketchInputEntity] {
        self.as_ref()
    }
    fn with_roster<T>(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &SketchInputEntity,
        complete: bool,
        read: impl FnOnce(&[&'a SketchInputEntity]) -> T,
    ) -> Result<T, CodecError> {
        let mut roster = super::collect_endpoint_markers(
            ctx,
            self.as_ref().iter().copied(),
            owner,
            |marker| complete || has_coordinates(marker),
            OPERATION,
        )?;
        super::sort_endpoint_markers(ctx, &mut roster, OPERATION)?;
        Ok(read(&roster))
    }
}
impl<'a> CoordinateRosterSource<'a> for CoordinateRosters<'_, 'a, '_> {
    fn markers(&self) -> &[&'a SketchInputEntity] {
        &self.markers
    }
    fn with_roster<T>(
        &self,
        ctx: &DecodeContext<'_>,
        owner: &SketchInputEntity,
        complete: bool,
        read: impl FnOnce(&[&'a SketchInputEntity]) -> T,
    ) -> Result<T, CodecError> {
        if !std::ptr::eq(self.ctx, ctx) {
            return Err(CodecError::malformed(
                "coordinate roster belongs to another decode context",
            ));
        }
        let mut rows = self.rows.borrow_mut();
        let index = usize::from(complete);
        if rows[index].is_none() {
            let owners = self.storage.borrow_mut().with_storage(|| {
                let mut owners: OwnerRows<'a> = HashMap::new();
                for &marker in self.markers.iter() {
                    ctx.charge_work(1, OPERATION)?;
                    if !complete && !has_coordinates(marker) {
                        continue;
                    }
                    let owner = marker.feature_ref.as_deref();
                    super::charge_endpoint_work(ctx, owner.map_or(0, str::len), 4, OPERATION)?;
                    if !owners.contains_key(&owner) {
                        if owners.len() == owners.capacity() {
                            for key in owners.keys() {
                                super::charge_endpoint_work(
                                    ctx,
                                    key.map_or(0, str::len),
                                    4,
                                    OPERATION,
                                )?;
                            }
                        }
                        ctx.reserve_map(&mut owners, 1, OPERATION)?;
                    }
                    let roster = owners.entry(owner).or_default();
                    if roster.len() == roster.capacity() {
                        ctx.charge_work(u64_from_index(roster.len()), OPERATION)?;
                    }
                    ctx.push_vec(roster, marker, OPERATION)?;
                }
                for roster in owners.values_mut() {
                    super::sort_endpoint_markers(ctx, roster, OPERATION)?;
                }
                Ok::<_, CodecError>(owners)
            })?;
            rows[index] = Some(owners);
        }
        let owner = owner.feature_ref.as_deref();
        super::charge_endpoint_work(ctx, owner.map_or(0, str::len), 4, OPERATION)?;
        let roster = rows[index].as_ref().and_then(|rows| rows.get(&owner));
        Ok(read(roster.map_or(&[], Vec::as_slice)))
    }
}

#[cfg(test)]
mod tests;
