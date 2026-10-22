// SPDX-License-Identifier: Apache-2.0
//! Assertions over source-only bytes and layout retained by the codec.

use std::collections::BTreeMap;

pub(crate) fn bytes<'a>(fidelity: &'a cadmpeg_ir::SourceFidelity, id: &str) -> &'a [u8] {
    if let Some(layout) = fidelity.record_layout().get(id) {
        if let Some(bytes) = layout.payloads().get("raw_xml") {
            // A kept object is captured at its declaration. Its unchanged
            // ObjectData XML payload has a separate, original pointer range.
            let (payload_start, payload_end) = match (
                layout.pointers().get("byte_start"),
                layout.pointers().get("byte_end"),
            ) {
                (Some(&start), Some(&end)) => (start, end),
                (None, None) => (
                    layout.offset(),
                    layout
                        .offset()
                        .checked_add(layout.byte_len())
                        .expect("XML capture end"),
                ),
                _ => panic!("XML payload must retain both original range pointers"),
            };
            assert_eq!(
                cadmpeg_core::decode::u64_from_index(bytes.len()),
                payload_end
                    .checked_sub(payload_start)
                    .expect("XML payload range")
            );
            let start = usize::try_from(payload_start).expect("XML source offset");
            let end = usize::try_from(payload_end).expect("XML source end");
            let capture_start = usize::try_from(layout.offset()).expect("XML capture offset");
            let capture_end = capture_start
                .checked_add(usize::try_from(layout.byte_len()).expect("XML capture length"))
                .expect("XML capture end");
            if let Some(image) = fidelity.retained_records().values().find(|record| {
                record.stream() == layout.stream()
                    && record.offset() == 0
                    && record.data().is_some()
            }) {
                let source = image.data().expect("XML source bytes");
                assert!(
                    source.get(capture_start..capture_end).is_some(),
                    "XML record capture must fit its source"
                );
                assert_eq!(bytes.as_ref(), &source[start..end]);
            } else {
                // Logical ZIP stream names include their physical parent extent.
                // Reopen the original archive and match the actual captured stream,
                // rather than assuming a plain entry name is its canonical stream.
                use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
                let archive_bytes = fidelity
                    .retained_record(crate::fidelity::SOURCE_IMAGE)
                    .expect("original source archive")
                    .data()
                    .expect("original archive bytes");
                let arena = DecodeArena::new();
                let (ctx, root) =
                    DecodeContext::from_root_bytes(archive_bytes, &arena, &DecodePolicy::service())
                        .expect("original archive source");
                let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
                    .expect("original archive directory");
                let mut matched = false;
                for entry in archive.entries() {
                    let Ok(view) = archive.open(&ctx, &entry.name) else {
                        continue;
                    };
                    let capture = ctx
                        .capture_part_extent(view)
                        .expect("original entry capture");
                    if capture.stream() != layout.stream() {
                        continue;
                    }
                    let relative_capture = layout
                        .offset()
                        .checked_sub(capture.offset())
                        .expect("XML capture within entry");
                    let from_capture =
                        usize::try_from(relative_capture).expect("XML capture entry offset");
                    let to_capture = from_capture
                        .checked_add(
                            usize::try_from(layout.byte_len()).expect("XML capture entry length"),
                        )
                        .expect("XML capture entry end");
                    assert!(
                        view.window().get(from_capture..to_capture).is_some(),
                        "XML record capture must fit its original member"
                    );
                    let relative = payload_start
                        .checked_sub(capture.offset())
                        .expect("XML offset within entry");
                    let from = usize::try_from(relative).expect("XML entry offset");
                    let to = from.checked_add(bytes.len()).expect("XML entry end");
                    assert_eq!(bytes.as_ref(), &view.window()[from..to]);
                    matched = true;
                    break;
                }
                assert!(matched, "original XML source stream must exist in archive");
                ctx.finish_session()
                    .expect("original XML evidence verification budget");
            }
            return bytes.as_ref();
        }
    }
    let record = fidelity.retained_record(id).expect("source record");
    let bytes = if let Some(bytes) = record.data() {
        bytes
    } else {
        let stream = fidelity
            .retained_records()
            .values()
            .find(|candidate| {
                candidate.stream() == record.stream()
                    && candidate.offset() == 0
                    && candidate.data().is_some()
            })
            .expect("source stream image");
        let start = usize::try_from(record.offset()).expect("source offset");
        let end = usize::try_from(record.end_offset()).expect("source end");
        &stream.data().expect("retained image")[start..end]
    };
    assert_eq!(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        record.byte_len()
    );
    assert_eq!(cadmpeg_ir::hash::sha256_hex(bytes), record.sha256());
    bytes
}

