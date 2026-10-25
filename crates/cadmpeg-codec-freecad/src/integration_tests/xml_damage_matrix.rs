// SPDX-License-Identifier: Apache-2.0
//! Appendix C XML content damage with independent source and comparison oracles.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::ops::Range;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::comparison::{assert_decodes_agree, Allowances, DecodeSnapshot, Mode};
use cadmpeg_test_support::framing::{freecad, zip, PartTable};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};

mod known_kind;
mod public;

fn properties() -> String {
    let mut text = "<Properties Count='3'>".to_owned();
    for index in 0..3 {
        text.push_str(&format!("<Property name='Items{index}' type='App::PropertyStringList'><StringList count='3'><String value='alpha'/><String value='beta'/><String value='gamma'/></StringList></Property>"));
    }
    text.push_str("</Properties>");
    text
}

fn fixture() -> Vec<u8> {
    let names = ["A", "B", "C", "X", "Y", "Z"];
    let mut document = "<Document SchemaVersion='4' FileVersion='1'>".to_owned();
    document.push_str(&properties());
    document
        .push_str("<StringHasher count='0'></StringHasher><Objects Count='6' Dependencies='1'>");
    for (index, name) in names.iter().enumerate() {
        let dependencies = if index < 3 {
            "<Dep Name='X'/><Dep Name='Y'/><Dep Name='Z'/>"
        } else {
            ""
        };
        document.push_str(&format!(
            "<ObjectDeps Name='{name}' Count='{}'>{dependencies}</ObjectDeps>",
            if index < 3 { 3 } else { 0 }
        ));
    }
    for name in names {
        document.push_str(&format!(
            "<Object name='{name}' type='App::FeaturePython'/>"
        ));
    }
    document.push_str("</Objects><ObjectData Count='6'>");
    for name in names {
        document.push_str(&format!("<Object name='{name}'><Extensions Count='3'>"));
        for index in 0..3 {
            document.push_str(&format!(
                "<Extension name='E{index}' type='Vendor::Extension{index}'>"
            ));
            if index == 1 {
                document.push_str(&properties());
            }
            document.push_str("</Extension>");
        }
        document.push_str("</Extensions>");
        document.push_str(&properties());
        document.push_str("</Object>");
    }
    document.push_str("</ObjectData></Document>");
    let mut gui = "<Document SchemaVersion='1'>".to_owned();
    for state in 0..3 {
        gui.push_str(&format!("<Workbench name='S{state}'>"));
        for panel in 0..3 {
            gui.push_str(&format!("<Panel name='P{panel}'><Leaf value='a'/><Leaf value='b'/><Leaf value='c'/></Panel>"));
        }
        gui.push_str("</Workbench>");
    }
    gui.push_str("<ViewProviderData Count='3'>");
    for name in ["A", "B", "C"] {
        gui.push_str(&format!(
            "<ViewProvider name='{name}' expanded='1'><Extensions Count='3'>"
        ));
        for index in 0..3 {
            gui.push_str(&format!(
                "<Extension name='E{index}' type='Vendor::GuiExtension{index}'/>"
            ));
        }
        gui.push_str("</Extensions>");
        gui.push_str(&properties());
        gui.push_str("</ViewProvider>");
    }
    gui.push_str("</ViewProviderData></Document>");
    zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document.as_bytes()),
        RawEntry::stored("GuiDocument.xml", gui.as_bytes()),
        RawEntry::stored("independent.bin", b"independent opaque source"),
    ])
}

#[derive(Debug)]
struct Selected {
    member: String,
    level: String,
    extent: Range<usize>,
    framing: Vec<Range<usize>>,
}

