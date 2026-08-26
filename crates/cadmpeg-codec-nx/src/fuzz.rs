// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Desktop salvage ceilings for fuzz wrappers.
///
/// `DecodePolicy::service()` tightens collection and entity limits 8–16× and
/// would silently shrink coverage. Wrappers must not copy that profile.
fn fuzz_policy() -> DecodePolicy {
    DecodePolicy::default()
}

/// Exercise the NX deltas walker.
pub fn deltas(data: &[u8]) {
    let _ = crate::deltas::walk(data);
    let mid = data.len() / 2;
    let _ = crate::deltas::unmatched_terminal_tombstones(&data[..mid], &data[mid..]);
}

/// Exercise NX object-model indexed section framing.
pub fn om(data: &[u8]) {
    let _ = (
        crate::om_tokens::ROOT_MARKER,
        crate::om_tokens::HOST_GLOBALS,
        crate::om_tokens::CLASS_NAME_PREFIX,
        crate::om_tokens::NUMBER_PREFIX,
        crate::om_tokens::unit_for(std::str::from_utf8(data).unwrap_or("")),
    );
    let _ = crate::om::compact_indices(data);
    for section in crate::om::indexed_sections(data) {
        let _ = section.numeric_expressions();
    }
    for section in crate::om::sections(data) {
        let _ = section.operation_body_references();
    }
}

/// Exercise NX analytic point extraction.
pub fn geometry_points(data: &[u8]) {
    let _ = crate::geometry::points(data);
}

/// Exercise NX analytic curve extraction.
pub fn geometry_curves(data: &[u8]) {
    let _ = crate::geometry::curves(data);
}

/// Exercise NX analytic surface extraction.
pub fn geometry_surfaces(data: &[u8]) {
    let _ = crate::geometry::surfaces(data);
}

/// Exercise NX surface-intersection chart decoding.
pub fn intersection(data: &[u8]) {
    for curve in crate::intersection::curves(data, crate::intersection::ChartPointLayout::Xyz3) {
        let _ = (curve.references, curve.pos);
    }
}

/// Exercise NX NURBS curve extraction.
pub fn nurbs_curves(data: &[u8]) {
    let _ = crate::nurbs::curves(data);
}

/// Exercise NX NURBS parameter-space curve extraction.
pub fn nurbs_pcurves(data: &[u8]) {
    let _ = crate::nurbs::pcurves(data);
}

/// Exercise NX NURBS surface extraction.
pub fn nurbs_surfaces(data: &[u8]) {
    let _ = crate::nurbs::surfaces(data);
}

/// Exercise NX Parasolid topology parsing.
pub fn topology(data: &[u8]) {
    let graph = crate::topology::Graph::parse(data);
    for node in graph.of_kind(12) {
        let _ = node.byte_at(0);
        let _ = node.f64_at(0);
    }
    let _ = crate::topology::composite_curves(data);
    let _ = crate::topology::intersection_data_curves(data);
    let _ = crate::topology::blend_surfaces(data);
    let _ = crate::topology::offset_surfaces(data);
    let _ = crate::topology::surface_curves(data);
    let _ = crate::topology::trimmed_curves(data);
}

/// Exercise NX Parasolid stream extraction.
pub fn parasolid(data: &[u8]) {
    let arena = DecodeArena::new();
    let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &fuzz_policy()) else {
        return;
    };
    let Ok(container) = crate::container::scan_bytes(data.to_vec()) else {
        return;
    };
    if let Ok(streams) = crate::parasolid::extract_streams(&ctx, root, &container) {
        for stream in streams {
            let _ = stream.consumed;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::deltas(&[]);
        super::om(&[]);
        super::geometry_points(&[]);
        super::geometry_curves(&[]);
        super::geometry_surfaces(&[]);
        super::intersection(&[]);
        super::nurbs_curves(&[]);
        super::nurbs_surfaces(&[]);
        super::topology(&[]);
        super::parasolid(&[]);
    }

    #[test]
    fn deltas_wrapper_accepts_fixture() {
        let stream = crate::test_support::status_framed_deltas_stream();
        super::deltas(&stream);
    }

    #[test]
    fn om_wrapper_accepts_fixture() {
        super::om(&crate::test_support::indexed_om_section());
        super::om(&crate::test_support::size_framed_om_section());
    }

    #[test]
    fn geometry_wrappers_accept_fixture() {
        let stream = crate::test_support::partition_stream();
        super::geometry_points(&stream);
        super::geometry_curves(&stream);
        super::geometry_surfaces(&stream);
    }

    #[test]
    fn intersection_wrapper_accepts_fixture() {
        let stream = crate::test_support::charted_intersection_curve_topology_partition_stream();
        super::intersection(&stream);
    }

    #[test]
    fn nurbs_wrappers_accept_fixture() {
        let stream = crate::test_support::bspline_partition_stream();
        super::nurbs_curves(&stream);
        super::nurbs_surfaces(&stream);
    }

    #[test]
    fn topology_wrapper_accepts_fixture() {
        let stream = crate::test_support::topology_partition_stream();
        super::topology(&stream);
    }

    #[test]
    fn parasolid_wrapper_accepts_fixture() {
        let bytes = crate::test_support::single_part_prt();
        super::parasolid(&bytes);
    }
}
