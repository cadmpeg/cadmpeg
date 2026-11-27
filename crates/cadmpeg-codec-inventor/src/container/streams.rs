// SPDX-License-Identifier: Apache-2.0
//! Read outcomes for Inventor compound streams and their independently framed parts.

use std::collections::BTreeMap;

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamEntry, CompoundStreamId};
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::unknown::part::outcome::PartOutcome;
use cadmpeg_ir::unknown::part::{Cause, GivenUpPart, Part};

#[derive(Default)]
pub(crate) struct Streams<'a> {
    outcomes: BTreeMap<CompoundStreamId, PartOutcome<'a>>,
    children: Vec<PartOutcome<'a>>,
}

impl<'a> Streams<'a> {
    pub(crate) fn has_outcome(&self, id: CompoundStreamId) -> bool {
        self.outcomes.contains_key(&id)
    }

    pub(crate) fn read<T>(
        &mut self,
        ctx: &DecodeContext<'a>,
        snapshot: &CompoundSnapshot<'a>,
        entry: &CompoundStreamEntry,
        kind: &'static str,
        read: impl FnOnce(View<'a>) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let view = match ctx.run_part(|| snapshot.open_part(ctx, entry))? {
            Ok(view) => view,
            Err(error) => {
                let part = stream_part(entry, kind, &[])
                    .digest_chunks(snapshot.stream_chunks(entry)?.map(View::window));
                let outcome = match Cause::Unreadable.of(&error) {
                    Cause::Unreadable => PartOutcome::damaged(part, &error),
                    Cause::Undecoded => PartOutcome::refused(part, Cause::Undecoded, &error),
                };
                self.outcomes.insert(entry.id(), outcome);
                return Err(decode_error(error));
            }
        };
        let result = ctx.run_part(|| read(view))?;
        let part = stream_part(entry, kind, view.window());
        let outcome = match &result {
            Ok(_) => PartOutcome::admitted(part),
            Err(error @ CodecError::ResourceLimit(_)) => {
                PartOutcome::refused(part.digest_only(), Cause::Undecoded, error)
            }
            Err(error) => PartOutcome::refused(part, Cause::Unreadable, error),
        };
        self.outcomes.insert(entry.id(), outcome);
        result.map_err(decode_error)
    }

    pub(crate) fn inventory(
        &mut self,
        snapshot: &CompoundSnapshot<'a>,
        entry: &CompoundStreamEntry,
        kind: &'static str,
        unread: Option<CodecError>,
    ) -> Result<(), CodecError> {
        let part = stream_part(entry, kind, &[]);
        let outcome = match (snapshot.validate_stream(entry), unread) {
            (Ok(()), None) => PartOutcome::inventory(part),
            (Ok(()), Some(error)) => PartOutcome::refused(
                part.digest_chunks(snapshot.stream_chunks(entry)?.map(View::window)),
                Cause::Unreadable,
                &error,
            ),
            (Err(error), _) => PartOutcome::damaged(
                part.digest_chunks(snapshot.stream_chunks(entry)?.map(View::window)),
                &error,
            ),
        };
        self.outcomes.insert(entry.id(), outcome);
        Ok(())
    }

    pub(crate) fn add(&mut self, outcomes: impl IntoIterator<Item = PartOutcome<'a>>) {
        self.children.extend(outcomes);
    }

    pub(crate) fn failure(&self, id: CompoundStreamId) -> Option<(Cause, &str)> {
        self.outcomes.get(&id)?.failure()
    }

    pub(crate) fn damaged(&self) -> impl Iterator<Item = CompoundStreamId> + '_ {
        self.outcomes
            .iter()
            .filter(|(_, outcome)| outcome.framing_failed())
            .map(|(id, _)| *id)
    }

    pub(crate) fn give_ups(
        &self,
        ctx: &DecodeContext<'_>,
        inspect: bool,
    ) -> Result<Vec<GivenUpPart>, CodecError> {
        let mut parts = Vec::new();
        for outcome in self.outcomes.values().chain(&self.children) {
            parts.extend(outcome.give_up(ctx, inspect)?);
        }
        Ok(parts)
    }
}

fn stream_part<'a>(entry: &CompoundStreamEntry, kind: &'static str, bytes: &'a [u8]) -> Part<'a> {
    let holds = match kind {
        "rse-table" | "rse-metadata" | "rse-bulk" => &[ArenaClass::Model, ArenaClass::Design][..],
        _ => &[ArenaClass::Model][..],
    };
    Part::at("inventor", kind, 0, bytes, holds.iter().copied())
        .in_stream(cadmpeg_ir::stream_name!("inventor:").with_suffix(entry.path()))
}

fn decode_error(error: CodecError) -> CodecError {
    match error {
        error @ CodecError::ResourceLimit(_) => CodecError::NotImplemented(error.to_string()),
        error => error,
    }
}
