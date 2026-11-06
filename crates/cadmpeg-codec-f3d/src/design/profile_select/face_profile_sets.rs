// SPDX-License-Identifier: Apache-2.0
//! Infer an Extrude's face profile from its own transition when face recipes
//! leave members unresolved.
//!
//! A blind Extrude from planar faces inserts, at its length from their plane,
//! planar end caps that repeat the swept region: the floor of a cut or the
//! top of a join. The preceding faces on that plane whose interiors lie
//! inside the caps are the profile when they are as many as the group's
//! members, include every resolved face, cover the caps' area, and have an
//! outline on the caps' boundary through every cap vertex, and when no other
//! plane, side or face that could be the profile contradicts that set. A
//! face could be the profile when it is resolved, when the transition
//! deletes or updates it, or when the Extrude makes a new body. Only when no
//! cap reaches such a face, a transition that deletes as many faces as the
//! group has members, every one on the plane of the resolved members and the
//! resolved faces among them, deletes the profile.
//!
//! Resolved members keep their faces at their member positions. The other
//! faces fill the remaining positions in face-slot order.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::loss::LossNote;

use super::cap_regions::{
    distance, face_cap, oriented_loops, relation, Cap, PlaneFrame, AREA_AGREEMENT, CAP_EDGES,
    PARALLEL_CAP, SIDE_OFFSET,
};
use crate::design::face_resolve::{
    extrude_profile_group_operand_indices, extrude_profile_group_roots,
    historical_face_selection_with_native,
};
use crate::design::geometry::{point_in_polygon, point_segment_distance};
use crate::history_records::{AsmHistoricalPlane, AsmHistoricalTopology, AsmHistory};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignExtrudeOperandRole,
    face::DesignFaceOperand,
};

/// Grid lines per axis searched for a point well inside a face.
const FACE_SAMPLE_GRID: u32 = 24;
/// How many linear tolerances a face's sample point keeps from its boundary.
const FACE_CLEARANCE: f64 = 10.0;

/// Local work caps of one cap search.
#[derive(Clone, Copy)]
struct SearchLimits {
    /// Most preceding faces on one candidate plane.
    pool_faces: usize,
    /// Most polyline points in the faces one candidate plane selects.
    outline_points: usize,
    /// Most loop edges of one cap or preceding face.
    face_edges: usize,
}

const SEARCH_LIMITS: SearchLimits = SearchLimits {
    pool_faces: 256,
    outline_points: 4096,
    face_edges: CAP_EDGES,
};

/// The arenas that infer Extrude face profiles.
pub(crate) struct FaceProfileSetResolution<'a> {
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a [DesignFaceOperand],
    pub(crate) histories: &'a [AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
    pub(crate) linear_tolerance: f64,
    pub(crate) ctx: &'a DecodeContext<'a>,
}

/// The topologies and counts one end-cap search reads.
struct CapSearch<'a> {
    preceding: &'a AsmHistoricalTopology,
    topology: &'a AsmHistoricalTopology,
    inserted: &'a [i64],
    length: f64,
    members: usize,
    resolved: &'a [i64],
    /// The faces the transition deletes or updates.
    changed: &'a HashSet<i64>,
    /// Whether every preceding face could be the profile.
    new_body: bool,
    tolerance: f64,
    limits: SearchLimits,
}

/// What the inserted caps state about the profile faces.
#[derive(Debug, PartialEq)]
enum CapReading {
    /// One face set, sorted by slot.
    Selected(Vec<i64>),
    /// A selection that could be the profile fails its checks, two such
    /// sets differ, or a cap over such faces or such a face that reaches
    /// into a cap is unreadable.
    Disputed,
    /// No cap reaches a preceding face that could be the profile.
    Absent,
    /// The search passed a local work cap.
    Capped,
}

/// The rule that infers a face set.
#[derive(Clone, Copy)]
enum SetRule {
    EndCaps,
    DeletedFaces,
}

