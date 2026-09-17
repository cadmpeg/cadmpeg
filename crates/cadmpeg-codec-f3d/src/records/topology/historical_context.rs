// SPDX-License-Identifier: Apache-2.0
//! Historical face, loop and edge contexts of a feature input topology.

use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::Point3;
use serde::Deserialize;
use serde::Serialize;

/// Stable surface-support relation from an active face candidate to the
/// topology preceding its owning feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignHistoricalFaceSupportContext {
    /// Stable slot of the active face candidate.
    pub active_face_slot: i64,
    /// Invariant stable surface-carrier slot.
    pub surface_slot: i64,
    /// Preceding face slots owning the surface carrier.
    pub preceding_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the preceding carrier owners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding owners deleted or updated by the feature transition.
    pub changed_preceding_face_slots: Vec<i64>,
}

/// Historical edge-boundary context for one ordered edge-recipe prefix reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignEdgeRecipeReferenceContext {
    /// Zero-based position in the edge recipe's prefix reference sequence.
    pub reference_ordinal: u32,
    /// Referenced faces present in the owning feature's result topology.
    pub result_faces: Vec<FaceId>,
    /// Ordered loop boundaries of each referenced result face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Stable result edge slots shared by the referenced-face boundaries and
    /// the primary candidate-face boundaries.
    pub result_shared_edge_slots: Vec<i64>,
    /// Referenced faces present in the immediately preceding ASM topology.
    pub preceding_faces: Vec<FaceId>,
    /// Ordered loop boundaries of each referenced preceding face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Preceding faces uniquely owning the surface carriers of the referenced
    /// result faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_support_face_slots: Vec<i64>,
    /// Ordered loop boundaries of the uniquely matched preceding support
    /// faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preceding_support_face_boundaries: Vec<DesignHistoricalFaceBoundaryContext>,
    /// Stable edge slots shared by the referenced-face boundaries and the
    /// primary candidate-face boundaries.
    pub shared_edge_slots: Vec<i64>,
    /// Shared edge slots deleted or updated by the owning feature transition.
    pub changed_shared_edge_slots: Vec<i64>,
    /// Changed primary-boundary edges belonging to either a directly
    /// persistent referenced face or its unique preceding surface support.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_reference_edge_slots: Vec<i64>,
}

/// Ordered loop topology retained for one historical face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignHistoricalFaceBoundaryContext {
    /// Stable ASM face slot.
    pub face_slot: i64,
    /// Face loops in their serialized membership order.
    pub loops: Vec<DesignHistoricalFaceLoopContext>,
}

/// Ordered topology and available geometry of one historical face loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignHistoricalFaceLoopWire",
    into = "DesignHistoricalFaceLoopWire"
)]
pub struct DesignHistoricalFaceLoopContext {
    pub loop_slot: i64,
    pub boundary: DesignHistoricalLoopBoundary,
}

