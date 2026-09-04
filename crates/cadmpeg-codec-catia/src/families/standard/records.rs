//! Standard-nested `SurfacicReps` record decoders.
//!
//! Decodes per-face analytic surface records, plane bounds, the `0x60`
//! curve-support/edge-incidence table, standard vertex rosters, and the
//! inline big-endian curved-surface parameter block.

use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assemble::unit_vector;
use crate::families::standard::fbb::FbbPopulationLayout;
use crate::layout::analytic_surface_cone as analytic_cone;
use crate::layout::analytic_surface_cylinder as analytic_cylinder;
use crate::layout::analytic_surface_plane as analytic_plane;
use crate::layout::analytic_surface_sphere as analytic_sphere;
use crate::layout::analytic_surface_torus as analytic_torus;
use crate::layout::freeform_surface_core as freeform_core;
use crate::layout::vertex_roster_row as vertex_roster;

/// Binary32 multiplication and addition can leave a unit XY direction just
/// below the unit circle.  Treat that deficit as roundoff instead of creating
/// a false binary64 Z component when the carrier stores only the Z sign.
const F32_UNIT_NORM2_ROUNDING_TOLERANCE: f64 = 4.0 * (f32::EPSILON as f64);

/// The standard-nested plane bounds record. Its three-byte tag is the bridge to
/// the matching `SurfacicReps` plane marker.
#[derive(Debug, Clone)]
pub struct PlaneParams {
    /// The little-endian u24 carrier tag.
    pub target: u32,
    /// Bounding-sphere center, which lies on the plane and fixes its origin.
    pub origin: Point3,
    /// Unit plane normal from the positionally paired trim packet.
    pub normal: Vector3,
}

/// The `00 33 <kind>` surface kinds and their required strict-template prebyte
/// (the byte at `marker_pos - 1`), which filters collisional signature matches.
fn kind_prebyte(kind: u8) -> Option<u8> {
    match kind {
        0x32 => Some(0x02), // plane
        0x33 => Some(0x1a), // cylinder
        0x34 => Some(0x1a), // cone
        0x35 => Some(0x12), // sphere
        0x38 => Some(0x1e), // torus
        _ => None,
    }
}

/// A located per-face analytic surface record.
#[derive(Debug, Clone)]
pub struct SurfacePrefix {
    /// Offset of the `00 33 <kind>` signature within the BREP stream.
    pub pos: usize,
    /// The little-endian u24 tag that identifies this carrier.
    pub target: u32,
    /// The kind byte (`0x32`..=`0x38`).
    pub kind: u8,
}

/// One face-local record in the standard `SurfacicReps` surface roster.
#[derive(Debug, Clone)]
pub enum StandardSurfaceRecord {
    /// Fixed-length analytic carrier record.
    Analytic(SurfacePrefix),
    /// Face bounds and orientation for a carrier linked through an outer alias.
    Freeform {
        /// Record byte offset.
        pos: usize,
        /// Little-endian u24 carrier tag.
        tag: u32,
        /// Trimmed-face spatial bounds stored in the roster core.
        bounds: StandardFaceBounds,
        /// Face orientation relative to the linked carrier.
        forward: bool,
    },
}

/// Spatial bounds stored by one standard face roster core.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StandardFaceBounds {
    /// Axis-aligned bounding-box centre.
    pub aabb_center: [f64; 3],
    /// Non-negative axis-aligned bounding-box half-extents.
    pub aabb_half_extents: [f64; 3],
    /// Bounding-sphere centre.
    pub sphere_center: [f64; 3],
    /// Non-negative bounding-sphere radius.
    pub sphere_radius: f64,
}

