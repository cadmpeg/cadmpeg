// SPDX-License-Identifier: Apache-2.0
//! Boundary-representation topology.
//!
//! Flat arenas in [`crate::document::Model`] store the hierarchy
//! `body → region → shell → face → loop → coedge → edge → vertex`. Faces,
//! edges, coedges, and vertices reference surface, curve, pcurve, and point
//! carriers by typed ID.

use crate::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use crate::math::Point3;
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// RGBA color, components in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Color {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha (opacity).
    pub a: f32,
}

/// Orientation relative to referenced geometry.
///
/// For a coedge this compares traversal with its edge curve. For a face it
/// compares the face normal with its surface normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Sense {
    /// Same direction as the referenced geometry.
    Forward,
    /// Opposite direction to the referenced geometry.
    Reversed,
}

/// A top-level solid, sheet, wire, or general body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    /// A closed, volume-bounding solid body.
    #[default]
    Solid,
    /// An open, zero-thickness sheet body.
    Sheet,
    /// A one-dimensional body composed of wires.
    Wire,
    /// A body containing mixed-dimensional topology.
    General,
}

/// A top-level solid, sheet, wire, or general body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Body {
    /// Arena id.
    pub id: BodyId,
    /// The dimensional kind of topology contained by the body.
    #[serde(default)]
    pub kind: BodyKind,
    /// Constituent regions.
    pub regions: Vec<RegionId>,
    /// Optional world placement of the body's geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Whether the source document displays the body. `None` when the source
    /// format does not record body visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

/// A connected region of a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Region {
    /// Arena id.
    pub id: RegionId,
    /// Owning body.
    pub body: BodyId,
    /// Ordered boundary shells. For a solid region, the first shell is the
    /// exterior boundary and all subsequent shells bound voids.
    pub shells: Vec<ShellId>,
}

impl Region {
    /// Exterior boundary of a solid region.
    pub fn exterior_shell(&self) -> Option<&ShellId> {
        self.shells.first()
    }

    /// Ordered void boundaries of a solid region.
    pub fn void_shells(&self) -> impl Iterator<Item = &ShellId> {
        self.shells.iter().skip(1)
    }
}

/// An oriented boundary of a region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Shell {
    /// Arena id.
    pub id: ShellId,
    /// Owning region.
    pub region: RegionId,
    /// Faces of the shell.
    pub faces: Vec<FaceId>,
    /// Edges belonging directly to a wire shell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wire_edges: Vec<EdgeId>,
    /// Vertices belonging directly to a shell and not bounding an edge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub free_vertices: Vec<VertexId>,
}

/// A face: a bounded region of a surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Face {
    /// Arena id.
    pub id: FaceId,
    /// Owning shell.
    pub shell: ShellId,
    /// Underlying surface carrier.
    pub surface: SurfaceId,
    /// Whether the face normal agrees with the surface normal.
    pub sense: Sense,
    /// Boundary loops (first is conventionally the outer loop).
    pub loops: Vec<LoopId>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional display color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

/// A loop's boundary role within its owning face.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LoopBoundaryRole {
    /// The source does not classify this loop as outer or inner.
    #[default]
    Unspecified,
    /// The loop is the explicit exterior boundary of the face.
    Outer,
    /// The loop bounds material excluded from the face; all loops may be inner
    /// when the surface parameter domain supplies the exterior boundary.
    Inner,
}

/// A closed boundary of a face, expressed as an ordered ring of coedges or one
/// vertex use at a surface singularity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Loop {
    /// Arena id.
    pub id: LoopId,
    /// Owning face.
    pub face: FaceId,
    /// Boundary role within the owning face.
    #[serde(default)]
    pub boundary_role: LoopBoundaryRole,
    /// Vertex-only or coedge-ring boundary.
    #[serde(flatten, with = "loop_boundary_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "LoopBoundarySchemaWire"))]
    pub boundary: LoopBoundary,
}

/// One ordered parameter-space representation of a coedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PcurveUse {
    /// Parameter-space curve carrier.
    pub pcurve: PcurveId,
    /// Whether the source declares this curve isoparametric on the face surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isoparametric: Option<bool>,
    /// Interval on the pcurve's own parameterization used by this coedge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_range: Option<[f64; 2]>,
}

/// The mutually exclusive forms of a loop boundary.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum LoopBoundary {
    /// One unanchored vertex at a surface singularity.
    Vertex {
        /// Referenced pole vertex.
        vertex: VertexId,
        /// Ordered parameter-space images associated with the pole.
        pcurves: Vec<PcurveUse>,
    },
    /// An ordered coedge ring and its anchored pole occurrences.
    Ring {
        /// Coedges in ring order.
        coedges: Vec<CoedgeId>,
        /// Ordered pole-vertex occurrences within the cyclic traversal.
        vertex_uses: Vec<AnchoredVertexUse>,
    },
}

