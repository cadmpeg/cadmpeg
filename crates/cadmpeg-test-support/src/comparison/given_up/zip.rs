// SPDX-License-Identifier: Apache-2.0
//! Actual ZIP-source correspondence for source-backed partial decode comparisons.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, SourceOrigin, View};
use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::source_fidelity::parts::{PartFraming, PartState};

use super::{Allowances, DecodeSnapshot};
use crate::framing::{zip, PartTable};

struct Member<'a> {
    method: u16,
    stream: String,
    source: View<'a>,
    entry: (String, u64, u64),
    header: (u64, u64),
}
struct Sources<'a> {
    root: View<'a>,
    stream: String,
    members: BTreeMap<Vec<u8>, Member<'a>>,
    incomplete_header: Option<(u64, u64)>,
}

impl<'a> Sources<'a> {
    fn root(
        snapshot: DecodeSnapshot<'_>,
        ctx: &DecodeContext<'_>,
        root: View<'a>,
    ) -> Result<Self, String> {
        ctx.require_source(root)
            .map_err(|error| error.to_string())?;
        let capture = ctx
            .capture_part_extent(root)
            .map_err(|error| error.to_string())?;
        if capture.offset() != 0
            || !ctx
                .source_streams()
                .map_err(|error| error.to_string())?
                .iter()
                .any(|stream| {
                    stream.name() == capture.stream()
                        && stream.byte_len() == capture.byte_len()
                        && stream.origin() == &SourceOrigin::Root
                })
        {
            return Err("ZIP correspondence needs the actual complete physical root".into());
        }
        let images = snapshot
            .fidelity
            .retained_records()
            .values()
            .filter(|image| {
                image.stream() == capture.stream()
                    && image.offset() == 0
                    && image.byte_len() == capture.byte_len()
                    && image.data() == Some(root.window())
            })
            .count();
        if images != 1 {
            return Err("ZIP source differs from its snapshot's unique retained root image".into());
        }
        if snapshot
            .fidelity
            .parts()
            .values()
            .filter(|part| {
                part.level().as_str() == "zip.archive"
                    && part.parent().is_none()
                    && part.stream() == capture.stream()
                    && part.offset() == 0
                    && part.byte_len() == capture.byte_len()
            })
            .count()
            != 1
        {
            return Err("ZIP source has no unique actual archive extent in its snapshot".into());
        }
        Ok(Self {
            root,
            stream: capture.stream().to_owned(),
            members: BTreeMap::new(),
            incomplete_header: None,
        })
    }

    fn members(&mut self, ctx: &DecodeContext<'a>, prefix: bool) -> Result<(), String> {
        let mut frames = Vec::new();
        let entries = match zip::scan(ctx, self.root, &mut frames)? {
            zip::ZipScan::Complete { entries } => entries.into_iter()
                .map(|entry| (entry.name(), entry.method(), entry.frame_index(), entry.payload())).collect::<Vec<_>>(),
            zip::ZipScan::Local { entries, stop } if prefix && stop.reason() == zip::RecoveryReason::HeaderIncomplete => {
                let incomplete = stop.frame_index().and_then(|index| frames.get(index))
                    .ok_or("recovered ZIP prefix has no independent incomplete header")?;
                self.incomplete_header = Some((incomplete.capture().offset(), incomplete.capture().byte_len()));
                entries.into_iter().map(|entry| (entry.name(), entry.method(), entry.frame_index(), entry.payload())).collect()
            }
            _ => return Err("ZIP correspondence needs a complete directory or authenticated incomplete-header prefix".into()),
        };
        PartTable::load()?.validate(ctx, &frames)?;
        let archive = cadmpeg_container::ArchiveSnapshot::new(ctx, self.root)
            .map_err(|error| error.to_string())?;
        for (name_source, method, frame_index, payload) in entries {
            let raw = name_source.window();
            let name = std::str::from_utf8(raw)
                .map_err(|_| "ZIP correspondence member name is not UTF-8")?;
            let source = archive.open(ctx, name).map_err(|error| error.to_string())?;
            let capture = ctx
                .capture_part_extent(source)
                .map_err(|error| error.to_string())?;
            if capture.offset() != 0 {
                return Err("ZIP member source does not start at zero".into());
            }
            if self
                .members
                .insert(
                    raw.to_vec(),
                    Member {
                        method,
                        stream: capture.stream().to_owned(),
                        source,
                        entry: (
                            frames[frame_index].capture().stream().to_owned(),
                            frames[frame_index].capture().offset(),
                            frames[frame_index].capture().byte_len(),
                        ),
                        header: frames
                            .iter()
                            .find(|part| {
                                part.level().as_str() == "zip.local_header"
                                    && part.view().end() == payload.start()
                            })
                            .map(|part| (part.capture().offset(), part.capture().byte_len()))
                            .ok_or("ZIP member has no independent preceding local header")?,
                    },
                )
                .is_some()
            {
                return Err("ZIP correspondence has duplicate member names".into());
            }
        }
        Ok(())
    }

