// SPDX-License-Identifier: Apache-2.0
//! Bounded Compound File Binary container reader.

use std::collections::BTreeSet;

const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
const FREE_SECTOR: u32 = 0xffff_ffff;
const END_OF_CHAIN: u32 = 0xffff_fffe;
const FAT_SECTOR: u32 = 0xffff_fffd;
const DIFAT_SECTOR: u32 = 0xffff_fffc;
const NO_STREAM: u32 = 0xffff_ffff;
const MAX_DECODED_STREAM: usize = 512 * 1024 * 1024;
const WRAPPED_PAYLOAD_MAGIC: [u8; 16] = [
    0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99,
];

#[derive(Debug, Clone)]
pub(crate) struct Stream {
    pub(crate) path: String,
    pub(crate) directory_id: u32,
    pub(crate) start_sector: u32,
    pub(crate) bytes: Vec<u8>,
    pub(crate) decoded_bytes: Option<Vec<u8>>,
}

#[derive(Clone)]
struct DirectoryEntry {
    name: String,
    object_type: u8,
    left: u32,
    right: u32,
    child: u32,
    start_sector: u32,
    size: u64,
}

pub(crate) fn streams(bytes: &[u8]) -> Option<Vec<Stream>> {
    let file = CompoundFile::parse(bytes)?;
    file.read_streams()
}

struct CompoundFile<'a> {
    bytes: &'a [u8],
    sector_size: usize,
    mini_sector_size: usize,
    mini_stream_cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    directory: Vec<DirectoryEntry>,
}

