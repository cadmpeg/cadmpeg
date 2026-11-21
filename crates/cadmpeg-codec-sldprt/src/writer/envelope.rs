// SPDX-License-Identifier: Apache-2.0
//! Native ZIP envelope regeneration: retained signature tokens where the
//! source envelope is proved, synthesized tokens otherwise.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use sha2::{Digest, Sha256};

use crate::container::{native_layout, ContainerScan};
use crate::layout::block_frame_header as local;
use crate::layout::outer_header as outer;
use crate::layout::zip_central_descriptor as descriptor;
use crate::layout::zip_central_header as central;

/// The outer header's big-endian `version` word (§1.1).
const NATIVE_VERSION: [u8; 4] = 4u32.to_be_bytes();

/// A central header's `version_made_by` (§1.3).
const VERSION_MADE_BY: [u8; 2] = [0; 2];

/// The residual fields of a synthesized envelope: one `file_id` and one token
/// of each kind per file (§1.1, §1.3, §1.4).
const SYNTHESIZED_FIELDS: [&str; 4] = [
    "outer header file_id",
    "local signature",
    "central signature",
    "end-of-directory signature",
];

/// A member with no source time/date word takes this word plus its index.
const PLACEHOLDER_TIME_DATE: u32 = 0x20;

/// The first nonzero word of SHA-256(`parts`, counter) that `taken` does not
/// hold.
fn derived_word(parts: &[&[u8]], taken: &[[u8; 4]]) -> [u8; 4] {
    (0u32..)
        .find_map(|counter| {
            let mut hasher = Sha256::new();
            for part in parts {
                hasher.update(part);
            }
            hasher.update(counter.to_le_bytes());
            let word = bytes_array::<4>(hasher.finalize().as_slice(), 0)?;
            (word != [0; 4] && !taken.contains(&word)).then_some(word)
        })
        .unwrap_or([0xff; 4])
}

/// The `file_id` of a synthesized envelope: a word of the SHA-256 of the
/// written members.
fn content_file_id(sections: &[(String, Vec<u8>)], time_dates: &[u32]) -> [u8; 4] {
    let mut content = Sha256::new();
    for ((name, payload), time_date) in sections.iter().zip(time_dates) {
        for field in [name.as_bytes(), payload.as_slice()] {
            content.update(Sha256::digest(field));
        }
        content.update(time_date.to_le_bytes());
    }
    derived_word(&[b"file_id".as_slice(), content.finalize().as_slice()], &[])
}

/// The local, central and end-of-directory signature tokens of a file,
/// a function of its `file_id`: nonzero and pairwise distinct.
struct FileTokens {
    local: [u8; 4],
    central: [u8; 6],
    root: [u8; 4],
}

impl FileTokens {
    fn of(file_id: [u8; 4]) -> Self {
        let local = derived_word(&[b"local".as_slice(), &file_id], &[]);
        let central = derived_word(&[b"central".as_slice(), &file_id], &[local]);
        let root = derived_word(&[b"root".as_slice(), &file_id], &[local, central]);
        let mut record = [0; 6];
        record[..4].copy_from_slice(&central);
        record[4..].copy_from_slice(&VERSION_MADE_BY);
        Self {
            local,
            central: record,
            root,
        }
    }
}

fn bytes_array<const N: usize>(source: &[u8], start: usize) -> Option<[u8; N]> {
    source
        .get(start..start.checked_add(N)?)
        .and_then(|bytes| bytes.try_into().ok())
}

/// The signature tokens and attributes one source member stores.
struct SourceMember {
    name: String,
    type_id: u32,
    local: [u8; 4],
    central: [u8; 6],
    attributes: [u8; 10],
}

/// A proved source envelope with every token it stores.
struct RetainedEnvelope {
    header: [u8; 8],
    root: [u8; 4],
    padding_bytes: usize,
    members: Vec<SourceMember>,
}

