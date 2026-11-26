// SPDX-License-Identifier: Apache-2.0
//! The topology records that emission leaves out of the document.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{dropped_with, GivenUpPart};

use crate::framing::node_kind::NodeKind;
use crate::topology::Node;

/// Each record of one stream that emission leaves out, by its kind and XMT
/// index. A record given up as a part of the stream maps to the part's stub
/// id. A record dropped because it needs another maps to its own id. A later
/// charge names either id as the record it needs.
pub(super) struct LeftOut<'ctx, 'data, 'stream> {
    ctx: &'ctx DecodeContext<'data>,
    stream: (usize, &'stream cadmpeg_ir::StreamName),
    ids: BTreeMap<(NodeKind, u32), Identity>,
    losses: Vec<LossNote>,
    given_up: Vec<GivenUpPart>,
}

impl<'ctx, 'data, 'stream> LeftOut<'ctx, 'data, 'stream> {
    pub(super) fn new(
        ctx: &'ctx DecodeContext<'data>,
        stream: (usize, &'stream cadmpeg_ir::StreamName),
    ) -> Self {
        Self {
            ctx,
            stream,
            ids: BTreeMap::new(),
            losses: Vec::new(),
            given_up: Vec::new(),
        }
    }

    pub(super) fn get(&self, kind: NodeKind, xmt: u32) -> Option<&Identity> {
        self.ids.get(&(kind, xmt))
    }

    /// Enters a record that another site gave up as the part `id`.
    pub(super) fn insert(&mut self, kind: NodeKind, xmt: u32, id: Identity) {
        self.ids.entry((kind, xmt)).or_insert(id);
    }

    /// Gives up `node` for `why`, unless an earlier site left it out, and
    /// returns the id a charge names for it.
    pub(super) fn give_up(
        &mut self,
        kind: NodeKind,
        node: &Node,
        why: LossNote,
    ) -> Result<Identity, CodecError> {
        if let Some(id) = self.ids.get(&(kind, node.xmt())) {
            return Ok(id.clone());
        }
        let part = crate::decode::build::give_up_record(self.ctx, self.stream, node, &why)?;
        let id = Identity::from(part.id().clone());
        self.given_up.push(part);
        self.losses.push(why);
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
        self.ids.insert((kind, xmt), record);
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
                self.give_up(kind, node, why())?;
            }
        }
        Ok(())
    }

    /// Names `record`, which emission built for the left-out record `key`, as
    /// dropped with the part that `key` maps to, so the records that
    /// reference `record` leave the document with it.
    pub(super) fn release_with_entry(&mut self, key: (NodeKind, u32), record: impl Into<Identity>) {
        let record = record.into();
        if let Some(entry) = self.ids.get(&key).filter(|entry| **entry != record) {
            self.losses.push(dropped_with(record, entry));
        }
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

    /// Moves the charges into the decode's losses and given-up parts.
    pub(super) fn finish(self, losses: &mut Vec<LossNote>, given_up: &mut Vec<GivenUpPart>) {
        losses.extend(self.losses);
        given_up.extend(self.given_up);
    }
}