fn selected(bytes: &[u8], complete_inventory: bool) -> Vec<Selected> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut groups = BTreeMap::<String, Vec<Selected>>::new();
    for entry in entries {
        if !matches!(entry.name().window(), b"Document.xml" | b"GuiDocument.xml") {
            continue;
        }
        let name = std::str::from_utf8(entry.name().window()).unwrap();
        let member = archive.open(&ctx, name).unwrap();
        let start = frames.len();
        freecad::frame_xml_member(&ctx, member, entry.name(), entry.frame_index(), &mut frames)
            .unwrap();
        for frame in &frames[start..] {
            groups
                .entry(frame.level().as_str().to_owned())
                .or_default()
                .push(Selected {
                    member: name.to_owned(),
                    level: frame.level().as_str().to_owned(),
                    extent: frame.view().start()..frame.view().end(),
                    framing: frame
                        .framing()
                        .iter()
                        .map(|range| {
                            usize::try_from(range.start).unwrap()
                                ..usize::try_from(range.end).unwrap()
                        })
                        .collect(),
                });
        }
    }
    PartTable::load().unwrap().validate(&ctx, &frames).unwrap();
    let expected = [
        "document_xml",
        "document",
        "string_hasher",
        "objects",
        "object_deps",
        "object_dep",
        "object_declaration",
        "object_data_section",
        "object_data",
        "extensions",
        "extension",
        "properties",
        "property",
        "property_value",
        "value_element",
        "gui_document_xml",
        "gui_document",
        "gui_state",
        "gui_state_element",
        "view_provider_data",
        "view_provider",
        "gui_extensions",
        "gui_extension",
        "gui_properties",
        "gui_property",
        "gui_property_value",
        "gui_value_element",
    ]
    .into_iter()
    .map(|level| format!("freecad.{level}"))
    .collect::<BTreeSet<_>>();
    if complete_inventory {
        assert_eq!(groups.keys().cloned().collect::<BTreeSet<_>>(), expected);
    } else {
        assert!(!groups.is_empty(), "fixture has no XML part inventory");
        assert!(groups.keys().all(|level| expected.contains(level)));
    }
    let mut result = Vec::new();
    for mut peers in groups.into_values() {
        let indices = [0, peers.len() / 2, peers.len() - 1]
            .into_iter()
            .collect::<BTreeSet<_>>();
        for (index, peer) in peers.drain(..).enumerate() {
            if indices.contains(&index) {
                result.push(peer);
            }
        }
    }
    result
}

fn damage(bytes: &[u8], selected: &Selected, replacement: u8) -> Vec<u8> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let member = archive.open(&ctx, &selected.member).unwrap();
    let mut changed = member.window().to_vec();
    let mut count = 0;
    for offset in selected.extent.clone() {
        if !selected.framing.iter().any(|range| range.contains(&offset)) {
            changed[offset] = replacement;
            count += 1;
        }
    }
    assert!(
        count > 0,
        "selected XML part must have actual content: {selected:?}"
    );
    zip::rebuild_member(&ctx, root, selected.member.as_bytes(), &changed).unwrap()
}

