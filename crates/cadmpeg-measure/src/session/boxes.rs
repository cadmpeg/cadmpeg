// SPDX-License-Identifier: Apache-2.0
//! Body boxes from one box pass.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

use super::{faces_by_inverted_boundary, Measurer};
use crate::body::BodyInput;
use crate::canonical::placement::Placement;
use crate::context::{Eval, Work};
use crate::edges::{measure_edge, EdgeGeometry};
use crate::face::{FaceInput, FaceResult};
use crate::failure::Outcome;
use crate::report::BodyBox;
use crate::witness::WitnessInput;

impl<'ir> Measurer<'ir> {
    /// The boxes of every body, in body arena order, under one pass
    /// allowance. A session refusal of `ctx` ends the call.
    pub(super) fn boxes(&self, ctx: &DecodeContext<'_>) -> Result<Vec<BodyBox>, ResourceLimit> {
        let geometry = &self.input.ir.geometry;
        let pass = self.pass("measurement box allowance")?;
        let placements = self.body_placements()?;
        let placement_of = |body: Option<&str>| -> Outcome<Placement> {
            body.and_then(|body| placements.get(body))
                .cloned()
                .unwrap_or(Ok(Placement::identity()))
        };

        let mut needed_faces: BTreeSet<&'ir str> = BTreeSet::new();
        let mut needed_edges: BTreeSet<&'ir str> = BTreeSet::new();
        for body in &geometry.bodies {
            needed_faces.extend(self.owners.body_faces(body.id.as_str()));
            needed_edges.extend(self.owners.body_edges(body.id.as_str()));
        }
        for face in &needed_faces {
            let Some(face) = self.owners.face(face) else {
                continue;
            };
            for loop_id in &face.loops {
                if let Some(face_loop) = self.owners.loop_(loop_id.as_str()) {
                    for coedge in face_loop.coedges() {
                        if let Some(coedge) = self.owners.coedge(coedge.as_str()) {
                            needed_edges.insert(coedge.edge.as_str());
                        }
                    }
                }
            }
        }

        let mut edges: HashMap<&'ir str, EdgeGeometry<'ir>> = HashMap::new();
        for edge_id in needed_edges {
            let Some(edge) = self.owners.edge(edge_id) else {
                continue;
            };
            let slice = pass.edge_slice();
            let eval = Eval {
                ctx,
                index: &self.index,
                work: &slice,
            };
            let placement = placement_of(self.owners.edge_body(edge_id));
            edges.insert(
                edge_id,
                measure_edge(&eval, &self.owners, edge, &placement, Work::Box)?,
            );
        }

        let mut faces: HashMap<&'ir str, FaceResult<'ir>> = HashMap::new();
        for face_id in faces_by_inverted_boundary(&self.owners, &needed_faces) {
            let Some(face) = self.owners.face(face_id) else {
                continue;
            };
            let body = self.owners.face_body(face);
            let slice = pass.face_slice();
            let eval = Eval {
                ctx,
                index: &self.index,
                work: &slice,
            };
            let input = FaceInput {
                work: Work::Box,
                owners: &self.owners,
                edges: &edges,
                fidelity: None,
                body,
                placement: placement_of(body),
                flux_origin: None,
            };
            faces.insert(face_id, input.measure(&eval, face)?);
        }
        let witness_slice = pass.face_slice();
        WitnessInput {
            owners: &self.owners,
            edges: &edges,
            pass: &pass,
            eval: Eval {
                ctx,
                index: &self.index,
                work: &witness_slice,
            },
            placements: &placements,
        }
        .run(&mut faces)?;
        if let Some(limit) = ctx.resource_refusal() {
            return Err(limit);
        }

        let bodies = BodyInput {
            owners: &self.owners,
            faces: &faces,
            edges: &edges,
            origins: &HashMap::new(),
            placements: &placements,
            density: None,
        };
        Ok(geometry
            .bodies
            .iter()
            .map(|body| bodies.boxes(body))
            .collect())
    }
}
