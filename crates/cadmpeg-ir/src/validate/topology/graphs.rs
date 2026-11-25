// SPDX-License-Identifier: Apache-2.0
//! Radial-ring, child-list, wire ownership and shell connectivity checks.

use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::topology::RegionUnstated;
use crate::validate::record_finding;

#[derive(Clone, Copy)]
enum RadialStatus {
    Closed(usize),
    DoesNotClose,
    CrossesEdge,
}

pub(in crate::validate) fn check_coedge_pairing(
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let by_id = BorrowedIdentities::build(|add| {
        for coedge in &ir.geometry.coedges {
            add(coedge.id.as_str(), coedge);
        }
    });
    let mut statuses = BorrowedIdentities::new();
    for coedge in &ir.geometry.coedges {
        let start = coedge.id.as_str();
        if statuses.contains(start) {
            continue;
        }
        let expected_edge = &coedge.edge;
        let mut path = Vec::<&str>::new();
        let mut positions = BorrowedIdentities::new();
        let mut current = start;
        loop {
            if let Some(status) = statuses.get(current).copied() {
                let status = match status {
                    RadialStatus::Closed(_) => RadialStatus::DoesNotClose,
                    status => status,
                };
                for member in path {
                    statuses.insert(member, status);
                }
                break;
            }
            if let Some(&cycle_start) = positions.get(current) {
                let cycle_len = path.len() - cycle_start;
                for &member in &path[cycle_start..] {
                    statuses.insert(member, RadialStatus::Closed(cycle_len));
                }
                for &member in &path[..cycle_start] {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            }
            positions.insert(current, path.len());
            path.push(current);
            let Some(current_coedge) = by_id.get(current) else {
                for member in path {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            };
            let Some(next) = by_id.get(current_coedge.radial_next.as_str()) else {
                for member in path {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            };

            if next.edge != *expected_edge {
                for member in path {
                    statuses.insert(member, RadialStatus::CrossesEdge);
                }
                break;
            }
            current = next.id.as_str();
        }
    }
    for coedge in &ir.geometry.coedges {
        let owner = Some(coedge.id.as_str());
        let status = statuses
            .get(coedge.id.as_str())
            .copied()
            .ok_or_else(|| CodecError::malformed("radial ring has no classified status"))?;
        match status {
            RadialStatus::CrossesEdge => {
                record_finding(
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    crate::rule!(
                        "topology.radial-ring-crosses-edges",
                        "radial ring crosses edges"
                    ),
                );
                record_finding(
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    crate::rule!(
                        "topology.radial-ring-does-not-close",
                        "radial ring does not close"
                    ),
                );
            }
            RadialStatus::DoesNotClose => {
                record_finding(
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    crate::rule!(
                        "topology.radial-ring-does-not-close",
                        "radial ring does not close"
                    ),
                );
            }
            RadialStatus::Closed(2) => {
                if let Some(other) = by_id.get(coedge.radial_next.as_str()) {
                    if other.sense == coedge.sense {
                        record_finding(
                            findings,
                            Check::CoedgePairing,
                            Severity::Warning,
                            owner,
                            crate::rule!(
                                "topology.two-member-radial-ring-equal-coedge",
                                "two-member radial ring has equal coedge senses"
                            ),
                        );
                    }
                }
            }
            RadialStatus::Closed(_) => {}
        }
    }
    Ok(())
}

pub(in crate::validate) fn check_wire_topology(ir: &CadIr, findings: &mut Vec<Finding>) {
    let coedge_edges = BorrowedIdentities::build(|add| {
        for coedge in &ir.geometry.coedges {
            add(coedge.edge.as_str(), ());
        }
    });
    let edge_vertices = BorrowedIdentities::build(|add| {
        for edge in &ir.geometry.edges {
            for vertex in edge.vertices() {
                add(vertex.as_str(), ());
            }
        }
    });
    let loop_vertices = BorrowedIdentities::build(|add| {
        for loop_ in &ir.geometry.loops {
            for vertex in loop_.vertices() {
                add(vertex.as_str(), ());
            }
        }
    });
    let wire_owners = BorrowedIdentities::build(|add| {
        for shell in &ir.geometry.shells {
            for edge in shell.wire_edges() {
                add(edge.as_str(), ());
            }
        }
    });
    let free_owners = BorrowedIdentities::build(|add| {
        for shell in &ir.geometry.shells {
            for vertex in shell.free_vertices() {
                add(vertex.as_str(), ());
            }
        }
    });
    for shell in &ir.geometry.shells {
        for edge in shell.wire_edges() {
            if coedge_edges.contains(edge.as_str()) {
                record_finding(
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    crate::rule!(
                        "topology.wire-edge-also-referenced-coedge",
                        "wire edge is also referenced by a coedge"
                    ),
                );
            }
        }
        for vertex in shell.free_vertices() {
            if edge_vertices.contains(vertex.as_str()) {
                record_finding(
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    crate::rule!(
                        "topology.free-vertex-also-referenced-edge",
                        "free vertex is also referenced by an edge"
                    ),
                );
            }
        }
    }
    for edge in &ir.geometry.edges {
        if !coedge_edges.contains(edge.id.as_str())
            && wire_owners.match_count(edge.id.as_str()) != 1
        {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(edge.id.as_str()),
                crate::rule!(
                    "topology.wire-edge-must-belong-exactly-one",
                    "wire edge must belong to exactly one shell"
                ),
            );
        }
    }
    for vertex in &ir.geometry.vertices {
        let owner_count = free_owners.match_count(vertex.id.as_str());
        if owner_count > 1
            || (!edge_vertices.contains(vertex.id.as_str())
                && !loop_vertices.contains(vertex.id.as_str())
                && owner_count != 1)
        {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(vertex.id.as_str()),
                crate::rule!(
                    "topology.free-vertex-must-belong-exactly-one",
                    "free vertex must belong to exactly one shell"
                ),
            );
        }
    }
    let region_bodies = BorrowedIdentities::build(|add| {
        for region in &ir.geometry.regions {
            add(region.id.as_str(), region.body.as_str());
        }
    });
    let shell_bodies = BorrowedIdentities::build(|add| {
        for shell in &ir.geometry.shells {
            if let Some(body) = region_bodies.get(shell.region.as_str()) {
                add(shell.id.as_str(), body);
            }
        }
    });
    let bodies_with_faces = BorrowedIdentities::build(|add| {
        for face in &ir.geometry.faces {
            if let Some(body) = shell_bodies.get(face.shell.as_str()) {
                add(body, ());
            }
        }
    });
    for body in &ir.geometry.bodies {
        if body.kind == crate::topology::BodyKind::Wire
            && bodies_with_faces.contains(body.id.as_str())
        {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Warning,
                Some(body.id.as_str()),
                crate::rule!(
                    "topology.wire-body-contains-faces",
                    "wire body contains faces"
                ),
            );
        }
    }
    check_cage_shells(ir, findings);
}