/// Reopen an original member without retaining a second expanded image.
pub(crate) fn entry_bytes(fidelity: &cadmpeg_ir::SourceFidelity, id: &str) -> Vec<u8> {
    if let Some(bytes) = fidelity
        .retained_record(id)
        .and_then(|record| record.data())
    {
        return bytes.to_vec();
    }
    let layout = fidelity
        .record_layout()
        .get(id)
        .expect("entry source layout");
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let image = crate::source_archive::image(fidelity).expect("original archive image");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(image, &arena, &policy)
        .expect("genuine original archive source");
    let archive = crate::source_archive::OriginalArchive::new(&ctx, root, fidelity)
        .expect("original archive");
    let view = archive.stream(&ctx, layout).expect("original named member");
    let capture = ctx
        .capture_part_extent(view)
        .expect("original entry capture");
    assert_eq!(capture.stream(), layout.stream());
    let relative = layout
        .offset()
        .checked_sub(capture.offset())
        .expect("source offset within entry");
    let from = usize::try_from(relative).expect("entry source offset");
    let length = usize::try_from(layout.byte_len()).expect("entry source length");
    let to = from.checked_add(length).expect("entry source end");
    let bytes = view
        .window()
        .get(from..to)
        .expect("entry source range")
        .to_vec();
    assert_eq!(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        layout.byte_len()
    );
    ctx.finish_session()
        .expect("original entry source verification budget");
    bytes
}

pub(crate) fn layout<T: serde::de::DeserializeOwned>(
    fidelity: &cadmpeg_ir::SourceFidelity,
    name: &str,
) -> T {
    let mut all = fidelity.freecad_ledgers().values();
    let ledgers = all.next().expect("FCStd source ledgers");
    assert!(all.next().is_none(), "one original FCStd archive");
    let value = match name {
        "physical-ledger" => serde_json::to_value(&ledgers.physical_ledger),
        "logical-ledger" => serde_json::to_value(&ledgers.logical_ledger),
        "byte-coverage" => serde_json::to_value(&ledgers.byte_coverage),
        _ => panic!("unknown FCStd source-ledger name {name}"),
    }
    .expect("typed source-ledger value");
    serde_json::from_value(value).expect("source layout has its declared typed shape")
}

/// Preserve an authored source-XML fixture after its native wrapper is removed.
pub(crate) fn xml_layout_fixture(owner: &str, xml: &str) -> cadmpeg_ir::SourceFidelity {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeMap;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
            .expect("fixture source");
    let mut payloads = BTreeMap::new();
    crate::fidelity::xml_layout_payload(&ctx, root, owner, 0..xml.len(), &mut payloads)
        .expect("fixture XML payload");
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    crate::fidelity::install_layouts(&ctx, &mut fidelity, payloads, BTreeMap::new())
        .expect("fixture XML layout");
    assert_eq!(
        fidelity.record_layout()[owner].payloads()["raw_xml"].as_ref(),
        xml.as_bytes()
    );
    fidelity
}

/// An authored test source kept outside the semantic native record.
#[derive(Clone)]
pub(crate) struct PropertyFixture {
    pub(crate) property: crate::native::PropertyRecord,
    pub(crate) source: String,
}

impl PropertyFixture {
    pub(crate) fn new(property: crate::native::PropertyRecord, source: impl Into<String>) -> Self {
        Self {
            property,
            source: source.into(),
        }
    }
}

impl std::ops::Deref for PropertyFixture {
    type Target = crate::native::PropertyRecord;
    fn deref(&self) -> &Self::Target {
        &self.property
    }
}

impl std::ops::DerefMut for PropertyFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.property
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EntryFixture {
    pub(crate) record: crate::native::EntryRecord,
    pub(crate) source: Vec<u8>,
}

