//! Standard-nested `SurfacicReps` record decoders.
//!
//! Decodes per-face analytic surface records, plane bounds, the `0x60`
//! curve-support/edge-incidence table, standard vertex rosters, and the
//! inline big-endian curved-surface parameter block.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{
    FiniteReal, NonNegativeLength, NonZeroAngle, NonZeroLength, PositiveLength, PositiveReal,
};
use cadmpeg_ir::units::{FiniteVector, OrthonormalFrame3, UnitVector3};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::families::standard::fbb::FbbPopulationLayout;
use crate::layout::analytic_surface_cone as analytic_cone;
use crate::layout::analytic_surface_cylinder as analytic_cylinder;
use crate::layout::analytic_surface_plane as analytic_plane;
use crate::layout::analytic_surface_sphere as analytic_sphere;
use crate::layout::analytic_surface_torus as analytic_torus;
use crate::layout::freeform_surface_core as freeform_core;
use crate::layout::vertex_roster_row as vertex_roster;
use crate::math::unit_vector;

/// Binary32 multiplication and addition can leave a unit XY direction just
/// below the unit circle.  Treat that deficit as roundoff instead of creating
/// a false binary64 Z component when the carrier stores only the Z sign.
const F32_UNIT_NORM2_ROUNDING_TOLERANCE: f64 = 4.0 * (f32::EPSILON as f64);

/// The standard-nested plane bounds record. Its three-byte tag is the bridge to
/// the matching `SurfacicReps` plane marker.
#[derive(Debug, Clone)]
pub(super) struct PlaneParams {
    /// The little-endian u24 carrier tag.
    pub(super) target: u32,
    /// Bounding-sphere center, which lies on the plane and fixes its origin.
    pub(super) origin: FinitePoint3,
    /// Plane normal from the positionally paired trim packet: finite, with a
    /// squared length within `1e-6` of one.
    pub(super) normal: FiniteVector<3>,
}

/// An analytic surface marker kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnalyticSurfaceKind {
    /// A plane carrier.
    Plane,
    /// A cylinder carrier.
    Cylinder,
    /// A cone carrier.
    Cone,
    /// A sphere carrier.
    Sphere,
    /// A torus carrier.
    Torus,
}

impl AnalyticSurfaceKind {
    pub(super) fn from_marker(marker: u8) -> Option<Self> {
        match marker {
            0x32 => Some(Self::Plane),
            0x33 => Some(Self::Cylinder),
            0x34 => Some(Self::Cone),
            0x35 => Some(Self::Sphere),
            0x38 => Some(Self::Torus),
            _ => None,
        }
    }
    pub(super) const fn marker(self) -> u8 {
        match self {
            Self::Plane => 0x32,
            Self::Cylinder => 0x33,
            Self::Cone => 0x34,
            Self::Sphere => 0x35,
            Self::Torus => 0x38,
        }
    }
    const fn prebyte(self) -> u8 {
        match self {
            Self::Plane => 0x02,
            Self::Cylinder => 0x1a,
            Self::Cone => 0x1a,
            Self::Sphere => 0x12,
            Self::Torus => 0x1e,
        }
    }
    const fn record_len(self) -> usize {
        match self {
            Self::Plane => analytic_plane::LEN,
            Self::Cylinder => analytic_cylinder::LEN,
            Self::Cone => analytic_cone::LEN,
            Self::Sphere => analytic_sphere::LEN,
            Self::Torus => analytic_torus::LEN,
        }
    }
    const fn sign_offset(self) -> usize {
        match self {
            Self::Plane => analytic_plane::SIGN,
            Self::Cylinder => analytic_cylinder::SIGN,
            Self::Cone => analytic_cone::SIGN,
            Self::Sphere => analytic_sphere::SIGN,
            Self::Torus => analytic_torus::SIGN,
        }
    }
    const fn bounds_offset(self) -> usize {
        match self {
            Self::Plane => 3,
            Self::Cylinder => 27,
            Self::Cone => 27,
            Self::Sphere => 19,
            Self::Torus => 31,
        }
    }
}

