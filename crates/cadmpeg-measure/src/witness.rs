// SPDX-License-Identifier: Apache-2.0
//! Witnesses of faces whose region rests on their declared orientation.
//!
//! A face is established when its loops and its sense agree, or when a
//! witness confirms its declared reading. Witnesses run over each shell
//! breadth first from its established faces.
//!
//! - Tangent edge: a non-seam edge used by exactly two coedges with
//!   opposite senses, one in the unconfirmed face and one in an established
//!   face, where the two oriented normals are parallel within
//!   `TANGENT_WITNESS_ANGLE` at both vertices and at the midpoint. The edge
//!   votes `agree` when the normals point the same way at every point and
//!   `disagree` when they point opposite ways at every point. A face with
//!   votes that all agree is confirmed; a face with a disagreeing vote is
//!   contradicted. A tangent edge is taken as a smooth continuation of the
//!   two faces.
//! - Shell sign: in a closed shell whose faces are all established except
//!   one unconfirmed face, the shell volume must be positive for an outer
//!   shell and negative for a void. When exactly one reading of that face
//!   gives the required sign beyond the volume error, it is the face's
//!   reading.
//! - Shared sphere: unconfirmed faces of one sphere linked by edges with
//!   opposite senses and agreeing normals lie on opposite sides of each edge,
//!   so their readings flip together. Regions of one sphere have disjoint
//!   interiors: the declared readings need a total area of at most the
//!   sphere's, and the `k` complements need at least `k - 1` spheres' area.
//!   When exactly one holds beyond the errors, it is every linked face's
//!   reading.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Coedge, Face};

use crate::body::closure;
use crate::boundary::FaceFrame;
use crate::budget::PassAllowance;
use crate::canonical::placement::Placement;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::edges::EdgeGeometry;
use crate::face::{FaceResult, Verdict};
use crate::failure::{settle, Measure, Outcome};
use crate::owners::Owners;
use crate::report::{NormalReading, RegionRule, Witness, WitnessKind};
use crate::tolerance::TANGENT_WITNESS_ANGLE;

/// What the witness passes read.
pub(crate) struct WitnessInput<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
    pub(crate) pass: &'a PassAllowance,
    pub(crate) eval: Eval<'a, 'ir>,
    /// The placement of each body.
    pub(crate) placements: &'a HashMap<&'ir str, Outcome<Placement>>,
}

/// The sign of an oriented normal reading.
fn sign(reading: NormalReading) -> f64 {
    match reading {
        NormalReading::Reversed => -1.0,
        _ => 1.0,
    }
}

/// Whether a face's reading is established.
fn established(result: &FaceResult<'_>) -> bool {
    match result.verdict {
        Verdict::Confirmed => true,
        Verdict::Settled => {
            result.row.orientation.consistent == Some(true)
                || result.row.measures.region == Some(RegionRule::WholeSurface)
        }
        Verdict::Conflict | Verdict::Unconfirmed | Verdict::Contradicted => false,
    }
}

/// One edge's vote on a face's declared reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vote {
    Agree,
    Disagree,
}

/// The unit oriented normal of a face at a model point, or `None` at a
/// singular point or where the point does not invert onto the chart.
fn normal(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    point: Point3,
    alpha: f64,
    orientation: f64,
) -> Measure<Option<Vector3>> {
    if chart.singular_at(point, alpha) {
        return Ok(None);
    }
    let Some([u, v]) = chart.invert(eval, point, None, alpha)? else {
        return Ok(None);
    };
    let partials = chart.partials(eval, u, v)?;
    let normal = partials.du.cross(partials.dv);
    let length = normal.norm();
    if length == 0.0 {
        return Ok(None);
    }
    Ok(Some(normal.scale(orientation / length)))
}

