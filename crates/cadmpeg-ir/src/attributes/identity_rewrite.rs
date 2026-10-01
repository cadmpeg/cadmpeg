// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{AttributeTarget, AttributeValue, SourceAttribute};

impl crate::schema::rewrite::typed::RewriteIdentities for AttributeTarget {
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(ctx: &cadmpeg_core::decode::DecodeContext<'_>, value: &mut serde_json::Value, map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>) -> Result<(), cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("walk native attribute target")?;
        let work = cadmpeg_core::decode::u64_from_index(value.as_object().map_or(0, serde_json::Map::len)).checked_mul(4).ok_or_else(|| ctx.refuse_codec_limit("find native attribute kind", u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(work, "find native attribute kind")?;
        let kind = value.get("kind").and_then(serde_json::Value::as_str);
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(kind.map_or(0, str::len)).checked_mul(8).ok_or_else(|| ctx.refuse_codec_limit("match native attribute kind", u64::MAX - 1, u64::MAX))?, "match native attribute kind")?;
        match kind {
            Some("body") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Body(id) => Some(id), _ => None }),
            Some("face") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Face(id) => Some(id), _ => None }),
            Some("shell") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Shell(id) => Some(id), _ => None }),
            Some("loop") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Loop(id) => Some(id), _ => None }),
            Some("coedge") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Coedge(id) => Some(id), _ => None }),
            Some("edge") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Edge(id) => Some(id), _ => None }),
            Some("vertex") => crate::schema::rewrite::typed::native_fields::rewrite_field(ctx, value, "id", map, |owner: &Self| match owner { Self::Vertex(id) => Some(id), _ => None }),
            Some("document") => Ok(()),
            _ => Err(cadmpeg_core::CodecError::malformed("native attribute target has an invalid kind")),
        }
    }
    fn visit_identity_references(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>) -> Result<(), cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("walk typed reference fields")?;
        ctx.charge_work(1, "walk typed reference fields")?;
        match self {
            Self::Document => Ok(()),
            Self::Body(id) => id.visit_identity_references(ctx, visitor),
            Self::Face(id) => id.visit_identity_references(ctx, visitor),
            Self::Shell(id) => id.visit_identity_references(ctx, visitor),
            Self::Loop(id) => id.visit_identity_references(ctx, visitor),
            Self::Coedge(id) => id.visit_identity_references(ctx, visitor),
            Self::Edge(id) => id.visit_identity_references(ctx, visitor),
            Self::Vertex(id) => id.visit_identity_references(ctx, visitor),
        }
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>) -> Result<Self, cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("typed rewrite variant")?;
        ctx.charge_work(1, "typed rewrite variant")?;
        Ok(match self {
            Self::Document => Self::Document,
            Self::Body(id) => Self::Body(id.rewrite_identities(ctx, map)?),
            Self::Face(id) => Self::Face(id.rewrite_identities(ctx, map)?),
            Self::Shell(id) => Self::Shell(id.rewrite_identities(ctx, map)?),
            Self::Loop(id) => Self::Loop(id.rewrite_identities(ctx, map)?),
            Self::Coedge(id) => Self::Coedge(id.rewrite_identities(ctx, map)?),
            Self::Edge(id) => Self::Edge(id.rewrite_identities(ctx, map)?),
            Self::Vertex(id) => Self::Vertex(id.rewrite_identities(ctx, map)?),
        })
    }
}

rewrite_enum!(AttributeValue, []; {
    Integer(field0),
    Float(field0),
    String(field0),
    Boolean(field0),
    Reference(field0),
    Vector(field0),
});
rewrite_record!(SourceAttribute, []; {id, target, name, values});
