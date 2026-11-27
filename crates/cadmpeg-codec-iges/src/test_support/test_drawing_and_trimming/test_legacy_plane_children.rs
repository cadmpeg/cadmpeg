// SPDX-License-Identifier: Apache-2.0
//! Legacy single-parent bounded planes whose Type 402 Form 9 children bound
//! no hole of the parent's face.

use super::legacy_perforated_plane_entities;
use crate::test_support::test_owned::owned_test_file_with_global_and_line_fonts;

/// [`super::legacy_perforated_plane_file`] with a second Form 9 child
/// pointer, to DE 99, which names no entry.
pub(crate) fn legacy_dangling_child_single_parent_file(global: &[u8]) -> Vec<u8> {
    let mut entities = legacy_perforated_plane_entities();
    entities[4].parameters = "402,1,2,1,3,99;".into();
    owned_test_file_with_global_and_line_fonts(&entities, global, &[(1, 1), (3, 1), (5, 1), (7, 1)])
}

/// [`super::legacy_perforated_plane_file`] whose Type 108 hole at DE 3 is
/// not physically dependent.
pub(crate) fn legacy_independent_hole_single_parent_file(global: &[u8]) -> Vec<u8> {
    let mut entities = legacy_perforated_plane_entities();
    entities[1].status = "00000000";
    owned_test_file_with_global_and_line_fonts(&entities, global, &[(1, 1), (3, 1), (5, 1), (7, 1)])
}

/// [`super::legacy_perforated_plane_file`] whose one child pointer, to DE
/// 99, names no entry.
pub(crate) fn legacy_dangling_only_child_single_parent_file(global: &[u8]) -> Vec<u8> {
    let mut entities = legacy_perforated_plane_entities();
    entities[4].parameters = "402,1,1,1,99;".into();
    owned_test_file_with_global_and_line_fonts(&entities, global, &[(1, 1), (3, 1), (5, 1), (7, 1)])
}

/// [`super::legacy_two_hole_single_parent_file`] whose second hole, at DE
/// 11, lies in the parent's plane and carries no association back pointer
/// to the relation.
pub(crate) fn legacy_unlinked_child_single_parent_file(global: &[u8]) -> Vec<u8> {
    let mut entities = super::legacy_two_hole_entities();
    entities[5].parameters = "108,0,0,1,0,13,0,0,0,0,0,0;".into();
    owned_test_file_with_global_and_line_fonts(
        &entities,
        global,
        &[(1, 1), (3, 1), (5, 1), (7, 1), (11, 1), (13, 1)],
    )
}
