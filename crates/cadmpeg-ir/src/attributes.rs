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
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        ctx.charge_work(0, operation)?;
        Ok(match self {
            Self::Document => Self::Document,
            Self::Body(id) => Self::Body(id.try_clone_for_decode(ctx, operation)?),
            Self::Face(id) => Self::Face(id.try_clone_for_decode(ctx, operation)?),
            Self::Shell(id) => Self::Shell(id.try_clone_for_decode(ctx, operation)?),
            Self::Loop(id) => Self::Loop(id.try_clone_for_decode(ctx, operation)?),
            Self::Coedge(id) => Self::Coedge(id.try_clone_for_decode(ctx, operation)?),
            Self::Edge(id) => Self::Edge(id.try_clone_for_decode(ctx, operation)?),
            Self::Vertex(id) => Self::Vertex(id.try_clone_for_decode(ctx, operation)?),
        })
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
            .map(Self::vector_component)
            .collect::<Option<Vec<_>>>()
            .map(Self::Vector)
    }

    fn vector_component(value: f64) -> Option<FiniteReal> {
        FiniteReal::new(value)
    }

    /// Admit component visits and candidate storage; retain only a finite vector.
    pub fn vector_for_decode(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        values: impl IntoIterator<Item = f64>,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        const OPERATION: &str = "IR attribute vector components";
        let mut storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut components = Vec::new();
        let mut values = values.into_iter();
        while let Some(value) = ctx.next_charged(&mut values, OPERATION)? {
            let Some(value) = Self::vector_component(value) else {
                return Ok(None);
            };
            ctx.push_scoped_vec(&mut storage, &mut components, value, OPERATION)?;
        }
        storage.commit()?;
        Ok(Some(Self::Vector(components)))
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
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn attribute_vector_decode_preserves_values_and_releases_rejected_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        for values in [[1.0, f64::NAN], [f64::INFINITY, 2.0]] {
            assert_eq!(super::AttributeValue::vector(values), None);
            assert_eq!(
                super::AttributeValue::vector_for_decode(&ctx, values).unwrap(),
                None
            );
        }
        ctx.finish_session().unwrap();
        let ctx = cadmpeg_test_support::service_decode_context();
        for values in [vec![], vec![1.0, -2.0, 0.0]] {
            assert_eq!(
                super::AttributeValue::vector_for_decode(&ctx, values.clone()).unwrap(),
                super::AttributeValue::vector(values)
            );
        }
    }

    #[test]
    fn attribute_vector_decode_refuses_each_resource_dimension() {
        for dimension in [
            ResourceDimension::WorkUnits,
            ResourceDimension::CollectionItems,
            ResourceDimension::MaterializedBytes,
            ResourceDimension::RetainedBytes,
        ] {
            cadmpeg_test_support::refusal::resource_limit_at(
                dimension,
                "IR attribute vector components",
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    match dimension {
                        ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                        ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                        _ => unreachable!(),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let result = super::AttributeValue::vector_for_decode(&ctx, [1.0, 2.0]);
                    if let Err(CodecError::ResourceLimit(limit)) = &result {
                        assert!(
                            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit)
                        );
                    }
                    result
                },
            );
        }
    }

    #[test]
    fn attribute_target_copy_charges_only_owned_identity() {
        let target = AttributeTarget::Face(crate::ids::FaceId::mint("test:model:face#17").unwrap());
        let identity_len = "test:model:face#17".len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(identity_len).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            target
                .try_clone_for_decode(&ctx, "attribute target copy")
                .unwrap(),
            target
        );
        let error = target
            .try_clone_for_decode(&ctx, "attribute target copy")
            .unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "attribute target copy"));
        assert!(matches!(
            (error, AttributeTarget::Document.try_clone_for_decode(&ctx, "later document target")),
            (CodecError::ResourceLimit(original), Err(CodecError::ResourceLimit(sticky))) if original == sticky
        ));
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            AttributeTarget::Document
                .try_clone_for_decode(&ctx, "document target copy")
                .unwrap(),
            AttributeTarget::Document
        );
    }
}

mod identity_rewrite;