fn compare(previous: &[u8], bytes: &[u8], before: &DecodeResult, after: &DecodeResult) {
    let old_arena = DecodeArena::new();
    let (old_ctx, old_root) =
        DecodeContext::from_root_bytes(previous, &old_arena, &DecodePolicy::service()).unwrap();
    let old_root = old_ctx.name_stream(old_root, "file").unwrap();
    let new_arena = DecodeArena::new();
    let (new_ctx, new_root) =
        DecodeContext::from_root_bytes(bytes, &new_arena, &DecodePolicy::service()).unwrap();
    let new_root = new_ctx.name_stream(new_root, "file").unwrap();
    let allowances = if after
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.state() == &(PartState::GivenUp {}))
    {
        Allowances::given_up_zip(
            DecodeSnapshot::new(before.ir(), before.source_fidelity()),
            DecodeSnapshot::new(after.ir(), after.source_fidelity()),
            after.report(),
            (&old_ctx, old_root),
            (&new_ctx, new_root),
        )
        .unwrap()
    } else {
        // A complete local-member prefix can lose only its ZIP directory.
        // With no failed source helper there is no permission to change a row.
        Allowances::default()
    };
    assert_decodes_agree(
        &serde_json::to_value(before.ir()).unwrap(),
        &serde_json::to_value(after.ir()).unwrap(),
        &allowances,
        Mode::Same,
    );
    let baseline = before
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect::<BTreeSet<_>>();
    let missing = after
        .ir()
        .scope
        .missing_from(cadmpeg_core::decode::DecodeDepth::Full.required_scope());
    let failed_root_prefix = bytes.len() < previous.len()
        && previous.starts_with(bytes)
        && after.source_fidelity().parts().values().any(|part| {
            part.level().as_str() == "zip.archive"
                && part.parent().is_none()
                && part.state() == &(PartState::GivenUp {})
        });
    let expected_depth = (failed_root_prefix && !missing.is_empty()).then(|| {
        cadmpeg_ir::codec::DecodeLossCode::DepthNotDelivered {
            delivered: after.ir().scope,
        }
        .note(format!(
            "requested full depth but the fcstd decoder did not deliver classes: {missing}"
        ))
    });
    assert_eq!(
        after
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.to_string() == "decode/depth.not-delivered")
            .collect::<Vec<_>>(),
        expected_depth.iter().collect::<Vec<_>>(),
        "only the exact shared diagnostic follows proven class loss"
    );
    for loss in &after.report().losses {
        assert!(
            loss.code.local_code() == "record.unreadable"
                || baseline.contains(&loss.code.to_string())
                || expected_depth.as_ref() == Some(loss),
            "new non-helper loss: {loss:?}"
        );
    }
}

#[test]
fn application_dependencies_read_their_actual_source_fields() {
    let bytes = fixture();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let document = archive.open(&ctx, "Document.xml").unwrap();
    let text = std::str::from_utf8(document.window()).unwrap();
    let mut reads = Vec::new();
    let owner = text.find("<Object name='A' type=").unwrap();
    for (key, value, length) in [(owner + "<Object ".len(), owner + "<Object name='".len(), 1)] {
        reads.push(u64::try_from(key).unwrap()..u64::try_from(key + 4).unwrap());
        reads.push(u64::try_from(value).unwrap()..u64::try_from(value + length).unwrap());
    }
    for name in ["X", "Y", "Z"] {
        let at = text.find(&format!("<Dep Name='{name}'/>")).unwrap();
        let key = at + "<Dep ".len();
        let value = at + "<Dep Name='".len();
        reads.push(u64::try_from(key).unwrap()..u64::try_from(key + 4).unwrap());
        reads.push(u64::try_from(value).unwrap()..u64::try_from(value + 1).unwrap());
    }
    reads.sort_by_key(|range| range.start);
    let stream = ctx.capture_part_extent(document).unwrap();
    let application = crate::native::native_id("application", "A");
    assert_eq!(
        decoded.source_fidelity().record_layout()[&application]
            .reads()
            .get(stream.stream()),
        Some(&reads)
    );
    let records = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    let record = records
        .iter()
        .find(|record| record["id"] == application)
        .unwrap();
    assert_eq!(
        record["dependencies"],
        serde_json::json!([
            crate::native::native_id("object", "X"),
            crate::native::native_id("object", "Y"),
            crate::native::native_id("object", "Z")
        ])
    );
}

#[test]
fn view_provider_object_reference_reads_its_actual_application_name() {
    let bytes = fixture();
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let document = archive.open(&ctx, "Document.xml").unwrap();
    let text = std::str::from_utf8(document.window()).unwrap();
    let declaration = text.find("<Object name='A' type=").unwrap();
    let key = u64::try_from(declaration + "<Object ".len()).unwrap();
    let value = u64::try_from(declaration + "<Object name='".len()).unwrap();
    let stream = ctx.capture_part_extent(document).unwrap();
    let id = crate::native::native_id("gui-view-provider", "A");
    let layout = &decoded.source_fidelity().record_layout()[&id];
    assert_ne!(layout.stream(), stream.stream());
    assert_eq!(
        layout.reads().get(stream.stream()),
        Some(&vec![key..key + 4, value..value + 1]),
        "the provider's object field comes from the actual independent Name getter"
    );
    let providers = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
        .unwrap();
    assert_eq!(
        providers
            .iter()
            .find(|provider| provider.id == id)
            .unwrap()
            .object
            .as_ref()
            .map(|id| id.as_str()),
        Some(crate::native::native_id("object", "A").as_str())
    );
}