fn face_bounds_at(brep: &[u8], position: usize) -> Option<StandardFaceBounds> {
    let values = (0..10)
        .map(|index| f32_le(brep, position + 4 * index))
        .collect::<Vec<_>>();
    if values.iter().any(|value| !value.is_finite())
        || values[3..6].iter().any(|extent| *extent < 0.0)
        || values[9] < 0.0
    {
        return None;
    }
    if (0..3).any(|axis| {
        let containment_error = (f64::from(values[axis]) - f64::from(values[6 + axis])).abs()
            + f64::from(values[3 + axis])
            - f64::from(values[9]);
        let rounding_slack = MAX_F32_CONTAINMENT_ULPS
            * [values[axis], values[6 + axis], values[3 + axis], values[9]]
                .into_iter()
                .map(f32_ulp)
                .fold(0.0, f64::max);
        containment_error > rounding_slack
    }) {
        return None;
    }
    Some(StandardFaceBounds {
        aabb_center: [
            f64::from(values[0]),
            f64::from(values[1]),
            f64::from(values[2]),
        ],
        aabb_half_extents: [
            f64::from(values[3]),
            f64::from(values[4]),
            f64::from(values[5]),
        ],
        sphere_center: [
            f64::from(values[6]),
            f64::from(values[7]),
            f64::from(values[8]),
        ],
        sphere_radius: f64::from(values[9]),
    })
}

/// Maximum per-coordinate mismatch admitted for independently computed
/// binary32 bounds before the bounds are considered malformed.
const MAX_F32_CONTAINMENT_ULPS: f64 = 3.0;

/// Return the spacing between adjacent finite binary32 values at `value`.
fn f32_ulp(value: f32) -> f64 {
    let exponent = (value.abs().to_bits() >> 23) & 0xff;
    if exponent == 0 {
        f64::from(f32::from_bits(1))
    } else {
        2.0_f64.powi(exponent as i32 - 127 - 23)
    }
}

/// Read the spatial bounds of one complete face-local surface record.
#[must_use]
pub fn standard_face_bounds(
    brep: &[u8],
    record: &StandardSurfaceRecord,
) -> Option<StandardFaceBounds> {
    match record {
        StandardSurfaceRecord::Freeform { bounds, .. } => Some(*bounds),
        StandardSurfaceRecord::Analytic(prefix) => {
            let relative = match prefix.kind {
                0x32 => 3,
                0x33 | 0x34 => 27,
                0x35 => 19,
                0x38 => 31,
                _ => return None,
            };
            face_bounds_at(brep, prefix.pos + relative).filter(|bounds| bounds.sphere_radius > 0.0)
        }
    }
}

impl StandardSurfaceRecord {
    fn pos(&self) -> usize {
        match self {
            Self::Analytic(prefix) => prefix.pos - analytic_plane::MARKER,
            Self::Freeform { pos, .. } => *pos,
        }
    }

    fn end(&self) -> usize {
        match self {
            Self::Analytic(prefix) => {
                self.pos()
                    + match prefix.kind {
                        0x32 => analytic_plane::LEN,
                        0x33 => analytic_cylinder::LEN,
                        0x34 => analytic_cone::LEN,
                        0x35 => analytic_sphere::LEN,
                        0x38 => analytic_torus::LEN,
                        _ => unreachable!("analytic roster kinds are filtered"),
                    }
            }
            Self::Freeform { pos, .. } => pos + freeform_core::LEN,
        }
    }
}

struct StandardSurfaceRecordTable {
    records: Vec<StandardSurfaceRecord>,
    successors: Vec<Option<usize>>,
}

