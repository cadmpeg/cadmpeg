// SPDX-License-Identifier: Apache-2.0
//! Assertions over source-only bytes and layout retained by the codec.

pub(crate) fn bytes<'a>(fidelity: &'a cadmpeg_ir::SourceFidelity, id: &str) -> &'a [u8] {
    if let Some(layout) = fidelity.record_layout().get(id) {
        if let Some(bytes) = layout.payloads().get("raw_xml") {
            assert_eq!(
                cadmpeg_core::decode::u64_from_index(bytes.len()),
                layout.byte_len()
            );
            let start = usize::try_from(layout.offset()).expect("XML source offset");
            let end = start.checked_add(bytes.len()).expect("XML source end");
            if let Some(image) = fidelity.retained_records().values().find(|record| {
                record.stream() == layout.stream()
                    && record.offset() == 0
                    && record.data().is_some()
            }) {
                assert_eq!(
                    bytes.as_ref(),
                    &image.data().expect("XML source bytes")[start..end]
                );
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
                    let relative = layout
                        .offset()
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

pub(crate) fn layout<T: serde::de::DeserializeOwned>(
    fidelity: &cadmpeg_ir::SourceFidelity,
    name: &str,
) -> T {
    let id = crate::native::native_id("source-layout", name);
    serde_json::from_slice(bytes(fidelity, &id)).expect("retained source layout")
}

/// Preserve source-value fixture inputs after their field leaves native records.
pub(crate) fn value_layout_fixture(owner: &str, raw_values: &[&str]) -> cadmpeg_ir::SourceFidelity {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeMap;
    let xml = format!("<Property>{}</Property>", raw_values.join(""));
    let document = roxmltree::Document::parse(&xml).expect("source-value fixture XML");
    let values = document
        .root_element()
        .descendants()
        .filter(|node| node.is_element() && *node != document.root_element())
        .enumerate()
        .map(|(order, node)| crate::native::ValueRecord {
            tag: node.tag_name().name().into(),
            order,
            parent: None,
            attributes: node
                .attributes()
                .map(|attribute| (attribute.name().into(), attribute.value().into()))
                .collect(),
            decoded_json: None,
            text: node.text().map(str::to_owned),
        })
        .collect::<Vec<_>>();
    let retained = crate::native::RetainedXml::from_source(&xml, 0).expect("fixture source extent");
    let sources = crate::fidelity::value_sources(
        &document,
        std::iter::once((owner, &retained, values.as_slice())),
    )
    .expect("fixture value ranges");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &DecodePolicy::service())
            .expect("fixture source");
    let mut pending = BTreeMap::new();
    for source in &sources {
        crate::fidelity::value_layout_payloads(&ctx, root, source, &mut pending)
            .expect("fixture source payloads");
    }
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    crate::fidelity::install_layouts(&ctx, &mut fidelity, pending, BTreeMap::new())
        .expect("fixture layouts");
    for (order, raw) in raw_values.iter().enumerate() {
        assert_eq!(
            fidelity.record_layout()[owner].payloads()[&format!("values.{order}.raw_xml")].as_ref(),
            raw.as_bytes()
        );
    }
    fidelity
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