/// A subdivision cage belongs to at most one shell, and a shell whose
/// `members` name a cage names nothing else, holds no face, and is the only
/// shell of its body's only region (§5.6).
fn check_cage_shells(ir: &CadIr, findings: &mut Vec<Finding>) {
    let cage_owners = BorrowedIdentities::build(|add| {
        for shell in &ir.geometry.shells {
            for cage in shell.subds() {
                add(cage.as_str(), ());
            }
        }
    });
    for cage in &ir.geometry.subds {
        if cage_owners.match_count(cage.id.as_str()) > 1 {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(cage.id.as_str()),
                crate::rule!(
                    "topology.cage-belongs-more-one-shell",
                    "subdivision cage belongs to more than one shell"
                ),
            );
        }
    }
    let face_shells = BorrowedIdentities::build(|add| {
        for face in &ir.geometry.faces {
            add(face.shell.as_str(), ());
        }
    });
    let region_shells = BorrowedIdentities::build(|add| {
        for shell in &ir.geometry.shells {
            add(shell.region.as_str(), ());
        }
    });
    let region_bodies = BorrowedIdentities::build(|add| {
        for region in &ir.geometry.regions {
            add(region.id.as_str(), region.body.as_str());
        }
    });
    let body_regions = BorrowedIdentities::build(|add| {
        for region in &ir.geometry.regions {
            add(region.body.as_str(), ());
        }
    });
    for shell in &ir.geometry.shells {
        if shell.subds().next().is_none() {
            continue;
        }
        let owner = Some(shell.id.as_str());
        if shell.members.len() > 1 {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                owner,
                crate::rule!(
                    "topology.cage-shell-holds-other-members",
                    "shell holding a subdivision cage holds other members"
                ),
            );
        }
        if face_shells.contains(shell.id.as_str()) {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                owner,
                crate::rule!(
                    "topology.cage-shell-holds-faces",
                    "shell holding a subdivision cage holds faces"
                ),
            );
        }
        let only_region = region_bodies
            .get(shell.region.as_str())
            .is_none_or(|body| body_regions.match_count(body) == 1);
        if region_shells.match_count(shell.region.as_str()) > 1 || !only_region {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                owner,
                crate::rule!(
                    "topology.cage-shell-not-only-shell-body",
                    "shell holding a subdivision cage is not the only shell of its body's only region"
                ),
            );
        }
    }
}

