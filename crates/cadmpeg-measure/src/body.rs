// SPDX-License-Identifier: Apache-2.0
//! Body rows: counts, shells and closure, area, volume, centroid, length,
//! box and orientation.

mod volume;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Body, Sense, Shell};

use crate::canonical::placement::Placement;
use crate::failure::Outcome;
use volume::{Member, VolumeInput};

use crate::aggregate::{BoxUnion, ReadingSum};
use crate::edges::EdgeGeometry;
use crate::face::{box_value, FaceResult};
use crate::geom::Aabb;
use crate::owners::Owners;
use crate::reason::{CaveatCode, UnavailableReason};
use crate::report::{
    BodyMeasures, BodyOrientation, CaveatCount, ShellMeasures, ShellRole, StatusTally,
};
use crate::tolerance::model_tolerance;
use crate::value::{
    measured, BoxKind, BoxValue, Candidate, Errors, Method, Quantity, Scalar, Unavailable, Unit,
};

/// Open edges a shell row lists.
const LISTED_OPEN_EDGES: usize = 16;

/// What a body row reads.
pub(crate) struct BodyInput<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) faces: &'a HashMap<&'ir str, FaceResult<'ir>>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
    /// The reference point of each body's fluxes.
    pub(crate) origins: &'a HashMap<&'ir str, Point3>,
    /// The placement of each body.
    pub(crate) placements: &'a HashMap<&'ir str, Outcome<Placement>>,
    /// The material density of mass and inertia.
    pub(crate) density: Option<crate::session::Density>,
}

