// SPDX-License-Identifier: Apache-2.0
//! Source-native attributes attached to IR entities.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AttributeId, BodyId, CoedgeId, EdgeId, FaceId, VertexId};
use crate::provenance::EntityMeta;

/// An entity which owns a source attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum AttributeTarget {
    Document,
    Body(BodyId),
    Face(FaceId),
    Coedge(CoedgeId),
    Edge(EdgeId),
    Vertex(VertexId),
}

/// One ordered typed value from a source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AttributeValue {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Reference(String),
    Vector(Vec<f64>),
}

/// A linked source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceAttribute {
    pub id: AttributeId,
    pub target: AttributeTarget,
    pub name: String,
    pub values: Vec<AttributeValue>,
    pub meta: EntityMeta,
}