/// Complete runs of the available loop member bindings.
#[derive(Debug, Clone, PartialEq)]
pub enum DesignHistoricalLoopBoundary {
    Coedges(Vec<DesignHistoricalLoopCoedge>),
    Vertices(Vec<DesignHistoricalLoopVertex>),
    Points(Vec<DesignHistoricalLoopPoint>),
    Positions(Vec<DesignHistoricalLoopPosition>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopCoedge {
    pub coedge_slot: i64,
    pub edge_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopVertex {
    pub coedge: DesignHistoricalLoopCoedge,
    pub vertex_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopPoint {
    pub vertex: DesignHistoricalLoopVertex,
    pub point_slot: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignHistoricalLoopPosition {
    pub point: DesignHistoricalLoopPoint,
    pub position: Point3,
}

impl DesignHistoricalLoopBoundary {
    pub(crate) fn coedges(&self) -> impl Iterator<Item = &DesignHistoricalLoopCoedge> {
        let (mut coedges, mut vertices, mut points, mut positions): (&[_], &[_], &[_], &[_]) =
            (&[], &[], &[], &[]);
        match self {
            Self::Coedges(rows) => coedges = rows,
            Self::Vertices(rows) => vertices = rows,
            Self::Points(rows) => points = rows,
            Self::Positions(rows) => positions = rows,
        }
        coedges
            .iter()
            .chain(vertices.iter().map(|row| &row.coedge))
            .chain(points.iter().map(|row| &row.vertex.coedge))
            .chain(positions.iter().map(|row| &row.point.vertex.coedge))
    }
}

/// Ordered coedge and edge membership of one historical face loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignHistoricalFaceLoopWire {
    /// Stable ASM loop slot.
    loop_slot: i64,
    /// Stable coedge slots in cyclic loop order.
    coedge_slots: Vec<i64>,
    /// Stable edge slots aligned one-to-one with `coedge_slots`.
    edge_slots: Vec<i64>,
    /// Stable boundary-vertex slots preceding the aligned coedges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_slots: Vec<i64>,
    /// Stable point-carrier slots aligned one-to-one with `vertex_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    point_slots: Vec<i64>,
    /// Model-space positions aligned one-to-one with `point_slots`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    positions: Vec<cadmpeg_ir::math::Point3>,
}

impl TryFrom<DesignHistoricalFaceLoopWire> for DesignHistoricalFaceLoopContext {
    type Error = String;

    fn try_from(wire: DesignHistoricalFaceLoopWire) -> Result<Self, Self::Error> {
        let count = wire.coedge_slots.len();
        if wire.edge_slots.len() != count {
            return Err("coedge_slots and edge_slots must have equal lengths".into());
        }
        if !wire.vertex_slots.is_empty() && wire.vertex_slots.len() != count {
            return Err("vertex_slots must be empty or match coedge_slots".into());
        }
        if !wire.point_slots.is_empty() && wire.point_slots.len() != wire.vertex_slots.len() {
            return Err("point_slots must be empty or match vertex_slots".into());
        }
        if !wire.positions.is_empty() && wire.positions.len() != wire.point_slots.len() {
            return Err("positions must be empty or match point_slots".into());
        }
        let coedges =
            wire.coedge_slots
                .into_iter()
                .zip(wire.edge_slots)
                .map(|(coedge_slot, edge_slot)| DesignHistoricalLoopCoedge {
                    coedge_slot,
                    edge_slot,
                });
        let boundary = if wire.vertex_slots.is_empty() {
            DesignHistoricalLoopBoundary::Coedges(coedges.collect())
        } else {
            let vertices = coedges.zip(wire.vertex_slots).map(|(coedge, vertex_slot)| {
                DesignHistoricalLoopVertex {
                    coedge,
                    vertex_slot,
                }
            });
            if wire.point_slots.is_empty() {
                DesignHistoricalLoopBoundary::Vertices(vertices.collect())
            } else {
                let points = vertices
                    .zip(wire.point_slots)
                    .map(|(vertex, point_slot)| DesignHistoricalLoopPoint { vertex, point_slot });
                if wire.positions.is_empty() {
                    DesignHistoricalLoopBoundary::Points(points.collect())
                } else {
                    DesignHistoricalLoopBoundary::Positions(
                        points
                            .zip(wire.positions)
                            .map(|(point, position)| DesignHistoricalLoopPosition {
                                point,
                                position,
                            })
                            .collect(),
                    )
                }
            }
        };
        Ok(Self {
            loop_slot: wire.loop_slot,
            boundary,
        })
    }
}

impl From<DesignHistoricalFaceLoopContext> for DesignHistoricalFaceLoopWire {
    fn from(context: DesignHistoricalFaceLoopContext) -> Self {
        let mut wire = Self {
            loop_slot: context.loop_slot,
            coedge_slots: Vec::new(),
            edge_slots: Vec::new(),
            vertex_slots: Vec::new(),
            point_slots: Vec::new(),
            positions: Vec::new(),
        };
        match context.boundary {
            DesignHistoricalLoopBoundary::Coedges(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.coedge_slot);
                    wire.edge_slots.push(row.edge_slot);
                }
            }
            DesignHistoricalLoopBoundary::Vertices(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.coedge.coedge_slot);
                    wire.edge_slots.push(row.coedge.edge_slot);
                    wire.vertex_slots.push(row.vertex_slot);
                }
            }
            DesignHistoricalLoopBoundary::Points(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.vertex.coedge.coedge_slot);
                    wire.edge_slots.push(row.vertex.coedge.edge_slot);
                    wire.vertex_slots.push(row.vertex.vertex_slot);
                    wire.point_slots.push(row.point_slot);
                }
            }
            DesignHistoricalLoopBoundary::Positions(rows) => {
                for row in rows {
                    wire.coedge_slots.push(row.point.vertex.coedge.coedge_slot);
                    wire.edge_slots.push(row.point.vertex.coedge.edge_slot);
                    wire.vertex_slots.push(row.point.vertex.vertex_slot);
                    wire.point_slots.push(row.point.point_slot);
                    wire.positions.push(row.position);
                }
            }
        }
        wire
    }
}

/// Historical topology surrounding one candidate edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignHistoricalEdgeContext {
    /// Stable ASM edge slot.
    pub edge_slot: i64,
    /// Incident coedge uses in stable coedge-slot order.
    pub incident_loops: Vec<DesignHistoricalEdgeLoopContext>,
}

/// One historical coedge use of a candidate edge and its ordered loop neighbors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignHistoricalEdgeLoopContext {
    /// Stable ASM coedge slot using the candidate edge.
    pub coedge_slot: i64,
    /// Stable ASM owner-loop slot.
    pub loop_slot: i64,
    /// Stable ASM owner-face slot.
    pub face_slot: i64,
    /// Number of coedges in the owner loop.
    pub boundary_edge_count: u32,
    /// Zero-based position of this coedge in the owner loop's ordered membership.
    pub coedge_ordinal: u32,
    /// Stable edge slot used by the preceding coedge.
    pub previous_edge_slot: i64,
    /// Stable edge slot used by the following coedge.
    pub next_edge_slot: i64,
}