/// A located per-face analytic surface record.
#[derive(Debug, Clone)]
pub(crate) struct SurfacePrefix {
    /// Offset of the `00 33 <kind>` signature within the BREP stream.
    pub(super) pos: usize,
    /// The little-endian u24 tag that identifies this carrier.
    pub(super) target: u32,
    /// The kind byte (`0x32`..=`0x38`).
    pub(super) kind: AnalyticSurfaceKind,
}

/// One face-local record in the standard `SurfacicReps` surface roster.
#[derive(Debug, Clone)]
pub(super) enum StandardSurfaceRecord {
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
pub(super) struct StandardFaceBounds {
    /// Axis-aligned bounding-box centre.
    pub(super) aabb_center: [FiniteReal; 3],
    /// Non-negative axis-aligned bounding-box half-extents.
    pub(super) aabb_half_extents: [NonNegativeLength; 3],
    /// Bounding-sphere centre.
    pub(super) sphere_center: [FiniteReal; 3],
    /// Non-negative bounding-sphere radius.
    pub(super) sphere_radius: NonNegativeLength,
}

fn face_bounds_at(brep: &[u8], position: usize) -> Option<StandardFaceBounds> {
    let mut values = [0.0f32; 10];
    let mut finite = [FiniteReal::ZERO; 10];
    for (index, value) in values.iter_mut().enumerate() {
        *value = f32_le(brep, position + 4 * index)?;
        finite[index] = FiniteReal::new(f64::from(*value))?;
    }
    let extent = |index: usize| NonNegativeLength::new(finite[index].get());
    let aabb_half_extents = [extent(3)?, extent(4)?, extent(5)?];
    let sphere_radius = extent(9)?;
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
        aabb_center: [finite[0], finite[1], finite[2]],
        aabb_half_extents,
        sphere_center: [finite[6], finite[7], finite[8]],
        sphere_radius,
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
pub(super) fn standard_face_bounds(
    brep: &[u8],
    record: &StandardSurfaceRecord,
) -> Option<StandardFaceBounds> {
    match record {
        StandardSurfaceRecord::Freeform { bounds, .. } => Some(*bounds),
        StandardSurfaceRecord::Analytic(prefix) => {
            let relative = prefix.kind.bounds_offset();
            face_bounds_at(brep, prefix.pos + relative)
                .filter(|bounds| bounds.sphere_radius.get() > 0.0)
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
            Self::Analytic(prefix) => self.pos() + prefix.kind.record_len(),
            Self::Freeform { pos, .. } => pos + freeform_core::LEN,
        }
    }
}

struct StandardSurfaceRecordTable {
    records: Vec<StandardSurfaceRecord>,
    successors: Vec<Option<usize>>,
}

fn standard_surface_record_table(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
) -> Result<StandardSurfaceRecordTable, CodecError> {
    let mut records = BTreeMap::<usize, StandardSurfaceRecord>::new();
    for prefix in surface_prefixes(ctx, brep)? {
        if face_sense(brep, &prefix).is_some() {
            if !records.contains_key(&(prefix.pos - analytic_plane::MARKER)) {
                ctx.charge_collection_items(1, "catia_surface_record_tree")?;
            }
            records.insert(
                prefix.pos - analytic_plane::MARKER,
                StandardSurfaceRecord::Analytic(prefix),
            );
        }
    }
    let mut analytic_ranges = Vec::new();
    for record in records.values() {
        if let StandardSurfaceRecord::Analytic(prefix) = record {
            crate::resource::push(
                ctx,
                &mut analytic_ranges,
                (prefix.pos - analytic_plane::MARKER, record.end()),
                "catia_surface_analytic_ranges",
            )?;
        }
    }
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
        if !records.contains_key(&pos) {
            ctx.charge_collection_items(1, "catia_surface_record_tree")?;
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

    let mut ordered_records = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut ordered_records,
        records.len(),
        "catia_surface_ordered_records",
    )?;
    ordered_records.extend(records.into_values());
    let mut record_indices = HashMap::new();
    for (index, record) in ordered_records.iter().enumerate() {
        crate::resource::insert_map(
            ctx,
            &mut record_indices,
            record.pos(),
            index,
            "catia_surface_record_indices",
        )?;
    }
    let mut successors = Vec::new();
    crate::resource::reserve_vec(
        ctx,
        &mut successors,
        ordered_records.len(),
        "catia_surface_successors",
    )?;
    for record in &ordered_records {
        successors.push(record_indices.get(&record.end()).copied());
    }
    Ok(StandardSurfaceRecordTable {
        records: ordered_records,
        successors,
    })
}

/// Return every surface roster chain that ends directly at a complete `0x60`
/// support table. Each chain is a source-closed face population; records that
/// cannot reach that boundary are not assigned to a population.
pub(super) fn standard_surface_record_groups(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
) -> Result<Vec<Vec<StandardSurfaceRecord>>, CodecError> {
    let table = standard_surface_record_table(ctx, brep)?;
    let mut has_predecessor =
        ctx.alloc_filled(table.records.len(), false, "catia_surface_has_predecessor")?;
    for successor in table.successors.iter().flatten() {
        has_predecessor[*successor] = true;
    }
    let mut groups = Vec::new();
    for start in 0..table.records.len() {
        if has_predecessor[start] {
            continue;
        }
        let mut current = Some(start);
        let mut group = Vec::new();
        while let Some(index) = current {
            crate::resource::push(
                ctx,
                &mut group,
                table.records[index].clone(),
                "catia_surface_group_records",
            )?;
            current = table.successors[index];
        }
        if group
            .last()
            .is_some_and(|last| brep.get(last.end()) == Some(&0x60))
        {
            crate::resource::push(ctx, &mut groups, group, "catia_surface_record_groups")?;
        }
    }
    Ok(groups)
}

/// One surface roster and its positionally following, face-local support
/// table. Support face references remain local to this population.
#[derive(Debug, Clone)]
pub(super) struct StandardSurfacePopulation {
    /// The source-closed face-local surface roster.
    pub(super) records: Vec<StandardSurfaceRecord>,
    /// The source-closed `0x60` edge-support roster.
    pub(super) supports: Vec<StandardCurveSupport>,
}

type StandardPopulationPair = (FbbPopulationLayout, StandardSurfacePopulation);

/// A nonempty source-ordered population relation.
pub(in crate::families::standard) struct StandardPopulationPairs {
    pub(super) first: StandardPopulationPair,
    pub(super) rest: Vec<StandardPopulationPair>,
}

/// Return every source-closed surface/support population with valid local
/// face references. No population is selected by row count or allocation
/// order.
pub(super) fn standard_surface_populations(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
) -> Result<Vec<StandardSurfacePopulation>, CodecError> {
    let mut populations = Vec::new();
    for records in standard_surface_record_groups(ctx, brep)? {
        let Some(support_start) = records.last().map(StandardSurfaceRecord::end) else {
            continue;
        };
        let Some(supports) = standard_curve_supports_at(ctx, brep, records.len(), support_start)?
        else {
            continue;
        };
        crate::resource::push(
            ctx,
            &mut populations,
            StandardSurfacePopulation { records, supports },
            "catia_surface_populations",
        )?;
    }
    Ok(populations)
}

/// Pair source-ordered, source-closed FBB layouts with source-ordered,
/// source-closed surface/support populations. The relation is admitted only
/// when both lanes have the same population count and every local face and
/// edge cardinality agrees. Allocation order and a repeated count key never
/// select a population.
pub(super) fn pair_standard_populations(
    ctx: &DecodeContext<'_>,
    layouts: &[FbbPopulationLayout],
    populations: &[StandardSurfacePopulation],
) -> Result<Option<StandardPopulationPairs>, CodecError> {
    if layouts.len() != populations.len() {
        return Ok(None);
    }
    let Some((first_layout, layouts)) = layouts.split_first() else {
        return Ok(None);
    };
    let Some((first_population, populations)) = populations.split_first() else {
        return Ok(None);
    };
    let pair = |layout: FbbPopulationLayout,
                population: &StandardSurfacePopulation|
     -> Result<Option<StandardPopulationPair>, CodecError> {
        if layout.face_run.face_count() != population.records.len()
            || layout.edge_count != population.supports.len()
        {
            return Ok(None);
        }
        Ok(Some((
            layout,
            StandardSurfacePopulation {
                records: crate::resource::copy_retained_slice(
                    ctx,
                    &population.records,
                    "catia_population_pair_records",
                )?,
                supports: crate::resource::copy_retained_slice(
                    ctx,
                    &population.supports,
                    "catia_population_pair_supports",
                )?,
            },
        )))
    };
    let Some(first) = pair(*first_layout, first_population)? else {
        return Ok(None);
    };
    let mut rest = Vec::new();
    for (layout, population) in layouts.iter().copied().zip(populations) {
        let Some(next) = pair(layout, population)? else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut rest, next, "catia_population_pairs")?;
    }
    Ok(Some(StandardPopulationPairs { first, rest }))
}

/// Walk the complete face-local surface roster. Records are accepted only as a
/// unique contiguous chain of `face_count` non-overlapping entries terminated
/// by the first curve-support row. A byte pattern inside an analytic payload
/// cannot create a competing freeform record.
pub(super) fn standard_surface_records(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
    face_count: usize,
) -> Result<Option<Vec<StandardSurfaceRecord>>, CodecError> {
    let table = standard_surface_record_table(ctx, brep)?;
    if face_count == 0 || face_count > table.records.len() {
        return Ok(None);
    }
    let ordered_records = &table.records;
    let successors = &table.successors;
    let remaining_steps = face_count - 1;
    let level_count = usize::BITS as usize - remaining_steps.leading_zeros() as usize;
    let mut jumps = Vec::new();
    crate::resource::reserve_vec(ctx, &mut jumps, level_count, "catia_surface_jump_levels")?;
    if level_count > 0 {
        let mut previous = crate::resource::copy_slice(ctx, successors, "catia_surface_jump_rows")?;
        for _ in 1..level_count {
            let mut next = Vec::new();
            crate::resource::reserve_vec(
                ctx,
                &mut next,
                previous.len(),
                "catia_surface_jump_rows",
            )?;
            for successor in &previous {
                next.push(successor.and_then(|middle| previous[middle]));
            }
            jumps.push(previous);
            previous = next;
        }
        jumps.push(previous);
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
            return Ok(None);
        }
    }

