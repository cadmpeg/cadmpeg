// SPDX-License-Identifier: Apache-2.0
//! Evidence-only views of history states
//! ([spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! A solid loft names its caps and side faces in the history state that its
//! naming tag first appears in. Later features can remove or copy those faces
//! in the active body. The view decodes that state into a scratch model that
//! the loft result check reads. It adds no record to the document, and the
//! nest cap and nest work budget of [`super::history_bodies`] do not apply to
//! it.

use cadmpeg_asm::brep::{decode_with_header, DecodePurpose};
use cadmpeg_asm::history::{materialize_record_table, walk_entity_versions};
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

use super::history_states::Closed;
use crate::kernel::ActiveCarrier;

/// Cap of records materialized for all views of a part.
pub(crate) const MAX_STATE_VIEW_WORK: usize = 1 << 21;

/// The model of one history state.
pub(crate) enum StateModel {
    /// The state decoded.
    Model(Box<CadIr>),
    /// No state lists the feature, or the state does not materialize or decode.
    Unavailable,
    /// The views of the part passed their work cap.
    Bounded,
}

/// Source of the models of history states.
pub(crate) trait ResultStates {
    /// The model of the oldest state whose live naming tags list
    /// `source_index`.
    fn after(&self, ctx: &DecodeContext<'_>, source_index: u32) -> Result<StateModel, CodecError>;
}

/// The views of the states of one closed chain.
pub(crate) struct StateViews<'c, 'a> {
    carrier: &'c ActiveCarrier<'a>,
    closed: &'c Closed<'a>,
    work: WorkBudget,
}

impl<'c, 'a> StateViews<'c, 'a> {
    /// Views over a closed chain with a work cap of `limit` records.
    pub(crate) fn new(
        carrier: &'c ActiveCarrier<'a>,
        closed: &'c Closed<'a>,
        limit: usize,
    ) -> Self {
        Self {
            carrier,
            closed,
            work: WorkBudget::new(limit),
        }
    }
}

impl ResultStates for StateViews<'_, '_> {
    fn after(&self, ctx: &DecodeContext<'_>, source_index: u32) -> Result<StateModel, CodecError> {
        let Closed {
            views,
            archived_ids,
            active_count,
            index,
            ..
        } = self.closed;
        let (Some(&ordinal), Ok(header), Some(archive)) = (
            index.first.get(&source_index),
            self.carrier.header.as_ref(),
            self.closed.archive(),
        ) else {
            return Ok(StateModel::Unavailable);
        };
        let state_count = views.len();
        let mut visited = 0_usize;
        let mut bounded = false;
        let mut table = None;
        walk_entity_versions(views, *active_count, archived_ids, |_, versions| {
            let visit = visited;
            visited += 1;
            let at = state_count
                .checked_sub(visit + 1)
                .and_then(|at| u32::try_from(at).ok());
            if at != Some(ordinal) {
                return;
            }
            if !self.work.charge_by(work_units(versions.len())) {
                bounded = true;
                return;
            }
            table = materialize_record_table(versions, |revision| archive.get(&revision));
        });
        if bounded {
            return Ok(StateModel::Bounded);
        }
        let Some(table) = table else {
            return Ok(StateModel::Unavailable);
        };
        let stream = format!("inventor:history:{ordinal}");
        let brep = match decode_with_header(
            ctx,
            &table,
            self.carrier.bytes.window(),
            Some(&header.metadata),
            &stream,
            cadmpeg_asm::asm_format!("inventor"),
            DecodePurpose::Model,
        ) {
            Ok(brep) => brep,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(StateModel::Unavailable),
        };
        drop(table);
        let mut model = CadIr::empty();
        match cadmpeg_asm::brep::transfer::transfer_into_ir(
            ctx,
            &mut model,
            cadmpeg_ir::codec_id!("inventor"),
            brep,
        ) {
            Ok(_) => Ok(StateModel::Model(Box::new(model))),
            Err(error @ CodecError::ResourceLimit(_)) => Err(error),
            Err(_) => Ok(StateModel::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests;
