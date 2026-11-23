// SPDX-License-Identifier: Apache-2.0
//! Feature-scan access to selected ZIP members.

use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::scan::{InflateFraming, ScanBudget, StopReason};

use crate::layout::end_record as end;

/// ZIP directory entry available to a feature-list selector.
pub struct ScanZipEntry {
    /// Directory ordinal.
    pub ordinal: usize,
    /// Exact member name.
    pub name: String,
    /// Claimed expanded size.
    pub declared_size: u64,
}

struct Member {
    name: String,
    method: u16,
    flags: u16,
    compressed: u64,
    expanded: u64,
    local: u64,
    crc: u32,
}

fn malformed(what: &str) -> StopReason {
    StopReason::Malformed(what.into())
}

fn field16(mut view: View<'_>, at: usize) -> Result<u16, StopReason> {
    view.req_take(at)?;
    Ok(view.req_u16_le()?)
}
fn field32(mut view: View<'_>, at: usize) -> Result<u32, StopReason> {
    view.req_take(at)?;
    Ok(view.req_u32_le()?)
}

/// ZIP reader whose directory and selected payloads share the scan meter.
pub struct ScanZip<'a, 'b> {
    source: View<'a>,
    budget: &'b ScanBudget<'b>,
    physical: bool,
    directory: usize,
    archive_offset: usize,
    members: Vec<Member>,
}

