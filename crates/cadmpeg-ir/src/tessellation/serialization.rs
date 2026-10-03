// SPDX-License-Identifier: Apache-2.0
//! Borrow mesh lanes, addressing tables and channel payloads during serialization.

use super::{
    ChannelAddressing, Tessellation, TessellationChannel, TessellationId, TessellationMesh,
    TessellationTextureAssignment, TessellationTriangleGroup,
};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::ids::{BodyId, FaceId};
use crate::provenance::SourceObjectAssociation;
use crate::scalar::NonNegativeReal;
use serde::{Serialize, Serializer};

impl Serialize for Tessellation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            id: &'a TessellationId,
            #[serde(skip_serializing_if = "Option::is_none")]
            body: Option<&'a BodyId>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            faces: &'a Vec<FaceId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            chordal_deflection: Option<f64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            source_object: Option<&'a SourceObjectAssociation>,
            mesh: &'a TessellationMesh<FinitePoint3, FiniteVector3>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            feature_edges: &'a Vec<[u32; 2]>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            triangle_groups: &'a Vec<TessellationTriangleGroup>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            texture_assignments: &'a Vec<TessellationTextureAssignment>,
            channels: &'a [TessellationChannel],
        }
        Wire {
            id: &self.id,
            body: self.body.as_ref(),
            faces: &self.faces,
            chordal_deflection: self.chordal_deflection.map(NonNegativeReal::get),
            source_object: self.source_object.as_ref(),
            mesh: &self.mesh,
            feature_edges: &self.feature_edges,
            triangle_groups: &self.triangle_groups,
            texture_assignments: &self.texture_assignments,
            channels: &self.channels,
        }
        .serialize(serializer)
    }
}

impl Serialize for TessellationChannel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            addressing: &'a ChannelAddressing,
            item_size: u32,
            kind: u32,
            flags: u32,
            #[serde(with = "crate::bytes")]
            data: &'a [u8],
        }
        Wire {
            addressing: &self.addressing,
            item_size: self.item_size,
            kind: self.kind,
            flags: self.flags,
            data: &self.data,
        }
        .serialize(serializer)
    }
}
