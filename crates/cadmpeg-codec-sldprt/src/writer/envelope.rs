// SPDX-License-Identifier: Apache-2.0
//! Regeneration of proved native ZIP envelopes with retained signature tokens.

use std::collections::BTreeSet;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::container::{native_layout, ContainerScan};
use crate::layout::block_frame_header as local;
use crate::layout::outer_header as outer;
use crate::layout::zip_central_descriptor as descriptor;
use crate::layout::zip_central_header as central;

fn unsupported() -> CodecError {
    CodecError::NotImplemented("SLDPRT envelope regeneration needs validated retained local, central and EOCD signature tokens".into())
}

fn bytes_array<const N: usize>(source: &[u8], start: usize) -> Result<[u8; N], CodecError> {
    let end = start.checked_add(N).ok_or_else(unsupported)?;
    source
        .get(start..end)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(unsupported)
}

fn unique<const N: usize>(values: &BTreeSet<[u8; N]>) -> Result<[u8; N], CodecError> {
    if values.len() != 1 {
        return Err(unsupported());
    }
    values.first().copied().ok_or_else(unsupported)
}

pub(super) struct Regenerated {
    pub(super) bytes: Vec<u8>,
    pub(super) omitted_envelope_bytes: usize,
}

pub(super) fn auxiliary_loss(bytes: usize) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (bytes != 0).then(|| {
        crate::loss::SldprtLossCode::WriterNativeEnvelopeAuxiliaryOmitted.note(format!(
            "{bytes} native envelope auxiliary bytes were omitted during semantic envelope regeneration"
        ))
    })
}

