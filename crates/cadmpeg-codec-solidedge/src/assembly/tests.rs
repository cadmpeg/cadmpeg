// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::products::{Occurrence, ProductDefinition, Storage};

use crate::test_support::{
    assembly_document, assembly_document_with_structure, assembly_document_with_table,
    assembly_document_with_visibility, family_document, family_info, member_table, member_table_of,
    mirror_document, structure_stream, visibility_stream, MemberSpec, MirrorSpec, OccurrenceSpec,
    RecordSpec, SiteSpec,
};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

const ABSOLUTE: &str = "C:\\x\\a.par";
const RELATIVE: &str = "a.par";

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

fn count(result: &DecodeResult, code: &str) -> usize {
    codes(result).iter().filter(|found| *found == code).count()
}

fn occurrences(result: &DecodeResult) -> &[Occurrence] {
    &result.ir().model.occurrences
}

fn definition<'a>(result: &'a DecodeResult, occurrence: &Occurrence) -> &'a ProductDefinition {
    result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.id == occurrence.prototype)
        .unwrap()
}

fn path<'a>(result: &'a DecodeResult, occurrence: &Occurrence) -> Option<&'a str> {
    match &definition(result, occurrence).storage {
        Storage::External(document) => document.path.as_ref().map(NonBlankString::as_str),
        other => panic!("expected an external definition, found {other:?}"),
    }
}

/// A part point `p` mapped by the occurrence's transform as a column vector.
fn place(occurrence: &Occurrence, p: [f64; 3]) -> [f64; 3] {
    let rows = occurrence.transform.unwrap().affine_rows();
    std::array::from_fn(|j| rows[j][0] * p[0] + rows[j][1] * p[1] + rows[j][2] * p[2] + rows[j][3])
}

fn near(found: [f64; 3], want: [f64; 3]) -> bool {
    found
        .iter()
        .zip(want)
        .all(|(a, b)| (a - b).abs() <= EPS_VALUE)
}

const SITE: u32 = 10;

fn site() -> SiteSpec<'static> {
    SiteSpec {
        id: SITE,
        links: Some([ABSOLUTE, RELATIVE]),
    }
}

/// A document with one occurrence, object 20, of the site.
fn single(change: impl FnOnce(&mut OccurrenceSpec)) -> DecodeResult {
    let mut spec = OccurrenceSpec::at_identity(20, SITE);
    change(&mut spec);
    decode(assembly_document(&[20], &[spec], &[site()]))
}

#[test]
fn identity_occurrence_states_its_document() {
    let result = single(|_| {});
    let [occurrence] = occurrences(&result) else {
        panic!("one occurrence")
    };
    assert_eq!(occurrence.suppressed, None);
    assert_eq!(occurrence.visible, None);
    assert!(near(
        place(occurrence, [40.0, 5.0, -3.0]),
        [40.0, 5.0, -3.0]
    ));
    assert_eq!(path(&result, occurrence), Some(ABSOLUTE));
    let root = result
        .ir()
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .unwrap();
    assert_eq!(root.occurrences.len(), 1);
    assert_eq!(root.occurrences[0], occurrence.id);
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 0);
    assert_eq!(count(&result, "assembly.reference-unread"), 0);
    assert_eq!(count(&result, "feature.navigator-absent"), 0);
}

#[test]
fn translation_is_millimetres() {
    let result = single(|spec| spec.translation = [0.01, 0.02, 0.03]);
    let occurrence = &occurrences(&result)[0];
    assert!(near(place(occurrence, [0.0; 3]), [10.0, 20.0, 30.0]));
    assert!(near(place(occurrence, [1.0, 2.0, 3.0]), [11.0, 22.0, 33.0]));
}

