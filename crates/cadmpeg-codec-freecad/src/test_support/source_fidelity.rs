// SPDX-License-Identifier: Apache-2.0
//! Assertions over source-only bytes retained by the codec.

/// The exact source bytes of a retained record, reopened from the original
/// archive member when the record holds only a digest.
pub(crate) fn bytes(fidelity: &cadmpeg_ir::SourceFidelity, id: &str) -> Vec<u8> {
    let record = fidelity.retained_record(id).expect("source record");
    let bytes = if let Some(bytes) = record.data() {
        bytes.to_vec()
    } else {
        let start = usize::try_from(record.offset()).expect("source offset");
        let end = usize::try_from(record.end_offset()).expect("source end");
        member_bytes(fidelity, record.stream())[start..end].to_vec()
    };
    assert_eq!(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        record.byte_len()
    );
    assert_eq!(cadmpeg_ir::hash::sha256_hex(&bytes), record.sha256());
    bytes
}

/// Retained records of given-up parts, in identity order.
pub(crate) fn given_up(
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> Vec<(&str, &cadmpeg_ir::RetainedSourceRecord)> {
    fidelity
        .retained_records()
        .iter()
        .filter(|(id, _)| id.as_str().starts_with("fcstd:source:part#"))
        .map(|(id, record)| (id.as_str(), record))
        .collect()
}

/// Reopen an original member without retaining a second expanded image.
pub(crate) fn entry_bytes(fidelity: &cadmpeg_ir::SourceFidelity, id: &str) -> Vec<u8> {
    if let Some(bytes) = fidelity
        .retained_record(id)
        .and_then(|record| record.data())
    {
        return bytes.to_vec();
    }
    let image = crate::source_archive::image(fidelity).expect("original archive image");
    let name = archive_names(image)
        .into_iter()
        .find(|name| crate::native::native_id("entry", name) == id)
        .expect("original named member");
    member_bytes(fidelity, &name)
}

fn archive_names(image: &[u8]) -> Vec<String> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(image, &arena, &policy)
        .expect("genuine original archive source");
    cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
        .expect("original archive")
        .entries()
        .iter()
        .map(|entry| entry.name.clone())
        .collect()
}

fn member_bytes(fidelity: &cadmpeg_ir::SourceFidelity, name: &str) -> Vec<u8> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let image = crate::source_archive::image(fidelity).expect("original archive image");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(image, &arena, &policy)
        .expect("genuine original archive source");
    cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
        .expect("original archive")
        .open(&ctx, name)
        .expect("original member")
        .window()
        .to_vec()
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
    let mut fidelity = cadmpeg_ir::SourceFidelity::with_annotations(
        cadmpeg_ir::Annotations::default(),
        cadmpeg_ir::hash::digest::Sha256Digest::digest(&entry.source),
    );
    crate::fidelity::retain(
        &mut fidelity,
        entry.id(),
        entry.name(),
        0,
        &entry.source,
        cadmpeg_ir::ArenaClasses::new(),
    )
    .unwrap();
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
    let reader = crate::property_reader::PropertyReader::fixtures(&context, properties);
    let sources = crate::entry_sources::EntrySources::new(&entries);
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