impl<'ir> WitnessInput<'_, 'ir> {
    /// Settle every unconfirmed face that a tangent edge or its shell's
    /// sign decides.
    pub(crate) fn run(
        &self,
        faces: &mut HashMap<&'ir str, FaceResult<'ir>>,
    ) -> Result<(), ResourceLimit> {
        loop {
            let mut pending: Vec<&'ir str> = faces
                .iter()
                .filter(|(_, result)| result.verdict == Verdict::Unconfirmed)
                .map(|(id, _)| *id)
                .collect();
            pending.sort_unstable();
            let mut changed = false;
            for face in pending {
                if let Some((verdict, witness)) = self.tangent(faces, face)? {
                    if let Some(result) = faces.get_mut(face) {
                        result.settle_reading(verdict, Some(witness));
                        changed = true;
                    }
                }
            }
            if !changed && !self.shared_spheres(faces)? {
                break;
            }
        }
        self.shell_signs(faces);
        Ok(())
    }

    /// The tangent-edge verdict of one unconfirmed face, if its edges vote.
    fn tangent(
        &self,
        faces: &HashMap<&'ir str, FaceResult<'ir>>,
        face_id: &'ir str,
    ) -> Result<Option<(Verdict, Witness)>, ResourceLimit> {
        let (Some(face), Some(result)) = (self.owners.face(face_id), faces.get(face_id)) else {
            return Ok(None);
        };
        let Some(chart) = result.chart else {
            return Ok(None);
        };
        let slice = self.pass.face_slice();
        let eval = Eval {
            work: &slice,
            ..self.eval
        };
        let mut votes: Vec<(&'ir str, Vote)> = Vec::new();
        for loop_id in &face.loops {
            let Some(face_loop) = self.owners.loop_(loop_id.as_str()) else {
                continue;
            };
            for coedge_id in face_loop.coedges() {
                let Some(coedge) = self.owners.coedge(coedge_id.as_str()) else {
                    continue;
                };
                let Some(other_face) = self.opposite_face(face_id, face, coedge) else {
                    continue;
                };
                let Some(neighbour) = faces.get(other_face) else {
                    continue;
                };
                if !established(neighbour) {
                    continue;
                }
                let Some(neighbour_chart) = neighbour.chart else {
                    continue;
                };
                let Some(edge) = self.edges.get(coedge.edge.as_str()) else {
                    continue;
                };
                let sides = [
                    (
                        chart,
                        result.alpha,
                        sign(result.row.orientation.declared_normal),
                    ),
                    (
                        neighbour_chart,
                        neighbour.alpha,
                        sign(neighbour.row.orientation.declared_normal),
                    ),
                ];
                let vote: Option<Vote> =
                    settle(edge_vote(&eval, edge, &sides))?.unwrap_or_default();
                if let Some(vote) = vote {
                    votes.push((coedge.edge.as_str(), vote));
                }
            }
        }
        if let Some((edge, _)) = votes.iter().find(|(_, vote)| *vote == Vote::Disagree) {
            return Ok(Some((
                Verdict::Contradicted,
                Witness {
                    kind: WitnessKind::TangentEdge,
                    entity: (*edge).to_owned(),
                    agrees: false,
                },
            )));
        }
        Ok(votes.first().map(|(edge, _)| {
            (
                Verdict::Confirmed,
                Witness {
                    kind: WitnessKind::TangentEdge,
                    entity: (*edge).to_owned(),
                    agrees: true,
                },
            )
        }))
    }

    /// Settle the unconfirmed faces of one sphere that their shared edges
    /// link. Returns whether any face settled.
    fn shared_spheres(
        &self,
        faces: &mut HashMap<&'ir str, FaceResult<'ir>>,
    ) -> Result<bool, ResourceLimit> {
        let mut groups: BTreeMap<&'ir str, &'ir str> = faces
            .iter()
            .filter(|(_, result)| {
                result.verdict == Verdict::Unconfirmed
                    && matches!(result.chart, Some(Chart::Sphere(_)))
            })
            .map(|(id, _)| (*id, *id))
            .collect();
        let mut witnesses: BTreeMap<&'ir str, &'ir str> = BTreeMap::new();
        let members: Vec<&'ir str> = groups.keys().copied().collect();
        for face_id in members {
            let Some(face) = self.owners.face(face_id) else {
                continue;
            };
            for loop_id in &face.loops {
                let Some(face_loop) = self.owners.loop_(loop_id.as_str()) else {
                    continue;
                };
                for coedge_id in face_loop.coedges() {
                    let Some(coedge) = self.owners.coedge(coedge_id.as_str()) else {
                        continue;
                    };
                    let Some(other_face) = self.opposite_face(face_id, face, coedge) else {
                        continue;
                    };
                    let (Some(first), Some(second)) = (faces.get(face_id), faces.get(other_face))
                    else {
                        continue;
                    };
                    if !groups.contains_key(other_face)
                        || !same_sphere(first, second)
                        || self.linked_vote(coedge.edge.as_str(), first, second)?
                            != Some(Vote::Agree)
                    {
                        continue;
                    }
                    let keep = group_of(&groups, face_id);
                    let merged = group_of(&groups, other_face);
                    groups.insert(merged, keep);
                    witnesses.insert(face_id, coedge.edge.as_str());
                    witnesses.insert(other_face, coedge.edge.as_str());
                }
            }
        }
        let mut components: BTreeMap<&'ir str, Vec<&'ir str>> = BTreeMap::new();
        for face in groups.keys() {
            components
                .entry(group_of(&groups, face))
                .or_default()
                .push(face);
        }
        let mut settled = false;
        for members in components.into_values().filter(|group| group.len() > 1) {
            let Some(verdict) = shared_sphere_verdict(faces, &members) else {
                continue;
            };
            for face in members {
                let Some(edge) = witnesses.get(face) else {
                    continue;
                };
                if let Some(result) = faces.get_mut(face) {
                    result.settle_reading(
                        verdict,
                        Some(Witness {
                            kind: WitnessKind::SharedSphere,
                            entity: (*edge).to_owned(),
                            agrees: verdict == Verdict::Confirmed,
                        }),
                    );
                    settled = true;
                }
            }
        }
        Ok(settled)
    }

    /// The other face of a non-seam edge that exactly two coedges with
    /// opposite senses use.
    fn opposite_face(
        &self,
        face_id: &str,
        face: &'ir Face,
        coedge: &'ir Coedge,
    ) -> Option<&'ir str> {
        if FaceFrame::is_seam(self.owners, face, coedge) {
            return None;
        }
        let [first, second] = self.owners.edge_coedges(coedge.edge.as_str()) else {
            return None;
        };
        let other = if first.id == coedge.id { second } else { first };
        let other_face = (other.sense != coedge.sense)
            .then(|| self.owners.coedge_face(other))
            .flatten()?;
        (other_face != face_id).then_some(other_face)
    }

    /// The vote of `edge` on two faces under their declared normals.
    fn linked_vote(
        &self,
        edge: &str,
        first: &FaceResult<'ir>,
        second: &FaceResult<'ir>,
    ) -> Result<Option<Vote>, ResourceLimit> {
        let (Some(chart), Some(other), Some(edge)) =
            (first.chart, second.chart, self.edges.get(edge))
        else {
            return Ok(None);
        };
        let slice = self.pass.face_slice();
        let eval = Eval {
            work: &slice,
            ..self.eval
        };
        let sides = [
            (
                chart,
                first.alpha,
                sign(first.row.orientation.declared_normal),
            ),
            (
                other,
                second.alpha,
                sign(second.row.orientation.declared_normal),
            ),
        ];
        Ok(settle(edge_vote(&eval, edge, &sides))?.unwrap_or_default())
    }

    /// Settle the one unconfirmed face of each closed shell whose other
    /// faces are established, by the sign of the shell's volume.
    fn shell_signs(&self, faces: &mut HashMap<&'ir str, FaceResult<'ir>>) {
        let mut shells: BTreeSet<&'ir str> = BTreeSet::new();
        for (id, result) in faces.iter() {
            if result.verdict == Verdict::Unconfirmed {
                if let Some(face) = self.owners.face(id) {
                    shells.insert(face.shell.as_str());
                }
            }
        }
        for shell_id in shells {
            if let Some((face, verdict)) = self.shell_sign(faces, shell_id) {
                if let Some(result) = faces.get_mut(face) {
                    result.settle_reading(
                        verdict,
                        Some(Witness {
                            kind: WitnessKind::ShellSign,
                            entity: shell_id.to_owned(),
                            agrees: verdict == Verdict::Confirmed,
                        }),
                    );
                }
            }
        }
    }

    /// The shell-sign verdict of a shell's one unconfirmed face.
    fn shell_sign(
        &self,
        faces: &HashMap<&'ir str, FaceResult<'ir>>,
        shell_id: &'ir str,
    ) -> Option<(&'ir str, Verdict)> {
        let shell = self.owners.shell(shell_id)?;
        if shell.faces().is_empty() || !closure(self.owners, faces, self.edges, shell).closed() {
            return None;
        }
        let region = self.owners.region(shell.region.as_str())?;
        let outer = region
            .shells
            .first()
            .is_some_and(|first| *first == shell.id);
        let handedness = match self.placements.get(region.body.as_str()) {
            Some(Ok(placement)) if placement.reflection => -1.0,
            _ => 1.0,
        };
        let mut unconfirmed = None;
        let mut fixed = 0.0;
        let mut error = 0.0;
        for face in shell.faces() {
            let (id, result) = faces.get_key_value(face.as_str())?;
            let Some(Ok(flux)) = &result.flux else {
                return None;
            };
            let sigma = handedness * sign(result.row.orientation.declared_normal);
            let area = result
                .row
                .measures
                .area
                .measured()
                .map_or(0.0, |area| area.value().value.get());
            error += flux.declared.volume_error() + area * result.tau_b;
            if result.verdict == Verdict::Unconfirmed {
                if unconfirmed.is_some() {
                    return None;
                }
                unconfirmed = Some((*id, sigma, *flux));
            } else if established(result) {
                fixed += sigma * flux.declared.volume();
            } else {
                return None;
            }
        }
        let (face, sigma, flux) = unconfirmed?;
        let complement = flux.complement?;
        error += complement.volume_error();
        let required = if outer { 1.0 } else { -1.0 };
        let declared = required * (fixed + sigma * flux.declared.volume()) > error;
        let opposite = required * (fixed - sigma * complement.volume()) > error;
        match (declared, opposite) {
            (true, false) => Some((face, Verdict::Confirmed)),
            (false, true) => Some((face, Verdict::Contradicted)),
            _ => None,
        }
    }
}

