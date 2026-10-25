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
    if let Some(view) = archive
        .entry_for_id(&ctx, id)
        .expect("actual entry identity")
    {
        return view.window().to_vec();
    }
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
    crate::fidelity::retain(&mut fidelity, entry.id(), entry.name(), 0, source.window()).unwrap();
    crate::fidelity::install_layouts(
        &context,
        &mut fidelity,
        BTreeMap::new(),
        BTreeMap::new(),
        None,
    )
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
    let reader = crate::property_reader::PropertyReader::fixtures(&context, properties, root)
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

/// Compare every failed XML part with independently selected source literals.
pub(crate) fn assert_failed_xml_parts(
    decoded: &cadmpeg_ir::codec::DecodeResult,
    source: &str,
    expected: &[(&str, &str)],
) {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let mut seen = BTreeMap::<&str, usize>::new();
    let mut expected = expected
        .iter()
        .map(|(level, literal)| {
            let ordinal = seen.entry(literal).or_default();
            let start = source
                .match_indices(literal)
                .nth(*ordinal)
                .expect("authored failed source literal ordinal")
                .0;
            *ordinal += 1;
            (
                *level,
                u64::try_from(start).unwrap(),
                u64::try_from(literal.len()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    expected.sort_unstable();
    let mut actual = decoded
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &PartState::GivenUp {})
        .map(|part| (part.level().as_str(), part.offset(), part.byte_len()))
        .collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(
        actual, expected,
        "every failure stays at its exact authored XML part"
    );
    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), expected.len());
    for (_, offset, length) in expected {
        assert_eq!(
            losses
                .iter()
                .filter(|loss| loss.provenance.as_ref().is_some_and(|home| {
                    home.offset == offset && home.byte_len() == Some(length)
                }))
                .count(),
            1,
            "one diagnostic at the same source home"
        );
    }
}

pub(crate) fn assert_single_property_part(decoded: &cadmpeg_ir::codec::DecodeResult, xml: &str) {
    let document = roxmltree::Document::parse(xml).unwrap();
    let elements = document
        .descendants()
        .filter(|node| node.has_tag_name("Property"))
        .collect::<Vec<_>>();
    let [element] = elements.as_slice() else {
        panic!("one authored property")
    };
    let parts = decoded.source_fidelity().parts();
    let properties = parts
        .values()
        .filter(|part| part.level().as_str() == "freecad.property")
        .collect::<Vec<_>>();
    let [part] = properties.as_slice() else {
        panic!("one actual property part: {properties:?}")
    };
    assert_eq!(
        part.state(),
        &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
    );
    assert_eq!(part.offset(), u64::try_from(element.range().start).unwrap());
    assert_eq!(
        part.byte_len(),
        u64::try_from(element.range().len()).unwrap()
    );
    assert_eq!(
        parts[part.parent().unwrap()].level().as_str(),
        "freecad.properties"
    );
}

pub(crate) fn assert_entry_header_layout(
    decoded: &cadmpeg_ir::codec::DecodeResult,
    bytes: &[u8],
    name: &str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_test_support::framing::zip;
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let entry = entries
        .iter()
        .find(|entry| entry.name().window() == name.as_bytes())
        .unwrap();
    let headers = frames
        .iter()
        .filter(|part| {
            part.level().as_str() == "zip.local_header"
                && part.view().end() == entry.payload().start()
        })
        .collect::<Vec<_>>();
    let [header] = headers.as_slice() else {
        panic!("one exact physical local header")
    };
    let layout =
        &decoded.source_fidelity().record_layout()[&crate::native::native_id("entry", name)];
    let expected = header.capture();
    assert_eq!(layout.stream(), expected.stream());
    assert_eq!(layout.offset(), expected.offset());
    assert_eq!(layout.byte_len(), expected.byte_len());
    assert_eq!(
        layout.reads(),
        &BTreeMap::from([(
            "file".into(),
            vec![
                u64::try_from(entry.name().start()).unwrap()
                    ..u64::try_from(entry.name().end()).unwrap()
            ]
        )])
    );
    assert!(layout.payloads().is_empty());
}

pub(crate) fn project_document_attributes<'source>(
    ctx: &cadmpeg_core::decode::DecodeContext<'source>,
    source: cadmpeg_core::decode::View<'source>,
    properties: &crate::persistence::document_properties::RootProperties,
    output: &mut cadmpeg_ir::source_fidelity::parts::PartOutput,
) {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let reader = crate::property_reader::PropertyReader::typed(ctx);
    let bindings = crate::design::ExpressionBindings::default();
    let (losses, unformed) = crate::attributes::transfer_source(
        ctx,
        &mut ir,
        &[],
        &properties.properties,
        source,
        &properties.sources,
        &reader,
        &bindings,
    )
    .unwrap();
    assert!(losses.is_empty());
    assert!(unformed.is_empty());
    for attribute in ir.model.attributes {
        output.insert_model(ctx, attribute).unwrap();
    }
}
