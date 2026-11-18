// SPDX-License-Identifier: Apache-2.0

use super::{entity_name, table, LIST};

#[test]
fn every_list_line_is_one_pair() {
    assert!(LIST
        .lines()
        .all(|line| line.split_whitespace().count() == 2));
}

#[test]
fn each_short_name_names_one_entity() {
    assert!(table().windows(2).all(|pair| pair[0].0 < pair[1].0));
}

#[test]
fn no_short_name_is_another_entity_name() {
    for (short, entity) in table() {
        assert!(
            table().iter().all(|(_, other)| other != short),
            "{short} {entity}"
        );
    }
}

#[test]
fn a_short_name_resolves_to_its_entity() {
    assert_eq!(entity_name("CRTPNT"), Some("CARTESIAN_POINT"));
    assert_eq!(entity_name("CMRPIT"), Some("COMPOUND_REPRESENTATION_ITEM"));
    assert_eq!(entity_name("DRCACT"), Some("DIRECTED_ACTION"));
    assert_eq!(entity_name("CARTESIAN_POINT"), None);
    assert_eq!(entity_name("CIRCLE"), None);
}