/// The representative of a face's linked group.
fn group_of<'ir>(groups: &BTreeMap<&'ir str, &'ir str>, face: &'ir str) -> &'ir str {
    let mut current = face;
    while let Some(parent) = groups.get(current).filter(|parent| **parent != current) {
        current = parent;
    }
    current
}

/// Whether two faces rest on one sphere within their boundary allowances.
fn same_sphere(first: &FaceResult<'_>, second: &FaceResult<'_>) -> bool {
    let (Some(Chart::Sphere(a)), Some(Chart::Sphere(b))) = (first.chart, second.chart) else {
        return false;
    };
    let allowance = first.alpha.max(second.alpha);
    a.center.distance(b.center) <= allowance && (a.radius.abs() - b.radius.abs()).abs() <= allowance
}

/// The reading the total area of linked faces of one sphere leaves.
fn shared_sphere_verdict(
    faces: &HashMap<&str, FaceResult<'_>>,
    members: &[&str],
) -> Option<Verdict> {
    let mut total = 0.0;
    let mut error = 0.0;
    let mut sphere = None;
    let mut alpha = 0.0_f64;
    for member in members {
        let result = faces.get(member)?;
        let area = result.row.measures.area.measured()?;
        total += area.value().value.get();
        error += area.numeric_error().get() + area.model_uncertainty().get();
        alpha = alpha.max(result.alpha);
        sphere = result.chart;
    }
    let Chart::Sphere(chart) = sphere? else {
        return None;
    };
    let whole = sphere?.closed_area()?;
    let count = f64_from_index(members.len())?;
    error += count * 2.0 * std::f64::consts::TAU * chart.radius.abs() * alpha;
    let declared = total + error < whole;
    let complement = total - error > (count - 1.0) * whole;
    match (declared, complement) {
        (true, false) => Some(Verdict::Confirmed),
        (false, true) => Some(Verdict::Contradicted),
        _ => None,
    }
}

