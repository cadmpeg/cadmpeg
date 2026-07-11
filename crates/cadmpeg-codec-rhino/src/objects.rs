// SPDX-License-Identifier: Apache-2.0
//! Rhino object-record identity and framing.

use std::fmt;
use std::ops::Range;

use crate::chunks::{chunk_at, verify_checksum, ArchiveVersion, ChecksumStatus, FramingError};
use crate::container::Record;

const OBJECT_RECORD_TYPE: u32 = 0x82a0_0071;
const OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const OBJECT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0073;
const OBJECT_RECORD_HISTORY: u32 = 0x0200_8074;
const OBJECT_RECORD_END: u32 = 0x82a0_007f;
const OPENNURBS_CLASS: u32 = 0x0002_7ffa;
const CLASS_USERDATA: u32 = 0x0002_7ffd;
const CLASS_USERDATA_HEADER: u32 = 0x0002_fff9;
const CLASS_UUID: u32 = 0x0002_fffb;
const CLASS_DATA: u32 = 0x0002_fffc;
const CLASS_END: u32 = 0x8202_7fff;
const ANONYMOUS: u32 = 0x4000_8000;
const HISTORY_HEADER: u32 = 0x0200_8075;
const HISTORY_DATA: u32 = 0x0200_8076;

/// A UUID in canonical textual byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Uuid {
    bytes: [u8; 16],
}

impl Uuid {
    /// Parses the mixed-endian UUID wire representation.
    pub(crate) fn from_wire(bytes: [u8; 16]) -> Self {
        let mut canonical = [0; 16];
        canonical[..4].copy_from_slice(&bytes[..4].iter().rev().copied().collect::<Vec<_>>());
        canonical[4..6].copy_from_slice(&bytes[4..6].iter().rev().copied().collect::<Vec<_>>());
        canonical[6..8].copy_from_slice(&bytes[6..8].iter().rev().copied().collect::<Vec<_>>());
        canonical[8..].copy_from_slice(&bytes[8..]);
        Self { bytes: canonical }
    }

    /// Returns the nil UUID.
    pub(crate) fn nil() -> Self {
        Self { bytes: [0; 16] }
    }

    /// Returns whether this UUID is nil.
    pub(crate) fn is_nil(self) -> bool {
        self == Self::nil()
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A class-userdata descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserdataDescriptor {
    /// Complete wrapper range.
    pub(crate) range: Range<usize>,
    /// Packed wrapper version.
    pub(crate) version: (u8, u8),
    /// Userdata class UUID.
    pub(crate) class_uuid: Uuid,
    /// Userdata item UUID.
    pub(crate) item_uuid: Uuid,
    /// Copy count.
    pub(crate) copy_count: i32,
    /// Transform byte range.
    pub(crate) transform_range: Range<usize>,
    /// Optional application UUID.
    pub(crate) application_uuid: Option<Uuid>,
    /// Optional last-saved-as-goo flag.
    pub(crate) last_saved_as_goo: Option<bool>,
    /// Optional userdata archive version.
    pub(crate) archive_version: Option<i32>,
    /// Optional userdata writer version.
    pub(crate) writer_version: Option<i32>,
    /// Anonymous payload range, excluding its framing.
    pub(crate) payload_range: Range<usize>,
    /// Unknown future-version payload range.
    pub(crate) unknown_version: bool,
}

/// A bounded object-history descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryDescriptor {
    /// Complete history wrapper range.
    pub(crate) range: Range<usize>,
    /// Packed history version.
    pub(crate) version: (u8, u8),
    /// History header child range.
    pub(crate) header_range: Range<usize>,
    /// History data child range.
    pub(crate) data_range: Range<usize>,
}

/// A fully framed Rhino object record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectDescriptor {
    /// Complete object-record range.
    pub(crate) range: Range<usize>,
    /// Object type filter bits.
    pub(crate) object_type: u32,
    /// Class UUID.
    pub(crate) class_uuid: Uuid,
    /// Class-data payload range.
    pub(crate) class_data_range: Range<usize>,
    /// Class userdata descriptors.
    pub(crate) userdata: Vec<UserdataDescriptor>,
    /// Optional attributes range.
    pub(crate) attributes_range: Option<Range<usize>>,
    /// Optional attribute-userdata range.
    pub(crate) attributes_userdata_range: Option<Range<usize>>,
    /// Optional history descriptor.
    pub(crate) history: Option<HistoryDescriptor>,
    /// Unknown bounded trailer child ranges.
    pub(crate) unknown_trailer: Vec<Range<usize>>,
    /// Checksum warning messages.
    pub(crate) checksum_warnings: Vec<String>,
}