/// Each stored child list names children whose parent pointer names its
/// owner, and each child is listed by the owner its parent pointer names: a
/// region's `shells`, a face's `loops`, and a loop ring's `coedges` (D29). A
/// shell's `members` name each wire edge, free vertex, and cage once.
pub(in crate::validate) fn check_child_list_reciprocity(ir: &CadIr, findings: &mut Vec<Finding>) {
    let shell_regions = BorrowedIdentities::build(|add| {
        for shell in &ir.geometry.shells {
            add(shell.id.as_str(), shell.region.as_str());
        }
    });
    let loop_faces = BorrowedIdentities::build(|add| {
        for loop_ in &ir.geometry.loops {
            add(loop_.id.as_str(), loop_.face.as_str());
        }
    });
    let coedge_loops = BorrowedIdentities::build(|add| {
        for coedge in &ir.geometry.coedges {
            add(coedge.id.as_str(), coedge.owner_loop.as_str());
        }
    });
    let mut listed_shells = BorrowedIdentities::new();
    for region in &ir.geometry.regions {
        let marked = matches!(
            region.unstated,
            Some(RegionUnstated::Exterior { .. } | RegionUnstated::Unclassified { .. })
        );
        let mut seen = BorrowedIdentities::new();
        for shell in region.stated_shells() {
            if marked && seen.insert(shell.as_str(), ()).is_some() {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(region.id.as_str()),
                    crate::rule!(
                        "topology.region-mark-names-shell-twice-one",
                        "region's mark names shell `{}` twice, or one its `shells` names",
                        shell.as_str()
                    ),
                );
            }
            listed_shells.insert(shell.as_str(), region.id.as_str());
            if shell_regions
                .get(shell.as_str())
                .is_some_and(|owner| *owner != region.id.as_str())
            {
                reciprocity_error(findings, region.id.as_str(), "shell", shell.as_str());
            }
        }
    }
    let mut listed_loops = BorrowedIdentities::new();
    for face in &ir.geometry.faces {
        let mut seen = BorrowedIdentities::new();
        for loop_ in face.loops.stated() {
            listed_loops.insert(loop_.as_str(), face.id.as_str());
            if seen.insert(loop_.as_str(), ()).is_some() {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(face.id.as_str()),
                    crate::rule!(
                        "topology.face-lists-loop-more-once",
                        "face lists loop `{}` more than once",
                        loop_.as_str()
                    ),
                );
            }
            if loop_faces
                .get(loop_.as_str())
                .is_some_and(|owner| *owner != face.id.as_str())
            {
                reciprocity_error(findings, face.id.as_str(), "loop", loop_.as_str());
            }
        }
    }
    let mut listed_coedges = BorrowedIdentities::new();
    for loop_ in &ir.geometry.loops {
        for coedge in loop_.coedges() {
            listed_coedges.insert(coedge.as_str(), loop_.id.as_str());
            if coedge_loops
                .get(coedge.as_str())
                .is_some_and(|owner| *owner != loop_.id.as_str())
            {
                reciprocity_error(findings, loop_.id.as_str(), "coedge", coedge.as_str());
            }
        }
    }
    for shell in &ir.geometry.shells {
        if listed_shells.get(shell.id.as_str()) != Some(&shell.region.as_str()) {
            unlisted_error(findings, shell.id.as_str(), "region", shell.region.as_str());
        }
        let mut seen = BorrowedIdentities::new();
        for member in shell
            .members
            .iter()
            .map(crate::topology::ShellMember::as_str)
        {
            if seen.insert(member, ()).is_some() {
                record_finding(
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    crate::rule!(
                        "topology.shell-lists-member-more-once",
                        "shell lists member `{member}` more than once"
                    ),
                );
            }
        }
    }
    for loop_ in &ir.geometry.loops {
        if listed_loops.get(loop_.id.as_str()) != Some(&loop_.face.as_str()) {
            unlisted_error(findings, loop_.id.as_str(), "face", loop_.face.as_str());
        }
    }
    for coedge in &ir.geometry.coedges {
        if listed_coedges.get(coedge.id.as_str()) != Some(&coedge.owner_loop.as_str()) {
            unlisted_error(
                findings,
                coedge.id.as_str(),
                "loop",
                coedge.owner_loop.as_str(),
            );
        }
    }
}

