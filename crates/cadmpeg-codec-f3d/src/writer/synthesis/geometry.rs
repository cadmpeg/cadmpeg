// SPDX-License-Identifier: Apache-2.0
//! Native topology lanes choose layout; neutral incidence and values win.

use cadmpeg_asm::brep::records::{EndpointSlot, EvaluatedToleranceSlot, TolerantCoedgeExtension};
use cadmpeg_ir::document::CadIr;

use crate::native::F3dNative;

pub(super) fn reconcile(target: &CadIr, native: &mut F3dNative) {
    // Canonical native row id to the neutral entity linked from it.
    let mut owners: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for body in &target.model.bodies {
        if let Some(native_ref) = body.native_ref.as_deref() {
            owners
                .entry(native_ref.to_owned())
                .or_insert_with(|| body.id.as_str().to_owned());
        }
    }
    for face in &target.model.faces {
        if let Some(native_ref) = face.native_ref.as_deref() {
            owners
                .entry(native_ref.to_owned())
                .or_insert_with(|| face.id.as_str().to_owned());
        }
    }
    for edge in &target.model.edges {
        if let Some(native_ref) = edge.native_ref.as_deref() {
            owners
                .entry(native_ref.to_owned())
                .or_insert_with(|| edge.id.as_str().to_owned());
        }
    }
    for vertex in &target.model.vertices {
        if let Some(native_ref) = vertex.native_ref.as_deref() {
            owners
                .entry(native_ref.to_owned())
                .or_insert_with(|| vertex.id.as_str().to_owned());
        }
    }
    let coedge_ids = target
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.id.as_str().to_owned())
        .collect::<std::collections::HashSet<_>>();
    native.face_native_keys.retain(|record| {
        target
            .model
            .faces
            .iter()
            .any(|face| record.owns_native_ref(face.native_ref.as_deref(), &native.face_sidedness))
    });
    native.wire_topologies.retain_mut(|record| {
        let member_neutrals = record
            .members
            .edges()
            .iter()
            .map(|member| member.as_str())
            .chain(
                record
                    .members
                    .free_vertex()
                    .map(|vertex| vertex.as_str()),
            )
            .filter_map(|member| owners.get(member))
            .collect::<Vec<_>>();
        let Some(shell) = target.model.shells.iter().find(|shell| {
            member_neutrals.iter().any(|neutral| {
                shell
                    .wire_edges()
                    .iter()
                    .any(|edge| &edge.as_str() == neutral)
                    || shell
                        .free_vertices()
                        .iter()
                        .any(|vertex| &vertex.as_str() == neutral)
            })
        }) else {
            return false;
        };
        match &mut record.members {
            cadmpeg_asm::brep::records::WireMembers::Edges(edges) => {
                edges.retain(|edge| {
                    owners
                        .get(edge.as_str())
                        .is_some_and(|neutral| shell.wire_edges().iter().any(|candidate| &candidate.as_str() == neutral))
                });
                !edges.is_empty()
            }
            cadmpeg_asm::brep::records::WireMembers::Vertex(vertex) => owners
                .get(vertex.as_str())
                .is_some_and(|neutral| {
                    shell
                        .free_vertices()
                        .iter()
                        .any(|candidate| &candidate.as_str() == neutral)
                }),
        }
    });
    native.edge_ownerships.retain(|record| {
        let Some(edge) = owners.get(record.id().as_str()) else {
            return false;
        };
        if target.model.edges.iter().all(|candidate| candidate.id.as_str() != edge) {
            return false;
        }
        match record.owner_coedge_index {
            None => true,
            Some(index) => target.model.coedges.iter().any(|coedge| {
                coedge.edge.as_str() == edge
                    && cadmpeg_asm::brep::records::neutral_record_index(coedge.id.as_str())
                        == Some(index)
            }),
        }
    });
    native.vertex_ownerships.retain(|record| {
        let Some(vertex) = owners.get(record.id().as_str()) else {
            return false;
        };
        if target
            .model
            .vertices
            .iter()
            .all(|candidate| candidate.id.as_str() != vertex)
        {
            return false;
        }
        let Some(owner) = record.owner.as_ref().map(|owner| owner.as_str()) else {
            return true;
        };
        let Some(edge) = owners.get(owner) else {
            return false;
        };
        target.model.edges.iter().any(|candidate| {
            candidate.id.as_str() == edge
                && match record.endpoint_index {
                    Some(EndpointSlot::Start) => candidate.start.as_str() == vertex,
                    Some(EndpointSlot::End) => candidate.end.as_str() == vertex,
                    None => true,
                }
        })
    });
    native.transform_hints.retain_mut(|record| {
        let Some(body) = owners.get(record.owner.as_str()) else {
            return false;
        };
        let Some(transform) = target
            .model
            .bodies
            .iter()
            .find(|candidate| candidate.id.as_str() == body)
            .and_then(|body| body.transform)
        else {
            return false;
        };
        let [rotation, reflection, shear] =
            super::super::generate::native_bytes::derived_transform_hints(transform);
        record.rotation = rotation;
        record.reflection = reflection;
        record.shear = shear;
        true
    });
    native.body_visibilities.retain_mut(|record| {
        let Some(body) = owners.get(record.body.as_str()) else {
            return false;
        };
        let Some(neutral) = target
            .model
            .bodies
            .iter()
            .find(|candidate| candidate.id.as_str() == body)
        else {
            return false;
        };
        let Some(visible) = neutral.visible else {
            return false;
        };
        let key = native
            .body_native_keys
            .iter()
            .find(|key| key.id().as_str() == record.body.as_str())
            .and_then(|key| key.asm_body_key);
        if key.is_some_and(|key| key != record.asm_body_key()) {
            return false;
        }
        record.visible = visible;
        true
    });
    native.tolerant_edge_tails.retain(|record| {
        owners.get(record.owner.as_str()).is_some_and(|edge| {
            target
                .model
                .edges
                .iter()
                .any(|candidate| candidate.id.as_str() == edge && candidate.tolerance.is_some())
        })
    });
    native.tolerant_vertex_tails.retain_mut(|record| {
        let Some(vertex) = owners.get(record.owner.as_str()) else {
            return false;
        };
        let Some(neutral) = target
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id.as_str() == vertex)
        else {
            return false;
        };
        if neutral.tolerance.is_none()
            && matches!(
                record.evaluated_slot,
                EvaluatedToleranceSlot::Evaluated { .. }
            )
        {
            return false;
        }
        let trailing = record.evaluated_slot.trailing();
        record.evaluated_slot = match (neutral.tolerance, record.evaluated_slot) {
            (Some(_), _) => EvaluatedToleranceSlot::Evaluated { trailing },
            (None, EvaluatedToleranceSlot::Evaluated { .. }) => EvaluatedToleranceSlot::Absent {},
            (_, slot) => slot,
        };
        true
    });
    native.tolerant_coedge_parameters.retain_mut(|record| {
        let Some(entity) = cadmpeg_asm::brep::records::native_entity_id(
            &record.source_namespace,
            record.record_index,
        ) else {
            return false;
        };
        if !coedge_ids.contains(entity.as_str()) {
            return false;
        }
        let Some(coedge) = target
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id.as_str() == entity)
        else {
            return false;
        };
        match &mut record.extension {
            TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                parameter_range,
                ..
            } => {
                let Some(use_curve) = &coedge.use_curve else {
                    return false;
                };
                let range = use_curve.parameter_range.endpoints();
                let Some(range) = cadmpeg_ir::units::FiniteVector::new(range) else {
                    return false;
                };
                *parameter_range = Some(range);
                record.parameter_range = range;
                true
            }
            TolerantCoedgeExtension::None {} | TolerantCoedgeExtension::Empty { target: None } => {
                true
            }
            _ => false,
        }
    });
}
