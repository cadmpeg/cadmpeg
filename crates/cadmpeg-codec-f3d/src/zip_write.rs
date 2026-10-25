//! Deterministic ZIP write options for Fusion archives.
//!
//! The `zip` crate defaults `version made by` to the host OS (`Unix` on macOS
//! and Linux, `Dos` on Windows). Pin Unix so source-less generation, retained
//! patching, and synthetic fixtures stay byte-identical across CI platforms.
//!
//! The workspace builds `zip` without its `zstd` feature (a libzstd binding),
//! so `zip` can neither expand nor write a Zstandard (method 93) member. A
//! rewrite loop asks [`is_zstd_member`] first, expands such a member with
//! [`expand_zstd_member`], and writes it back with [`write_zstd_member`].

use std::io::{Read, Seek, Write};

use cadmpeg_container::compression::expand_zstd_member_owned;
use cadmpeg_core::decode::{DecodeContext, ExpandSpec};
use cadmpeg_core::CodecError;
use zip::read::ZipFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, System, ZipArchive, ZipWriter};

use crate::container::{read_entry_bounded, MAX_INFLATED_ENTRY_BYTES};

/// File options with a fixed Unix host OS for cross-platform archive bytes.
pub(crate) fn file_options(method: CompressionMethod) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(method)
        .system(System::Unix)
}

/// Whether member `index` is Zstandard, which `zip` cannot expand or write.
///
/// Asked through `by_index_raw`, which builds no decompressor, so it answers
/// for a method `zip` would refuse to open.
pub(crate) fn is_zstd_member<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
) -> Result<bool, CodecError> {
    let entry = archive
        .by_index_raw(index)
        .map_err(|error| CodecError::malformed(format_args!("invalid ZIP entry: {error}")))?;
    Ok(entry.compression() == CompressionMethod::ZSTD)
}

/// The expanded bytes of a Zstandard member, opened with `by_index_raw`.
///
/// Expanded by [`cadmpeg_container::compression::expand_zstd_member_owned`],
/// held to the member's declared size and CRC-32 and to
/// [`MAX_INFLATED_ENTRY_BYTES`]. The context carries the actual containing
/// archive's input length and shares the caller's encode account.
pub(crate) fn expand_zstd_member<R: Read>(
    ctx: &DecodeContext<'_>,
    entry: &mut ZipFile<'_, R>,
    name: &str,
) -> Result<Vec<u8>, CodecError> {
    let declared_size = entry.size();
    if declared_size > MAX_INFLATED_ENTRY_BYTES {
        return Err(CodecError::malformed(format_args!(
            "ZIP entry {name} exceeds the {MAX_INFLATED_ENTRY_BYTES}-byte inflated limit"
        )));
    }
    let crc32 = entry.crc32();
    let compressed_size = entry.compressed_size();
    let stored = read_entry_bounded(entry, compressed_size, name)?;
    expand_zstd_member_owned(ctx, &stored, ExpandSpec::Exact(declared_size), crc32).map_err(
        |error| match error {
            CodecError::Malformed(message) => {
                CodecError::malformed(format_args!("ZIP entry {name}: {message}"))
            }
            other => other,
        },
    )
}

/// Writes Zstandard member `index` back after a rewrite loop has read it.
///
/// Unchanged (`bytes == expanded`), the member is copied as stored, so the
/// output carries the source's frame byte for byte. Changed, it cannot be
/// written as Zstandard and is written as Deflate, with a note naming it.
pub(crate) fn write_zstd_member<R: Read + Seek, W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    archive: &mut ZipArchive<R>,
    index: usize,
    expanded: &[u8],
    bytes: &[u8],
    notes: &mut Vec<String>,
) -> Result<(), CodecError> {
    let entry = archive
        .by_index_raw(index)
        .map_err(|error| CodecError::malformed(format_args!("invalid ZIP entry: {error}")))?;
    if bytes == expanded {
        return zip.raw_copy_file(entry).map_err(|error| {
            CodecError::malformed(format_args!("cannot copy Zstandard ZIP entry: {error}"))
        });
    }
    let name = entry
        .name()
        .map_err(|error| CodecError::malformed(format_args!("invalid ZIP entry name: {error}")))?
        .into_owned();
    drop(entry);
    zip.start_file(name.as_str(), file_options(CompressionMethod::Deflated))
        .map_err(|error| CodecError::malformed(format_args!("cannot write ZIP entry: {error}")))?;
    zip.write_all(bytes)?;
    notes.push(format!(
        "ZIP entry {name} was Zstandard and is written as Deflate: it was edited, and this \
         build has no Zstandard encoder"
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    use zip::ZipArchive;

    use super::{expand_zstd_member, is_zstd_member};

    /// A member larger than the 16 MiB a context with no input allows expands, because its
    /// allowances are proportional to the archive that holds it, as on read.
    #[test]
    fn a_zstandard_member_above_the_base_allowance_expands() {
        let plain = (0..17_u32 * 1024 * 1024)
            .map(|index| u8::try_from(index % 251).expect("below 251"))
            .collect::<Vec<_>>();
        let archive = zip_fixture::archive(&[RawEntry::compressed(
            "large.bin",
            zip_fixture::ZSTD,
            &zip_fixture::zstd_stored_frame(&plain),
            &plain,
        )]);
        let archive_len = cadmpeg_core::decode::u64_from_index(archive.len());
        let mut zip = ZipArchive::new(Cursor::new(archive.as_slice())).expect("open the archive");
        assert!(is_zstd_member(&mut zip, 0).expect("read the method"));
        let mut entry = zip.by_index_raw(0).expect("open the member raw");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let ctx = cadmpeg_core::decode::DecodeContext::for_loaded_input(
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
            archive_len,
        )
        .expect("actual archive input allowance");
        let expanded = expand_zstd_member(&ctx, &mut entry, "large.bin")
            .expect("expand a member above the base allowance");
        assert!(expanded == plain, "the member expands to its plaintext");
    }
}