fn standard_surface_record_table(brep: &[u8]) -> StandardSurfaceRecordTable {
    let mut records = BTreeMap::<usize, StandardSurfaceRecord>::new();
    for prefix in surface_prefixes(brep) {
        if face_sense(brep, &prefix).is_some() {
            records.insert(
                prefix.pos - analytic_plane::MARKER,
                StandardSurfaceRecord::Analytic(prefix),
            );
        }
    }
    let analytic_ranges = records
        .values()
        .filter_map(|record| match record {
            StandardSurfaceRecord::Analytic(prefix) => {
                Some((prefix.pos - analytic_plane::MARKER, record.end()))
            }
            StandardSurfaceRecord::Freeform { .. } => None,
        })
        .collect::<Vec<_>>();
    let mut next_analytic = analytic_ranges.iter().copied().peekable();
    for pos in 0..brep.len().saturating_sub(freeform_core::SIGN) {
        if brep.get(pos + freeform_core::ZERO_RUN..pos + freeform_core::BOUNDS) != Some(&[0, 0, 0])
        {
            continue;
        }
        while next_analytic
            .peek()
            .is_some_and(|(_, analytic_end)| *analytic_end <= pos)
        {
            next_analytic.next();
        }
        if next_analytic
            .peek()
            .is_some_and(|(analytic_start, _)| *analytic_start < pos + freeform_core::LEN)
        {
            continue;
        }
        let tag = u24_le(brep, pos);
        let forward = match brep[pos + freeform_core::SIGN] {
            0x01 => true,
            0xff => false,
            _ => continue,
        };
        let Some(bounds) = face_bounds_at(brep, pos + freeform_core::BOUNDS) else {
            continue;
        };
        if tag == 0 {
            continue;
        }
        records.insert(
            pos,
            StandardSurfaceRecord::Freeform {
                pos,
                tag,
                bounds,
                forward,
            },
        );
    }

    let records = records.into_values().collect::<Vec<_>>();
    let record_indices = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.pos(), index))
        .collect::<HashMap<_, _>>();
    let successors = records
        .iter()
        .map(|record| record_indices.get(&record.end()).copied())
        .collect();
    StandardSurfaceRecordTable {
        records,
        successors,
    }
}

/// Return every surface roster chain that ends directly at a complete `0x60`
/// support table. Each chain is a source-closed face population; records that
/// cannot reach that boundary are not assigned to a population.
#[must_use]
pub fn standard_surface_record_groups(brep: &[u8]) -> Vec<Vec<StandardSurfaceRecord>> {
    let table = standard_surface_record_table(brep);
    let mut has_predecessor = vec![false; table.records.len()];
    for successor in table.successors.iter().flatten() {
        has_predecessor[*successor] = true;
    }
    table
        .records
        .iter()
        .enumerate()
        .filter(|(start, _)| !has_predecessor[*start])
        .filter_map(|(start, _)| {
            let mut current = Some(start);
            let mut group = Vec::new();
            while let Some(index) = current {
                group.push(table.records[index].clone());
                current = table.successors[index];
            }
            let last = group.last()?;
            (brep.get(last.end()) == Some(&0x60)).then_some(group)
        })
        .collect()
}

/// One surface roster and its positionally following, face-local support
/// table. Support face references remain local to this population.
#[derive(Debug, Clone)]
pub struct StandardSurfacePopulation {
    /// The source-closed face-local surface roster.
    pub records: Vec<StandardSurfaceRecord>,
    /// The source-closed `0x60` edge-support roster.
    pub supports: Vec<StandardCurveSupport>,
}

/// Return every source-closed surface/support population with valid local
/// face references. No population is selected by row count or allocation
/// order.
#[must_use]
pub fn standard_surface_populations(brep: &[u8]) -> Vec<StandardSurfacePopulation> {
    standard_surface_record_groups(brep)
        .into_iter()
        .filter_map(|records| {
            let support_start = records.last()?.end();
            let supports = standard_curve_supports_at(brep, records.len(), support_start)?;
            Some(StandardSurfacePopulation { records, supports })
        })
        .collect()
}

/// Pair source-ordered, source-closed FBB layouts with source-ordered,
/// source-closed surface/support populations. The relation is admitted only
/// when both lanes have the same population count and every local face and
/// edge cardinality agrees. Allocation order and a repeated count key never
/// select a population.
#[must_use]
pub(crate) fn pair_standard_populations(
    layouts: &[FbbPopulationLayout],
    populations: &[StandardSurfacePopulation],
) -> Option<Vec<(FbbPopulationLayout, StandardSurfacePopulation)>> {
    if layouts.is_empty() || layouts.len() != populations.len() {
        return None;
    }
    layouts
        .iter()
        .copied()
        .zip(populations.iter().cloned())
        .map(|(layout, population)| {
            (layout.face_count == population.records.len()
                && layout.edge_count == population.supports.len())
            .then_some((layout, population))
        })
        .collect()
}

