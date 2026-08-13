// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

/// Exercise the NX deltas walker.
pub fn deltas(data: &[u8]) {
    let _ = crate::deltas::walk(data);
}

/// Exercise NX object-model indexed section framing.
pub fn om(data: &[u8]) {
    for section in crate::om::indexed_sections(data) {
        let _ = section.numeric_expressions();
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
    let _ = crate::intersection::curves(data, crate::intersection::ChartPointLayout::Xyz3);
}

/// Exercise NX NURBS curve extraction.
pub fn nurbs_curves(data: &[u8]) {
    let _ = crate::nurbs::curves(data);
}

/// Exercise NX NURBS surface extraction.
pub fn nurbs_surfaces(data: &[u8]) {
    let _ = crate::nurbs::surfaces(data);
}

/// Exercise NX Parasolid topology parsing.
pub fn topology(data: &[u8]) {
    let _ = crate::topology::Graph::parse(data);
    let _ = crate::topology::composite_curves(data);
    let _ = crate::topology::intersection_data_curves(data);
    let _ = crate::topology::blend_surfaces(data);
    let _ = crate::topology::offset_surfaces(data);
    let _ = crate::topology::surface_curves(data);
    let _ = crate::topology::trimmed_curves(data);
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
}