    let Some(mut current) = solution_start else {
        return Ok(None);
    };
    let mut chain = Vec::new();
    crate::resource::reserve_vec(ctx, &mut chain, face_count, "catia_surface_record_chain")?;
    for ordinal in 0..face_count {
        chain.push(ordered_records[current].clone());
        if ordinal + 1 < face_count {
            let Some(next) = successors[current] else {
                return Ok(None);
            };
            current = next;
        }
    }
    Ok(Some(chain))
}

/// Read the trailing per-face orientation byte from a complete analytic
/// `SurfacicReps` record. `true` means the face follows the carrier normal.
pub(super) fn face_sense(brep: &[u8], prefix: &SurfacePrefix) -> Option<bool> {
    let sign = prefix.kind.sign_offset();
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
pub(super) fn standard_vertex_roster(
    ctx: &DecodeContext<'_>,
    source: &[u8],
    vertex_count: usize,
) -> Result<Option<Vec<u32>>, CodecError> {
    if vertex_count == 0 {
        return Ok(None);
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
            let Some(identity) = View::u24_le_at(source, position + vertex_roster::TAG) else {
                return Ok(None);
            };
            if identities
                .last()
                .is_some_and(|previous| *previous >= identity)
            {
                break;
            }
            crate::resource::push(
                ctx,
                &mut identities,
                identity,
                "catia_vertex_roster_identities",
            )?;
            position += vertex_roster::LEN;
        }
        if identities.len() == vertex_count {
            crate::resource::push(
                ctx,
                &mut solutions,
                identities,
                "catia_vertex_roster_solutions",
            )?;
        }
        if position == start {
            position += 1;
        }
    }
    Ok(<[Vec<u32>; 1]>::try_from(solutions)
        .ok()
        .map(|[identities]| identities))
}

