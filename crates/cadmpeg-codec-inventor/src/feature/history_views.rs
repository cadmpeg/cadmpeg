// SPDX-License-Identifier: Apache-2.0
//! Evidence-only views of history states
//! ([spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! A solid loft names its caps and side faces in the history state that its
//! naming tag first appears in. Later features can remove or copy those faces
//! in the active body. The view decodes that state into a scratch model that
//! the loft result check reads. The evidence stays in the view cache, and the
//! nest cap and nest work budget of [`super::history_bodies`] do not apply to
//! it. The decode of one state into a scratch model is shared with the nests.
//! A stored-body Base result that the final model no longer holds reads the
//! same view.

use cadmpeg_asm::brep::inspect_with_header;
use cadmpeg_asm::brep::transfer::{transfer_inspection, InspectionGeometry};
use cadmpeg_asm::history::{materialize_record_table, walk_entity_versions_until};
use cadmpeg_asm::kernel_header::BinaryHeader;
use cadmpeg_asm::sab::Record;
use cadmpeg_core::decode::{
    u64_from_index, work_units, DecodeContext, ResourceLimit, ScratchContext, WorkBudget,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::document::Topology;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::ops::ControlFlow;
use std::rc::Rc;

use super::history_states::Closed;
use crate::kernel::ActiveCarrier;

/// Cap of records materialized for all views of a part.
pub(crate) const MAX_STATE_VIEW_WORK: usize = 1 << 21;

/// Cap of cached state outcomes per part.
pub(crate) const MAX_STATE_VIEW_MODELS: usize = 4;

/// The model of one history state.
pub(crate) enum StateModel {
    /// The state decoded.
    Model(Box<InspectionGeometry>),
    /// No state lists the feature, or the carrier has no archive to read.
    Unavailable,
    /// The state lists the feature, and it does not materialize, decode or
    /// transfer.
    Failed,
    /// The state view reached a work or decode resource limit.
    Bounded(StateLimit),
}

/// The limit that gave up a state view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateLimit {
    Work,
    Resource(ResourceLimit),
}

impl fmt::Display for StateLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Work => f.write_str("the local work cap of state views"),
            Self::Resource(limit) => write!(
                f,
                "{} during {}",
                CodecError::ResourceLimit(*limit),
                limit.operation
            ),
        }
    }
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
            return Ok(StateModel::Bounded(StateLimit::Work));
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
        let state = decode_scratch(ctx, header, archive, &versions, ordinal, |_, mut state| {
            keep_view_geometry(&mut state);
            Ok(Box::new(state))
        })?;
        Ok(match state {
            Decoded::Scratch(state) => match admit_view(ctx, &state) {
                Ok(()) => StateModel::Model(state),
                Err(limit) => StateModel::Bounded(StateLimit::Resource(limit)),
            },
            Decoded::Failed => StateModel::Failed,
            Decoded::Bounded(limit) => StateModel::Bounded(StateLimit::Resource(limit)),
        })
    }
}

pub(crate) const LEGACY_CAP_CLASS: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
pub(crate) const LEGACY_SIDE_CLASS: &str =
    "NMx_Loft_Surface_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";

fn keep_view_geometry(state: &mut InspectionGeometry) {
    let Topology {
        bodies,
        regions,
        shells,
        faces,
        surfaces,
        procedural_surfaces,
        ..
    } = std::mem::take(&mut state.geometry);
    state.geometry = Topology {
        bodies,
        regions,
        shells,
        faces,
        surfaces,
        procedural_surfaces,
        ..Topology::default()
    };
    state.attributes.retain_mut(|attribute| {
        if !matches!(attribute.target.entity, AttributeTarget::Face(_))
            || !matches!(
                attribute.name.as_str(),
                "ATTRIB_CUSTOM-attrib" | LEGACY_CAP_CLASS | LEGACY_SIDE_CLASS
            )
        {
            return false;
        }
        for value in attribute.values.iter_mut().take(5) {
            *value = None;
        }
        true
    });
    for key in &mut state.body_keys {
        key.source_brep = None;
    }
}

fn admit_view(ctx: &DecodeContext<'_>, evidence: &InspectionGeometry) -> Result<(), ResourceLimit> {
    let geometry = &evidence.geometry;
    let names = geometry
        .bodies
        .iter()
        .filter_map(|body| body.name.as_deref())
        .chain(
            geometry
                .faces
                .iter()
                .filter_map(|face| face.name.as_deref()),
        );
    let mut bytes: usize = names.map(str::len).sum();
    for attribute in &evidence.attributes {
        bytes += attribute.name.len() + std::mem::size_of_val(attribute.values.as_slice());
        for value in attribute.values.iter().flatten() {
            bytes += match value {
                AttributeValue::String(text) | AttributeValue::Reference(text) => text.len(),
                AttributeValue::Vector(values) => std::mem::size_of_val(values.as_slice()),
                _ => 0,
            };
        }
    }
    let entities = geometry.entity_count() + evidence.attributes.len() + evidence.body_keys.len();
    ctx.admit_optional(
        u64_from_index(bytes),
        u64_from_index(entities),
        "keep Inventor history state evidence",
    )
}

/// The outcome of the decode of one history state.
pub(crate) enum Decoded<T> {
    Scratch(T),
    /// The state does not materialize, decode or transfer.
    Failed,
    /// The decode reached a resource limit.
    Bounded(ResourceLimit),
}

/// Read the state at `ordinal` and let the caller extract its evidence.
pub(crate) fn decode_scratch<T>(
    ctx: &DecodeContext<'_>,
    header: &BinaryHeader,
    archive: &HashMap<i64, Record>,
    versions: &BTreeMap<i64, i64>,
    ordinal: u32,
    use_state: impl for<'s> FnOnce(&ScratchContext<'s>, InspectionGeometry) -> Result<T, CodecError>,
) -> Result<Decoded<T>, CodecError> {
    let Some(table) = materialize_record_table(versions, |revision| archive.get(&revision)) else {
        return Ok(Decoded::Failed);
    };
    let input_bytes = table
        .iter()
        .try_fold(0_usize, |total, record| total.checked_add(record.len))
        .ok_or_else(|| CodecError::malformed("Inventor history source extents exceed usize"))?;
    let stream = format!("inventor:history:{ordinal}");
    let decoded = ctx.scratch(input_bytes, |scratch| {
        let brep = inspect_with_header(
            scratch,
            &table,
            Some(&header.metadata),
            &stream,
            cadmpeg_asm::asm_format!("inventor"),
        )?;
        use_state(scratch, transfer_inspection(scratch, brep)?)
    });
    match decoded {
        Ok(Ok(state)) => Ok(Decoded::Scratch(state)),
        Ok(Err(limit)) => Ok(Decoded::Bounded(limit)),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(_) => Ok(Decoded::Failed),
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
        keep(
            &mut self.decoded.borrow_mut(),
            MAX_STATE_VIEW_MODELS,
            ordinal,
            &model,
        );
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
