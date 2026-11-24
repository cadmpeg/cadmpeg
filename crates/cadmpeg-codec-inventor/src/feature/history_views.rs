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
use cadmpeg_asm::history::{materialize_record_table, walk_entity_versions_until};
use cadmpeg_asm::kernel_header::BinaryHeader;
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::rc::Rc;

use super::history_states::Closed;
use crate::kernel::ActiveCarrier;

/// Cap of records materialized for all views of a part.
pub(crate) const MAX_STATE_VIEW_WORK: usize = 1 << 21;

/// The model of one history state.
pub(crate) enum StateModel {
    /// The state decoded.
    Model(Box<CadIr>),
    /// No state lists the feature, or the carrier has no archive to read.
    Unavailable,
    /// The state lists the feature, and it does not materialize, decode or
    /// transfer.
    Failed,
    /// The views of the part passed their work cap.
    Bounded,
}

/// Source of the models of history states.
pub(crate) trait ResultStates {
    /// The model of the oldest state whose live naming tags list
    /// `source_index`. Requests that map to one state share one decode.
    fn after(
        &self,
        ctx: &DecodeContext<'_>,
        source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError>;
}

/// The views of the states of one closed chain.
pub(crate) struct StateViews<'c, 'a> {
    carrier: &'c ActiveCarrier<'a>,
    closed: &'c Closed<'a>,
    work: WorkBudget,
    decoded: RefCell<HashMap<u32, Rc<StateModel>>>,
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
            decoded: RefCell::default(),
        }
    }

    /// Decode the state of `ordinal`. The walk that finds its record map
    /// stops at the state and charges every state it passes.
    fn decode(
        &self,
        ctx: &DecodeContext<'_>,
        header: &BinaryHeader,
        ordinal: u32,
    ) -> Result<StateModel, CodecError> {
        let Closed {
            views,
            archived_ids,
            active_count,
            ..
        } = self.closed;
        let state_count = views.len();
        let mut visited = 0_usize;
        let mut bounded = false;
        let mut versions = None;
        let walk = walk_entity_versions_until(views, *active_count, archived_ids, |_, map| {
            if !self.work.charge_by(work_units(map.len())) {
                bounded = true;
                return false;
            }
            let at = state_count
                .checked_sub(visited + 1)
                .and_then(|at| u32::try_from(at).ok());
            visited += 1;
            if at == Some(ordinal) {
                versions = Some(map.clone());
                return false;
            }
            true
        });
        if bounded {
            return Ok(StateModel::Bounded);
        }
        if walk == ControlFlow::Continue(false) {
            return Ok(StateModel::Failed);
        }
        let Some(archive) = self.closed.archive() else {
            return Ok(StateModel::Unavailable);
        };
        let Some(table) = versions.and_then(|versions| {
            materialize_record_table(&versions, |revision| archive.get(&revision))
        }) else {
            return Ok(StateModel::Failed);
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
            Err(_) => return Ok(StateModel::Failed),
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
            Err(_) => Ok(StateModel::Failed),
        }
    }
}

impl ResultStates for StateViews<'_, '_> {
    fn after(
        &self,
        ctx: &DecodeContext<'_>,
        source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        // Bind the state and the header first: a source that no state lists
        // never builds the archive.
        let (Some(&ordinal), Ok(header)) = (
            self.closed.index.first.get(&source_index),
            self.carrier.header.as_ref(),
        ) else {
            return Ok(Rc::new(StateModel::Unavailable));
        };
        if let Some(model) = self.decoded.borrow().get(&ordinal) {
            return Ok(Rc::clone(model));
        }
        let model = Rc::new(self.decode(ctx, header, ordinal)?);
        // A bounded request can succeed after the cap frees, so only a
        // decided outcome is kept.
        if !matches!(*model, StateModel::Bounded) {
            self.decoded.borrow_mut().insert(ordinal, Rc::clone(&model));
        }
        Ok(model)
    }
}

#[cfg(test)]
mod tests;
