// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{AttributeTarget, SourceAttribute};

impl crate::schema::rewrite::typed::RewriteIdentities for AttributeTarget {
    const IS_REFERENCE_LEAF: bool = true;
    fn rewrite_native_value<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("walk native attribute target")?;

        let kind = value.get("kind").and_then(serde_json::Value::as_str);

        match kind {
            Some("body") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Body(id) => Some(id),
                    _ => None,
                },
            ),
            Some("face") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Face(id) => Some(id),
                    _ => None,
                },
            ),
            Some("shell") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Shell(id) => Some(id),
                    _ => None,
                },
            ),
            Some("loop") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Loop(id) => Some(id),
                    _ => None,
                },
            ),
            Some("coedge") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Coedge(id) => Some(id),
                    _ => None,
                },
            ),
            Some("edge") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Edge(id) => Some(id),
                    _ => None,
                },
            ),
            Some("vertex") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Vertex(id) => Some(id),
                    _ => None,
                },
            ),
            Some("region") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Region(id) => Some(id),
                    _ => None,
                },
            ),
            Some("surface") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Surface(id) => Some(id),
                    _ => None,
                },
            ),
            Some("curve") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Curve(id) => Some(id),
                    _ => None,
                },
            ),
            Some("point") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Point(id) => Some(id),
                    _ => None,
                },
            ),
            Some("feature") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Feature(id) => Some(id),
                    _ => None,
                },
            ),
            Some("sketch") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Sketch(id) => Some(id),
                    _ => None,
                },
            ),
            Some("parameter") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Parameter(id) => Some(id),
                    _ => None,
                },
            ),
            Some("product_definition") => {
                crate::schema::rewrite::typed::native_fields::rewrite_field(
                    ctx,
                    value,
                    "id",
                    map,
                    |owner: &Self| match owner {
                        Self::ProductDefinition(id) => Some(id),
                        _ => None,
                    },
                )
            }
            Some("occurrence") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Occurrence(id) => Some(id),
                    _ => None,
                },
            ),
            Some("tessellation") => {
                let id = value
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "attribute tessellation target has no id",
                        )
                    })?;
                let mapped = map.identity(id)?;
                value["id"] = serde_json::Value::String(mapped);
                Ok(())
            }
            Some("pmi") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Pmi(id) => Some(id),
                    _ => None,
                },
            ),
            Some("semantic_annotation") => {
                crate::schema::rewrite::typed::native_fields::rewrite_field(
                    ctx,
                    value,
                    "id",
                    map,
                    |owner: &Self| match owner {
                        Self::SemanticAnnotation(id) => Some(id),
                        _ => None,
                    },
                )
            }
            Some("drawing") => crate::schema::rewrite::typed::native_fields::rewrite_field(
                ctx,
                value,
                "id",
                map,
                |owner: &Self| match owner {
                    Self::Drawing(id) => Some(id),
                    _ => None,
                },
            ),
            Some("view_presentation") => {
                crate::schema::rewrite::typed::native_fields::rewrite_field(
                    ctx,
                    value,
                    "id",
                    map,
                    |owner: &Self| match owner {
                        Self::ViewPresentation(id) => Some(id),
                        _ => None,
                    },
                )
            }
            Some("record") => {
                let id = value
                    .get("id")
                    .and_then(|record| record.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("attribute record target has no id")
                    })?;
                let mapped = map.identity(id)?;
                value["id"]["id"] = serde_json::Value::String(mapped);
                Ok(())
            }
            Some("source") => {
                let id = value
                    .get("id")
                    .and_then(|id| id.get("source_id"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed(
                            "attribute source target has no source_id",
                        )
                    })?;
                if crate::ids::Identity::admit_text(id.to_owned()).is_ok() {
                    let mapped = map.identity(id)?;
                    value["id"]["source_id"] = serde_json::Value::String(mapped);
                }
                Ok(())
            }
            Some("document") => Ok(()),
            _ => Err(cadmpeg_core::CodecError::malformed(
                "native attribute target has an invalid kind",
            )),
        }
    }
    fn visit_identity_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("walk typed reference fields")?;

        match self {
            Self::Document => Ok(()),
            Self::Body(id) => id.visit_identity_references(ctx, visitor),
            Self::Face(id) => id.visit_identity_references(ctx, visitor),
            Self::Shell(id) => id.visit_identity_references(ctx, visitor),
            Self::Loop(id) => id.visit_identity_references(ctx, visitor),
            Self::Coedge(id) => id.visit_identity_references(ctx, visitor),
            Self::Edge(id) => id.visit_identity_references(ctx, visitor),
            Self::Vertex(id) => id.visit_identity_references(ctx, visitor),
            Self::Region(id) => id.visit_identity_references(ctx, visitor),
            Self::Surface(id) => id.visit_identity_references(ctx, visitor),
            Self::Curve(id) => id.visit_identity_references(ctx, visitor),
            Self::Point(id) => id.visit_identity_references(ctx, visitor),
            Self::Feature(id) => id.visit_identity_references(ctx, visitor),
            Self::Sketch(id) => id.visit_identity_references(ctx, visitor),
            Self::Parameter(id) => id.visit_identity_references(ctx, visitor),
            Self::ProductDefinition(id) => id.visit_identity_references(ctx, visitor),
            Self::Occurrence(id) => id.visit_identity_references(ctx, visitor),
            Self::Tessellation(id) => visitor(id),
            Self::Pmi(id) => id.visit_identity_references(ctx, visitor),
            Self::SemanticAnnotation(id) => id.visit_identity_references(ctx, visitor),
            Self::Drawing(id) => id.visit_identity_references(ctx, visitor),
            Self::ViewPresentation(id) => id.visit_identity_references(ctx, visitor),
            Self::Record { id, .. } => visitor(id),
        }
    }
    fn rewrite_identities<F: FnMut(&str) -> Result<String, cadmpeg_core::CodecError>>(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        map: &mut crate::schema::rewrite::typed::IdentityMap<'_, F>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested("typed rewrite variant")?;

        Ok(match self {
            Self::Document => Self::Document,
            Self::Body(id) => Self::Body(id.rewrite_identities(ctx, map)?),
            Self::Face(id) => Self::Face(id.rewrite_identities(ctx, map)?),
            Self::Shell(id) => Self::Shell(id.rewrite_identities(ctx, map)?),
            Self::Loop(id) => Self::Loop(id.rewrite_identities(ctx, map)?),
            Self::Coedge(id) => Self::Coedge(id.rewrite_identities(ctx, map)?),
            Self::Edge(id) => Self::Edge(id.rewrite_identities(ctx, map)?),
            Self::Vertex(id) => Self::Vertex(id.rewrite_identities(ctx, map)?),
            Self::Region(id) => Self::Region(id.rewrite_identities(ctx, map)?),
            Self::Surface(id) => Self::Surface(id.rewrite_identities(ctx, map)?),
            Self::Curve(id) => Self::Curve(id.rewrite_identities(ctx, map)?),
            Self::Point(id) => Self::Point(id.rewrite_identities(ctx, map)?),
            Self::Feature(id) => Self::Feature(id.rewrite_identities(ctx, map)?),
            Self::Sketch(id) => Self::Sketch(id.rewrite_identities(ctx, map)?),
            Self::Parameter(id) => Self::Parameter(id.rewrite_identities(ctx, map)?),
            Self::ProductDefinition(id) => {
                Self::ProductDefinition(id.rewrite_identities(ctx, map)?)
            }
            Self::Occurrence(id) => Self::Occurrence(id.rewrite_identities(ctx, map)?),
            Self::Tessellation(id) => Self::Tessellation(map.identity(&id)?),
            Self::Pmi(id) => Self::Pmi(id.rewrite_identities(ctx, map)?),
            Self::SemanticAnnotation(id) => {
                Self::SemanticAnnotation(id.rewrite_identities(ctx, map)?)
            }
            Self::Drawing(id) => Self::Drawing(id.rewrite_identities(ctx, map)?),
            Self::ViewPresentation(id) => Self::ViewPresentation(id.rewrite_identities(ctx, map)?),
            Self::Record { arena, id } => Self::Record {
                arena,
                id: map.identity(&id)?,
            },
        })
    }
}

rewrite_record!(SourceAttribute, []; {id, native_ref, target, name, values});
