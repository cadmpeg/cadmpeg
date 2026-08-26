// SPDX-License-Identifier: Apache-2.0
//! Typed views over `SolidWorks` `ResolvedFeatures` sketch records.

const SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x03];

const LEGACY_SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x07, 0x00, 0x01];

const LEGACY_EXTENDED_SKETCH_MARKER: &[u8] = &[0xff, 0xff, 0x1f, 0x00, 0x01];

const CLASS_MARKER: &[u8] = &[0xff, 0xff, 0x01, 0x00];

const NAME_MARKER: &[u8] = &[0x04, 0x80, 0xff, 0xfe, 0xff];

const SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xfe, 0xff, 0x00, 0x00, 0x00,
];

const COMPACT_SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00,
];

const VALUE_ONLY_SCALAR_HEADER: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00,
];

const SKETCH_POINT_TOLERANCE: f64 = 1.0e-9;

const SKETCH_ANGLE_TOLERANCE: f64 = 1.0e-9;

const SPATIAL_VERTEX_PREFIX: &[u8] = &[
    0xff, 0xfe, 0xff, 0x06, b'V', 0x00, b'e', 0x00, b'r', 0x00, b't', 0x00, b'e', 0x00, b'x', 0x00,
];

fn is_class_token(token: u16) -> bool {
    token & 0x8000 != 0 && token != u16::MAX
}

pub(crate) mod assembly;

pub(crate) mod axes;

pub(crate) mod bindings;

pub(crate) mod classes;

mod compact_reference_planes;

pub(crate) mod component_paths;

mod curves;

pub(crate) mod dimensions;

pub(crate) mod direct_edits;

mod drafts;

mod endpoints;

pub(crate) mod hashes;

pub(crate) mod helix;

pub(crate) mod holes;

pub(crate) mod markers;

pub(crate) mod names;

pub(crate) mod operands;

pub(crate) mod operations;

pub(crate) mod parameters;

pub(crate) mod profiles;

pub(crate) mod projections;

pub(crate) mod reference_geometry;

pub(crate) mod relation_geometry;

mod relation_loci;

mod relation_records;

pub(crate) mod scalars;

pub(crate) mod selections;

mod sketch_edges;

pub(crate) mod sketch_projection;

mod sketch_write;

pub(crate) mod terminations;

mod transforms;

pub(crate) mod typed_relations;

pub(crate) mod validate;

mod write_generate;

pub(crate) mod write_prepare;