struct Bytes<'a> {
    data: &'a [u8],
    position: usize,
    end: usize,
}

impl<'a> Bytes<'a> {
    fn new(bytes: &'a [u8], range: Range<usize>) -> Self {
        Self {
            data: bytes,
            position: range.start,
            end: range.end,
        }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], FramingError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(FramingError::Overflow {
                offset: self.position,
            })?;
        if end > self.end {
            return Err(FramingError::OutOfBounds {
                offset: self.position,
                end,
                bound: self.end,
            });
        }
        let result = &self.data[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn u8(&mut self) -> Result<u8, FramingError> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32, FramingError> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("length checked"),
        ))
    }

    fn uuid(&mut self) -> Result<Uuid, FramingError> {
        Ok(Uuid::from_wire(
            self.take(16)?.try_into().expect("length checked"),
        ))
    }

    fn remaining(&self) -> Range<usize> {
        self.position..self.end
    }
}

fn malformed(message: impl Into<String>) -> FramingError {
    FramingError::InvalidLength {
        offset: 0,
        value: message.into().len() as i128,
    }
}

fn child(
    bytes: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
    class_uuid: bool,
) -> Result<crate::chunks::Chunk, FramingError> {
    chunk_at(bytes, offset, end, archive, class_uuid)
}

fn require_long(chunk: &crate::chunks::Chunk, typecode: u32) -> Result<(), FramingError> {
    if chunk.typecode != typecode || chunk.short {
        return Err(malformed(format!(
            "expected long chunk {typecode:#x}, got {:#x}",
            chunk.typecode
        )));
    }
    Ok(())
}

fn require_short_zero(chunk: &crate::chunks::Chunk, typecode: u32) -> Result<(), FramingError> {
    if chunk.typecode != typecode || !chunk.short || chunk.value != 0 {
        return Err(malformed(format!(
            "expected short zero chunk {typecode:#x}, got {:#x}",
            chunk.typecode
        )));
    }
    Ok(())
}

fn chunk_range(chunk: &crate::chunks::Chunk, archive: ArchiveVersion) -> Range<usize> {
    let width = if archive.uses_eight_byte_values() {
        8
    } else {
        4
    };
    chunk.body_start - 4 - width..chunk.next_offset
}

fn checksum_warning(
    bytes: &[u8],
    chunk: &crate::chunks::Chunk,
) -> Result<Option<String>, FramingError> {
    match verify_checksum(bytes, chunk)? {
        ChecksumStatus::Mismatch { expected, actual } => {
            Ok(Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            chunk.body_start.saturating_sub(if chunk.short { 8 } else { 12 }),
            chunk.typecode
        )))
        }
        _ => Ok(None),
    }
}