impl<'a> BodyInput<'a, '_> {
    pub(crate) fn row(&self, body: &Body) -> BodyMeasures {
        let id = body.id.as_str();
        let face_ids: Vec<&str> = self.owners.body_faces(id).collect();
        let edge_ids: Vec<&str> = self.owners.body_edges(id).collect();
        let mut surface_kinds = BTreeMap::new();
        let mut area = ReadingSum::new();
        let mut caveats: BTreeMap<CaveatCode, BTreeSet<&str>> = BTreeMap::new();
        for face in &face_ids {
            let Some(result) = self.faces.get(face) else {
                continue;
            };
            *surface_kinds.entry(result.row.carrier.kind).or_insert(0) += 1;
            area.add(face, "area", &result.row.measures.area);
            for caveat in &result.row.caveats {
                caveats.entry(caveat.code).or_default().insert(face);
            }
        }
        let area = (!face_ids.is_empty()).then(|| area.finish(Unit::SquareMillimeter, id));
        let length = (face_ids.is_empty() && !edge_ids.is_empty()).then(|| {
            let mut sum = ReadingSum::new();
            for edge in &edge_ids {
                let Some(geometry) = self.edges.get(edge) else {
                    continue;
                };
                let quantity = match &geometry.length {
                    Ok(length) => measured(
                        Scalar::new(length.value),
                        Unit::Millimeter,
                        length.method,
                        Errors::new(length.numeric_error, 2.0 * geometry.tau),
                        edge,
                    ),
                    Err(missing) => missing.quantity(),
                };
                sum.add(edge, "length", &quantity);
            }
            sum.finish(Unit::Millimeter, id)
        });
        let bbox = self.bbox(id, &face_ids, &edge_ids);
        let vertex_box = self.vertex_box(body);
        let mut shells = self.shells(body);
        let caveats = caveats
            .into_iter()
            .map(|(code, faces)| CaveatCount {
                code,
                category: code.category(),
                faces: faces.len(),
            })
            .collect();
        let volume = (!face_ids.is_empty()).then(|| {
            let placement = self.placements.get(id);
            let handedness = handedness(placement);
            let placement_term = |dimension: u8, value: f64| match &placement {
                Some(Ok(placement)) => placement.model_term(dimension, value),
                _ => 0.0,
            };
            // Without a body box, the union of the faces' reading boxes.
            let diagonal = match &bbox {
                Quantity::Measured(bbox) => Ok(std::iter::once(bbox.value())
                    .chain(bbox.candidates().iter().map(Candidate::value))
                    .map(|value| value.min.get().distance(value.max.get()))
                    .fold(0.0, f64::max)),
                Quantity::Unavailable(unavailable) => self
                    .members(body)
                    .iter()
                    .map(|member| member.result.reading_boxes().copied().reduce(Aabb::union))
                    .collect::<Option<Vec<_>>>()
                    .and_then(|boxes| boxes.into_iter().reduce(Aabb::union))
                    .map(|union| union.diagonal())
                    .ok_or_else(|| {
                        Unavailable::aggregate(
                            unavailable.category(),
                            id,
                            format!(
                                "the body's box, which bounds its model uncertainty, is unavailable: {}",
                                unavailable.message()
                            ),
                        )
                    }),
            };
            let input = VolumeInput {
                id,
                members: self.members(body),
                shells: shells
                    .iter()
                    .map(|shell| (shell.closed, shell.role == ShellRole::Outer))
                    .collect(),
                origin: self
                    .origins
                    .get(id)
                    .copied()
                    .unwrap_or(Point3::new(0.0, 0.0, 0.0)),
                handedness,
                diagonal,
                placement: &placement_term,
                density: self.density,
            };
            input.measure()
        });
        let (volume, centroid, area_centroid, mass, orientation) = match volume {
            Some(volume) => {
                for (shell, value) in shells.iter_mut().zip(volume.shell_volumes) {
                    shell.volume = value;
                }
                (
                    Some(volume.volume),
                    Some(volume.centroid),
                    volume.area_centroid,
                    Some(volume.mass),
                    volume.orientation,
                )
            }
            None => (
                None,
                None,
                None,
                None,
                BodyOrientation {
                    consistent: None,
                    inconsistent: Vec::new(),
                    mixed: Vec::new(),
                    unconfirmed: Vec::new(),
                    conflict: Vec::new(),
                    closure_flux: None,
                },
            ),
        };
        let mut row = BodyMeasures {
            body: id.to_owned(),
            placement: crate::report::RowPlacement::Model {},
            name: body.name.clone(),
            kind: body.kind,
            visible: body.visible,
            faces: face_ids.len(),
            surface_kinds,
            edges: edge_ids.len(),
            vertices: self.owners.body_vertex_count(id),
            shells,
            area,
            volume,
            centroid,
            second_moments: mass.as_ref().map(|mass| mass.second_moments.clone()),
            principal_moments: mass.as_ref().map(|mass| mass.principal_moments.clone()),
            principal_axes: mass.as_ref().map(|mass| mass.principal_axes.clone()),
            mass: mass.as_ref().and_then(|mass| mass.mass.clone()),
            inertia: mass.and_then(|mass| mass.inertia),
            area_centroid,
            length,
            bbox,
            vertex_box,
            orientation,
            caveats,
            status: crate::report::RowStatus::Measured,
        };
        let mut tally = StatusTally::default();
        row.visit(&mut tally);
        row.status = tally.status();
        row
    }

    /// Why the volume readings of a body whose shells are closed conflict:
    /// an orientation conflict or a closure flux above its tolerance.
    /// `None` for a body with no shell, an open shell, or no conflict.
    pub(crate) fn volume_conflict(&self, body: &Body) -> Option<VolumeConflict> {
        let shells = self.shells(body);
        if shells.is_empty() {
            return None;
        }
        let id = body.id.as_str();
        let placement = self.placements.get(id);
        let no_term = |_: u8, _: f64| 0.0;
        let input = VolumeInput {
            id,
            members: self.members(body),
            shells: shells
                .iter()
                .map(|shell| (shell.closed, shell.role == ShellRole::Outer))
                .collect(),
            origin: self
                .origins
                .get(id)
                .copied()
                .unwrap_or(Point3::new(0.0, 0.0, 0.0)),
            handedness: handedness(placement),
            diagonal: Err(Unavailable::new(
                UnavailableReason::AggregateMemberUnavailable,
                Some(id),
                "the conflict check reads no model uncertainty",
            )),
            placement: &no_term,
            density: None,
        };
        let failure = input.conflict()?;
        let mut related = failure.related;
        if let Some(shell) = failure.shell.and_then(|position| shells.get(position)) {
            related.push(shell.shell.clone());
        }
        Some(VolumeConflict {
            reason: failure.reason,
            message: failure.message,
            related,
        })
    }

    /// The union of the face boxes, else of the edge boxes.
    fn bbox(&self, id: &str, faces: &[&str], edges: &[&str]) -> Quantity<BoxValue> {
        let mut union = BoxUnion::new();
        if faces.is_empty() {
            for edge in edges {
                let Some(geometry) = self.edges.get(edge) else {
                    continue;
                };
                match &geometry.bbox {
                    Ok(curve_box) => union.include(
                        curve_box.aabb,
                        BoxKind::Tight,
                        Some(curve_box.aabb),
                        Errors::new(curve_box.numeric_error, geometry.tau),
                    ),
                    Err(missing) => {
                        let unavailable: Unavailable<BoxValue> = missing.unavailable();
                        union.miss(edge, &unavailable);
                    }
                }
            }
        } else {
            for face in faces {
                if let Some(result) = self.faces.get(face) {
                    union.add(face, &result.row.measures.bbox);
                }
            }
        }
        union.finish(Some(id), "the body has no bounded member")
    }

    /// The box of the body's placed vertex points; `None` for a body with
    /// no vertex. Its model uncertainty is the larger of the document's
    /// linear tolerance and the vertex tolerances.
    fn vertex_box(&self, body: &Body) -> Option<Quantity<BoxValue>> {
        let id = body.id.as_str();
        let points = self.owners.body_vertex_points(id);
        let (first, rest) = points.split_first()?;
        let placement = match self.placements.get(id) {
            Some(Ok(placement)) => *placement,
            Some(Err(missing)) => return Some(missing.quantity()),
            None => Placement::identity(),
        };
        let mut aabb = Aabb::of_point(placement.point(*first));
        for point in rest {
            aabb.include(placement.point(*point));
        }
        let tau = model_tolerance(
            self.owners.ir.tolerances.linear.get(),
            self.owners.body_vertex_tolerance(id),
        );
        Some(measured(
            box_value(aabb, BoxKind::Tight, None),
            Unit::Millimeter,
            Method::ClosedForm,
            Errors::new(0.0, tau),
            id,
        ))
    }

    /// The body's faces with their shell positions in region order.
    fn members(&self, body: &Body) -> Vec<Member<'a>> {
        let mut members = Vec::new();
        let mut position = 0;
        for region in &body.regions {
            let Some(region) = self.owners.region(region.as_str()) else {
                continue;
            };
            for shell_id in &region.shells {
                let Some(shell) = self.owners.shell(shell_id.as_str()) else {
                    continue;
                };
                for face in shell.faces() {
                    if let Some((id, result)) = self.faces.get_key_value(face.as_str()) {
                        members.push(Member {
                            id,
                            shell: position,
                            result,
                        });
                    }
                }
                position += 1;
            }
        }
        members
    }

    /// Shell rows in region order; a shell is closed when every edge its
    /// coedges use is used by exactly two of them with opposite senses and
    /// no face's region reaches a natural domain edge.
    fn shells(&self, body: &Body) -> Vec<ShellMeasures> {
        let mut rows = Vec::new();
        for region in &body.regions {
            let Some(region) = self.owners.region(region.as_str()) else {
                continue;
            };
            for (position, shell_id) in region.shells.iter().enumerate() {
                let Some(shell) = self.owners.shell(shell_id.as_str()) else {
                    continue;
                };
                let Closure {
                    open,
                    natural_edge_faces,
                } = closure(self.owners, self.faces, self.edges, shell);
                rows.push(ShellMeasures {
                    shell: shell_id.as_str().to_owned(),
                    role: if position == 0 {
                        ShellRole::Outer
                    } else {
                        ShellRole::Void
                    },
                    closed: !shell.faces().is_empty()
                        && open.is_empty()
                        && natural_edge_faces.is_empty(),
                    open_edges: open
                        .iter()
                        .take(LISTED_OPEN_EDGES)
                        .map(|edge| (*edge).to_owned())
                        .collect(),
                    open_edge_count: open.len(),
                    natural_edge_faces,
                    volume: None,
                });
            }
        }
        rows
    }
}