impl SetRule {
    fn name(self) -> &'static str {
        match self {
            Self::EndCaps => "end caps",
            Self::DeletedFaces => "deleted faces",
        }
    }

    /// The loss that names this rule's inference of `faces` for `group`.
    fn inference(self, group: &str, faces: &[i64], resolved: &[i64]) -> LossNote {
        match self {
            Self::EndCaps => F3dLossCode::FeatureExtrudeProfileCapFacesInferred.note(format!(
                "Extrude profile group {group} takes faces {faces:?}: the transition's end caps at the blind length cover these preceding faces, and no other plane or side holds a selection that can be the profile.",
            )),
            Self::DeletedFaces => F3dLossCode::FeatureExtrudeProfileDeletedFacesInferred.note(format!(
                "Extrude profile group {group} takes faces {faces:?}: the transition deletes these faces, one per member, on the plane of the resolved faces {resolved:?}, and no end cap reaches a face that can be the profile.",
            )),
        }
    }
}

/// Replace each native single-group Extrude face profile whose set its own
/// transition infers with the inferred historical faces.
pub(crate) fn bind_extrude_face_profile_sets(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &FaceProfileSetResolution<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for feature in features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| resolution.scopes.iter().find(|scope| scope.id == id))
        else {
            continue;
        };
        let mut outcome = Ok(Vec::new());
        feature.evaluation.edit(|definition, _| {
            outcome = bind_one(definition, scope, resolution, SEARCH_LIMITS);
        });
        losses.extend(outcome?);
    }
    Ok(losses)
}

fn bind_one(
    definition: &mut cadmpeg_ir::features::FeatureDefinition,
    scope: &DesignParameterScope,
    resolution: &FaceProfileSetResolution<'_>,
    limits: SearchLimits,
) -> Result<Vec<LossNote>, CodecError> {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection,
        FeatureDefinition, FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    };

    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        direction: ExtrudeDirection::ProfileNormal {} | ExtrudeDirection::ReversedProfileNormal {},
        start: ExtrudeStart::ProfilePlane {},
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length },
                        draft: None,
                    },
            },
        op,
        ..
    }) = definition
    else {
        return Ok(Vec::new());
    };
    let ProfileRef::Planar(PlanarProfileRef::Native(native)) = profile else {
        return Ok(Vec::new());
    };
    let ctx = resolution.ctx;
    let Some(roots) = extrude_profile_group_roots(ctx, scope, resolution.groups)? else {
        return Ok(Vec::new());
    };
    let [root] = roots.as_slice() else {
        return Ok(Vec::new());
    };
    if root.id != *native
        || native_stream(&root.id) != native_stream(&scope.id)
        || root.extrude_role() != Some(DesignExtrudeOperandRole::Profile)
    {
        return Ok(Vec::new());
    }
    let Some(indices) =
        extrude_profile_group_operand_indices(ctx, root, resolution.groups, resolution.operands)?
    else {
        return Ok(Vec::new());
    };
    let mut members = Vec::new();
    for index in indices {
        let Some(operand) = resolution.operands.get(index) else {
            return Ok(Vec::new());
        };
        match (
            operand.resolved_face_slots.as_slice(),
            &operand.resolved_active_face,
        ) {
            ([], None) => members.push(None),
            ([slot], None) => members.push(Some(*slot)),
            _ => return Ok(Vec::new()),
        }
    }
    if members.iter().all(Option::is_some) {
        return Ok(Vec::new());
    }
    let resolved: Vec<i64> = members.iter().flatten().copied().collect();
    let (Some(state_id), Some(previous_state_id)) =
        (scope.history_state_id(), scope.previous_history_state_id())
    else {
        return Ok(Vec::new());
    };
    let histories =
        super::histories_for_scope(&scope.id, resolution.scope_histories, resolution.histories);
    let Some((history, state, previous_state)) =
        crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
    else {
        return Ok(Vec::new());
    };
    let Some(transition) = state
        .transition
        .as_ref()
        .filter(|transition| transition.previous_state_id == Some(previous_state_id))
    else {
        return Ok(Vec::new());
    };
    let (Some(topology), Some(preceding)) =
        (history.topology(state), history.topology(previous_state))
    else {
        return Ok(Vec::new());
    };
    let tolerance = resolution.linear_tolerance;
    let faces = &transition.topology.faces;
    let changed: HashSet<i64> = faces
        .deleted
        .iter()
        .chain(&faces.updated)
        .copied()
        .collect();
    let group = root.id.clone();
    let inferred = match cap_profile_faces(&CapSearch {
        preceding,
        topology,
        inserted: &faces.inserted,
        length: length.abs().get(),
        members: members.len(),
        resolved: &resolved,
        changed: &changed,
        new_body: *op == BooleanOp::NewBody,
        tolerance,
        limits,
    }) {
        CapReading::Selected(faces) => (SetRule::EndCaps, faces),
        CapReading::Absent => {
            let Some(faces) = deleted_profile_faces(
                preceding,
                &faces.deleted,
                members.len(),
                &resolved,
                tolerance,
            ) else {
                return Ok(Vec::new());
            };
            (SetRule::DeletedFaces, faces)
        }
        CapReading::Disputed => return Ok(Vec::new()),
        CapReading::Capped => {
            return Ok(vec![F3dLossCode::FeatureExtrudeProfileFaceSearchCapped.note(
                format!(
                    "Extrude profile group {group} stays native: its end-cap search passed a local cap of {} faces on one plane, {} outline points, or {} edges on one face.",
                    limits.pool_faces, limits.outline_points, limits.face_edges,
                ),
            )]);
        }
    };
    let (rule, faces) = inferred;
    let Some(ordered) = member_order(&members, &faces) else {
        return Ok(Vec::new());
    };
    let inference = rule.inference(&group, &ordered, &resolved);
    let Some(FaceSelection::Historical {
        state,
        faces,
        native: selection_native,
    }) = historical_face_selection_with_native(scope, previous_state_id, ordered, root.id.clone())
    else {
        return Ok(Vec::new());
    };
    let Ok(native_ids) = vec![selection_native.as_str().to_owned()].try_into() else {
        return Ok(Vec::new());
    };
    let unresolved = members.iter().filter(|member| member.is_none()).count();
    *profile = ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
        state,
        faces,
        native: native_ids,
    });
    let mut losses = vec![inference];
    if unresolved > 1 {
        losses.push(F3dLossCode::FeatureExtrudeProfileFaceOrderAssumed.note(format!(
            "Extrude profile group {group}: the transition's {} give its {} faces; its {unresolved} unresolved members take the faces left after the resolved members in face-slot order.",
            rule.name(),
            members.len(),
        )));
    }
    Ok(losses)
}