/// Locate every per-face analytic surface record by the strict 5-byte template
/// `[target_u24 le][00][prebyte] 00 33 <kind>` ([spec §5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#58-analytic-surface-records-in-surfacicreps)). The strict template
/// rejects collisional `00 33` matches inside other binary data.
pub(crate) fn surface_prefixes(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
) -> Result<Vec<SurfacePrefix>, CodecError> {
    let mut out = Vec::new();
    if brep.len() < 8 {
        return Ok(out);
    }
    for i in analytic_plane::MARKER..brep.len() - 3 {
        if brep[i] != 0x00 || brep[i + 1] != 0x33 {
            continue;
        }
        let kind = brep[i + 2];
        let Some(kind) = AnalyticSurfaceKind::from_marker(kind) else {
            continue;
        };
        if brep[i - 2] != 0x00 || brep[i - 1] != kind.prebyte() {
            continue;
        }
        crate::resource::push(
            ctx,
            &mut out,
            SurfacePrefix {
                pos: i,
                target: u24_le(brep, i - analytic_plane::MARKER),
                kind,
            },
            "catia_surface_prefixes",
        )?;
    }
    Ok(out)
}

/// Locate plane bounds records and bind each persistent carrier tag to the
/// frame vector of its face-local trim packet. A tag is emitted only when one
/// valid bounds record carries it.
pub(super) fn plane_params<S: std::hash::BuildHasher>(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
    normals: &HashMap<u32, FiniteVector<3>, S>,
) -> Result<Vec<PlaneParams>, CodecError> {
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
        let Some(bounds) = face_bounds_at(brep, pos + MARKER.len())
            .filter(|bounds| bounds.sphere_radius.get() > 0.0)
        else {
            continue;
        };
        let target = u24_le(brep, pos - 3);
        if !crate::resource::insert_set(ctx, &mut seen_targets, target, "catia_plane_seen_targets")?
        {
            crate::resource::insert_set(
                ctx,
                &mut duplicate_targets,
                target,
                "catia_plane_duplicate_targets",
            )?;
        }
        let Some(normal) = normals.get(&target).copied() else {
            continue;
        };
        let [x, y, z] = bounds.sphere_center;
        crate::resource::push(
            ctx,
            &mut out,
            PlaneParams {
                target,
                origin: FinitePoint3::from_coordinates(x, y, z),
                normal,
            },
            "catia_plane_params",
        )?;
    }
    out.retain(|plane| !duplicate_targets.contains(&plane.target));
    Ok(out)
}