impl<'a> CompoundFile<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.get(..8)? != MAGIC
            || bytes.get(8..24)? != [0; 16]
            || le_u16(bytes, 24)? != 0x003e
            || le_u16(bytes, 28)? != 0xfffe
        {
            return None;
        }
        let major_version = le_u16(bytes, 26)?;
        let sector_shift = le_u16(bytes, 30)?;
        let mini_sector_shift = le_u16(bytes, 32)?;
        if !matches!((major_version, sector_shift), (3, 9) | (4, 12))
            || mini_sector_shift != 6
            || bytes.get(34..40)? != [0; 6]
        {
            return None;
        }
        let sector_size = 1usize.checked_shl(u32::from(sector_shift))?;
        if bytes.len() < sector_size || !(bytes.len() - sector_size).is_multiple_of(sector_size) {
            return None;
        }
        let sector_count = (bytes.len() - sector_size) / sector_size;
        let directory_sector_count = usize::try_from(le_u32(bytes, 40)?).ok()?;
        let fat_count = usize::try_from(le_u32(bytes, 44)?).ok()?;
        let directory_start = le_u32(bytes, 48)?;
        let mini_stream_cutoff = u64::from(le_u32(bytes, 56)?);
        let mini_fat_start = le_u32(bytes, 60)?;
        let mini_fat_count = usize::try_from(le_u32(bytes, 64)?).ok()?;
        let difat_start = le_u32(bytes, 68)?;
        let difat_count = usize::try_from(le_u32(bytes, 72)?).ok()?;
        if (major_version == 3 && directory_sector_count != 0)
            || mini_stream_cutoff != 4096
            || fat_count > sector_count
            || difat_count > sector_count
        {
            return None;
        }

        let sector = |id: u32| sector_slice(bytes, sector_size, sector_count, id);
        let mut fat_sectors = Vec::with_capacity(fat_count);
        for index in 0..109 {
            let id = le_u32(bytes, 76 + index * 4)?;
            if id != FREE_SECTOR {
                fat_sectors.push(id);
            }
        }
        let mut next_difat = difat_start;
        let difat_entries = sector_size / 4 - 1;
        let mut seen_difat = BTreeSet::new();
        for _ in 0..difat_count {
            if next_difat >= sector_count as u32 || !seen_difat.insert(next_difat) {
                return None;
            }
            let data = sector(next_difat)?;
            for index in 0..difat_entries {
                let id = le_u32(data, index * 4)?;
                if id != FREE_SECTOR {
                    fat_sectors.push(id);
                }
            }
            next_difat = le_u32(data, difat_entries * 4)?;
        }
        if (difat_count == 0 && difat_start != END_OF_CHAIN)
            || (difat_count != 0 && next_difat != END_OF_CHAIN)
            || fat_sectors.len() != fat_count
            || fat_sectors.iter().any(|id| *id >= sector_count as u32)
        {
            return None;
        }
        let mut unique_fat = BTreeSet::new();
        if !fat_sectors.iter().all(|id| unique_fat.insert(*id)) {
            return None;
        }
        let mut fat = Vec::with_capacity(fat_count.saturating_mul(sector_size / 4));
        for &id in &fat_sectors {
            let data = sector(id)?;
            fat.extend((0..sector_size / 4).filter_map(|index| le_u32(data, index * 4)));
        }
        if fat.len() < sector_count {
            return None;
        }
        if fat_sectors
            .iter()
            .any(|id| fat.get(*id as usize) != Some(&FAT_SECTOR))
            || seen_difat
                .iter()
                .any(|id| fat.get(*id as usize) != Some(&DIFAT_SECTOR))
        {
            return None;
        }

        let directory_size = (major_version == 4)
            .then(|| directory_sector_count.checked_mul(sector_size))
            .flatten();
        let directory_bytes = read_regular_chain(
            bytes,
            sector_size,
            sector_count,
            &fat,
            directory_start,
            directory_size,
        )?;
        let directory = parse_directory(&directory_bytes, major_version)?;
        if directory.first()?.object_type != 5 {
            return None;
        }

        let mini_fat = if mini_fat_count == 0 {
            if mini_fat_start != END_OF_CHAIN {
                return None;
            }
            Vec::new()
        } else {
            let expected = mini_fat_count.checked_mul(sector_size)?;
            let data = read_regular_chain(
                bytes,
                sector_size,
                sector_count,
                &fat,
                mini_fat_start,
                Some(expected),
            )?;
            data.chunks_exact(4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
                .collect()
        };

        Some(Self {
            bytes,
            sector_size,
            mini_sector_size: 64,
            mini_stream_cutoff,
            fat,
            mini_fat,
            directory,
        })
    }

    fn read_streams(&self) -> Option<Vec<Stream>> {
        let sector_count = (self.bytes.len() - self.sector_size) / self.sector_size;
        let root = self.directory.first()?;
        let mini_stream = if root.size == 0 {
            Vec::new()
        } else {
            read_regular_chain(
                self.bytes,
                self.sector_size,
                sector_count,
                &self.fat,
                root.start_sector,
                Some(usize::try_from(root.size).ok()?),
            )?
        };
        let mut stream_ids = Vec::new();
        let mut visited = BTreeSet::new();
        let mut pending = vec![(root.child, String::new())];
        while let Some((id, parent)) = pending.pop() {
            if id == NO_STREAM {
                continue;
            }
            let index = usize::try_from(id).ok()?;
            let entry = self.directory.get(index)?;
            if entry.object_type == 0 || entry.object_type == 5 || !visited.insert(id) {
                return None;
            }
            let path = if parent.is_empty() {
                entry.name.clone()
            } else {
                format!("{parent}/{}", entry.name)
            };
            pending.push((entry.right, parent.clone()));
            if entry.object_type == 2 {
                stream_ids.push((index, path));
            } else {
                pending.push((entry.child, path));
            }
            pending.push((entry.left, parent));
        }
        if self
            .directory
            .iter()
            .enumerate()
            .skip(1)
            .any(|(id, entry)| entry.object_type != 0 && !visited.contains(&(id as u32)))
        {
            return None;
        }
        let mut streams = Vec::with_capacity(stream_ids.len());
        for (id, path) in stream_ids {
            let entry = self.directory.get(id)?;
            let size = usize::try_from(entry.size).ok()?;
            let payload = if entry.size < self.mini_stream_cutoff {
                read_mini_chain(
                    &mini_stream,
                    self.mini_sector_size,
                    &self.mini_fat,
                    entry.start_sector,
                    size,
                )?
            } else {
                read_regular_chain(
                    self.bytes,
                    self.sector_size,
                    sector_count,
                    &self.fat,
                    entry.start_sector,
                    Some(size),
                )?
            };
            streams.push(Stream {
                path,
                directory_id: u32::try_from(id).ok()?,
                start_sector: entry.start_sector,
                decoded_bytes: decode_wrapped_payload(&payload),
                bytes: payload,
            });
        }
        Some(streams)
    }
}

fn decode_wrapped_payload(payload: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;

    if payload.get(..16)? != WRAPPED_PAYLOAD_MAGIC {
        return None;
    }
    let uncompressed_size = le_u32(payload, 16)? as usize;
    let compressed_size = le_u32(payload, 20)? as usize;
    if uncompressed_size == 0 || uncompressed_size > MAX_DECODED_STREAM || compressed_size == 0 {
        return None;
    }
    let member = payload.get(24..24usize.checked_add(compressed_size)?)?;
    let mut decoder = flate2::read::ZlibDecoder::new(member);
    let mut decoded = Vec::with_capacity(uncompressed_size.min(1 << 20));
    decoder.read_to_end(&mut decoded).ok()?;
    (decoded.len() == uncompressed_size && decoder.total_in() as usize == compressed_size)
        .then_some(decoded)
}