/// Resolved members' faces at their positions, then the other `faces` in
/// slot order at the remaining positions. `None` when the counts disagree.
fn member_order(members: &[Option<i64>], faces: &[i64]) -> Option<Vec<i64>> {
    let mut rest: Vec<i64> = faces
        .iter()
        .copied()
        .filter(|face| !members.contains(&Some(*face)))
        .collect();
    rest.sort_unstable();
    let mut rest = rest.into_iter();
    let ordered = members
        .iter()
        .map(|member| member.or_else(|| rest.next()))
        .collect::<Option<Vec<_>>>()?;
    rest.next().is_none().then_some(ordered)
}

/// The plane carrier of each face of one topology that has exactly one
/// surface binding to a surface with exactly one plane row.
struct FacePlanes<'a>(HashMap<i64, &'a AsmHistoricalPlane>);

impl<'a> FacePlanes<'a> {
    fn of(topology: &'a AsmHistoricalTopology) -> Self {
        let mut planes: HashMap<i64, Option<&AsmHistoricalPlane>> = HashMap::new();
        for plane in &topology.surface_planes {
            planes
                .entry(plane.surface)
                .and_modify(|unique| *unique = None)
                .or_insert(Some(plane));
        }
        let mut surfaces: HashMap<i64, Option<i64>> = HashMap::new();
        for binding in &topology.face_surfaces {
            surfaces
                .entry(binding.entity)
                .and_modify(|surface| *surface = None)
                .or_insert(Some(binding.carrier));
        }
        Self(
            surfaces
                .into_iter()
                .filter_map(|(face, surface)| Some((face, (*planes.get(&surface?)?)?)))
                .collect(),
        )
    }

