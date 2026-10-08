// SPDX-License-Identifier: Apache-2.0
//! Segment identity admission and source row order.

use super::definitions::{
    FeatureBoundedCurveSegment, FeatureCenteredLineSegment, FeatureCircleSegment,
    FeatureConicSegment, FeatureOpaqueSegment, FeaturePointSegment, FeatureReferenceLineSegment,
    FeatureSegment,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};

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

impl cadmpeg_core::decode::cost::DecodeCost for SegmentRow {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Ordinary(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Circle(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Point(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::CenteredLine(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::ReferenceLine(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::BoundedCurve(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Conic(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
            Self::Opaque(field_0) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(&0_u8, field_0),
                ctx,
                operation,
            ),
        }
    }
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
    /// Row position of each identifier in key order; a repeated identifier
    /// keeps a `None` marker.
    identities: BTreeMap<u32, Option<usize>>,
    /// The same positions for keyed lookup: a probe of a `u32` key is
    /// constant work, so lookups need no context.
    positions: HashMap<u32, Option<usize>>,
}

impl cadmpeg_core::decode::cost::DecodeCost for SegmentRows {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        let identities = ctx.admit_iter(&self.identities, operation)?.try_fold(
            0_u64,
            |bytes, (key, value)| {
                let entry = cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                    &(key, value),
                    ctx,
                    operation,
                )?;
                bytes
                    .checked_add(entry)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
            },
        )?;
        let rows = cadmpeg_core::decode::cost::DecodeCost::decode_cost(&self.rows, ctx, operation)?;
        rows.checked_add(identities)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
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
        let mut positions = HashMap::new();
        for (ordinal, row) in ctx
            .admit_iter(&rows, "creo segment identity rows")?
            .enumerate()
        {
            let external_id = row.external_id();
            match ctx.entry_btree_map(
                &mut identities,
                external_id,
                "creo segment identity nodes",
            )? {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Some(ordinal));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
            }
            match ctx.entry_hash_map(&mut positions, external_id, "creo segment identity index")? {
                Entry::Vacant(entry) => {
                    entry.insert(Some(ordinal));
                }
                Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
        Ok(Self {
            rows,
            identities,
            positions,
        })
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
        self.positions
            .entry(id)
            .and_modify(|position| *position = None)
            .or_insert(Some(ordinal));
        self.rows.push(row);
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns the stored rows in source order for admitted borrowed traversal.
    pub(crate) fn as_slice(&self) -> &[SegmentRow] {
        self.rows.as_slice()
    }

    /// Returns the existing identity index for admitted borrowed traversal.
    pub(crate) fn identity_entries(&self) -> &BTreeMap<u32, Option<usize>> {
        &self.identities
    }

    pub(crate) fn get(&self, id: u32) -> Option<&SegmentRow> {
        self.rows.get((*self.positions.get(&id)?)?)
    }

    pub(crate) fn contains_id(&self, id: u32) -> bool {
        self.positions.contains_key(&id)
    }

    #[cfg(test)]
    pub(crate) fn unique_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.identities
            .iter()
            .filter_map(|(&id, ordinal)| ordinal.map(|_| id))
    }

    #[cfg(test)]
    pub(crate) fn conflicting_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.identities
            .iter()
            .filter_map(|(&id, ordinal)| ordinal.is_none().then_some(id))
    }

    #[cfg(test)]
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

    pub(crate) fn add_offset(
        &mut self,
        ctx: &DecodeContext<'_>,
        base: usize,
    ) -> Result<(), CodecError> {
        for row in ctx.admit_iter(&mut self.rows, "creo segment offset traversal")? {
            row.add_offset(base);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn ordinary(&self) -> impl Iterator<Item = &FeatureSegment> {
        self.select(|row| match row {
            SegmentRow::Ordinary(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn circles(&self) -> impl Iterator<Item = &FeatureCircleSegment> {
        self.select(|row| match row {
            SegmentRow::Circle(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn points(&self) -> impl Iterator<Item = &FeaturePointSegment> {
        self.select(|row| match row {
            SegmentRow::Point(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn centered_lines(&self) -> impl Iterator<Item = &FeatureCenteredLineSegment> {
        self.select(|row| match row {
            SegmentRow::CenteredLine(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn reference_lines(&self) -> impl Iterator<Item = &FeatureReferenceLineSegment> {
        self.select(|row| match row {
            SegmentRow::ReferenceLine(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn bounded_curves(&self) -> impl Iterator<Item = &FeatureBoundedCurveSegment> {
        self.select(|row| match row {
            SegmentRow::BoundedCurve(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn conics(&self) -> impl Iterator<Item = &FeatureConicSegment> {
        self.select(|row| match row {
            SegmentRow::Conic(row) => Some(row),
            _ => None,
        })
    }

    #[cfg(test)]
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

        crate::decode::with_test_decode_ctx(|ctx| rows.add_offset(ctx, 200))
            .expect("offset admission");
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
