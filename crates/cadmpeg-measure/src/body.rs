// SPDX-License-Identifier: Apache-2.0
//! Body rows: counts, shells and closure, area, volume, centroid, length,
//! box and orientation.

mod volume;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Body, Sense, Shell};

use crate::canonical::placement::Placement;
use volume::{Member, VolumeInput};

use crate::aggregate::BoxUnion;
use crate::edges::EdgeGeometry;
use crate::face::{FaceResult, Verdict};
use crate::owners::Owners;
use crate::reason::CaveatCode;
use crate::report::{
    BodyMeasures, BodyOrientation, CaveatCount, ShellMeasures, ShellRole, StatusTally,
};
use crate::value::{
    measured, BoxKind, BoxValue, Errors, Method, Quantity, Reading, Scalar, Unavailable, Unit,
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
}

/// One aggregate of member values: a sum, or the first unavailable member.
struct Sum {
    value: f64,
    errors: Errors,
    method: Method,
    missing: Option<Unavailable<Scalar>>,
}

impl Sum {
    fn new() -> Self {
        Self {
            value: 0.0,
            errors: Errors::ZERO,
            method: Method::Sum,
            missing: None,
        }
    }

    fn add(&mut self, member: &str, quantity: &Quantity<Scalar>) {
        match quantity {
            Quantity::Measured(measured) => {
                self.value += measured.value().value.get();
                self.errors = self.errors.add(Errors::new(
                    measured.numeric_error().get(),
                    measured.model_uncertainty().get(),
                ));
            }
            Quantity::Unavailable(unavailable) => {
                if self.missing.is_none() {
                    self.missing = Some(Unavailable::aggregate(
                        unavailable.category(),
                        member,
                        format!(
                            "member {member} is unavailable: {}",
                            unavailable.reason().code()
                        ),
                    ));
                }
            }
        }
    }

    fn finish(self, unit: Unit, entity: &str) -> Quantity<Scalar> {
        match self.missing {
            Some(missing) => Quantity::Unavailable(missing),
            None => measured(
                Scalar::new(self.value),
                unit,
                self.method,
                self.errors,
                entity,
            ),
        }
    }
}

impl<'a> BodyInput<'a, '_> {
    pub(crate) fn row(&self, body: &Body) -> BodyMeasures {
        let id = body.id.as_str();
        let face_ids: Vec<&str> = self.owners.body_faces(id).collect();
        let edge_ids: Vec<&str> = self.owners.body_edges(id).collect();
        let mut surface_kinds = BTreeMap::new();
        let mut area = Sum::new();
        let mut caveats: BTreeMap<CaveatCode, BTreeSet<&str>> = BTreeMap::new();
        for face in &face_ids {
            let Some(result) = self.faces.get(face) else {
                continue;
            };
            *surface_kinds.entry(result.row.carrier.kind).or_insert(0) += 1;
            area.add(face, &result.row.measures.area);
            for caveat in &result.row.caveats {
                caveats.entry(caveat.code).or_default().insert(face);
            }
        }
        let area = (!face_ids.is_empty()).then(|| {
            with_complement(
                area.finish(Unit::SquareMillimeter, id),
                &face_ids,
                self.faces,
            )
        });
        let length = (face_ids.is_empty() && !edge_ids.is_empty()).then(|| {
            let mut sum = Sum::new();
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
                sum.add(edge, &quantity);
            }
            sum.finish(Unit::Millimeter, id)
        });
        let bbox = self.bbox(id, &face_ids, &edge_ids);
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
            let placement = body.transform.map(Placement::of);
            let handedness = match &placement {
                Some(Ok(placement)) if placement.reflection => -1.0,
                _ => 1.0,
            };
            let placement_term = |dimension: u8, value: f64| match &placement {
                Some(Ok(placement)) => placement.model_term(dimension, value),
                _ => 0.0,
            };
            let area_value = area
                .as_ref()
                .and_then(Quantity::measured)
                .map_or(0.0, |area| area.value().value.get());
            let diagonal = bbox.measured().map_or(0.0, |bbox| {
                bbox.value().min.get().distance(bbox.value().max.get())
            });
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
                area: area_value,
                diagonal,
                placement: &placement_term,
            };
            input.measure()
        });
        let (volume, centroid, area_centroid, orientation) = match volume {
            Some(volume) => {
                for (shell, value) in shells.iter_mut().zip(volume.shell_volumes) {
                    shell.volume = value;
                }
                (
                    Some(volume.volume),
                    Some(volume.centroid),
                    volume.area_centroid,
                    volume.orientation,
                )
            }
            None => (
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
            area_centroid,
            length,
            bbox,
            orientation,
            caveats,
            status: crate::report::RowStatus::Measured,
        };
        let mut tally = StatusTally::default();
        row.visit(&mut tally);
        row.status = tally.status();
        row
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

/// A body area under the declared reading of its one unconfirmed face,
/// with the area under that face's complement reading as a candidate. With
/// several unconfirmed faces the sum stays under the declared readings and
/// the body's caveat count states them.
fn with_complement(
    area: Quantity<Scalar>,
    faces: &[&str],
    results: &HashMap<&str, FaceResult<'_>>,
) -> Quantity<Scalar> {
    let unconfirmed: Vec<&FaceResult<'_>> = faces
        .iter()
        .filter_map(|face| results.get(face))
        .filter(|result| result.verdict == Verdict::Unconfirmed)
        .collect();
    let ([face], Quantity::Measured(total)) = (unconfirmed.as_slice(), &area) else {
        return area;
    };
    let Quantity::Measured(declared) = &face.row.measures.area else {
        return area;
    };
    let [complement] = declared.candidates() else {
        return area;
    };
    let value =
        total.value().value.get() - declared.value().value.get() + complement.value().value.get();
    let errors = Errors::new(
        total.numeric_error().get() + complement.numeric_error().get(),
        total.model_uncertainty().get() + complement.model_uncertainty().get(),
    );
    match measured(
        Scalar::new(value),
        Unit::SquareMillimeter,
        Method::Sum,
        errors,
        &face.row.face,
    ) {
        Quantity::Measured(candidate) => Quantity::Measured(total.clone().under_reading(
            Reading::Declared,
            vec![candidate.into_candidate(Reading::Complement)],
        )),
        Quantity::Unavailable(_) => area,
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
