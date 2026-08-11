// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
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

/// Exercise standard-family vertex-record scanning.
pub fn geometry_vertices(data: &[u8]) {
    let _ = crate::wire::records::scan_vertex_records(data);
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

/// Exercise standard-nested and FBB topology parsing.
pub fn standard_topology(data: &[u8]) {
    if let Some(topology) = crate::families::standard::fbb::parse_standard(data) {
        let _ = topology.edge_vertices();
    }
    let _ = crate::families::standard::topology::parse_fbb(data);
}

/// Exercise `7C0B` value-block parsing.
pub fn value_blocks(data: &[u8]) {
    let _ = crate::value_block::parse(data);
}

/// Exercise `7C08` object-graph parsing.
pub fn object_graph(data: &[u8]) {
    let _ = crate::object_graph::parse(data);
    let _ = crate::object_graph::surface_aliases(data);
    let _ = crate::object_graph::markers_7cd9(data, data.len());
}

/// Exercise `7C02` string-catalog parsing.
pub fn catalog(data: &[u8]) {
    let _ = crate::catalog::parse(data);
}

/// Exercise zero-entity record inventory parsing.
pub fn zero_entity(data: &[u8]) {
    let _ = crate::families::zero_entity::records::zero_entity_record_inventory(data);
}
