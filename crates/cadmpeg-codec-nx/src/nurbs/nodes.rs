// SPDX-License-Identifier: Apache-2.0
//! NURBS lanes read from a terminated node sequence.

use super::{
    carrier_payloads, finite_f64_bytes, unique_records, ArrayValues, Arrays, CurveBasis,
    CurveDescriptor, CurveDescriptorReferences, Lanes, Payload, RecordIndex, SurfaceDescriptor,
};
use cadmpeg_parasolid::records::node_type::{
    BSPLINE_VERTICES, B_CURVE, B_SURFACE, KNOT_MULT, KNOT_SET, NURBS_CURVE, NURBS_SURF,
};
use cadmpeg_parasolid::records::primary::Node;
use cadmpeg_parasolid::records::PrimaryRecords;
use cadmpeg_parasolid::xmt::NonNullXmt;
use std::collections::BTreeMap;

/// The lanes of every B-spline node family, keyed as the decoders join them:
/// knot arrays and descriptors by their own index, control points by the
/// `data` index of the `B_SURFACE` or `B_CURVE` that names them.
pub(super) fn lanes<'a>(bytes: &'a [u8], primary: &PrimaryRecords<'_>) -> Lanes<'a> {
    let mut by_index = BTreeMap::<(u16, u32), Option<Node<'_>>>::new();
    for node in primary.nodes() {
        by_index
            .entry((node.kind(), node.index()))
            .and_modify(|entry| *entry = None)
            .or_insert(Some(node));
    }
    let node = |kind: u16, index: u32| by_index.get(&(kind, index)).copied().flatten();
    // The typed accessors admit a field only when its effective schema
    // declares a transmitted variable field of the lane's type.
    let reals = |node: &Node<'_>, name: &str| {
        node.reals(name)?;
        let raw = bytes.get(node.field_range(name)?)?;
        finite_f64_bytes(raw)?;
        Some(raw)
    };
    let shorts = |node: &Node<'_>, name: &str| {
        node.shorts(name)?;
        bytes.get(node.field_range(name)?)
    };
    let vertices = |nurbs_kind: u16, nurbs: u32| {
        let definition = node(nurbs_kind, nurbs)?;
        let reference = definition.pointer("bspline_vertices")?;
        let raw = reals(&node(BSPLINE_VERTICES, reference)?, "vertices")?;
        Some(Payload {
            raw,
            vertices: reference,
        })
    };
    let payloads = |carrier: u16, nurbs_kind: u16| -> RecordIndex<Payload<'a>> {
        carrier_payloads(
            primary
                .nodes()
                .filter(|node| node.kind() == carrier)
                .filter_map(|node| {
                    let data = node.pointer("data").filter(|data| *data > 1)?;
                    let payload = node
                        .pointer("nurbs")
                        .and_then(|nurbs| vertices(nurbs_kind, nurbs));
                    Some((data, payload))
                }),
        )
    };
    let mut u16s = RecordIndex::new();
    let mut f64s = RecordIndex::new();
    for node in primary.nodes() {
        let (index, values) = match node.kind() {
            KNOT_MULT => (&mut u16s, shorts(&node, "mult").map(ArrayValues::U16)),
            KNOT_SET => (&mut f64s, reals(&node, "knots").map(ArrayValues::F64)),
            _ => continue,
        };
        index
            .entry(node.index())
            .and_modify(|entry| *entry = None)
            .or_insert(values);
    }
    Lanes {
        arrays: Arrays { u16s, f64s },
        surface_payloads: payloads(B_SURFACE, NURBS_SURF),
        curve_payloads: payloads(B_CURVE, NURBS_CURVE),
        surface_descriptors: unique_records(
            primary
                .nodes()
                .filter(|node| node.kind() == NURBS_SURF)
                .map(|node| Some((node.index(), surface_descriptor(&node)?))),
        ),
        curve_descriptors: unique_records(
            primary
                .nodes()
                .filter(|node| node.kind() == NURBS_CURVE)
                .map(|node| Some((node.index(), curve_descriptor(&node)?))),
        ),
    }
}

