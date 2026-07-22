// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic. This facade exists only to
//! keep those parsers reachable from the fuzz harness without widening the
//! crate's public API; it is gated behind the `fuzz` feature and hidden from
//! documentation.
#![doc(hidden)]

/// Exercise `V5_CFV2` container stream-directory parsing.
pub fn container_directory(data: &[u8]) {
    let _ = crate::container::parse_stream_directory(data);
}

/// Exercise `b5 03` object-stream graph parsing.
pub fn b5_parse(data: &[u8]) {
    let _ = crate::families::b5::graph::parse(data);
}

/// Exercise `e5 0d 03` topology parsing and orientation solving.
pub fn e5_topology(data: &[u8]) {
    let _ = crate::families::e5::graph::parse_topology(data);
}

/// Exercise zero-entity `a9 03` topology parsing.
pub fn zero_entity_parse(data: &[u8]) {
    let _ = crate::families::zero_entity::graph::parse(data);
}

/// Exercise standard-family vertex-record scanning.
pub fn geometry_vertices(data: &[u8]) {
    let _ = crate::families::standard::records::scan_vertex_records(data);
}

/// Exercise standard-family surface-prefix extraction.
pub fn geometry_surface_prefixes(data: &[u8]) {
    let _ = crate::families::standard::records::surface_prefixes(data);
}

/// Exercise A5 freeform surface extraction.
pub fn geometry_a5_surfaces(data: &[u8]) {
    let _ = crate::families::a5a8::records::a5_surfaces(data);
}

/// Exercise A8 NURBS surface extraction.
pub fn geometry_a8_surfaces(data: &[u8]) {
    let _ = crate::families::a5a8::records::a8_surfaces(data);
}