impl Loop {
    /// Returns the ordered coedges when this is a ring boundary.
    #[must_use]
    pub fn coedges(&self) -> &[CoedgeId] {
        match &self.boundary {
            LoopBoundary::Vertex { .. } => &[],
            LoopBoundary::Ring { coedges, .. } => coedges,
        }
    }

    /// Returns the anchored vertex uses when this is a ring boundary.
    #[must_use]
    pub fn anchored_vertex_uses(&self) -> &[AnchoredVertexUse] {
        match &self.boundary {
            LoopBoundary::Vertex { .. } => &[],
            LoopBoundary::Ring { vertex_uses, .. } => vertex_uses,
        }
    }

    /// Returns mutable ring members when this is a ring boundary.
    pub fn ring_mut(&mut self) -> Option<(&mut Vec<CoedgeId>, &mut Vec<AnchoredVertexUse>)> {
        match &mut self.boundary {
            LoopBoundary::Vertex { .. } => None,
            LoopBoundary::Ring {
                coedges,
                vertex_uses,
            } => Some((coedges, vertex_uses)),
        }
    }

    /// Returns the singular vertex and its parameter-space images.
    #[must_use]
    pub fn singular_vertex(&self) -> Option<(&VertexId, &[PcurveUse])> {
        match &self.boundary {
            LoopBoundary::Vertex { vertex, pcurves } => Some((vertex, pcurves)),
            LoopBoundary::Ring { .. } => None,
        }
    }

    /// Iterates over every vertex referenced directly by this boundary.
    pub fn vertices(&self) -> impl Iterator<Item = &VertexId> {
        let (singular, anchored) = match &self.boundary {
            LoopBoundary::Vertex { vertex, .. } => (Some(vertex), &[][..]),
            LoopBoundary::Ring { vertex_uses, .. } => (None, vertex_uses.as_slice()),
        };
        singular
            .into_iter()
            .chain(anchored.iter().map(|use_| &use_.vertex))
    }

    /// Iterates over every parameter-space image attached to a boundary vertex.
    pub fn vertex_pcurves(&self) -> impl Iterator<Item = &PcurveUse> {
        let (singular, anchored) = match &self.boundary {
            LoopBoundary::Vertex { pcurves, .. } => (Some(pcurves.as_slice()), &[][..]),
            LoopBoundary::Ring { vertex_uses, .. } => (None, vertex_uses.as_slice()),
        };
        singular
            .into_iter()
            .flatten()
            .chain(anchored.iter().flat_map(|use_| use_.pcurves.iter()))
    }

    /// Iterates over boundary vertices with their optional ring anchor and pcurves.
    pub fn vertex_occurrences(
        &self,
    ) -> impl Iterator<Item = (&VertexId, Option<&CoedgeId>, &[PcurveUse])> {
        let (singular, anchored) = match &self.boundary {
            LoopBoundary::Vertex { vertex, pcurves } => {
                (Some((vertex, pcurves.as_slice())), &[][..])
            }
            LoopBoundary::Ring { vertex_uses, .. } => (None, vertex_uses.as_slice()),
        };
        singular
            .into_iter()
            .map(|(vertex, pcurves)| (vertex, None, pcurves))
            .chain(
                anchored
                    .iter()
                    .map(|use_| (&use_.vertex, Some(&use_.after), use_.pcurves.as_slice())),
            )
    }
}

/// One pole-vertex occurrence anchored after a coedge in a ring traversal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AnchoredVertexUse {
    /// Referenced pole vertex.
    pub vertex: VertexId,
    /// Preceding coedge in the cyclic traversal.
    pub after: CoedgeId,
    /// Ordered parameter-space images associated with this pole occurrence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the loop boundary wire schema")]
struct LoopBoundarySchemaWire {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    coedges: Vec<CoedgeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    vertex_uses: Vec<LoopVertexUseSchemaWire>,
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the loop vertex-use wire schema")]
struct LoopVertexUseSchemaWire {
    vertex: VertexId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after: Option<CoedgeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pcurves: Vec<PcurveUse>,
}

mod loop_boundary_wire {
    use super::{AnchoredVertexUse, CoedgeId, LoopBoundary, PcurveUse, VertexId};
    use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        coedges: Vec<CoedgeId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vertex_uses: Vec<VertexUseWire>,
    }

    #[derive(Serialize, Deserialize)]
    struct VertexUseWire {
        vertex: VertexId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<CoedgeId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pcurves: Vec<PcurveUse>,
    }