#[test]
fn quarter_turn_about_z_maps_x_to_y() {
    let result = single(|spec| spec.rotation = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let occurrence = &occurrences(&result)[0];
    assert!(near(place(occurrence, [40.0, 0.0, 0.0]), [0.0, 40.0, 0.0]));
    assert!(near(place(occurrence, [0.0, 40.0, 0.0]), [-40.0, 0.0, 0.0]));
    assert!(near(place(occurrence, [0.0, 0.0, 7.0]), [0.0, 0.0, 7.0]));
}

#[test]
fn rotation_and_translation_combine() {
    let result = single(|spec| {
        spec.rotation = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        spec.translation = [0.1, 0.0, 0.0];
    });
    let occurrence = &occurrences(&result)[0];
    assert!(near(
        place(occurrence, [40.0, 0.0, 0.0]),
        [100.0, 40.0, 0.0]
    ));
}

#[test]
fn occurrences_of_one_site_share_a_definition() {
    let other = SiteSpec {
        id: 11,
        links: Some(["C:\\x\\b.par", "b.par"]),
    };
    let result = decode(assembly_document(
        &[20, 21, 22],
        &[
            OccurrenceSpec::at_identity(20, SITE),
            OccurrenceSpec::at_identity(21, SITE),
            OccurrenceSpec::at_identity(22, 11),
        ],
        &[site(), other],
    ));
    let found = occurrences(&result);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].prototype, found[1].prototype);
    assert_ne!(found[0].prototype, found[2].prototype);
    // The root and one external definition per site.
    assert_eq!(result.ir().model.product_definitions.len(), 3);
    assert_eq!(path(&result, &found[2]), Some("C:\\x\\b.par"));
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}

#[test]
fn state_word_states_no_suppression() {
    for state in [2, 4, 6, 0] {
        let result = single(|spec| spec.state = state);
        assert_eq!(occurrences(&result).len(), 1);
        assert_eq!(occurrences(&result)[0].suppressed, None);
    }
}

#[test]
fn rotation_that_is_not_orthonormal_states_no_transform() {
    let result = single(|spec| spec.rotation = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let [occurrence] = occurrences(&result) else {
        panic!("the occurrence stays")
    };
    assert_eq!(occurrence.transform, None);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 1);
}

#[test]
fn reflection_states_no_transform() {
    let result = single(|spec| spec.rotation = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0]);
    assert_eq!(occurrences(&result)[0].transform, None);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 1);
}

#[test]
fn trailing_word_other_than_one_states_no_transform() {
    let result = single(|spec| spec.unit = 2.0);
    assert_eq!(occurrences(&result)[0].transform, None);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 1);
}

#[test]
fn missing_moniker_keeps_the_occurrence_without_a_path() {
    let bare = SiteSpec {
        id: SITE,
        links: None,
    };
    let result = decode(assembly_document(
        &[20],
        &[OccurrenceSpec::at_identity(20, SITE)],
        &[bare],
    ));
    let [occurrence] = occurrences(&result) else {
        panic!("the occurrence stays")
    };
    assert_eq!(path(&result, occurrence), None);
    assert!(occurrence.transform.is_some());
    assert_eq!(count(&result, "assembly.reference-unread"), 1);
}

#[test]
fn truncated_occurrence_is_not_transferred() {
    let mut cut = OccurrenceSpec::at_identity(20, SITE);
    cut.keep = Some(100);
    let result = decode(assembly_document(
        &[20, 21],
        &[cut, OccurrenceSpec::at_identity(21, SITE)],
        &[site()],
    ));
    assert_eq!(occurrences(&result).len(), 1);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 1);
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}

#[test]
fn listed_object_that_is_absent_is_not_transferred() {
    let result = decode(assembly_document(
        &[20, 99],
        &[OccurrenceSpec::at_identity(20, SITE)],
        &[site()],
    ));
    assert_eq!(occurrences(&result).len(), 1);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 1);
}

#[test]
fn site_that_is_not_a_site_object_is_not_transferred() {
    // Object 21 is an occurrence, not a site.
    let result = decode(assembly_document(
        &[20, 21],
        &[
            OccurrenceSpec::at_identity(20, 21),
            OccurrenceSpec::at_identity(21, SITE),
        ],
        &[site()],
    ));
    assert_eq!(occurrences(&result).len(), 1);
    assert_eq!(count(&result, "assembly.occurrence-unread"), 1);
}

const SHOWN: u32 = 0x4000_0001;
const HIDDEN: u32 = 0x4000_0003;

/// Two occurrences, objects 20 and 22, with the structure list `structure`.
fn pair(structure: Option<Vec<u8>>) -> DecodeResult {
    decode(assembly_document_with_structure(
        &[20, 22],
        &[
            OccurrenceSpec::at_identity(20, SITE),
            OccurrenceSpec::at_identity(22, SITE),
        ],
        &[site()],
        structure,
    ))
}

