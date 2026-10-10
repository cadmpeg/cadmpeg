// SPDX-License-Identifier: Apache-2.0
//! Shared fixtures for the Protein unit tests: paged instance streams, schema
//! archives and decode contexts.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::{
    framing, CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
};

pub(crate) fn decode_fixture(
    protein: &[u8],
    instance: &[u8],
) -> Result<crate::DecodeOutcome, CodecError> {
    let mut bytes = protein.to_vec();
    bytes.extend_from_slice(instance);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())?;
    let protein_view = root.child(0, protein.len()).expect("fixture Protein range");
    let instance_view = root
        .child(protein.len(), bytes.len())
        .expect("fixture instance range");
    crate::decode_detailed(&ctx, protein_view, instance_view)
}

pub(crate) fn with_service_context<T>(
    bytes: &[u8],
    use_context: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("fixture fits service profile");
    use_context(&ctx)
}

/// Frames a paged stream under a service context and keeps copies of the
/// frames past that context.
pub(crate) fn frames_of(stream: &[u8]) -> Result<Vec<framing::RecordFrame>, CodecError> {
    with_service_context(stream, |ctx| {
        Ok(framing::record_frames_admitted(ctx, stream)?
            .frames()
            .to_vec())
    })
}

/// Empty scoped storage for a schema parse or inheritance resolution.
pub(crate) fn scratch<'ctx>(ctx: &'ctx DecodeContext<'_>) -> ScopedReservation<'ctx> {
    ctx.reserve_scoped(0, "test schema catalog")
        .expect("empty reservation")
}

/// A catalog from parsed schemas and already resolved property closures.
pub(crate) fn catalog_of<'ctx, 'input>(
    ctx: &'ctx DecodeContext<'input>,
    schemas: HashMap<String, crate::Schema>,
    properties: HashMap<String, BTreeMap<String, crate::Property>>,
) -> crate::SchemaCatalog<&'ctx DecodeContext<'input>> {
    crate::SchemaCatalog {
        schemas,
        properties,
        storage: scratch(ctx),
    }
}

/// Lay records out as `InstanceProperties.bin` does: a 16-byte stream header,
/// then a marker page, continuation pages, and a terminal page per record.
pub(crate) fn paged_stream(records: &[&[u8]]) -> Vec<u8> {
    const BODY: usize = PAGE_SIZE - 8;
    let mut out = u32::try_from(PAGE_SIZE)
        .expect("test page size fits u32")
        .to_le_bytes()
        .to_vec();
    out.resize(STREAM_HEADER_LEN, 0);
    let mut page = |header: [u8; 8], body: &[u8]| {
        out.extend_from_slice(&header);
        out.extend_from_slice(body);
        out.resize(out.len() + BODY - body.len(), 0);
    };
    let opening = |marker: &[u8]| {
        let mut header = [0_u8; 8];
        header[4..8].copy_from_slice(marker);
        header
    };
    for record in records {
        // A marker or continuation page always contributes its whole body,
        // so only the terminal page can hold a partial tail.
        if record.len() < BODY {
            let mut header = [0_u8; 8];
            header[..4].copy_from_slice(TERMINAL_MARKER);
            header[4..6].copy_from_slice(
                &u16::try_from(record.len())
                    .expect("test record fits u16")
                    .to_le_bytes(),
            );
            page(header, record);
            continue;
        }
        let (head, rest) = record.split_at(BODY);
        page(opening(RECORD_MARKER), head);
        let mut chunks = rest.chunks(BODY).peekable();
        while let Some(chunk) = chunks.next() {
            if chunks.peek().is_some() {
                page(opening(CONTINUATION_MARKER), chunk);
            } else {
                let mut header = [0_u8; 8];
                header[0..4].copy_from_slice(TERMINAL_MARKER);
                header[4..6].copy_from_slice(
                    &u16::try_from(chunk.len())
                        .expect("test chunk fits u16")
                        .to_le_bytes(),
                );
                page(header, chunk);
            }
        }
    }
    out
}

pub(crate) fn schema_archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .system(zip::System::Unix);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, xml) in entries {
        archive.start_file(name, options).expect("start schema");
        archive.write_all(xml.as_bytes()).expect("write schema");
    }
    archive.finish().expect("finish schemas").into_inner()
}

pub(crate) fn push_lp(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .expect("test value fits u32")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value.as_bytes());
}

pub(crate) fn push_connections(bytes: &mut Vec<u8>, values: &[&str]) {
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(
        &u32::try_from(values.len())
            .expect("test count fits u32")
            .to_le_bytes(),
    );
    for value in values {
        push_lp(bytes, value);
    }
}
