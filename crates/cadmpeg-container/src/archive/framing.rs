// SPDX-License-Identifier: Apache-2.0
//! Source-backed ZIP navigation fields for byte representation accounting.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use super::{parse_data_descriptor, signature_at, u16_at, u32_at, ArchiveSnapshot, EntryRecord};

const STORAGE: &str = "ZIP framing view storage";

impl<'a> ArchiveSnapshot<'a> {
    /// Returns the exact source views occupied by ZIP framing fields.
    /// Ordering and borrowed-view descriptors are temporary working storage,
    /// not retained source bytes or output.
    ///
    /// Names, methods, CRCs, comments, payloads and ordinary extra-field data
    /// are not framing. ZIP64 scalar fields and fixed footer prefixes follow
    /// Appendix C. Unreadable local frames contribute only bounded fields
    /// whose actual header signature and fixed prefix are present.
    /// Navigation does not attribute semantic reads or register new streams.
    ///
    /// # Errors
    /// Refuses foreign or detached source views, invalid structural boundaries,
    /// and the caller's first refusal or working allocation failure. The real
    /// tracking infrastructure is admitted once on this caller's account.
    pub fn framing_views(&self, ctx: &DecodeContext<'a>) -> Result<Vec<View<'a>>, CodecError> {
        ctx.framing_part_extent(self.root)?;
        let mut order = Vec::new();
        ctx.reserve_working(&mut order, self.entries.len(), STORAGE)?;
        order.resize(self.entries.len(), 0_usize);
        for (index, value) in order.iter_mut().enumerate() {
            *value = index;
        }
        order.sort_unstable_by_key(|index| self.entries[*index].header_start);
        let mut count = 0_usize;
        walk(self, &order, &mut |_| {
            count = count
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("ZIP framing view count overflows"))?;
            Ok(())
        })?;
        let mut views = Vec::new();
        ctx.reserve_working(&mut views, count, STORAGE)?;
        views.resize(count, self.root);
        let mut at = 0;
        walk(self, &order, &mut |view| {
            let output = views
                .get_mut(at)
                .ok_or_else(|| CodecError::malformed("ZIP framing count changed"))?;
            *output = view;
            at += 1;
            Ok(())
        })?;
        Ok(views)
    }
}

struct Fields<'a, 'emit> {
    source: View<'a>,
    emit: &'emit mut dyn FnMut(View<'a>) -> Result<(), CodecError>,
}

impl Fields<'_, '_> {
    fn range(&mut self, start: u64, end: u64) -> Result<(), CodecError> {
        if start == end {
            return Ok(());
        }
        let origin = u64_from_index(self.source.start());
        let start = origin
            .checked_add(start)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| CodecError::malformed("ZIP framing source offset overflows"))?;
        let end = origin
            .checked_add(end)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| CodecError::malformed("ZIP framing source offset overflows"))?;
        let view = self
            .source
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("ZIP framing field escapes its actual source"))?;
        (self.emit)(view)
    }

    fn field(&mut self, at: u64, offset: u64, length: u64) -> Result<(), CodecError> {
        let start = add(at, offset)?;
        self.range(start, add(start, length)?)
    }

    fn fixed(&mut self, at: u64, fields: &[(u64, u64)]) -> Result<(), CodecError> {
        for &(offset, length) in fields {
            self.field(at, offset, length)?;
        }
        Ok(())
    }

    fn extras(&mut self, mut at: u64, end: u64, zip64_fields: u32) -> Result<(), CodecError> {
        while at < end {
            let data = add(at, 4)?;
            if data > end {
                return Err(CodecError::malformed("ZIP extra header is cut short"));
            }
            let tag = u16_at(self.source.window(), at)?;
            let next = add(data, u64::from(u16_at(self.source.window(), add(at, 2)?)?))?;
            if next > end {
                return Err(CodecError::malformed("ZIP extra field escapes its header"));
            }
            self.range(at, data)?;
            if tag == 1 {
                let size_end = add(data, u64::from(zip64_fields) * 8)?;
                if size_end > next {
                    return Err(CodecError::malformed("ZIP64 framing scalar is cut short"));
                }
                self.range(data, size_end)?;
            }
            at = next;
        }
        Ok(())
    }
}

