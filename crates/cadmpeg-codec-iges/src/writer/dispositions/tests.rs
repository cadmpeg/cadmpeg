// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

mod readback;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{Sources, ENTITY_VIEW_ARENAS};
use crate::IgesCodec;
use readback::{Facts, GlobalFacts};

/// Golden fixtures that the decoder or the writer refuses, so that no write
/// declares a disposition for them, with the step that refuses.
const EXPECTED_UNSUPPORTED: &[(&str, &str)] = &[];

/// Golden fixtures whose entity facts this test does not compare, with the
/// reason. Their Global cards, entity views, and cards are checked.
const UNATTRIBUTED: &[(&str, &str)] = &[
    (
        "compressed_ascii_5_3",
        "Compressed ASCII holds no fixed-column Directory entry to replace by a null entity",
    ),
    (
        "explicit_open_shell",
        "the trimmed-sheet path writes the face's four line edges as one composite boundary \
         curve, which a record-by-record comparison does not match to its spans",
    ),
];

/// Over every golden fixture, the written file is decoded again. Each native
/// entity the write declares Regenerated reads back with the facts the
/// decoder derives from it, and each entity it declares Dropped has a fact
/// that does not read back. The facts of an entity are the geometry and
/// topology records that change when the entity is replaced by a null
/// entity. The Global cards are Regenerated exactly when the Global facts
/// the source states read back equal. Each view of an entity and each of its
/// Directory and Parameter cards follows the entity.
#[test]
fn every_golden_declaration_reads_back_from_the_written_file() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");
    let mut paths = std::fs::read_dir(&fixtures)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "igs"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut failures = Vec::new();
    let mut unsupported = Vec::new();
    let (mut regenerated, mut dropped) = (0_usize, 0_usize);
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let checked = match readback::write_and_read_back(&bytes) {
            Ok(checked) => checked,
            Err(step) => {
                unsupported.push((name, step));
                continue;
            }
        };
        let readback::Checked {
            source,
            native,
            output,
            output_global,
        } = &checked;
        let namespace = source.native.namespace("iges").unwrap();
        let arenas = namespace.arenas();
        let of = |arena: &str, id: &str| native.disposition("iges", arena, id);
        let source_facts = Facts::of(source);
        let output_facts = Facts::of(output);
        let mut entity_dispositions = BTreeMap::new();
        let mut owned = Vec::new();
        for record in arenas.get("entities").into_iter().flatten() {
            let disposition = of("entities", record.id());
            entity_dispositions.insert(record.id().to_owned(), disposition);
            let Some(sequence) = super::directory_sequence(record) else {
                failures.push(format!("{name} {}: no Directory sequence", record.id()));
                continue;
            };
            if UNATTRIBUTED
                .iter()
                .any(|(unattributed, _)| *unattributed == name)
            {
                continue;
            }
            let Some(without) = readback::without_entity(&bytes, sequence) else {
                failures.push(format!(
                    "{name} {}: no fixed-column Directory entry",
                    record.id()
                ));
                continue;
            };
            owned.push((record.id(), disposition, source_facts.changed_in(&without)));
        }
        let claimed = owned
            .iter()
            .filter(|(_, disposition, _)| *disposition == NativeDisposition::Regenerated)
            .flat_map(|(_, _, facts)| facts.keys())
            .collect::<BTreeSet<_>>();
        for (id, disposition, facts) in &owned {
            match disposition {
                NativeDisposition::Regenerated => {
                    regenerated += 1;
                    let absent = facts
                        .iter()
                        .filter(|(_, fact)| {
                            output_facts.count(fact)
                                < source_facts.count_among(fact, claimed.iter().copied())
                        })
                        .map(|(id, _)| id.as_str())
                        .collect::<Vec<_>>();
                    if !absent.is_empty() {
                        failures.push(format!("{name} {id} Regenerated, facts absent: {absent:?}"));
                    }
                }
                _ => {
                    dropped += 1;
                    let changed = facts
                        .values()
                        .any(|fact| output_facts.count(fact) < source_facts.count(fact));
                    if !facts.is_empty() && !changed {
                        failures.push(format!(
                            "{name} {id} Dropped, every fact reads back: {:?}",
                            facts.keys().collect::<Vec<_>>()
                        ));
                    }
                }
            }
        }
        for arena in ENTITY_VIEW_ARENAS {
            for record in arenas.get(*arena).into_iter().flatten() {
                let Some(source) = record.field("source_entity") else {
                    continue;
                };
                let Some(owner) = entity_dispositions.get(source.as_str().unwrap()) else {
                    continue;
                };
                if of(arena, record.id()) != *owner {
                    failures.push(format!("{name} {} differs from its entity", record.id()));
                }
            }
        }
        let global = match (GlobalFacts::of(source), output_global) {
            (Some(source), Some(output)) if source.read_back_in(output) => {
                NativeDisposition::Regenerated
            }
            _ => NativeDisposition::Dropped,
        };
        for card in arenas.get("cards").into_iter().flatten() {
            let section = card.field("section").unwrap();
            let section = section.as_str().unwrap();
            let sequence = card.field("sequence").and_then(|value| value.as_u64());
            let expected = match (section, sequence) {
                ("global", _) => global,
                ("terminate", _) => NativeDisposition::Regenerated,
                ("directory" | "parameter", Some(sequence)) => {
                    match card_owner(namespace, section, sequence) {
                        Some(owner) => entity_dispositions[&owner],
                        None => NativeDisposition::Dropped,
                    }
                }
                _ => NativeDisposition::Dropped,
            };
            if of("cards", card.id()) != expected {
                failures.push(format!("{name} {}: expected {expected:?}", card.id()));
            }
        }
    }
    let unexpected = unsupported
        .iter()
        .filter(|(name, step)| !EXPECTED_UNSUPPORTED.contains(&(name.as_str(), *step)))
        .collect::<Vec<_>>();
    let missing = EXPECTED_UNSUPPORTED
        .iter()
        .filter(|(name, step)| {
            !unsupported
                .iter()
                .any(|(other, other_step)| other == name && other_step == step)
        })
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty() && unexpected.is_empty() && missing.is_empty(),
        "{failures:#?}\nunexpected refusals {unexpected:?}\nlisted refusals that pass {missing:?}"
    );
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

/// A point or vertex the decoder derives at an end of a curve states that
/// end, and not the curve.
#[test]
fn a_curve_endpoint_does_not_resolve_to_the_curve() {
    let file = crate::test_support::test_owned::owned_test_file(&[
        crate::test_support::test_owned::OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "LINE".into(),
            status: "00000000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
    ]);
    let ir = IgesCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone();
    let sources = Sources::new(&ir, ir.native.namespace("iges").unwrap());
    let curve = &ir.geometry.curves[0].id;
    assert_eq!(sources.resolve(curve.as_str()), Some(1));
    for id in [
        "iges:model:point#D1-start",
        "iges:model:point#D1-end",
        "iges:model:vertex#D1-start",
        "iges:model:vertex#D1:end",
    ] {
        assert_eq!(sources.resolve(id), None, "{id}");
    }
}