impl RetainedEnvelope {
    /// The first source member's token, and whether every source member
    /// shares it.
    fn token<const N: usize>(
        &self,
        token: impl Fn(&SourceMember) -> [u8; N],
    ) -> Option<([u8; N], bool)> {
        let first = token(self.members.first()?);
        Some((
            first,
            self.members.iter().all(|member| token(member) == first),
        ))
    }
}

/// The source's proved envelope, or `None` when the source has none or a
/// stored token cannot be read.
fn retained(scan: &ContainerScan<'_>) -> Result<Option<RetainedEnvelope>, CodecError> {
    let envelope = match native_layout::source_envelope(scan.source_image, &scan.blocks) {
        Ok(Some(envelope)) => envelope,
        Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => return Err(error),
        Ok(None) | Err(_) => return Ok(None),
    };
    let image = scan.source_image;
    let member = |entry: &crate::container::DirectoryEntry| {
        let relative = View::u32_le_at(&entry.descriptor, descriptor::LOCAL_OFFSET)?;
        let signature = usize::try_from(relative).ok()?.checked_add(outer::LEN)?;
        Some(SourceMember {
            name: entry.name.clone(),
            type_id: entry.type_id,
            local: bytes_array(image, signature)?,
            central: bytes_array(image, entry.offset.checked_sub(central::VERSION_NEEDED)?)?,
            attributes: bytes_array(&entry.descriptor, 0)?,
        })
    };
    let Some(members) = envelope
        .entries
        .iter()
        .map(member)
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let (Some(header), Some(root)) = (bytes_array(image, 0), bytes_array(image, envelope.root))
    else {
        return Ok(None);
    };
    Ok(Some(RetainedEnvelope {
        header,
        root,
        padding_bytes: envelope.padding_bytes,
        members,
    }))
}

/// One regenerated envelope and what it could not hold.
pub(super) struct Regenerated {
    pub(super) bytes: Vec<u8>,
    pub(super) omitted_envelope_bytes: usize,
    /// The opaque token fields written without a stored value, each naming
    /// its record.
    pub(super) residual: Vec<String>,
    /// The member fields written with a placeholder, each naming its member.
    pub(super) placeholders: Vec<String>,
    /// The members the envelope cannot store: a name it cannot encode, or a
    /// count or offset past its field.
    pub(super) dropped: Vec<String>,
}

impl Regenerated {
    pub(super) fn losses(&self) -> impl Iterator<Item = LossNote> {
        auxiliary_loss(self.omitted_envelope_bytes)
            .into_iter()
            .chain(cadmpeg_ir::codec::write::record_residual_unavailable(
                "SLDPRT",
                "writes these opaque envelope tokens with values of its own, since no stated rule \
                 gives the values SolidWorks stores: a synthesized envelope takes a file_id from a \
                 SHA-256 of the written members and three nonzero, distinct tokens derived from \
                 that file_id, and a new member of a retained envelope whose members disagree \
                 takes the first member's token",
                self.residual.iter().cloned(),
            ))
            .chain(cadmpeg_ir::codec::write::record_residual_unavailable(
                "SLDPRT",
                "writes placeholders in these member fields, since no stated rule gives the values \
                 SolidWorks stores: a time/date word of 0x20 plus the member's index, and \
                 internal_attrs 0",
                self.placeholders.iter().cloned(),
            ))
            .chain(cadmpeg_ir::codec::write::native_records_omitted(
                "SLDPRT",
                "whose name, member count or offset the native envelope cannot store",
                self.dropped.iter().cloned(),
            ))
    }
}

fn auxiliary_loss(bytes: usize) -> Option<LossNote> {
    (bytes != 0).then(|| {
        crate::loss::SldprtLossCode::WriterNativeEnvelopeAuxiliaryOmitted.note(format!(
            "{bytes} native envelope auxiliary bytes were omitted during semantic envelope regeneration"
        ))
    })
}