/// The vote of one edge: its two vertices and its midpoint, each with both
/// faces' oriented normals. A singular point is skipped except the
/// midpoint, which every vote needs.
fn edge_vote(
    eval: &Eval<'_, '_>,
    edge: &EdgeGeometry<'_>,
    sides: &[(Chart<'_>, f64, f64); 2],
) -> Measure<Option<Vote>> {
    let (Ok(curve), Ok([t0, t1])) = (&edge.curve, &edge.interval) else {
        return Ok(None);
    };
    let tolerance = TANGENT_WITNESS_ANGLE.sin();
    let mut signs = Vec::new();
    for (position, t) in [t0.midpoint(*t1), *t0, *t1].into_iter().enumerate() {
        let point = curve.point(eval, t)?;
        let [(chart, alpha, orientation), (other, other_alpha, other_orientation)] = *sides;
        let (Some(n), Some(m)) = (
            normal(eval, &chart, point, alpha, orientation)?,
            normal(eval, &other, point, other_alpha, other_orientation)?,
        ) else {
            if position == 0 {
                return Ok(None);
            }
            continue;
        };
        if n.cross(m).norm() > tolerance {
            return Ok(None);
        }
        signs.push(n.dot(m));
    }
    Ok(if signs.iter().all(|dot| *dot > 0.0) {
        Some(Vote::Agree)
    } else if signs.iter().all(|dot| *dot < 0.0) {
        Some(Vote::Disagree)
    } else {
        None
    })
}