    fn resolve(&self, stream: &str, range: Range<u64>) -> Option<&'a [u8]> {
        let source = if stream == self.stream {
            self.root
        } else {
            self.members
                .values()
                .find(|member| member.stream == stream)?
                .source
        };
        source
            .window()
            .get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
    }
}

impl Allowances {
    /// Compare actual ZIP members even when recompression changes their source names.
    ///
    /// Source views must be their snapshots' complete retained physical inputs.
    /// Only equal member names and methods establish correspondence. Outside a
    /// failed part, changed bytes in the same member need B1 coverage and no
    /// semantic read or framing overlap in both snapshots. Those bytes grant no
    /// record-change permission. A shortened member must be an exact prefix,
    /// with its cut inside a genuinely given-up source part.
    ///
    /// # Errors
    /// Refuses mismatched roots, ambiguous ZIP members, unproved truncation,
    /// unrelated member changes, invalid failure homes and source errors.
    pub fn given_up_zip<'before, 'after>(
        before: DecodeSnapshot<'_>,
        outcome: DecodeSnapshot<'_>,
        report: &DecodeReport,
        previous: (&DecodeContext<'before>, View<'before>),
        current: (&DecodeContext<'after>, View<'after>),
    ) -> Result<Self, String> {
        Self::given_up_zip_target(before, outcome, report, previous, current, None)
    }

    /// Authenticate an original independently framed content-damage target.
    /// Sequential read allowances start at that target, rather than at the
    /// later read failure. Original descendant framing remains immutable.
    ///
    /// # Errors
    /// Refuses foreign source/part authority or changed original framing.
    pub fn given_up_zip_damaged<'before, 'after>(
        before: DecodeSnapshot<'_>,
        outcome: DecodeSnapshot<'_>,
        report: &DecodeReport,
        previous: (&DecodeContext<'before>, View<'before>),
        current: (&DecodeContext<'after>, View<'after>),
        target: &super::damage::DamageTarget<'_>,
    ) -> Result<Self, String> {
        Self::given_up_zip_target(before, outcome, report, previous, current, Some(target))
    }