fn storable_name(name: &str) -> bool {
    !name.is_empty()
        && u16::try_from(name.len()).is_ok()
        && name.bytes().all(|byte| (0x20..0x7f).contains(&byte))
}

/// Whether the members named `name` are written with the time/date and
/// payload multiset the source stores, so the source's cache cell for them
/// still describes them.
fn cell_members_unchanged(
    scan: &ContainerScan<'_>,
    name: &str,
    written: &[(&str, u32, &[u8])],
) -> bool {
    let mut original: Vec<_> = scan
        .blocks
        .iter()
        .filter(|block| block.section.name() == Some(name))
        .map(|block| (block.type_id, block.payload.as_slice()))
        .collect();
    let mut current: Vec<_> = written
        .iter()
        .filter(|(member, ..)| *member == name)
        .map(|(_, time_date, payload)| (*time_date, *payload))
        .collect();
    original.sort_unstable();
    current.sort_unstable();
    !original.is_empty() && original == current
}

/// Write `sections` in a native envelope. A section takes the tokens and
/// attributes of the source member of its name and time/date word, the k-th
/// such section the k-th such member. A section with no source member takes
/// the token every source member shares, else the first member's. Without a
/// proved source envelope, every member takes the synthesized file's
/// tokens. A section with no source time/date word (`None`) takes
/// `0x20 + index`.
pub(super) fn build(
    sections: &[(String, Vec<u8>)],
    stored_time_dates: &[Option<u32>],
    source_scan: Option<&ContainerScan<'_>>,
) -> Result<Regenerated, CodecError> {
    debug_assert_eq!(sections.len(), stored_time_dates.len());
    let time_dates = stored_time_dates
        .iter()
        .enumerate()
        .map(|(index, time_date)| match time_date {
            Some(time_date) => Ok(*time_date),
            None => u32::try_from(index)
                .ok()
                .and_then(|index| PLACEHOLDER_TIME_DATE.checked_add(index))
                .ok_or_else(|| {
                    CodecError::NotImplemented(
                        "SLDPRT section count exceeds the time/date word".into(),
                    )
                }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut placeholders = Vec::new();
    let retained = source_scan.map(retained).transpose()?.flatten();
    let mut residual = Vec::new();
    let mut dropped = Vec::new();
    let (header, root, new_local, new_central) = match &retained {
        Some(retained) => {
            let fallback = FileTokens::of(bytes_array(&retained.header, 0).unwrap_or_default());
            (
                retained.header,
                retained.root,
                retained
                    .token(|member| member.local)
                    .unwrap_or((fallback.local, false)),
                retained
                    .token(|member| member.central)
                    .unwrap_or((fallback.central, false)),
            )
        }
        None => {
            let file_id = content_file_id(sections, &time_dates);
            let tokens = FileTokens::of(file_id);
            residual.extend(SYNTHESIZED_FIELDS.map(str::to_owned));
            let mut header = [0; outer::LEN];
            header[..outer::VERSION].copy_from_slice(&file_id);
            header[outer::VERSION..].copy_from_slice(&NATIVE_VERSION);
            (
                header,
                tokens.root,
                (tokens.local, true),
                (tokens.central, true),
            )
        }
    };
    let mut used = retained
        .iter()
        .flat_map(|retained| &retained.members)
        .map(|_| false)
        .collect::<Vec<_>>();
    let mut output = header.to_vec();
    let mut central_entries = Vec::new();
    let mut written = Vec::new();
    for (index, ((name, payload), time_date)) in sections.iter().zip(&time_dates).enumerate() {
        if !storable_name(name) {
            dropped.push(name.clone());
            continue;
        }
        let relative = u32::try_from(output.len() - outer::LEN)
            .ok()
            .filter(|_| central_entries.len() < usize::from(u16::MAX));
        let Some(relative) = relative else {
            dropped.extend(sections[index..].iter().map(|(name, _)| name.clone()));
            break;
        };
        if stored_time_dates.get(index).is_some_and(Option::is_none) {
            placeholders.push(format!("{name} time/date word"));
        }
        let source = retained.as_ref().and_then(|retained| {
            let position = retained
                .members
                .iter()
                .zip(&used)
                .position(|(member, used)| {
                    !used && member.name == *name && member.type_id == *time_date
                })?;
            used[position] = true;
            retained.members.get(position)
        });
        let (local_token, central_token, attributes) = match source {
            Some(member) => (member.local, member.central, member.attributes),
            None => {
                if !new_local.1 {
                    residual.push(format!("{name} local signature"));
                }
                if !new_central.1 {
                    residual.push(format!("{name} central signature"));
                }
                placeholders.push(format!("{name} internal_attrs"));
                (new_local.0, new_central.0, [0; 10])
            }
        };
        let frame = super::block(payload, name, *time_date)?;
        output.extend(local_token);
        output.extend(&frame);
        let mut directory = central_token.to_vec();
        directory.extend(&frame[..local::LEN]);
        directory.extend(attributes);
        directory.extend(relative.to_le_bytes());
        directory.extend(&frame[local::LEN..local::LEN + name.len()]);
        central_entries.push(directory);
        written.push((name.as_str(), *time_date, payload.as_slice()));
    }
    let mut omitted_envelope_bytes = retained
        .as_ref()
        .map_or(0, |retained| retained.padding_bytes);
    if let Some(scan) = source_scan {
        for cell in &scan.cache_cells {
            let Some(range) = cell.physical_range.clone() else {
                continue;
            };
            let kept = retained.is_some() && cell_members_unchanged(scan, &cell.name, &written);
            match scan.source_image.get(range.clone()) {
                Some(bytes) if kept => output.extend(bytes),
                _ => omitted_envelope_bytes += range.len(),
            }
        }
    }
    let too_large = || {
        CodecError::NotImplemented(
            "SLDPRT output exceeds the native envelope's 32-bit offsets".into(),
        )
    };
    let central_start = output.len();
    let count = u16::try_from(central_entries.len()).map_err(|_| too_large())?;
    for directory in central_entries {
        output.extend(directory);
    }
    let central_size = u32::try_from(output.len() - central_start).map_err(|_| too_large())?;
    let central_offset = u32::try_from(central_start - outer::LEN).map_err(|_| too_large())?;
    output.extend(root);
    output.extend([0u8; 4]);
    output.extend(count.to_le_bytes());
    output.extend(count.to_le_bytes());
    output.extend(central_size.to_le_bytes());
    output.extend(central_offset.to_le_bytes());
    output.extend(0u16.to_le_bytes());
    Ok(Regenerated {
        bytes: output,
        omitted_envelope_bytes,
        residual,
        placeholders,
        dropped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::container::{
        anchored_cache_cell, make_block, native_envelope_fixture, outer_header,
    };

    fn source_fixture(fill: u8) -> Vec<u8> {
        let mut source = outer_header();
        source.extend(make_block(10, "A", b"unchanged"));
        source.extend(make_block(11, "B", b"old"));
        source.extend(anchored_cache_cell(100, 9, "A", fill));
        source.extend(anchored_cache_cell(100, 10, "B", fill));
        native_envelope_fixture(&source)
    }

    /// The `file_id` and the local, central and root tokens of a written
    /// single-envelope file.
    fn tokens(bytes: &[u8]) -> [[u8; 4]; 4] {
        let written = crate::test_support::container::scan(bytes);
        let envelope = native_layout::source_envelope(bytes, &written.blocks)
            .unwrap()
            .unwrap();
        let block = &written.blocks[0];
        [
            bytes_array(bytes, 0).unwrap(),
            bytes_array(bytes, block.offset - 4).unwrap(),
            bytes_array(bytes, written.directory[0].offset - 6).unwrap(),
            bytes_array(bytes, envelope.root).unwrap(),
        ]
    }

    #[test]
    fn source_less_and_unproved_envelopes_take_tokens_derived_from_the_written_members() {
        let sections = vec![
            ("A".into(), b"new".to_vec()),
            ("\u{c4}".into(), b"x".to_vec()),
        ];
        let synthesized = build(&sections, &[None, None], None).unwrap();
        assert_eq!(synthesized.dropped, ["\u{c4}"]);
        assert_eq!(synthesized.residual, SYNTHESIZED_FIELDS);
        assert_eq!(
            synthesized.placeholders,
            ["A time/date word", "A internal_attrs"]
        );
        assert_eq!(
            &synthesized.bytes[outer::VERSION..outer::LEN],
            &NATIVE_VERSION
        );
        let written = crate::test_support::container::scan(&synthesized.bytes);
        assert_eq!(written.blocks.len(), 1);
        assert_eq!(written.blocks[0].payload, b"new");
        let [file_id, local, central, root] = tokens(&synthesized.bytes);
        let all = [file_id, local, central, root];
        assert!(all.iter().all(|token| *token != [0; 4]));
        assert!(local != central && local != root && central != root);
        let file_tokens = FileTokens::of(file_id);
        assert_eq!(
            [local, root],
            [file_tokens.local, file_tokens.root],
            "the tokens are a function of file_id"
        );
        assert_eq!(&file_tokens.central[..4], &central);
        let directory = &written.directory[0];
        assert_eq!(
            &synthesized.bytes[directory.offset - 2..directory.offset],
            &VERSION_MADE_BY
        );
        assert_eq!(
            build(&sections, &[None, None], None).unwrap().bytes,
            synthesized.bytes,
            "the derivation is stable"
        );
        let other = build(&[("A".into(), b"other".to_vec())], &[None], None).unwrap();
        assert_ne!(tokens(&other.bytes)[0], file_id);
        let codes = synthesized
            .losses()
            .map(|note| note.code.local_code().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                "record.residual-unavailable",
                "record.residual-unavailable",
                "native.records-omitted"
            ]
        );

        let mut legacy = outer_header();
        legacy.extend(make_block(10, "A", b"old"));
        let scan = crate::test_support::container::scan(&legacy);
        let unproved = build(&[("A".into(), b"new".to_vec())], &[Some(10)], Some(&scan)).unwrap();
        assert_eq!(unproved.residual, SYNTHESIZED_FIELDS);
        let [file_id, local, _, root] = tokens(&unproved.bytes);
        assert_ne!(file_id, [0; 4]);
        assert_eq!(
            [local, root],
            [FileTokens::of(file_id).local, FileTokens::of(file_id).root]
        );
    }

    #[test]
    fn repeated_name_and_time_date_members_take_their_source_tokens_in_order() {
        let mut source = outer_header();
        source.extend(make_block(10, "A", b"first"));
        source.extend(make_block(10, "A", b"second"));
        let source = native_envelope_fixture(&source);
        let original = crate::test_support::container::scan(&source);
        let sections = vec![
            ("A".into(), b"first".to_vec()),
            ("A".into(), b"second".to_vec()),
        ];
        let regenerated = build(&sections, &[Some(10); 2], Some(&original)).unwrap();
        assert!(regenerated.residual.is_empty());
        let written = crate::test_support::container::scan(&regenerated.bytes);
        assert_eq!(written.directory.len(), 2);
        assert!(written
            .directory
            .iter()
            .all(|entry| entry.descriptor[4] == 1));
    }

    #[test]
    fn retained_cells_and_native_tokens_survive_complete_directory_readback() {
        for fill in [0, 0xff] {
            let source = source_fixture(fill);
            let original = crate::test_support::container::scan(&source);
            let retained = original
                .cache_cells
                .iter()
                .find(|cell| cell.name == "A")
                .unwrap();
            let exact_cell = &source[retained.physical_range.clone().unwrap()];
            let sections = vec![
                ("A".into(), b"unchanged".to_vec()),
                ("B".into(), b"changed".to_vec()),
                ("C".into(), b"new".to_vec()),
            ];
            let regenerated =
                build(&sections, &[Some(10), Some(11), Some(12)], Some(&original)).unwrap();
            assert_eq!(regenerated.omitted_envelope_bytes, 100);
            let output = regenerated.bytes;
            let written = crate::test_support::container::scan(&output);
            assert_eq!(written.blocks.len(), 3);
            assert_eq!(written.directory.len(), 3);
            assert_eq!(written.cache_cells.len(), 1);
            assert_eq!(output.get(..outer::LEN), source.get(..outer::LEN));
            assert_eq!(
                &output[written.cache_cells[0].physical_range.clone().unwrap()],
                exact_cell
            );
            let before = native_layout::source_envelope(&source, &original.blocks)
                .unwrap()
                .unwrap();
            let after = native_layout::source_envelope(&output, &written.blocks)
                .unwrap()
                .unwrap();
            assert_eq!(
                bytes_array::<4>(&output, after.root).unwrap(),
                bytes_array::<4>(&source, before.root).unwrap()
            );
            for entry in &written.directory {
                let local_offset =
                    View::u32_le_at(&entry.descriptor, descriptor::LOCAL_OFFSET).unwrap();
                let block = written
                    .blocks
                    .iter()
                    .find(|block| block.section.name() == Some(entry.name.as_str()))
                    .unwrap();
                assert_eq!(
                    usize::try_from(local_offset).unwrap() + outer::LEN + 4,
                    block.offset
                );
                assert_eq!(
                    &output[block.offset - 4..block.offset],
                    &[0x5a, 0xd9, 0xc8, 0x87]
                );
                assert_eq!(
                    &output[entry.offset - 6..entry.offset],
                    &[0x45, 0x2b, 0x7e, 0x28, 0, 0]
                );
                assert_eq!(entry.descriptor[4], u8::from(entry.name != "C"));
            }
            assert_eq!(
                written
                    .blocks
                    .iter()
                    .find(|block| block.section.name() == Some("B"))
                    .unwrap()
                    .payload,
                b"changed"
            );
        }
    }

    #[test]
    fn new_member_takes_the_first_source_token_where_retained_signature_tokens_differ() {
        let mut source = source_fixture(0);
        let parsed = crate::test_support::container::scan(&source);
        let at = parsed.blocks[1].offset - 4;
        source[at] ^= 1;
        let parsed = crate::test_support::container::scan(&source);
        let first = bytes_array::<4>(&source, parsed.blocks[0].offset - 4).unwrap();
        let sections = vec![("C".into(), b"new".to_vec())];
        let regenerated = build(&sections, &[None], Some(&parsed)).unwrap();
        assert_eq!(regenerated.residual, ["C local signature"]);
        assert_eq!(
            regenerated.placeholders,
            ["C time/date word", "C internal_attrs"]
        );
        let written = crate::test_support::container::scan(&regenerated.bytes);
        let block = &written.blocks[0];
        assert_eq!(&regenerated.bytes[block.offset - 4..block.offset], &first);
        assert_eq!(block.type_id, PLACEHOLDER_TIME_DATE);
        assert_eq!(
            &regenerated.bytes[written.directory[0].offset - 6..written.directory[0].offset],
            &[0x45, 0x2b, 0x7e, 0x28, 0, 0]
        );
    }

    #[test]
    fn omitted_padding_and_unretained_cache_bytes_have_one_counted_loss() {
        for fill in [0, 0xff] {
            let source =
                crate::test_support::container::native_padding_fixture(&source_fixture(fill), fill);
            let parsed = crate::test_support::container::scan(&source);
            let sections = vec![
                ("A".into(), b"unchanged".to_vec()),
                ("B".into(), b"changed".to_vec()),
            ];
            let regenerated = build(&sections, &[Some(10), Some(11)], Some(&parsed)).unwrap();
            assert_eq!(regenerated.omitted_envelope_bytes, 247);
            let note = auxiliary_loss(regenerated.omitted_envelope_bytes).unwrap();
            assert_eq!(
                note.code.local_code(),
                "writer.native-envelope-auxiliary-omitted"
            );
            assert!(note
                .message
                .starts_with("247 native envelope auxiliary bytes"));
            let written = crate::test_support::container::scan(&regenerated.bytes);
            assert_eq!(
                native_layout::source_envelope(&regenerated.bytes, &written.blocks)
                    .unwrap()
                    .unwrap()
                    .padding_bytes,
                0
            );
            assert!(auxiliary_loss(0).is_none());
        }
    }

    #[test]
    fn duplicate_name_cache_requires_unchanged_identity_and_payload_multiset() {
        for fill in [0, 0xff] {
            let mut source = outer_header();
            source.extend(make_block(0x0021_0049, "A", b"first"));
            source.extend(make_block(0x0021_004a, "A", b"second"));
            source.extend(anchored_cache_cell(100, 9, "A", fill));
            let source = native_envelope_fixture(&source);
            let original = crate::test_support::container::scan(&source);
            assert_eq!(original.blocks.len(), 2);
            assert_eq!(original.cache_cells.len(), 1);
            let raw_cell = &source[original.cache_cells[0].physical_range.clone().unwrap()];
            let unchanged = [
                ("A".into(), b"second".to_vec()),
                ("A".into(), b"first".to_vec()),
            ];
            let retained = build(
                &unchanged,
                &[Some(0x0021_004a), Some(0x0021_0049)],
                Some(&original),
            )
            .unwrap();
            assert_eq!(retained.omitted_envelope_bytes, 0);
            let parsed = crate::test_support::container::scan(&retained.bytes);
            assert_eq!(parsed.cache_cells.len(), 1);
            assert_eq!(
                &retained.bytes[parsed.cache_cells[0].physical_range.clone().unwrap()],
                raw_cell
            );
            let cases = [
                (
                    vec![
                        ("A".into(), b"edited".to_vec()),
                        ("A".into(), b"second".to_vec()),
                    ],
                    vec![0x0021_0049, 0x0021_004a],
                ),
                (
                    vec![
                        ("A".into(), b"second".to_vec()),
                        ("A".into(), b"first".to_vec()),
                    ],
                    vec![0x0021_0049, 0x0021_004a],
                ),
                (vec![("A".into(), b"first".to_vec())], vec![0x0021_0049]),
                (
                    vec![
                        ("A".into(), b"first".to_vec()),
                        ("A".into(), b"second".to_vec()),
                        ("A".into(), b"third".to_vec()),
                    ],
                    vec![0x0021_0049, 0x0021_004a, 0x0021_004b],
                ),
                (
                    vec![
                        ("A".into(), b"first".to_vec()),
                        ("A".into(), b"second".to_vec()),
                    ],
                    vec![0x0021_0049, 0x0021_004b],
                ),
            ];
            for (sections, time_dates) in cases {
                let time_dates = time_dates.into_iter().map(Some).collect::<Vec<_>>();
                let omitted = build(&sections, &time_dates, Some(&original)).unwrap();
                assert_eq!(omitted.omitted_envelope_bytes, 100);
                assert!(auxiliary_loss(omitted.omitted_envelope_bytes).is_some());
                let parsed = crate::test_support::container::scan(&omitted.bytes);
                assert!(parsed.cache_cells.is_empty());
                assert_eq!(parsed.blocks.len(), sections.len());
                assert_eq!(parsed.directory.len(), sections.len());
                assert!(
                    native_layout::source_envelope(&omitted.bytes, &parsed.blocks)
                        .unwrap()
                        .is_some()
                );
            }
        }
    }
}