fn reciprocity_error(findings: &mut Vec<Finding>, owner: &str, kind: &str, child: &str) {
    record_finding(
        findings,
        Check::ReferentialIntegrity,
        Severity::Error,
        Some(owner),
        crate::rule!(
            "topology.lists-parent-another-record",
            "lists {kind} `{child}`, whose parent is another record"
        ),
    );
}

fn unlisted_error(findings: &mut Vec<Finding>, child: &str, kind: &str, parent: &str) {
    record_finding(
        findings,
        Check::ReferentialIntegrity,
        Severity::Error,
        Some(child),
        crate::rule!(
            "topology.not-listed",
            "is not listed by its {kind} `{parent}`"
        ),
    );
}

/// An edge with no vertex has a carrier curve and no `param_range`, and each
/// of its coedges is the only coedge of its loop. The carrier's closure is
/// checked with the edge endpoints.
pub(in crate::validate) fn check_closed_edges(ir: &CadIr, findings: &mut Vec<Finding>) {
    if ir.geometry.edges.iter().all(|edge| edge.ends.is_some()) {
        return;
    }
    let vertexless_edges = BorrowedIdentities::build(|add| {
        for edge in &ir.geometry.edges {
            if edge.ends.is_none() {
                add(edge.id.as_str(), ());
            }
        }
    });
    for edge in &ir.geometry.edges {
        if edge.ends.is_none() && (edge.curve().is_none() || edge.param_range().is_some()) {
            record_finding(
                findings,
                Check::GeometricConsistency,
                Severity::Error,
                Some(edge.id.as_str()),
                crate::rule!(
                    "topology.edge-no-vertex-must-carrier-curve",
                    "edge with no vertex must have a carrier curve and no parameter range"
                ),
            );
        }
    }
    let ring_lengths = BorrowedIdentities::build(|add| {
        for loop_ in &ir.geometry.loops {
            add(loop_.id.as_str(), loop_.coedges().len());
        }
    });
    for coedge in &ir.geometry.coedges {
        if vertexless_edges.contains(coedge.edge.as_str())
            && ring_lengths
                .get(coedge.owner_loop.as_str())
                .is_some_and(|length| *length != 1)
        {
            record_finding(
                findings,
                Check::RingOrder,
                Severity::Error,
                Some(coedge.id.as_str()),
                crate::rule!(
                    "topology.coedge-edge-no-vertex-must-only",
                    "coedge of an edge with no vertex must be the only coedge of its loop"
                ),
            );
        }
    }
}

