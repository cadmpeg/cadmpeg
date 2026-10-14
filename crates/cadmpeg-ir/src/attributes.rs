// SPDX-License-Identifier: Apache-2.0
//! Source-native attributes attached to IR entities.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{AttributeId, BodyId, CoedgeId, EdgeId, FaceId, LoopId, ShellId, VertexId};
use crate::scalar::FiniteReal;

/// An entity which owns a source attribute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
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

impl AttributeTarget {
    /// Copy the owning identity under the decode budget.
    pub fn try_clone_for_decode(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(self.clone())
    }
}

/// One ordered typed value from a source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum AttributeValue {
    /// A signed integer value.
    Integer(i64),
    /// A floating-point value.
    Float(#[cfg_attr(feature = "schema", schemars(with = "f64"))] FiniteReal),
    /// A text value.
    String(String),
    /// A boolean value.
    Boolean(bool),
    /// A string-encoded reference to another entity, opaque to this crate.
    Reference(String),
    /// A fixed- or variable-length numeric vector value.
    Vector(#[cfg_attr(feature = "schema", schemars(with = "Vec<f64>"))] Vec<FiniteReal>),
}

impl AttributeValue {
    /// A float value, or `None` when `value` is not finite.
    #[must_use]
    pub fn float(value: f64) -> Option<Self> {
        FiniteReal::new(value).map(Self::Float)
    }

    /// A vector value, or `None` when a component is not finite.
    pub fn vector(values: impl IntoIterator<Item = f64>) -> Option<Self> {
        values
            .into_iter()
            .map(FiniteReal::new)
            .collect::<Option<Vec<_>>>()
            .map(Self::Vector)
    }
}

/// A linked source attribute record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
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

#[cfg(test)]
mod tests {
    use super::AttributeTarget;

    #[test]
    fn attribute_target_copy_preserves_the_target() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let target = AttributeTarget::Face(crate::ids::FaceId::mint("test:model:face#17").unwrap());
        assert_eq!(
            target
                .try_clone_for_decode(&ctx, "attribute target copy")
                .unwrap(),
            target
        );
        assert_eq!(
            AttributeTarget::Document
                .try_clone_for_decode(&ctx, "document target copy")
                .unwrap(),
            AttributeTarget::Document
        );
    }
}

mod identity_rewrite;
