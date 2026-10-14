// SPDX-License-Identifier: Apache-2.0
//! Immutable source markers and lazy owner-specific coordinate rosters.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::records::{SketchInputEntity, SketchInputKind};

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
pub(in crate::resolved_features) struct CoordinateRosters<'markers, 'a> {
    markers: Cow<'markers, [&'a SketchInputEntity]>,
    rows: RefCell<[Option<OwnerRows<'a>>; 2]>,
}
impl<'markers, 'a> CoordinateRosters<'markers, 'a> {
    pub(in crate::resolved_features) fn new(markers: Vec<&'a SketchInputEntity>) -> Self {
        Self {
            markers: Cow::Owned(markers),
            rows: RefCell::new([None, None]),
        }
    }
    pub(in crate::resolved_features) fn borrowed(
        markers: &'markers [&'a SketchInputEntity],
    ) -> Self {
        Self {
            markers: Cow::Borrowed(markers),
            rows: RefCell::new([None, None]),
        }
    }
}
impl<'a> std::ops::Deref for CoordinateRosters<'_, 'a> {
    type Target = [&'a SketchInputEntity];
    fn deref(&self) -> &Self::Target {
        &self.markers
    }
}
impl<'a, 'r> IntoIterator for &'r CoordinateRosters<'_, 'a> {
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
        owner: &SketchInputEntity,
        complete: bool,
        read: impl FnOnce(&[&'a SketchInputEntity]) -> T,
    ) -> T;
}
impl<'a, S: AsRef<[&'a SketchInputEntity]> + ?Sized> CoordinateRosterSource<'a> for S {
    fn markers(&self) -> &[&'a SketchInputEntity] {
        self.as_ref()
    }
    fn with_roster<T>(
        &self,
        owner: &SketchInputEntity,
        complete: bool,
        read: impl FnOnce(&[&'a SketchInputEntity]) -> T,
    ) -> T {
        let mut roster =
            super::collect_endpoint_markers(self.as_ref().iter().copied(), owner, |marker| {
                complete || has_coordinates(marker)
            });
        super::sort_endpoint_markers(&mut roster);
        read(&roster)
    }
}
impl<'a> CoordinateRosterSource<'a> for CoordinateRosters<'_, 'a> {
    fn markers(&self) -> &[&'a SketchInputEntity] {
        &self.markers
    }
    fn with_roster<T>(
        &self,
        owner: &SketchInputEntity,
        complete: bool,
        read: impl FnOnce(&[&'a SketchInputEntity]) -> T,
    ) -> T {
        let mut rows = self.rows.borrow_mut();
        let index = usize::from(complete);
        let owners = rows[index].get_or_insert_with(|| {
            let mut owners: OwnerRows<'a> = HashMap::new();
            for &marker in self.markers.iter() {
                if !complete && !has_coordinates(marker) {
                    continue;
                }
                owners
                    .entry(marker.feature_ref.as_deref())
                    .or_default()
                    .push(marker);
            }
            for roster in owners.values_mut() {
                super::sort_endpoint_markers(roster);
            }
            owners
        });
        let roster = owners.get(&owner.feature_ref.as_deref());
        read(roster.map_or(&[], Vec::as_slice))
    }
}

#[cfg(test)]
mod tests;
