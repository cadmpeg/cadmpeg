// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::{DecodeContext, WorkBudget};
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

/// Floor of the adaptive geometry work admitted for one decoded model.
pub(super) const MAX_ADAPTIVE_GEOMETRY_WORK: usize = 8_000_000;

/// Floor of the geometry evaluation work admitted while completing
/// intersection pcurves for one decoded model.
///
/// Pcurve completion is a separate model-wide phase. Keeping its budget
/// independent prevents a large but valid completion set from exhausting the
/// carrier and topology-certification budget, while retaining a hard bound on
/// the completion phase itself.
pub(super) const MAX_PCURVE_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Floor of the geometry evaluation work admitted while validating serialized
/// EXT11 support-UV lanes for one decoded model.
pub(super) const MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Floor of the geometry evaluation work admitted while validating or
/// completing EXT11 support-UV lanes for one decoded model.
pub(super) const MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Floor of the geometry evaluation work admitted while continuing coupled
/// surface-intersection support lanes for one decoded model.
pub(super) const MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Geometry work each selected Parasolid record adds to a model-wide budget.
pub(super) const GEOMETRY_WORK_PER_RECORD: usize = 4096;

/// The limit of one model-wide geometry budget: [`GEOMETRY_WORK_PER_RECORD`]
/// per selected Parasolid record, and never below the phase's `floor`.
pub(super) fn model_geometry_work_limit(floor: usize, records: usize) -> usize {
    records
        .checked_mul(GEOMETRY_WORK_PER_RECORD)
        .map_or(floor, |work| work.max(floor))
}

/// Local geometry work slice plus the cache of successful blend-geometry
/// certificates earned within the same slice.
pub(super) struct GeometryWorkBudget<'a> {
    work: WorkBudget,
    pub(super) ctx: &'a DecodeContext<'a>,
    blend_frame_cache: Rc<RefCell<super::blend::BlendSurfaceFrameCache>>,
}

impl<'a> GeometryWorkBudget<'a> {
    pub(super) fn from_context(ctx: &'a DecodeContext<'_>, limit: usize) -> Self {
        Self {
            work: WorkBudget::new(limit),
            ctx,
            blend_frame_cache: Rc::new(RefCell::new(
                super::blend::BlendSurfaceFrameCache::default(),
            )),
        }
    }

    pub(super) fn child_slice(&self, limit: usize) -> GeometryWorkBudget<'_> {
        GeometryWorkBudget {
            work: self.work.child_slice(limit),
            ctx: self.ctx,
            blend_frame_cache: Rc::clone(&self.blend_frame_cache),
        }
    }

    pub(super) fn clear_blend_frame_cache(&self) {
        self.blend_frame_cache.borrow_mut().clear();
    }

    pub(super) fn blend_frame_cache(&self) -> &RefCell<super::blend::BlendSurfaceFrameCache> {
        self.blend_frame_cache.as_ref()
    }
}

impl Deref for GeometryWorkBudget<'_> {
    type Target = WorkBudget;

    fn deref(&self) -> &Self::Target {
        &self.work
    }
}

#[cfg(test)]
mod tests {
    use super::{model_geometry_work_limit, GEOMETRY_WORK_PER_RECORD, MAX_ADAPTIVE_GEOMETRY_WORK};

    #[test]
    fn model_geometry_work_scales_with_records_above_the_floor() {
        let floor = MAX_ADAPTIVE_GEOMETRY_WORK;
        let past_floor = floor / GEOMETRY_WORK_PER_RECORD * 4;
        assert_eq!(model_geometry_work_limit(floor, 0), floor);
        assert_eq!(model_geometry_work_limit(floor, 100), floor);
        assert_eq!(
            model_geometry_work_limit(floor, past_floor),
            past_floor * GEOMETRY_WORK_PER_RECORD
        );
        assert_eq!(model_geometry_work_limit(floor, usize::MAX), floor);
    }
}
