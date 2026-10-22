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
    let mut frames = Vec::new();
    for entry in archive
        .entries()
        .iter()
        .filter(|entry| entry.name != "Document.xml")
    {
        if !crate::native::is_safe_entry_name(&entry.name) {
            return Err(CodecError::malformed(format_args!(
                "unsafe ZIP entry path {:?}",
                entry.name
            )));
        }
        let Some(start) = entry.data_start else {
            // The legacy unframed-header recovery remains outside zip.entry.
            continue;
        };
        let end = entry.data_end()?;
        let absolute = |offset| {
            usize::try_from(offset)
                .ok()
                .and_then(|offset| root.start().checked_add(offset))
        };
        let extent = absolute(start)
            .zip(absolute(end))
            .and_then(|(start, end)| root.child(start, end))
            .ok_or_else(|| {
                CodecError::malformed("ZIP member frame escapes its actual archive View")
            })?;
        let additional = size_of::<(&str, View<'_>)>();
        ctx.retain_bytes(
            u64_from_index(additional),
            "FreeCAD optional member frame index",
        )?;
        frames.try_reserve(1).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(additional),
                "FreeCAD optional member frame index",
            )
        })?;
        frames.push((entry.name.as_str(), extent));
    }
    if frames.is_empty() {
        return Ok(OptionalMembers::default());
    }
    let parsed = ctx.decode_parser_scope(output, root, spec("zip.archive")?, |stage| {
        let mut members = OptionalMembers::default();
        for (name, extent) in &frames {
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
            match ctx.decode_parser_part(stage, *extent, spec("zip.entry")?, || {
                archive.open(ctx, name)
            })? {
                Some(view) => {
                    members.data.insert((*name).to_owned(), view);
                }
                None => {
                    members.failed.insert((*name).to_owned());
                }
            }
        }
        Ok(members)
    })?;
    match parsed {
        Some(members) => Ok(members),
        None => {
            let mut failed = BTreeSet::new();
            for (name, _) in frames {
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
    let (_, mut native, source, mut diagnostics) = output.into_parts().into_parts();
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
