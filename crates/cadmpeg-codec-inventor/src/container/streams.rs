// SPDX-License-Identifier: Apache-2.0
//! Open outcomes for the compound streams read by Inventor.

use std::collections::BTreeMap;

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamEntry, CompoundStreamId};
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpPart, Part};

#[derive(Default)]
pub(crate) struct Streams<'a> {
    opened: BTreeMap<CompoundStreamId, View<'a>>,
    skipped: BTreeMap<CompoundStreamId, Skipped>,
}

struct Skipped {
    kind: &'static str,
    cause: Cause,
    detail: String,
    digest_only: bool,
}

impl Skipped {
    fn error(&self) -> CodecError {
        match self.cause {
            Cause::Unreadable => CodecError::Malformed(self.detail.clone()),
            Cause::Undecoded => CodecError::NotImplemented(self.detail.clone()),
        }
    }
}

impl<'a> Streams<'a> {
    pub(crate) fn open(
        &mut self,
        ctx: &DecodeContext<'a>,
        snapshot: &CompoundSnapshot<'a>,
        stream: &CompoundStreamEntry,
        kind: Option<&'static str>,
    ) -> Result<View<'a>, CodecError> {
        let id = stream.id();
        if let Some(skipped) = self.skipped.get(&id) {
            return Err(skipped.error());
        }
        if let Some(view) = self.opened.get(&id) {
            return Ok(*view);
        }
        match snapshot.open_part(ctx, stream) {
            Ok(view) => {
                self.opened.insert(id, view);
                Ok(view)
            }
            Err(error) => {
                if let Some(kind) = kind {
                    let cause = Cause::Unreadable.of(&error);
                    let detail = match &error {
                        CodecError::Malformed(detail) | CodecError::NotImplemented(detail) => {
                            detail.clone()
                        }
                        _ => return Err(error),
                    };
                    self.skipped.insert(
                        id,
                        Skipped {
                            kind,
                            cause,
                            detail,
                            digest_only: true,
                        },
                    );
                }
                Err(error)
            }
        }
    }

    pub(crate) fn read<T>(
        &mut self,
        ctx: &DecodeContext<'a>,
        snapshot: &CompoundSnapshot<'a>,
        stream: &CompoundStreamEntry,
        kind: &'static str,
        read: impl FnOnce(View<'a>) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let view = self.open(ctx, snapshot, stream, Some(kind))?;
        match ctx.run_part(|| read(view))? {
            Ok(value) => Ok(value),
            Err(error) => {
                let digest_only = matches!(&error, CodecError::ResourceLimit(_));
                let detail = match &error {
                    CodecError::Malformed(detail) | CodecError::NotImplemented(detail) => {
                        detail.clone()
                    }
                    _ => error.to_string(),
                };
                let skipped = Skipped {
                    kind,
                    cause: Cause::Unreadable.of(&error),
                    detail,
                    digest_only,
                };
                let error = if digest_only { skipped.error() } else { error };
                self.skipped.insert(stream.id(), skipped);
                Err(error)
            }
        }
    }

    pub(crate) fn skipped(&self) -> impl Iterator<Item = CompoundStreamId> + '_ {
        self.skipped.keys().copied()
    }

    pub(crate) fn give_ups(
        &self,
        ctx: &DecodeContext<'_>,
        snapshot: &CompoundSnapshot<'_>,
        inspect: bool,
    ) -> Result<Vec<GivenUpPart>, CodecError> {
        self.skipped
            .iter()
            .map(|(id, issue)| {
                let entry = snapshot.stream_by_id(*id).ok_or_else(|| {
                    CodecError::malformed("given-up Inventor stream has no directory entry")
                })?;
                let holds = match issue.kind {
                    "rse-table" | "rse-metadata" | "rse-bulk" => {
                        &[ArenaClass::Model, ArenaClass::Design][..]
                    }
                    _ => &[ArenaClass::Model][..],
                };
                let part = Part::at(
                    "inventor",
                    issue.kind,
                    0,
                    self.opened_bytes(*id),
                    holds.iter().copied(),
                )
                .in_stream(cadmpeg_ir::stream_name!("inventor:").with_suffix(entry.path()));
                let part = if inspect || issue.digest_only {
                    part.digest_only()
                } else {
                    part
                };
                ctx.give_up_part(part, issue.cause, issue.error())
            })
            .collect()
    }

    pub(crate) fn was_skipped(&self, id: CompoundStreamId) -> bool {
        self.skipped.contains_key(&id)
    }

    pub(crate) fn opened_bytes(&self, id: CompoundStreamId) -> &'a [u8] {
        self.opened.get(&id).copied().map_or(&[], View::window)
    }
}