fn parse_directory(bytes: &[u8], major_version: u16) -> Option<Vec<DirectoryEntry>> {
    let mut entries = Vec::with_capacity(bytes.len() / 128);
    for raw in bytes.chunks_exact(128) {
        let name_len = usize::from(le_u16(raw, 64)?);
        let object_type = *raw.get(66)?;
        let name = if object_type == 0 {
            String::new()
        } else {
            if !(2..=64).contains(&name_len)
                || name_len % 2 != 0
                || raw.get(name_len - 2..name_len)? != [0, 0]
            {
                return None;
            }
            let units = raw[..name_len - 2]
                .chunks_exact(2)
                .map(|word| u16::from_le_bytes([word[0], word[1]]));
            String::from_utf16(&units.collect::<Vec<_>>()).ok()?
        };
        if !matches!(object_type, 0 | 1 | 2 | 5) {
            return None;
        }
        let mut size = le_u64(raw, 120)?;
        if major_version == 3 {
            size &= 0xffff_ffff;
        }
        entries.push(DirectoryEntry {
            name,
            object_type,
            left: le_u32(raw, 68)?,
            right: le_u32(raw, 72)?,
            child: le_u32(raw, 76)?,
            start_sector: le_u32(raw, 116)?,
            size,
        });
    }
    Some(entries)
}

fn read_regular_chain(
    bytes: &[u8],
    sector_size: usize,
    sector_count: usize,
    fat: &[u32],
    start: u32,
    size: Option<usize>,
) -> Option<Vec<u8>> {
    read_chain(start, fat, sector_count, size, sector_size, |id| {
        sector_slice(bytes, sector_size, sector_count, id)
    })
}

fn read_mini_chain(
    mini_stream: &[u8],
    sector_size: usize,
    fat: &[u32],
    start: u32,
    size: usize,
) -> Option<Vec<u8>> {
    let sector_count = mini_stream.len().div_ceil(sector_size);
    read_chain(start, fat, sector_count, Some(size), sector_size, |id| {
        let start = usize::try_from(id).ok()?.checked_mul(sector_size)?;
        mini_stream.get(start..start.checked_add(sector_size)?)
    })
}

fn read_chain<'a>(
    start: u32,
    fat: &[u32],
    sector_count: usize,
    size: Option<usize>,
    sector_size: usize,
    mut sector: impl FnMut(u32) -> Option<&'a [u8]>,
) -> Option<Vec<u8>> {
    if size == Some(0) {
        return (start == END_OF_CHAIN || start == FREE_SECTOR).then(Vec::new);
    }
    let max_sectors = size.map_or(sector_count, |length| length.div_ceil(sector_size));
    let mut output = Vec::with_capacity(size.unwrap_or(0));
    let mut seen = BTreeSet::new();
    let mut current = start;
    while current != END_OF_CHAIN {
        if current >= sector_count as u32 || !seen.insert(current) || seen.len() > max_sectors {
            return None;
        }
        output.extend_from_slice(sector(current)?);
        current = *fat.get(usize::try_from(current).ok()?)?;
        if matches!(current, FREE_SECTOR | FAT_SECTOR | DIFAT_SECTOR) {
            return None;
        }
    }
    if let Some(length) = size {
        if output.len() < length || seen.len() != length.div_ceil(sector_size) {
            return None;
        }
        output.truncate(length);
    }
    Some(output)
}