    fn get(&self, face: i64) -> Option<&'a AsmHistoricalPlane> {
        self.0.get(&face).copied()
    }
}

fn parallel(normal: Vector3, frame: &PlaneFrame) -> bool {
    normal
        .unit()
        .is_some_and(|normal| (normal.dot(frame.normal()).abs() - 1.0).abs() <= PARALLEL_CAP)
}

/// What the inserted caps at the search length from a plane state about a
/// set of preceding faces, one per member, that holds every resolved face.
fn cap_profile_faces(search: &CapSearch<'_>) -> CapReading {
    let &CapSearch {
        preceding,
        topology,
        inserted,
        length,
        members,
        resolved,
        changed,
        new_body,
        tolerance,
        limits,
    } = search;
    let could_be_profile = |faces: &[i64]| {
        new_body
            || faces
                .iter()
                .any(|face| resolved.contains(face) || changed.contains(face))
    };
    let oversized = |topology: &AsmHistoricalTopology, face: i64| {
        edge_count(topology, face).is_some_and(|edges| edges > limits.face_edges)
    };
    let preceding_planes = FacePlanes::of(preceding);
    let current_planes = FacePlanes::of(topology);
    let planar: Vec<(i64, &AsmHistoricalPlane)> = inserted
        .iter()
        .filter_map(|face| current_planes.get(*face).map(|plane| (*face, plane)))
        .collect();
    let mut grouped = vec![false; planar.len()];
    let mut selections: Vec<Vec<i64>> = Vec::new();
    for first in 0..planar.len() {
        if grouped[first] {
            continue;
        }
        let Some(frame) = PlaneFrame::of_plane(planar[first].1.origin, planar[first].1.normal)
        else {
            return CapReading::Disputed;
        };
        let mut cap_faces = Vec::new();
        for (index, (face, plane)) in planar.iter().enumerate() {
            if !grouped[index]
                && parallel(plane.normal, &frame)
                && frame.height(plane.origin).abs() <= tolerance
            {
                grouped[index] = true;
                cap_faces.push(*face);
            }
        }
        for height in [length, -length] {
            let pool: Vec<i64> = preceding
                .faces
                .iter()
                .copied()
                .filter(|face| {
                    preceding_planes.get(*face).is_some_and(|plane| {
                        parallel(plane.normal, &frame)
                            && (frame.height(plane.origin) - height).abs() <= tolerance
                    })
                })
                .collect();
            if pool.is_empty() || !could_be_profile(&pool) {
                continue;
            }
            if pool.len() > limits.pool_faces
                || cap_faces.iter().any(|face| oversized(topology, *face))
            {
                return CapReading::Capped;
            }
            let Some(caps) = cap_faces
                .iter()
                .map(|face| face_cap(&frame, topology, *face, tolerance))
                .collect::<Option<Vec<Cap>>>()
            else {
                return CapReading::Disputed;
            };
            let mut selected = Vec::new();
            let mut outlines = Vec::new();
            let mut unread_in_cap = false;
            let mut unread_oversized = false;
            for face in pool {
                if oversized(preceding, face) {
                    if could_be_profile(&[face]) {
                        return CapReading::Capped;
                    }
                    unread_oversized = true;
                    continue;
                }
                let outline = face_cap(&frame, preceding, face, tolerance);
                match outline
                    .as_ref()
                    .and_then(|outline| interior_point(outline, tolerance))
                {
                    Some(point) => {
                        if caps.iter().any(|cap| cap.contains(point)) {
                            selected.push(face);
                            outlines.extend(outline);
                        }
                    }
                    None => {
                        let rings = match outline {
                            Some(outline) => Some(outline.loops),
                            None => face_vertex_rings(&frame, preceding, face),
                        };
                        if rings.is_none_or(|rings| reaches_a_cap(&rings, &caps, tolerance)) {
                            if could_be_profile(&[face]) {
                                return CapReading::Disputed;
                            }
                            unread_in_cap = true;
                        }
                    }
                }
            }
            if selected.is_empty() || !could_be_profile(&selected) {
                continue;
            }
            if unread_oversized {
                return CapReading::Capped;
            }
            if unread_in_cap {
                return CapReading::Disputed;
            }
            let points: usize = outlines
                .iter()
                .flat_map(|outline| outline.loops.iter())
                .map(Vec::len)
                .sum();
            if points > limits.outline_points {
                return CapReading::Capped;
            }
            let cap_area: f64 = caps.iter().map(|cap| cap.area).sum();
            let selected_area: f64 = outlines.iter().map(|outline| outline.area).sum();
            if selected.len() != members
                || !resolved.iter().all(|face| selected.contains(face))
                || (selected_area - cap_area).abs() > AREA_AGREEMENT * cap_area.max(1.0)
                || !outlines_agree(&caps, &outlines, tolerance)
            {
                return CapReading::Disputed;
            }
            selected.sort_unstable();
            if !selections.contains(&selected) {
                selections.push(selected);
            }
        }
    }
    match <[Vec<i64>; 1]>::try_from(selections) {
        Ok([selection]) => CapReading::Selected(selection),
        Err(selections) if selections.is_empty() => CapReading::Absent,
        Err(_) => CapReading::Disputed,
    }
}

