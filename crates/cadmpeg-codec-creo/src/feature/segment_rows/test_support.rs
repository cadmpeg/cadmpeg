// SPDX-License-Identifier: Apache-2.0
//! Fixture edits rebuild identity admission before the rows become readable.

use super::{SegmentRow, SegmentRows};
use crate::feature::definitions::{
    FeatureBoundedCurveSegment, FeatureCenteredLineSegment, FeatureCircleSegment,
    FeatureConicSegment, FeatureOpaqueSegment, FeaturePointSegment, FeatureReferenceLineSegment,
    FeatureSegment,
};

impl SegmentRows {
    fn replace_family(
        &mut self,
        belongs: impl Fn(&SegmentRow) -> bool,
        replacement: impl IntoIterator<Item = SegmentRow>,
    ) {
        *self = self
            .iter()
            .filter(|row| !belongs(row))
            .cloned()
            .chain(replacement)
            .collect();
    }
    pub(crate) fn edit_ordinary<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureSegment>) -> R,
    ) -> R {
        let mut rows = self.ordinary().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::Ordinary(_)),
            rows.into_iter().map(SegmentRow::Ordinary),
        );
        result
    }
    pub(crate) fn edit_circles<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureCircleSegment>) -> R,
    ) -> R {
        let mut rows = self.circles().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::Circle(_)),
            rows.into_iter().map(SegmentRow::Circle),
        );
        result
    }
    pub(crate) fn edit_points<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeaturePointSegment>) -> R,
    ) -> R {
        let mut rows = self.points().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::Point(_)),
            rows.into_iter().map(SegmentRow::Point),
        );
        result
    }
    pub(crate) fn edit_centered_lines<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureCenteredLineSegment>) -> R,
    ) -> R {
        let mut rows = self.centered_lines().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::CenteredLine(_)),
            rows.into_iter().map(SegmentRow::CenteredLine),
        );
        result
    }
    pub(crate) fn edit_reference_lines<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureReferenceLineSegment>) -> R,
    ) -> R {
        let mut rows = self.reference_lines().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::ReferenceLine(_)),
            rows.into_iter().map(SegmentRow::ReferenceLine),
        );
        result
    }
    pub(crate) fn edit_bounded_curves<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureBoundedCurveSegment>) -> R,
    ) -> R {
        let mut rows = self.bounded_curves().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::BoundedCurve(_)),
            rows.into_iter().map(SegmentRow::BoundedCurve),
        );
        result
    }
    pub(crate) fn edit_conics<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureConicSegment>) -> R,
    ) -> R {
        let mut rows = self.conics().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::Conic(_)),
            rows.into_iter().map(SegmentRow::Conic),
        );
        result
    }
    pub(crate) fn edit_opaque<R>(
        &mut self,
        edit: impl FnOnce(&mut Vec<FeatureOpaqueSegment>) -> R,
    ) -> R {
        let mut rows = self.opaque().cloned().collect();
        let result = edit(&mut rows);
        self.replace_family(
            |row| matches!(row, SegmentRow::Opaque(_)),
            rows.into_iter().map(SegmentRow::Opaque),
        );
        result
    }
}
