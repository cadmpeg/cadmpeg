// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::{
    DecodeContext, ResourceDimension, ResourceFailure, ResourceLimit, ScopedReservation, WorkBudget,
};
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

/// Maximum adaptive geometry work admitted for one decoded model.
pub(super) const MAX_ADAPTIVE_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while completing intersection
/// pcurves for one decoded model.
///
/// Pcurve completion is a separate model-wide phase. Keeping its budget
/// independent prevents a large but valid completion set from exhausting the
/// carrier and topology-certification budget, while retaining a hard bound on
/// the completion phase itself.
pub(super) const MAX_PCURVE_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while validating serialized
/// EXT11 support-UV lanes for one decoded model.
pub(super) const MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while validating or completing
/// EXT11 support-UV lanes for one decoded model.
pub(super) const MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK: usize = 8_000_000;

/// Maximum geometry evaluation work admitted while continuing coupled
/// surface-intersection support lanes for one decoded model.
pub(super) const MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK: usize = 8_000_000;

/// Geometry work accounting plus the cache of successful blend-geometry
/// certificates earned within the same accounting scope.
pub(super) struct GeometryWorkBudget<'a> {
    work: WorkBudget<'a>,
    charges: Option<&'a dyn ScratchCharges>,
    blend_frame_cache: Rc<RefCell<super::blend::BlendSurfaceFrameCache>>,
}

trait ScratchCharges {
    fn charge_items(&self, count: u64, operation: &'static str) -> Result<(), ResourceLimit>;
    fn resource_refusal(&self) -> Option<ResourceLimit>;
    fn fuse_local_work(&self, limit: u64);
    fn reserve_bytes(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit>;
}

impl ScratchCharges for DecodeContext<'_> {
    fn charge_items(&self, count: u64, operation: &'static str) -> Result<(), ResourceLimit> {
        self.charge_collection_items_limit(count, operation)
    }

    fn resource_refusal(&self) -> Option<ResourceLimit> {
        DecodeContext::resource_refusal(self)
    }

    fn fuse_local_work(&self, limit: u64) {
        let additional = limit + u64::from(limit != u64::MAX);
        drop(self.refuse_codec_limit("nx adaptive geometry work", limit, additional));
    }

    fn reserve_bytes(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit> {
        self.reserve_scoped_limit(bytes, operation)
    }
}

impl<'a> GeometryWorkBudget<'a> {
    #[cfg(test)]
    pub(super) fn new(limit: usize) -> Self {
        Self::from_work_budget(WorkBudget::new(limit))
    }

    #[cfg(test)]
    fn from_work_budget(work: WorkBudget<'a>) -> Self {
        Self {
            work,
            charges: None,
            blend_frame_cache: Rc::new(RefCell::new(
                super::blend::BlendSurfaceFrameCache::default(),
            )),
        }
    }

    pub(super) fn from_context(ctx: &'a DecodeContext<'_>, limit: u64) -> Self {
        Self {
            work: ctx.work_budget(limit),
            charges: Some(ctx),
            blend_frame_cache: Rc::new(RefCell::new(
                super::blend::BlendSurfaceFrameCache::default(),
            )),
        }
    }

    pub(super) fn reserve_vec<T>(
        &self,
        output: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<Option<ScopedReservation<'_>>, ResourceLimit> {
        let count_u64 = u64::try_from(count).unwrap_or(u64::MAX);
        let invalid_size = || ResourceLimit {
            dimension: ResourceDimension::Codec(operation),
            reason: ResourceFailure::BudgetExceeded,
            limit: u64::MAX,
            used: 0,
            additional: count_u64,
            operation,
        };
        let item_bytes = u64::try_from(std::mem::size_of::<T>()).map_err(|_| invalid_size())?;
        let bytes = count_u64.checked_mul(item_bytes).ok_or_else(invalid_size)?;
        let reservation = if let Some(charges) = self.charges {
            charges.charge_items(count_u64, operation)?;
            Some(charges.reserve_bytes(bytes, operation)?)
        } else {
            None
        };
        output.try_reserve_exact(count).map_err(|_| ResourceLimit {
            dimension: ResourceDimension::Codec(operation),
            reason: ResourceFailure::AllocationFailed,
            limit: count_u64,
            used: count_u64,
            additional: 0,
            operation,
        })?;
        Ok(reservation)
    }

    pub(super) fn resource_refusal(&self) -> Option<ResourceLimit> {
        let charges = self.charges?;
        if let Some(limit) = charges.resource_refusal() {
            return Some(limit);
        }
        if self.work.exhausted() {
            let limit = u64::try_from(self.work.consumed()).unwrap_or(u64::MAX);
            charges.fuse_local_work(limit);
        }
        charges.resource_refusal()
    }

    pub(super) fn exhausted(&self) -> bool {
        let exhausted = self.work.exhausted();
        if exhausted {
            let _resource_refusal = self.resource_refusal();
        }
        exhausted
    }

    pub(super) fn child_slice(&self, limit: usize) -> GeometryWorkBudget<'_> {
        GeometryWorkBudget {
            work: self.work.session_child_slice(limit),
            charges: self.charges,
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

impl<'a> Deref for GeometryWorkBudget<'a> {
    type Target = WorkBudget<'a>;

    fn deref(&self) -> &Self::Target {
        &self.work
    }
}
