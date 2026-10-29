// SPDX-License-Identifier: Apache-2.0
//! Native topology lanes choose layout; neutral incidence and values win.

use cadmpeg_asm::brep::records::{EndpointSlot, EvaluatedToleranceSlot, TolerantCoedgeExtension};
use cadmpeg_ir::document::CadIr;

use crate::native::F3dNative;

pub(super) fn reconcile(target: &CadIr, native: &mut F3dNative) {
    native.face_native_keys.retain(|record| {
        target
            .model
            .faces
            .iter()
            .any(|face| record.owns_native_ref(face.native_ref.as_deref(), &native.face_sidedness))
    });
    native.wire_topologies.retain_mut(|record| {
        let Some(shell) = target
            .model
            .shells
            .iter()
            .find(|shell| shell.id == record.shell)
        else {
            return false;
        };
        match &mut record.members {
            cadmpeg_asm::brep::records::WireMembers::Edges(edges) => {
                edges.retain(|edge| shell.wire_edges().contains(edge));
                !edges.is_empty()
            }
            cadmpeg_asm::brep::records::WireMembers::Vertex(vertex) => {
                shell.free_vertices().contains(vertex)
            }
        }
    });
    native.edge_ownerships.retain(|record| {
        target.model.edges.iter().any(|edge| edge.id == record.edge)
            && record.owner_coedge.as_ref().is_none_or(|id| {
                target
                    .model
                    .coedges
                    .iter()
                    .any(|coedge| &coedge.id == id && coedge.edge == record.edge)
            })
    });
    native.vertex_ownerships.retain(|record| {
        target
            .model
            .vertices
            .iter()
            .any(|vertex| vertex.id == record.vertex)
            && target.model.edges.iter().any(|edge| {
                edge.id == record.owning_edge
                    && match record.endpoint_index {
                        EndpointSlot::Start => edge.start == record.vertex,
                        EndpointSlot::End => edge.end == record.vertex,
                    }
            })
    });
    native.transform_hints.retain_mut(|record| {
        let Some(transform) = target
            .model
            .bodies
            .iter()
            .find(|body| body.id == record.body)
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
        let Some(body) = target
            .model
            .bodies
            .iter()
            .find(|body| body.id == record.body)
        else {
            return false;
        };
        let Some(visible) = body.visible else {
            return false;
        };
        let key = native
            .body_native_keys
            .iter()
            .find(|key| key.body == record.body)
            .and_then(|key| key.asm_body_key);
        if key.is_some_and(|key| key != record.asm_body_key()) {
            return false;
        }
        record.visible = visible;
        true
    });
    native.tolerant_edge_tails.retain(|record| {
        target
            .model
            .edges
            .iter()
            .any(|edge| edge.id == record.edge && edge.tolerance.is_some())
    });
    native.tolerant_vertex_tails.retain_mut(|record| {
        let Some(vertex) = target
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == record.vertex)
        else {
            return false;
        };
        if vertex.tolerance.is_none()
            && matches!(
                record.evaluated_slot,
                EvaluatedToleranceSlot::Evaluated { .. }
            )
        {
            return false;
        }
        let trailing = record.evaluated_slot.trailing();
        record.evaluated_slot = match (vertex.tolerance, record.evaluated_slot) {
            (Some(_), _) => EvaluatedToleranceSlot::Evaluated { trailing },
            (None, EvaluatedToleranceSlot::Evaluated { .. }) => EvaluatedToleranceSlot::Absent {},
            (_, slot) => slot,
        };
        true
    });
    native.tolerant_coedge_parameters.retain_mut(|record| {
        let Some(coedge) = target
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == record.coedge)
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
