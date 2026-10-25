// SPDX-License-Identifier: Apache-2.0
//! Each asset consumes its own original controls, fixed row, and used UUID.

use crate::native::entry_contents::EntryContents;
use crate::native::EntryRecord;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, DecodeSnapshot, Mode};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::ops::Range;

const DOCUMENT: &str = "<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='A' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='A'><Properties Count='0'/></Object></ObjectData></Document>";
const GUI: &str = "<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='A' expanded='1'><Properties Count='2'><Property name='DiffuseColor' type='App::PropertyColorList'><ColorList file='colors' version='99'/></Property><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='materials' version='3'/></Property></Properties></ViewProvider></ViewProviderData></Document>";

fn material() -> (Vec<u8>, Vec<Option<Range<u64>>>) {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    for color in [0x11223344_u32, 0x55667788, 0x99aabbcc] {
        for packed in [color, color, 0xffeeddcc, 0x01020304] {
            bytes.extend_from_slice(&packed.to_le_bytes());
        }
        bytes.extend_from_slice(&0.5_f32.to_le_bytes());
        bytes.extend_from_slice(&0.25_f32.to_le_bytes());
    }
    let mut uuids = Vec::new();
    for (index, uuid) in ["uuid-0", "", "uuid-2"].into_iter().enumerate() {
        for text in [
            format!("image-{index}"),
            format!("path-{index}"),
            uuid.into(),
        ] {
            let start = bytes.len();
            bytes.extend_from_slice(&u32::try_from(text.len()).unwrap().to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
            if text == uuid {
                uuids.push(
                    (!uuid.is_empty()).then_some(
                        u64::try_from(start).unwrap()..u64::try_from(bytes.len()).unwrap(),
                    ),
                );
            }
        }
    }
    assert_eq!(uuids.len(), 3);
    (bytes, uuids)
}

fn archive(material: &[u8]) -> Vec<u8> {
    let mut colors = 3_u32.to_le_bytes().to_vec();
    for packed in [0x11223344_u32, 0x55667788, 0x99aabbcc] {
        colors.extend_from_slice(&packed.to_le_bytes());
    }
    crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", DOCUMENT.as_bytes()),
        ("GuiDocument.xml", GUI.as_bytes()),
        ("colors", &colors),
        ("materials", material),
    ])
}

fn decode(bytes: &[u8]) -> DecodeResult {
    let result = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    crate::fidelity::b1::assert_classified_spans(&result);
    result
}

fn merge(mut ranges: Vec<Range<u64>>) -> Vec<Range<u64>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<u64>> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn attribute(node: roxmltree::Node<'_, '_>, name: &str) -> Vec<Range<u64>> {
    let field = node.attribute_node(name).unwrap();
    [field.range_qname(), field.range_value()]
        .into_iter()
        .filter(|range| !range.is_empty())
        .map(|range| u64::try_from(range.start).unwrap()..u64::try_from(range.end).unwrap())
        .collect()
}

fn controls(name: &str) -> Vec<Range<u64>> {
    let xml = roxmltree::Document::parse(GUI).unwrap();
    let provider = xml
        .descendants()
        .find(|node| node.has_tag_name("ViewProvider"))
        .unwrap();
    let property = provider
        .descendants()
        .find(|node| node.has_tag_name("Property") && node.attribute("name") == Some(name))
        .unwrap();
    let value = property
        .children()
        .find(roxmltree::Node::is_element)
        .unwrap();
    let mut ranges = attribute(provider, "name");
    ranges.extend(attribute(property, "name"));
    ranges.extend(attribute(property, "type"));
    ranges.extend(attribute(value, "file"));
    if name == "ShapeAppearance" {
        ranges.extend(attribute(value, "version"));
    }
    merge(ranges)
}

fn entries(result: &DecodeResult) -> Vec<EntryRecord> {
    result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as("entries")
        .unwrap()
}