/// Walk the complete face-local surface roster. Records are accepted only as a
/// unique contiguous chain of `face_count` non-overlapping entries terminated
/// by the first curve-support row. A byte pattern inside an analytic payload
/// cannot create a competing freeform record.
#[must_use]
pub fn standard_surface_records(
    brep: &[u8],
    face_count: usize,
) -> Option<Vec<StandardSurfaceRecord>> {
    let table = standard_surface_record_table(brep);
    if face_count == 0 || face_count > table.records.len() {
        return None;
    }
    let ordered_records = &table.records;
    let successors = &table.successors;
    let remaining_steps = face_count - 1;
    let level_count = usize::BITS as usize - remaining_steps.leading_zeros() as usize;
    let mut jumps = Vec::new();
    if level_count > 0 {
        jumps.push(successors.clone());
    }
    while jumps.len() < level_count {
        let previous = jumps.last().expect("one lower jump level");
        jumps.push(
            previous
                .iter()
                .map(|next| next.and_then(|middle| previous[middle]))
                .collect(),
        );
    }

    let mut solution_start = None;
    for start in 0..ordered_records.len() {
        let mut current = Some(start);
        let mut steps = remaining_steps;
        let mut level = 0;
        while steps != 0 {
            if steps & 1 != 0 {
                current = current.and_then(|index| jumps[level][index]);
            }
            steps >>= 1;
            level += 1;
        }
        let Some(last) = current else {
            continue;
        };
        if brep.get(ordered_records[last].end()) == Some(&0x60)
            && solution_start.replace(start).is_some()
        {
            return None;
        }
    }

    let mut current = solution_start?;
    let mut chain = Vec::with_capacity(face_count);
    for ordinal in 0..face_count {
        chain.push(ordered_records[current].clone());
        if ordinal + 1 < face_count {
            current = successors[current]?;
        }
    }
    Some(chain)
}

/// Read the trailing per-face orientation byte from a complete analytic
/// `SurfacicReps` record. `true` means the face follows the carrier normal.
pub fn face_sense(brep: &[u8], prefix: &SurfacePrefix) -> Option<bool> {
    let sign = match prefix.kind {
        0x32 => analytic_plane::SIGN,
        0x33 => analytic_cylinder::SIGN,
        0x34 => analytic_cone::SIGN,
        0x35 => analytic_sphere::SIGN,
        0x38 => analytic_torus::SIGN,
        _ => return None,
    };
    match *brep.get(
        prefix
            .pos
            .checked_sub(analytic_plane::MARKER)?
            .checked_add(sign)?,
    )? {
        0x01 => Some(true),
        0xff => Some(false),
        _ => None,
    }
}

/// Read the unique contiguous standard vertex roster with the requested
/// cardinality. Each seven-byte row stores `54 <identity:u24le> 00 00 00`;
/// roster order is coordinate-table order.
#[must_use]
pub fn standard_vertex_roster(source: &[u8], vertex_count: usize) -> Option<Vec<u32>> {
    if vertex_count == 0 {
        return None;
    }
    let mut solutions = Vec::new();
    let mut position = 0usize;
    while position + vertex_roster::LEN <= source.len() {
        if source[position + vertex_roster::MARKER] != 0x54
            || source[position + vertex_roster::ZERO_RUN..position + vertex_roster::LEN]
                != [0, 0, 0]
        {
            position += 1;
            continue;
        }
        let start = position;
        let mut identities = Vec::new();
        while position + vertex_roster::LEN <= source.len()
            && source[position + vertex_roster::MARKER] == 0x54
            && source[position + vertex_roster::ZERO_RUN..position + vertex_roster::LEN]
                == [0, 0, 0]
        {
            let identity = View::u24_le_at(source, position + vertex_roster::TAG)?;
            if identities
                .last()
                .is_some_and(|previous| *previous >= identity)
            {
                break;
            }
            identities.push(identity);
            position += vertex_roster::LEN;
        }
        if identities.len() == vertex_count {
            solutions.push(identities);
        }
        if position == start {
            position += 1;
        }
    }
    <[Vec<u32>; 1]>::try_from(solutions)
        .ok()
        .map(|[identities]| identities)
}