    fn given_up_zip_target<'before, 'after>(
        before: DecodeSnapshot<'_>,
        outcome: DecodeSnapshot<'_>,
        report: &DecodeReport,
        previous: (&DecodeContext<'before>, View<'before>),
        current: (&DecodeContext<'after>, View<'after>),
        target: Option<&super::damage::DamageTarget<'_>>,
    ) -> Result<Self, String> {
        let mut old = Sources::root(before, previous.0, previous.1)?;
        let mut new = Sources::root(outcome, current.0, current.1)?;
        old.members(previous.0, false)?;
        let root_cut = new.root.window().len() < old.root.window().len()
            && old.root.window().starts_with(new.root.window());
        let root_failed = outcome.fidelity.parts().values().any(|part| {
            part.level().as_str() == "zip.archive"
                && part.parent().is_none()
                && part.state() == &(PartState::GivenUp {})
                && part.stream() == new.stream
                && part.offset() == 0
                && part.byte_len() == u64_from_index(new.root.window().len())
        });
        let mut witnesses = Vec::new();
        if root_cut && root_failed {
            witnesses.push((
                old.stream.clone(),
                0,
                u64_from_index(old.root.window().len()),
            ));
            for member in old.members.values() {
                witnesses.push((
                    member.stream.clone(),
                    0,
                    u64_from_index(member.source.window().len()),
                ));
            }
        } else {
            new.members(current.0, root_cut)?;
            if root_cut && new.incomplete_header.is_some() {
                incomplete_archive_witnesses(before, outcome, &old, &new, &mut witnesses)?;
            } else if old.members.keys().ne(new.members.keys()) {
                return Err("ZIP correspondence changes the actual member-name set".into());
            }
            if let Some(target) = target {
                let name = old
                    .members
                    .iter()
                    .find(|(_, member)| member.stream == target.stream())
                    .map(|(name, _)| name)
                    .ok_or("damage target is not an actual original ZIP member source")?;
                let next = new.members.get(name).ok_or("damaged member is absent")?;
                let old_stream = target.stream();
                witnesses.extend(target.witness(
                    before,
                    |stream, range| old.resolve(stream, range),
                    |stream, range| {
                        if stream != old_stream {
                            return None;
                        }
                        let start = usize::try_from(range.start).ok()?;
                        let end = usize::try_from(range.end).ok()?;
                        next.source.window().get(start..end)
                    },
                )?);
            }
            for (name, next) in &new.members {
                let prior = old
                    .members
                    .get(name)
                    .ok_or("ZIP correspondence gains a member name")?;
                if prior.method != next.method {
                    return Err("ZIP correspondence changes a member method".into());
                }
                if let Some(target) = target {
                    if prior.source.window().len() != next.source.window().len()
                        || prior
                            .source
                            .window()
                            .iter()
                            .zip(next.source.window())
                            .enumerate()
                            .any(|(offset, (old, new))| {
                                old != new
                                    && (prior.stream != target.stream()
                                        || !target.content_contains(u64_from_index(offset)))
                            })
                    {
                        return Err(
                            "content-only damage changed bytes outside its authenticated target"
                                .into(),
                        );
                    }
                }
                member_witnesses(before, outcome, prior, next, &mut witnesses)?;
            }
        }
        let mut allowances = Self::given_up_witnessed(
            before,
            outcome,
            report,
            &new.stream,
            |stream, range| new.resolve(stream, range),
            &witnesses,
        )?;
        if root_cut && root_failed {
            allowances.prove_root_scope_losses(before.document, outcome.document)?;
        }
        Ok(allowances)
    }
}

fn incomplete_archive_witnesses(
    before: DecodeSnapshot<'_>,
    outcome: DecodeSnapshot<'_>,
    old: &Sources<'_>,
    new: &Sources<'_>,
    witnesses: &mut Vec<(String, u64, u64)>,
) -> Result<(), String> {
    let cut = u64_from_index(new.root.window().len());
    let (start, length) = new
        .incomplete_header
        .ok_or("ZIP prefix has no incomplete header")?;
    if start.checked_add(length) != Some(cut) {
        return Err("incomplete ZIP header does not reach the actual prefix EOF".into());
    }
    let failed = outcome
        .fidelity
        .parts()
        .values()
        .find(|part| {
            part.level().as_str() == "zip.local_header"
                && part.state() == &(PartState::GivenUp {})
                && part.stream() == new.stream
                && part.offset() == start
                && part.byte_len() == length
                && part
                    .parent()
                    .and_then(|id| outcome.fidelity.parts().get(id))
                    .is_some_and(|parent| {
                        parent.level().as_str() == "zip.archive"
                            && parent.parent().is_none()
                            && parent.stream() == new.stream
                            && parent.offset() == 0
                            && parent.byte_len() == cut
                    })
        })
        .ok_or("recovered ZIP header has no matching actual failed helper")?;
    let original = old
        .members
        .values()
        .find(|member| {
            member.header.0 == failed.offset()
                && member
                    .header
                    .0
                    .checked_add(member.header.1)
                    .is_some_and(|end| end > cut)
        })
        .ok_or("incomplete ZIP header has no original cut-containing physical counterpart")?;
    if !before.fidelity.parts().values().any(|part| {
        part.level().as_str() == "zip.local_header"
            && part.stream() == old.stream
            && part.offset() == original.header.0
            && part.byte_len() == original.header.1
            && part
                .parent()
                .and_then(|id| before.fidelity.parts().get(id))
                .is_some_and(|parent| {
                    parent.level().as_str() == "zip.archive"
                        && parent.parent().is_none()
                        && parent.stream() == old.stream
                        && parent.offset() == 0
                        && parent.byte_len() == u64_from_index(old.root.window().len())
                })
    }) {
        return Err("original incomplete-header counterpart has no actual source part".into());
    }
    witnesses.push((
        old.stream.clone(),
        cut,
        u64_from_index(old.root.window().len()) - cut,
    ));
    for (name, member) in &old.members {
        if !new.members.contains_key(name) {
            if member.entry.1 < cut || (member.header.0 != start && member.header.0 < cut) {
                return Err(
                    "recovered ZIP prefix loses a previously complete physical member".into(),
                );
            }
            witnesses.push((
                member.stream.clone(),
                0,
                u64_from_index(member.source.window().len()),
            ));
        }
    }
    Ok(())
}

