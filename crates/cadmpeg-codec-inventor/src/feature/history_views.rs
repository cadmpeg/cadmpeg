// SPDX-License-Identifier: Apache-2.0
//! Evidence-only views of history states
//! ([spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! A solid loft names its caps and side faces in the history state that its
//! naming tag first appears in. Later features can remove or copy those faces
//! in the active body. The view decodes that state into a scratch model that
//! the loft result check reads. It adds no record to the document, and the
//! nest cap and nest work budget of [`super::history_bodies`] do not apply to
//! it. The decode of one state into a scratch model is shared with the nests.
//! A stored-body Base result that the final model no longer holds reads the
//! same view.

use cadmpeg_asm::brep::annotations::AnnotationRecord;
use cadmpeg_asm::brep::{decode_with_header, DecodePurpose};
use cadmpeg_asm::history::{materialize_record_table, walk_entity_versions_until};
use cadmpeg_asm::kernel_header::BinaryHeader;
use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::{work_units, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::UnknownRecord;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::ControlFlow;
use std::rc::Rc;

use super::history_states::Closed;
use crate::kernel::ActiveCarrier;

/// Cap of records materialized for all views of a part.
pub(crate) const MAX_STATE_VIEW_WORK: usize = 1 << 21;

/// Cap of decoded state models that the views of a part keep. Each model is a
/// whole scratch document, so the cache holds a few at once, which covers
/// lofts that share a state. A request past the cap drops the cached models
/// first and decodes again, which the work cap bounds.
pub(crate) const MAX_STATE_VIEW_MODELS: usize = 4;

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

    /// Whether the oldest state whose live naming tags list `source_index`
    /// holds a body record with `key` and the next older state holds none.
    fn introduces(&self, source_index: u32, key: i64) -> bool;

    /// The model of the state before the one `after` selects, which is the
    /// next older state. `Unavailable` when no state lists `source_index` or
    /// the listing state is the oldest.
    fn before(
        &self,
        _ctx: &DecodeContext<'_>,
        _source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        Ok(Rc::new(StateModel::Unavailable))
    }
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
        let Some(versions) = versions else {
            return Ok(StateModel::Failed);
        };
        Ok(
            match decode_scratch(
                ctx,
                self.carrier.bytes.window(),
                header,
                archive,
                &versions,
                ordinal,
            )? {
                Some(scratch) => StateModel::Model(Box::new(scratch.model)),
                None => StateModel::Failed,
            },
        )
    }
}

/// The scratch model of one history state.
pub(crate) struct Scratch {
    /// Native body keys of the bodies of the state, by body identity.
    pub(crate) body_keys: Vec<(String, u64)>,
    /// The decoded and transferred state.
    pub(crate) model: CadIr,
    /// The records of the state that the transfer keeps undecoded.
    pub(crate) unknowns: Vec<UnknownRecord>,
    /// The source location of each record of `unknowns`.
    pub(crate) provenance: Vec<AnnotationRecord>,
}

/// Materialize the records of the state at `ordinal` from its `versions` and
/// transfer them into a scratch model. `None` when the state does not
/// materialize, decode or transfer. Only a resource limit is an error.
pub(crate) fn decode_scratch(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &BinaryHeader,
    archive: &HashMap<i64, Record>,
    versions: &BTreeMap<i64, i64>,
    ordinal: u32,
) -> Result<Option<Scratch>, CodecError> {
    let Some(table) = materialize_record_table(versions, |revision| archive.get(&revision)) else {
        return Ok(None);
    };
    let stream = format!("inventor:history:{ordinal}");
    let brep = match decode_with_header(
        ctx,
        &table,
        bytes,
        Some(&header.metadata),
        &stream,
        cadmpeg_asm::asm_format!("inventor"),
        DecodePurpose::Inspect,
    ) {
        Ok(brep) => brep,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    drop(table);
    let body_keys = brep
        .body_native_keys
        .iter()
        .filter_map(|record| Some((record.body.as_str().to_owned(), record.asm_body_key?)))
        .collect();
    let mut model = CadIr::empty();
    match cadmpeg_asm::brep::transfer::transfer_into_ir(
        ctx,
        &mut model,
        cadmpeg_ir::codec_id!("inventor"),
        brep,
    ) {
        Ok((_, remainder)) => {
            let ids = remainder
                .unknowns
                .iter()
                .map(|record| record.id().as_str())
                .collect::<HashSet<_>>();
            let provenance = remainder
                .annotation_records
                .into_iter()
                .filter(|record| ids.contains(record.id.as_str()))
                .collect();
            Ok(Some(Scratch {
                body_keys,
                model,
                unknowns: remainder.unknowns,
                provenance,
            }))
        }
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(None),
    }
}

impl StateViews<'_, '_> {
    /// The model of the state at `ordinal`, shared between requests.
    fn model_at(
        &self,
        ctx: &DecodeContext<'_>,
        header: &BinaryHeader,
        ordinal: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        if let Some(model) = self.decoded.borrow().get(&ordinal) {
            return Ok(Rc::clone(model));
        }
        let model = Rc::new(self.decode(ctx, header, ordinal)?);
        // A bounded request can succeed after the cap frees, so only a
        // decided outcome is kept.
        if !matches!(*model, StateModel::Bounded) {
            keep(
                &mut self.decoded.borrow_mut(),
                MAX_STATE_VIEW_MODELS,
                ordinal,
                &model,
            );
        }
        Ok(model)
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
        self.model_at(ctx, header, ordinal)
    }

    fn before(
        &self,
        ctx: &DecodeContext<'_>,
        source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        let (Some(ordinal), Ok(header)) = (
            self.closed
                .index
                .first
                .get(&source_index)
                .and_then(|ordinal| ordinal.checked_sub(1)),
            self.carrier.header.as_ref(),
        ) else {
            return Ok(Rc::new(StateModel::Unavailable));
        };
        self.model_at(ctx, header, ordinal)
    }

    fn introduces(&self, source_index: u32, key: i64) -> bool {
        let index = &self.closed.index;
        let Some(&ordinal) = index.first.get(&source_index) else {
            return false;
        };
        let holds = |ordinal: u32| {
            usize::try_from(ordinal)
                .ok()
                .and_then(|ordinal| index.states.get(ordinal))
                .is_some_and(|state| state.body_keys.binary_search(&key).is_ok())
        };
        ordinal
            .checked_sub(1)
            .is_some_and(|before| holds(ordinal) && !holds(before))
    }
}

/// Keep `model` for `ordinal` in `cache`, which holds at most `limit` models.
/// A full cache drops its models first.
fn keep(
    cache: &mut HashMap<u32, Rc<StateModel>>,
    limit: usize,
    ordinal: u32,
    model: &Rc<StateModel>,
) {
    if cache.len() >= limit {
        cache.clear();
    }
    cache.insert(ordinal, Rc::clone(model));
}

#[cfg(test)]
mod tests;