fn sector_slice(bytes: &[u8], sector_size: usize, count: usize, id: u32) -> Option<&[u8]> {
    let index = usize::try_from(id).ok()?;
    if index >= count {
        return None;
    }
    let start = sector_size.checked_add(index.checked_mul(sector_size)?)?;
    bytes.get(start..start.checked_add(sector_size)?)
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const SECTOR_SIZE: usize = 512;

    #[test]
    fn reads_storage_paths_and_both_stream_allocation_modes() {
        let file = fixture();
        let streams = streams(&file).expect("valid compound file");
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].path, "Small");
        assert_eq!(streams[0].bytes, b"small");
        assert_eq!(streams[1].path, "Store/Large");
        assert_eq!(streams[1].bytes, vec![0x5a; 4096]);
    }

    #[test]
    fn rejects_cyclic_stream_sector_chains() {
        let mut file = fixture();
        put_u32(sector_mut(&mut file, 11), 2 * 4, 2);
        assert!(streams(&file).is_none());
    }

    #[test]
    fn exposes_compound_parasolid_streams_to_the_container_scan() {
        let mut file = fixture();
        let mut payload = b"PS\0\0".to_vec();
        payload.extend_from_slice(&14u16.to_be_bytes());
        payload.extend_from_slice(b"partition body");
        payload.extend_from_slice(&[0, 0]);
        payload.push(18);
        payload.extend_from_slice(b"SCH_SW_33103_11000");
        sector_mut(&mut file, 2)[..payload.len()].copy_from_slice(&payload);

        let scan = crate::container::scan_bytes(&file);
        let partition = scan
            .compound_streams
            .iter()
            .find(|stream| stream.path == "Store/Large")
            .expect("compound partition stream");
        assert_eq!(partition.ps_streams.len(), 1);
        assert!(crate::container::has_parasolid_body_stream(&scan));
        assert!(crate::container::summarize(&scan)
            .notes
            .iter()
            .any(|note| note.contains("active Parasolid B-rep candidate: Store/Large")));
    }

    #[test]
    fn exposes_inflated_zlb_bytes_as_the_semantic_section_payload() {
        let mut file = fixture();
        let semantic = b"semantic payload";
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(semantic)
            .expect("compress fixture payload");
        let compressed = encoder.finish().expect("finish fixture payload");
        let mut wrapped = WRAPPED_PAYLOAD_MAGIC.to_vec();
        wrapped.extend_from_slice(&(semantic.len() as u32).to_le_bytes());
        wrapped.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        wrapped.extend_from_slice(&compressed);
        wrapped.extend_from_slice(&[0; 8]);
        sector_mut(&mut file, 2)[..wrapped.len()].copy_from_slice(&wrapped);

        let scan = crate::container::scan_bytes(&file);
        let section = scan
            .sections()
            .find(|section| section.name() == Some("Store/Large"))
            .expect("compound semantic section");
        assert_eq!(section.payload(), semantic);
    }

    fn fixture() -> Vec<u8> {
        let mut file = vec![0u8; SECTOR_SIZE * 13];
        file[..8].copy_from_slice(&MAGIC);
        put_u16(&mut file, 24, 0x003e);
        put_u16(&mut file, 26, 3);
        put_u16(&mut file, 28, 0xfffe);
        put_u16(&mut file, 30, 9);
        put_u16(&mut file, 32, 6);
        put_u32(&mut file, 44, 1);
        put_u32(&mut file, 48, 0);
        put_u32(&mut file, 56, 4096);
        put_u32(&mut file, 60, 10);
        put_u32(&mut file, 64, 1);
        put_u32(&mut file, 68, END_OF_CHAIN);
        for index in 0..109 {
            put_u32(&mut file, 76 + index * 4, FREE_SECTOR);
        }
        put_u32(&mut file, 76, 11);

        let directory = sector_mut(&mut file, 0);
        directory_entry(
            directory,
            0,
            "Root Entry",
            5,
            NO_STREAM,
            NO_STREAM,
            1,
            1,
            512,
        );
        directory_entry(directory, 1, "Small", 2, NO_STREAM, 2, NO_STREAM, 0, 5);
        directory_entry(directory, 2, "Store", 1, NO_STREAM, NO_STREAM, 3, 0, 0);
        directory_entry(
            directory, 3, "Large", 2, NO_STREAM, NO_STREAM, NO_STREAM, 2, 4096,
        );

        sector_mut(&mut file, 1)[..5].copy_from_slice(b"small");
        for id in 2..=9 {
            sector_mut(&mut file, id).fill(0x5a);
        }
        let mini_fat = sector_mut(&mut file, 10);
        mini_fat.fill(0xff);
        put_u32(mini_fat, 0, END_OF_CHAIN);

        let fat = sector_mut(&mut file, 11);
        fat.fill(0xff);
        put_u32(fat, 0, END_OF_CHAIN);
        put_u32(fat, 4, END_OF_CHAIN);
        for id in 2..9 {
            put_u32(
                fat,
                id * 4,
                u32::try_from(id + 1).expect("fixture sector id fits u32"),
            );
        }
        put_u32(fat, 9 * 4, END_OF_CHAIN);
        put_u32(fat, 10 * 4, END_OF_CHAIN);
        put_u32(fat, 11 * 4, FAT_SECTOR);
        file
    }

    #[allow(clippy::too_many_arguments)]
    fn directory_entry(
        directory: &mut [u8],
        index: usize,
        name: &str,
        object_type: u8,
        left: u32,
        right: u32,
        child: u32,
        start_sector: u32,
        size: u64,
    ) {
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let encoded = name.encode_utf16().collect::<Vec<_>>();
        for (offset, unit) in encoded.iter().enumerate() {
            put_u16(entry, offset * 2, *unit);
        }
        put_u16(
            entry,
            64,
            u16::try_from((encoded.len() + 1) * 2).expect("fixture name length fits u16"),
        );
        entry[66] = object_type;
        entry[67] = 1;
        put_u32(entry, 68, left);
        put_u32(entry, 72, right);
        put_u32(entry, 76, child);
        put_u32(entry, 116, start_sector);
        entry[120..128].copy_from_slice(&size.to_le_bytes());
    }

    fn sector_mut(file: &mut [u8], id: usize) -> &mut [u8] {
        let start = SECTOR_SIZE * (id + 1);
        &mut file[start..start + SECTOR_SIZE]
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