/// Locate every per-face analytic surface record by the strict 5-byte template
/// `[target_u24 le][00][prebyte] 00 33 <kind>` ([spec §5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#58-analytic-surface-records-in-surfacicreps)). The strict template
/// rejects collisional `00 33` matches inside other binary data.
pub fn surface_prefixes(brep: &[u8]) -> Vec<SurfacePrefix> {
    let mut out = Vec::new();
    if brep.len() < 8 {
        return out;
    }
    for i in analytic_plane::MARKER..brep.len() - 3 {
        if brep[i] != 0x00 || brep[i + 1] != 0x33 {
            continue;
        }
        let kind = brep[i + 2];
        let Some(prebyte) = kind_prebyte(kind) else {
            continue;
        };
        if brep[i - 2] != 0x00 || brep[i - 1] != prebyte {
            continue;
        }
        out.push(SurfacePrefix {
            pos: i,
            target: u24_le(brep, i - analytic_plane::MARKER),
            kind,
        });
    }
    out
}

/// Locate plane bounds records and bind each persistent carrier tag to the
/// frame vector of its face-local trim packet. A tag is emitted only when one
/// valid bounds record carries it.
pub fn plane_params<S: std::hash::BuildHasher>(
    brep: &[u8],
    normals: &HashMap<u32, [f64; 3], S>,
) -> Vec<PlaneParams> {
    const MARKER: &[u8; 5] = b"\x00\x02\x00\x33\x32";

    let mut out = Vec::new();
    let mut duplicate_targets = HashSet::new();
    let mut seen_targets = HashSet::new();
    let mut p = 0usize;
    while p + MARKER.len() + 40 <= brep.len() {
        let Some(relative) = brep[p..].windows(MARKER.len()).position(|w| w == MARKER) else {
            break;
        };
        let pos = p + relative;
        p = pos + 1;
        if pos < 4 || pos + MARKER.len() + 40 > brep.len() {
            continue;
        }
        let Some(bounds) =
            face_bounds_at(brep, pos + MARKER.len()).filter(|bounds| bounds.sphere_radius > 0.0)
        else {
            continue;
        };
        let target = u24_le(brep, pos - 3);
        if !seen_targets.insert(target) {
            duplicate_targets.insert(target);
        }
        let Some(normal) = normals.get(&target).copied() else {
            continue;
        };
        out.push(PlaneParams {
            target,
            origin: Point3::new(
                bounds.sphere_center[0],
                bounds.sphere_center[1],
                bounds.sphere_center[2],
            ),
            normal: Vector3::new(normal[0], normal[1], normal[2]),
        });
    }
    out.retain(|plane| !duplicate_targets.contains(&plane.target));
    out
}

/// Decode a plane carrier from its bridged bounds and trim-frame records.
pub fn decode_plane(params: &PlaneParams) -> Option<SurfaceGeometry> {
    let normal = unit_vector(params.normal)?;
    Some(SurfaceGeometry::Plane {
        origin: params.origin,
        normal,
        u_axis: cadmpeg_ir::geometry::derive_reference_direction(normal),
    })
}

/// Geometry family carried by one positional standard `0x60` edge row.
#[derive(Debug, Clone)]
pub enum StandardCurveGeometry {
    /// The line equation is derived from endpoints or adjacent surfaces.
    Line,
    /// Inline circle parameters.
    Circle {
        /// Circle center in millimetres.
        center: Point3,
        /// Circle radius in millimetres.
        radius: f64,
    },
    /// A separately allocated spline carrier.
    Bspline,
}

/// One row of the standard positional edge-support/incidence table (spec
/// [§5.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#55-0x60-curve-support-edge-incidence-table)): `60 <tag:u24le> <curve_body> <face_ref> <face_ref>`, one row per
/// spine edge.
#[derive(Debug, Clone)]
pub struct StandardCurveSupport {
    /// Offset of the `0x60` row marker in the BREP stream.
    pub pos: usize,
    /// Little-endian u24 object id in the file-global allocation journal.
    pub tag: u32,
    /// The two adjacent standard face ordinals forming this edge's
    /// edge-to-face incidence.
    pub faces: [usize; 2],
    /// The row's curve geometry family and, where inline, its parameters.
    pub geometry: StandardCurveGeometry,
}

