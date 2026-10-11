// SPDX-License-Identifier: Apache-2.0
//! Completed singleton pcurve selections shared across topology roots.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::PcurveId;

use super::{PcurveSelectionFailure, SelectedPcurve};

#[derive(Clone, Copy)]
enum Selection {
    Accepted(Option<[f64; 2]>),
    Carrier,
    Endpoint,
    Locus,
}

/// Source edges, vertex positions, associations and carrier geometry stay
/// fixed during topology commits. Only topology and implicit surfaces are
/// appended. A source edge/surface pair therefore has one selection result.
/// Store its range or geometric miss, and construct each use's identity and
/// warning separately. Resource refusals do not complete a selection.
pub(super) struct PcurveSelections<'ctx> {
    completed: BTreeMap<(u64, u64), Selection>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> PcurveSelections<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            completed: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "STEP completed pcurve selections")?,
        })
    }

    pub(super) fn select(
        &mut self,
        edge: u64,
        surface: u64,
        candidates: &[PcurveId],
        ctx: &DecodeContext<'_>,
        run: impl FnOnce() -> Result<SelectedPcurve, PcurveSelectionFailure>,
    ) -> Result<SelectedPcurve, PcurveSelectionFailure> {
        let [candidate] = candidates else {
            return run();
        };
        let key = (edge, surface);
        if let Some(selection) = ctx.get_btree_map(
            &self.completed,
            &key,
            "STEP completed pcurve selection lookup",
        )? {
            return match selection {
                Selection::Accepted(parameter_range) => Ok(SelectedPcurve {
                    id: candidate.try_clone_for_decode(ctx, "step_selected_pcurve_id")?,
                    parameter_range: *parameter_range,
                }),
                Selection::Carrier => Err(PcurveSelectionFailure::Carrier),
                Selection::Endpoint => Err(PcurveSelectionFailure::Endpoint),
                Selection::Locus => Err(PcurveSelectionFailure::Locus),
            };
        }
        let result = run();
        let selection = match &result {
            Ok(selected) => Selection::Accepted(selected.parameter_range),
            Err(PcurveSelectionFailure::Carrier) => Selection::Carrier,
            Err(PcurveSelectionFailure::Endpoint) => Selection::Endpoint,
            Err(PcurveSelectionFailure::Locus) => Selection::Locus,
            Err(PcurveSelectionFailure::NotUnique { .. }
                | PcurveSelectionFailure::ResourceLimit(_)
                | PcurveSelectionFailure::Resource(_)) => return result,
        };
        self.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut self.completed,
                key,
                selection,
                "step_completed_pcurve_selections",
            )
        })?;
        result
    }
}

#[cfg(test)]
mod tests;
