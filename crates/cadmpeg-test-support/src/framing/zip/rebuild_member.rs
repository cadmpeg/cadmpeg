// SPDX-License-Identifier: Apache-2.0
//! Member recompression with physical ZIP metadata conservation.

use super::layout::{add, directory, u16_at, u32_at, u64_at};
use super::{frame, ZipEntry};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};

/// Replace one member's expanded content without changing its compression method.
///
/// Preserves every byte outside the member payload and the CRC, compressed-size,
/// and relocated-offset fields that depend on it. ZIP32, ZIP64, deferred local
/// values, descriptors, archive prefixes and comments retain their encoding.
///
/// # Errors
/// Refuses encryption, unsupported methods, a changed expanded length, signature
/// invalidation, scalar overflow, ambiguous framing or exhausted resources.
pub fn rebuild_member(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    name: &[u8],
    expanded: &[u8],
) -> Result<Vec<u8>, String> {
    let mut frames = Vec::new();
    let entries = frame(ctx, source, &mut frames)?;
    let index = entries
        .iter()
        .position(|entry| entry.name().window() == name)
        .ok_or("ZIP replacement member is absent")?;
    let entry = &entries[index];
    if entry.flags & 1 != 0 || entry.local_flags & 1 != 0 {
        return Err("ZIP member replacement cannot renew encryption".into());
    }
    if u64_from_index(expanded.len()) != entry.uncompressed {
        return Err("ZIP member replacement changes the expanded length".into());
    }
    let encoded = match entry.method {
        0 => None,
        8 => Some(deflate(ctx, expanded)?),
        _ => return Err("ZIP member replacement compression method is unsupported".into()),
    };
    let replacement = encoded.as_deref().unwrap_or(expanded);
    let old = source.window();
    let start = entry.payload.start() - source.start();
    let end = entry.payload.end() - source.start();
    let delta = i128::try_from(replacement.len()).map_err(|_| "ZIP replacement too large")?
        - i128::try_from(end - start).map_err(|_| "ZIP original payload too large")?;
    let new_len =
        usize::try_from(i128::try_from(old.len()).map_err(|_| "ZIP source too large")? + delta)
            .map_err(|_| "ZIP rebuilt length exceeds usize")?;
    ctx.retain_bytes(u64_from_index(new_len), "ZIP recompressed source")
        .map_err(|error| error.to_string())?;
    let mut bytes = ctx
        .alloc_filled(new_len, 0_u8, "ZIP recompressed source")
        .map_err(|error| error.to_string())?;
    bytes[..start].copy_from_slice(&old[..start]);
    let next = add(start, replacement.len())?;
    bytes[start..next].copy_from_slice(replacement);
    bytes[next..].copy_from_slice(&old[end..]);
    let mut patch = Patch {
        bytes: &mut bytes,
        old,
        end,
        delta,
    };
    patch.member(
        entry,
        u64_from_index(replacement.len()),
        crate::zip_fixture::crc32(expanded),
    )?;
    let directory = directory(old)?;
    for candidate in &entries {
        let central = candidate.central_crc - 16;
        let local = candidate.local_crc - 14;
        if local >= end {
            let scalar = central_offset(old, central)?;
            patch.shift_scalar(scalar)?;
        }
    }
    let end_offset = add(directory.end_record, 16)?;
    if u32_at(old, end_offset)? != u32::MAX {
        patch.shift_scalar((end_offset, false))?;
    }
    if let Some(wide) = directory.zip64 {
        patch.shift_scalar((add(wide.start, 48)?, true))?;
        patch.shift_scalar((add(wide.locator, 8)?, true))?;
    }
    if let Some(signature) = directory.signature {
        let relocated_start = patch.position(directory.start)?;
        let relocated_end = patch.position(signature.start)?;
        if patch.bytes.get(relocated_start..relocated_end)
            != old.get(directory.start..signature.start)
        {
            return Err("ZIP replacement invalidates the directory signature".into());
        }
    }
    Ok(bytes)
}

fn deflate(ctx: &DecodeContext<'_>, expanded: &[u8]) -> Result<Vec<u8>, String> {
    let length = expanded.len();
    let bound = length
        .checked_add(length >> 12)
        .and_then(|n| n.checked_add(length >> 14))
        .and_then(|n| n.checked_add(length >> 25))
        .and_then(|n| n.checked_add(13))
        .ok_or("ZIP deflate bound overflows")?;
    ctx.retain_bytes(u64_from_index(bound), "ZIP recompression buffer")
        .map_err(|error| error.to_string())?;
    let mut bytes = ctx
        .alloc_filled(bound, 0_u8, "ZIP recompression buffer")
        .map_err(|error| error.to_string())?;
    let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
    let status = compressor
        .compress(expanded, &mut bytes, flate2::FlushCompress::Finish)
        .map_err(|error| error.to_string())?;
    if status != flate2::Status::StreamEnd || compressor.total_in() != u64_from_index(length) {
        return Err("ZIP compressor did not complete within its bounded buffer".into());
    }
    bytes.truncate(
        usize::try_from(compressor.total_out()).map_err(|_| "ZIP deflate output exceeds usize")?,
    );
    Ok(bytes)
}