impl<'a, 'b> ScanZip<'a, 'b> {
    /// Reads the directory after admitting its metadata storage.
    pub fn new(
        source: View<'a>,
        budget: &'b ScanBudget<'b>,
        physical: bool,
    ) -> Result<Self, StopReason> {
        budget.check()?;
        let mut reader = if physical {
            cadmpeg_core::scan::ScanReader::source(source, budget)
        } else {
            cadmpeg_core::scan::ScanReader::retained(source, budget)
        };
        let last_start = source
            .end()
            .checked_sub(end::LEN)
            .filter(|at| *at >= source.start())
            .ok_or_else(|| StopReason::Truncated("ZIP end record".into()))?;
        reader.seek(last_start)?;
        let last = reader.take(end::LEN)?;
        let (record, record_at) = if last.window().starts_with(b"PK\x05\x06")
            && View::u16_le_at(last.window(), end::COMMENT_LENGTH) == Some(0)
        {
            (last.window(), last_start)
        } else {
            let tail_length = source.window().len().min(65557);
            reader.seek(source.end() - tail_length)?;
            let tail = reader.take(tail_length)?;
            let mut found = None;
            for offset in (0..=tail_length - end::LEN).rev() {
                budget.check()?;
                let bytes = &tail.window()[offset..];
                if bytes.starts_with(b"PK\x05\x06") {
                    let comment = View::u16_le_at(bytes, end::COMMENT_LENGTH)
                        .ok_or_else(|| StopReason::Truncated("ZIP comment length".into()))?;
                    if offset + end::LEN + usize::from(comment) == tail_length {
                        found = Some((bytes, source.end() - tail_length + offset));
                        break;
                    }
                }
            }
            found.ok_or_else(|| StopReason::Truncated("ZIP end record".into()))?
        };
        let mut count = u64::from(
            View::u16_le_at(record, end::ENTRIES)
                .ok_or_else(|| StopReason::Truncated("ZIP directory count".into()))?,
        );
        let mut directory_size = u64::from(
            View::u32_le_at(record, end::DIRECTORY_SIZE)
                .ok_or_else(|| StopReason::Truncated("ZIP directory size".into()))?,
        );
        let mut directory_position = u64::from(
            View::u32_le_at(record, end::DIRECTORY_POSITION)
                .ok_or_else(|| StopReason::Truncated("ZIP directory position".into()))?,
        );
        let mut directory_end = record_at;
        if count == u64::from(u16::MAX)
            || directory_size == u64::from(u32::MAX)
            || directory_position == u64::from(u32::MAX)
        {
            use crate::layout::{zip64_end as wide, zip64_locator as locator};
            reader.seek(
                record_at
                    .checked_sub(locator::LEN)
                    .ok_or_else(|| StopReason::Truncated("ZIP64 locator".into()))?,
            )?;
            let locator = reader.take(locator::LEN)?;
            if !locator.window().starts_with(b"PK\x06\x07") {
                return Err(StopReason::Malformed("ZIP64 locator".into()));
            }
            let position = View::u64_le_at(locator.window(), locator::END_RECORD_POSITION)
                .ok_or_else(|| StopReason::Truncated("ZIP64 end position".into()))?;
            let at = usize::try_from(position)
                .ok()
                .and_then(|at| source.start().checked_add(at))
                .ok_or(StopReason::SourceBytes)?;
            directory_end = at;
            reader.seek(at)?;
            let wide = reader.take(wide::LEN)?;
            if !wide.window().starts_with(b"PK\x06\x06") {
                return Err(StopReason::Malformed("ZIP64 end record".into()));
            }
            count = View::u64_le_at(wide.window(), wide::ENTRIES)
                .ok_or_else(|| StopReason::Truncated("ZIP64 entries".into()))?;
            directory_size = View::u64_le_at(wide.window(), wide::DIRECTORY_SIZE)
                .ok_or_else(|| StopReason::Truncated("ZIP64 directory size".into()))?;
            directory_position = View::u64_le_at(wide.window(), wide::DIRECTORY_POSITION)
                .ok_or_else(|| StopReason::Truncated("ZIP64 directory position".into()))?;
        }
        if cadmpeg_core::decode::bounded_len(
            count,
            crate::layout::central_header::LEN,
            usize::try_from(directory_size).map_err(|_| StopReason::SourceBytes)?,
        )
        .is_none()
        {
            return Err(StopReason::Malformed(
                "ZIP directory count exceeds its extent".into(),
            ));
        }
        budget.retain(
            directory_size
                .checked_mul(8)
                .and_then(|size| {
                    count
                        .checked_mul(1024)
                        .and_then(|rows| size.checked_add(rows))
                })
                .ok_or(StopReason::RetainedBytes)?,
        )?;
        let directory = directory_end
            .checked_sub(usize::try_from(directory_size).map_err(|_| StopReason::SourceBytes)?)
            .filter(|at| *at >= source.start())
            .ok_or_else(|| malformed("ZIP directory extent"))?;
        let archive_offset = directory
            .checked_sub(usize::try_from(directory_position).map_err(|_| StopReason::SourceBytes)?)
            .filter(|at| *at >= source.start())
            .ok_or_else(|| malformed("ZIP directory position"))?;
        reader.seek(directory)?;
        let mut members = Vec::new();
        for _ in 0..count {
            use crate::layout::central_header as central;
            budget.check()?;
            let header_end = reader
                .position()
                .checked_add(central::LEN)
                .filter(|at| *at <= directory_end)
                .ok_or_else(|| malformed("ZIP central header extent"))?;
            let header = reader.take(header_end - reader.position())?;
            let bytes = header.window();
            if !bytes.starts_with(b"PK\x01\x02") {
                return Err(malformed("ZIP central header"));
            }
            let method = field16(header, central::COMPRESSION)?;
            let flags = field16(header, central::FLAGS)?;
            let name_len = usize::from(field16(header, central::NAME_LENGTH)?);
            let extra_len = usize::from(field16(header, central::EXTRA_LENGTH)?);
            let comment_len = usize::from(field16(header, central::COMMENT_LENGTH)?);
            let next = reader
                .position()
                .checked_add(name_len)
                .and_then(|at| at.checked_add(extra_len))
                .and_then(|at| at.checked_add(comment_len))
                .filter(|at| *at <= directory_end)
                .ok_or_else(|| malformed("ZIP central extent"))?;
            let name = reader.take(name_len)?;
            let extra = reader.take(extra_len)?;
            reader.seek(next)?;
            let mut expanded = u64::from(field32(header, central::EXPANDED_LENGTH)?);
            let mut compressed = u64::from(field32(header, central::COMPRESSED_LENGTH)?);
            let mut local = u64::from(field32(header, central::LOCAL_HEADER_POSITION)?);
            let needs_wide = [expanded, compressed, local].contains(&u64::from(u32::MAX));
            let mut fields = cadmpeg_core::scan::ScanReader::retained(extra, budget);
            let mut wide = false;
            while fields.remaining() != 0 {
                budget.check()?;
                let tag = fields.u16()?;
                let size = usize::from(fields.u16()?);
                let value = fields.take(size)?;
                if tag == 1 {
                    if wide {
                        return Err(malformed("duplicate ZIP64 central extra"));
                    }
                    wide = true;
                    let mut value = cadmpeg_core::scan::ScanReader::retained(value, budget);
                    for target in [&mut expanded, &mut compressed, &mut local] {
                        if *target == u64::from(u32::MAX) {
                            *target = value.u64()?;
                        }
                    }
                }
            }
            if needs_wide && !wide {
                return Err(malformed("ZIP64 central value is absent"));
            }
            members.push(Member {
                name: crate::archive::local::decode_name(name.window(), flags & 0x800 != 0),
                method,
                flags,
                compressed,
                expanded,
                local,
                crc: field32(header, central::CRC32)?,
            });
        }
        Ok(Self {
            source,
            budget,
            physical,
            directory,
            archive_offset,
            members,
        })
    }