/// Parse the unique complete standard `0x60` table in physical-edge order.
///
/// The face-local surface roster supplies the primary anchor. If that roster
/// is unavailable, every complete row run is considered. When the fixed
/// physical-edge cardinality is available, it must match; carrier-only
/// decoding without that count still requires exactly one run. A suffix of a
/// longer run is not a table candidate because a valid predecessor row
/// disqualifies it.
///
/// `edge_count` is present for topology transfer only after the fixed standard
/// edge table is complete. A missing count permits carrier-only transfer from
/// one unique complete run but never permits topology attachment.
#[must_use]
pub fn standard_curve_supports(
    brep: &[u8],
    face_count: usize,
    edge_count: Option<usize>,
) -> Vec<StandardCurveSupport> {
    let populations = standard_surface_populations(brep);
    let matching_populations = populations
        .iter()
        .filter(|population| {
            population.records.len() == face_count
                && edge_count.is_none_or(|count| population.supports.len() == count)
        })
        .collect::<Vec<_>>();
    if populations
        .iter()
        .any(|population| population.records.len() == face_count)
    {
        return <[&StandardSurfacePopulation; 1]>::try_from(matching_populations)
            .ok()
            .map(|[population]| population.supports.clone())
            .unwrap_or_default();
    }
    if let Some(first) = standard_surface_records(brep, face_count)
        .and_then(|records| records.last().map(StandardSurfaceRecord::end))
    {
        let Some(rows) = standard_curve_supports_at(brep, face_count, first) else {
            return Vec::new();
        };
        return if edge_count.is_none_or(|count| rows.len() == count) {
            rows
        } else {
            Vec::new()
        };
    }

    let candidates = (0..brep.len())
        .filter(|&start| {
            brep.get(start) == Some(&0x60)
                && !standard_curve_support_has_predecessor(brep, face_count, start)
        })
        .filter_map(|start| {
            let rows = standard_curve_supports_at(brep, face_count, start)?;
            edge_count
                .is_none_or(|count| rows.len() == count)
                .then_some(rows)
        })
        .collect::<Vec<_>>();
    <[Vec<StandardCurveSupport>; 1]>::try_from(candidates)
        .ok()
        .map(|[rows]| rows)
        .unwrap_or_default()
}

fn standard_curve_supports_at(
    brep: &[u8],
    face_count: usize,
    mut position: usize,
) -> Option<Vec<StandardCurveSupport>> {
    let mut rows = Vec::new();
    while brep.get(position) == Some(&0x60) {
        let (row, end) = standard_curve_support_row_at(brep, face_count, position)?;
        rows.push(row);
        position = end;
    }
    (!rows.is_empty()).then_some(rows)
}

fn standard_curve_support_row_at(
    brep: &[u8],
    face_count: usize,
    position: usize,
) -> Option<(StandardCurveSupport, usize)> {
    const LINE: [u8; 5] = [0x00, 0x02, 0x00, 0x33, 0x36];
    const CIRCLE: [u8; 5] = [0x00, 0x12, 0x00, 0x33, 0x37];

    let tag = View::u24_le_at(brep, position + 1)?;
    let header = brep.get(position + 4..position + 9);
    let (geometry, refs) = if header == Some(&LINE) {
        (StandardCurveGeometry::Line, position + 9)
    } else if header == Some(&CIRCLE) {
        let cx = View::f32_be_at(brep, position + 9)?;
        let cy = View::f32_be_at(brep, position + 13)?;
        let cz = View::f32_be_at(brep, position + 17)?;
        let radius = View::f32_be_at(brep, position + 21)?;
        if !cx.is_finite()
            || !cy.is_finite()
            || !cz.is_finite()
            || !radius.is_finite()
            || radius <= 0.0
        {
            return None;
        }
        (
            StandardCurveGeometry::Circle {
                center: Point3::new(f64::from(cx), f64::from(cy), f64::from(cz)),
                radius: f64::from(radius),
            },
            position + 25,
        )
    } else if brep.get(position + 4..position + 7) == Some(&[0, 0, 0]) {
        (StandardCurveGeometry::Bspline, position + 7)
    } else {
        return None;
    };
    let (face0, next) = face_ref(brep, refs)?;
    let (face1, end) = face_ref(brep, next)?;
    (face0 < face_count && face1 < face_count).then_some((
        StandardCurveSupport {
            pos: position,
            tag,
            faces: [face0, face1],
            geometry,
        },
        end,
    ))
}