#[test]
fn structure_list_states_shown_hidden_and_suppressed() {
    let result = pair(Some(structure_stream(&[(SHOWN, 20), (HIDDEN, 22)])));
    let [first, second] = occurrences(&result) else {
        panic!("two occurrences")
    };
    assert_eq!((first.suppressed, first.visible), (Some(false), Some(true)));
    assert_eq!(
        (second.suppressed, second.visible),
        (Some(false), Some(false))
    );
    assert_eq!(count(&result, "assembly.visibility-unread"), 0);

    let result = pair(Some(structure_stream(&[(SHOWN, 20)])));
    let [first, second] = occurrences(&result) else {
        panic!("two occurrences")
    };
    assert_eq!((first.suppressed, first.visible), (Some(false), Some(true)));
    assert_eq!(second.suppressed, Some(true));
    assert_eq!(second.visible, None);
    assert_eq!(count(&result, "assembly.visibility-unread"), 0);
}

#[test]
fn structure_list_of_the_wrong_length_states_nothing() {
    let mut stream = structure_stream(&[(SHOWN, 20), (SHOWN, 22)]);
    stream.pop();
    let result = pair(Some(stream));
    for occurrence in occurrences(&result) {
        assert_eq!((occurrence.suppressed, occurrence.visible), (None, None));
    }
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}

#[test]
fn structure_list_naming_an_occurrence_twice_states_nothing() {
    let result = pair(Some(structure_stream(&[(SHOWN, 20), (HIDDEN, 20)])));
    for occurrence in occurrences(&result) {
        assert_eq!(occurrence.suppressed, None);
    }
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}

#[test]
fn no_occurrence_adds_no_visibility_loss() {
    let result = decode(assembly_document(&[], &[], &[]));
    assert!(occurrences(&result).is_empty());
    assert_eq!(count(&result, "assembly.visibility-unread"), 0);
}

#[test]
fn document_without_a_collection_adds_nothing() {
    let result = decode(mirror_document(&MirrorSpec {
        count: 1,
        seeds: &[10],
        plane: None,
    }));
    assert!(occurrences(&result).is_empty());
    assert!(codes(&result)
        .iter()
        .all(|code| !code.starts_with("assembly.")));
}

fn member<'a>(name: &'a str, listed: &'a [u32], ids: &[u32]) -> MemberSpec<'a> {
    MemberSpec {
        name,
        listed,
        occurrences: ids
            .iter()
            .map(|&id| OccurrenceSpec::at_identity(id, SITE))
            .collect(),
    }
}

fn ids(result: &DecodeResult) -> Vec<String> {
    occurrences(result)
        .iter()
        .map(|occurrence| occurrence.id.to_string())
        .collect()
}

#[test]
fn family_places_the_occurrences_the_active_member_lists() {
    let members = [
        member("alpha", &[20], &[20, 21]),
        member("beta", &[20, 21], &[20]),
    ];
    let result = decode(family_document(
        family_info("alpha"),
        None,
        &members,
        &[site()],
    ));
    assert_eq!(ids(&result), ["solidedge:model:occurrence#20"]);
    assert_eq!(count(&result, "family.member-unread"), 0);
    let result = decode(family_document(
        family_info("BETA"),
        None,
        &members,
        &[site()],
    ));
    assert_eq!(ids(&result), ["solidedge:model:occurrence#20"]);
    let all = [member("alpha", &[20, 21], &[20, 21])];
    let result = decode(family_document(family_info("alpha"), None, &all, &[site()]));
    assert_eq!(occurrences(&result).len(), 2);
}

#[test]
fn family_without_a_member_record_keeps_every_occurrence() {
    let members = [member("alpha", &[20], &[20, 21])];
    let table = member_table(&[("other", &[20])]);
    let result = decode(family_document(
        family_info("alpha"),
        Some(table),
        &members,
        &[site()],
    ));
    assert_eq!(occurrences(&result).len(), 2);
    assert_eq!(count(&result, "family.member-unread"), 1);
}

#[test]
fn family_naming_no_member_storage_places_nothing() {
    let members = [member("alpha", &[20], &[20])];
    let result = decode(family_document(
        family_info("gone"),
        None,
        &members,
        &[site()],
    ));
    assert!(occurrences(&result).is_empty());
    assert_eq!(count(&result, "family.member-unread"), 1);
}

