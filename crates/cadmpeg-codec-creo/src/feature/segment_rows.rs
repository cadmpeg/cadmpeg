// SPDX-License-Identifier: Apache-2.0
//! Segment identity admission and source row order.

use super::definitions::{
    FeatureBoundedCurveSegment, FeatureCenteredLineSegment, FeatureCircleSegment,
    FeatureConicSegment, FeatureOpaqueSegment, FeaturePointSegment, FeatureReferenceLineSegment,
    FeatureSegment,
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct OrderedRow {
    ordinal: usize,
    row: SegmentRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Identity {
    Unique(OrderedRow),
    Conflict {
        first: OrderedRow,
        second: OrderedRow,
        rest: Vec<OrderedRow>,
    },
}

impl Identity {
    fn rows(&self) -> impl Iterator<Item = &OrderedRow> {
        let (first, second, rest) = match self {
            Self::Unique(row) => (row, None, &[][..]),
            Self::Conflict {
                first,
                second,
                rest,
            } => (first, Some(second), rest.as_slice()),
        };
        std::iter::once(first).chain(second).chain(rest)
    }
}

/// An ID resolves only while its source row is unique across all families.
/// Conflicting rows remain available for the native record projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SegmentRows {
    identities: BTreeMap<u32, Identity>,
}

impl FromIterator<SegmentRow> for SegmentRows {
    fn from_iter<T: IntoIterator<Item = SegmentRow>>(rows: T) -> Self {
        let mut result = Self::default();
        for (ordinal, row) in rows.into_iter().enumerate() {
            result.insert_ordered(OrderedRow { ordinal, row });
        }
        result
    }
}

impl SegmentRows {
    #[cfg(test)]
    pub(crate) fn insert(&mut self, row: SegmentRow) {
        self.insert_ordered(OrderedRow {
            ordinal: self.len(),
            row,
        });
    }

    fn insert_ordered(&mut self, row: OrderedRow) {
        let id = row.row.external_id();
        let identity = match self.identities.remove(&id) {
            None => Identity::Unique(row),
            Some(Identity::Unique(first)) => Identity::Conflict {
                first,
                second: row,
                rest: Vec::new(),
            },
            Some(Identity::Conflict {
                first,
                second,
                mut rest,
            }) => {
                rest.push(row);
                Identity::Conflict {
                    first,
                    second,
                    rest,
                }
            }
        };
        self.identities.insert(id, identity);
    }

    pub(crate) fn len(&self) -> usize {
        self.identities
            .values()
            .map(|identity| match identity {
                Identity::Unique(_) => 1,
                Identity::Conflict { rest, .. } => 2 + rest.len(),
            })
            .sum()
    }

    pub(crate) fn get(&self, id: u32) -> Option<&SegmentRow> {
        match self.identities.get(&id)? {
            Identity::Unique(row) => Some(&row.row),
            Identity::Conflict { .. } => None,
        }
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
            .filter_map(|(&id, identity)| matches!(identity, Identity::Unique(_)).then_some(id))
    }

    pub(crate) fn conflicting_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.identities.iter().filter_map(|(&id, identity)| {
            matches!(identity, Identity::Conflict { .. }).then_some(id)
        })
    }

    fn select<'a, T: 'a>(
        &'a self,
        select: impl Fn(&'a SegmentRow) -> Option<&'a T>,
    ) -> impl Iterator<Item = &'a T> {
        let mut rows = self
            .identities
            .values()
            .flat_map(Identity::rows)
            .filter_map(|row| Some((row.ordinal, select(&row.row)?)))
            .collect::<Vec<_>>();
        rows.sort_unstable_by_key(|(ordinal, _)| *ordinal);
        rows.into_iter().map(|(_, row)| row)
    }

    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &SegmentRow> {
        self.select(Some)
    }

    pub(crate) fn add_offset(&mut self, base: usize) {
        for identity in self.identities.values_mut() {
            match identity {
                Identity::Unique(row) => row.row.add_offset(base),
                Identity::Conflict {
                    first,
                    second,
                    rest,
                } => {
                    first.row.add_offset(base);
                    second.row.add_offset(base);
                    for row in rest {
                        row.row.add_offset(base);
                    }
                }
            }
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