#[test]
fn every_xml_level_keeps_framing_and_independent_peers_under_zero_and_ff_content_damage() {
    assert_damage_matrix("authored", &fixture(), true);
}

fn assert_damage_matrix(name: &str, bytes: &[u8], complete_inventory: bool) {
    let before = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    assert!(!before
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
    crate::test_support::framing::assert_xml_parts_agree(bytes, &before);
    let mut failed = Vec::new();
    let mut cases = 0;
    for selected in selected(bytes, complete_inventory) {
        if selected
            .extent
            .clone()
            .all(|offset| selected.framing.iter().any(|range| range.contains(&offset)))
        {
            // An empty part has no content byte to overwrite. Its exact
            // independent framing and positive decode were checked above.
            assert!(
                !complete_inventory,
                "authored control unexpectedly has no content: {selected:?}"
            );
            continue;
        }
        for replacement in [0, 0xff] {
            cases += 1;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let changed = damage(bytes, &selected, replacement);
                let after = crate::FcstdCodec
                    .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
                    .unwrap_or_else(|error| {
                        panic!("{name}: {selected:?} replacement={replacement:#x}: {error}")
                    });
                let failures = after
                    .source_fidelity()
                    .parts()
                    .iter()
                    .filter(|(_, part)| part.state() == &(PartState::GivenUp {}))
                    .collect::<Vec<_>>();
                assert_eq!(
                failures.len(),
                1,
                "one invalid XML header has one home: {name}: {selected:?} replacement={replacement:#x}"
            );
                assert_eq!(failures[0].1.level().as_str(), selected.level);
                let arena = DecodeArena::new();
                let (ctx, root) =
                    DecodeContext::from_root_bytes(&changed, &arena, &DecodePolicy::service())
                        .unwrap();
                let root = ctx.name_stream(root, "file").unwrap();
                let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
                let member = archive.open(&ctx, &selected.member).unwrap();
                let actual_source = ctx.capture_part_extent(member).unwrap();
                for (id, part) in failures {
                    assert_eq!(part.stream(), actual_source.stream());
                    assert!(
                        part.offset() >= u64::try_from(selected.extent.start).unwrap()
                            && part.offset().checked_add(part.byte_len()).unwrap()
                                <= u64::try_from(selected.extent.end).unwrap(),
                        "failure outside selected content: {selected:?} {part:?}"
                    );
                    let layout = &after.source_fidelity().record_layout()[id.as_str()];
                    assert_eq!(layout.stream(), part.stream());
                    assert_eq!(layout.offset(), part.offset());
                    assert_eq!(layout.byte_len(), part.byte_len());
                }
                crate::test_support::framing::assert_xml_parts_agree(&changed, &after);
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    compare(bytes, &changed, &before, &after);
                }))
                .is_err()
                {
                    let previous = serde_json::to_value(before.ir()).unwrap();
                    let current = serde_json::to_value(after.ir()).unwrap();
                    panic!(
                    "XML conservation failure: {name}: {selected:?} replacement={replacement:#x}; before objects={} providers={} views={}; after objects={} providers={} views={}",
                    previous["native"]["fcstd"]["objects"],
                    previous["native"]["fcstd"]["gui_view_providers"],
                    previous["model"]["view_presentations"],
                    current["native"]["fcstd"]["objects"],
                    current["native"]["fcstd"]["gui_view_providers"],
                    current["model"]["view_presentations"],
                );
                }
            }));
            if result.is_err() {
                failed.push(format!("{selected:?} replacement={replacement:#x}"));
            }
        }
    }
    assert!(cases > 0, "{name}: no content controls were reached");
    assert!(
        failed.is_empty(),
        "{name}: {} of {cases} XML damage controls failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

fn record_ids(decoded: &DecodeResult) -> BTreeSet<String> {
    let mut ids = decoded
        .ir()
        .native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas().values())
        .flatten()
        .map(|record| record.id().to_owned())
        .collect::<BTreeSet<_>>();
    let model = serde_json::to_value(&decoded.ir().model).unwrap();
    for rows in model
        .as_object()
        .unwrap()
        .values()
        .filter_map(serde_json::Value::as_array)
    {
        for row in rows {
            if let Some(id) = row.get("id").and_then(serde_json::Value::as_str) {
                ids.insert(id.to_owned());
            }
        }
    }
    ids
}