/// Occurrence identifiers of a family of four, in insertion order.
const FOUR: [u32; 4] = [20, 21, 22, 23];

/// The suppression of each occurrence of a family of four whose second member
/// excludes `excluded`, with the first record's last word `word`.
fn suppression(word: u32, excluded: &[u32]) -> Vec<Option<bool>> {
    let table = member_table_of(&[
        RecordSpec {
            name: "alpha",
            excluded: &[],
            listed: &FOUR,
            word,
        },
        RecordSpec {
            name: "beta",
            excluded,
            listed: &FOUR,
            word: 3,
        },
    ]);
    let members = [member("alpha", &FOUR, &FOUR)];
    let result = decode(family_document(
        family_info("alpha"),
        Some(table),
        &members,
        &[site()],
    ));
    assert_eq!(count(&result, "family.member-unread"), 0);
    occurrences(&result)
        .iter()
        .map(|occurrence| occurrence.suppressed)
        .collect()
}

#[test]
fn family_member_state_suppresses_the_excluded_occurrences_it_keeps() {
    // The family documents hold no structure list, so only an excluded
    // occurrence states its suppression.
    let off = None;
    let on = Some(true);
    // The first member is active: nothing is excluded.
    assert_eq!(suppression(3, &[21]), [off; 4]);
    // The second member is active.
    assert_eq!(suppression(1, &[]), [off; 4]);
    assert_eq!(suppression(1, &[20]), [on, off, off, off]);
    assert_eq!(suppression(1, &[23]), [off, off, off, on]);
    assert_eq!(suppression(1, &[21, 23]), [off, on, off, on]);
}

/// Four occurrences, objects 20 to 23, of the site, with a visibility table
/// that hides `hidden`.
fn four_with_visibility(hidden: &[u32]) -> DecodeResult {
    let specs: Vec<OccurrenceSpec> = (20..24)
        .map(|id| OccurrenceSpec::at_identity(id, SITE))
        .collect();
    decode(assembly_document_with_visibility(
        &[20, 21, 22, 23],
        &specs,
        &[site()],
        hidden,
    ))
}

fn visible_states(result: &DecodeResult) -> Vec<Option<bool>> {
    occurrences(result)
        .iter()
        .map(|occurrence| occurrence.visible)
        .collect()
}

#[test]
fn empty_visibility_table_states_every_occurrence_visible() {
    let result = four_with_visibility(&[]);
    assert_eq!(visible_states(&result), vec![Some(true); 4]);
    assert_eq!(count(&result, "assembly.visibility-unread"), 0);
}

#[test]
fn listed_objects_are_hidden() {
    for hidden in [&[20][..], &[20, 21], &[20, 21, 22], &[21, 23]] {
        let result = four_with_visibility(hidden);
        let want: Vec<Option<bool>> = (20..24).map(|id| Some(!hidden.contains(&id))).collect();
        assert_eq!(visible_states(&result), want);
        assert_eq!(count(&result, "assembly.visibility-unread"), 0);
    }
}

#[test]
fn hidden_object_that_is_no_occurrence_changes_nothing() {
    let result = four_with_visibility(&[99]);
    assert_eq!(visible_states(&result), vec![Some(true); 4]);
}

#[test]
fn absent_visibility_table_states_no_visibility() {
    let result = single(|_| {});
    assert_eq!(occurrences(&result)[0].visible, None);
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}

#[test]
fn visibility_table_shorter_than_its_count_states_no_visibility() {
    let mut table = visibility_stream(&[20, 21]);
    table.truncate(table.len() - 4);
    let result = decode(assembly_document_with_table(
        &[20],
        &[OccurrenceSpec::at_identity(20, SITE)],
        &[site()],
        Some(table),
    ));
    assert_eq!(occurrences(&result)[0].visible, None);
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}

#[test]
fn visibility_table_without_a_count_states_no_visibility() {
    let result = decode(assembly_document_with_table(
        &[20],
        &[OccurrenceSpec::at_identity(20, SITE)],
        &[site()],
        Some(vec![0; 100]),
    ));
    assert_eq!(occurrences(&result)[0].visible, None);
    assert_eq!(count(&result, "assembly.visibility-unread"), 1);
}
