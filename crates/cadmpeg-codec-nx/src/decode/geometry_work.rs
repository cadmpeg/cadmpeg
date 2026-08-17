// SPDX-License-Identifier: Apache-2.0
//! Shared work accounting for adaptive geometry certification.

use cadmpeg_core::decode::WorkBudget;

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

pub(crate) type GeometryWorkBudget<'a> = WorkBudget<'a>;