    pub fn serialize<S>(value: &LoopBoundary, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            LoopBoundary::Vertex { vertex, pcurves } => Wire {
                coedges: Vec::new(),
                vertex_uses: vec![VertexUseWire {
                    vertex: vertex.clone(),
                    after: None,
                    pcurves: pcurves.clone(),
                }],
            },
            LoopBoundary::Ring {
                coedges,
                vertex_uses,
            } => Wire {
                coedges: coedges.clone(),
                vertex_uses: vertex_uses
                    .iter()
                    .map(|vertex_use| VertexUseWire {
                        vertex: vertex_use.vertex.clone(),
                        after: Some(vertex_use.after.clone()),
                        pcurves: vertex_use.pcurves.clone(),
                    })
                    .collect(),
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<LoopBoundary, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        if wire.coedges.is_empty() {
            let [vertex_use] = <[VertexUseWire; 1]>::try_from(wire.vertex_uses).map_err(|_| {
                D::Error::custom("loop vertex_uses must contain one item when coedges is empty")
            })?;
            if vertex_use.after.is_some() {
                return Err(D::Error::custom(
                    "loop vertex use must omit after when coedges is empty",
                ));
            }
            return Ok(LoopBoundary::Vertex {
                vertex: vertex_use.vertex,
                pcurves: vertex_use.pcurves,
            });
        }

        let vertex_uses = wire
            .vertex_uses
            .into_iter()
            .map(|vertex_use| {
                let after = vertex_use
                    .after
                    .ok_or_else(|| D::Error::custom("loop ring vertex use must include after"))?;
                if !wire.coedges.contains(&after) {
                    return Err(D::Error::custom(
                        "loop ring vertex use after must name a coedge in the ring",
                    ));
                }
                Ok(AnchoredVertexUse {
                    vertex: vertex_use.vertex,
                    after,
                    pcurves: vertex_use.pcurves,
                })
            })
            .collect::<Result<_, D::Error>>()?;
        Ok(LoopBoundary::Ring {
            coedges: wire.coedges,
            vertex_uses,
        })
    }
}

/// One use of an edge by a loop.
///
/// Coedges form a loop ring through `next` and `previous`, and a radial ring
/// around their shared edge through `radial_next`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Coedge {
    /// Arena id.
    pub id: CoedgeId,
    /// Owning loop.
    pub owner_loop: LoopId,
    /// Underlying edge.
    pub edge: EdgeId,
    /// Next coedge in the loop ring.
    pub next: CoedgeId,
    /// Previous coedge in the loop ring.
    pub previous: CoedgeId,
    /// Next coedge around the edge; self-reference denotes a laminar boundary.
    pub radial_next: CoedgeId,
    /// Direction relative to the edge curve.
    pub sense: Sense,
    /// Ordered parameter-space images of this coedge on the face surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pcurves: Vec<PcurveUse>,
    /// Optional coedge-local 3D carrier used instead of the shared edge curve.
    #[serde(flatten, with = "coedge_use_curve_wire")]
    #[cfg_attr(feature = "schema", schemars(with = "CoedgeUseCurveSchemaWire"))]
    pub use_curve: Option<CoedgeUseCurve>,
}

/// A coedge-local curve and its loop-traversal interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CoedgeUseCurve {
    /// Local 3D curve carrier.
    pub curve: CurveId,
    /// Interval on the carrier in loop-traversal order.
    pub parameter_range: [f64; 2],
}

#[cfg(feature = "schema")]
#[derive(JsonSchema)]
#[expect(dead_code, reason = "fields define the coedge use-curve wire schema")]
struct CoedgeUseCurveSchemaWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    use_curve: Option<CurveId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    use_curve_parameter_range: Option<[f64; 2]>,
}

mod coedge_use_curve_wire {
    use super::{CoedgeUseCurve, CurveId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        use_curve: Option<CurveId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        use_curve_parameter_range: Option<[f64; 2]>,
    }

    pub fn serialize<S>(value: &Option<CoedgeUseCurve>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match value {
            Some(value) => Wire {
                use_curve: Some(value.curve.clone()),
                use_curve_parameter_range: Some(value.parameter_range),
            },
            None => Wire {
                use_curve: None,
                use_curve_parameter_range: None,
            },
        };
        wire.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<CoedgeUseCurve>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Wire::deserialize(deserializer)?;
        match (wire.use_curve, wire.use_curve_parameter_range) {
            (Some(curve), Some(parameter_range)) => Ok(Some(CoedgeUseCurve {
                curve,
                parameter_range,
            })),
            (None, None) => Ok(None),
            _ => Err(serde::de::Error::custom(
                "use_curve and use_curve_parameter_range must occur together",
            )),
        }
    }
}

