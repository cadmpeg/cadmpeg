// SPDX-License-Identifier: Apache-2.0
//! Resolve emitted vertex owners through the admitted boundary topology.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{BodyId, VertexId};

fn missing() -> CodecError {
    CodecError::Malformed("SLDPRT vertex ownership references missing topology".into())
}

pub(super) fn vertex_body_owners(
    ir: &CadIr,
    body_attrs: &HashMap<BodyId, u16>,
) -> Result<HashMap<VertexId, u16>, CodecError> {
    let regions = ir
        .model
        .regions
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    let shells = ir
        .model
        .shells
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|v| (&v.id, v))
        .collect::<HashMap<_, _>>();
    let mut owners = HashMap::new();
    let mut assign = |vertex: &VertexId, owner| -> Result<(), CodecError> {
        if owners
            .insert(vertex.clone(), owner)
            .is_some_and(|old| old != owner)
        {
            return Err(CodecError::NotImplemented(
                "SLDPRT vertex belongs to more than one BODY".into(),
            ));
        }
        Ok(())
    };
    for boundary in &ir.model.loops {
        let face = faces.get(&boundary.face).ok_or_else(missing)?;
        let shell = shells.get(&face.shell).ok_or_else(missing)?;
        let region = regions.get(&shell.region).ok_or_else(missing)?;
        let owner = *body_attrs.get(&region.body).ok_or_else(missing)?;
        for vertex in boundary.vertices() {
            assign(vertex, owner)?;
        }
        for coedge in boundary.coedges() {
            let coedge = coedges.get(coedge).ok_or_else(missing)?;
            let edge = edges.get(&coedge.edge).ok_or_else(missing)?;
            assign(&edge.start, owner)?;
            assign(&edge.end, owner)?;
        }
    }
    if ir
        .model
        .vertices
        .iter()
        .any(|vertex| !owners.contains_key(&vertex.id))
    {
        return Err(CodecError::NotImplemented(
            "SLDPRT vertex has no unique incident BODY".into(),
        ));
    }
    Ok(owners)
}
