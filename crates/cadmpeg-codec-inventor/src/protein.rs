// SPDX-License-Identifier: Apache-2.0
//! Inventor Protein package framing and inventory.

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug)]
pub(crate) enum ProteinState<'a> {
    Absent,
    Empty {
        stream: CompoundStreamId,
    },
    Package(ProteinEnvelope<'a>),
    Malformed {
        stream: CompoundStreamId,
        detail: String,
    },
}

#[derive(Debug)]
pub(crate) struct ProteinEnvelope<'a> {
    pub(crate) stream: CompoundStreamId,
    pub(crate) declared_len: u32,
    pub(crate) archive: ArchiveSnapshot<'a>,
}

pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
) -> Result<ProteinState<'a>, CodecError> {
    let Some(stream) = snapshot.stream("Protein") else {
        return Ok(ProteinState::Absent);
    };
    let source = snapshot.open(ctx, stream)?;
    let result = parse_stream(ctx, source);
    Ok(match result {
        Ok(ParsedProtein::Empty) => ProteinState::Empty {
            stream: stream.id(),
        },
        Ok(ParsedProtein::Package {
            declared_len,
            archive,
        }) => ProteinState::Package(ProteinEnvelope {
            stream: stream.id(),
            declared_len,
            archive,
        }),
        Err(error) => ProteinState::Malformed {
            stream: stream.id(),
            detail: error.to_string(),
        },
    })
}

enum ParsedProtein<'a> {
    Empty,
    Package {
        declared_len: u32,
        archive: ArchiveSnapshot<'a>,
    },
}

fn parse_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: cadmpeg_core::decode::View<'a>,
) -> Result<ParsedProtein<'a>, CodecError> {
    let header = source
        .window()
        .get(..4)
        .ok_or_else(|| CodecError::Malformed("truncated Inventor Protein length".into()))?;
    let declared_len = u32::from_le_bytes(header.try_into().expect("four-byte header"));
    if declared_len == 0 {
        if source.window().len() != 4 {
            return Err(CodecError::Malformed(
                "empty Inventor Protein stream has trailing bytes".into(),
            ));
        }
        return Ok(ParsedProtein::Empty);
    }
    let payload_len = source.window().len().saturating_sub(4);
    if declared_len as usize != payload_len {
        return Err(CodecError::Malformed(format!(
            "Inventor Protein declares {declared_len} bytes but stores {payload_len}"
        )));
    }
    let payload = source
        .child(source.start() + 4, source.end())
        .ok_or_else(|| CodecError::Malformed("Inventor Protein payload range is invalid".into()))?;
    let archive = ArchiveSnapshot::new(payload)?;
    for entry in archive.entries() {
        validate_entry_name(&entry.name)?;
    }
    ctx.charge_collection_items(
        archive.entries().len() as u64,
        "admit Inventor Protein package entries",
    )?;
    Ok(ParsedProtein::Package {
        declared_len,
        archive,
    })
}

fn validate_entry_name(name: &str) -> Result<(), CodecError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
    {
        return Err(CodecError::Malformed(format!(
            "Inventor Protein package has unsafe entry name {name:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn protein_distinguishes_empty_and_exact_package() {
        with_stream(&0_u32.to_le_bytes(), |ctx, root| {
            assert!(matches!(parse_stream(ctx, root), Ok(ParsedProtein::Empty)));
        });
        let zip = zip_fixture("Schemas/ExampleSchema.xml");
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        with_stream(&bytes, |ctx, root| {
            let ParsedProtein::Package { archive, .. } =
                parse_stream(ctx, root).expect("synthetic Protein package parses")
            else {
                panic!("package state")
            };
            assert_eq!(archive.entries().len(), 1);
        });
    }

    #[test]
    fn protein_rejects_length_mismatch_and_unsafe_paths() {
        let mut mismatch = 5_u32.to_le_bytes().to_vec();
        mismatch.extend_from_slice(b"four");
        with_stream(&mismatch, |ctx, root| {
            assert!(parse_stream(ctx, root).is_err());
        });

        let zip = zip_fixture("../escape");
        let mut bytes = (zip.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&zip);
        with_stream(&bytes, |ctx, root| {
            assert!(parse_stream(ctx, root).is_err());
        });
    }

    fn with_stream(
        bytes: &[u8],
        test: impl FnOnce(&DecodeContext<'_>, cadmpeg_core::decode::View<'_>),
    ) {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic Protein stream fits policy");
        test(&ctx, root);
    }

    fn zip_fixture(name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(name, SimpleFileOptions::default())
            .expect("start synthetic ZIP member");
        writer
            .write_all(b"synthetic")
            .expect("write synthetic ZIP member");
        writer.finish().expect("finish synthetic ZIP").into_inner()
    }
}
