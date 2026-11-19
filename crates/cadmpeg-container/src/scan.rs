// SPDX-License-Identifier: Apache-2.0
//! Feature-scan access to selected ZIP members.

use std::io::{Read, Seek, SeekFrom};

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

struct Input<'a, 'b> {
    view: View<'a>,
    budget: &'b ScanBudget<'b>,
    physical: bool,
}

impl Read for Input<'_, '_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let length = bytes.len().min(self.view.remaining());
        if self.physical {
            self.budget.source(u64_from_index(length))
        } else {
            self.budget.check()
        }
        .map_err(std::io::Error::other)?;
        let read = self
            .view
            .req_take(length)
            .map_err(|error| std::io::Error::other(StopReason::from(error)))?;
        bytes[..length].copy_from_slice(read);
        Ok(length)
    }
}

impl Seek for Input<'_, '_> {
    fn seek(&mut self, at: SeekFrom) -> std::io::Result<u64> {
        self.budget.check().map_err(std::io::Error::other)?;
        let start = self.view.start();
        let position = match at {
            SeekFrom::Start(at) => usize::try_from(at)
                .ok()
                .and_then(|at| start.checked_add(at)),
            SeekFrom::Current(delta) => i64::try_from(self.view.position())
                .ok()
                .and_then(|at| at.checked_add(delta))
                .and_then(|at| usize::try_from(at).ok()),
            SeekFrom::End(delta) => i64::try_from(self.view.end())
                .ok()
                .and_then(|at| at.checked_add(delta))
                .and_then(|at| usize::try_from(at).ok()),
        }
        .ok_or_else(|| std::io::Error::other("ZIP seek overflow"))?;
        self.view
            .seek(position)
            .ok_or_else(|| std::io::Error::other("ZIP seek outside source"))?;
        Ok(u64_from_index(position - start))
    }
}

fn zip_error(error: &zip::result::ZipError) -> StopReason {
    if matches!(error, zip::result::ZipError::UnsupportedArchive(_)) {
        return StopReason::Unsupported(error.to_string());
    }
    if let zip::result::ZipError::Io(error) = error {
        if let Some(reason) = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<StopReason>())
        {
            return reason.clone();
        }
    }
    StopReason::Malformed(error.to_string())
}

/// ZIP reader whose seeks and reads share the scan meter.
pub struct ScanZip<'a, 'b> {
    archive: zip::ZipArchive<Input<'a, 'b>>,
    budget: &'b ScanBudget<'b>,
    methods: Vec<(u16, u16)>,
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
        let input = Input {
            view: source,
            budget,
            physical,
        };
        let archive = zip::ZipArchive::new(input).map_err(|error| zip_error(&error))?;
        let mut methods = Vec::new();
        let directory_at = archive
            .offset()
            .checked_add(directory_position)
            .and_then(|at| usize::try_from(at).ok())
            .and_then(|at| source.start().checked_add(at))
            .ok_or(StopReason::SourceBytes)?;
        let directory_end = usize::try_from(directory_size)
            .ok()
            .and_then(|size| directory_at.checked_add(size))
            .ok_or(StopReason::SourceBytes)?;
        reader.seek(directory_at)?;
        for _ in 0..count {
            use crate::layout::central_header as central;
            budget.check()?;
            let header = reader.take(central::LEN)?;
            let bytes = header.window();
            if !bytes.starts_with(b"PK\x01\x02") {
                return Err(StopReason::Malformed("ZIP central header".into()));
            }
            let method = View::u16_le_at(bytes, central::COMPRESSION)
                .ok_or_else(|| StopReason::Truncated("ZIP compression method".into()))?;
            let flags = View::u16_le_at(bytes, central::FLAGS)
                .ok_or_else(|| StopReason::Truncated("ZIP member flags".into()))?;
            let name = usize::from(
                View::u16_le_at(bytes, central::NAME_LENGTH)
                    .ok_or_else(|| StopReason::Truncated("ZIP name length".into()))?,
            );
            let extra = usize::from(
                View::u16_le_at(bytes, central::EXTRA_LENGTH)
                    .ok_or_else(|| StopReason::Truncated("ZIP extra length".into()))?,
            );
            let comment = usize::from(
                View::u16_le_at(bytes, central::COMMENT_LENGTH)
                    .ok_or_else(|| StopReason::Truncated("ZIP comment length".into()))?,
            );
            let next = reader
                .position()
                .checked_add(name)
                .and_then(|at| at.checked_add(extra))
                .and_then(|at| at.checked_add(comment))
                .filter(|at| *at <= directory_end)
                .ok_or_else(|| StopReason::Malformed("ZIP central extent".into()))?;
            reader.seek(next)?;
            methods.push((method, flags));
        }
        if u64_from_index(archive.len()) != count {
            return Err(StopReason::Malformed("ZIP directory entry census".into()));
        }
        Ok(Self {
            archive,
            budget,
            methods,
        })
    }

    /// Lists directory entries in source order.
    pub fn entries(&mut self) -> Result<Vec<ScanZipEntry>, StopReason> {
        let mut entries = Vec::new();
        for ordinal in 0..self.archive.len() {
            self.budget.check()?;
            let member = self
                .archive
                .by_index_raw(ordinal)
                .map_err(|error| zip_error(&error))?;
            let name = member.name().map_err(|error| zip_error(&error))?;
            self.budget.retain(
                u64_from_index(name.len())
                    .checked_add(64)
                    .ok_or(StopReason::RetainedBytes)?,
            )?;
            entries.push(ScanZipEntry {
                ordinal,
                name: name.into_owned(),
                declared_size: member.size(),
            });
        }
        Ok(entries)
    }

    /// Acquires and expands one selected member in bounded steps.
    pub fn open(&mut self, ordinal: usize) -> Result<Vec<u8>, StopReason> {
        self.budget.check()?;
        let (method, flags) = self
            .methods
            .get(ordinal)
            .copied()
            .ok_or_else(|| StopReason::Malformed("ZIP member ordinal".into()))?;
        if flags & 1 != 0 || method == 99 {
            return Err(StopReason::Unsupported("encrypted ZIP listing".into()));
        }
        let mut member = self
            .archive
            .by_index_raw(ordinal)
            .map_err(|error| zip_error(&error))?;
        let size = member.size();
        let crc = member.crc32();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            self.budget.check()?;
            let count = member.read(&mut chunk).map_err(|error| {
                error
                    .get_ref()
                    .and_then(|inner| inner.downcast_ref::<StopReason>())
                    .cloned()
                    .unwrap_or_else(|| StopReason::Malformed(error.to_string()))
            })?;
            if count == 0 {
                break;
            }
            self.budget.retain(u64_from_index(count))?;
            bytes
                .try_reserve_exact(count)
                .map_err(|_| StopReason::RetainedBytes)?;
            bytes.extend_from_slice(&chunk[..count]);
        }
        let output = match method {
            0 => bytes,
            8 => {
                let (output, consumed) = self
                    .budget
                    .inflate(self.budget.view(&bytes)?, InflateFraming::Raw)?;
                if consumed != bytes.len() {
                    return Err(StopReason::Malformed("ZIP compressed extent".into()));
                }
                output
            }
            93 => {
                self.budget
                    .inflate(self.budget.view(&bytes)?, InflateFraming::Zstd)?
                    .0
            }
            method => return Err(StopReason::Unsupported(format!("ZIP method {method}"))),
        };
        if u64_from_index(output.len()) != size || crc32fast::hash(&output) != crc {
            return Err(StopReason::Malformed("ZIP member size or checksum".into()));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
