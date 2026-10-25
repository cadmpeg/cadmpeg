// SPDX-License-Identifier: Apache-2.0
//! Explicit original damaged-part authority, independent of failure locations.

use cadmpeg_core::decode::{DecodeContext, PartCapture, View};
use cadmpeg_ir::source_fidelity::parts::{PartFraming, PartLevel};
use std::ops::Range;

use super::DecodeSnapshot;
use crate::framing::{FrameParent, FramedPart, PartTable};

/// An actual independently framed part selected for a content-only mutation.
/// No loss message or later failure can manufacture this authority.
#[derive(Debug)]
pub struct DamageTarget<'a> {
    source: View<'a>,
    capture: PartCapture,
    level: PartLevel,
    framing: Vec<Range<u64>>,
    content: Vec<Range<u64>>,
    parent: Option<(PartLevel, String, u64, u64)>,
}

impl<'a> DamageTarget<'a> {
    /// Bind a selected part to its genuine source and validated inventory.
    ///
    /// # Errors
    /// Rejects an invalid inventory, source, index, or descendant ancestry.
    pub fn new(
        ctx: &DecodeContext<'_>,
        inventory: &[FramedPart<'a>],
        index: usize,
    ) -> Result<Self, String> {
        PartTable::load()?.validate(ctx, inventory)?;
        let frame = inventory
            .get(index)
            .ok_or("damage index outside independent inventory")?;
        ctx.require_source(frame.view())
            .map_err(|error| error.to_string())?;
        let capture = ctx
            .capture_part_extent(frame.view())
            .map_err(|error| error.to_string())?;
        let parent = match frame.parent() {
            FrameParent::Part(index) | FrameParent::Host(index) => {
                let parent = inventory
                    .get(index)
                    .ok_or("damage parent outside independent inventory")?;
                Some((
                    parent.level(),
                    parent.capture().stream().to_owned(),
                    parent.capture().offset(),
                    parent.capture().byte_len(),
                ))
            }
            FrameParent::File => None,
        };
        Ok(Self {
            source: frame.view(),
            capture,
            level: frame.level(),
            framing: frame.framing().to_vec(),
            content: frame.content_ranges_in(inventory)?,
            parent,
        })
    }

    pub(super) fn stream(&self) -> &str {
        self.capture.stream()
    }
    pub(super) fn content_contains(&self, offset: u64) -> bool {
        self.content.iter().any(|range| range.contains(&offset))
    }

    pub(super) fn witness<'source>(
        &self,
        before: DecodeSnapshot<'_>,
        mut original: impl FnMut(&str, Range<u64>) -> Option<&'source [u8]>,
        mut damaged: impl FnMut(&str, Range<u64>) -> Option<&'source [u8]>,
    ) -> Result<Vec<(String, u64, u64)>, String> {
        let start = self.capture.offset();
        let end = start
            .checked_add(self.capture.byte_len())
            .ok_or("damage target overflows")?;
        if original(self.stream(), start..end) != Some(self.source.window()) {
            return Err("damage target does not describe the actual original source".into());
        }
        let actual = before
            .fidelity
            .parts()
            .iter()
            .filter(|(_, part)| {
                part.level() == self.level
                    && part.stream() == self.stream()
                    && part.offset() == start
                    && part.byte_len() == self.capture.byte_len()
                    && part.framing_bytes() == self.framing
            })
            .collect::<Vec<_>>();
        let [(actual_id, actual)] = actual.as_slice() else {
            return Err("damage target has no unique original helper part".into());
        };
        let parent = actual
            .parent()
            .and_then(|id| before.fidelity.parts().get(id));
        match (&self.parent, parent) {
            (Some((level, stream, offset, length)), Some(parent))
                if parent.level() == *level
                    && parent.stream() == stream
                    && parent.offset() == *offset
                    && parent.byte_len() == *length => {}
            (None, None) => {}
            _ => return Err("damage target does not retain its actual original parent".into()),
        }
        let bytes = damaged(self.stream(), start..end)
            .ok_or("damage target is not wholly present in the changed source")?;
        if bytes.len() != self.source.window().len() {
            return Err("content damage changed its source extent".into());
        }
        // The supplied inventory cannot erase framing from actual original
        // helper descendants. This also rejects an incomplete caller census.
        let mut actual_framing = Vec::new();
        for part in before.fidelity.parts().values() {
            let mut parent = part.parent();
            let mut walked = 0;
            while let Some(id) = parent {
                walked += 1;
                if walked > before.fidelity.parts().len() {
                    return Err("original helper part ancestry contains a cycle".into());
                }
                if id == *actual_id {
                    if part.stream() == self.stream() {
                        actual_framing.extend_from_slice(part.framing_bytes());
                    }
                    break;
                }
                parent = before
                    .fidelity
                    .parts()
                    .get(id)
                    .ok_or("original helper part has an absent parent")?
                    .parent();
            }
        }
        let mut changed = false;
        for (index, (old, new)) in self.source.window().iter().zip(bytes).enumerate() {
            if old == new {
                continue;
            }
            let offset = start
                .checked_add(u64::try_from(index).map_err(|_| "damage index exceeds u64")?)
                .ok_or("damage index overflows")?;
            if !self.content.iter().any(|range| range.contains(&offset))
                || actual_framing.iter().any(|range| range.contains(&offset))
            {
                return Err(
                    "damage changed original framing bytes, including a descendant's framing"
                        .into(),
                );
            }
            changed = true;
        }
        if !changed {
            return Ok(Vec::new());
        }
        let end = if self.level.framing() == PartFraming::Sequential {
            let parent = parent.ok_or("sequential damage target has no original parent")?;
            if parent.stream() != self.stream() {
                return Err("sequential damage parent has a different source address space".into());
            }
            parent
                .offset()
                .checked_add(parent.byte_len())
                .ok_or("sequential damage parent overflows")?
        } else {
            end
        };
        Ok(vec![(
            self.stream().to_owned(),
            start,
            end.checked_sub(start)
                .ok_or("damage parent ends before target")?,
        )])
    }
}

#[cfg(test)]
mod tests;
