// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::WorkBudget;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

/// Maximum adaptive geometry work admitted for one decoded model.
pub(crate) const MAX_ADAPTIVE_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while completing intersection
/// pcurves for one decoded model.
///
/// Pcurve completion is a separate model-wide phase. Keeping its budget
/// independent prevents a large but valid completion set from exhausting the
/// carrier and topology-certification budget, while retaining a hard bound on
/// the completion phase itself.
pub(crate) const MAX_PCURVE_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while validating serialized
/// EXT11 support-UV lanes for one decoded model.
pub(crate) const MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while validating or completing
/// EXT11 support-UV lanes for one decoded model.
pub(crate) const MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while continuing coupled
/// surface-intersection support lanes for one decoded model.
pub(crate) const MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Geometry work accounting plus the cache of successful blend-geometry
/// certificates earned within the same accounting scope.
pub(crate) struct GeometryWorkBudget<'a> {
    work: WorkBudget<'a>,
    blend_frame_cache: Rc<RefCell<super::blend::BlendSurfaceFrameCache>>,
}

impl<'a> GeometryWorkBudget<'a> {
    #[cfg(test)]
    pub(crate) fn new(limit: usize) -> Self {
        Self::from_work_budget(WorkBudget::new(limit))
    }

    pub(crate) fn from_work_budget(work: WorkBudget<'a>) -> Self {
        Self {
            work,
            blend_frame_cache: Rc::new(RefCell::new(
                super::blend::BlendSurfaceFrameCache::default(),
            )),
        }
    }

    pub(crate) fn child_slice(&self, limit: usize) -> GeometryWorkBudget<'static> {
        GeometryWorkBudget {
            work: self.work.child_slice(limit),
            blend_frame_cache: Rc::clone(&self.blend_frame_cache),
        }
    }

    pub(crate) fn clear_blend_frame_cache(&self) {
        self.blend_frame_cache.borrow_mut().clear();
    }

    pub(crate) fn blend_frame_cache(&self) -> &RefCell<super::blend::BlendSurfaceFrameCache> {
        self.blend_frame_cache.as_ref()
    }
}

impl<'a> Deref for GeometryWorkBudget<'a> {
    type Target = WorkBudget<'a>;

    fn deref(&self) -> &Self::Target {
        &self.work
    }
}