struct Patch<'a> {
    bytes: &'a mut [u8],
    old: &'a [u8],
    end: usize,
    delta: i128,
}
impl Patch<'_> {
    fn position(&self, old: usize) -> Result<usize, String> {
        let value = i128::try_from(old).map_err(|_| "ZIP field offset exceeds i128")?;
        usize::try_from(value + if old >= self.end { self.delta } else { 0 })
            .map_err(|_| "ZIP relocated field exceeds usize".into())
    }
    fn put(&mut self, at: usize, wide: bool, value: u64) -> Result<(), String> {
        let at = self.position(at)?;
        if wide {
            self.bytes
                .get_mut(at..add(at, 8)?)
                .ok_or("ZIP field exceeds rebuilt source")?
                .copy_from_slice(&value.to_le_bytes());
        } else {
            let value = u32::try_from(value).map_err(|_| "ZIP32 scalar needs a new ZIP64 field")?;
            self.bytes
                .get_mut(at..add(at, 4)?)
                .ok_or("ZIP field exceeds rebuilt source")?
                .copy_from_slice(&value.to_le_bytes());
        }
        Ok(())
    }
    fn shift_scalar(&mut self, (at, wide): (usize, bool)) -> Result<(), String> {
        let value = if wide {
            u64_at(self.old, at)?
        } else {
            u64::from(u32_at(self.old, at)?)
        };
        let value = u64::try_from(i128::from(value) + self.delta)
            .map_err(|_| "ZIP relocated offset exceeds u64")?;
        self.put(at, wide, value)
    }
    fn member(&mut self, entry: &ZipEntry<'_>, compressed: u64, crc: u32) -> Result<(), String> {
        let central = entry.central_crc - 16;
        let local = entry.local_crc - 14;
        self.put(entry.central_crc, false, u64::from(crc))?;
        if entry.local_flags & 8 == 0 || u32_at(self.old, entry.local_crc)? != 0 {
            self.put(entry.local_crc, false, u64::from(crc))?;
        }
        let central_sizes = central_sizes(self.old, central)?;
        self.put(central_sizes.1 .0, central_sizes.1 .1, compressed)?;
        let name_end = add(
            add(local, 30)?,
            usize::from(u16_at(self.old, add(local, 26)?)?),
        )?;
        let extra_end = add(name_end, usize::from(u16_at(self.old, add(local, 28)?)?))?;
        let local_compressed = add(local, 18)?;
        let local_expanded = add(local, 22)?;
        if u32_at(self.old, local_compressed)? == u32::MAX {
            let extra = super::zip64::field(self.old, name_end..extra_end)?
                .ok_or("ZIP local size sentinel has no extra")?;
            self.put(add(extra.start, 8)?, true, compressed)?;
        } else if entry.local_flags & 8 == 0 || u32_at(self.old, local_compressed)? != 0 {
            self.put(local_compressed, false, compressed)?;
        }
        if let Some(descriptor) = entry.descriptor_crc {
            let wide = central_sizes.0 .1
                || central_sizes.1 .1
                || u32_at(self.old, local_compressed)? == u32::MAX
                || u32_at(self.old, local_expanded)? == u32::MAX
                || super::zip64::field(self.old, name_end..extra_end)?.is_some();
            self.put(descriptor, false, u64::from(crc))?;
            self.put(add(descriptor, 4)?, wide, compressed)?;
        }
        Ok(())
    }
}

type Scalar = (usize, bool);
fn central_sizes(bytes: &[u8], central: usize) -> Result<(Scalar, Scalar), String> {
    let compressed = add(central, 20)?;
    let expanded = add(central, 24)?;
    let offset = add(central, 42)?;
    let fields = [
        u32_at(bytes, expanded)?,
        u32_at(bytes, compressed)?,
        u32_at(bytes, offset)?,
    ];
    let name_end = add(
        add(central, 46)?,
        usize::from(u16_at(bytes, add(central, 28)?)?),
    )?;
    let extra_end = add(name_end, usize::from(u16_at(bytes, add(central, 30)?)?))?;
    let extra = super::zip64::field(bytes, name_end..extra_end)?;
    let mut position = extra.map(|range| range.start);
    let mut scalars = [(expanded, false), (compressed, false)];
    for (index, scalar) in scalars.iter_mut().enumerate() {
        if fields[index] == u32::MAX {
            let at = position.ok_or("ZIP64 size has no actual extra field")?;
            *scalar = (at, true);
            position = Some(add(at, 8)?);
        }
    }
    Ok((scalars[0], scalars[1]))
}
fn central_offset(bytes: &[u8], central: usize) -> Result<Scalar, String> {
    let at = add(central, 42)?;
    if u32_at(bytes, at)? != u32::MAX {
        return Ok((at, false));
    }
    let name_end = add(
        add(central, 46)?,
        usize::from(u16_at(bytes, add(central, 28)?)?),
    )?;
    let extra_end = add(name_end, usize::from(u16_at(bytes, add(central, 30)?)?))?;
    let extra =
        super::zip64::field(bytes, name_end..extra_end)?.ok_or("ZIP64 offset has no extra")?;
    let mut position = extra.start;
    for field in [24, 20] {
        if u32_at(bytes, add(central, field)?)? == u32::MAX {
            position = add(position, 8)?;
        }
    }
    Ok((position, true))
}