/// An edge: a bounded segment of a 3D curve between two vertices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Edge {
    /// Arena id.
    pub id: EdgeId,
    /// Underlying 3D curve carrier. `None` for a degenerate/tolerant edge with
    /// no attributed curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<CurveId>,
    /// Start vertex.
    pub start: VertexId,
    /// End vertex.
    pub end: VertexId,
    /// Parameter range `[t_start, t_end]` on the curve's own
    /// parameterization, when known: the start vertex lies at `t_start`.
    /// Conic parameters are angles from the reference direction; line
    /// parameters are signed distances along the unit direction in the
    /// document's length unit.
    /// A carrier-less degenerate or tolerant edge has no canonical domain;
    /// finite native endpoint values may still be retained without ordering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_range: Option<[f64; 2]>,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

/// A vertex: a topological point referencing a position carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Vertex {
    /// Arena id.
    pub id: VertexId,
    /// Position carrier.
    pub point: PointId,
    /// Optional geometric tolerance in the document's length unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f64>,
}

/// A position carrier for a vertex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Point {
    /// Arena id.
    pub id: PointId,
    /// Coordinates in the document's length unit.
    pub position: Point3,
    /// Source object carrying this free point, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object: Option<crate::provenance::SourceObjectAssociation>,
}

#[cfg(test)]
mod tests {
    use super::{Coedge, CoedgeUseCurve, Loop, LoopBoundary};

    fn coedge_json() -> serde_json::Value {
        serde_json::json!({
            "id": "test:model:coedge#0",
            "owner_loop": "test:model:loop#0",
            "edge": "test:model:edge#0",
            "next": "test:model:coedge#0",
            "previous": "test:model:coedge#0",
            "radial_next": "test:model:coedge#0",
            "sense": "forward",
            "use_curve": "test:model:curve#0",
            "use_curve_parameter_range": [0.25, 0.75]
        })
    }

    #[test]
    fn coedge_use_curve_preserves_the_flat_wire_fields() {
        let coedge: Coedge = serde_json::from_value(coedge_json()).unwrap();
        assert_eq!(
            coedge.use_curve,
            Some(CoedgeUseCurve {
                curve: "test:model:curve#0".into(),
                parameter_range: [0.25, 0.75],
            })
        );
        let encoded = serde_json::to_value(coedge).unwrap();
        assert_eq!(encoded["use_curve"], "test:model:curve#0");
        assert_eq!(
            encoded["use_curve_parameter_range"],
            serde_json::json!([0.25, 0.75])
        );
    }

    #[test]
    fn coedge_use_curve_rejects_a_split_wire_pair() {
        let mut json = coedge_json();
        json.as_object_mut()
            .unwrap()
            .remove("use_curve_parameter_range");
        assert!(serde_json::from_value::<Coedge>(json).is_err());
    }

    #[test]
    fn vertex_loop_preserves_the_flat_wire_fields() {
        let json = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "vertex_uses": [{ "vertex": "test:model:vertex#0" }]
        });
        let loop_: Loop = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(
            loop_.boundary,
            LoopBoundary::Vertex { ref vertex, ref pcurves }
                if vertex.0 == "test:model:vertex#0" && pcurves.is_empty()
        ));
        assert_eq!(serde_json::to_value(loop_).unwrap(), json);
    }

    #[test]
    fn ring_loop_preserves_the_flat_wire_fields() {
        let json = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "coedges": ["test:model:coedge#0"],
            "vertex_uses": [{
                "vertex": "test:model:vertex#0",
                "after": "test:model:coedge#0"
            }]
        });
        let loop_: Loop = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(loop_.boundary, LoopBoundary::Ring { .. }));
        assert_eq!(serde_json::to_value(loop_).unwrap(), json);
    }

    #[test]
    fn loop_boundary_rejects_split_wire_forms() {
        let vertex_only_with_anchor = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "vertex_uses": [{
                "vertex": "test:model:vertex#0",
                "after": "test:model:coedge#0"
            }]
        });
        assert!(serde_json::from_value::<Loop>(vertex_only_with_anchor).is_err());

        let ring_without_anchor = serde_json::json!({
            "id": "test:model:loop#0",
            "face": "test:model:face#0",
            "boundary_role": "outer",
            "coedges": ["test:model:coedge#0"],
            "vertex_uses": [{ "vertex": "test:model:vertex#0" }]
        });
        assert!(serde_json::from_value::<Loop>(ring_without_anchor).is_err());
    }
}