    /// Lists directory entries in source order.
    pub fn entries(&mut self) -> Result<Vec<ScanZipEntry>, StopReason> {
        let mut entries = Vec::new();
        for (ordinal, member) in self.members.iter().enumerate() {
            self.budget.check()?;
            self.budget.retain(
                u64_from_index(member.name.len())
                    .checked_add(256)
                    .ok_or(StopReason::RetainedBytes)?,
            )?;
            entries.push(ScanZipEntry {
                ordinal,
                name: member.name.clone(),
                declared_size: member.expanded,
            });
        }
        Ok(entries)
    }

    /// Acquires and expands one selected member in bounded steps.
    pub fn open(&mut self, ordinal: usize) -> Result<Vec<u8>, StopReason> {
        use crate::layout::local_header as local;
        self.budget.check()?;
        let member = self
            .members
            .get(ordinal)
            .ok_or_else(|| malformed("ZIP member ordinal"))?;
        if member.flags & 1 != 0 || member.method == 99 {
            return Err(StopReason::Unsupported("encrypted ZIP listing".into()));
        }
        let mut reader = if self.physical {
            cadmpeg_core::scan::ScanReader::source(self.source, self.budget)
        } else {
            cadmpeg_core::scan::ScanReader::retained(self.source, self.budget)
        };
        let at = usize::try_from(member.local)
            .ok()
            .and_then(|at| self.archive_offset.checked_add(at))
            .filter(|at| *at < self.directory)
            .ok_or_else(|| malformed("ZIP local header position"))?;
        reader.seek(at)?;
        let header = reader.take(local::LEN)?;
        if !header.window().starts_with(b"PK\x03\x04") {
            return Err(malformed("ZIP local header"));
        }
        let mut fields = header;
        fields.req_u32_le()?;
        fields.req_u16_le()?;
        let flags = fields.req_u16_le()?;
        let method = fields.req_u16_le()?;
        if method != member.method || flags != member.flags {
            return Err(malformed("ZIP local member metadata"));
        }
        let name_size = usize::from(field16(header, local::NAME_LENGTH)?);
        let extra_size = usize::from(field16(header, local::EXTRA_LENGTH)?);
        let data = reader
            .position()
            .checked_add(name_size)
            .and_then(|at| at.checked_add(extra_size))
            .ok_or_else(|| malformed("ZIP local extent"))?;
        let size = usize::try_from(member.compressed).map_err(|_| StopReason::SourceBytes)?;
        let end = data
            .checked_add(size)
            .filter(|at| *at <= self.directory)
            .ok_or_else(|| malformed("ZIP payload extent"))?;
        reader.seek(data)?;
        let mut bytes = Vec::new();
        while reader.position() < end {
            self.budget.check()?;
            let size = (end - reader.position()).min(4096);
            self.budget.retain(u64_from_index(size))?;
            bytes
                .try_reserve_exact(size)
                .map_err(|_| StopReason::RetainedBytes)?;
            bytes.extend_from_slice(reader.take(size)?.window());
        }
        let output = match member.method {
            0 => bytes,
            method @ (8 | 93) => {
                let framing = if method == 8 {
                    InflateFraming::Raw
                } else {
                    InflateFraming::Zstd
                };
                let (output, consumed) = self.budget.inflate(self.budget.view(&bytes)?, framing)?;
                if consumed != bytes.len() {
                    return Err(malformed("ZIP compressed extent"));
                }
                output
            }
            method => return Err(StopReason::Unsupported(format!("ZIP method {method}"))),
        };
        if u64_from_index(output.len()) != member.expanded {
            return Err(malformed("ZIP member size"));
        }
        let mut checksum = crc32fast::Hasher::new();
        for chunk in output.chunks(4096) {
            self.budget.check()?;
            checksum.update(chunk);
        }
        if checksum.finalize() != member.crc {
            return Err(malformed("ZIP member checksum"));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
