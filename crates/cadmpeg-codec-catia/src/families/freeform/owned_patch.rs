// SPDX-License-Identifier: Apache-2.0
//! Bounded patches from source-owned faces and their boundaries.

mod edge_alignment;
mod extrusion;
mod roster;
mod ruled;
mod sheet_carrier;
mod sheet_group;
mod stations;
mod surface_fit;
mod transfer;
mod trim_curves;
mod trimmed_face;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::AnnotationStorage;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{self, admission::EvaluationAdmission};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::AnnotationBuilder;

use crate::families::b2::records::{b2_edge_nodes_from_records, b2_parameter_points_from_records};
use crate::families::FamilyEntityAdmission;
use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::records::{
    records_are_contiguous, ConsolidatedFamily as Family, ConsolidatedFrameFlag as Flag,
    ConsolidatedRecord, SourceExtent,
};

use super::rolling_ball_patch::fit_rectangular_patch;
use roster::{Member, RecordKey};

const CHART_TOLERANCE: f64 = 1.0e-9;
const GAP_MM: f64 = 0.001;
const MAX_FIT_CANDIDATES_FLOOR: usize = 512;
const RECORDS_PER_FIT_CANDIDATE: usize = 8;

struct Side {
    edge: usize,
    vertices: [usize; 2],
    uv: [[f64; 2]; 2],
}

struct Boundary {
    carrier: usize,
    sides: [Side; 4],
    u: [f64; 2],
    radius: f64,
}

#[derive(Clone, Copy)]
enum SheetSurfaceFit {
    RollingBall { max_rail_shift_mm: f64 },
    Offset,
    Ruled,
    Extrusion,
}

#[derive(Clone, Copy, Default)]
struct SheetBoundaryRecovery {
    rolling_extension_mm: f64,
    affine_vertex_shift_mm: f64,
}

struct Patch {
    recovery: SheetBoundaryRecovery,
    face: usize,
    body: RecordKey,
    forward: bool,
    sense_inferred: bool,
    surface_fit: Option<SheetSurfaceFit>,
    surface: SolvedSurfaceGeometry,
    priority: u8,
    sides: Vec<Use>,
    inner_starts: Vec<usize>,
}

#[derive(Clone, Copy)]
struct InferredEdgeAccuracy {
    source: usize,
    bound_mm: f64,
}

#[derive(Clone)]
struct Use {
    edge: usize,
    // Increasing curve parameter order.
    vertices: [(usize, Point3); 2],
    curve: NurbsCurve,
    pcurve: PcurveGeometry,
    forward: bool,
    inferred_accuracy: Option<InferredEdgeAccuracy>,
    lift_error_mm: f64,
}

fn payload<'a>(data: &'a [u8], record: &ConsolidatedRecord) -> Option<&'a [u8]> {
    data.get(record.payload()?)
}

fn resolve(
    records: &[ConsolidatedRecord],
    index: usize,
    reference: AllocationReference,
    family: Family,
    class: u8,
) -> Option<usize> {
    let value = usize::try_from(reference.value).ok()?;
    let target = match reference.encoding {
        AllocationReferenceEncoding::OwnedChild => index.checked_add(1)?.checked_add(value)?,
        AllocationReferenceEncoding::BackwardDistance if value != 0 => index.checked_sub(value)?,
        _ => return None,
    };
    let frame = records.get(target)?;
    (frame.family() == family
        && frame.class() == class
        && records_are_contiguous(records.get(index.min(target)..=index.max(target))?))
    .then_some(target)
}

fn bridge_radius(bytes: &[u8], sheet: bool) -> Option<(AllocationReference, f64)> {
    if bytes.first()? != &0x85 {
        return None;
    }
    let mut at = 1;
    let carrier = allocation_reference(bytes, &mut at)?;
    for _ in 0..4 {
        allocation_reference(bytes, &mut at)?;
    }
    let mut tail = View::over_retained(bytes.get(at..)?);
    if tail.take(2)? != [0x11, 0x05] {
        return None;
    }
    let radius = tail.f64_le()?;
    let controls = tail.take(2)?;
    if (!sheet && controls != [0x03, 0x05])
        || (sheet && !matches!(controls, [0x01 | 0x03 | 0x05, 0x03 | 0x05]))
        || tail.take(8)? != [0; 8]
        || tail.take(2)? != [0x01, 0x05]
        || tail.remaining() != 0
        || !radius.is_finite()
        || radius <= 0.0
    {
        return None;
    }
    Some((carrier, radius))
}

