// SPDX-License-Identifier: Apache-2.0
//! Native ZIP envelope regeneration: retained signature tokens where the
//! source envelope is proved, synthesized tokens otherwise.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::container::{native_layout, ContainerScan};
use crate::layout::block_frame_header as local;
use crate::layout::outer_header as outer;
use crate::layout::zip_central_descriptor as descriptor;
use crate::layout::zip_central_header as central;

/// The outer header's big-endian `version` word (§1.1).
const NATIVE_VERSION: [u8; 4] = 4u32.to_be_bytes();

/// A central header's `version_made_by`: the decoder selects central records
/// whose field is zero (§1.3).
const VERSION_MADE_BY: [u8; 2] = [0; 2];

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
    /// The token every source member shares, when they share one.
    fn shared<const N: usize>(&self, token: impl Fn(&SourceMember) -> [u8; N]) -> Option<[u8; N]> {
        let first = token(self.members.first()?);
        self.members
            .iter()
            .all(|member| token(member) == first)
            .then_some(first)
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
    /// The opaque token fields written as zero, each naming its record.
    pub(super) residual: Vec<String>,
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
                "writes zero for these opaque envelope fields, whose value the SLDPRT specification \
                 leaves unstated; whether SolidWorks accepts zero there cannot be tested",
                self.residual.iter().cloned(),
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
/// attributes of the source member of its name and type id, the k-th such
/// section the k-th such member. A section with no source member takes the
/// token every source member shares, else zero.
pub(super) fn build(
    sections: &[(String, Vec<u8>)],
    time_dates: &[u32],
    source_scan: Option<&ContainerScan<'_>>,
) -> Result<Regenerated, CodecError> {
    debug_assert_eq!(sections.len(), time_dates.len());
    let retained = source_scan.map(retained).transpose()?.flatten();
    let mut residual = Vec::new();
    let mut dropped = Vec::new();
    let header = match (&retained, source_scan) {
        (Some(retained), _) => retained.header,
        (None, Some(scan))
            if scan.source_image.get(outer::VERSION..outer::LEN) == Some(&NATIVE_VERSION[..]) =>
        {
            bytes_array(scan.source_image, 0).unwrap_or_default()
        }
        (None, _) => {
            residual.push("outer header file_id".to_owned());
            let mut header = [0; outer::LEN];
            header[outer::VERSION..].copy_from_slice(&NATIVE_VERSION);
            header
        }
    };
    let shared_local = retained
        .as_ref()
        .and_then(|retained| retained.shared(|member| member.local));
    let shared_central = retained
        .as_ref()
        .and_then(|retained| retained.shared(|member| member.central));
    let mut used = retained
        .iter()
        .flat_map(|retained| &retained.members)
        .map(|_| false)
        .collect::<Vec<_>>();
    let mut output = header.to_vec();
    let mut central_entries = Vec::new();
    let mut written = Vec::new();
    for (index, ((name, payload), time_date)) in sections.iter().zip(time_dates).enumerate() {
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
                let local_token = shared_local.unwrap_or_else(|| {
                    residual.push(format!("{name} local signature"));
                    [0; 4]
                });
                let central_token = shared_central.unwrap_or_else(|| {
                    residual.push(format!("{name} central signature"));
                    let mut token = [0; 6];
                    token[4..].copy_from_slice(&VERSION_MADE_BY);
                    token
                });
                (local_token, central_token, [0; 10])
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
    let root = retained.as_ref().map_or_else(
        || {
            residual.push("end-of-directory signature".to_owned());
            [0; 4]
        },
        |retained| retained.root,
    );
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

    #[test]
    fn source_less_and_unproved_envelopes_are_synthesized_with_zero_tokens() {
        let sections = vec![
            ("A".into(), b"new".to_vec()),
            ("\u{c4}".into(), b"x".to_vec()),
        ];
        let synthesized = build(&sections, &[10, 11], None).unwrap();
        assert_eq!(synthesized.dropped, ["\u{c4}"]);
        assert_eq!(
            synthesized.residual,
            [
                "outer header file_id",
                "A local signature",
                "A central signature",
                "end-of-directory signature"
            ]
        );
        assert_eq!(&synthesized.bytes[..outer::LEN], &[0, 0, 0, 0, 0, 0, 0, 4]);
        let written = crate::test_support::container::scan(&synthesized.bytes);
        assert_eq!(written.blocks.len(), 1);
        assert_eq!(written.blocks[0].payload, b"new");
        assert_eq!(
            &synthesized.bytes[written.blocks[0].offset - 4..written.blocks[0].offset],
            &[0; 4]
        );
        let envelope = native_layout::source_envelope(&synthesized.bytes, &written.blocks)
            .unwrap()
            .unwrap();
        assert_eq!(envelope.entries.len(), 1);
        assert_eq!(
            bytes_array::<4>(&synthesized.bytes, envelope.root),
            Some([0; 4])
        );
        let codes = synthesized
            .losses()
            .map(|note| note.code.local_code().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            ["record.residual-unavailable", "native.records-omitted"]
        );

        let mut legacy = outer_header();
        legacy.extend(make_block(10, "A", b"old"));
        let scan = crate::test_support::container::scan(&legacy);
        let synthesized = build(&[("A".into(), b"new".to_vec())], &[10], Some(&scan)).unwrap();
        assert_eq!(&synthesized.bytes[..outer::LEN], &legacy[..outer::LEN]);
        assert_eq!(
            synthesized.residual,
            [
                "A local signature",
                "A central signature",
                "end-of-directory signature"
            ]
        );
        let written = crate::test_support::container::scan(&synthesized.bytes);
        assert!(
            native_layout::source_envelope(&synthesized.bytes, &written.blocks)
                .unwrap()
                .is_some()
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
        let regenerated = build(&sections, &[10, 10], Some(&original)).unwrap();
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
    fn new_member_takes_zero_where_retained_signature_tokens_differ() {
        let mut source = source_fixture(0);
        let parsed = crate::test_support::container::scan(&source);
        let at = parsed.blocks[1].offset - 4;
        source[at] ^= 1;
        let parsed = crate::test_support::container::scan(&source);
        let sections = vec![("C".into(), b"new".to_vec())];
        let regenerated = build(&sections, &[12], Some(&parsed)).unwrap();
        assert_eq!(regenerated.residual, ["C local signature"]);
        let written = crate::test_support::container::scan(&regenerated.bytes);
        let block = &written.blocks[0];
        assert_eq!(&regenerated.bytes[block.offset - 4..block.offset], &[0; 4]);
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
