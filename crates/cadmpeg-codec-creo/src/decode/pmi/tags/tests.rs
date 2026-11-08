// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{datum_join, DatumIndex, DatumJoin};

#[test]
fn incomplete_label_roster_does_not_merge_a_unique_existing_datum() {
    let datum_indices = BTreeMap::from([("A".to_owned(), DatumIndex::Unique(0))]);
    assert_eq!(
        datum_join(false, 1, &datum_indices, "A"),
        DatumJoin::Separate
    );
}

#[test]
fn complete_unique_label_roster_can_join_one_existing_datum() {
    let datum_indices = BTreeMap::from([("A".to_owned(), DatumIndex::Unique(0))]);
    assert_eq!(
        datum_join(true, 1, &datum_indices, "A"),
        DatumJoin::Existing(0)
    );
}

#[test]
fn repeated_tag_labels_stay_separate_from_a_unique_existing_datum() {
    let datum_indices = BTreeMap::from([("A".to_owned(), DatumIndex::Unique(0))]);
    assert_eq!(
        datum_join(true, 2, &datum_indices, "A"),
        DatumJoin::Separate
    );
}