fn boundary(
    data: &[u8],
    records: &[ConsolidatedRecord],
    index: usize,
    resolve: impl Fn(usize, AllocationReference, Family, u8) -> Option<usize>,
    physical: impl Fn(usize) -> Option<usize>,
    sheet: bool,
) -> Option<Boundary> {
    let face = records.get(index)?;
    let bytes = payload(data, face)?;
    if bytes.first()? != &0x82 {
        return None;
    }
    let mut at = 1;
    let bridge = resolve(
        index,
        allocation_reference(bytes, &mut at)?,
        Family::B,
        0x37,
    )?;
    let owner = resolve(
        index,
        allocation_reference(bytes, &mut at)?,
        Family::B,
        0x62,
    )?;
    let terminal = bytes.get(at..)?;
    if (terminal != [0x05] && !(sheet && terminal == [0x03]))
        || records[owner].flag() != Flag::Flag03
    {
        return None;
    }
    let (carrier, radius) = bridge_radius(payload(data, &records[bridge])?, sheet)?;
    let carrier = resolve(bridge, carrier, Family::A, 0x32)?;
    let bytes = payload(data, &records[owner])?;
    if bytes.first()? != &0x89 {
        return None;
    }
    let mut at = 1;
    let mut sides = Vec::new();
    for _ in 0..4 {
        let line = resolve(
            owner,
            allocation_reference(bytes, &mut at)?,
            Family::B,
            0x18,
        )?;
        let edge = resolve(
            owner,
            allocation_reference(bytes, &mut at)?,
            Family::B,
            0x5e,
        )?;
        let uv = b2_parameter_points_from_records(data, &records[line..=line])
            .next()?
            .axis_line()?;
        let node = b2_edge_nodes_from_records(data, &records[edge..=edge]).next()?;
        let vertices = [
            (node.start_vertex_ref, node.reference_encodings[1]),
            (node.end_vertex_ref, node.reference_encodings[2]),
        ]
        .map(|(value, encoding)| {
            let vertex = resolve(
                edge,
                AllocationReference { value, encoding },
                Family::B,
                0x5d,
            )?;
            physical(records[vertex].byte_offset())
        });
        sides.push(Side {
            edge: physical(node.pos)?,
            vertices: [vertices[0]?, vertices[1]?],
            uv,
        });
    }
    if resolve(
        owner,
        allocation_reference(bytes, &mut at)?,
        Family::B,
        0x37,
    )? != bridge
    {
        return None;
    }
    let tail = bytes.get(at..)?;
    if sheet && tail.len() == 5 {
        if tail[0] != 0x84 || tail[4] != 1 {
            return None;
        }
        let mut u = [f64::INFINITY, f64::NEG_INFINITY];
        for point in sides.iter().flat_map(|side| side.uv) {
            u = [u[0].min(point[0]), u[1].max(point[0])];
        }
        if u[0] >= u[1] {
            return None;
        }
        return Some(Boundary {
            carrier: physical(records[carrier].byte_offset())?,
            sides: sides.try_into().ok()?,
            u,
            radius,
        });
    }
    if tail.len() != 62 || tail[..5] != [0x84, 0x41, 0xbb, 0x05, 0x0d] || tail[37] != 1 {
        return None;
    }
    let mut limits = View::over_retained(tail.get(5..37)?);
    let u_min = limits.f64_le()?;
    let v_min = limits.f64_le()?;
    let u_max = limits.f64_le()?;
    let v_max = limits.f64_le()?;
    if !u_min.is_finite()
        || !u_max.is_finite()
        || u_min >= u_max
        || !near(v_min, 0.0)
        || !near(v_max, radius)
    {
        return None;
    }
    Some(Boundary {
        carrier: physical(records[carrier].byte_offset())?,
        sides: sides.try_into().ok()?,
        u: [u_min, u_max],
        radius,
    })
}