fn standard_curve_support_has_predecessor(brep: &[u8], face_count: usize, start: usize) -> bool {
    const MAX_ROW_BYTES: usize = 35;
    (start.saturating_sub(MAX_ROW_BYTES)..start).any(|candidate| {
        standard_curve_support_row_at(brep, face_count, candidate)
            .is_some_and(|(_, end)| end == start)
    })
}

/// Decode the analytic parameters carried inline in a curved surface's kind
/// record. The big-endian `f32` payload begins immediately after the 3-byte
/// `00 33 <kind>` marker ([spec §5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#58-analytic-surface-records-in-surfacicreps)). Returns `None` for the plane kind (its
/// parameters are in a separate bridged record) and for any non-finite or
/// invalid payload.
pub fn decode_curved(brep: &[u8], prefix: &SurfacePrefix) -> Option<SurfaceGeometry> {
    let mut view = View::over_retained(brep);
    view.seek(prefix.pos + 3)?; // skip `00 33 <kind>`
    match prefix.kind {
        0x35 => {
            // sphere: cx cy cz radius
            let (cx, cy, cz, r) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            if !all_finite(&[cx, cy, cz, r]) || r <= 0.0 {
                return None;
            }
            Some(SurfaceGeometry::Sphere {
                center: pt(cx, cy, cz),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: r as f64,
            })
        }
        0x38 => {
            // torus: cx cy cz ax ay signed_major minor; sign(major) carries sign(az).
            let (cx, cy, cz, ax, ay, major, minor) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            if !all_finite(&[cx, cy, cz, ax, ay, major, minor]) {
                return None;
            }
            if !(major.abs() > 0.0 && minor > 0.0 && ax * ax + ay * ay <= 1.0 + 1e-4) {
                return None;
            }
            let axis = axis_from_xy(ax, ay, major)?;
            Some(SurfaceGeometry::Torus {
                center: pt(cx, cy, cz),
                axis,
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                major_radius: major.abs() as f64,
                minor_radius: minor as f64,
            })
        }
        0x33 => {
            // cylinder: px py pz ax ay radius; sign(radius) carries sign(az).
            let (px, py, pz, ax, ay, radius) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            if !all_finite(&[px, py, pz, ax, ay, radius]) {
                return None;
            }
            if radius == 0.0 || ax * ax + ay * ay > 1.0 + 1e-4 {
                return None;
            }
            let axis = axis_from_xy(ax, ay, radius)?;
            Some(SurfaceGeometry::Cylinder {
                origin: pt(px, py, pz),
                axis,
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                radius: radius.abs() as f64,
            })
        }
        0x34 => {
            // cone: apex_x apex_y apex_z ax ay semi_angle; radius at apex is 0.
            let (x, y, z, ax, ay, semi) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            if !all_finite(&[x, y, z, ax, ay, semi]) {
                return None;
            }
            if !(semi.abs() > 0.0 && semi.abs() < std::f32::consts::FRAC_PI_2) {
                return None;
            }
            let axis = axis_from_xy(ax, ay, semi)?;
            Some(SurfaceGeometry::Cone {
                origin: pt(x, y, z),
                axis,
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                radius: 0.0,
                ratio: 1.0,
                half_angle: semi.abs() as f64,
            })
        }
        _ => None, // plane: parameters in a separate bridged record.
    }
}

