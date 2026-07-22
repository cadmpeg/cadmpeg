// SPDX-License-Identifier: Apache-2.0
//! Source-native attributes attached to IR entities.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AttributeId, BodyId, CoedgeId, EdgeId, FaceId, LoopId, ShellId, VertexId};

/// An entity which owns a source attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AttributeTarget {
    /// Attribute is owned by the document as a whole, not a specific entity.
    Document,
    /// Attribute is owned by a body.
    Body(BodyId),
    /// Attribute is owned by a face.
    Face(FaceId),
    /// Attribute is owned by a shell.
    Shell(ShellId),
    /// Attribute is owned by a loop.
    Loop(LoopId),
    /// Attribute is owned by a coedge.
    Coedge(CoedgeId),
    /// Attribute is owned by an edge.
    Edge(EdgeId),
    /// Attribute is owned by a vertex.
    Vertex(VertexId),
}

/// One ordered typed value from a source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AttributeValue {
    /// A signed integer value.
    Integer(i64),
    /// A floating-point value.
    Float(f64),
    /// A text value.
    String(String),
    /// A boolean value.
    Boolean(bool),
    /// A string-encoded reference to another entity, opaque to this crate.
    Reference(String),
    /// A fixed- or variable-length numeric vector value.
    Vector(Vec<f64>),
}

/// A linked source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceAttribute {
    /// Stable id of this attribute record.
    pub id: AttributeId,
    /// Entity this attribute is attached to.
    pub target: AttributeTarget,
    /// Source attribute name, as recorded in the native attribute table.
    pub name: String,
    /// Ordered typed values carried by this attribute; length and types are
    /// source-defined and vary per attribute name.
    pub values: Vec<AttributeValue>,
}