/// The deleted faces, sorted by slot, when they are as many as the members,
/// are preceding faces, hold every resolved face, and all lie on the
/// resolved faces' plane.
fn deleted_profile_faces(
    preceding: &AsmHistoricalTopology,
    deleted: &[i64],
    members: usize,
    resolved: &[i64],
    tolerance: f64,
) -> Option<Vec<i64>> {
    let mut faces = deleted.to_vec();
    faces.sort_unstable();
    faces.dedup();
    if faces.len() != deleted.len()
        || faces.len() != members
        || !resolved.iter().all(|face| faces.contains(face))
        || !faces.iter().all(|face| preceding.faces.contains(face))
    {
        return None;
    }
    let planes = FacePlanes::of(preceding);
    let plane = planes.get(*resolved.first()?)?;
    let frame = PlaneFrame::of_plane(plane.origin, plane.normal)?;
    faces
        .iter()
        .all(|face| {
            planes.get(*face).is_some_and(|plane| {
                parallel(plane.normal, &frame) && frame.height(plane.origin).abs() <= tolerance
            })
        })
        .then_some(faces)
}

/// The vertex positions of each loop of `face` in `frame`, in loop order.
fn face_vertex_rings(
    frame: &PlaneFrame,
    topology: &AsmHistoricalTopology,
    face: i64,
) -> Option<Vec<Vec<Point2>>> {
    Some(
        oriented_loops(topology, face)?
            .into_iter()
            .map(|steps| {
                steps
                    .into_iter()
                    .map(|(_, from, _)| frame.project(from))
                    .collect()
            })
            .collect(),
    )
}

/// The loop edges of `face`, when its loops are readable.
fn edge_count(topology: &AsmHistoricalTopology, face: i64) -> Option<usize> {
    relation(&topology.face_loops, face)?
        .iter()
        .try_fold(0usize, |edges, loop_| {
            edges.checked_add(relation(&topology.loop_coedges, *loop_)?.len())
        })
}

