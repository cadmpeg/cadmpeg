// SPDX-License-Identifier: Apache-2.0
//! Segment identity admission and source row order.

use super::definitions::{
    FeatureBoundedCurveSegment, FeatureCenteredLineSegment, FeatureCircleSegment,
    FeatureConicSegment, FeatureOpaqueSegment, FeaturePointSegment, FeatureReferenceLineSegment,
    FeatureSegment,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SegmentRow {
    Ordinary(FeatureSegment),
    Circle(FeatureCircleSegment),
    Point(FeaturePointSegment),
    CenteredLine(FeatureCenteredLineSegment),
    ReferenceLine(FeatureReferenceLineSegment),
    BoundedCurve(FeatureBoundedCurveSegment),
    Conic(FeatureConicSegment),
    Opaque(FeatureOpaqueSegment),
}

impl SegmentRow {
    fn external_id(&self) -> u32 {
        match self {
            Self::Ordinary(row) => row.external_id,
            Self::Circle(row) => row.external_id,
            Self::Point(row) => row.external_id,
            Self::CenteredLine(row) => row.external_id,
            Self::ReferenceLine(row) => row.external_id,
            Self::BoundedCurve(row) => row.external_id,
            Self::Conic(row) => row.external_id,
            Self::Opaque(row) => row.external_id,
        }
    }

    fn add_offset(&mut self, base: usize) {
        let offset = match self {
            Self::Ordinary(row) => &mut row.offset,
            Self::Circle(row) => &mut row.offset,
            Self::Point(row) => &mut row.offset,
            Self::CenteredLine(row) => &mut row.offset,
            Self::ReferenceLine(row) => &mut row.offset,
            Self::BoundedCurve(row) => &mut row.offset,
            Self::Conic(row) => &mut row.offset,
            Self::Opaque(row) => &mut row.offset,
        };
        *offset += base;
    }
}

/// An ID resolves only while its source row is unique across all families.
/// Conflicting rows remain available for the native record projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SegmentRows {
    rows: Vec<SegmentRow>,
    identities: BTreeMap<u32, Option<usize>>,
}

#[cfg(test)]
impl FromIterator<SegmentRow> for SegmentRows {
    fn from_iter<T: IntoIterator<Item = SegmentRow>>(rows: T) -> Self {
        let mut result = Self::default();
        for row in rows {
            result.insert(row);
        }
        result
    }
}

impl SegmentRows {
    pub(crate) fn from_parsed_rows(
        ctx: &DecodeContext<'_>,
        rows: Vec<SegmentRow>,
    ) -> Result<Self, CodecError> {
        let mut identities = BTreeMap::new();
        for (ordinal, row) in rows.iter().enumerate() {
            match identities.entry(row.external_id()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo segment identity nodes")?;
                    entry.insert(Some(ordinal));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
        }
        Ok(Self { rows, identities })
    }

    #[cfg(test)]
    pub(crate) fn insert(&mut self, row: SegmentRow) {
        let id = row.external_id();
        let ordinal = self.rows.len();
        match self.identities.entry(id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Some(ordinal));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
        self.rows.push(row);
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn get(&self, id: u32) -> Option<&SegmentRow> {
        self.rows.get((*self.identities.get(&id)?)?)
    }

    pub(crate) fn contains_id(&self, id: u32) -> bool {
        self.identities.contains_key(&id)
    }

    pub(crate) fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.identities.keys().copied()
    }

    pub(crate) fn unique_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.identities
            .iter()
            .filter_map(|(&id, ordinal)| ordinal.map(|_| id))
    }

    pub(crate) fn conflicting_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.identities
            .iter()
            .filter_map(|(&id, ordinal)| ordinal.is_none().then_some(id))
    }

    fn select<'a, T: 'a>(
        &'a self,
        select: impl Fn(&'a SegmentRow) -> Option<&'a T>,
    ) -> impl Iterator<Item = &'a T> {
        self.rows.iter().filter_map(select)
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &SegmentRow> {
        self.select(Some)
    }

    pub(crate) fn add_offset(&mut self, base: usize) {
        for row in &mut self.rows {
            row.add_offset(base);
        }
    }

    pub(crate) fn ordinary(&self) -> impl Iterator<Item = &FeatureSegment> {
        self.select(|row| match row {
            SegmentRow::Ordinary(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn circles(&self) -> impl Iterator<Item = &FeatureCircleSegment> {
        self.select(|row| match row {
            SegmentRow::Circle(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn points(&self) -> impl Iterator<Item = &FeaturePointSegment> {
        self.select(|row| match row {
            SegmentRow::Point(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn centered_lines(&self) -> impl Iterator<Item = &FeatureCenteredLineSegment> {
        self.select(|row| match row {
            SegmentRow::CenteredLine(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn reference_lines(&self) -> impl Iterator<Item = &FeatureReferenceLineSegment> {
        self.select(|row| match row {
            SegmentRow::ReferenceLine(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn bounded_curves(&self) -> impl Iterator<Item = &FeatureBoundedCurveSegment> {
        self.select(|row| match row {
            SegmentRow::BoundedCurve(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn conics(&self) -> impl Iterator<Item = &FeatureConicSegment> {
        self.select(|row| match row {
            SegmentRow::Conic(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn opaque(&self) -> impl Iterator<Item = &FeatureOpaqueSegment> {
        self.select(|row| match row {
            SegmentRow::Opaque(row) => Some(row),
            _ => None,
        })
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests {
    use super::{FeatureCircleSegment, FeaturePointSegment, SegmentRow, SegmentRows};

    #[test]
    fn conflicting_ids_retain_each_family_in_source_order() {
        let point = |external_id, offset| {
            SegmentRow::Point(FeaturePointSegment {
                point_id: 1,
                external_id,
                offset,
            })
        };
        let circle = |external_id, offset| {
            SegmentRow::Circle(FeatureCircleSegment {
                center_id: 2,
                radius_ref: 3,
                external_id,
                offset,
            })
        };
        let mut rows: SegmentRows = [
            point(7, 300),
            circle(2, 10),
            point(3, 200),
            circle(7, 9),
            point(7, 1),
            point(8, 100),
        ]
        .into_iter()
        .collect();

        assert_eq!(rows.len(), 6);
        assert!(rows.get(7).is_none());
        assert_eq!(rows.unique_ids().collect::<Vec<_>>(), [2, 3, 8]);
        assert_eq!(rows.conflicting_ids().collect::<Vec<_>>(), [7]);
        assert_eq!(
            rows.points()
                .map(|row| (row.external_id, row.offset))
                .collect::<Vec<_>>(),
            [(7, 300), (3, 200), (7, 1), (8, 100)]
        );
        assert_eq!(
            rows.circles()
                .map(|row| (row.external_id, row.offset))
                .collect::<Vec<_>>(),
            [(2, 10), (7, 9)]
        );

        rows.add_offset(200);
        assert_eq!(
            rows.points().map(|row| row.offset).collect::<Vec<_>>(),
            [500, 400, 201, 300]
        );
        assert_eq!(
            rows.circles().map(|row| row.offset).collect::<Vec<_>>(),
            [210, 209]
        );
    }
}
