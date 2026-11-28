// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` compound directory access.

use cadmpeg_container::compound::{
    CompoundClassId, CompoundDeviation, CompoundEntry, CompoundPrefixProbe, CompoundSnapshot,
    CompoundStreamEntry, CompoundStreamId,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Confidence;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::outcome::PartOutcome;
use cadmpeg_ir::unknown::part::{Cause, GivenUpPart, Part};
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::dialect;
use crate::loss::IronCadLossCode;

/// Stream that holds the kernel record tree.
pub(crate) const BREP_DATA: &str = "Brep Data";

/// The stream that holds the part and assembly records.
pub(crate) const PUBLIC_ACCESS: &str = "Public Access";

/// Root streams that identify a scene, with [`BREP_DATA`].
const SHAPE_DATA: &str = "Shape Data";
pub(crate) const DOCUMENT_DATA: &str = "Document Data";

/// Root storage class of a scene.
pub(crate) const SCENE_CLASS: CompoundClassId = CompoundClassId::from_guid(
    0x0002_c52b,
    0x0000,
    0x0000,
    [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
);

fn evidence<'a>(paths: impl IntoIterator<Item = &'a str>, class: CompoundClassId) -> bool {
    let (mut brep, mut shape, mut document) = (false, false, false);
    for path in paths {
        brep |= path.eq_ignore_ascii_case(BREP_DATA);
        shape |= path.eq_ignore_ascii_case(SHAPE_DATA);
        document |= path.eq_ignore_ascii_case(DOCUMENT_DATA);
    }
    brep && shape && document && class == SCENE_CLASS
}

pub(crate) fn detect(ctx: &DecodeContext<'_>, prefix: View<'_>) -> Result<Confidence, CodecError> {
    Ok(match CompoundPrefixProbe::inspect(ctx, prefix)? {
        CompoundPrefixProbe::DirectoryEvidence { paths, root_class }
            if evidence(paths.iter().map(String::as_str), root_class) =>
        {
            Confidence::High
        }
        _ => Confidence::No,
    })
}

pub(crate) fn open<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<CompoundSnapshot<'a>, CodecError> {
    let snapshot = CompoundSnapshot::new(ctx, root)?;
    if !evidence(
        snapshot.entries().iter().map(CompoundEntry::path),
        snapshot.root_class(),
    ) {
        return Err(CodecError::WrongFormat(
            "IronCAD scene streams or root class are absent".into(),
        ));
    }
    Ok(snapshot)
}

#[derive(Default)]
pub(crate) struct Streams<'a> {
    outcomes: Vec<(CompoundStreamId, PartOutcome<'a>)>,
}

impl<'a> Streams<'a> {
    pub(crate) fn read<T>(
        &mut self,
        ctx: &DecodeContext<'a>,
        snapshot: &CompoundSnapshot<'a>,
        name: &str,
        read: impl FnOnce(View<'a>) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        let Some(entry) = snapshot.stream(name) else {
            return Ok(None);
        };
        let (outcome, value) = match ctx.run_part(|| snapshot.open_part(ctx, entry))? {
            Ok(stream) => {
                let part = stream_part(entry, stream.window()).digest_only();
                match ctx.run_part(|| read(stream))? {
                    Ok(value) => (PartOutcome::admitted(part), Some(value)),
                    Err(error) => (PartOutcome::refused(part, Cause::Unreadable, &error), None),
                }
            }
            Err(error) => {
                let part = stream_part(entry, &[])
                    .digest_chunks(snapshot.stream_chunks(entry)?.map(View::window));
                let outcome = match Cause::Unreadable.of(&error) {
                    Cause::Unreadable => PartOutcome::damaged(part, &error),
                    Cause::Undecoded => PartOutcome::refused(part, Cause::Undecoded, &error),
                };
                (outcome, None)
            }
        };
        self.outcomes.push((entry.id(), outcome));
        Ok(value)
    }

    pub(crate) fn inventory(&mut self, snapshot: &CompoundSnapshot<'a>) -> Result<(), CodecError> {
        for entry in snapshot.entries() {
            let CompoundEntry::Stream(entry) = entry else {
                continue;
            };
            if self.outcomes.iter().any(|(id, _)| *id == entry.id()) {
                continue;
            }
            let part = stream_part(entry, &[])
                .digest_chunks(snapshot.stream_chunks(entry)?.map(View::window));
            let outcome = match snapshot.validate_stream(entry) {
                Ok(()) => PartOutcome::inventory(part),
                Err(error) => PartOutcome::damaged(part, &error),
            };
            self.outcomes.push((entry.id(), outcome));
        }
        Ok(())
    }

    pub(crate) fn reports(
        &self,
        ctx: &DecodeContext<'_>,
        snapshot: &CompoundSnapshot<'_>,
    ) -> Result<(Vec<LossNote>, Vec<GivenUpPart>), CodecError> {
        let losses = snapshot
            .deviations()
            .filter(|deviation| {
                !matches!(deviation, CompoundDeviation::StreamChainBroken(id)
                    if self.outcomes.iter().any(|(stream, outcome)| stream == id && outcome.framing_failed()))
            })
            .map(|deviation| IronCadLossCode::ContainerNoncanonical.note(deviation.description()))
            .collect();
        let mut parts = Vec::new();
        for (_, outcome) in &self.outcomes {
            parts.extend(outcome.give_up(ctx, true)?);
        }
        Ok((losses, parts))
    }
}

fn stream_part<'a>(entry: &CompoundStreamEntry, bytes: &'a [u8]) -> Part<'a> {
    let path = entry.path();
    let part = Part::at("ironcad", "stream", 0, bytes, [ArenaClass::Model])
        .named(path)
        .in_stream(cadmpeg_ir::stream_name!("ironcad:").with_suffix(path));
    if path.eq_ignore_ascii_case(DOCUMENT_DATA) {
        part.costing("the root definition states no units")
    } else if path.eq_ignore_ascii_case(PUBLIC_ACCESS) {
        part.costing("bodies stay in their part frames")
    } else {
        part
    }
}

pub(crate) fn summary(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> Result<ContainerSummary, CodecError> {
    let snapshot = open(ctx, root)?;
    let mut streams = Streams::default();
    streams.inventory(&snapshot)?;
    let (mut losses, parts) = streams.reports(ctx, &snapshot)?;
    losses.extend(parts.iter().map(GivenUpPart::loss));
    Ok(ContainerSummary::classified(
        dialect::layers(),
        ContainerKind::Cfb,
        snapshot.container_entries(|entry| match entry {
            CompoundEntry::Storage(_) => ContainerRole::Storage,
            CompoundEntry::Stream(_) => ContainerRole::Stream,
        }),
        losses,
        Vec::new(),
    ))
}