fn member_witnesses(
    before: DecodeSnapshot<'_>,
    outcome: DecodeSnapshot<'_>,
    old: &Member<'_>,
    new: &Member<'_>,
    witnesses: &mut Vec<(String, u64, u64)>,
) -> Result<(), String> {
    let old_bytes = old.source.window();
    let new_bytes = new.source.window();
    let failures = outcome
        .fidelity
        .parts()
        .values()
        .filter(|part| part.state() == &(PartState::GivenUp {}) && part.stream() == new.stream)
        .collect::<Vec<_>>();
    if new_bytes.len() > old_bytes.len() {
        return Err("ZIP correspondence enlarges an expanded member".into());
    }
    if new_bytes.len() < old_bytes.len() {
        if !old_bytes.starts_with(new_bytes) {
            return Err("shortened ZIP member is not the original source prefix".into());
        }
        let cut = u64_from_index(new_bytes.len());
        let mut proved = false;
        for failed in &failures {
            if failed.offset().checked_add(failed.byte_len()) != Some(cut) {
                continue;
            }
            for original in before.fidelity.parts().values().filter(|part| {
                part.level() == failed.level()
                    && part.stream() == old.stream
                    && part.offset() == failed.offset()
            }) {
                let end = original
                    .offset()
                    .checked_add(original.byte_len())
                    .ok_or("prior ZIP member part extent overflows")?;
                if end <= cut || end > u64_from_index(old_bytes.len()) {
                    continue;
                }
                if !parent_corresponds(before, outcome, original, failed, old, new)? {
                    continue;
                }
                let end = if failed.level().framing() == PartFraming::Sequential {
                    let parent = original
                        .parent()
                        .and_then(|id| before.fidelity.parts().get(id))
                        .ok_or("sequential ZIP member failure has no actual parent")?;
                    if parent.stream() == old.stream {
                        parent
                            .offset()
                            .checked_add(parent.byte_len())
                            .ok_or("sequential prior parent extent overflows")?
                    } else {
                        u64_from_index(old_bytes.len())
                    }
                } else {
                    end
                };
                let length = end
                    .checked_sub(original.offset())
                    .ok_or("prior sequential suffix is reversed")?;
                if end > u64_from_index(old_bytes.len()) {
                    return Err("prior sequential suffix exceeds its real member".into());
                }
                witnesses.push((old.stream.clone(), original.offset(), length));
                proved = true;
            }
        }
        if !proved {
            return Err("shortened ZIP member has no actual cut-containing failed part".into());
        }
        // The exact original prefix and the actual EOF-containing failure
        // prove physical absence of these bytes, even beyond that terminated
        // record's old extent. Earlier surviving source bytes get no permission.
        witnesses.push((
            old.stream.clone(),
            cut,
            u64_from_index(old_bytes.len()) - cut,
        ));
    } else {
        for (offset, (prior, current)) in old_bytes.iter().zip(new_bytes).enumerate() {
            if prior == current {
                continue;
            }
            let offset = u64_from_index(offset);
            let content_damage = witnesses.iter().any(|(stream, start, len)| {
                stream == &old.stream
                    && *start <= offset
                    && start.checked_add(*len).is_some_and(|end| offset < end)
            });
            if !content_damage
                && !failures.iter().any(|part| {
                    part.offset() <= offset
                        && part
                            .offset()
                            .checked_add(part.byte_len())
                            .is_some_and(|end| offset < end)
                })
                && (failures.is_empty()
                    || !authentic_b1(before, old, offset)
                    || !authentic_b1(outcome, new, offset))
            {
                let spans = |snapshot: DecodeSnapshot<'_>, stream: &str| {
                    snapshot.fidelity.b1_spans().get(stream).map(|source| {
                        source
                            .spans()
                            .iter()
                            .map(|span| span.range())
                            .collect::<Vec<_>>()
                    })
                };
                let reads = |snapshot: DecodeSnapshot<'_>, stream: &str| {
                    snapshot
                        .fidelity
                        .record_layout()
                        .iter()
                        .filter_map(|(id, layout)| {
                            layout
                                .reads()
                                .get(stream)
                                .filter(|ranges| ranges.iter().any(|range| range.contains(&offset)))
                                .map(|ranges| (id.clone(), ranges.clone()))
                        })
                        .collect::<Vec<_>>()
                };
                return Err(format!(
                    "ZIP member changed outside its actual given-up source extents at {offset}; before_stream={} after_stream={} before_b1={:?} after_b1={:?} before_readers={:?} after_readers={:?} failures={:?}",
                    old.stream, new.stream, spans(before, &old.stream), spans(outcome, &new.stream),
                    reads(before, &old.stream), reads(outcome, &new.stream),
                    failures.iter().map(|part| (part.level().as_str(), part.offset(), part.byte_len())).collect::<Vec<_>>()
                ));
            }
        }
    }
    for part in failures {
        let end = part
            .offset()
            .checked_add(part.byte_len())
            .ok_or("failed ZIP member part extent overflows")?;
        if end > u64_from_index(new_bytes.len()) {
            return Err("failed ZIP member part exceeds its actual source".into());
        }
        witnesses.push((old.stream.clone(), part.offset(), part.byte_len()));
    }
    Ok(())
}