/// Decode a plane carrier from its bridged bounds and trim-frame records.
pub(super) fn decode_plane(params: &PlaneParams) -> Option<SurfaceGeometry> {
    let normal = unit_vector(Vector3::from(params.normal.get()))?;
    let ref_direction = UnitVector3::new(cadmpeg_ir::geometry::derive_reference_direction(
        *normal.as_raw(),
    ))?;
    let frame = OrthonormalFrame3::from_units(normal, ref_direction)?;
    Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::new(params.origin, frame),
    )))
}

/// Geometry family carried by one positional standard `0x60` edge row.
#[derive(Debug, Clone)]
pub(super) enum StandardCurveGeometry {
    /// The line equation is derived from endpoints or adjacent surfaces.
    Line,
    /// Inline circle parameters.
    Circle {
        /// Circle center in millimetres.
        center: FinitePoint3,
        /// Circle radius in millimetres.
        radius: PositiveLength,
    },
    /// A separately allocated spline carrier.
    Bspline,
}

/// One row of the standard positional edge-support/incidence table (spec
/// [§5.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#55-0x60-curve-support-edge-incidence-table)): `60 <tag:u24le> <curve_body> <face_ref> <face_ref>`, one row per
/// spine edge.
#[derive(Debug, Clone)]
pub(super) struct StandardCurveSupport {
    /// Offset of the `0x60` row marker in the BREP stream.
    pub(super) pos: usize,
    /// Little-endian u24 object id in the file-global allocation journal.
    pub(super) tag: u32,
    /// The two adjacent standard face ordinals forming this edge's
    /// edge-to-face incidence.
    pub(super) faces: [usize; 2],
    /// The row's curve geometry family and, where inline, its parameters.
    pub(super) geometry: StandardCurveGeometry,
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
pub(super) fn standard_curve_supports(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
    face_count: usize,
    edge_count: Option<usize>,
) -> Result<Vec<StandardCurveSupport>, CodecError> {
    let populations = standard_surface_populations(ctx, brep)?;
    let mut matching_populations = Vec::new();
    for population in &populations {
        if population.records.len() == face_count
            && edge_count.is_none_or(|count| population.supports.len() == count)
        {
            crate::resource::push(
                ctx,
                &mut matching_populations,
                population,
                "catia_matching_surface_populations",
            )?;
        }
    }
    if populations
        .iter()
        .any(|population| population.records.len() == face_count)
    {
        let Ok([population]) = <[&StandardSurfacePopulation; 1]>::try_from(matching_populations)
        else {
            return Ok(Vec::new());
        };
        return crate::resource::copy_retained_slice(
            ctx,
            &population.supports,
            "catia_curve_support_copy",
        );
    }
    if let Some(first) = standard_surface_records(ctx, brep, face_count)?
        .and_then(|records| records.last().map(StandardSurfaceRecord::end))
    {
        let Some(rows) = standard_curve_supports_at(ctx, brep, face_count, first)? else {
            return Ok(Vec::new());
        };
        return if edge_count.is_none_or(|count| rows.len() == count) {
            Ok(rows)
        } else {
            Ok(Vec::new())
        };
    }

    let mut candidates = Vec::new();
    for start in 0..brep.len() {
        if brep.get(start) != Some(&0x60)
            || standard_curve_support_has_predecessor(brep, face_count, start)
        {
            continue;
        }
        let Some(rows) = standard_curve_supports_at(ctx, brep, face_count, start)? else {
            continue;
        };
        if edge_count.is_none_or(|count| rows.len() == count) {
            crate::resource::push(ctx, &mut candidates, rows, "catia_curve_support_candidates")?;
        }
    }
    Ok(<[Vec<StandardCurveSupport>; 1]>::try_from(candidates)
        .ok()
        .map(|[rows]| rows)
        .unwrap_or_default())
}

fn standard_curve_supports_at(
    ctx: &DecodeContext<'_>,
    brep: &[u8],
    face_count: usize,
    mut position: usize,
) -> Result<Option<Vec<StandardCurveSupport>>, CodecError> {
    let mut rows = Vec::new();
    while brep.get(position) == Some(&0x60) {
        let Some((row, end)) = standard_curve_support_row_at(brep, face_count, position) else {
            return Ok(None);
        };
        crate::resource::push(ctx, &mut rows, row, "catia_curve_support_rows")?;
        position = end;
    }
    Ok((!rows.is_empty()).then_some(rows))
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
        let center = FinitePoint3::new(Point3::new(f64::from(cx), f64::from(cy), f64::from(cz)))?;
        let radius = PositiveLength::new(f64::from(radius))?;
        (
            StandardCurveGeometry::Circle { center, radius },
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
        brep[candidate] == 0x60
            && standard_curve_support_row_at(brep, face_count, candidate)
                .is_some_and(|(_, end)| end == start)
    })
}

