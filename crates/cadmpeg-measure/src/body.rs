// SPDX-License-Identifier: Apache-2.0
//! Body rows: counts, shells and closure, area, length and box.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::topology::{Body, Sense};

use crate::edges::EdgeGeometry;
use crate::face::{box_value, FaceResult};
use crate::geom::Aabb;
use crate::owners::Owners;
use crate::reason::CaveatCode;
use crate::report::{BodyMeasures, CaveatCount, ShellMeasures, ShellRole, StatusTally};
use crate::value::{
    measured, BoxKind, BoxValue, Errors, Method, Quantity, Scalar, Unavailable, Unit,
};

/// Open edges a shell row lists.
const LISTED_OPEN_EDGES: usize = 16;

/// What a body row reads.
pub(crate) struct BodyInput<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) faces: &'a HashMap<&'ir str, FaceResult>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
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

impl BodyInput<'_, '_> {
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
        let area = (!face_ids.is_empty()).then(|| area.finish(Unit::SquareMillimeter, id));
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
        let shells = self.shells(body);
        let caveats = caveats
            .into_iter()
            .map(|(code, faces)| CaveatCount {
                code,
                category: code.category(),
                faces: faces.len(),
            })
            .collect();
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
            length,
            bbox,
            caveats,
            status: crate::report::RowStatus::Measured,
        };
        let mut tally = StatusTally::default();
        row.visit(&mut tally);
        row.status = tally.status();
        row
    }

    /// The union of the face boxes, else of the edge boxes, else of the
    /// vertex points.
    fn bbox(&self, id: &str, faces: &[&str], edges: &[&str]) -> Quantity<BoxValue> {
        let mut aabb: Option<Aabb> = None;
        let mut errors = Errors::ZERO;
        let mut kind = BoxKind::Tight;
        let mut include = |other: Aabb| {
            aabb = Some(aabb.map_or(other, |aabb| aabb.union(other)));
        };
        if !faces.is_empty() {
            for face in faces {
                let Some(result) = self.faces.get(face) else {
                    continue;
                };
                match (&result.row.measures.bbox, result.aabb) {
                    (Quantity::Measured(measured), Some(face_box)) => {
                        include(face_box);
                        if measured.value().kind == BoxKind::Conservative {
                            kind = BoxKind::Conservative;
                        }
                        errors = Errors::new(
                            errors.numeric.max(measured.numeric_error().get()),
                            errors.model.max(measured.model_uncertainty().get()),
                        );
                    }
                    (Quantity::Unavailable(unavailable), _) => {
                        return Quantity::Unavailable(Unavailable::aggregate(
                            unavailable.category(),
                            face,
                            format!(
                                "member {face} box is unavailable: {}",
                                unavailable.reason().code()
                            ),
                        ));
                    }
                    (Quantity::Measured(_), None) => {}
                }
            }
        } else if !edges.is_empty() {
            for edge in edges {
                let Some(geometry) = self.edges.get(edge) else {
                    continue;
                };
                match &geometry.bbox {
                    Ok(curve_box) => {
                        include(curve_box.aabb);
                        errors = Errors::new(
                            errors.numeric.max(curve_box.numeric_error),
                            errors.model.max(geometry.tau),
                        );
                    }
                    Err(missing) => {
                        let unavailable: Unavailable<BoxValue> = missing.unavailable();
                        return Quantity::Unavailable(Unavailable::aggregate(
                            unavailable.category(),
                            edge,
                            format!(
                                "member {edge} box is unavailable: {}",
                                missing.reason.code()
                            ),
                        ));
                    }
                }
            }
        }
        let Some(aabb) = aabb else {
            return Quantity::missing(
                crate::reason::UnavailableReason::BoundaryUnboundedFace,
                Some(id),
                "the body has no bounded member",
            );
        };
        measured(
            box_value(aabb, kind, None),
            Unit::Millimeter,
            Method::Sum,
            errors,
            id,
        )
    }

    /// Shell rows in region order; a shell is closed when every edge its
    /// coedges use is used by exactly two of them with opposite senses.
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
                let mut uses: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
                for face in shell.faces() {
                    let Some(face) = self.owners.face(face.as_str()) else {
                        continue;
                    };
                    for loop_id in &face.loops {
                        let Some(face_loop) = self.owners.loop_(loop_id.as_str()) else {
                            continue;
                        };
                        for coedge in face_loop.coedges() {
                            let Some(coedge) = self.owners.coedge(coedge.as_str()) else {
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
                let open: Vec<&str> = uses
                    .iter()
                    .filter(|(edge, count)| {
                        let degenerate = self
                            .edges
                            .get(*edge)
                            .is_some_and(|geometry| geometry.degenerate);
                        !degenerate && **count != (1, 1)
                    })
                    .map(|(edge, _)| *edge)
                    .collect();
                rows.push(ShellMeasures {
                    shell: shell_id.as_str().to_owned(),
                    role: if position == 0 {
                        ShellRole::Outer
                    } else {
                        ShellRole::Void
                    },
                    closed: !shell.faces().is_empty() && open.is_empty(),
                    open_edges: open
                        .iter()
                        .take(LISTED_OPEN_EDGES)
                        .map(|edge| (*edge).to_owned())
                        .collect(),
                    open_edge_count: open.len(),
                });
            }
        }
        rows
    }
}
