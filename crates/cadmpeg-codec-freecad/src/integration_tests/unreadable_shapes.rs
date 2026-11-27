// SPDX-License-Identifier: Apache-2.0
//! An unreadable shape payload drops the Geometry properties that need it and
//! keeps the rest of the document.

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::native::{ObjectRecord, PropertyRecord};
use crate::FcstdCodec;

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/freecad_fcstd/fixtures");
    let mut fixtures = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "FCStd")
        })
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

/// Each `.brp` entry with a nonempty stored payload, and that payload's first
/// byte offset.
fn shape_entries(bytes: &[u8]) -> Vec<(String, usize)> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let snapshot = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    snapshot
        .entries()
        .iter()
        .filter(|entry| {
            std::path::Path::new(&entry.name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("brp"))
        })
        .filter_map(|entry| {
            let range = entry.stored_range().ok()?;
            (range.end > range.start)
                .then(|| (entry.name.clone(), usize::try_from(range.start).unwrap()))
        })
        .collect()
}

fn objects(ir: &cadmpeg_ir::CadIr) -> Vec<ObjectRecord> {
    ir.native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ObjectRecord>("objects")
        .unwrap()
}

fn properties(ir: &cadmpeg_ir::CadIr) -> Vec<PropertyRecord> {
    ir.native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<PropertyRecord>("properties")
        .unwrap()
}

/// The ids in the `dropped=[...]` field of a `record.unreadable` message.
/// The lowercase `file` of a Geometry property's direct payload carrier.
fn payload_root_file(property: &PropertyRecord) -> Option<String> {
    let tag = match property.type_name.as_str() {
        "Part::PropertyPartShape" => "Part",
        "Mesh::PropertyMeshKernel" => "Mesh",
        "Points::PropertyPointKernel" => "Points",
        _ => return None,
    };
    let document = roxmltree::Document::parse(property.xml.text()).ok()?;
    document
        .root_element()
        .children()
        .find(|node| node.has_tag_name(tag))?
        .attribute("file")
        .filter(|file| !file.is_empty())
        .map(str::to_owned)
}

fn dropped_ids(message: &str) -> Vec<String> {
    message
        .split("; dropped=[")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .map(|list| {
            list.split(", ")
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn unreadable_messages(result: &cadmpeg_ir::DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
        .map(|loss| loss.message.clone())
        .collect()
}

fn corrupt(original: &[u8], entries: &[&str]) -> Vec<u8> {
    let payloads = shape_entries(original);
    let mut changed = original.to_vec();
    for entry in entries {
        let (_, payload) = payloads
            .iter()
            .find(|(name, _)| name == entry)
            .expect("the fixture holds the entry");
        changed[*payload] ^= 0xff;
    }
    changed
}

#[test]
fn an_unreadable_shape_entry_drops_only_the_properties_that_need_it() {
    let mut cases = 0;
    for (fixture, original) in fixtures() {
        let Ok(baseline) = crate::test_support::cached_full_decode(&original) else {
            continue;
        };
        let baseline_objects = objects(baseline.ir());
        let baseline_properties = properties(baseline.ir());
        let entries = shape_entries(&original);
        let next = std::sync::atomic::AtomicUsize::new(0);
        cases += std::thread::scope(|scope| {
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        let mut cases = 0;
                        while let Some((entry, payload)) =
                            entries.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                        {
                            let needing = baseline_properties
                                .iter()
                                .filter(|property| {
                                    payload_root_file(property).as_deref() == Some(entry.as_str())
                                })
                                .map(|property| property.id.clone())
                                .collect::<BTreeSet<_>>();
                            let mut changed = original.clone();
                            changed[*payload] ^= 0xff;
                            let context = format!("{fixture} {entry}");
                            let decoded = FcstdCodec
                                .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
                                .unwrap_or_else(|error| panic!("{context}: {error}"));
                            let messages = unreadable_messages(&decoded);
                            let [message] = messages.as_slice() else {
                                panic!("{context}: one record.unreadable loss: {messages:?}");
                            };
                            assert!(
                                message.starts_with(&format!("part archive-entry \"{entry}\"")),
                                "{context}: {message}"
                            );
                            assert_eq!(
                                dropped_ids(message).into_iter().collect::<BTreeSet<_>>(),
                                needing,
                                "{context}"
                            );
                            let kept = properties(decoded.ir());
                            assert_eq!(
                                kept.len() + needing.len(),
                                baseline_properties.len(),
                                "{context}"
                            );
                            assert!(
                                kept.iter().all(|property| !needing.contains(&property.id)),
                                "{context}"
                            );
                            assert_eq!(
                                objects(decoded.ir()).len(),
                                baseline_objects.len(),
                                "{context}"
                            );
                            let findings = crate::test_support::validate_native(decoded.ir());
                            assert!(findings.is_empty(), "{context}: {findings:?}");
                            cases += 1;
                        }
                        cases
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .sum::<usize>()
        });
    }
    assert!(cases > 0, "the corpus fixtures hold shape entries");
}

#[test]
fn the_object_and_its_links_stay_when_its_shape_entry_is_unreadable() {
    let original = crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(corrupt(original, &["Box.Shape.brp"])),
            &DecodeOptions::default(),
        )
        .expect("the rest of the document decodes");
    let messages = unreadable_messages(&decoded);
    let [message] = messages.as_slice() else {
        panic!("one record.unreadable loss: {messages:?}");
    };
    assert_eq!(dropped_ids(message), ["fcstd:native:property#Box:Shape"]);
    assert!(objects(decoded.ir())
        .iter()
        .any(|object| object.id() == "fcstd:native:object#Box"));
    let kept = properties(decoded.ir());
    for id in [
        "fcstd:native:property#Cut:Base",
        "fcstd:native:property#Product:Group",
    ] {
        assert!(kept.iter().any(|property| property.id == id), "{id} kept");
    }
}

#[test]
fn each_unreadable_entry_names_the_property_that_needs_it() {
    let original = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/freecad_fcstd/fixtures/design_history.FCStd"),
    )
    .unwrap();
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(corrupt(
                &original,
                &["Pad.Shape.brp", "Pad.AddSubShape.brp"],
            )),
            &DecodeOptions::default(),
        )
        .expect("the rest of the document decodes");
    let mut named = unreadable_messages(&decoded)
        .iter()
        .map(|message| dropped_ids(message))
        .collect::<Vec<_>>();
    named.sort();
    assert_eq!(
        named,
        [
            vec!["fcstd:native:property#Pad:AddSubShape".to_owned()],
            vec!["fcstd:native:property#Pad:Shape".to_owned()],
        ]
    );
}