/// Decode the analytic parameters carried inline in a curved surface's kind
/// record. The big-endian `f32` payload begins immediately after the 3-byte
/// `00 33 <kind>` marker ([spec §5.8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#58-analytic-surface-records-in-surfacicreps)). Returns `None` for the plane kind (its
/// parameters are in a separate bridged record) and for any non-finite or
/// invalid payload.
pub(super) fn decode_curved(brep: &[u8], prefix: &SurfacePrefix) -> Option<SurfaceGeometry> {
    let mut view = View::over_retained(brep);
    view.seek(prefix.pos + 3)?; // skip `00 33 <kind>`
    match prefix.kind {
        AnalyticSurfaceKind::Sphere => {
            // sphere: cx cy cz radius
            let (cx, cy, cz, r) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            let center = pt(cx, cy, cz)?;
            let radius = PositiveLength::new(f64::from(r))?;
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                cadmpeg_ir::geometry::analytic::SphereSurface::new(
                    center,
                    OrthonormalFrame3::IDENTITY,
                    NonZeroLength::from(radius),
                ),
            )))
        }
        AnalyticSurfaceKind::Torus => {
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
            let center = pt(cx, cy, cz)?;
            let signed_major = NonZeroLength::new(f64::from(major))?;
            let major_radius = signed_major.abs();
            let minor_radius = PositiveLength::new(f64::from(minor))?;
            if ax * ax + ay * ay > 1.0 + 1e-4 {
                return None;
            }
            let axis = axis_from_xy(ax, ay, signed_major.get())?;
            let frame = OrthonormalFrame3::new(
                *axis.as_raw(),
                cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw()),
            )?;
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                cadmpeg_ir::geometry::analytic::TorusSurface::new(
                    center,
                    frame,
                    major_radius,
                    NonZeroLength::from(minor_radius),
                ),
            )))
        }
        AnalyticSurfaceKind::Cylinder => {
            // cylinder: px py pz ax ay radius; sign(radius) carries sign(az).
            let (px, py, pz, ax, ay, radius) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            let origin = pt(px, py, pz)?;
            let signed_radius = NonZeroLength::new(f64::from(radius))?;
            if ax * ax + ay * ay > 1.0 + 1e-4 {
                return None;
            }
            let axis = axis_from_xy(ax, ay, signed_radius.get())?;
            let frame = OrthonormalFrame3::new(
                *axis.as_raw(),
                cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw()),
            )?;
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::new(
                    origin,
                    frame,
                    signed_radius.abs(),
                ),
            )))
        }
        AnalyticSurfaceKind::Cone => {
            // cone: apex_x apex_y apex_z ax ay semi_angle; radius at apex is 0.
            let (x, y, z, ax, ay, semi) = (
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
                view.f32_be()?,
            );
            let origin = pt(x, y, z)?;
            let signed_semi = NonZeroAngle::new(f64::from(semi))?;
            let half_angle = signed_semi.abs();
            if half_angle.get() >= f64::from(std::f32::consts::FRAC_PI_2) {
                return None;
            }
            let axis = axis_from_xy(ax, ay, signed_semi.get())?;
            let frame = OrthonormalFrame3::new(
                *axis.as_raw(),
                cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw()),
            )?;
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::new(
                    origin,
                    frame,
                    NonNegativeLength::ZERO,
                    PositiveReal::ONE,
                    half_angle.into(),
                ),
            )))
        }
        AnalyticSurfaceKind::Plane => None, // plane: parameters in a separate bridged record.
    }
}