/// Read the face-side witness point following a standard cylinder or torus
/// carrier's big-endian parameter block.
#[must_use]
pub fn standard_face_witness(brep: &[u8], marker_pos: usize) -> Option<Point3> {
    if brep.get(marker_pos..marker_pos + 2) != Some(&[0x00, 0x33]) {
        return None;
    }
    let kind = *brep.get(marker_pos + 2)?;
    let offset = match kind {
        0x33 => 27,
        0x38 => 31,
        _ => return None,
    };
    let values = [
        f32_le(brep, marker_pos + offset),
        f32_le(brep, marker_pos + offset + 4),
        f32_le(brep, marker_pos + offset + 8),
    ];
    values
        .iter()
        .all(|value| value.is_finite())
        .then(|| pt(values[0], values[1], values[2]))
}

fn pt(x: f32, y: f32, z: f32) -> Point3 {
    Point3::new(x as f64, y as f64, z as f64)
}

/// Recover the third axis component from the unit-norm constraint, taking its
/// sign from a companion signed field (the cone/cylinder store `sign(az)` in the
/// sign of the semi-angle / radius).
fn axis_from_xy(ax: f32, ay: f32, signed: f32) -> Option<Vector3> {
    let norm2 = f64::from(ax).mul_add(f64::from(ax), f64::from(ay) * f64::from(ay));
    let residual = 1.0 - norm2;
    let az = if residual > F32_UNIT_NORM2_ROUNDING_TOLERANCE {
        residual.sqrt().copysign(signed as f64)
    } else {
        0.0
    };
    unit_vector(Vector3::new(ax as f64, ay as f64, az))
}

fn f32_le(bytes: &[u8], at: usize) -> f32 {
    let mut view = View::over_retained(bytes);
    view.seek(at)
        .and_then(|()| view.f32_le())
        .unwrap_or(f32::NAN)
}

fn face_ref(bytes: &[u8], at: usize) -> Option<(usize, usize)> {
    match *bytes.get(at)? {
        0xff => Some((View::u32_le_at(bytes, at + 1)? as usize, at + 5)),
        value => Some((value as usize, at + 1)),
    }
}

fn u24_le(bytes: &[u8], at: usize) -> u32 {
    bytes[at] as u32 | ((bytes[at + 1] as u32) << 8) | ((bytes[at + 2] as u32) << 16)
}

fn all_finite(vs: &[f32]) -> bool {
    vs.iter().all(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::{axis_from_xy, unit_vector};
    use cadmpeg_ir::math::Vector3;

    #[test]
    fn unit_vector_preserves_tiny_finite_direction() {
        assert_eq!(
            unit_vector(Vector3::new(1e-200, 0.0, 0.0)),
            Some(Vector3::new(1.0, 0.0, 0.0))
        );
        assert_eq!(unit_vector(Vector3::new(0.0, 0.0, 0.0)), None);
        assert_eq!(unit_vector(Vector3::new(f64::from_bits(1), 0.0, 0.0)), None);
    }

    #[test]
    fn axis_from_xy_discards_binary32_equatorial_norm_roundoff() {
        const AXIS_COMPONENT_TOLERANCE: f64 = 1e-7;
        let axis = axis_from_xy(0.707_106_77_f32, -0.707_106_77_f32, 1.0).expect("axis");

        assert_eq!(axis.z, 0.0);
        assert!((axis.x - std::f64::consts::FRAC_1_SQRT_2).abs() < AXIS_COMPONENT_TOLERANCE);
        assert!((axis.y + std::f64::consts::FRAC_1_SQRT_2).abs() < AXIS_COMPONENT_TOLERANCE);
    }

    #[test]
    fn axis_from_xy_preserves_a_genuine_third_component() {
        const AXIS_COMPONENT_TOLERANCE: f64 = 1e-15;
        let x = 0.8_f32;
        let y = 0.5_f32;
        let axis = axis_from_xy(x, y, -1.0).expect("axis");
        let x = f64::from(x);
        let y = f64::from(y);
        let expected = (1.0 - x * x - y * y).sqrt();

        assert!((axis.z + expected).abs() < AXIS_COMPONENT_TOLERANCE);
    }
}