/// Whether every B-spline node carries each field that the lanes read.
pub(super) fn has_lane_fields(primary: &PrimaryRecords<'_>) -> bool {
    primary.nodes().all(|node| {
        lane_fields(node.kind())
            .iter()
            .all(|name| node.field_range(name).is_some())
    })
}

fn lane_fields(kind: u16) -> &'static [&'static str] {
    match kind {
        B_SURFACE | B_CURVE => &["nurbs", "data"],
        NURBS_SURF => &[
            "u_periodic",
            "v_periodic",
            "u_degree",
            "v_degree",
            "n_u_vertices",
            "n_v_vertices",
            "u_knot_type",
            "v_knot_type",
            "n_u_knots",
            "n_v_knots",
            "rational",
            "vertex_dim",
            "bspline_vertices",
            "u_knot_mult",
            "v_knot_mult",
            "u_knots",
            "v_knots",
        ],
        NURBS_CURVE => &[
            "degree",
            "n_vertices",
            "vertex_dim",
            "n_knots",
            "knot_type",
            "periodic",
            "rational",
            "bspline_vertices",
            "knot_mult",
            "knots",
        ],
        KNOT_MULT => &["mult"],
        KNOT_SET => &["knots"],
        BSPLINE_VERTICES => &["vertices"],
        _ => &[],
    }
}

fn count(node: &Node<'_>, name: &str) -> Option<usize> {
    usize::try_from(node.integer(name)?)
        .ok()
        .filter(|count| *count > 0)
}

fn surface_descriptor(node: &Node<'_>) -> Option<SurfaceDescriptor> {
    let u_knot_type = node.byte("u_knot_type")?;
    let v_knot_type = node.byte("v_knot_type")?;
    (super::valid_knot_type(u_knot_type) && super::valid_knot_type(v_knot_type)).then_some(())?;
    Some(SurfaceDescriptor {
        u_degree: node.short("u_degree")?,
        v_degree: node.short("v_degree")?,
        u_count: count(node, "n_u_vertices")?,
        v_count: count(node, "n_v_vertices")?,
        u_periodic: node.logical("u_periodic")?,
        v_periodic: node.logical("v_periodic")?,
        u_knot_type,
        v_knot_type,
        u_distinct: count(node, "n_u_knots")?,
        v_distinct: count(node, "n_v_knots")?,
        u_mult: node.pointer("u_knot_mult")?,
        v_mult: node.pointer("v_knot_mult")?,
        u_knots: node.pointer("u_knots")?,
        v_knots: node.pointer("v_knots")?,
        vertices: Some(node.pointer("bspline_vertices")?),
        payload: None,
        vertex_layout: Some((node.short("vertex_dim")?, node.logical("rational")?)),
    })
}

fn curve_descriptor(node: &Node<'_>) -> Option<CurveDescriptor> {
    let dimension = node.short("vertex_dim")?;
    let knot_type = node.byte("knot_type")?;
    (matches!(dimension, 2..=4) && super::valid_knot_type(knot_type)).then_some(())?;
    let reference = |name: &str| NonNullXmt::try_from(node.pointer(name)?).ok();
    Some(CurveDescriptor {
        basis: CurveBasis {
            degree: node.short("degree")?,
            poles: count(node, "n_vertices")?,
            dimension,
            rational: Some(node.logical("rational")?),
            distinct: count(node, "n_knots")?,
            knot_type,
            periodic: node.logical("periodic")?,
        },
        vertices: Some(node.pointer("bspline_vertices")?),
        references: CurveDescriptorReferences::Status([
            reference("bspline_vertices")?,
            reference("knot_mult")?,
            reference("knots")?,
        ]),
    })
}

#[cfg(test)]
mod tests;