/// Read the face-side witness point following a standard cylinder or torus
/// carrier's big-endian parameter block.
#[must_use]
pub(super) fn standard_face_witness(brep: &[u8], marker_pos: usize) -> Option<FinitePoint3> {
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
        f32_le(brep, marker_pos + offset)?,
        f32_le(brep, marker_pos + offset + 4)?,
        f32_le(brep, marker_pos + offset + 8)?,
    ];
    pt(values[0], values[1], values[2])
}

fn pt(x: f32, y: f32, z: f32) -> Option<FinitePoint3> {
    FinitePoint3::new(Point3::new(f64::from(x), f64::from(y), f64::from(z)))
}

/// Recover the third axis component from the unit-norm constraint, taking its
/// sign from a companion signed field (the cone/cylinder store `sign(az)` in the
/// sign of the semi-angle / radius).
fn axis_from_xy(ax: f32, ay: f32, signed: f64) -> Option<UnitVector3> {
    let norm2 = f64::from(ax).mul_add(f64::from(ax), f64::from(ay) * f64::from(ay));
    let residual = 1.0 - norm2;
    let az = if residual > F32_UNIT_NORM2_ROUNDING_TOLERANCE {
        residual.sqrt().copysign(signed)
    } else {
        0.0
    };
    UnitVector3::normalized_by_largest_component(Vector3::new(f64::from(ax), f64::from(ay), az))
}

fn f32_le(bytes: &[u8], at: usize) -> Option<f32> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    view.f32_le()
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

#[cfg(test)]
mod tests {
    use super::axis_from_xy;

