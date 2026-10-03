// SPDX-License-Identifier: Apache-2.0
//! Borrowed wire projections for shell membership and edge carriers.

use super::{EdgeCarrier, ShellMembers};
use crate::ids::{CurveId, EdgeId, FaceId, VertexId};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

impl Serialize for ShellMembers {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum Member<'a> {
            Face { id: &'a FaceId },
            WireEdge { id: &'a EdgeId },
            FreeVertex { id: &'a VertexId },
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for id in &self.faces {
            sequence.serialize_element(&Member::Face { id })?;
        }
        for id in &self.wire_edges {
            sequence.serialize_element(&Member::WireEdge { id })?;
        }
        for id in &self.free_vertices {
            sequence.serialize_element(&Member::FreeVertex { id })?;
        }
        sequence.end()
    }
}

impl Serialize for EdgeCarrier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            curve: Option<&'a CurveId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            param_range: Option<[f64; 2]>,
        }
        Wire {
            curve: self.curve(),
            param_range: self.param_range().map(crate::units::FiniteVector::get),
        }
        .serialize(serializer)
    }
}