/// Whether a face drawn by its vertex rings reaches into a cap: no ring has
/// three vertices, a ring vertex lies inside a cap away from its boundary, a
/// cap vertex lies inside the rings away from them, or the mean of the
/// largest ring's vertices lies inside both.
fn reaches_a_cap(rings: &[Vec<Point2>], caps: &[Cap], tolerance: f64) -> bool {
    let inside_a_cap = |point: Point2| {
        caps.iter()
            .any(|cap| cap.contains(point) && cap.boundary_distance(point) > tolerance)
    };
    let ring_area = |ring: &[Point2]| {
        cadmpeg_ir::math::planar::polygon_area_twice(ring).map_or(0.0, |area| area.get().abs())
    };
    let Some(largest) = rings
        .iter()
        .filter(|ring| ring.len() >= 3)
        .max_by(|a, b| ring_area(a).total_cmp(&ring_area(b)))
    else {
        return true;
    };
    let (u, v, count) = largest
        .iter()
        .fold((0.0, 0.0, 0.0), |(u, v, count), point| {
            (u + point.u, v + point.v, count + 1.0)
        });
    let mean = Point2::new(u / count, v / count);
    let inside_the_face = |point: Point2| {
        let parity = rings
            .iter()
            .filter(|ring| point_in_polygon(point, ring))
            .count()
            % 2
            == 1;
        let clearance = rings
            .iter()
            .flat_map(|ring| (0..ring.len()).map(move |k| (ring[k], ring[(k + 1) % ring.len()])))
            .map(|segment| point_segment_distance(point, segment))
            .fold(f64::INFINITY, f64::min);
        parity && clearance > tolerance
    };
    (inside_the_face(mean) && inside_a_cap(mean))
        || rings.iter().flatten().any(|point| inside_a_cap(*point))
        || caps
            .iter()
            .flat_map(|cap| cap.corners.iter())
            .any(|corner| inside_the_face(*corner))
}

/// A point inside `face`, outside its holes, farthest from its boundary on a
/// grid over its loops' bounds.
fn interior_point(face: &Cap, tolerance: f64) -> Option<Point2> {
    let (mut low, mut high) = (
        Point2::new(f64::INFINITY, f64::INFINITY),
        Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for point in face.loops.iter().flatten() {
        low = Point2::new(low.u.min(point.u), low.v.min(point.v));
        high = Point2::new(high.u.max(point.u), high.v.max(point.v));
    }
    let mut best: Option<(Point2, f64)> = None;
    for i in 1..FACE_SAMPLE_GRID {
        for j in 1..FACE_SAMPLE_GRID {
            let point = Point2::new(
                low.u + (high.u - low.u) * f64::from(i) / f64::from(FACE_SAMPLE_GRID),
                low.v + (high.v - low.v) * f64::from(j) / f64::from(FACE_SAMPLE_GRID),
            );
            if !face.contains(point) {
                continue;
            }
            let room = face.boundary_distance(point);
            if best.is_none_or(|(_, most)| room > most) {
                best = Some((point, room));
            }
        }
    }
    let (point, room) = best?;
    (room > FACE_CLEARANCE * tolerance).then_some(point)
}

/// Whether the outline of the union of `faces` lies on the caps' boundary
/// and every cap vertex lies on that outline.
///
/// A face boundary segment is on the outline when the union holds one of its
/// sides and not the other; both its ends must then lie on a cap piece.
fn outlines_agree(caps: &[Cap], faces: &[Cap], tolerance: f64) -> bool {
    let in_union = |point: Point2| faces.iter().any(|face| face.contains(point));
    let on_caps = |point: Point2| {
        caps.iter()
            .map(|cap| cap.boundary_distance(point))
            .fold(f64::INFINITY, f64::min)
            <= tolerance
    };
    let offset = SIDE_OFFSET * tolerance;
    let mut outline = Vec::new();
    for ring in faces.iter().flat_map(|face| face.loops.iter()) {
        for k in 0..ring.len() {
            let (start, end) = (ring[k], ring[(k + 1) % ring.len()]);
            let length = distance(start, end);
            if length <= tolerance {
                continue;
            }
            let middle = Point2::new(f64::midpoint(start.u, end.u), f64::midpoint(start.v, end.v));
            let (du, dv) = (
                -(end.v - start.v) / length * offset,
                (end.u - start.u) / length * offset,
            );
            let left = in_union(Point2::new(middle.u + du, middle.v + dv));
            let right = in_union(Point2::new(middle.u - du, middle.v - dv));
            if left == right {
                continue;
            }
            if !on_caps(start) || !on_caps(end) {
                return false;
            }
            outline.push((start, end));
        }
    }
    caps.iter()
        .flat_map(|cap| cap.corners.iter())
        .all(|corner| {
            outline
                .iter()
                .any(|segment| point_segment_distance(*corner, *segment) <= tolerance)
        })
}

#[cfg(test)]
mod tests;
