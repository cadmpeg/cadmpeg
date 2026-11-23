// SPDX-License-Identifier: Apache-2.0
//! Kernel carrier projection through the shared Parasolid readers.

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_parasolid::geometry::nurbs::{self, BCurve, CurveFields, Knots, SurfaceFields};
use cadmpeg_parasolid::geometry::{self, Analytic};
use cadmpeg_parasolid::records::node_type as kind;
use cadmpeg_parasolid::records::primary::Node;

pub(crate) type Nodes<'a> = BTreeMap<u32, Node<'a>>;

pub(crate) fn analytic(node: Node<'_>) -> Option<Analytic> {
    let mut values = Vec::new();
    let vectors: &[&str] = match node.kind() {
        kind::LINE => &["pvec", "direction"],
        kind::PLANE => &["pvec", "normal", "x_axis"],
        kind::CIRCLE | kind::ELLIPSE => &["centre", "normal", "x_axis"],
        kind::CYLINDER | kind::CONE => &["pvec", "axis"],
        kind::SPHERE => &["centre"],
        kind::TORUS => &["centre", "axis"],
        _ => return None,
    };
    for name in vectors {
        values.extend(node.vector(name)?);
    }
    match node.kind() {
        kind::CIRCLE => values.push(node.real("radius")?),
        kind::ELLIPSE => {
            values.push(node.real("major_radius")?);
            values.push(node.real("minor_radius")?);
        }
        kind::CYLINDER => {
            values.push(node.real("radius")?);
            values.extend(node.vector("x_axis")?);
        }
        kind::CONE => {
            values.push(node.real("radius")?);
            values.push(node.real("sin_half_angle")?);
            values.push(node.real("cos_half_angle")?);
            values.extend(node.vector("x_axis")?);
        }
        kind::SPHERE => {
            values.push(node.real("radius")?);
            values.extend(node.vector("axis")?);
            values.extend(node.vector("x_axis")?);
        }
        kind::TORUS => {
            values.push(node.real("major_radius")?);
            values.push(node.real("minor_radius")?);
            values.extend(node.vector("x_axis")?);
        }
        _ => {}
    }
    geometry::analytic(node.kind(), &values).ok()
}

fn referenced<'a>(nodes: &Nodes<'a>, node: Node<'_>, field: &str) -> Option<Node<'a>> {
    nodes.get(&node.pointer(field)?).copied()
}

/// The curve a trimmed-curve chain restricts: follows `basis_curve` links
/// through nested trimmed curves and returns the first other node. A node that
/// is not a trimmed curve is its own basis. A dangling link or a cycle ends at
/// the last resolved node.
pub(crate) fn trim_basis<'a>(nodes: &Nodes<'a>, node: Node<'a>) -> Node<'a> {
    let mut current = node;
    let mut seen = std::collections::BTreeSet::new();
    while current.kind() == kind::TRIMMED_CURVE && seen.insert(current.index()) {
        match referenced(nodes, current, "basis_curve") {
            Some(basis) => current = basis,
            None => break,
        }
    }
    current
}

pub(crate) fn curve(nodes: &Nodes<'_>, node: Node<'_>) -> Option<CurveGeometry> {
    if let Some(Analytic::Curve(curve)) = analytic(node) {
        return Some(curve);
    }
    if node.kind() == kind::INTERSECTION {
        return crate::intersection::curve(nodes, node).ok();
    }
    if node.kind() == kind::TRIMMED_CURVE {
        let basis = referenced(nodes, node, "basis_curve")?;
        if basis.kind() != kind::TRIMMED_CURVE {
            return curve(nodes, basis);
        }
        return None;
    }
    match b_curve(nodes, node)? {
        BCurve::Spatial(curve) => Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))),
        BCurve::Planar(_) => None,
    }
}

