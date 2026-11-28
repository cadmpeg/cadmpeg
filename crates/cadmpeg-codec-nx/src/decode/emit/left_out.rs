// SPDX-License-Identifier: Apache-2.0
//! The topology records that emission leaves out of the document.

use std::collections::BTreeMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{dropped_with, Cause};

use crate::decode::build::PendingParts;
use crate::framing::node_kind::NodeKind;
use crate::topology::{Graph, Node};

/// Each record of one stream that emission leaves out, by its kind and XMT
/// index. A record given up as a part of the stream maps to the part's stub
/// id. A record dropped because it needs another maps to its own id. A later
/// charge names either id as the record it needs.
pub(super) struct LeftOut<'parts, 'stream> {
    parts: &'parts mut PendingParts,
    stream: (usize, &'stream cadmpeg_ir::StreamName),
    ids: BTreeMap<(NodeKind, u32), Identity>,
    /// The record that each dropped record needs.
    needs: BTreeMap<Identity, Identity>,
    losses: Vec<LossNote>,
}

impl<'parts, 'stream> LeftOut<'parts, 'stream> {
    /// The ledger of `stream`, which starts with each record of the stream
    /// that `parts` gives up and `graph` does not hold.
    pub(super) fn new(
        parts: &'parts mut PendingParts,
        stream: (usize, &'stream cadmpeg_ir::StreamName),
        graph: &Graph,
    ) -> Self {
        let ids = parts
            .records_of(stream.0)
            .filter(|((kind, xmt), _)| graph.get(*kind, *xmt).is_none())
            .map(|(key, id)| (key, id.clone()))
            .collect();
        Self {
            parts,
            stream,
            ids,
            needs: BTreeMap::new(),
            losses: Vec::new(),
        }
    }

    pub(super) fn get(&self, kind: NodeKind, xmt: u32) -> Option<&Identity> {
        self.ids.get(&(kind, xmt))
    }

    /// The id a charge names for the left-out record `xmt` of any kind.
    pub(super) fn get_xmt(&self, xmt: u32) -> Option<&Identity> {
        self.ids
            .iter()
            .find_map(|((_, left_out), id)| (*left_out == xmt).then_some(id))
    }

    /// Gives up `node` with the text of `why`, unless an earlier site left
    /// it out, and returns the id a charge names for it. Emission leaves a
    /// record out when a record that it names is not emitted or its fields
    /// have no reading, so the give-up is [`Cause::Undecoded`].
    pub(super) fn give_up(
        &mut self,
        kind: NodeKind,
        node: &Node,
        why: &LossNote,
    ) -> Result<Identity, CodecError> {
        if let Some(id) = self.ids.get(&(kind, node.xmt())) {
            return Ok(id.clone());
        }
        let id = self
            .parts
            .push_record(self.stream, node, (Cause::Undecoded, why))?;
        self.ids.insert((kind, node.xmt()), id.clone());
        Ok(id)
    }

    /// Drops `record`, entered as `kind` `xmt`, with the left-out record
    /// `needs` names.
    pub(super) fn drop_with(
        &mut self,
        (kind, xmt): (NodeKind, u32),
        record: impl Into<Identity>,
        needs: &Identity,
    ) {
        if self.ids.contains_key(&(kind, xmt)) {
            return;
        }
        let record = record.into();
        self.losses.push(dropped_with(record.clone(), needs));
        self.needs.insert(record.clone(), needs.clone());
        self.ids.insert((kind, xmt), record);
    }

    /// Names `record`, which emission states partial because it loses the
    /// left-out record `key`, among the records that the charge of `key`
    /// marks.
    pub(super) fn mark(&mut self, key: (NodeKind, u32), record: impl Into<Identity>) {
        let Some(mut root) = self.ids.get(&key).cloned() else {
            return;
        };
        for _ in 0..=self.needs.len() {
            match self.needs.get(&root) {
                Some(needs) => root = needs.clone(),
                None => break,
            }
        }
        self.parts.mark(root, record.into());
    }

    /// Drops `record` with the left-out record that `needs` names, or else
    /// gives up `node` for `why`.
    pub(super) fn drop_with_or_give_up(
        &mut self,
        (kind, node): (NodeKind, &Node),
        record: impl Into<Identity>,
        needs: Option<(NodeKind, u32)>,
        why: impl FnOnce() -> LossNote,
    ) -> Result<(), CodecError> {
        match needs.and_then(|(kind, xmt)| self.get(kind, xmt)).cloned() {
            Some(needs) => self.drop_with((kind, node.xmt()), record, &needs),
            None => {
                self.give_up(kind, node, &why())?;
            }
        }
        Ok(())
    }

    /// Drops `record`, which emission built from no stream record of its
    /// own, with the left-out record that `needs` names. Without such a
    /// record, `record` is scaffolding that no record of the stream carries.
    pub(super) fn drop_built(
        &mut self,
        record: impl Into<Identity>,
        needs: Option<(NodeKind, u32)>,
    ) {
        if let Some(needs) = needs.and_then(|(kind, xmt)| self.get(kind, xmt)).cloned() {
            self.losses.push(dropped_with(record, &needs));
        }
    }

    /// Moves the notes into the decode's losses.
    pub(super) fn finish(self, losses: &mut Vec<LossNote>) {
        losses.extend(self.losses);
    }
}