pub(super) fn build(
    sections: &[(String, Vec<u8>)],
    time_dates: &[u32],
    source_scan: Option<&ContainerScan<'_>>,
) -> Result<Regenerated, CodecError> {
    let scan = source_scan.ok_or_else(unsupported)?;
    let envelope =
        native_layout::source_envelope(scan.source_image, &scan.blocks)?.ok_or_else(unsupported)?;
    if sections.len() != time_dates.len() {
        return Err(unsupported());
    }
    let count = u16::try_from(sections.len()).map_err(|_| unsupported())?;
    let mut local_tokens = BTreeSet::new();
    let mut central_tokens = BTreeSet::new();
    for entry in &envelope.entries {
        let relative =
            View::u32_le_at(&entry.descriptor, descriptor::LOCAL_OFFSET).ok_or_else(unsupported)?;
        let signature = usize::try_from(relative)
            .map_err(|_| unsupported())?
            .checked_add(outer::LEN)
            .ok_or_else(unsupported)?;
        local_tokens.insert(bytes_array::<4>(scan.source_image, signature)?);
        central_tokens.insert(bytes_array::<6>(
            scan.source_image,
            entry
                .offset
                .checked_sub(central::VERSION_NEEDED)
                .ok_or_else(unsupported)?,
        )?);
    }
    let mut output = bytes_array::<8>(scan.source_image, 0)?.to_vec();
    let mut central_entries = Vec::new();
    for ((name, payload), time_date) in sections.iter().zip(time_dates) {
        if u16::try_from(name.len()).is_err()
            || name.is_empty()
            || !name.bytes().all(|byte| (0x20..0x7f).contains(&byte))
        {
            return Err(unsupported());
        }
        let mut matches = envelope
            .entries
            .iter()
            .filter(|entry| entry.name == *name && entry.type_id == *time_date);
        let source = matches.next();
        if matches.next().is_some() {
            return Err(unsupported());
        }
        let (local_token, central_token, attributes) = if let Some(entry) = source {
            let relative = View::u32_le_at(&entry.descriptor, descriptor::LOCAL_OFFSET)
                .ok_or_else(unsupported)?;
            let signature = usize::try_from(relative)
                .map_err(|_| unsupported())?
                .checked_add(outer::LEN)
                .ok_or_else(unsupported)?;
            (
                bytes_array::<4>(scan.source_image, signature)?,
                bytes_array::<6>(
                    scan.source_image,
                    entry
                        .offset
                        .checked_sub(central::VERSION_NEEDED)
                        .ok_or_else(unsupported)?,
                )?,
                bytes_array::<10>(&entry.descriptor, 0)?,
            )
        } else {
            (unique(&local_tokens)?, unique(&central_tokens)?, [0; 10])
        };
        let relative = u32::try_from(
            output
                .len()
                .checked_sub(outer::LEN)
                .ok_or_else(unsupported)?,
        )
        .map_err(|_| unsupported())?;
        let frame = super::block(payload, name, *time_date)?;
        output.extend(local_token);
        output.extend(&frame);
        let mut directory = central_token.to_vec();
        directory.extend(frame.get(..local::LEN).ok_or_else(unsupported)?);
        directory.extend(attributes);
        directory.extend(relative.to_le_bytes());
        let name_end = local::LEN.checked_add(name.len()).ok_or_else(unsupported)?;
        directory.extend(frame.get(local::LEN..name_end).ok_or_else(unsupported)?);
        central_entries.push(directory);
    }
    let mut omitted_envelope_bytes = envelope.padding_bytes;
    for cell in &scan.cache_cells {
        let mut original_members: Vec<_> = scan
            .blocks
            .iter()
            .filter(|block| block.section.name() == Some(cell.name.as_str()))
            .map(|block| (block.type_id, block.payload.as_slice()))
            .collect();
        let mut written_members: Vec<_> = sections
            .iter()
            .zip(time_dates)
            .filter_map(|((name, payload), time_date)| {
                (name == &cell.name).then_some((*time_date, payload.as_slice()))
            })
            .collect();
        original_members.sort_unstable();
        written_members.sort_unstable();
        let original_matches = !original_members.is_empty() && original_members == written_members;
        let range = cell.physical_range.clone().ok_or_else(unsupported)?;
        if original_matches {
            output.extend(scan.source_image.get(range).ok_or_else(unsupported)?);
        } else {
            omitted_envelope_bytes = omitted_envelope_bytes
                .checked_add(range.len())
                .ok_or_else(unsupported)?;
        }
    }
    let central_start = output.len();
    for directory in central_entries {
        output.extend(directory);
    }
    let central_size = u32::try_from(
        output
            .len()
            .checked_sub(central_start)
            .ok_or_else(unsupported)?,
    )
    .map_err(|_| unsupported())?;
    let central_offset = u32::try_from(
        central_start
            .checked_sub(outer::LEN)
            .ok_or_else(unsupported)?,
    )
    .map_err(|_| unsupported())?;
    output.extend(bytes_array::<4>(scan.source_image, envelope.root)?);
    output.extend([0u8; 4]);
    output.extend(count.to_le_bytes());
    output.extend(count.to_le_bytes());
    output.extend(central_size.to_le_bytes());
    output.extend(central_offset.to_le_bytes());
    output.extend(0u16.to_le_bytes());
    Ok(Regenerated {
        bytes: output,
        omitted_envelope_bytes,
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

    #[test]
    fn source_less_and_unproved_signature_envelopes_abstain() {
        assert!(matches!(
            build(&[], &[], None),
            Err(CodecError::NotImplemented(_))
        ));
        let mut legacy = outer_header();
        legacy.extend(make_block(10, "A", b"old"));
        let scan = crate::test_support::container::scan(&legacy);
        assert!(matches!(
            build(&[("A".into(), b"new".to_vec())], &[10], Some(&scan)),
            Err(CodecError::NotImplemented(_))
        ));
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
            let regenerated = build(&sections, &[10, 11, 12], Some(&original)).unwrap();
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
    fn new_member_does_not_choose_between_distinct_retained_signature_tokens() {
        let mut source = source_fixture(0);
        let parsed = crate::test_support::container::scan(&source);
        let at = parsed.blocks[1].offset - 4;
        source[at] ^= 1;
        let parsed = crate::test_support::container::scan(&source);
        let sections = vec![("C".into(), b"new".to_vec())];
        assert!(matches!(
            build(&sections, &[12], Some(&parsed)),
            Err(CodecError::NotImplemented(_))
        ));
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
            let regenerated = build(&sections, &[10, 11], Some(&parsed)).unwrap();
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
            let retained = build(&unchanged, &[0x0021_004a, 0x0021_0049], Some(&original)).unwrap();
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