#[test]
fn each_color_and_material_asset_reads_only_its_own_original_fields() {
    let (material, uuids) = material();
    let bytes = archive(&material);
    let result = decode(&bytes);
    assert!(result
        .source_fidelity()
        .parts()
        .values()
        .all(|part| part.state() != &(PartState::GivenUp {})));
    assert_eq!(result.ir().model.appearances.len(), 6);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let physical = cadmpeg_test_support::framing::zip::frame(&ctx, root, &mut Vec::new()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let stream = |name| {
        ctx.capture_part_extent(archive.open(&ctx, name).unwrap())
            .unwrap()
            .stream()
            .to_owned()
    };
    let app = roxmltree::Document::parse(DOCUMENT).unwrap();
    let declaration = app
        .descendants()
        .find(|node| node.has_tag_name("Object") && node.attribute("type").is_some())
        .unwrap();
    let object_name = attribute(declaration, "name");
    let all_entries = entries(&result);
    for (property, member, schema, width) in [
        ("DiffuseColor", "colors", "FCStd DiffuseColor", 4_u64),
        (
            "ShapeAppearance",
            "materials",
            "FCStd ShapeAppearance",
            24_u64,
        ),
    ] {
        let entry = all_entries
            .iter()
            .find(|entry| entry.name() == member)
            .unwrap();
        let mut assets = result
            .ir()
            .model
            .appearances
            .iter()
            .filter(|asset| asset.schema.as_deref() == Some(schema))
            .collect::<Vec<_>>();
        assets.sort_by(|left, right| left.name.cmp(&right.name));
        assert_eq!(assets.len(), 3);
        for (index, asset) in assets.into_iter().enumerate() {
            assert_eq!(asset.native_ref.as_deref(), Some(entry.id()));
            let index = u64::try_from(index).unwrap();
            let mut binary = vec![0..4, 4 + index * width..4 + (index + 1) * width];
            if property == "ShapeAppearance" {
                if let Some(uuid) = &uuids[usize::try_from(index).unwrap()] {
                    binary.push(uuid.clone());
                }
                assert_eq!(
                    asset.asset_guid.as_deref(),
                    [Some("uuid-0"), None, Some("uuid-2")][usize::try_from(index).unwrap()]
                );
            }
            let mut expected = BTreeMap::from([
                (stream("Document.xml"), object_name.clone()),
                (stream("GuiDocument.xml"), controls(property)),
                (stream(member), merge(binary)),
            ]);
            let mut original = Vec::new();
            for name in ["Document.xml", "GuiDocument.xml", member] {
                let entry = physical
                    .iter()
                    .find(|entry| entry.name().window() == name.as_bytes())
                    .unwrap();
                assert_eq!(
                    entry.method(),
                    0,
                    "authored source has exact stored aliases"
                );
                let offset = u64::try_from(entry.payload().start()).unwrap();
                original.extend(
                    expected[&stream(name)]
                        .iter()
                        .map(|read| offset + read.start..offset + read.end),
                );
            }
            expected.insert("file".into(), merge(original));
            assert_eq!(
                result.source_fidelity().record_layout()[asset.id.as_str()].reads(),
                &expected,
                "exact asset source fields: {property} slot {index}"
            );
        }
    }
}

#[test]
fn a_nonfinite_material_fixed_row_keeps_its_strings_entry_and_independent_assets() {
    let (material, _) = material();
    let original = archive(&material);
    let before = decode(&original);
    let mut damaged = material;
    damaged[44..48].copy_from_slice(&f32::NAN.to_le_bytes());
    let changed = archive(&damaged);
    let after = decode(&changed);
    let failed = after
        .source_fidelity()
        .parts()
        .iter()
        .filter(|(_, part)| part.state() == &(PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1);
    let (_, part) = failed[0];
    assert_eq!(part.level().as_str(), "freecad.material_list_material");
    assert_eq!((part.offset(), part.byte_len()), (28, 24));
    let errors = after
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
    let source = errors[0].provenance.as_ref().unwrap();
    assert_eq!(source.stream_name(), Some(part.stream()));
    assert_eq!((source.offset, source.byte_len()), (28, Some(24)));
    let mut expected = entries(&before);
    let materials = expected
        .iter_mut()
        .find(|entry| entry.name() == "materials")
        .unwrap();
    let EntryContents::Material { values, .. } = &mut materials.contents[0] else {
        panic!("registered materials");
    };
    values[1].fixed = None;
    assert_eq!(
        serde_json::to_value(entries(&after)).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let peer_assets = before
        .ir()
        .model
        .appearances
        .iter()
        .filter(|asset| asset.name.as_deref() != Some("A face 2 material"))
        .collect::<Vec<_>>();
    assert_eq!(after.ir().model.appearances.len(), 5);
    for asset in peer_assets {
        assert_eq!(
            after
                .ir()
                .model
                .appearances
                .iter()
                .find(|current| current.id == asset.id),
            Some(asset)
        );
    }
    let old_arena = DecodeArena::new();
    let (old_ctx, old_root) =
        DecodeContext::from_root_bytes(&original, &old_arena, &DecodePolicy::service()).unwrap();
    let old_root = old_ctx.name_stream(old_root, "file").unwrap();
    let new_arena = DecodeArena::new();
    let (new_ctx, new_root) =
        DecodeContext::from_root_bytes(&changed, &new_arena, &DecodePolicy::service()).unwrap();
    let new_root = new_ctx.name_stream(new_root, "file").unwrap();
    let allowed = Allowances::given_up_zip(
        DecodeSnapshot::new(before.ir(), before.source_fidelity()),
        DecodeSnapshot::new(after.ir(), after.source_fidelity()),
        after.report(),
        (&old_ctx, old_root),
        (&new_ctx, new_root),
    )
    .unwrap();
    assert_decodes_agree(
        &serde_json::to_value(before.ir()).unwrap(),
        &serde_json::to_value(after.ir()).unwrap(),
        &allowed,
        Mode::Same,
    );
}