fn authentic_b1(snapshot: DecodeSnapshot<'_>, member: &Member<'_>, offset: u64) -> bool {
    let Some(coverage) = snapshot.fidelity.b1_spans().get(&member.stream) else {
        return false;
    };
    if coverage.byte_len() != u64_from_index(member.source.window().len()) {
        return false;
    }
    let Some(span) = coverage
        .spans()
        .iter()
        .map(|span| span.range())
        .find(|range| range.contains(&offset))
    else {
        return false;
    };
    let overlaps = |range: &Range<u64>| range.start < span.end && span.start < range.end;
    !snapshot.fidelity.record_layout().values().any(|layout| {
        layout
            .reads()
            .get(&member.stream)
            .is_some_and(|ranges| ranges.iter().any(overlaps))
    }) && !snapshot
        .fidelity
        .parts()
        .values()
        .filter(|part| part.stream() == member.stream)
        .any(|part| part.framing_bytes().iter().any(overlaps))
}

fn parent_corresponds(
    before: DecodeSnapshot<'_>,
    outcome: DecodeSnapshot<'_>,
    original: &cadmpeg_ir::source_fidelity::parts::PartRecord,
    failed: &cadmpeg_ir::source_fidelity::parts::PartRecord,
    old: &Member<'_>,
    new: &Member<'_>,
) -> Result<bool, String> {
    let Some(old_parent) = original
        .parent()
        .and_then(|id| before.fidelity.parts().get(id))
    else {
        return Ok(false);
    };
    let Some(new_parent) = failed
        .parent()
        .and_then(|id| outcome.fidelity.parts().get(id))
    else {
        return Ok(false);
    };
    if old_parent.level() != new_parent.level() {
        return Ok(false);
    }
    if old_parent.stream() == old.stream && new_parent.stream() == new.stream {
        let old_end = old_parent
            .offset()
            .checked_add(old_parent.byte_len())
            .ok_or("prior parent extent overflows")?;
        let new_end = new_parent
            .offset()
            .checked_add(new_parent.byte_len())
            .ok_or("current parent extent overflows")?;
        return Ok(old_parent.offset() == new_parent.offset()
            && old_end >= new_end
            && old_parent.offset() <= original.offset()
            && new_parent.offset() <= failed.offset()
            && original
                .offset()
                .checked_add(original.byte_len())
                .is_some_and(|end| end <= old_end)
            && failed
                .offset()
                .checked_add(failed.byte_len())
                .is_some_and(|end| end <= new_end));
    }
    Ok(old_parent.level().as_str() == "zip.entry"
        && (
            old_parent.stream(),
            old_parent.offset(),
            old_parent.byte_len(),
        ) == (old.entry.0.as_str(), old.entry.1, old.entry.2)
        && (
            new_parent.stream(),
            new_parent.offset(),
            new_parent.byte_len(),
        ) == (new.entry.0.as_str(), new.entry.1, new.entry.2))
}

#[cfg(test)]
mod tests;
