// SPDX-License-Identifier: Apache-2.0
//! Optional member opening under actual archive and compressed-member scopes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

#[derive(Default)]
pub(super) struct OptionalMembers<'source> {
    pub(super) data: BTreeMap<String, View<'source>>,
    pub(super) failed: BTreeSet<String>,
}

pub(super) fn open_optional<'source>(
    ctx: &DecodeContext<'source>,
    root: View<'source>,
    archive: &ArchiveSnapshot<'source>,
    output: &mut ParserPartOutput,
) -> Result<OptionalMembers<'source>, CodecError> {
    let frames = archive.member_frames(ctx)?;
    for entry in archive.entries() {
        if !crate::native::is_safe_entry_name(&entry.name) {
            return Err(CodecError::malformed(format_args!(
                "unsafe ZIP entry path {:?}",
                entry.name
            )));
        }
    }
    if !archive
        .entries()
        .iter()
        .zip(&frames)
        .any(|(entry, frame)| entry.name != "Document.xml" && frame.is_some())
    {
        return Ok(OptionalMembers::default());
    }
    let parsed = ctx.decode_parser_scope(output, root, spec("zip.archive")?, |stage| {
        let mut members = OptionalMembers::default();
        for (entry, frame) in archive.entries().iter().zip(&frames) {
            if entry.name == "Document.xml" {
                continue;
            }
            let Some(frame) = *frame else {
                // An unprovable local boundary still uses the existing fallback.
                continue;
            };
            let name = entry.name.as_str();
            ctx.retain_bytes(
                u64_from_index(
                    name.len()
                        .checked_add(size_of::<(String, View<'_>)>())
                        .ok_or_else(|| {
                            CodecError::malformed("optional ZIP member index overflows")
                        })?,
                ),
                "FreeCAD optional member source index",
            )?;
            let payload =
                ctx.decode_parser_part(stage, frame.header(), header_spec(ctx, frame)?, || {
                    archive.read_member_header(ctx, frame)
                })?;
            let Some(payload) = payload else {
                members.failed.insert(name.to_owned());
                continue;
            };
            match ctx.decode_parser_part(stage, payload.view(), spec("zip.entry")?, || {
                archive.open_member(ctx, payload)
            })? {
                Some(view) => {
                    members.data.insert(name.to_owned(), view);
                }
                None => {
                    members.failed.insert(name.to_owned());
                }
            }
        }
        Ok(members)
    })?;
    match parsed {
        Some(members) => Ok(members),
        None => {
            let mut failed = BTreeSet::new();
            for (entry, frame) in archive.entries().iter().zip(frames) {
                if entry.name == "Document.xml" || frame.is_none() {
                    continue;
                }
                let name = entry.name.as_str();
                ctx.retain_bytes(
                    u64_from_index(name.len().checked_add(size_of::<String>()).ok_or_else(
                        || CodecError::malformed("failed ZIP member index overflows"),
                    )?),
                    "FreeCAD failed optional member index",
                )?;
                failed.insert(name.to_owned());
            }
            Ok(OptionalMembers {
                data: BTreeMap::new(),
                failed,
            })
        }
    }
}

fn header_spec(
    ctx: &DecodeContext<'_>,
    frame: cadmpeg_container::MemberFrame<'_>,
) -> Result<PartSpec, CodecError> {
    let fields = frame.header_framing()?;
    let bytes = fields
        .len()
        .checked_mul(size_of::<std::ops::Range<u64>>())
        .ok_or_else(|| CodecError::malformed("ZIP local-header framing storage overflows"))?;
    ctx.retain_bytes(u64_from_index(bytes), "FreeCAD local-header framing")?;
    let mut ranges = ctx.alloc_filled(fields.len(), 0..0, "FreeCAD local-header framing")?;
    for (range, field) in ranges.iter_mut().zip(fields) {
        *range = u64_from_index(field.start())..u64_from_index(field.end());
    }
    let level = PartLevel::parse("zip.local_header")
        .ok_or_else(|| CodecError::malformed("ZIP header level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, ranges))
}

fn spec(name: &str) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name)
        .ok_or_else(|| CodecError::malformed("ZIP member level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, Vec::new()))
}

pub(crate) fn append(
    ctx: &DecodeContext<'_>,
    output: ParserPartOutput,
    ir: &mut cadmpeg_ir::CadIr,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let (_, mut native, source, mut diagnostics) = output.into_parts(ctx)?.into_parts();
    for (name, mut records) in std::mem::take(native.namespace_mut("fcstd").arenas_mut()) {
        let destination = ir
            .native
            .namespace_mut("fcstd")
            .arenas_mut()
            .entry(name)
            .or_default();
        let additional = records
            .len()
            .checked_mul(size_of::<cadmpeg_ir::native::NativeRecord>())
            .map(u64_from_index)
            .ok_or_else(|| CodecError::malformed("FreeCAD shared part arena size overflows"))?;
        ctx.retain_bytes(additional, "FreeCAD shared part arena composition")?;
        destination.try_reserve(records.len()).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                additional,
                "FreeCAD shared part arena composition",
            )
        })?;
        destination.append(&mut records);
    }
    fidelity.append(source)?;
    losses.append(&mut diagnostics);
    Ok(())
}

#[cfg(test)]
mod tests;
