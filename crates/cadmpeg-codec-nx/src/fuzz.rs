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

#[cfg(test)]
mod tests {
    #[test]
    fn wrappers_accept_empty() {
        super::deltas(&[]);
        super::om(&[]);
        super::geometry_points(&[]);
        super::geometry_curves(&[]);
        super::geometry_surfaces(&[]);
    }

    #[test]
    fn geometry_wrappers_accept_fixture() {
        let stream = crate::test_support::partition_stream();
        super::geometry_points(&stream);
        super::geometry_curves(&stream);
        super::geometry_surfaces(&stream);
    }
}
