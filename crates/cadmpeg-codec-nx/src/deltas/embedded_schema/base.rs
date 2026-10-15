// SPDX-License-Identifier: Apache-2.0
//! V13 effective field arrays for supported inline node types.

use super::{BorrowedField, DataType};

pub(super) fn fields(kind: u16) -> Option<&'static [BorrowedField<'static>]> {
    match kind {
        12 => Some(&TYPE_12),
        19 => Some(&TYPE_19),
        38 => Some(&TYPE_38),
        41 => Some(&TYPE_41),
        70 => Some(&TYPE_70),
        74 => Some(&TYPE_74),
        100 => Some(&TYPE_100),
        101 => Some(&TYPE_101),
        _ => None,
    }
}

const TYPE_12: [BorrowedField<'static>; 23] = [
    BorrowedField::new("highest_node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("attribute_chains", DataType::Pointer, 70, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("key", DataType::Pointer, 102, 0, true),
    BorrowedField::new("res_size", DataType::Real, 0, 0, true),
    BorrowedField::new("res_linear", DataType::Real, 0, 0, true),
    BorrowedField::new("ref_instance", DataType::Pointer, 11, 0, true),
    BorrowedField::new("next", DataType::Pointer, 12, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 12, 0, true),
    BorrowedField::new("state", DataType::Byte, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 101, 0, true),
    BorrowedField::new("body_type", DataType::Byte, 0, 0, true),
    BorrowedField::new("nom_geom_state", DataType::Byte, 0, 0, true),
    BorrowedField::new("shell", DataType::Pointer, 13, 0, true),
    BorrowedField::new("boundary_surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("boundary_curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("boundary_point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("region", DataType::Pointer, 19, 0, true),
    BorrowedField::new("edge", DataType::Pointer, 16, 0, true),
    BorrowedField::new("vertex", DataType::Pointer, 18, 0, true),
];

const TYPE_19: [BorrowedField<'static>; 7] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("body", DataType::Pointer, 12, 0, true),
    BorrowedField::new("next", DataType::Pointer, 19, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 19, 0, true),
    BorrowedField::new("shell", DataType::Pointer, 13, 0, true),
    BorrowedField::new("type", DataType::Char, 0, 0, true),
];

const TYPE_38: [BorrowedField<'static>; 11] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("attributes_features", DataType::Pointer, 1019, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1010, 0, true),
    BorrowedField::new("next", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("geometric_owner", DataType::Pointer, 141, 0, true),
    BorrowedField::new("sense", DataType::Char, 0, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 2, true),
    BorrowedField::new("chart", DataType::Pointer, 40, 0, true),
    BorrowedField::new("start", DataType::Pointer, 41, 0, true),
    BorrowedField::new("end", DataType::Pointer, 41, 0, true),
];

const TYPE_41: [BorrowedField<'static>; 2] = [
    BorrowedField::new("type", DataType::Char, 0, 0, true),
    BorrowedField::new("hvec", DataType::Hvec, 0, 1, true),
];

const TYPE_70: [BorrowedField<'static>; 12] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1013, 0, true),
    BorrowedField::new("next", DataType::Pointer, 70, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 70, 0, true),
    BorrowedField::new("list_type", DataType::Integer, 0, 0, true),
    BorrowedField::new("list_length", DataType::Integer, 0, 0, true),
    BorrowedField::new("block_length", DataType::Integer, 0, 0, true),
    BorrowedField::new("size_of_entry", DataType::Integer, 0, 0, true),
    BorrowedField::new("list_block", DataType::Pointer, 1012, 0, true),
    BorrowedField::new("finger_block", DataType::Pointer, 1012, 0, true),
    BorrowedField::new("finger_index", DataType::Integer, 0, 0, true),
    BorrowedField::new("notransmit", DataType::Logical, 0, 0, true),
];

const TYPE_74: [BorrowedField<'static>; 3] = [
    BorrowedField::new("n_entries", DataType::Integer, 0, 0, true),
    BorrowedField::new("next_block", DataType::Pointer, 74, 0, true),
    BorrowedField::new("entries", DataType::Pointer, 1001, 1, true),
];

const TYPE_100: [BorrowedField<'static>; 9] = [
    BorrowedField::new("node_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("owner", DataType::Pointer, 1023, 0, true),
    BorrowedField::new("next", DataType::Pointer, 100, 0, true),
    BorrowedField::new("previous", DataType::Pointer, 100, 0, true),
    BorrowedField::new("rotation_matrix", DataType::Real, 0, 9, true),
    BorrowedField::new("translation_vector", DataType::Vector, 0, 0, true),
    BorrowedField::new("scale", DataType::Real, 0, 0, true),
    BorrowedField::new("flag", DataType::Integer, 0, 0, true),
    BorrowedField::new("perspective_vector", DataType::Vector, 0, 0, true),
];

const TYPE_101: [BorrowedField<'static>; 11] = [
    BorrowedField::new("assembly", DataType::Pointer, 10, 0, true),
    BorrowedField::new("attribute", DataType::Pointer, 81, 0, true),
    BorrowedField::new("body", DataType::Pointer, 12, 0, true),
    BorrowedField::new("transform", DataType::Pointer, 100, 0, true),
    BorrowedField::new("surface", DataType::Pointer, 1006, 0, true),
    BorrowedField::new("curve", DataType::Pointer, 1008, 0, true),
    BorrowedField::new("point", DataType::Pointer, 29, 0, true),
    BorrowedField::new("alive", DataType::Logical, 0, 0, true),
    BorrowedField::new("attrib_def", DataType::Pointer, 80, 0, true),
    BorrowedField::new("highest_id", DataType::Integer, 0, 0, true),
    BorrowedField::new("current_id", DataType::Integer, 0, 0, true),
];
