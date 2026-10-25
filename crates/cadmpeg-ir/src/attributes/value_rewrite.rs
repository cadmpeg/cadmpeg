// SPDX-License-Identifier: Apache-2.0
//! Attribute references distinguish local identities from literal external names.

use super::{AttributeExpression, AttributeNumericList, AttributeValue};
use crate::schema::rewrite::typed::{
    prune::{DependencyPosition, DependencyReference},
    IdentityMap, RewriteIdentities,
};
use cadmpeg_core::{decode::DecodeContext, CodecError};

rewrite_scalar!(AttributeNumericList);

impl RewriteIdentities for AttributeExpression {
    fn visit_dependency_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.expression
            .visit_identity_references(ctx, &mut |target| {
                visitor(DependencyReference {
                    target,
                    position: DependencyPosition::Required,
                    field: Some("expression"),
                })
            })
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.expression.visit_identity_references(ctx, visitor)
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            expression: self.expression.rewrite_identities(ctx, map)?,
            ..self
        })
    }
}

impl RewriteIdentities for AttributeValue {
    fn visit_dependency_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(DependencyReference<'_>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        match self {
            Self::SubReference { object, .. } => visitor(DependencyReference {
                target: object,
                position: DependencyPosition::Required,
                field: Some("object"),
            }),
            Self::Expression(value) => value.visit_dependency_references(ctx, visitor),
            _ => Ok(()),
        }
    }

    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk attribute references")?;
        match self {
            Self::SubReference { object, .. } => visitor(object),
            Self::Expression(value) => value.visit_identity_references(ctx, visitor),
            _ => Ok(()),
        }
    }

    fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_, F>,
    ) -> Result<Self, CodecError> {
        let _depth = ctx.enter_nested("rewrite attribute references")?;
        Ok(match self {
            Self::SubReference {
                object,
                subelements,
            } => Self::SubReference {
                object: map.identity(&object)?,
                subelements,
            },
            Self::Expression(value) => Self::Expression(value.rewrite_identities(ctx, map)?),
            Self::String(value) => Self::String(value.rewrite_identities(ctx, map)?),
            Self::Reference(value) => Self::Reference(value.rewrite_identities(ctx, map)?),
            value => value,
        })
    }
}