/// An edge or vertex that faces share.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Incidence<'ir> {
    Edge(&'ir str),
    Vertex(&'ir str),
}

fn insert_incidence<'ir>(
    incidences: &mut BorrowedIdentities<'ir, Vec<Incidence<'ir>>>,
    face: &'ir str,
    incidence: Incidence<'ir>,
) {
    if let Some(face_incidences) = incidences.get_mut(face) {
        face_incidences.push(incidence);
    } else {
        incidences.insert(face, vec![incidence]);
    }
}

/// The root of `node`, halving the path on the way.
fn component_root(parents: &mut [usize], mut node: usize) -> usize {
    while parents[node] != node {
        parents[node] = parents[parents[node]];
        node = parents[node];
    }
    node
}

/// Whether `faces` form one component when two faces are joined by every
/// edge or vertex they share.
fn faces_connected<'ir>(
    faces: &BorrowedIdentities<'ir>,
    incidences: &BorrowedIdentities<'ir, Vec<Incidence<'ir>>>,
) -> bool {
    let mut parents: Vec<usize> = (0..faces.len()).collect();
    let mut components = faces.len();
    let mut first_faces = std::collections::HashMap::new();
    for (face_index, face) in faces.identities().enumerate() {
        for incidence in incidences.get(face).into_iter().flatten() {
            let first = *first_faces.entry(*incidence).or_insert(face_index);
            let (first, current) = (
                component_root(&mut parents, first),
                component_root(&mut parents, face_index),
            );
            if first != current {
                parents[current] = first;
                components -= 1;
            }
        }
    }
    components <= 1
}

pub(in crate::validate) fn check_shell_connectivity(ir: &CadIr, findings: &mut Vec<Finding>) {
    let loop_faces = BorrowedIdentities::build(|add| {
        for loop_ in &ir.geometry.loops {
            add(loop_.id.as_str(), loop_.face.as_str());
        }
    });
    let edges = BorrowedIdentities::build(|add| {
        for edge in &ir.geometry.edges {
            add(edge.id.as_str(), edge);
        }
    });
    let mut incidences = BorrowedIdentities::new();
    for coedge in &ir.geometry.coedges {
        let Some(face) = loop_faces.get(coedge.owner_loop.as_str()).copied() else {
            continue;
        };
        insert_incidence(&mut incidences, face, Incidence::Edge(coedge.edge.as_str()));
        if let Some(edge) = edges.get(coedge.edge.as_str()) {
            for vertex in edge.vertices() {
                insert_incidence(&mut incidences, face, Incidence::Vertex(vertex.as_str()));
            }
        }
    }
    for loop_ in &ir.geometry.loops {
        let Some(face) = loop_faces.get(loop_.id.as_str()).copied() else {
            continue;
        };
        match &loop_.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, .. } => {
                insert_incidence(&mut incidences, face, Incidence::Vertex(vertex.as_str()));
            }
            crate::topology::LoopBoundary::Ring(ring) => {
                for use_ in ring.vertex_uses() {
                    insert_incidence(
                        &mut incidences,
                        face,
                        Incidence::Vertex(use_.vertex.as_str()),
                    );
                }
            }
        }
    }
    let child_lists = ir.geometry.child_lists();
    for shell in &ir.geometry.shells {
        let Some(shell_faces) = child_lists.complete_shell_faces(&shell.id) else {
            continue;
        };
        if shell_faces.len() < 2
            || shell_faces.iter().any(|face| {
                face.loops
                    .boundary()
                    .is_none_or(crate::topology::FaceBoundary::is_empty)
            })
        {
            continue;
        }
        let mut owned = BorrowedIdentities::new();
        owned.extend_unique(shell_faces.iter().map(|face| face.id.as_str()));
        if !faces_connected(&owned, &incidences) {
            record_finding(
                findings,
                Check::ShellTopology,
                Severity::Error,
                Some(shell.id.as_str()),
                crate::rule!(
                    "topology.shell-faces-disconnected-through-shared-edges",
                    "shell faces are disconnected through shared edges or vertices"
                ),
            );
        }
    }
}