impl EntryFixture {
    pub(crate) fn try_new(
        id: String,
        name: String,
        role: cadmpeg_core::container::ContainerRole,
        referenced_by: Vec<String>,
        source: Vec<u8>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self {
            record: crate::native::EntryRecord::new(id, name, role, referenced_by)?,
            source,
        })
    }
}

pub(crate) fn entry_layout_fixture(entry: &EntryFixture) -> cadmpeg_ir::SourceFidelity {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let (context, source) =
        DecodeContext::from_root_bytes(&entry.source, &arena, &DecodePolicy::service()).unwrap();
    let source = context.name_stream(source, entry.name()).unwrap();
    context
        .record(
            entry.id(),
            source,
            || Ok::<(), cadmpeg_core::CodecError>(()),
        )
        .unwrap();
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    crate::fidelity::retain(
        &mut fidelity,
        entry.id(),
        entry.name(),
        0,
        source.window(),
        cadmpeg_ir::ArenaClasses::new(),
    )
    .unwrap();
    crate::fidelity::install_layouts(&context, &mut fidelity, BTreeMap::new(), BTreeMap::new())
        .unwrap();
    let layout = &fidelity.record_layout()[entry.id()];
    assert_eq!(
        layout.byte_len(),
        u64::try_from(entry.source.len()).unwrap()
    );
    assert!(layout.payloads().is_empty());
    fidelity
}

impl std::ops::Deref for EntryFixture {
    type Target = crate::native::EntryRecord;
    fn deref(&self) -> &Self::Target {
        &self.record
    }
}

impl std::ops::DerefMut for EntryFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.record
    }
}

pub(crate) fn with_property_reader<T>(
    input: &[u8],
    properties: &[&PropertyFixture],
    policy: &cadmpeg_core::decode::DecodePolicy,
    use_reader: impl FnOnce(&crate::property_reader::PropertyReader<'_, '_>) -> T,
) -> T {
    with_decode_sources(input, properties, &[], policy, |reader, _| {
        use_reader(reader)
    })
}

pub(crate) fn with_decode_sources<T>(
    input: &[u8],
    properties: &[&PropertyFixture],
    entries: &[&EntryFixture],
    policy: &cadmpeg_core::decode::DecodePolicy,
    use_sources: impl FnOnce(
        &crate::property_reader::PropertyReader<'_, '_>,
        &crate::entry_sources::EntrySources<'_>,
    ) -> T,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    use std::collections::BTreeMap;
    let mut bytes = input.to_vec();
    let mut property_ranges = BTreeMap::new();
    for property in properties {
        let range = fixture_range(&mut bytes, input, property.source.as_bytes());
        assert!(
            property_ranges.insert(property.id.clone(), range).is_none(),
            "unique source fixture id"
        );
    }
    let mut entry_ranges = BTreeMap::new();
    for entry in entries {
        let range = fixture_range(&mut bytes, input, &entry.source);
        assert!(
            entry_ranges
                .insert(entry.name().to_owned(), range)
                .is_none(),
            "unique source entry fixture name"
        );
    }
    let arena = DecodeArena::new();
    let (context, root) = DecodeContext::from_root_bytes(&bytes, &arena, policy)
        .expect("authored source bytes fit the test policy");
    let properties = property_ranges
        .into_iter()
        .map(|(id, range)| {
            (
                id,
                root.child(range.start, range.end)
                    .expect("original property source extent"),
            )
        })
        .collect();
    let entries = entry_ranges
        .into_iter()
        .map(|(name, range)| {
            (
                name,
                root.child(range.start, range.end)
                    .expect("original entry source extent"),
            )
        })
        .collect();
    let reader = crate::property_reader::PropertyReader::fixtures(&context, properties)
        .expect("property Views belong to the real source session");
    let sources = crate::entry_sources::EntrySources::new(&context, &entries)
        .expect("entry Views belong to the real source session");
    use_sources(&reader, &sources)
}

fn fixture_range(bytes: &mut Vec<u8>, input: &[u8], source: &[u8]) -> std::ops::Range<usize> {
    if source.is_empty() {
        return bytes.len()..bytes.len();
    }
    if let Some(start) = input
        .windows(source.len())
        .position(|window| window == source)
    {
        return start..start + source.len();
    }
    let start = bytes.len();
    bytes.extend_from_slice(source);
    start..bytes.len()
}
