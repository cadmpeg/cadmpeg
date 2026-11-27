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
