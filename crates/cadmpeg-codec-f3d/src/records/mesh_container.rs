// SPDX-License-Identifier: Apache-2.0
//! Decoded ParaMesh containers without a Design placement join.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::tessellation::{TessellationChannel, TessellationTriangleGroup};
use cadmpeg_ir::units::UnitVector3;
use serde::{Deserialize, Serialize};

use super::mesh::DesignMeshUuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MeshContainerRecord {
    pub(crate) id: String,
    pub(crate) fusion_uuid: String,
    pub(crate) mesh_uuid: DesignMeshUuid,
    pub(crate) container_vertices: Vec<FinitePoint3>,
    pub(crate) triangles: Vec<[u32; 3]>,
    pub(crate) feature_edges: Vec<[u32; 2]>,
    pub(crate) container_corner_normals: Option<Vec<UnitVector3>>,
    pub(crate) triangle_groups: Vec<TessellationTriangleGroup>,
    pub(crate) texture_ids: Option<Vec<u32>>,
    pub(crate) attributes: Vec<MeshContainerAttribute>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct MeshContainerAttribute {
    pub(crate) resource_guid: Option<String>,
    pub(crate) authored_name: Option<String>,
    pub(crate) groups: Vec<(u32, String)>,
    pub(crate) values: MeshContainerValues,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum MeshContainerValues {
    Typed {
        channel: TessellationChannel,
    },
    Untyped {
        role: u32,
        element_code: u32,
        addressing: MeshContainerAddressing,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum MeshContainerAddressing {
    Vertex,
    Corner { positions: Vec<u32> },
    UndecodedCorner,
    Triangle,
}