fn assert_complete_source_records_survive(
    bytes: &[u8],
    cut: usize,
    before: &DecodeResult,
    after: &DecodeResult,
) {
    let parts = after.source_fidelity().parts();
    if let Some((failed_id, _)) = parts.iter().find(|(_, part)| {
        part.level().as_str() == "zip.archive"
            && part.parent().is_none()
            && part.state() == &(PartState::GivenUp {})
    }) {
        for (id, part) in parts {
            if id == failed_id {
                continue;
            }
            let mut parent = part.parent();
            let mut descendant = false;
            while let Some(ancestor) = parent {
                if ancestor == failed_id {
                    descendant = true;
                    break;
                }
                parent = parts[ancestor].parent();
            }
            assert!(
                descendant,
                "this failed-archive fixture has no independent source root: {id}"
            );
            assert_eq!(
                part.state(),
                &PartState::Covered {
                    by: failed_id.clone()
                },
                "failed archive keeps only covered descendant metadata, never a live child"
            );
        }
        assert!(
            record_ids(before).is_disjoint(&record_ids(after)),
            "failed archive rolls back every formerly held record"
        );
        return;
    }
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let root = ctx.name_stream(root, "file").unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut frames = Vec::new();
    let entries = zip::frame(&ctx, root, &mut frames).unwrap();
    let mut complete = BTreeSet::new();
    for entry in entries {
        if entry.payload().end() > cut {
            continue;
        }
        let name = std::str::from_utf8(entry.name().window()).unwrap();
        let member = archive.open(&ctx, name).unwrap();
        complete.insert(ctx.capture_part_extent(member).unwrap().stream().to_owned());
    }
    let current = record_ids(after);
    for (id, layout) in before.source_fidelity().record_layout() {
        let held = if layout.stream() == "file" {
            layout.offset().checked_add(layout.byte_len()).unwrap() <= u64::try_from(cut).unwrap()
        } else {
            complete.contains(layout.stream())
        };
        if held {
            assert!(
                current.contains(id),
                "independent complete source record lost at cut {cut}: {id}"
            );
        }
    }
}

#[test]
fn xml_archive_half_and_ninety_percent_cuts_keep_every_independent_held_record() {
    assert_archive_cuts("authored", &fixture());
}

fn assert_archive_cuts(name: &str, bytes: &[u8]) {
    let before = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    crate::test_support::framing::assert_xml_parts_agree(bytes, &before);
    let mut failed = Vec::new();
    for percentage in [50, 90] {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let cut = &bytes[..bytes.len().checked_mul(percentage).unwrap() / 100];
            let after = crate::FcstdCodec
                .decode(&mut Cursor::new(cut), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{name} {percentage}% archive cut: {error}"));
            compare(bytes, cut, &before, &after);
            assert_complete_source_records_survive(bytes, cut.len(), &before, &after);
        }));
        if result.is_err() {
            failed.push(percentage);
        }
    }
    assert!(
        failed.is_empty(),
        "{name}: archive cut controls failed: {failed:?}"
    );
}
