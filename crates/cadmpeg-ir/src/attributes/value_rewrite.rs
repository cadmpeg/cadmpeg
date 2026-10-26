// SPDX-License-Identifier: Apache-2.0
//! Identity traversal for typed attribute values.

use super::{AttributeExpression, AttributeJson, AttributeMapEntry, AttributeValue};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

rewrite_record!(AttributeMapEntry, []; {key, value});
rewrite_scalar!(AttributeJson);
rewrite_record!(AttributeExpression, []; {path, expression, comment});

impl RewriteIdentities for AttributeValue {
    fn visit_dependency_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(
            crate::schema::rewrite::typed::prune::DependencyReference<'_>,
        ) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        use crate::schema::rewrite::typed::prune::{DependencyPosition, DependencyReference};
        let _depth = ctx.enter_nested("walk attribute value dependencies")?;
        match self {
            Self::SubReference {
                object: Some(object),
                ..
            } => visitor(DependencyReference {
                target: object,
                position: DependencyPosition::Required,
                field: Some("object"),
            }),
            Self::Expression(value) => {
                for parameter in value.expression.dependencies() {
                    visitor(DependencyReference {
                        target: parameter.as_str(),
                        position: DependencyPosition::Required,
                        field: Some("expression"),
                    })?;
                }
                Ok(())
            }
            Self::Map(values) => values.visit_dependency_references(ctx, visitor),
            Self::Integer(_)
            | Self::Float(_)
            | Self::String(_)
            | Self::Boolean(_)
            | Self::Reference(_)
            | Self::Vector(_)
            | Self::ExternalReference { .. }
            | Self::SubReference { object: None, .. }
            | Self::EmptyReference
            | Self::Json(_) => Ok(()),
        }
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk attribute value references")?;
        match self {
            Self::SubReference {
                object: Some(object),
                ..
            } => visitor(object),
            Self::Expression(value) => value.visit_identity_references(ctx, visitor),
            Self::Map(values) => values.visit_identity_references(ctx, visitor),
            Self::Integer(_)
            | Self::Float(_)
            | Self::String(_)
            | Self::Boolean(_)
            | Self::Reference(_)
            | Self::Vector(_)
            | Self::ExternalReference { .. }
            | Self::SubReference { object: None, .. }
            | Self::EmptyReference
            | Self::Json(_) => Ok(()),
        }
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite attribute value")?;
        Ok(match self {
            Self::Integer(value) => Self::Integer(value.rewrite_identities(ctx, map)?),
            Self::Float(value) => Self::Float(value.rewrite_identities(ctx, map)?),
            Self::String(value) => Self::String(value.rewrite_identities(ctx, map)?),
            Self::Boolean(value) => Self::Boolean(value.rewrite_identities(ctx, map)?),
            Self::Reference(value) => Self::Reference(value.rewrite_identities(ctx, map)?),
            Self::Vector(values) => Self::Vector(values.rewrite_identities(ctx, map)?),
            Self::Map(values) => Self::Map(values.rewrite_identities(ctx, map)?),
            Self::Expression(value) => Self::Expression(value.rewrite_identities(ctx, map)?),
            Self::SubReference {
                object,
                subelements,
            } => Self::SubReference {
                object: object.as_deref().map(|id| map.identity(id)).transpose()?,
                subelements: subelements.rewrite_identities(ctx, map)?,
            },
            Self::ExternalReference {
                document,
                object,
                subelements,
            } => Self::ExternalReference {
                document,
                object,
                subelements,
            },
            Self::EmptyReference => Self::EmptyReference,
            Self::Json(value) => Self::Json(value),
        })
    }
}
