// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

mod readback;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::write::NativeDisposition;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{appearances, Sources, ENTITY_VIEW_ARENAS};
use crate::IgesCodec;
use readback::{Charged, Compared, Facts, GlobalFacts};

/// Golden fixtures that the decoder or the writer refuses, so that no write
/// declares a disposition for them, with the step that refuses.
const EXPECTED_UNSUPPORTED: &[(&str, &str)] = &[];

/// Golden fixtures whose entity facts this test does not compare, with the
/// reason. Their Global facts, entity views, display attribute and colour
/// definition values, and cards are checked.
const UNATTRIBUTED: &[(&str, &str)] = &[(
    "compressed_ascii_5_3",
    "Compressed ASCII holds no fixed-column Directory entry to replace by a null entity",
)];

/// The arenas whose Regenerated records the read-back check compares other
/// than the entity views: the entities by their facts, the display attribute
/// records by their values, and the cards by their entity.
const COMPARED_ARENAS: &[&str] = &["cards", "display_attributes", "entities"];

/// Over every golden fixture, the written file is decoded again and each
/// declaration is checked against it (see [`check`]).
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
        match check(&name, &bytes, &mut failures) {
            Ok(counts) => {
                regenerated += counts.0;
                dropped += counts.1;
            }
            Err(step) => unsupported.push((name, step)),
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

/// Decodes `bytes`, writes it, decodes the written file, and pushes a
/// failure for each declaration the written file does not bear out. Each
/// native entity the write declares Regenerated states at least one fact and
/// reads back with the facts the decoder derives from it; each entity it
/// declares Dropped has a fact that does not read back. The facts of an
/// entity are the geometry and topology records that change when the entity
/// is replaced by a null entity. A fact of a record decoded from a Dropped
/// entity is that entity's loss, and so is a reference to such a record; the
/// rest of the fact is compared. The colour and blank status of a record
/// decoded from an entity whose display attribute record is Dropped are that
/// record's loss. Each Regenerated display attribute and colour definition
/// record reads back with its values. Each appearance and appearance binding
/// the write states reads back with its values, the binding with the colour
/// of its appearance and the record it binds; each it does not state is
/// charged with its reason and does not read back. The written Global section restates
/// the source Global cards, so they are Regenerated, and each source Global
/// fact that does not read back is charged by its field name. Each view of
/// an entity and each of its Directory and Parameter cards follows the
/// entity. Every other arena is Dropped. Returns the numbers of Regenerated
/// and Dropped entities checked, or the step that refuses the source.
fn check(
    name: &str,
    bytes: &[u8],
    failures: &mut Vec<String>,
) -> Result<(usize, usize), &'static str> {
    let checked = readback::write_and_read_back(bytes)?;
    let readback::Checked {
        source,
        native,
        output,
        output_global,
        losses,
        appearances,
    } = &checked;
    let namespace = source.native.namespace("iges").unwrap();
    let arenas = namespace.arenas();
    let of = |arena: &str, id: &str| native.disposition("iges", arena, id);
    for (arena, records) in arenas {
        let arena = arena.as_str();
        if COMPARED_ARENAS.contains(&arena) || ENTITY_VIEW_ARENAS.contains(&arena) {
            continue;
        }
        for record in records {
            if of(arena, record.id()) != NativeDisposition::Dropped {
                failures.push(format!(
                    "{name} {}: {arena} is not compared and is not Dropped",
                    record.id()
                ));
            }
        }
    }
    let mut entity_dispositions = BTreeMap::new();
    let mut charged = Charged::default();
    let mut sequences = Vec::new();
    for record in arenas.get("entities").into_iter().flatten() {
        let disposition = of("entities", record.id());
        entity_dispositions.insert(record.id().to_owned(), disposition);
        let Some(sequence) = super::directory_sequence(record) else {
            failures.push(format!("{name} {}: no Directory sequence", record.id()));
            continue;
        };
        if disposition != NativeDisposition::Regenerated {
            charged.entities.insert(sequence);
        }
        sequences.push((record.id(), disposition, sequence));
    }
    for record in arenas.get("display_attributes").into_iter().flatten() {
        let source_entity = record.field("source_entity").unwrap();
        let source_entity = source_entity.as_str().unwrap();
        if of("display_attributes", record.id()) == NativeDisposition::Regenerated {
            if entity_dispositions.get(source_entity) != Some(&NativeDisposition::Regenerated) {
                failures.push(format!(
                    "{name} {}: Regenerated, its entity is not",
                    record.id()
                ));
            }
        } else if let Some(sequence) = super::entity_sequence(source_entity) {
            charged.displays.insert(sequence);
        }
    }
    let source_facts = Facts::of(source, &charged);
    let output_facts = Facts::of(output, &Charged::default());
    let unattributed = UNATTRIBUTED
        .iter()
        .any(|(unattributed, _)| *unattributed == name);
    let mut owned = Vec::new();
    for (id, disposition, sequence) in sequences {
        if unattributed {
            continue;
        }
        let Some(without) = readback::without_entity(bytes, sequence) else {
            failures.push(format!("{name} {id}: no fixed-column Directory entry"));
            continue;
        };
        owned.push((id, disposition, source_facts.changed_in(&without)));
    }
    let claimed = owned
        .iter()
        .filter(|(_, disposition, _)| *disposition == NativeDisposition::Regenerated)
        .flat_map(|(_, _, facts)| facts.keys())
        .collect::<BTreeSet<_>>();
    let entity_charged = |id: &str| {
        readback::decoded_from(id).is_some_and(|sequence| charged.entities.contains(&sequence))
    };
    let (mut regenerated, mut dropped) = (0_usize, 0_usize);
    for (id, disposition, facts) in &owned {
        if *disposition == NativeDisposition::Regenerated {
            regenerated += 1;
            if facts.is_empty() {
                failures.push(format!("{name} {id} Regenerated, states no fact"));
            }
            let absent = facts
                .iter()
                .filter(|(fact_id, fact)| {
                    !entity_charged(fact_id)
                        && output_facts.count(fact, Compared::Regenerated)
                            < source_facts.count_among(fact, claimed.iter().copied())
                })
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>();
            if !absent.is_empty() {
                failures.push(format!("{name} {id} Regenerated, facts absent: {absent:?}"));
            }
        } else {
            dropped += 1;
            let changed = facts.values().any(|fact| {
                output_facts.count(fact, Compared::Dropped)
                    < source_facts.count(fact, Compared::Dropped)
            });
            if !facts.is_empty() && !changed {
                failures.push(format!(
                    "{name} {id} Dropped, every fact reads back: {:?}",
                    facts.keys().collect::<Vec<_>>()
                ));
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
            let disposition = of(arena, record.id());
            // A colour definition loses its Directory colour number while its
            // entity is written when the number is not 0.
            let follows = disposition == *owner
                || (*arena == "colors" && disposition == NativeDisposition::Dropped);
            if !follows {
                failures.push(format!("{name} {} differs from its entity", record.id()));
            }
        }
    }
    for (arena, written, stated) in [
        (
            "display_attributes",
            readback::display_values(output),
            readback::display_values(source),
        ),
        (
            "colors",
            readback::color_values(output),
            readback::color_values(source),
        ),
    ] {
        for (id, value) in stated {
            if of(arena, &id) == NativeDisposition::Regenerated
                && !readback::holds(written.values(), &value)
            {
                failures.push(format!(
                    "{name} {id} Regenerated, its values do not read back: {value}"
                ));
            }
        }
    }
    for (arena, drops, compared, stated, written) in [
        (
            "appearances",
            &appearances.appearances,
            readback::appearance_values(source),
            readback::appearance_values(source),
            readback::appearance_values(output),
        ),
        (
            "appearance_bindings",
            &appearances.bindings,
            readback::binding_values(source, &charged),
            readback::binding_values(source, &Charged::default()),
            readback::binding_values(output, &Charged::default()),
        ),
    ] {
        let kept = compared
            .iter()
            .filter(|(id, _)| !drops.contains_key(*id))
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        for (id, value) in &compared {
            let Some(reason) = drops.get(id) else {
                if readback::tally(written.values(), value)
                    < readback::tally(kept.iter().copied(), value)
                {
                    failures.push(format!(
                        "{name} {arena} {id} written, does not read back: {value}"
                    ));
                }
                continue;
            };
            let value = &stated[id];
            if readback::tally(written.values(), value) >= readback::tally(stated.values(), value) {
                failures.push(format!(
                    "{name} {arena} {id} charged {reason:?}, reads back: {value}"
                ));
            }
            let charges = |loss: &String| {
                loss.contains(reason)
                    && loss
                        .rsplit_once(": ")
                        .is_some_and(|(_, ids)| ids.split(", ").any(|charged| charged == id))
            };
            if !losses.iter().any(charges) {
                failures.push(format!("{name} {arena} {id} has no loss for {reason:?}"));
            }
        }
    }
    if let Some(source) = GlobalFacts::of(source) {
        match output_global {
            None => failures.push(format!(
                "{name}: the written Global section does not resolve"
            )),
            Some(output) => {
                for names in source.unrestated_in(output) {
                    let charge = |field: &&str| {
                        let prefix = format!("IGES Global {field} ");
                        losses.iter().any(|loss| loss.starts_with(&prefix))
                    };
                    if !names.iter().any(charge) {
                        failures.push(format!("{name}: Global {names:?} changes uncharged"));
                    }
                }
            }
        }
    }
    for card in arenas.get("cards").into_iter().flatten() {
        let section = card.field("section").unwrap();
        let section = section.as_str().unwrap();
        let sequence = card.field("sequence").and_then(|value| value.as_u64());
        let expected = match (section, sequence) {
            ("global" | "terminate", _) => NativeDisposition::Regenerated,
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
    Ok((regenerated, dropped))
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

/// A shell whose Directory colour names a Type 314 with a name, and whose
/// Type 406 Form 15 names its body. The face names the same Type 314.
const NAMED_BODY: &str = concat!(
    "original fixture                                                        S      1\n",
    "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMG      1\n",
    "M,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;      G      2\n",
    "     116       1       0       0       0       0       0       000010000D      1\n",
    "     116       0       0       1       0                  CENTER       0D      2\n",
    "     196       2       0       0       0       0       0       000010000D      3\n",
    "     196       0       0       1       0                  SPHERE       0D      4\n",
    "     502       3       0       0       0       0       0       000010000D      5\n",
    "     502       0       0       1       1                    POLE       0D      6\n",
    "     508       4       0       0       0       0       0       000010000D      7\n",
    "     508       0       0       1       1                   VLOOP       0D      8\n",
    "     510       5       0       0       0       0       0       000010000D      9\n",
    "     510       0     -13       1       1                    FACE       0D     10\n",
    "     514       6       0       0       0       0       0       000000000D     11\n",
    "     514       0     -13       1       2                   SHELL       0D     12\n",
    "     314       7       0       0       0       0       0       000000200D     13\n",
    "     314       0       0       1       0                   COLOR       0D     14\n",
    "     406       8       0       0       0       0       0       000010000D     15\n",
    "     406       0       0       1      15                    NAME       0D     16\n",
    "116,0,0,0,0;                                                           1P      1\n",
    "196,1,1;                                                               3P      2\n",
    "502,1,0,0,1;                                                           5P      3\n",
    "508,1,1,5,1,0,0;                                                       7P      4\n",
    "510,3,1,1,7;                                                           9P      5\n",
    "514,1,9,1,0,1,15;                                                     11P      6\n",
    "314,20,40,60,6Hcustom;                                                13P      7\n",
    "406,1,7HBRACKET;                                                      15P      8\n",
    "S0000001G0000002D0000016P0000008                                        T      1\n",
);

/// The write states the body and the face colour as one Type 314 with the
/// source colour and name, and the body name as a Type 406 Form 15, so the
/// Type 314, its colour definition, the Type 406, and the display attributes
/// of the shell, the face, and the Type 314 are Regenerated and read back,
/// and so are the appearance and both bindings.
#[test]
fn a_named_body_colour_and_a_body_name_read_back() {
    let mut failures = Vec::new();
    check("named_body", NAMED_BODY.as_bytes(), &mut failures).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(NAMED_BODY.as_bytes())
        .ok()
        .unwrap();
    let native = &checked.native;
    for (arena, id, disposition) in [
        (
            "entities",
            "iges:entity:directory#13",
            NativeDisposition::Regenerated,
        ),
        (
            "entities",
            "iges:entity:directory#15",
            NativeDisposition::Regenerated,
        ),
        (
            "colors",
            "iges:presentation:color#D13",
            NativeDisposition::Regenerated,
        ),
        (
            "display_attributes",
            "iges:presentation:display-attributes#D11",
            NativeDisposition::Regenerated,
        ),
        (
            "display_attributes",
            "iges:presentation:display-attributes#D13",
            NativeDisposition::Regenerated,
        ),
        (
            "display_attributes",
            "iges:presentation:display-attributes#D9",
            NativeDisposition::Regenerated,
        ),
    ] {
        assert_eq!(native.disposition("iges", arena, id), disposition, "{id}");
    }
    assert!(
        checked.appearances.appearances.is_empty() && checked.appearances.bindings.is_empty(),
        "{:?} {:?}",
        checked.appearances.appearances,
        checked.appearances.bindings
    );
    assert_eq!(checked.source.model.appearance_bindings.len(), 2);
    assert_eq!(
        checked
            .output
            .native
            .namespace("iges")
            .unwrap()
            .arenas()
            .get("entities")
            .into_iter()
            .flatten()
            .filter(|record| super::entity_type(record) == Some(314))
            .count(),
        1
    );
    let global = crate::writer::source_global::source_global_bytes(&checked.output).unwrap();
    assert!(
        matches!(
            crate::global::source_fields(&global).unwrap().get(19),
            Some(crate::global::SourceField::Real(maximum)) if maximum.get() == 1.0
        ),
        "the Type 314 has no coordinates, so the Global maximum coordinate is the vertex at 1"
    );
}

/// A trimmed surface whose one Directory colour field states standard colour
/// 2, which binds both the sheet body and its face.
const RED_TRIMMED_PLANE: &str = concat!(
    "original fixture                                                        S      1\n",
    "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMG      1\n",
    "M,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;      G      2\n",
    "     108       1       0       0       0       0       0       000010000D      1\n",
    "     108       0       0       1       0                   PLANE       0D      2\n",
    "     106       2       0       0       0       0       0       000010000D      3\n",
    "     106       0       0       1      63                   MODEL       0D      4\n",
    "     106       3       0       0       0       0       0       000010500D      5\n",
    "     106       0       0       1      63                  PCURVE       0D      6\n",
    "     142       4       0       0       0       0       0       000010000D      7\n",
    "     142       0       0       1       0                 ON_SURF       0D      8\n",
    "     144       5       0       0       0       0       0       000000000D      9\n",
    "     144       0       2       1       0                 TRIMMED       0D     10\n",
    "108,0,0,1,0,0,0,0,0,0;                                                 1P      1\n",
    "106,1,5,0,0,0,1,0,1,1,0,1,0,0;                                         3P      2\n",
    "106,1,5,0,0,0,1,0,1,1,0,1,0,0;                                         5P      3\n",
    "142,0,1,5,3,3;                                                         7P      4\n",
    "144,1,1,0,7;                                                           9P      5\n",
    "S0000001G0000002D0000010P0000005                                        T      1\n",
);

/// The write states the colour of the sheet body and of its face in the
/// Directory entries it writes, so the standard appearance and both bindings
/// read back.
#[test]
fn a_trimmed_surface_colour_binds_the_body_and_the_face() {
    let mut failures = Vec::new();
    check(
        "red_trimmed_plane",
        RED_TRIMMED_PLANE.as_bytes(),
        &mut failures,
    )
    .unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(RED_TRIMMED_PLANE.as_bytes())
        .ok()
        .unwrap();
    assert_eq!(
        checked
            .source
            .model
            .appearance_bindings
            .iter()
            .map(|binding| binding.id.as_str())
            .collect::<Vec<_>>(),
        [
            "iges:model:appearance-binding#body-D9",
            "iges:model:appearance-binding#face-D9"
        ]
    );
    assert!(
        checked.appearances.appearances.is_empty() && checked.appearances.bindings.is_empty(),
        "{:?} {:?}",
        checked.appearances.appearances,
        checked.appearances.bindings
    );
}

/// A free line whose Directory colour states standard colour 2.
const RED_LINE: &str = concat!(
    "original fixture                                                        S      1\n",
    "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMG      1\n",
    "M,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;      G      2\n",
    "     110       1       0       0       0       0       0       000000000D      1\n",
    "     110       0       2       1       0                    LINE       0D      2\n",
    "110,0,0,0,1,0,0;                                                       1P      1\n",
    "S0000001G0000002D0000002P0000001                                        T      1\n",
);

/// The write states the colour of bodies and faces only, so it charges the
/// binding of the line and the appearance that binding names, each with its
/// reason.
#[test]
fn a_curve_colour_is_charged_with_its_appearance() {
    let mut failures = Vec::new();
    check("red_line", RED_LINE.as_bytes(), &mut failures).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(RED_LINE.as_bytes())
        .ok()
        .unwrap();
    let bindings = checked
        .source
        .model
        .appearance_bindings
        .iter()
        .map(|binding| (binding.id.as_str().to_owned(), appearances::BINDING_TARGET))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(bindings.len(), 1);
    assert_eq!(checked.appearances.bindings, bindings);
    assert_eq!(
        checked.appearances.appearances,
        BTreeMap::from([(
            "iges:appearance:standard#2".to_owned(),
            appearances::APPEARANCE_UNUSED
        )])
    );
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