fn interval_parameter([lo, hi]: [f64; 2], fraction: f64) -> f64 {
    if fraction == 0.0 {
        lo
    } else if fraction == 1.0 {
        hi
    } else {
        (hi - lo).mul_add(fraction, lo)
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= CHART_TOLERANCE
}

#[derive(Clone, Copy)]
struct SideSpec {
    curve: usize,
    vertices: [usize; 2],
    increasing: bool,
}

const CORNERS: [[usize; 2]; 4] = [[0, 0], [1, 0], [1, 1], [0, 1]];
const ISOCURVE_CORNERS: [[usize; 2]; 4] = [[0, 1], [1, 2], [3, 2], [0, 3]];

fn line_pcurve([lo, hi]: [f64; 2], ends: [[f64; 2]; 2]) -> Option<PcurveGeometry> {
    let nurbs = PcurveNurbs::from_lanes(
        1,
        vec![lo, lo, hi, hi],
        ends.map(|[u, v]| Point2::new(u, v)).to_vec(),
        None,
        false,
    )
    .ok()?;
    Some(PcurveGeometry::Nurbs { nurbs })
}

fn rectangle(boundary: &Boundary) -> Option<([SideSpec; 4], bool)> {
    let corner = |uv: [f64; 2]| {
        CORNERS.iter().position(|&[u, v]| {
            near(uv[0], boundary.u[u]) && near(uv[1], [0.0, boundary.radius][v])
        })
    };
    let uv = boundary.sides.each_ref().map(|side| side.uv.map(corner));
    let edges = boundary.sides.each_ref().map(|side| side.vertices);
    let uv = uv.map(|[a, b]| a.zip(b).map(|(a, b)| [a, b]));
    let uv = [uv[0]?, uv[1]?, uv[2]?, uv[3]?];
    let (vertex_order, _) = cycle(edges)?;
    let (corner_order, corner_forward) = cycle(uv)?;
    let mut specs = [SideSpec {
        curve: 0,
        vertices: [0, 0],
        increasing: false,
    }; 4];
    let mut sides = BTreeSet::new();
    for i in 0..4 {
        let j = (i + 1) % 4;
        let a = corner_order[i];
        let b = corner_order[j];
        if !(a.abs_diff(b) == 1 || a.abs_diff(b) == 3) {
            return None;
        }
        let side = if (a + 1) % 4 == b { a } else { b };
        if !sides.insert(side) {
            return None;
        }
        // Upper and minimum-U isocurves increase opposite to the corner order.
        let increasing = ((a + 1) % 4 == b) == (side < 2);
        let (start, end) = if increasing { (i, j) } else { (j, i) };
        specs[i] = SideSpec {
            curve: side,
            vertices: [vertex_order[start], vertex_order[end]],
            increasing,
        };
    }
    Some((specs, corner_forward))
}

fn cycle(edges: [[usize; 2]; 4]) -> Option<([usize; 4], bool)> {
    let mut solution = None;
    for reverse in [false, true] {
        let start = edges[0][usize::from(reverse)];
        let mut vertex = start;
        let mut order = [0; 4];
        for (slot, [a, b]) in edges.into_iter().enumerate() {
            order[slot] = vertex;
            vertex = match (a == vertex, b == vertex) {
                (true, false) => b,
                (false, true) => a,
                _ => break,
            };
            if slot == 3
                && vertex == start
                && order.iter().copied().collect::<BTreeSet<_>>().len() == 4
            {
                if solution.is_some() {
                    return None;
                }
                solution = Some((order, (order[0] + 1) % 4 == order[1]));
            }
        }
    }
    solution
}

fn fit(
    ir: &CadIr,
    surface: usize,
    face: usize,
    member: Member,
    boundary: &Boundary,
) -> Option<Patch> {
    let (specs, loop_forward) = rectangle(boundary)?;
    let definition = rolling_ball(ir, surface, boundary.radius)?;
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return None;
    };
    let stations = jet.stations();
    if boundary.u[0] < stations.first()?.knot.get() || boundary.u[1] > stations.last()?.knot.get() {
        return None;
    }
    let fitted = fit_rectangular_patch(definition, boundary.u)?;
    let curves = [
        fitted.lower_curve,
        fitted.max_curve,
        fitted.upper_curve,
        fitted.min_curve,
    ];
    let mut sides = Vec::new();
    for (edge, spec) in boundary.sides.iter().zip(specs) {
        let curve = curves[spec.curve].clone();
        let [lo, hi] = curve.full_knot_endpoints().endpoints();
        let [start, end] = spec.vertices;
        let ends = ISOCURVE_CORNERS[spec.curve].map(|index| {
            let [u, v] = CORNERS[index];
            [boundary.u[u], [0.0, 1.0][v]]
        });
        sides.push(Use {
            edge: edge.edge,
            vertices: [
                (start, curve_point(&curve, lo)?),
                (end, curve_point(&curve, hi)?),
            ],
            forward: spec.increasing == (loop_forward == member.forward),
            curve,
            pcurve: line_pcurve([lo, hi], ends)?,
            inferred_accuracy: None,
            lift_error_mm: 0.0,
        });
    }
    if loop_forward != member.forward {
        sides.reverse();
    }
    Some(Patch {
        face,
        body: member.body,
        forward: member.forward,
        sense_inferred: false,
        surface_fit: Some(SheetSurfaceFit::RollingBall {
            max_rail_shift_mm: 0.0,
        }),
        surface: SolvedSurfaceGeometry::Nurbs(fitted.surface),
        priority: 0,
        sides,
        inner_starts: Vec::new(),
        recovery: SheetBoundaryRecovery::default(),
    })
}