    #[test]
    fn surface_prefix_and_vertex_roster_limits_refuse_before_growth() {
        let prefix = [0x12, 0x34, 0x56, 0, 0x1a, 0, 0x33, 0x33, 0];
        assert_eq!(
            crate::test_support::with_service_context(|ctx| super::surface_prefixes(ctx, &prefix))
                .expect("service resource budget")
                .len(),
            1
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| super::surface_prefixes(ctx, &prefix)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_surface_prefixes"
        ));
        let roster = [0x54, 1, 0, 0, 0, 0, 0];
        assert_eq!(
            crate::test_support::with_service_context(|ctx| super::standard_vertex_roster(
                ctx, &roster, 1
            ))
            .expect("service resource budget"),
            Some(vec![1])
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| super::standard_vertex_roster(ctx, &roster, 1)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_vertex_roster_identities"
        ));
    }

    #[test]
    fn surface_roster_tables_groups_and_supports_refuse_before_growth() {
        let mut bytes = vec![0x34, 0x12, 0, 0, 0, 0];
        for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x01);
        let analytic = bytes.len();
        bytes.extend_from_slice(&[0x78, 0x56, 0, 0, 0x1a, 0, 0x33, 0x33]);
        bytes.resize(analytic + 72, 0);
        bytes.push(0xff);
        bytes.extend_from_slice(&[0x60, 1, 0, 0, 0x00, 0x02, 0x00, 0x33, 0x36, 0, 1]);
        let populations = crate::test_support::with_service_context(|ctx| {
            super::standard_surface_populations(ctx, &bytes)
        })
        .expect("service resource budget");
        assert_eq!(populations.len(), 1);
        let mut operations = std::collections::HashSet::new();
        for limit in 0..40 {
            let result = crate::test_support::with_collection_limit(limit, |ctx| {
                super::standard_surface_populations(ctx, &bytes)
            });
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
                operations.insert(refusal.operation);
            }
        }
        for operation in [
            "catia_surface_record_tree",
            "catia_surface_analytic_ranges",
            "catia_surface_ordered_records",
            "catia_surface_record_indices",
            "catia_surface_successors",
            "catia_surface_has_predecessor",
            "catia_surface_group_records",
            "catia_surface_record_groups",
            "catia_curve_support_rows",
            "catia_surface_populations",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
        let mut operations = std::collections::HashSet::new();
        for limit in 0..40 {
            let result = crate::test_support::with_collection_limit(limit, |ctx| {
                super::standard_surface_records(ctx, &bytes, 2)
            });
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
                operations.insert(refusal.operation);
            }
        }
        for operation in [
            "catia_surface_jump_levels",
            "catia_surface_jump_rows",
            "catia_surface_record_chain",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
        let mut operations = std::collections::HashSet::new();
        for limit in 0..40 {
            let result = crate::test_support::with_collection_limit(limit, |ctx| {
                super::standard_curve_supports(ctx, &bytes, 2, Some(1))
            });
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = result {
                operations.insert(refusal.operation);
            }
        }
        for operation in [
            "catia_matching_surface_populations",
            "catia_curve_support_copy",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn support_predecessor_requires_the_row_marker() {
        // A line support row is 60, its u24 tag, the five-byte line body,
        // and two local face references (format specification section 5.5).
        let mut bytes = vec![
            0x60, 1, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 1, 0x60, 2, 0, 0, 0, 2, 0, 0x33, 0x36, 0, 1,
        ];
        assert!(
            crate::test_support::with_service_context(|ctx| super::standard_curve_supports(
                ctx,
                &bytes,
                2,
                Some(1)
            ))
            .expect("service resource budget")
            .is_empty()
        );
        for marker in 0..=u8::MAX {
            if marker == 0x60 {
                continue;
            }
            bytes[0] = marker;
            let rows = crate::test_support::with_service_context(|ctx| {
                super::standard_curve_supports(ctx, &bytes, 2, Some(1))
            })
            .expect("service resource budget");
            assert_eq!(rows.len(), 1, "preceding non-row marker {marker:#04x}");
            assert_eq!(rows[0].pos, 11);
            assert_eq!(rows[0].tag, 2);
            assert_eq!(rows[0].faces, [0, 1]);
        }
    }

    #[test]
    fn axis_from_xy_discards_binary32_equatorial_norm_roundoff() {
        const AXIS_COMPONENT_TOLERANCE: f64 = 1e-7;
        let axis = axis_from_xy(0.707_106_77_f32, -0.707_106_77_f32, 1.0).expect("axis");
        let axis = axis.as_raw();

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
        let axis = axis.as_raw();
        let x = f64::from(x);
        let y = f64::from(y);
        let expected = (1.0 - x * x - y * y).sqrt();

        assert!((axis.z + expected).abs() < AXIS_COMPONENT_TOLERANCE);
    }
    #[test]
    fn f32_read_distinguishes_truncation_from_stored_nan() {
        assert_eq!(super::f32_le(&[0; 3], 0), None);
        assert_eq!(super::f32_le(&[0; 4], 5), None);
        assert!(super::f32_le(&f32::NAN.to_le_bytes(), 0)
            .expect("complete stored NaN")
            .is_nan());
        assert_eq!(super::f32_le(&1.5_f32.to_le_bytes(), 0), Some(1.5));
    }
}
