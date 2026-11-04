// SPDX-License-Identifier: Apache-2.0
//! NURBS curve and surface carriers read from the node sequence of a
//! partition or plain stream (`parasolid.md` §2, `sldprt.md` §7.2).

use super::{
    assemble_curve, assemble_pcurve, assemble_surface, Arrays, CurveCarrier, CurveDescriptor,
    SurfaceCarrier, SurfaceDescriptor,
};
use cadmpeg_core::decode::index_from_u32;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_parasolid::records::node_type::{
    BSPLINE_VERTICES, B_CURVE, B_SURFACE, KNOT_MULT, KNOT_SET, NURBS_CURVE, NURBS_SURF,
};
use cadmpeg_parasolid::records::primary::Node;
use cadmpeg_parasolid::records::PrimaryRecords;
use std::collections::{HashMap, HashSet};

/// The NURBS carriers of one node sequence, keyed by `B_CURVE` and `B_SURFACE`
/// index. `pcurves` holds the parameter-space `B_CURVE`s.
#[derive(Default)]
pub(in crate::brep) struct NodeSplines {
    pub(in crate::brep) curves: HashMap<u32, CurveCarrier>,
    pub(in crate::brep) surfaces: HashMap<u32, SurfaceCarrier>,
    pub(in crate::brep) pcurves: HashMap<u32, PcurveNurbs>,
}

/// The NURBS carriers of a node sequence whose body starts at `body_offset`,
/// when every spline node carries its base fields and no two spline nodes
/// share an index. Offsets are body-relative. A descriptor outside the guards
/// of its compact form, or a carrier whose arrays do not assemble, is omitted;
/// lane refusals are recorded in `refusals`.
pub(in crate::brep) fn carriers(
    records: &PrimaryRecords<'_>,
    body_offset: usize,
    refusals: &mut Vec<LossNote>,
) -> Option<NodeSplines> {
    let mut arrays = Arrays {
        f64s: HashMap::new(),
        u16s: HashMap::new(),
        compact: HashMap::new(),
    };
    let mut curve_descriptors = HashMap::new();
    let mut surface_descriptors = HashMap::new();
    let mut indices = HashSet::new();
    let spline = |kind| {
        matches!(
            kind,
            BSPLINE_VERTICES
                | B_SURFACE
                | NURBS_SURF
                | KNOT_MULT
                | KNOT_SET
                | B_CURVE
                | NURBS_CURVE
        )
    };
    for node in records.nodes().filter(|node| spline(node.kind())) {
        indices.insert(node.index()).then_some(())?;
        match node.kind() {
            BSPLINE_VERTICES => {
                arrays.f64s.insert(node.index(), node.reals("vertices")?);
            }
            KNOT_SET => {
                arrays.f64s.insert(node.index(), node.reals("knots")?);
            }
            KNOT_MULT => {
                arrays.u16s.insert(node.index(), node.shorts("mult")?);
            }
            NURBS_CURVE => {
                let (descriptor, admitted) = curve_descriptor(&node)?;
                if admitted {
                    curve_descriptors.insert(node.index(), descriptor);
                }
            }
            NURBS_SURF => {
                let (descriptor, admitted) = surface_descriptor(&node, body_offset)?;
                if admitted {
                    surface_descriptors.insert(node.index(), descriptor);
                }
            }
            _ => {}
        }
    }
    let mut out = NodeSplines::default();
    for node in records.nodes() {
        let attr = node.index();
        let offset = node.range().start.checked_sub(body_offset)?;
        match node.kind() {
            B_CURVE => {
                let Some(descriptor) = curve_descriptors.get(&node.pointer("nurbs")?) else {
                    continue;
                };
                if !descriptor.spatial() {
                    if let Some(pcurve) = assemble_pcurve(&arrays, descriptor) {
                        out.pcurves.insert(attr, pcurve);
                    }
                } else if let Some(carrier) =
                    assemble_curve(&arrays, attr, offset, descriptor, refusals)
                {
                    out.curves.insert(attr, carrier);
                }
            }
            B_SURFACE => {
                let Some(descriptor) = surface_descriptors.get(&node.pointer("nurbs")?) else {
                    continue;
                };
                if let Some(carrier) =
                    assemble_surface(&[], &arrays, attr, offset, descriptor, refusals)
                {
                    out.surfaces.insert(attr, carrier);
                }
            }
            _ => {}
        }
    }
    Some(out)
}

/// A `NURBS_CURVE` descriptor, and whether it is a model-space or
/// parameter-space curve inside the guards of its compact form.
fn curve_descriptor(node: &Node<'_>) -> Option<(CurveDescriptor, bool)> {
    let descriptor = CurveDescriptor {
        degree: u32::from(node.short("degree")?),
        periodic: node.logical("periodic")?,
        control_count: index_from_u32(node.integer("n_vertices")?),
        dimension: usize::from(node.short("vertex_dim")?),
        rational: node.logical("rational")?,
        control_attr: node.pointer("bspline_vertices")?,
        multiplicity_attr: node.pointer("knot_mult")?,
        knot_attr: node.pointer("knots")?,
    };
    let admitted = (2..=4).contains(&descriptor.dimension)
        && descriptor.control_count > 0
        && (descriptor.spatial() || descriptor.planar());
    Some((descriptor, admitted))
}

/// A `NURBS_SURF` descriptor, and whether it is inside the guards of its
/// compact form.
fn surface_descriptor(node: &Node<'_>, body_offset: usize) -> Option<(SurfaceDescriptor, bool)> {
    let descriptor = SurfaceDescriptor {
        end: node.range().end.checked_sub(body_offset)?,
        attr: node.index(),
        u_periodic: node.logical("u_periodic")?,
        v_periodic: node.logical("v_periodic")?,
        u_degree: u32::from(node.short("u_degree")?),
        v_degree: u32::from(node.short("v_degree")?),
        u_count: index_from_u32(node.integer("n_u_vertices")?),
        v_count: index_from_u32(node.integer("n_v_vertices")?),
        u_knot_count: index_from_u32(node.integer("n_u_knots")?),
        v_knot_count: index_from_u32(node.integer("n_v_knots")?),
        rational: node.logical("rational")?,
        dimension: usize::from(node.short("vertex_dim")?),
        refs: node.pointers([
            "bspline_vertices",
            "u_knot_mult",
            "v_knot_mult",
            "u_knots",
            "v_knots",
        ])?,
    };
    let degree = |value: u32| usize::try_from(value).ok();
    let admitted = descriptor.u_degree > 0
        && descriptor.v_degree > 0
        && degree(descriptor.u_degree).is_some_and(|value| descriptor.u_count > value)
        && degree(descriptor.v_degree).is_some_and(|value| descriptor.v_count > value)
        && descriptor.u_knot_count > 0
        && descriptor.v_knot_count > 0
        && matches!(descriptor.dimension, 3 | 4)
        && descriptor.rational == (descriptor.dimension == 4)
        && descriptor.refs.iter().all(|&reference| reference > 1);
    Some((descriptor, admitted))
}

#[cfg(test)]
mod tests;