fn curve_point(curve: &NurbsCurve, t: f64) -> Option<Point3> {
    eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, curve, t)
        .ok()
        .map(FinitePoint3::get)
}

fn rolling_ball(ir: &CadIr, surface: usize, radius: f64) -> Option<&ProceduralSurfaceDefinition> {
    let geometry = &ir.model.surfaces.get(surface)?.geometry;
    let SurfaceGeometry::Procedural {
        construction,
        cache: None,
    } = geometry
    else {
        return None;
    };
    let procedure = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|p| &p.id == construction)?;
    matches_radius(procedure.definition(), radius).then_some(procedure.definition())
}

fn matches_radius(definition: &ProceduralSurfaceDefinition, radius: f64) -> bool {
    let ProceduralSurfaceDefinition::RollingBallJet(jet) = definition else {
        return false;
    };
    jet.stations().iter().all(|station| {
        near(
            station.site.first_limit.distance(station.site.center.get()),
            radius,
        )
    })
}

pub(super) struct Stream<'a> {
    pub(super) data: &'a [u8],
    pub(super) sources: &'a [Vec<SourceExtent>],
    pub(super) records: &'a [ConsolidatedRecord],
}

pub(super) fn append(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl AnnotationStorage>,
    stream: &Stream<'_>,
    carriers: &BTreeMap<usize, usize>,
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<Option<LossNote>, CodecError> {
    let &Stream {
        data,
        sources,
        records,
    } = stream;
    if carriers.is_empty()
        && !records
            .iter()
            .any(|frame| sheet_group::root(frame, data) || sheet_group::backward_root(frame, data))
    {
        return Ok(None);
    }
    let (mut patches, backward_sources) =
        sheet_group::patches(admission.context(), ir, stream, carriers, refusal)?;
    let members = roster::members(
        admission.context(),
        data,
        sources,
        records,
        &backward_sources,
    )?;
    let fit_limit = MAX_FIT_CANDIDATES_FLOOR.max(records.len().div_ceil(RECORDS_PER_FIT_CANDIDATE));
    let fit_budget = WorkBudget::new(fit_limit);
    for (index, frame) in records.iter().enumerate() {
        let key = (frame.source_index(), frame.source_range().start);
        if frame.class() != 0x5f || frame.family() != Family::B || frame.flag() != Flag::Flag03 {
            continue;
        }
        let Some(&member) = members.get(&key) else {
            continue;
        };
        let Some(boundary) = boundary(
            data,
            records,
            index,
            |index, reference, family, class| resolve(records, index, reference, family, class),
            Some,
            false,
        ) else {
            continue;
        };
        let Some(&surface) = carriers.get(&boundary.carrier) else {
            continue;
        };
        if !fit_budget.charge() {
            refusal.push_sheet_patch_search_bounded("owned face fitting", fit_limit, "candidates");
            break;
        }
        if let Some(patch) = fit(ir, surface, frame.byte_offset(), member, &boundary) {
            patches.push(patch);
        }
    }
    transfer::append(ir, annotations, patches, refusal, admission)
}

#[cfg(test)]
mod tests;
