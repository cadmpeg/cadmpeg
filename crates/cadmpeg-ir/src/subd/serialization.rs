// SPDX-License-Identifier: Apache-2.0
//! Borrow symmetry controls and correspondence arrays during serialization.

use super::{SubdPlaneFrame, SubdSymmetry, SubdSymmetryKind};
use serde::{Serialize, Serializer};

impl Serialize for SubdSymmetry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            kind: &'a SubdSymmetryKind,
            plane: &'a SubdPlaneFrame,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            face_pairs: &'a Vec<[u32; 2]>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            edge_pairs: &'a Vec<[u32; 2]>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            vertex_pairs: &'a Vec<[u32; 2]>,
        }
        Wire {
            kind: &self.kind,
            plane: &self.plane,
            face_pairs: &self.face_pairs,
            edge_pairs: &self.edge_pairs,
            vertex_pairs: &self.vertex_pairs,
        }
        .serialize(serializer)
    }
}
