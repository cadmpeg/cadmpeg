// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{directory_sequence, Held, Sources, ENTITY_VIEW_ARENAS};
use crate::writer::source_global::GlobalRestatement;
use crate::{IgesCodec, IgesVersion};

/// Over every golden fixture the semantic writer encodes, each native entity
/// is declared Regenerated exactly when an emitted entity states it, each
/// view of an entity and each of its Directory and Parameter cards follows
/// the entity, and the Global cards are Regenerated exactly when the writer
/// restates every source Global field.
#[test]
fn every_golden_declaration_agrees_with_the_emitted_entities() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");
    let mut paths = std::fs::read_dir(&fixtures)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "igs"))
        .collect::<Vec<_>>();
    paths.sort();
    let (mut regenerated, mut dropped) = (0_usize, 0_usize);
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let Ok(decoded) = IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        else {
            continue;
        };
        let recovered =
            cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(decoded.ir()).unwrap();
        let document = &recovered.document;
        let Ok((synthesis, native)) =
            crate::writer::target::synthesis_and_dispositions(document, IgesVersion::V5_3)
        else {
            continue;
        };
        let Some(namespace) = document.native.namespace("iges") else {
            continue;
        };
        let sources = Sources::new(document, namespace);
        let held = Held::of(&sources, &synthesis.entities);
        let arenas = namespace.arenas();
        let of = |arena: &str, id: &str| native.disposition("iges", arena, id);
        let mut entity_dispositions = std::collections::BTreeMap::new();
        for record in arenas.get("entities").into_iter().flatten() {
            let disposition = of("entities", record.id());
            let expected = match directory_sequence(record) {
                Some(sequence) if held.holds(sequence) => NativeDisposition::Regenerated,
                _ => NativeDisposition::Dropped,
            };
            assert_eq!(disposition, expected, "{name} {}", record.id());
            match disposition {
                NativeDisposition::Regenerated => regenerated += 1,
                _ => dropped += 1,
            }
            entity_dispositions.insert(record.id().to_owned(), disposition);
        }
        for arena in ENTITY_VIEW_ARENAS {
            for record in arenas.get(*arena).into_iter().flatten() {
                let Some(source) = record.field("source_entity") else {
                    continue;
                };
                let Some(owner) = entity_dispositions.get(source.as_str().unwrap()) else {
                    continue;
                };
                assert_eq!(of(arena, record.id()), *owner, "{name} {}", record.id());
            }
        }
        let global = GlobalRestatement::of(document, IgesVersion::V5_3);
        for card in arenas.get("cards").into_iter().flatten() {
            let section = card.field("section").unwrap();
            let sequence = card.field("sequence").and_then(|value| value.as_u64());
            let expected = match (section.as_str().unwrap(), sequence) {
                ("global", _) if global.drop_reason().is_none() => NativeDisposition::Regenerated,
                ("terminate", _) => NativeDisposition::Regenerated,
                ("directory" | "parameter", Some(sequence)) => {
                    match card_owner(namespace, section.as_str().unwrap(), sequence) {
                        Some(owner) => entity_dispositions[&owner],
                        None => NativeDisposition::Dropped,
                    }
                }
                _ => NativeDisposition::Dropped,
            };
            assert_eq!(of("cards", card.id()), expected, "{name} {}", card.id());
        }
    }
    assert!(regenerated > 0 && dropped > 0, "{regenerated} {dropped}");
}

/// The entity record whose Directory entry or Parameter Data holds the card
/// `sequence` of `section`.
fn card_owner(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    section: &str,
    sequence: u64,
) -> Option<String> {
    namespace
        .arenas()
        .get("entities")?
        .iter()
        .find_map(|record| {
            let field = |name: &str| record.field(name).and_then(|value| value.as_u64());
            let (start, count) = match section {
                "directory" => (field("directory_sequence")?, 2),
                _ => (field("parameter_start")?, field("parameter_line_count")?),
            };
            (start..start.checked_add(count)?)
                .contains(&sequence)
                .then(|| record.id().to_owned())
        })
}