/// The B-spline carried by a `B_CURVE` node: a model-space curve, or a curve
/// in the parameter space of a support surface.
pub(crate) fn b_curve(nodes: &Nodes<'_>, node: Node<'_>) -> Option<BCurve> {
    if node.kind() != kind::B_CURVE {
        return None;
    }
    let data = referenced(nodes, node, "nurbs")?;
    let vertices = referenced(nodes, data, "bspline_vertices")?.reals("vertices")?;
    let mult = referenced(nodes, data, "knot_mult")?.shorts("mult")?;
    let knots = referenced(nodes, data, "knots")?.reals("knots")?;
    let fields = CurveFields {
        degree: u32::from(data.short("degree")?),
        vertex_count: usize::try_from(data.integer("n_vertices")?).ok()?,
        vertex_dim: usize::from(data.short("vertex_dim")?),
        rational: data.logical("rational")?,
        periodic: data.logical("periodic")?,
        vertices: &vertices,
        knots: Knots {
            count: usize::try_from(data.integer("n_knots")?).ok()?,
            multiplicities: &mult,
            values: &knots,
        },
    };
    nurbs::b_curve(&fields).ok()
}

pub(crate) fn surface(nodes: &Nodes<'_>, node: Node<'_>) -> Option<SurfaceGeometry> {
    if let Some(Analytic::Surface { geometry, .. }) = analytic(node) {
        return Some(geometry);
    }
    if node.kind() != kind::B_SURFACE {
        return None;
    }
    let data = referenced(nodes, node, "nurbs")?;
    let vertices = referenced(nodes, data, "bspline_vertices")?.reals("vertices")?;
    let u_mult = referenced(nodes, data, "u_knot_mult")?.shorts("mult")?;
    let v_mult = referenced(nodes, data, "v_knot_mult")?.shorts("mult")?;
    let u_knots = referenced(nodes, data, "u_knots")?.reals("knots")?;
    let v_knots = referenced(nodes, data, "v_knots")?.reals("knots")?;
    let fields = SurfaceFields {
        degrees: [
            u32::from(data.short("u_degree")?),
            u32::from(data.short("v_degree")?),
        ],
        vertex_counts: [
            usize::try_from(data.integer("n_u_vertices")?).ok()?,
            usize::try_from(data.integer("n_v_vertices")?).ok()?,
        ],
        vertex_dim: usize::from(data.short("vertex_dim")?),
        rational: data.logical("rational")?,
        periodic: [data.logical("u_periodic")?, data.logical("v_periodic")?],
        vertices: &vertices,
        knots: [
            Knots {
                count: usize::try_from(data.integer("n_u_knots")?).ok()?,
                multiplicities: &u_mult,
                values: &u_knots,
            },
            Knots {
                count: usize::try_from(data.integer("n_v_knots")?).ok()?,
                multiplicities: &v_mult,
                values: &v_knots,
            },
        ],
    };
    nurbs::b_surface(&fields, false)
        .ok()
        .map(|surface| SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)))
}

#[cfg(test)]
mod tests {
    use super::{trim_basis, Nodes};
    use cadmpeg_parasolid::records::{node_type as kind, PrimaryRecords};

    #[test]
    fn trimmed_curve_chain_resolves_to_its_basis_curve() {
        let bytes = crate::test_support::trimmed_chart_partition();
        let records = PrimaryRecords::read(&bytes)
            .expect("header")
            .expect("partition");
        assert!(records.is_terminated());
        let nodes: Nodes<'_> = records.nodes().map(|node| (node.index(), node)).collect();
        let chain = nodes[&9];
        assert_eq!(chain.kind(), kind::TRIMMED_CURVE);
        assert_eq!(trim_basis(&nodes, chain).kind(), kind::INTERSECTION);
        assert_eq!(trim_basis(&nodes, nodes[&8]).index(), 2);
        assert_eq!(trim_basis(&nodes, nodes[&2]).index(), 2);
    }

    #[test]
    fn trimmed_curve_cycle_and_dangling_basis_stop_at_a_trimmed_curve() {
        let bytes = crate::test_support::trimmed_chart_partition();
        let records = PrimaryRecords::read(&bytes)
            .expect("header")
            .expect("partition");
        let nodes: Nodes<'_> = records.nodes().map(|node| (node.index(), node)).collect();
        assert_eq!(trim_basis(&nodes, nodes[&10]).kind(), kind::TRIMMED_CURVE);
        assert_eq!(trim_basis(&nodes, nodes[&11]).kind(), kind::TRIMMED_CURVE);
    }
}
