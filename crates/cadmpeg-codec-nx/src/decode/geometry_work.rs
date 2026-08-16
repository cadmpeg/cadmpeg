// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::WorkBudget;

/// Maximum adaptive geometry work admitted for one decoded model.
pub(crate) const MAX_ADAPTIVE_GEOMETRY_WORK: usize = 1_000_000;

pub(crate) type GeometryWorkBudget<'a> = WorkBudget<'a>;
