// SPDX-License-Identifier: Apache-2.0
//! An unreadable shape payload drops the objects that need it and keeps the
//! rest of the document.

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

#[test]
fn an_unreadable_shape_entry_drops_only_the_objects_that_need_it() {
    let mut cases = 0;
    for (fixture, original) in fixtures() {
        let Ok(baseline) =
            FcstdCodec.decode(&mut Cursor::new(&original), &DecodeOptions::default())
        else {
            continue;
        };
        let baseline_objects = objects(baseline.ir());
        let baseline_properties = properties(baseline.ir());
        for (entry, payload) in shape_entries(&original) {
            let owners = baseline_properties
                .iter()
                .filter(|property| property.family == crate::native::PropertyFamily::Geometry)
                .filter(|property| property.side_entries().contains(&entry))
                .map(|property| property.owner.clone())
                .filter(|owner| baseline_objects.iter().any(|object| object.id() == owner))
                .collect::<BTreeSet<_>>();
            let mut changed = original.clone();
            changed[payload] ^= 0xff;
            let context = format!("{fixture} {entry}");
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{context}: {error}"));
            let unreadable = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
                .map(|loss| loss.message.clone())
                .collect::<Vec<_>>();
            let [message] = unreadable.as_slice() else {
                panic!("{context}: one record.unreadable loss: {unreadable:?}");
            };
            assert!(
                message.starts_with(&format!("part archive-entry \"{entry}\"")),
                "{context}: {message}"
            );
            let kept = objects(decoded.ir());
            for owner in &owners {
                assert!(
                    !kept.iter().any(|object| object.id() == owner),
                    "{context}: {owner} kept"
                );
                assert!(
                    message.contains(owner.as_str()),
                    "{context}: {owner} not named in {message}"
                );
            }
            assert_eq!(
                kept.len() + owners.len(),
                baseline_objects.len(),
                "{context}"
            );
            let findings = crate::test_support::validate_native(decoded.ir());
            assert!(findings.is_empty(), "{context}: {findings:?}");
            cases += 1;
        }
    }
    assert!(cases > 0, "the corpus fixtures hold shape entries");
}

#[test]
fn single_links_to_a_dropped_object_drop_and_list_links_lose_the_entry() {
    let original = crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
    let (_, payload) = shape_entries(original)
        .into_iter()
        .find(|(name, _)| name == "Box.Shape.brp")
        .expect("Box has a shape entry");
    let mut changed = original.to_vec();
    changed[payload] ^= 0xff;
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
        .expect("the rest of the document decodes");
    let message = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.to_string() == "decode/record.unreadable")
        .expect("one record.unreadable loss")
        .message
        .clone();
    let dropped = message
        .split("; dropped=[")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("dropped list");
    for id in [
        "fcstd:native:object#Box",
        "fcstd:native:property#Box:Shape",
        "fcstd:native:property#Cut:Base",
    ] {
        assert!(dropped.contains(id), "{id} not dropped: {message}");
    }
    assert!(!dropped.contains("fcstd:native:object#Cut"), "{message}");
    assert!(
        message.contains("; lost=[")
            && message.contains("fcstd:native:property#Product:Group link"),
        "{message}"
    );
    let kept = properties(decoded.ir());
    assert!(kept
        .iter()
        .any(|property| property.id == "fcstd:native:property#Product:Group"));
    assert!(!kept
        .iter()
        .any(|property| property.id == "fcstd:native:property#Cut:Base"));
}