/// Why a body's volume readings conflict.
pub(crate) struct VolumeConflict {
    pub(crate) reason: crate::reason::UnavailableReason,
    pub(crate) message: String,
    /// The faces or the shell the conflict names.
    pub(crate) related: Vec<String>,
}

/// `−1` under a reflecting body placement, else `1`.
fn handedness<E>(placement: Option<&Result<Placement, E>>) -> f64 {
    match placement {
        Some(Ok(placement)) if placement.reflection => -1.0,
        _ => 1.0,
    }
}

/// The open edges of a shell and its faces that reach a natural domain
/// edge.
pub(crate) struct Closure<'ir> {
    pub(crate) open: Vec<&'ir str>,
    pub(crate) natural_edge_faces: Vec<String>,
}

impl Closure<'_> {
    /// Whether the shell is closed.
    pub(crate) fn closed(&self) -> bool {
        self.open.is_empty() && self.natural_edge_faces.is_empty()
    }
}

/// The closure of a shell: every edge its coedges use, except degenerate
/// edges, is used by exactly two of them with opposite senses, and no face's
/// region reaches a natural domain edge.
pub(crate) fn closure<'ir>(
    owners: &Owners<'ir>,
    faces: &HashMap<&'ir str, FaceResult<'_>>,
    edges: &HashMap<&'ir str, EdgeGeometry<'_>>,
    shell: &Shell,
) -> Closure<'ir> {
    let mut uses: BTreeMap<&'ir str, (usize, usize)> = BTreeMap::new();
    let mut natural_edge_faces = Vec::new();
    for face in shell.faces() {
        if faces
            .get(face.as_str())
            .is_some_and(|result| result.natural)
        {
            natural_edge_faces.push(face.as_str().to_owned());
        }
        let Some(face) = owners.face(face.as_str()) else {
            continue;
        };
        for loop_id in &face.loops {
            let Some(face_loop) = owners.loop_(loop_id.as_str()) else {
                continue;
            };
            for coedge in face_loop.coedges() {
                let Some(coedge) = owners.coedge(coedge.as_str()) else {
                    continue;
                };
                let entry = uses.entry(coedge.edge.as_str()).or_insert((0, 0));
                match coedge.sense {
                    Sense::Forward => entry.0 += 1,
                    Sense::Reversed => entry.1 += 1,
                }
            }
        }
    }
    let open = uses
        .iter()
        .filter(|(edge, count)| {
            let degenerate = edges.get(*edge).is_some_and(|geometry| geometry.degenerate);
            !degenerate && **count != (1, 1)
        })
        .map(|(edge, _)| *edge)
        .collect();
    Closure {
        open,
        natural_edge_faces,
    }
}
