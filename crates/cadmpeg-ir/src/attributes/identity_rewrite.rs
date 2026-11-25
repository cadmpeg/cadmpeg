// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{AttributeJson, AttributeMapEntry, AttributeTarget, AttributeValue, SourceAttribute};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// The native rewrite of the target arms that hold one id as `value`.
macro_rules! rewrite_native_target {
    ($ctx:ident, $value:ident, $map:ident, $kind:ident; $($name:literal => $variant:ident),* $(,)?) => {
        match $kind {
            $(Some($name) => crate::schema::rewrite::typed::native_fields::rewrite_field(
                $ctx,
                $value,
                "value",
                $map,
                |owner: &Self| match owner {
                    Self::$variant(id) => Some(id),
                    _ => None,
                },
            ),)*
            Some("document" | "tessellation" | "record") => Ok(()),
            _ => Err(CodecError::malformed(
                "native attribute target has an invalid kind",
            )),
        }
    };
}

impl RewriteIdentities for AttributeTarget {
    fn rewrite_native_value(
        ctx: &DecodeContext<'_>,
        value: &mut serde_json::Value,
        map: &mut IdentityMap<'_>,
    ) -> Result<(), CodecError> {
        let _depth = ctx.enter_nested("walk native attribute target")?;

        let kind = value.get("kind").and_then(serde_json::Value::as_str);

        rewrite_native_target!(ctx, value, map, kind;
            "body" => Body,
            "face" => Face,
            "shell" => Shell,
            "loop" => Loop,
            "coedge" => Coedge,
            "edge" => Edge,
            "vertex" => Vertex,
            "region" => Region,
            "surface" => Surface,
            "curve" => Curve,
            "point" => Point,
            "feature" => Feature,
            "sketch" => Sketch,
            "parameter" => Parameter,
            "product_definition" => ProductDefinition,
            "occurrence" => Occurrence,
            "pmi" => Pmi,
            "semantic_annotation" => SemanticAnnotation,
            "drawing" => Drawing,
            "view_presentation" => ViewPresentation,
            "configuration" => Configuration,
        )
    }
    fn visit_identity_references(
        &self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
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
            Self::Tessellation(id) => id.visit_identity_references(ctx, visitor),
            Self::Pmi(id) => id.visit_identity_references(ctx, visitor),
            Self::SemanticAnnotation(id) => id.visit_identity_references(ctx, visitor),
            Self::Drawing(id) => id.visit_identity_references(ctx, visitor),
            Self::ViewPresentation(id) => id.visit_identity_references(ctx, visitor),
            Self::Configuration(id) => id.visit_identity_references(ctx, visitor),
            Self::Record { arena, id } => {
                arena.visit_identity_references(ctx, visitor)?;
                id.visit_identity_references(ctx, visitor)
            }
        }
    }
    fn rewrite_identities(
        self,
        ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
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
            Self::Tessellation(id) => Self::Tessellation(id.rewrite_identities(ctx, map)?),
            Self::Pmi(id) => Self::Pmi(id.rewrite_identities(ctx, map)?),
            Self::SemanticAnnotation(id) => {
                Self::SemanticAnnotation(id.rewrite_identities(ctx, map)?)
            }
            Self::Drawing(id) => Self::Drawing(id.rewrite_identities(ctx, map)?),
            Self::ViewPresentation(id) => Self::ViewPresentation(id.rewrite_identities(ctx, map)?),
            Self::Configuration(id) => Self::Configuration(id.rewrite_identities(ctx, map)?),
            Self::Record { arena, id } => Self::Record {
                arena: arena.rewrite_identities(ctx, map)?,
                id: id.rewrite_identities(ctx, map)?,
            },
        })
    }
}

rewrite_enum!(AttributeValue, []; {
    Integer(field0),
    Float(field0),
    String(field0),
    Boolean(field0),
    Reference(field0),
    Parameter(field0),
    Vector(field0),
    Map(field0),
    SubReference {object, subelements},
    ExternalReference {document, object, subelements},
    EmptyReference,
    Json(field0),
});
rewrite_record!(AttributeMapEntry, []; {key, value});
rewrite_enum!(AttributeJson, []; {
    Null,
    Boolean(field0),
    Integer(field0),
    Unsigned(field0),
    Float(field0),
    String(field0),
    Array(field0),
    Object(field0),
});
rewrite_record!(SourceAttribute, []; {id, target, name, values, native_ref});