fn add(at: u64, length: u64) -> Result<u64, CodecError> {
    at.checked_add(length)
        .ok_or_else(|| CodecError::malformed("ZIP framing offset overflows"))
}

fn walk<'a>(
    snapshot: &ArchiveSnapshot<'a>,
    order: &[usize],
    emit: &mut dyn FnMut(View<'a>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let mut fields = Fields {
        source: snapshot.root,
        emit,
    };
    for (position, &index) in order.iter().enumerate() {
        let entry = &snapshot.entries[index];
        let limit = order
            .get(position + 1)
            .map_or(snapshot.central_start, |next| {
                snapshot.entries[*next].header_start
            });
        local(&mut fields, entry, limit)?;
    }
    for entry in snapshot
        .entries
        .iter()
        .filter(|entry| !entry.is_recovered())
    {
        central(&mut fields, entry)?;
    }
    if let Some(footer) = snapshot.footer {
        let end = u64_from_index(footer.end_record);
        fields.field(end, 0, 22)?;
        if let Some(record) = footer.zip64_record {
            fields.field(u64_from_index(record), 0, 56)?;
            let locator = end
                .checked_sub(20)
                .ok_or_else(|| CodecError::malformed("ZIP64 locator offset underflows"))?;
            fields.field(locator, 0, 20)?;
        }
        let central_end = snapshot
            .entries
            .iter()
            .filter(|entry| !entry.is_recovered())
            .map(|entry| entry.declaration_range().end)
            .max()
            .unwrap_or(snapshot.central_start);
        if signature_at(snapshot.root.window(), central_end) == Some(*b"PK\x05\x05") {
            let signature_end = add(
                add(central_end, 6)?,
                u64::from(u16_at(snapshot.root.window(), add(central_end, 4)?)?),
            )?;
            let limit = footer.zip64_record.map_or(end, u64_from_index);
            if signature_end > limit {
                return Err(CodecError::malformed(
                    "ZIP digital signature escapes directory footer",
                ));
            }
            fields.field(central_end, 0, 6)?;
        }
    }
    Ok(())
}

fn local(fields: &mut Fields<'_, '_>, entry: &EntryRecord, limit: u64) -> Result<(), CodecError> {
    let at = entry.header_start;
    if signature_at(fields.source.window(), at) != Some(*b"PK\x03\x04") {
        return Ok(());
    }
    if add(at, 30)? > limit {
        return Ok(());
    }
    fields.fixed(at, &[(0, 4), (6, 2), (18, 4), (26, 4)])?;
    let extra = add(
        add(at, 30)?,
        u64::from(u16_at(fields.source.window(), add(at, 26)?)?),
    )?;
    let end = add(
        extra,
        u64::from(u16_at(fields.source.window(), add(at, 28)?)?),
    )?;
    if end <= limit {
        fields.extras(extra, end, 2)?;
    }
    if entry.data_start.is_some() && u16_at(fields.source.window(), add(at, 6)?)? & 8 != 0 {
        let start = entry.data_end()?;
        let end = parse_data_descriptor(fields.source.window(), entry, limit)?;
        let length = end
            .checked_sub(start)
            .ok_or_else(|| CodecError::malformed("ZIP descriptor length underflows"))?;
        let prefix = match length {
            12 | 20 => 0,
            16 | 24 => 4,
            _ => return Err(CodecError::malformed("ZIP descriptor has an invalid width")),
        };
        fields.field(start, 0, prefix)?;
        fields.range(add(add(start, prefix)?, 4)?, end)?;
    }
    Ok(())
}

fn central(fields: &mut Fields<'_, '_>, entry: &EntryRecord) -> Result<(), CodecError> {
    let at = entry.central_start;
    fields.fixed(at, &[(0, 4), (8, 2), (20, 14), (42, 4)])?;
    let extra = add(
        add(at, 46)?,
        u64::from(u16_at(fields.source.window(), add(at, 28)?)?),
    )?;
    let end = add(
        extra,
        u64::from(u16_at(fields.source.window(), add(at, 30)?)?),
    )?;
    let mut zip64_fields = 0;
    for offset in [24, 20, 42] {
        if u32_at(fields.source.window(), add(at, offset)?)? == u32::MAX {
            zip64_fields += 1;
        }
    }
    fields.extras(extra, end, zip64_fields)
}

#[cfg(test)]
mod tests;
