// SPDX-License-Identifier: Apache-2.0
//! `()`-returning wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper feeds arbitrary bytes to one internal parser and discards the
//! result. The contract is that no input may panic.
#![doc(hidden)]

use crate::framing::node_kind::NodeKind;
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
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        drop(crate::deltas::census::walk(&ctx, data));
        let mid = data.len() / 2;
        drop(crate::deltas::unmatched_terminal_tombstones(
            &ctx,
            &data[..mid],
            &data[mid..],
        ));
    }
}

/// Exercise NX object-model indexed section framing.
pub fn om(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)?;
    let _ = (
        crate::om_tokens::ROOT_MARKER,
        crate::om_tokens::HOST_GLOBALS,
        crate::om_tokens::CLASS_NAME_PREFIX,
        crate::om_tokens::NUMBER_PREFIX,
        // `unit_for` reads text. Bytes that state no text are the refusal this
        // wrapper passes through: the lookup does not run for them and nothing
        // stands in for the text they do not state. The walkers below still
        // see the whole input.
        std::str::from_utf8(data)
            .ok()
            .map(crate::om_tokens::unit_for),
    );
    let mut at = 0;
    while let Some(token) = crate::om::compact::NullableCompactIndex::read(data, at) {
        at += token.raw().len();
    }
    for section in crate::om::indexed_sections(&ctx, data)? {
        drop(section.numeric_expressions(&ctx)?);
    }
    for section in crate::om::sections(&ctx, data)? {
        drop(section.operation_body_references());
    }
    Ok(())
}

/// Exercise NX analytic point extraction.
pub fn geometry_points(data: &[u8]) {
    with_geometry_context(data, |ctx| drop(crate::geometry::points(ctx, data)));
}

/// Exercise NX analytic curve extraction.
pub fn geometry_curves(data: &[u8]) {
    with_geometry_context(data, |ctx| drop(crate::geometry::curves(ctx, data)));
}

/// Exercise NX analytic surface extraction.
pub fn geometry_surfaces(data: &[u8]) {
    with_geometry_context(data, |ctx| drop(crate::geometry::surfaces(ctx, data)));
}

fn with_geometry_context(data: &[u8], parse: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &fuzz_policy()) {
        parse(&ctx);
    }
}

/// Exercise NX surface-intersection chart decoding.
pub fn intersection(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    if let Ok(curves) = crate::intersection::curves(&ctx, data, crate::intersection::ChartPointLayout::Xyz3) {
        for curve in curves {
            // discarded-value: fuzz decoded curve fields without using their values.
            let _ = (curve.references, curve.pos);
        }
    }
}

/// Exercise NX NURBS curve extraction.
pub fn nurbs_curves(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        drop(crate::nurbs::curves(&ctx, data));
    }
}

/// Exercise NX NURBS surface extraction.
pub fn nurbs_surfaces(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    if let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        drop(crate::nurbs::surfaces(&ctx, data));
    }
}

/// Exercise NX Parasolid topology parsing.
pub fn topology(data: &[u8]) {
    let arena = DecodeArena::new();
    let policy = fuzz_policy();
    let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    if let Ok(graph) = crate::topology::Graph::parse(&ctx, data) {
        for node in graph.of_kind(NodeKind::Body) {
            // discarded-value: fuzz this bounded node read without using its value.
            let _ = node.byte_at(0);
            // discarded-value: fuzz this bounded scalar read without using its value.
            let _ = node.f64_at(0);
        }
    }
    drop(crate::topology::composite_curves(&ctx, data));
    drop(crate::topology::intersection_data_curves(&ctx, data));
    drop(crate::topology::blend_surfaces(&ctx, data));
    drop(crate::topology::offset_surfaces(&ctx, data));
    drop(crate::topology::surface_curves(&ctx, data));
    drop(crate::topology::trimmed_curves(&ctx, data));
}

/// Exercise NX Parasolid stream extraction.
pub fn parasolid(data: &[u8]) {
    let arena = DecodeArena::new();
    let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &fuzz_policy()) else {
        return;
    };
    let Ok(container) = crate::container::scan_bytes(&ctx, data.to_vec()) else {
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
        drop(super::om(&[]));
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
        let stream = crate::test_support::test_deltas::status_framed_deltas_stream();
        super::deltas(&stream);
    }

    #[test]
    fn om_wrapper_accepts_fixture() {
        drop(super::om(&crate::test_support::test_om::indexed_om_section()));
        drop(super::om(&crate::test_support::test_om::size_framed_om_section()));
    }

    #[test]
    fn geometry_wrappers_accept_fixture() {
        let stream = crate::test_support::test_streams::partition_stream();
        super::geometry_points(&stream);
        super::geometry_curves(&stream);
        super::geometry_surfaces(&stream);
    }

    #[test]
    fn intersection_wrapper_accepts_fixture() {
        let stream =
            crate::test_support::test_streams::charted_intersection_curve_topology_partition_stream(
            );
        super::intersection(&stream);
    }

    #[test]
    fn nurbs_wrappers_accept_fixture() {
        let stream = crate::test_support::test_deltas::bspline_partition_stream();
        super::nurbs_curves(&stream);
        super::nurbs_surfaces(&stream);
    }

    #[test]
    fn topology_wrapper_accepts_fixture() {
        let stream = crate::test_support::test_streams::topology_partition_stream();
        super::topology(&stream);
    }

    #[test]
    fn parasolid_wrapper_accepts_fixture() {
        let bytes = crate::test_support::test_prt::single_part_prt();
        super::parasolid(&bytes);
    }

    /// Bytes that state no text are an input the wrapper carries to every
    /// parser below it. Only the token lookup that reads text is skipped, and
    /// nothing stands in for the text the input does not state.
    #[test]
    fn om_accepts_bytes_that_state_no_text() {
        let bytes: Vec<u8> = vec![0xff, 0xfe, 0x80, 0x41, 0x00, 0xc3, 0x28];
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "the fixture must state no text"
        );
        drop(super::om(&bytes));
    }
}