fn parse_userdata(
    bytes: &[u8],
    wrapper: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<UserdataDescriptor, FramingError> {
    let mut reader = Bytes::new(bytes, wrapper.body.clone());
    let packed = reader.u8()?;
    let version = (packed >> 4, packed & 0x0f);
    if version.0 == 1 {
        let class_uuid = reader.uuid()?;
        let item_uuid = reader.uuid()?;
        let copy_count = reader.i32()?;
        let transform_start = reader.position;
        reader.take(16 * 8)?;
        let transform_range = transform_start..reader.position;
        return Ok(UserdataDescriptor {
            range: chunk_range(wrapper, archive),
            version,
            class_uuid,
            item_uuid,
            copy_count,
            transform_range,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: reader.remaining(),
            unknown_version: false,
        });
    }
    if version.0 != 2 {
        return Ok(UserdataDescriptor {
            range: chunk_range(wrapper, archive),
            version,
            class_uuid: Uuid::nil(),
            item_uuid: Uuid::nil(),
            copy_count: 0,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: wrapper.body.clone(),
            unknown_version: true,
        });
    }
    let header = child(bytes, reader.position, wrapper.body.end, archive, false)?;
    require_long(&header, CLASS_USERDATA_HEADER)?;
    if let Some(note) = checksum_warning(bytes, &header)? {
        warnings.push(note);
    }
    let mut header_reader = Bytes::new(bytes, header.body.clone());
    let class_uuid = header_reader.uuid()?;
    let item_uuid = header_reader.uuid()?;
    let copy_count = header_reader.i32()?;
    let transform_start = header_reader.position;
    header_reader.take(16 * 8)?;
    let transform_range = transform_start..header_reader.position;
    let application_uuid = (version.1 >= 1).then(|| header_reader.uuid()).transpose()?;
    let last_saved_as_goo = if version.1 >= 2 {
        Some(header_reader.u8()? != 0)
    } else {
        None
    };
    let archive_version = (version.1 >= 2).then(|| header_reader.i32()).transpose()?;
    let writer_version = (version.1 >= 2).then(|| header_reader.i32()).transpose()?;
    if header_reader.position != header.body.end {
        return Err(malformed("userdata header has trailing bytes"));
    }
    let payload = child(bytes, header.next_offset, wrapper.body.end, archive, false)?;
    require_long(&payload, ANONYMOUS)?;
    if let Some(note) = checksum_warning(bytes, &payload)? {
        warnings.push(note);
    }
    if payload.next_offset != wrapper.body.end {
        return Err(malformed("userdata wrapper has trailing bytes"));
    }
    Ok(UserdataDescriptor {
        range: chunk_range(wrapper, archive),
        version,
        class_uuid,
        item_uuid,
        copy_count,
        transform_range,
        application_uuid,
        last_saved_as_goo,
        archive_version,
        writer_version,
        payload_range: payload.body,
        unknown_version: false,
    })
}

fn parse_history(
    bytes: &[u8],
    wrapper: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<HistoryDescriptor, FramingError> {
    let mut reader = Bytes::new(bytes, wrapper.body.clone());
    let packed = reader.u8()?;
    let first = child(bytes, reader.position, wrapper.body.end, archive, false)?;
    require_long(&first, HISTORY_HEADER)?;
    if let Some(note) = checksum_warning(bytes, &first)? {
        warnings.push(note);
    }
    let second = child(bytes, first.next_offset, wrapper.body.end, archive, false)?;
    require_long(&second, HISTORY_DATA)?;
    if let Some(note) = checksum_warning(bytes, &second)? {
        warnings.push(note);
    }
    if second.next_offset != wrapper.body.end {
        return Err(malformed("history wrapper has trailing bytes"));
    }
    Ok(HistoryDescriptor {
        range: chunk_range(wrapper, archive),
        version: (packed >> 4, packed & 0x0f),
        header_range: chunk_range(&first, archive),
        data_range: chunk_range(&second, archive),
    })
}

/// Parses one bounded object record and returns identity plus child ranges.
pub(crate) fn parse_object_record(
    bytes: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<ObjectDescriptor, FramingError> {
    if record.typecode != 0x2000_8070 || record.short {
        return Err(malformed("object record must be long-framed"));
    }
    let mut offset = record.body.start;
    let type_chunk = child(bytes, offset, record.body.end, archive, false)?;
    if type_chunk.typecode != OBJECT_RECORD_TYPE || !type_chunk.short {
        return Err(malformed("object type must be the first short child"));
    }
    let object_type =
        u32::try_from(type_chunk.value).map_err(|_| malformed("negative object type"))?;
    offset = type_chunk.next_offset;
    let class = child(bytes, offset, record.body.end, archive, false)?;
    eprintln!("object {:?} class {:?} body {:?} record {:?}", record.range, chunk_range(&class, archive), class.body, record.body);
    require_long(&class, OPENNURBS_CLASS)?;
    offset = class.next_offset;
    let uuid_chunk = child(bytes, offset, class.body.end, archive, true)?;
    require_long(&uuid_chunk, CLASS_UUID)?;
    if uuid_chunk.declared_end - uuid_chunk.body_start != 20 {
        return Err(malformed("class UUID chunk must have a 20-byte body"));
    }
    if let Some(note) = checksum_warning(bytes, &uuid_chunk)? {
        warnings.push(note);
    }
    let class_uuid = Uuid::from_wire(
        bytes[uuid_chunk.body.clone()]
            .try_into()
            .expect("UUID length checked"),
    );
    offset = uuid_chunk.next_offset;
    let data_chunk = child(bytes, offset, class.body.end, archive, false)?;
    require_long(&data_chunk, CLASS_DATA)?;
    if let Some(note) = checksum_warning(bytes, &data_chunk)? {
        warnings.push(note);
    }
    let class_data_range = data_chunk.body.clone();
    offset = data_chunk.next_offset;
    let mut userdata = Vec::new();
    while offset < class.body.end {
        let item = child(bytes, offset, class.body.end, archive, false)?;
        if item.typecode == CLASS_USERDATA {
            require_long(&item, CLASS_USERDATA)?;
            userdata.push(parse_userdata(bytes, &item, archive, warnings)?);
            offset = item.next_offset;
        } else {
            require_short_zero(&item, CLASS_END)?;
            offset = item.next_offset;
            break;
        }
    }
    if offset != class.body.end {
        return Err(malformed("class wrapper has trailing bytes"));
    }
    let mut attributes_range = None;
    let mut attributes_userdata_range = None;
    let mut history = None;
    let mut unknown_trailer = Vec::new();
    let mut phase = 0_u8;
    eprintln!("trailer starts {offset}..{}", record.body.end);
    while offset < record.body.end {
        let item = child(bytes, offset, record.body.end, archive, false)?;
        if item.typecode == OBJECT_RECORD_END {
            require_short_zero(&item, OBJECT_RECORD_END)?;
            if item.next_offset != record.body.end {
                return Err(malformed("object end is not final"));
            }
            offset = item.next_offset;
            break;
        }
        match item.typecode {
            OBJECT_RECORD_ATTRIBUTES if phase == 0 => {
                require_long(&item, OBJECT_RECORD_ATTRIBUTES)?;
                attributes_range = Some(item.body.clone());
                phase = 1;
            }
            OBJECT_RECORD_ATTRIBUTES_USERDATA if phase <= 1 => {
                require_long(&item, OBJECT_RECORD_ATTRIBUTES_USERDATA)?;
                attributes_userdata_range = Some(item.body.clone());
                phase = 2;
            }
            OBJECT_RECORD_HISTORY if phase <= 2 => {
                require_long(&item, OBJECT_RECORD_HISTORY)?;
                history = Some(parse_history(bytes, &item, archive, warnings)?);
                phase = 3;
            }
            _ if !item.short && phase >= 3 => {
                unknown_trailer.push(chunk_range(&item, archive));
            }
            _ => {
                return Err(malformed(
                    "object trailer child is out of order or malformed",
                ))
            }
        }
        if let Some(note) = checksum_warning(bytes, &item)? {
            warnings.push(note);
        }
        offset = item.next_offset;
    }
    if offset != record.body.end {
        return Err(malformed("object record is missing object end"));
    }
    Ok(ObjectDescriptor {
        range: record.range.clone(),
        object_type,
        class_uuid,
        class_data_range,
        userdata,
        attributes_range,
        attributes_userdata_range,
        history,
        unknown_trailer,
        checksum_warnings: warnings.clone(),
    })
}