/// Every edge that a face of a `Solid` body uses is used at least twice by
/// the faces of that body, unless the edge has no curve or a degenerate one
/// (Warning; part 10, Topology and model records). A body that holds a
/// partial face or a marked region or shell, or is marked itself, is not
/// read.
pub(in crate::validate) fn check_solid_edge_uses(ir: &CadIr, findings: &mut Vec<Finding>) {
    use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
    use crate::topology::{BodyKind, EdgeCarrier};
    let geometry = &ir.geometry;
    let child_lists = geometry.child_lists();
    let loop_faces: std::collections::HashMap<&str, &str> = geometry
        .loops
        .iter()
        .map(|loop_| (loop_.id.as_str(), loop_.face.as_str()))
        .collect();
    let edges: std::collections::HashMap<&str, &crate::topology::Edge> = geometry
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect();
    let degenerate: std::collections::HashSet<&str> = geometry
        .curves
        .iter()
        .filter(|curve| {
            matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
            )
        })
        .map(|curve| curve.id.as_str())
        .collect();
    for body in geometry
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Solid)
    {
        let Some(regions) = child_lists.complete_body_regions(&body.id) else {
            continue;
        };
        let mut faces = std::collections::HashSet::new();
        let complete = regions.iter().all(|region| {
            child_lists.complete_shells(region).is_some_and(|shells| {
                shells.iter().all(|shell| {
                    child_lists
                        .complete_shell_faces(shell)
                        .is_some_and(|shell_faces| {
                            shell_faces.iter().all(|face| {
                                faces.insert(face.id.as_str());
                                !face.loops.is_partial()
                            })
                        })
                })
            })
        });
        if !complete {
            continue;
        }
        let mut uses: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for coedge in &geometry.coedges {
            if loop_faces
                .get(coedge.owner_loop.as_str())
                .is_some_and(|face| faces.contains(face))
            {
                *uses.entry(coedge.edge.as_str()).or_default() += 1;
            }
        }
        for (edge, count) in uses {
            let curved = edges.get(edge).is_some_and(|record| match &record.carrier {
                EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _) => {
                    !degenerate.contains(curve.as_str())
                }
                EdgeCarrier::Free | EdgeCarrier::Endpoints(_) => false,
            });
            if count == 1 && curved {
                super::super::record_related_finding(
                    findings,
                    Check::ShellTopology,
                    Severity::Warning,
                    edge,
                    &[body.id.as_str()],
                    crate::rule!("topology.edge-used-once-faces-solid-body", "edge is used once by the faces of solid body {}; a solid's faces use each of its edges at least twice", body.id.as_str()),
                );
            }
        }
    }
}

/// A partial face and a marked region, shell, or body are reported at
/// Warning (part 10, Topology and model records).
pub(in crate::validate) fn check_partial_topology(ir: &CadIr, findings: &mut Vec<Finding>) {
    let geometry = &ir.geometry;
    let partial = geometry
        .bodies
        .iter()
        .filter(|body| body.unstated.is_some())
        .map(|body| (body.id.as_str(), "body does not state every region"))
        .chain(
            geometry
                .regions
                .iter()
                .filter(|region| region.unstated.is_some())
                .map(|region| (region.id.as_str(), "region does not state every shell")),
        )
        .chain(
            geometry
                .shells
                .iter()
                .filter(|shell| shell.unstated.is_some())
                .map(|shell| (shell.id.as_str(), "shell does not state every member")),
        )
        .chain(
            geometry
                .faces
                .iter()
                .filter(|face| face.loops.is_partial())
                .map(|face| (face.id.as_str(), "face does not state every loop")),
        );
    for (id, message) in partial {
        record_finding(
            findings,
            Check::TopologyPartial,
            Severity::Warning,
            Some(id),
            crate::rule!("topology.unstated-children", "{message}"),
        );
    }
}

#[cfg(test)]
mod tests;
