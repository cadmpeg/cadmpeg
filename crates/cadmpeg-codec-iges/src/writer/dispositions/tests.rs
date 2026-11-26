// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

mod readback;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder, NativeDisposition};
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
/// native entity the write declares Regenerated states at least one fact, at
/// least one of its facts reads back, and each of its facts that does not
/// read back is the loss of another disposition; each entity it declares
/// Dropped has a fact that does not read back. The facts of an entity are the
/// geometry and topology records that change when the entity is replaced by a
/// null entity. A fact of a record decoded from a Dropped entity is that
/// entity's loss, and so is a reference to such a record; the rest of the
/// fact is compared. The colour and blank status of a record decoded from an
/// entity whose display attribute record is Dropped are that record's loss.
/// Each Regenerated display attribute and colour definition record reads back
/// with its values, and each that is Dropped while its entity is Regenerated
/// reads back fewer times than the source states it. Each appearance and
/// appearance binding the write states reads back with its values, the
/// binding with the colour of its appearance and the record it binds; each it
/// does not state is charged with its reason and does not read back. The
/// written Global section restates the source Global cards, so they are
/// Regenerated, and each source Global fact that does not read back is
/// charged by its field name. Each view of an entity, each of its Directory
/// and Parameter cards, and its colour definition record follow the entity.
/// Every other arena is Dropped. Returns the numbers of Regenerated and
/// Dropped entities checked, or the step that refuses the source.
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
                continue;
            }
            let absent = facts
                .iter()
                .filter(|(_, fact)| {
                    output_facts.count(fact, Compared::Regenerated)
                        < source_facts.count_among(fact, claimed.iter().copied())
                })
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>();
            if absent.len() == facts.len() {
                failures.push(format!(
                    "{name} {id} Regenerated, no fact reads back: {absent:?}"
                ));
            }
            let absent = absent
                .into_iter()
                .filter(|fact_id| !entity_charged(fact_id))
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
            if of(arena, record.id()) != *owner {
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
        let entity_written = |id: &str| {
            arenas
                .get(arena)
                .into_iter()
                .flatten()
                .find(|record| record.id() == id)
                .and_then(|record| record.field("source_entity"))
                .and_then(|source| entity_dispositions.get(source.as_str()?).copied())
                == Some(NativeDisposition::Regenerated)
        };
        for (id, value) in &stated {
            match of(arena, id) {
                NativeDisposition::Regenerated if !readback::holds(written.values(), value) => {
                    failures.push(format!(
                        "{name} {id} Regenerated, its values do not read back: {value}"
                    ));
                }
                NativeDisposition::Dropped
                    if entity_written(id)
                        && readback::tally(written.values(), value)
                            >= readback::tally(stated.values(), value) =>
                {
                    failures.push(format!(
                        "{name} {id} Dropped while its entity is Regenerated, its values read \
                         back: {value}"
                    ));
                }
                _ => {}
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

/// [`NAMED_BODY`] with the Directory colour number of its Type 314 set to
/// standard colour 3 and its level set to `level`.
fn numbered_color(level: u32) -> String {
    NAMED_BODY
        .replace(
            "     314       0       0       1       0 ",
            "     314       0       3       1       0 ",
        )
        .replace(
            "     314       7       0       0       0 ",
            &format!("     314       7       0       0{level:>8} "),
        )
}

/// The decode of `file` with the colour of its Type 314 changed after the
/// decode on the appearance and on the body and the face it colours.
fn recolored(file: &str) -> cadmpeg_ir::CadIr {
    let mut ir = IgesCodec
        .decode(&mut Cursor::new(file.as_bytes()), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone();
    let edited = cadmpeg_ir::topology::Color::new(0.3, 0.4, 0.6, 1.0);
    let appearance = &mut ir.model.appearances[0];
    let source = appearance.base_color;
    appearance.base_color = edited;
    for body in &mut ir.geometry.bodies {
        if body.color == source {
            body.color = edited;
        }
    }
    for face in &mut ir.geometry.faces {
        if face.color == source {
            face.color = edited;
        }
    }
    ir
}

/// The messages that name `id` as a whole token.
fn naming<'a>(messages: impl IntoIterator<Item = &'a String>, id: &str) -> Vec<String> {
    messages
        .into_iter()
        .filter(|message| {
            message
                .split_whitespace()
                .any(|token| token.trim_end_matches([',', ':', ';']) == id)
        })
        .cloned()
        .collect()
}

/// The file `ir` is written as at IGES 5.3, its declarations, the messages
/// of its losses, and those of them that name `id`.
fn charges_naming(
    ir: &cadmpeg_ir::CadIr,
    id: &str,
) -> (
    Vec<u8>,
    cadmpeg_ir::codec::write::NativeDispositions,
    Vec<String>,
    Vec<String>,
) {
    let (_, native) =
        crate::writer::target::synthesis_and_dispositions(ir, crate::IgesVersion::V5_3)
            .ok()
            .unwrap();
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(crate::IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .unwrap();
    let messages = plan
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    let charges = naming(&messages, id);
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    (bytes, native, messages, charges)
}

/// The fallback colour number of each colour definition record of `ir`.
fn fallback_numbers(ir: &cadmpeg_ir::CadIr) -> Vec<Option<i64>> {
    ir.native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("colors"))
        .into_iter()
        .flatten()
        .map(|record| {
            record
                .field("fallback_color_number")
                .and_then(|value| value.as_i64())
        })
        .collect()
}

/// The Directory colour number of a Type 314 is the standard colour a
/// receiver uses in place of the definition, and its colour definition
/// record states it as its fallback colour. The written Type 314 restates the
/// source Type 314 with the source red, green, and blue, so it states the
/// source colour number, and both records are Regenerated and read back.
#[test]
fn a_source_colour_number_is_written_on_the_restated_colour_definition() {
    let file = numbered_color(0);
    let mut failures = Vec::new();
    check("numbered_color", file.as_bytes(), &mut failures).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(file.as_bytes()).ok().unwrap();
    for (arena, id) in [
        ("colors", "iges:presentation:color#D13"),
        (
            "display_attributes",
            "iges:presentation:display-attributes#D13",
        ),
    ] {
        assert_eq!(
            checked.native.disposition("iges", arena, id),
            NativeDisposition::Regenerated,
            "{id}"
        );
        assert!(
            naming(&checked.losses, id).is_empty(),
            "{:#?}",
            checked.losses
        );
    }
    assert_eq!(fallback_numbers(&checked.output), [Some(3)]);
}

/// Where the colour was changed after the decode, the written Type 314 states
/// another red, green, and blue than its source colour definition record, so
/// the writer writes colour number 0 there, and one charge names the display
/// attribute record and the colour definition record.
#[test]
fn an_edited_colour_definition_is_charged_for_its_colour_number() {
    let ir = recolored(&numbered_color(0));
    let (bytes, native, messages, charges) =
        charges_naming(&ir, "iges:presentation:display-attributes#D13");
    assert_eq!(
        native.disposition(
            "iges",
            "display_attributes",
            "iges:presentation:display-attributes#D13"
        ),
        NativeDisposition::Dropped
    );
    assert_eq!(charges.len(), 1, "{messages:#?}");
    assert!(
        charges[0].contains("Directory colour numbers of colour definitions")
            && charges[0].contains("iges:presentation:color#D13 "),
        "{charges:#?}"
    );
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone();
    assert_eq!(fallback_numbers(&output), [Some(0)]);
}

/// The red, green, and blue percentages of each colour definition record of
/// `ir`.
fn percents(ir: &cadmpeg_ir::CadIr) -> Vec<[Option<f64>; 3]> {
    ir.native
        .namespace("iges")
        .and_then(|namespace| namespace.arenas().get("colors"))
        .into_iter()
        .flatten()
        .map(|record| {
            ["red_percent", "green_percent", "blue_percent"]
                .map(|field| record.field(field).and_then(|value| value.as_f64()))
        })
        .collect()
}

/// A Type 314 states percentages whose hundredth times 100 is not the
/// percentage in binary floating point: `0.29 * 100` is 28.999999999999996.
/// The written Type 314 restates the source Type 314 and writes its
/// percentages, so its colour definition record is Regenerated with the
/// source values.
#[test]
fn source_colour_percentages_are_written_unchanged() {
    let file = NAMED_BODY.replace("314,20,40,60,6Hcustom;", "314,29,57,58,6Hcustom;");
    let mut failures = Vec::new();
    check("inexact_percents", file.as_bytes(), &mut failures).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(file.as_bytes()).ok().unwrap();
    assert_eq!(
        checked
            .native
            .disposition("iges", "colors", "iges:presentation:color#D13"),
        NativeDisposition::Regenerated
    );
    assert_eq!(
        percents(&checked.output),
        [[Some(29.0), Some(57.0), Some(58.0)]]
    );
}

/// A colour changed after the decode to 0.3, 0.4, and 0.6 has no source
/// percentages, and the written Type 314 states 30, 40, and 60, the
/// percentages that read back as that colour, not `0.3 * 100`,
/// 30.000000000000004.
#[test]
fn an_edited_colour_is_written_as_the_shortest_percentages_that_read_back() {
    let ir = recolored(NAMED_BODY);
    let (bytes, _, _, _) = charges_naming(&ir, "iges:presentation:color#D13");
    let output = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone();
    assert_eq!(percents(&output), [[Some(30.0), Some(40.0), Some(60.0)]]);
}

/// A Type 314 states a level that the writer writes as 0. The one charge of
/// its display attribute record names the level field group, with the colour
/// number where the writer writes 0 in place of it.
#[test]
fn a_colour_definition_level_is_charged_whatever_its_colour_number() {
    let file = numbered_color(5);
    let decoded = IgesCodec
        .decode(&mut Cursor::new(file.as_bytes()), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone();
    for (ir, colour_number) in [(decoded, false), (recolored(&file), true)] {
        let (_, _, messages, charges) =
            charges_naming(&ir, "iges:presentation:display-attributes#D13");
        assert_eq!(charges.len(), 1, "{messages:#?}");
        assert!(charges[0].contains(super::DISPLAY_FIELDS), "{charges:#?}");
        assert_eq!(
            charges[0].contains("Directory colour numbers of colour definitions"),
            colour_number,
            "{charges:#?}"
        );
    }
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
        .map(|binding| {
            (
                binding.id.as_str().to_owned(),
                appearances::BINDING_TARGET.to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(bindings.len(), 1);
    assert_eq!(checked.appearances.bindings, bindings);
    assert_eq!(
        checked.appearances.appearances,
        BTreeMap::from([(
            "iges:appearance:standard#2".to_owned(),
            appearances::APPEARANCE_UNUSED.to_owned()
        )])
    );
}

/// An open shell of one face that the write states as a trimmed surface. A
/// Type 406 Form 15 names the face and another names the shell, whose name
/// the decoder takes as the body name.
const FACE_AND_BODY_NAMES: &str = concat!(
    "original fixture                                                        S      1\n",
    "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMG      1\n",
    "M,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;      G      2\n",
    "     116       1       0       0       0       0       0       000010000D      1\n",
    "     116       0       0       1       0                LOCATION       0D      2\n",
    "     123       2       0       0       0       0       0       000010000D      3\n",
    "     123       0       0       1       0                  NORMAL       0D      4\n",
    "     190       3       0       0       0       0       0       000010000D      5\n",
    "     190       0       0       1       0                 SURFACE       0D      6\n",
    "     110       4       0       0       0       0       0       000010000D      7\n",
    "     110       0       0       1       0                   EDGE1       0D      8\n",
    "     110       5       0       0       0       0       0       000010000D      9\n",
    "     110       0       0       1       0                   EDGE2       0D     10\n",
    "     110       6       0       0       0       0       0       000010000D     11\n",
    "     110       0       0       1       0                   EDGE3       0D     12\n",
    "     110       7       0       0       0       0       0       000010000D     13\n",
    "     110       0       0       1       0                   EDGE4       0D     14\n",
    "     502       8       0       0       0       0       0       000010000D     15\n",
    "     502       0       0       1       1                VERTICES       0D     16\n",
    "     504       9       0       0       0       0       0       000010001D     17\n",
    "     504       0       0       1       1                   EDGES       0D     18\n",
    "     508      10       0       0       0       0       0       000010000D     19\n",
    "     508       0       0       1       1                    LOOP       0D     20\n",
    "     510      11       0       0       0       0       0       000010000D     21\n",
    "     510       0       0       1       1                    FACE       0D     22\n",
    "     514      12       0       0       0       0       0       000000000D     23\n",
    "     514       0       0       1       2                   SHELL       0D     24\n",
    "     406      13       0       0       0       0       0       000010000D     25\n",
    "     406       0       0       1      15                    NAME       0D     26\n",
    "     406      14       0       0       0       0       0       000010000D     27\n",
    "     406       0       0       1      15                    NAME       0D     28\n",
    "116,0,0,0,0;                                                           1P      1\n",
    "123,0,0,1;                                                             3P      2\n",
    "190,1,3;                                                               5P      3\n",
    "110,0,0,0,1,0,0;                                                       7P      4\n",
    "110,1,0,0,1,1,0;                                                       9P      5\n",
    "110,1,1,0,0,1,0;                                                      11P      6\n",
    "110,0,1,0,0,0,0;                                                      13P      7\n",
    "502,4,0,0,0,1,0,0,1,1,0,0,1,0;                                        15P      8\n",
    "504,4,7,15,1,15,2,9,15,2,15,3,11,15,3,15,4,13,15,4,15,1;              17P      9\n",
    "508,4,0,17,1,1,0,0,17,2,1,0,0,17,3,1,0,0,17,4,1,0;                    19P     10\n",
    "510,5,1,1,19,0,1,25;                                                  21P     11\n",
    "514,1,21,1,0,1,27;                                                    23P     12\n",
    "406,1,8HFACENAME;                                                     25P     13\n",
    "406,1,8HBODYNAME;                                                     27P     14\n",
    "S0000001G0000002D0000028P0000014                                        T      1\n",
);

/// The write states the body name as a Type 406 Form 15 on the trimmed
/// surface, so the name property of the shell is Regenerated and the name
/// property of the face, which the write does not state, is Dropped.
#[test]
fn a_body_name_written_on_a_trimmed_surface_restates_the_shell_name() {
    let mut failures = Vec::new();
    check(
        "face_and_body_names",
        FACE_AND_BODY_NAMES.as_bytes(),
        &mut failures,
    )
    .unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(FACE_AND_BODY_NAMES.as_bytes())
        .ok()
        .unwrap();
    for (id, disposition) in [
        ("iges:entity:directory#25", NativeDisposition::Dropped),
        ("iges:entity:directory#27", NativeDisposition::Regenerated),
        ("iges:entity:directory#23", NativeDisposition::Dropped),
    ] {
        assert_eq!(
            checked.native.disposition("iges", "entities", id),
            disposition,
            "{id}"
        );
    }
}

/// A free line whose Directory status blanks it.
const BLANKED_LINE: &str = concat!(
    "original fixture                                                        S      1\n",
    "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMG      1\n",
    "M,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;      G      2\n",
    "     110       1       0       0       0       0       0       001000000D      1\n",
    "     110       0       0       1       0                    LINE       0D      2\n",
    "110,0,0,0,1,0,0;                                                       1P      1\n",
    "S0000001G0000002D0000002P0000001                                        T      1\n",
);

/// The write states the line and no blank status of it, so the display
/// attribute record of the line is Dropped while the line is Regenerated.
#[test]
fn a_blank_status_the_write_does_not_state_is_dropped() {
    let mut failures = Vec::new();
    check("blanked_line", BLANKED_LINE.as_bytes(), &mut failures).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
    let checked = readback::write_and_read_back(BLANKED_LINE.as_bytes())
        .ok()
        .unwrap();
    for (arena, id, disposition) in [
        (
            "entities",
            "iges:entity:directory#1",
            NativeDisposition::Regenerated,
        ),
        (
            "display_attributes",
            "iges:presentation:display-attributes#D1",
            NativeDisposition::Dropped,
        ),
    ] {
        assert_eq!(
            checked.native.disposition("iges", arena, id),
            disposition,
            "{id}"
        );
    }
}

/// The body and the face of [`NAMED_BODY`] bound to one appearance of another
/// format, named `paint`, by bindings that state the source identity, the
/// source object type, and the change-version channels of their targets.
/// The write states the colour on the body and the face and the name in the
/// Type 314, so it charges neither the appearance nor the bindings.
#[test]
fn a_binding_that_states_its_source_object_is_written_with_its_target_and_colour() {
    let mut ir = IgesCodec
        .decode(
            &mut Cursor::new(NAMED_BODY.as_bytes()),
            &DecodeOptions::default(),
        )
        .unwrap()
        .ir()
        .clone();
    ir.native = cadmpeg_ir::native::Native::default();
    let paint =
        cadmpeg_ir::ids::AppearanceId::try_from("test:model:appearance#paint".to_owned()).unwrap();
    assert_eq!(ir.model.appearances.len(), 1);
    let appearance = &mut ir.model.appearances[0];
    appearance.id = paint.clone();
    appearance.name = Some("paint".to_owned());
    appearance.schema = None;
    let color = appearance.base_color.unwrap();
    for (ordinal, binding) in ir.model.appearance_bindings.iter_mut().enumerate() {
        binding.appearance = paint.clone();
        binding.source_entity_id = Some(format!("#{}", ordinal + 40));
        binding.object_type = Some("ADVANCED_FACE".to_owned());
        binding.channels = BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("geometry"),
            "0123".to_owned(),
        )]);
    }
    let (synthesis, _) =
        crate::writer::target::synthesis_and_dispositions(&ir, crate::IgesVersion::V5_3)
            .ok()
            .unwrap();
    let drops = appearances::AppearanceDrops::of(&ir, &synthesis);
    assert!(
        drops.appearances.is_empty() && drops.bindings.is_empty(),
        "{:?} {:?}",
        drops.appearances,
        drops.bindings
    );
    let output = IgesCodec
        .decode(&mut Cursor::new(synthesis.bytes), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone();
    let named = output
        .model
        .appearances
        .iter()
        .filter(|appearance| appearance.name.as_deref() == Some("paint"))
        .collect::<Vec<_>>();
    assert_eq!(named.len(), 1, "{:?}", output.model.appearances);
    assert_eq!(named[0].base_color, Some(color));
    assert_eq!(
        output
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| binding.appearance == named[0].id)
            .count(),
        2
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
