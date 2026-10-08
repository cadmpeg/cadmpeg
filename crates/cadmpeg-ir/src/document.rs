// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::hash::{Hash, Hasher};

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{
    ser::{SerializeMap, SerializeSeq, SerializeStruct},
    Deserialize, Deserializer, Serialize, Serializer,
};

use cadmpeg_core::decode::admission::{Admission, StandardAdmission};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::dialect::{DialectLayers, DialectMatch, FormatIdentity};
use cadmpeg_core::CodecError;

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
    FeatureRowWire, FeatureWriteWire,
};
use crate::geometry::{
    pcurve::Pcurve, Curve, ProceduralCurve, ProceduralCurveRow, ProceduralSurface,
    ProceduralSurfaceRow, Surface,
};
use crate::hash::finite_json::CanonicalJsonError;
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::native::Native;
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Occurrence, ProductDefinition};
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use crate::units::{CanonicalUnitsWire, Tolerances};
use crate::unknown::NativeUnknownRecord;
use cadmpeg_core::text::NonBlankString;

pub mod admission;
use admission::ModelAdmission;

pub(crate) mod census;
pub(crate) mod feature_parents;
mod procedural;

struct UnknownProjection<T>(T);

impl<T: Borrow<NativeUnknownRecord>> Serialize for UnknownProjection<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.borrow().serialize(serializer)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FeatureRegenerationParents(
    BTreeMap<crate::features::FeatureId, crate::features::FeatureId>,
);

impl FeatureRegenerationParents {
    pub(crate) fn equivalent(
        &self,
        other: &Self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        let operation = "compare model checkpoint parents";
        ctx.charge_work(1, operation)?;
        if self.0.len() != other.0.len() {
            return Ok(false);
        }
        for ((left_child, left_parent), (right_child, right_parent)) in self.0.iter().zip(&other.0)
        {
            let bytes = left_child
                .as_str()
                .len()
                .checked_add(left_parent.as_str().len())
                .and_then(|bytes| bytes.checked_add(2))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(u64_from_index(bytes), operation)?;
            if left_child != right_child || left_parent != right_parent {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Admit the nodes rebuilt when two nonempty parent tables are merged.
    pub(crate) fn reserve_append(
        &self,
        incoming: &Self,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if !self.0.is_empty() && !incoming.0.is_empty() {
            let count = self.0.len().checked_add(incoming.0.len()).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "append feature regeneration parents",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?;
            let mut longest = 0;
            for (len, (child, _)) in self.0.iter().chain(incoming.0.iter()).enumerate() {
                ctx.charge_work(1, "append feature regeneration parent scan")?;
                longest = longest.max(child.as_str().len());
                ctx.admit_btree_node_storage::<crate::features::FeatureId, crate::features::FeatureId>(len, "append feature regeneration parents")?;
                ctx.charge_collection_items(1, "append feature regeneration parents")?;
                ctx.charge_work(1, "append feature regeneration parents")?;
            }
            let work = count
                .checked_add(1)
                .and_then(|count| {
                    longest
                        .checked_add(std::mem::size_of::<(
                            crate::features::FeatureId,
                            crate::features::FeatureId,
                        )>())
                        .and_then(|bytes| count.checked_mul(bytes))
                })
                .and_then(|work| work.checked_mul(4))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "append feature regeneration parent moves",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            ctx.charge_work(
                u64_from_index(work),
                "append feature regeneration parent moves",
            )?;
        }
        Ok(())
    }

    pub(crate) fn try_clone_for_decode(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let _depth = ctx.enter_nested(operation)?;
        ctx.charge_work(1, operation)?;
        let mut parents = BTreeMap::new();
        for (child, parent) in &self.0 {
            let work = u64_from_index(child.as_str().len())
                .checked_add(1)
                .and_then(|length| {
                    length.checked_mul(u64_from_index(parents.len()).checked_add(1)?)
                })
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, operation)?;
            let child = child.try_clone_for_decode(ctx, operation)?;
            let parent = parent.try_clone_for_decode(ctx, operation)?;
            ctx.charge_work(1, operation)?;
            ctx.insert_btree_map(&mut parents, child, parent, operation)?;
        }
        Ok(Self(parents))
    }
}

macro_rules! arena_registry {
    ($macro:ident) => {
        $macro! {
            bodies: Body, "Body arena.", [];
            regions: Region, "Region arena.", [];
            shells: Shell, "Shell arena.", [];
            faces: Face, "Face arena.", [];
            loops: Loop, "Loop arena.", [];
            coedges: Coedge, "Coedge arena.", [];
            edges: Edge, "Edge arena.", [];
            vertices: Vertex, "Vertex arena.", [];
            points: Point, "Point arena.", [];
            surfaces: Surface, "Surface arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<Surface>"))];
            curves: Curve, "Curve arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<Curve>"))];
            subds: SubdSurface, "Subdivision surface arena.", [];
            pcurves: Pcurve, "Pcurve arena.", [];
            procedural_surfaces: ProceduralSurface, "Procedural surface arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<ProceduralSurface>"))];
            procedural_curves: ProceduralCurve, "Procedural curve arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<ProceduralCurve>"))];
            assets: crate::assets::Asset, "Embedded and externally referenced document resources.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            features: Feature, "Feature arena.", [], [cfg_attr(feature = "schema", schemars(with = "Vec<FeatureRowWire>"))];
            feature_input_topologies: FeatureInputTopology, "Feature input-topology arena.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            feature_result_topologies: FeatureResultTopology, "Feature result-topology arena.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            configurations: DesignConfiguration, "Design configuration arena.", [serde(default)];
            parameters: DesignParameter, "Design parameter arena.", [serde(default)];
            sketches: Sketch, "Planar sketch arena.", [serde(default)];
            sketch_entities: SketchEntity, "Solved sketch entity arena.", [serde(default)];
            sketch_constraints: SketchConstraint, "Sketch constraint arena.", [serde(default)];
            spatial_sketches: SpatialSketch, "Spatial sketch arena.", [serde(default)];
            spatial_sketch_entities: SpatialSketchEntity, "Solved spatial sketch entity arena.", [serde(default)];
            spatial_sketch_constraints: SpatialSketchConstraint, "Spatial sketch constraint arena.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            spreadsheets: Spreadsheet, "Spreadsheet arena.", [serde(default)];
            product_definitions: ProductDefinition, "Product definition arena.", [serde(default)];
            occurrences: Occurrence, "Product occurrence arena.", [serde(default)];
            assembly_joints: AssemblyJoint, "Assembly joint arena.", [serde(default)];
            drawings: Drawing, "Drawing page, resource, view, and annotation arena.", [serde(default)];
            semantic_annotations: SemanticAnnotation, "Semantic dimension, note, symbol, and callout arena.", [serde(default)];
            presentation_documents: PresentationDocument, "Document presentation arena.", [serde(default)];
            view_presentations: ViewPresentation, "View-provider presentation arena.", [serde(default)];
            tessellations: Tessellation, "Tessellation arena.", [];
            appearances: Appearance, "Appearance arena.", [];
            appearance_bindings: AppearanceBinding, "Appearance binding arena.", [];
            attributes: SourceAttribute, "Attribute arena.", [];
            pmi: crate::pmi::PmiAnnotation, "Product-manufacturing information arena.", [serde(default)];
            presentation_layers: crate::presentation::PresentationLayer, "Presentation layer arena.", [serde(default)];
        }
    };
}
pub(crate) use arena_registry;

struct SurfaceWire<'a>(&'a Surface);

impl Serialize for SurfaceWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Surface", 3)?;
        state.serialize_field("id", &self.0.id)?;
        match self.0.geometry.solved_cache() {
            Some(cache) => state.serialize_field("geometry", cache)?,
            None => state.serialize_field("geometry", &self.0.geometry)?,
        }
        if let Some(source_object) = &self.0.source_object {
            state.serialize_field("source_object", source_object)?;
        }
        state.end()
    }
}

struct CurveWire<'a>(&'a Curve);

impl Serialize for CurveWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Curve", 3)?;
        state.serialize_field("id", &self.0.id)?;
        match self.0.geometry.solved_cache() {
            Some(cache) => state.serialize_field("geometry", cache)?,
            None => state.serialize_field("geometry", &self.0.geometry)?,
        }
        if let Some(source_object) = &self.0.source_object {
            state.serialize_field("source_object", source_object)?;
        }
        state.end()
    }
}

struct ProceduralSurfaceWire<'a> {
    owner: Option<&'a SurfaceId>,
    procedural: &'a ProceduralSurface,
}

impl Serialize for ProceduralSurfaceWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let owner = self.owner.ok_or_else(|| {
            <S::Error as serde::ser::Error>::custom(format_args!(
                "procedural surface {} has no unique owning surface",
                self.procedural.id
            ))
        })?;
        ProceduralSurfaceRow::new(owner.clone(), self.procedural).serialize(serializer)
    }
}

struct ProceduralCurveWire<'a> {
    owner: Option<&'a CurveId>,
    procedural: &'a ProceduralCurve,
}

impl Serialize for ProceduralCurveWire<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let owner = self.owner.ok_or_else(|| {
            <S::Error as serde::ser::Error>::custom(format_args!(
                "procedural curve {} has no unique owning curve",
                self.procedural.id
            ))
        })?;
        ProceduralCurveRow::new(owner.clone(), self.procedural).serialize(serializer)
    }
}

macro_rules! model_write_type {
    (surfaces, $ty:ty, $l:lifetime) => { Vec<SurfaceWire<$l>> };
    (curves, $ty:ty, $l:lifetime) => { Vec<CurveWire<$l>> };
    (procedural_surfaces, $ty:ty, $l:lifetime) => { Vec<ProceduralSurfaceWire<$l>> };
    (procedural_curves, $ty:ty, $l:lifetime) => { Vec<ProceduralCurveWire<$l>> };
    (features, $ty:ty, $l:lifetime) => { Vec<FeatureWriteWire<$l>> };
    ($field:ident, $ty:ty, $l:lifetime) => { &$l Vec<$ty> };
}

macro_rules! model_write_value {
    ($model:expr, surfaces) => {
        $model.surfaces.iter().map(SurfaceWire).collect()
    };
    ($model:expr, curves) => {
        $model.curves.iter().map(CurveWire).collect()
    };
    ($model:expr, procedural_surfaces) => {{
        let owners = unadmitted_owner_index(&$model.surfaces, |surface| {
            surface
                .geometry
                .procedural_construction()
                .map(ProceduralSurfaceId::as_str)
        });
        $model
            .procedural_surfaces
            .iter()
            .map(|procedural| ProceduralSurfaceWire {
                owner: unadmitted_unique_owner(&owners, procedural.id.as_str())
                    .map(|surface| &surface.id),
                procedural,
            })
            .collect()
    }};
    ($model:expr, procedural_curves) => {{
        let owners = unadmitted_owner_index(&$model.curves, |curve| {
            curve
                .geometry
                .procedural_construction()
                .map(ProceduralCurveId::as_str)
        });
        $model
            .procedural_curves
            .iter()
            .map(|procedural| ProceduralCurveWire {
                owner: unadmitted_unique_owner(&owners, procedural.id.as_str())
                    .map(|curve| &curve.id),
                procedural,
            })
            .collect()
    }};
    ($model:expr, features) => {
        $model
            .features
            .iter()
            .map(|feature| {
                FeatureWriteWire::new(feature, $model.feature_regeneration_parent(&feature.id))
            })
            .collect()
    };
    ($model:expr, $field:ident) => {
        &$model.$field
    };
}

macro_rules! model_read_type {
    (procedural_surfaces, $ty:ty) => { Vec<ProceduralSurfaceRow> };
    (procedural_curves, $ty:ty) => { Vec<ProceduralCurveRow> };
    (features, $ty:ty) => { Vec<FeatureRowWire> };
    ($field:ident, $ty:ty) => { Vec<$ty> };
}

macro_rules! model_read_value {
    ($wire:expr, procedural_surfaces) => {
        Vec::new()
    };
    ($wire:expr, procedural_curves) => {
        Vec::new()
    };
    ($wire:expr, features) => {
        Vec::new()
    };
    ($wire:expr, $field:ident) => {
        std::mem::take(&mut $wire.$field)
    };
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureRegenerationEdge {
    child: crate::features::FeatureId,
    parent: crate::features::FeatureId,
}

macro_rules! sorted_model_type {
    (surfaces, $ty:ty, $l:lifetime) => { Vec<SurfaceWire<$l>> };
    (curves, $ty:ty, $l:lifetime) => { Vec<CurveWire<$l>> };
    (procedural_surfaces, $ty:ty, $l:lifetime) => { Vec<ProceduralSurfaceWire<$l>> };
    (procedural_curves, $ty:ty, $l:lifetime) => { Vec<ProceduralCurveWire<$l>> };
    (features, $ty:ty, $l:lifetime) => { Vec<FeatureWriteWire<$l>> };
    ($field:ident, $ty:ty, $l:lifetime) => { Vec<&$l $ty> };
}

macro_rules! sorted_model_value {
    ($model:expr, $ctx:expr, surfaces) => {
        sorted_rows($ctx, &$model.surfaces, |value| Ok(SurfaceWire(value)))?
    };
    ($model:expr, $ctx:expr, curves) => {
        sorted_rows($ctx, &$model.curves, |value| Ok(CurveWire(value)))?
    };
    ($model:expr, $ctx:expr, procedural_surfaces) => {{
        let owners = procedural_owner_index(
            $ctx,
            &$model.surfaces,
            |surface| {
                surface
                    .geometry
                    .procedural_construction()
                    .map(|id| id.as_str())
            },
            "index digest procedural owners",
        )?;
        sorted_rows($ctx, &$model.procedural_surfaces, |procedural| {
            Ok(ProceduralSurfaceWire {
                owner: unique_procedural_owner(
                    $ctx,
                    &owners,
                    procedural.id.as_str(),
                    "find digest procedural owner",
                )?
                .map(|surface| &surface.id),
                procedural,
            })
        })?
    }};
    ($model:expr, $ctx:expr, procedural_curves) => {{
        let owners = procedural_owner_index(
            $ctx,
            &$model.curves,
            |curve| {
                curve
                    .geometry
                    .procedural_construction()
                    .map(|id| id.as_str())
            },
            "index digest procedural owners",
        )?;
        sorted_rows($ctx, &$model.procedural_curves, |procedural| {
            Ok(ProceduralCurveWire {
                owner: unique_procedural_owner(
                    $ctx,
                    &owners,
                    procedural.id.as_str(),
                    "find digest procedural owner",
                )?
                .map(|curve| &curve.id),
                procedural,
            })
        })?
    }};
    ($model:expr, $ctx:expr, features) => {
        sorted_rows($ctx, &$model.features, |feature| {
            Ok(FeatureWriteWire::new(
                feature,
                $ctx.get_btree_map(
                    &$model.feature_regeneration_parents.0,
                    &feature.id,
                    "find digest feature parent",
                )?,
            ))
        })?
    };
    ($model:expr, $ctx:expr, $field:ident) => {
        sorted_rows($ctx, &$model.$field, Ok)?
    };
}

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct Model {
            $(
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
            pub(crate) feature_regeneration_parents: FeatureRegenerationParents,
        }

        #[cfg(feature = "schema")]
        impl JsonSchema for Model {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                "Model".into()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                concat!(module_path!(), "::Model").into()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                let mut schema = ModelReadWire::json_schema(generator);
                schema.ensure_object().remove("additionalProperties");
                schema.ensure_object().insert(
                    "description".into(),
                    "Format-neutral entity arenas connected by typed IDs.".into(),
                );
                schema
            }
        }

        #[derive(Serialize)]
        struct ModelWriteWire<'a> {
            $(
                $(#[$attribute])*
                $field: model_write_type!($field, $ty, 'a),
            )*
        }

        #[derive(Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[cfg_attr(feature = "schema", schemars(rename = "Model"))]
        #[serde(deny_unknown_fields)]
        struct ModelReadWire {
            $(
                $(#[$attribute])*
                $($(#[$schema_attr])*)?
                #[doc = $doc]
                $field: model_read_type!($field, $ty),
            )*
        }

        impl Serialize for Model {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                feature_parents::validate_reconstructed(&[self]).map_err(serde::ser::Error::custom)?;
                ModelWriteWire {
                    $($field: model_write_value!(self, $field),)*
                }
                .serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for Model {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let mut wire = ModelReadWire::deserialize(deserializer)?;
                let procedural_surfaces = std::mem::take(&mut wire.procedural_surfaces);
                let procedural_curves = std::mem::take(&mut wire.procedural_curves);
                let feature_wires = std::mem::take(&mut wire.features);
                let mut model = Self {
                    $($field: model_read_value!(wire, $field),)*
                    feature_regeneration_parents: FeatureRegenerationParents::default(),
                };
                for wire in feature_wires {
                    let (feature, parent) = wire.into_parts();
                    if let Some(parent) = parent {
                        model.feature_regeneration_parents.0.insert(feature.id.clone(), parent);
                    }
                    model.features.push(feature);
                }
                feature_parents::validate_reconstructed(&[&model]).map_err(serde::de::Error::custom)?;
                let surfaces = procedural_surfaces.into_iter().map(ProceduralSurfaceRow::into_parts).collect();
                for outcome in model
                    .add_procedural_surfaces(&crate::document::admission::StandardAdmission, surfaces)
                    .map_err(serde::de::Error::custom)?
                {
                    outcome.map_err(serde::de::Error::custom)?;
                }
                let curves = procedural_curves.into_iter().map(ProceduralCurveRow::into_parts).collect();
                for outcome in model
                    .add_procedural_curves(&crate::document::admission::StandardAdmission, curves)
                    .map_err(serde::de::Error::custom)?
                {
                    outcome.map_err(serde::de::Error::custom)?;
                }
                Ok(model)
            }
        }

        impl Model {
            /// Returns the identity at one canonical arena slot.
            pub(crate) fn identity_at(
                &self,
                kind: crate::schema::EntityKind,
                index: usize,
            ) -> Option<&str> {
                $(if kind == <$ty as crate::schema::EntitySchema>::KIND {
                    return self
                        .$field
                        .get(index)
                        .map(crate::schema::EntitySchema::identity);
                })*
                None
            }

            /// Total number of admitted neutral entities across all arenas.
            pub fn entity_count(&self) -> usize {
                0 $(+ self.$field.len())*
            }

            /// Moves every arena and feature-parent relation from another model.
            pub(crate) fn append(&mut self, mut other: Self) {
                $(self.$field.append(&mut other.$field);)*
                self.feature_regeneration_parents.0
                    .append(&mut other.feature_regeneration_parents.0);
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
                self.finalize_with(ctx)
            }

            fn finalize_with<A: Admission>(&mut self, admission: &A) -> Result<(), A::Error> {
                $(crate::ids::comparison::stable_sort_by_identity(
                    admission,
                    &mut self.$field,
                    crate::schema::EntitySchema::identity,
                    "finalize model arena",
                )?;)*
                Ok(())
            }

            /// Append every arena of `other` onto the matching arena of this
            /// model, passing each entity through `rewrite`.
            ///
            /// Derived from the same `arena_registry!` declaration as
            /// [`finalize`](Self::finalize), so a new arena is merged without
            /// editing any call site. One entity is handed to `rewrite` at a
            /// time, which bounds a rewriting caller's transient storage by the
            /// largest single entity rather than by the whole model.
            /// Append rewritten arenas after charging every destination entry.
            pub fn extend_rewritten<R: EntityRewrite>(
                &mut self, ctx: &DecodeContext<'_>, other: Self, rewrite: &mut R, operation: &'static str,
            ) -> Result<(), ModelRewriteError<R::Error>> {
                $(
                    ctx.reserve_capacity_limit(&mut self.$field, other.$field.len(), operation).map_err(|limit| ModelRewriteError::Resource(limit.into()))?;
                    for entity in other.$field {
                        ctx.charge_work_limit(1, operation).map_err(|limit| ModelRewriteError::Resource(limit.into()))?;
                        ctx.charge_collection_items_limit(1, operation).map_err(|limit| ModelRewriteError::Resource(limit.into()))?;
                        self.$field.push(rewrite.rewrite(entity).map_err(ModelRewriteError::Rewrite)?);
                    }
                )*
                for (child, parent) in other.feature_regeneration_parents.0 {
                    ctx.charge_work_limit(1, operation).map_err(|limit| ModelRewriteError::Resource(limit.into()))?;
                    let edge = rewrite.rewrite(FeatureRegenerationEdge { child, parent }).map_err(ModelRewriteError::Rewrite)?;
                    ctx.insert_btree_map(&mut self.feature_regeneration_parents.0, edge.child, edge.parent, operation).map_err(ModelRewriteError::Resource)?;
                }
                Ok(())
            }
        }
    };
}

macro_rules! assert_entity_schemas {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        const _: fn() = || {
            fn assert_schema<T: crate::schema::EntitySchema>() {}
            $(assert_schema::<$ty>();)*
        };
    };
}

/// Refusal while rewriting or admitting model storage.
#[derive(Debug, thiserror::Error)]
pub enum ModelRewriteError<E> {
    /// The entity rewriter refused its input.
    #[error("{0}")]
    Rewrite(E),
    /// The destination storage exceeded its resource limit.
    #[error("resource refusal: {0:?}")]
    Resource(CodecError),
}

impl From<ModelRewriteError<CodecError>> for CodecError {
    fn from(error: ModelRewriteError<CodecError>) -> Self {
        match error {
            ModelRewriteError::Rewrite(error) => error,
            ModelRewriteError::Resource(error) => error,
        }
    }
}

/// Per-entity rewrite applied by [`Model::extend_rewritten`].
pub trait EntityRewrite {
    /// Failure raised while rewriting one entity.
    type Error;

    /// Rewrite one arena entity.
    fn rewrite<T: crate::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, Self::Error>;
}

macro_rules! declare_model_view {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Every model arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedModel<'a> {
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
            #[serde(skip)]
            _storage: cadmpeg_core::decode::ScopedReservation<'a>,
        }

        impl Model {
            /// Borrow every arena after admitting its order and temporary storage.
            pub(crate) fn sorted<'a>(&'a self, ctx: &'a DecodeContext<'_>) -> Result<SortedModel<'a>, CodecError> {
                if let Err(error) = feature_parents::validate(ctx, &[self])? {
                    return Err(CodecError::Malformed(ctx.format_retained(format_args!("{error}"), "digest feature parent diagnostic")?));
                }
                let mut storage = ctx.reserve_scoped(0, "sorted digest model")?;
                let value = storage.with_storage(|| Ok::<_, CodecError>(( $(sorted_model_value!(self, ctx, $field),)* )))?;
                let ($($field,)*) = value;
                Ok(SortedModel { $($field,)* _storage: storage })
            }
        }
    };
}

fn sorted_rows<'a, T: crate::schema::EntitySchema, U>(
    ctx: &DecodeContext<'_>,
    entities: &'a [T],
    mut project: impl FnMut(&'a T) -> Result<U, CodecError>,
) -> Result<Vec<U>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "digest arena order")?;
    let mut refs =
        storage.with_storage(|| ctx.collect_vec(entities.iter(), "digest arena order"))?;
    crate::ids::comparison::stable_sort_by_identity(
        ctx,
        &mut refs,
        |value| value.identity(),
        "sort digest arena",
    )?;
    let result = ctx.try_collect_vec(
        refs.into_iter().map(&mut project),
        "digest arena projection",
    );
    drop(storage);
    result
}

/// The carriers naming each procedural construction, for the context-free
/// writer: the first carrier and how many name it.
fn unadmitted_owner_index<'a, T>(
    carriers: &'a [T],
    construction: impl Fn(&'a T) -> Option<&'a str>,
) -> std::collections::HashMap<&'a str, (&'a T, usize)> {
    let mut owners = std::collections::HashMap::new();
    for carrier in carriers {
        if let Some(id) = construction(carrier) {
            owners.entry(id).or_insert((carrier, 0)).1 += 1;
        }
    }
    owners
}

/// The carrier that names `construction` when exactly one does, for the
/// context-free writer.
fn unadmitted_unique_owner<'a, T>(
    owners: &std::collections::HashMap<&str, (&'a T, usize)>,
    construction: &str,
) -> Option<&'a T> {
    match owners.get(construction) {
        Some((owner, 1)) => Some(*owner),
        _ => None,
    }
}

/// Carriers that name a procedural construction, ordered by construction.
fn procedural_owner_index<'a, T>(
    ctx: &DecodeContext<'_>,
    carriers: &'a [T],
    construction: impl Fn(&'a T) -> Option<&'a str>,
    operation: &'static str,
) -> Result<Vec<(&'a str, &'a T)>, CodecError> {
    let mut owners = Vec::new();
    for carrier in ctx.admit_iter(carriers, operation)? {
        if let Some(id) = construction(carrier) {
            ctx.push_vec(&mut owners, (id, carrier), operation)?;
        }
    }
    crate::ids::comparison::stable_sort_by_identity(ctx, &mut owners, |owner| owner.0, operation)?;
    Ok(owners)
}

/// The carrier that names `construction`, when exactly one does.
fn unique_procedural_owner<'a, T>(
    ctx: &DecodeContext<'_>,
    owners: &[(&str, &'a T)],
    construction: &str,
    operation: &'static str,
) -> Result<Option<&'a T>, CodecError> {
    let names = |index: usize| -> Result<bool, CodecError> {
        let Some((id, _)) = owners.get(index) else {
            return Ok(false);
        };
        ctx.charge_work(1, operation)?;
        Ok(crate::ids::comparison::equal(
            ctx,
            id,
            construction,
            operation,
        )?)
    };
    let first = ctx.partition_point(
        owners,
        |(id, _)| Ok(crate::ids::comparison::compare(ctx, id, construction, operation)?.is_lt()),
        operation,
    )?;
    if !names(first)? || names(first + 1)? {
        return Ok(None);
    }
    Ok(Some(owners[first].1))
}

macro_rules! declare_arena_name {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Name of a registered model arena.
        ///
        /// Values use the `arena_registry!` field names so a new arena is
        /// counted and diffed under its declared wire key.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct ArenaName(&'static str);

        impl ArenaName {
            /// Every registered arena, in registry order.
            pub const ALL: &'static [Self] = &[$(Self(stringify!($field))),*];

            pub(crate) const fn registered(name: &'static str) -> Self {
                Self(name)
            }

            /// Registry field name, which is also the CADIR JSON object key.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                self.0
            }

            /// Parse a registry field name.
            #[must_use]
            pub fn parse(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|arena| arena.as_str() == name)
            }
        }

        impl fmt::Display for ArenaName {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl AsRef<str> for ArenaName {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "6";

/// The current IR wire version. Every value writes [`IR_VERSION`].
/// Deserialization refuses any other version.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IrVersion;

arena_registry!(declare_model);
arena_registry!(assert_entity_schemas);
arena_registry!(declare_model_view);
arena_registry!(declare_arena_name);

/// Borrowed geometry and topology arenas for an embedded semantic record.
pub struct GeometrySnapshot<'a> {
    model: &'a Model,
    kind: &'a str,
    /// The unique owner of each procedural surface, in arena order.
    surface_owners: Vec<Option<&'a SurfaceId>>,
    /// The unique owner of each procedural curve, in arena order.
    curve_owners: Vec<Option<&'a CurveId>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'a>,
}

impl Model {
    /// Admit parent validation and find each procedural construction's owner
    /// for a borrowed geometry serialization view.
    pub fn geometry_snapshot<'a>(
        &'a self,
        ctx: &'a DecodeContext<'_>,
        kind: &'a str,
    ) -> Result<GeometrySnapshot<'a>, CodecError> {
        if let Err(error) = feature_parents::validate(ctx, &[self])? {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("{error}"),
                "geometry snapshot parent diagnostic",
            )?));
        }
        let operation = "find geometry snapshot procedural owner";
        let mut storage = ctx.reserve_scoped(0, operation)?;
        let surface_owners = {
            let mut index_storage = ctx.reserve_scoped(0, operation)?;
            let owners = index_storage.with_storage(|| {
                procedural_owner_index(
                    ctx,
                    &self.surfaces,
                    |surface| {
                        surface
                            .geometry
                            .procedural_construction()
                            .map(ProceduralSurfaceId::as_str)
                    },
                    operation,
                )
            })?;
            storage.with_storage(|| {
                ctx.try_collect_vec(
                    self.procedural_surfaces.iter().map(|procedural| {
                        Ok::<_, CodecError>(
                            unique_procedural_owner(
                                ctx,
                                &owners,
                                procedural.id.as_str(),
                                operation,
                            )?
                            .map(|surface| &surface.id),
                        )
                    }),
                    operation,
                )
            })?
        };
        let curve_owners = {
            let mut index_storage = ctx.reserve_scoped(0, operation)?;
            let owners = index_storage.with_storage(|| {
                procedural_owner_index(
                    ctx,
                    &self.curves,
                    |curve| {
                        curve
                            .geometry
                            .procedural_construction()
                            .map(ProceduralCurveId::as_str)
                    },
                    operation,
                )
            })?;
            storage.with_storage(|| {
                ctx.try_collect_vec(
                    self.procedural_curves.iter().map(|procedural| {
                        Ok::<_, CodecError>(
                            unique_procedural_owner(
                                ctx,
                                &owners,
                                procedural.id.as_str(),
                                operation,
                            )?
                            .map(|curve| &curve.id),
                        )
                    }),
                    operation,
                )
            })?
        };
        Ok(GeometrySnapshot {
            model: self,
            kind,
            surface_owners,
            curve_owners,
            _storage: storage,
        })
    }
}

struct SurfaceRows<'a>(&'a [Surface]);

impl Serialize for SurfaceRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for surface in self.0 {
            sequence.serialize_element(&SurfaceWire(surface))?;
        }
        sequence.end()
    }
}

struct CurveRows<'a>(&'a [Curve]);

impl Serialize for CurveRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for curve in self.0 {
            sequence.serialize_element(&CurveWire(curve))?;
        }
        sequence.end()
    }
}

struct ProceduralSurfaceRows<'a>(&'a [ProceduralSurface], &'a [Option<&'a SurfaceId>]);

impl Serialize for ProceduralSurfaceRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Row<'a> {
            owner: &'a SurfaceId,
            procedural: &'a ProceduralSurface,
        }
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut state = serializer.serialize_struct("ProceduralSurfaceRow", 4)?;
                state.serialize_field("id", &self.procedural.id)?;
                state.serialize_field("surface", self.owner)?;
                state.serialize_field("definition", self.procedural.definition())?;
                if let Some(bounds) = self.procedural.record_bounds() {
                    state.serialize_field("record_bounds", &bounds)?;
                }
                state.end()
            }
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for (procedural, owner) in self.0.iter().zip(self.1) {
            let owner = owner.ok_or_else(|| {
                serde::ser::Error::custom(format_args!(
                    "procedural surface {} has no unique owning surface",
                    procedural.id
                ))
            })?;
            sequence.serialize_element(&Row { owner, procedural })?;
        }
        sequence.end()
    }
}

struct ProceduralCurveRows<'a>(&'a [ProceduralCurve], &'a [Option<&'a CurveId>]);

impl Serialize for ProceduralCurveRows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Row<'a> {
            owner: &'a CurveId,
            procedural: &'a ProceduralCurve,
        }
        impl Serialize for Row<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut state = serializer.serialize_struct("ProceduralCurveRow", 3)?;
                state.serialize_field("id", &self.procedural.id)?;
                state.serialize_field("curve", self.owner)?;
                state.serialize_field("definition", self.procedural.definition())?;
                state.end()
            }
        }
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for (procedural, owner) in self.0.iter().zip(self.1) {
            let owner = owner.ok_or_else(|| {
                serde::ser::Error::custom(format_args!(
                    "procedural curve {} has no unique owning curve",
                    procedural.id
                ))
            })?;
            sequence.serialize_element(&Row { owner, procedural })?;
        }
        sequence.end()
    }
}

impl Serialize for GeometrySnapshot<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let model = self.model;
        let mut map = serializer.serialize_map(Some(16))?;
        map.serialize_entry("bodies", &model.bodies)?;
        map.serialize_entry("coedges", &model.coedges)?;
        map.serialize_entry("curves", &CurveRows(&model.curves))?;
        map.serialize_entry("edges", &model.edges)?;
        map.serialize_entry("faces", &model.faces)?;
        map.serialize_entry("kind", self.kind)?;
        map.serialize_entry("loops", &model.loops)?;
        map.serialize_entry("pcurves", &model.pcurves)?;
        map.serialize_entry("points", &model.points)?;
        map.serialize_entry(
            "procedural_curves",
            &ProceduralCurveRows(&model.procedural_curves, &self.curve_owners),
        )?;
        map.serialize_entry(
            "procedural_surfaces",
            &ProceduralSurfaceRows(&model.procedural_surfaces, &self.surface_owners),
        )?;
        map.serialize_entry("regions", &model.regions)?;
        map.serialize_entry("shells", &model.shells)?;
        map.serialize_entry("surfaces", &SurfaceRows(&model.surfaces))?;
        map.serialize_entry("tessellations", &model.tessellations)?;
        map.serialize_entry("vertices", &model.vertices)?;
        map.end()
    }
}

const SURFACES_UNKNOWN_GEOMETRY: &str = "surfaces_unknown_geometry";

/// A document census key: a registered model arena, the unknown-surface
/// tally, a native arena row, or a target-record kind.
///
/// Construction from a wire string classifies registered arena names and
/// `surfaces_unknown_geometry` so those rows cannot be stored as an untyped
/// other key. Remaining strings are native arena rows (`native.{format}.{name}`)
/// and export target-record kinds.
#[derive(Debug, Clone)]
pub struct CensusKey(CensusKeyInner);

#[derive(Debug, Clone)]
enum CensusKeyInner {
    Model(ArenaName),
    SurfacesUnknownGeometry,
    Other(String),
}

impl CensusKey {
    /// Key for a registered model arena.
    #[must_use]
    pub const fn model(name: ArenaName) -> Self {
        Self(CensusKeyInner::Model(name))
    }

    /// Key for the unknown-surface geometry tally.
    #[must_use]
    pub const fn surfaces_unknown_geometry() -> Self {
        Self(CensusKeyInner::SurfacesUnknownGeometry)
    }

    /// Key for a native namespace arena row, serialized as `native.{format}.{name}`.
    #[must_use]
    pub fn native(format: &str, name: &str) -> Self {
        Self(CensusKeyInner::Other(format!("native.{format}.{name}")))
    }

    /// Classify a wire key, routing registry names and the unknown-surface
    /// tally to their closed variants.
    #[must_use]
    pub fn from_wire(value: impl Into<String>) -> Self {
        let value = value.into();
        if let Some(name) = ArenaName::parse(&value) {
            return Self::model(name);
        }
        if value == SURFACES_UNKNOWN_GEOMETRY {
            return Self::surfaces_unknown_geometry();
        }
        Self(CensusKeyInner::Other(value))
    }

    /// Rebuild a count map, classifying each wire key.
    #[must_use]
    pub fn count_map<K, I>(counts: I) -> BTreeMap<Self, usize>
    where
        I: IntoIterator<Item = (K, usize)>,
        K: Into<String>,
    {
        counts
            .into_iter()
            .map(|(key, count)| (Self::from_wire(key), count))
            .collect()
    }

    /// Wire spelling of this key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            CensusKeyInner::Model(name) => name.as_str(),
            CensusKeyInner::SurfacesUnknownGeometry => SURFACES_UNKNOWN_GEOMETRY,
            CensusKeyInner::Other(value) => value,
        }
    }
}

impl cadmpeg_core::decode::cost::DecodeCost for CensusKey {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(self.as_str(), ctx, operation)
    }
}

impl From<ArenaName> for CensusKey {
    fn from(name: ArenaName) -> Self {
        Self::model(name)
    }
}

impl fmt::Display for CensusKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for CensusKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for CensusKey {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for CensusKey {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CensusKey {}

impl PartialOrd for CensusKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CensusKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for CensusKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Serialize for CensusKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CensusKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_wire(String::deserialize(deserializer)?))
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for CensusKey {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CensusKey".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}

#[derive(Debug, thiserror::Error)]
enum RegenerationParentError<'a> {
    #[error("tree child `{0}` already has a structural parent")]
    TreeChild(&'a crate::features::FeatureId),
    #[error("missing child feature `{0}`")]
    MissingChild(&'a crate::features::FeatureId),
    #[error("missing parent feature `{0}`")]
    MissingParent(&'a crate::features::FeatureId),
    #[error("parent feature `{parent}` does not precede child `{child}`")]
    NotPreceding {
        child: &'a crate::features::FeatureId,
        parent: &'a crate::features::FeatureId,
    },
}

impl Model {
    /// Structural tree owner of `child`, derived from tree-node child lists.
    pub fn feature_tree_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.features.iter().find_map(|candidate| {
            let crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::TreeNode { children, .. },
            ) = candidate.evaluation.definition()
            else {
                return None;
            };
            children.contains(child).then_some(&candidate.id)
        })
    }

    pub(crate) fn has_feature_regeneration_parents(&self) -> bool {
        !self.feature_regeneration_parents.0.is_empty()
    }

    /// Regeneration predecessor of a feature that no tree node owns.
    pub fn feature_regeneration_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.feature_regeneration_parents.0.get(child)
    }

    /// Structural owner, or the regeneration predecessor when no tree owns it.
    pub fn feature_parent(
        &self,
        child: &crate::features::FeatureId,
    ) -> Option<&crate::features::FeatureId> {
        self.feature_tree_parent(child)
            .or_else(|| self.feature_regeneration_parents.0.get(child))
    }

    fn validate_regeneration_parent<'a>(
        &self,
        child: &'a crate::features::FeatureId,
        parent: &'a crate::features::FeatureId,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<(), RegenerationParentError<'a>>, CodecError> {
        const OPERATION: &str = "install decoded feature regeneration parent";
        let admission = ctx;
        for candidate in &self.features {
            ctx.charge_work(1, OPERATION)?;
            if let crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::TreeNode { children, .. },
            ) = candidate.evaluation.definition()
            {
                for member in children {
                    ctx.charge_work(1, OPERATION)?;
                    if admission.equal(member.as_str(), child.as_str(), OPERATION)? {
                        return Ok(Err(RegenerationParentError::TreeChild(child)));
                    }
                }
            }
        }
        let mut child_ordinal = None;
        for feature in &self.features {
            ctx.charge_work(1, OPERATION)?;
            if admission.equal(feature.id.as_str(), child.as_str(), OPERATION)? {
                child_ordinal = Some(feature.ordinal);
                break;
            }
        }
        let Some(child_ordinal) = child_ordinal else {
            return Ok(Err(RegenerationParentError::MissingChild(child)));
        };
        let mut parent_ordinal = None;
        for feature in &self.features {
            ctx.charge_work(1, OPERATION)?;
            if admission.equal(feature.id.as_str(), parent.as_str(), OPERATION)? {
                parent_ordinal = Some(feature.ordinal);
                break;
            }
        }
        let Some(parent_ordinal) = parent_ordinal else {
            return Ok(Err(RegenerationParentError::MissingParent(parent)));
        };
        if parent_ordinal >= child_ordinal {
            return Ok(Err(RegenerationParentError::NotPreceding { child, parent }));
        }
        Ok(Ok(()))
    }

    /// Set a decoded regeneration predecessor with charged text and map admission.
    pub fn set_feature_regeneration_parent(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        child: &crate::features::FeatureId,
        parent: &crate::features::FeatureId,
    ) -> Result<(), cadmpeg_core::CodecError> {
        const OPERATION: &str = "install decoded feature regeneration parent";
        if let Err(error) = self.validate_regeneration_parent(child, parent, ctx)? {
            return Err(cadmpeg_core::CodecError::malformed(
                ctx.format_retained(format_args!("{error}"), OPERATION)?,
            ));
        }
        let work = self
            .feature_regeneration_parents
            .0
            .len()
            .checked_add(1)
            .and_then(|count| {
                child
                    .as_str()
                    .len()
                    .checked_add(1)
                    .and_then(|bytes| count.checked_mul(bytes))
            })
            .and_then(|work| work.checked_mul(3))
            .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        ctx.charge_work(u64_from_index(work), OPERATION)?;
        let parent = parent.try_clone_for_decode(ctx, OPERATION)?;
        if let Some(existing) = self.feature_regeneration_parents.0.get_mut(child) {
            *existing = parent;
        } else {
            let child = child.try_clone_for_decode(ctx, OPERATION)?;
            ctx.charge_work(1, OPERATION)?;
            ctx.insert_btree_map(
                &mut self.feature_regeneration_parents.0,
                child,
                parent,
                OPERATION,
            )?;
        }
        Ok(())
    }
}

/// Failure to attach a procedural construction to its sole carrier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProceduralCarrierError {
    message: String,
}

impl ProceduralCarrierError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Model {
    /// Returns the unique surface that carries `construction`.
    #[must_use]
    pub fn procedural_surface_owner(
        &self,
        construction: &ProceduralSurfaceId,
    ) -> Option<&SurfaceId> {
        let mut owners = self
            .surfaces
            .iter()
            .filter(|surface| surface.geometry.procedural_construction() == Some(construction));
        let owner = &owners.next()?.id;
        owners.next().is_none().then_some(owner)
    }

    /// Returns the unique curve that carries `construction`.
    #[must_use]
    pub fn procedural_curve_owner(&self, construction: &ProceduralCurveId) -> Option<&CurveId> {
        let mut owners = self
            .curves
            .iter()
            .filter(|curve| curve.geometry.procedural_construction() == Some(construction));
        let owner = &owners.next()?.id;
        owners.next().is_none().then_some(owner)
    }

    /// Attach a procedural surface through typed construction admission.
    ///
    /// One attachment scans the surface arenas; attach many through
    /// [`Self::add_procedural_surfaces`], which indexes them once.
    pub fn add_procedural_surface<A: ModelAdmission>(
        &mut self,
        admission: &A,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<Result<(), ProceduralCarrierError>, A::Error> {
        procedural::attach_scanning::<procedural::SurfaceKind, A>(
            self, admission, owner, procedural,
        )
    }

    /// Attach procedural surfaces in order, deciding each exactly as
    /// [`Self::add_procedural_surface`] would, and return each outcome.
    pub fn add_procedural_surfaces<A: ModelAdmission, O: std::borrow::Borrow<SurfaceId>>(
        &mut self,
        admission: &A,
        attachments: Vec<(O, ProceduralSurface)>,
    ) -> Result<Vec<Result<(), ProceduralCarrierError>>, A::Error> {
        procedural::attach_indexed::<procedural::SurfaceKind, A, O>(self, admission, attachments)
    }

    /// Attach a procedural curve through typed construction admission.
    ///
    /// One attachment scans the curve arenas; attach many through
    /// [`Self::add_procedural_curves`], which indexes them once.
    pub fn add_procedural_curve<A: ModelAdmission>(
        &mut self,
        admission: &A,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<Result<(), ProceduralCarrierError>, A::Error> {
        procedural::attach_scanning::<procedural::CurveKind, A>(self, admission, owner, procedural)
    }

    /// Attach procedural curves in order, deciding each exactly as
    /// [`Self::add_procedural_curve`] would, and return each outcome.
    pub fn add_procedural_curves<A: ModelAdmission, O: std::borrow::Borrow<CurveId>>(
        &mut self,
        admission: &A,
        attachments: Vec<(O, ProceduralCurve)>,
    ) -> Result<Vec<Result<(), ProceduralCarrierError>>, A::Error> {
        procedural::attach_indexed::<procedural::CurveKind, A, O>(self, admission, attachments)
    }
}

/// Accept only the supported `ir_version`.
///
/// The version stays a [`serde_json::Value`] so a missing member and a
/// non-string one are both reported as an unsupported version rather than as a
/// missing-field or type error. This is the single version gate: both
/// [`Deserialize`] for [`CadIr`] and [`CadIr::from_json`] call it.
fn check_ir_version<E: serde::de::Error>(version: Option<&serde_json::Value>) -> Result<(), E> {
    let version = version.and_then(serde_json::Value::as_str);
    if version != Some(IR_VERSION) {
        return Err(E::custom(format!(
            "unsupported ir_version {version:?}; expected {IR_VERSION}"
        )));
    }
    Ok(())
}

impl Serialize for IrVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(IR_VERSION)
    }
}

impl<'de> Deserialize<'de> for IrVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = serde_json::Value::deserialize(deserializer)?;
        check_ir_version(Some(&version))?;
        Ok(Self)
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for IrVersion {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "IrVersion".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ir_version_schema(generator)
    }
}

#[cfg(feature = "schema")]
fn ir_version_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "const": IR_VERSION
    })
}

/// A versioned CAD document.
///
/// Construction state machine: [`crate::draft::ModelDraft`] (mutable, indexed)
/// commits into [`CadIr`] (structurally canonical after [`CadIr::finalize`]);
/// [`crate::report::check::ValidationReport`] is produced separately by `validate_neutral` and
/// is not embedded in the document.
///
/// `model` holds the format-neutral graph. `native` retains typed
/// format-specific product data without changing that graph's semantics.
/// Entity IDs must be globally unique across all document arenas.
/// `ir_version` is a serialized constant checked by the read adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct CadIr {
    /// Source-container metadata.
    pub source: Option<SourceMeta>,
    /// Document-wide tolerances.
    pub tolerances: Tolerances,
    /// Format-neutral model.
    pub model: Model,
    /// Per-format native namespaces, each an arena map.
    pub native: Native,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CadIrWriteWire<'a> {
    ir_version: IrVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    model: &'a Model,
    native: &'a Native,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CadIrReadWire {
    #[serde(
        default,
        rename = "ir_version",
        deserialize_with = "deserialize_ir_version"
    )]
    ir_version: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_source")]
    source: Option<SourceMeta>,
    #[serde(default, rename = "units")]
    _units: CanonicalUnitsWire,
    tolerances: Tolerances,
    model: Model,
    #[serde(default)]
    native: Native,
}

impl Serialize for CadIr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CadIrWriteWire {
            ir_version: IrVersion,
            source: self.source.as_ref(),
            units: CanonicalUnitsWire::default(),
            tolerances: &self.tolerances,
            model: &self.model,
            native: &self.native,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CadIr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CadIrReadWire::deserialize(deserializer)?;
        check_ir_version(wire.ir_version.as_ref())?;
        Ok(wire.into())
    }
}

impl From<CadIrReadWire> for CadIr {
    fn from(wire: CadIrReadWire) -> Self {
        Self {
            source: wire.source,
            tolerances: wire.tolerances,
            model: wire.model,
            native: wire.native,
        }
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for CadIr {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CadIr".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::CadIr").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        CadIrWriteWire::json_schema(generator)
    }
}

fn admit_append_key(
    ctx: &DecodeContext<'_>,
    entries: usize,
    bytes: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let work = entries
        .checked_add(1)
        .and_then(|count| {
            bytes
                .checked_add(1)
                .and_then(|bytes| count.checked_mul(bytes))
        })
        .and_then(|work| work.checked_mul(4))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(u64_from_index(work), operation)
}

impl CadIr {
    /// Appends staged neutral and native records, then admits the combined document.
    ///
    /// The admission callback can only read the document. On `Err`, every arena
    /// and feature-parent relation returns to its preceding state. Existing
    /// geometry and native payloads are not cloned. Source metadata and document
    /// tolerances are unchanged.
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::{CadIr, document::Model, native::Native};
    /// let mut ir = CadIr::empty();
    /// let ctx = cadmpeg_test_support::service_decode_context();
    /// ir.try_append(&ctx, Model::default(), Native::default(), |candidate| {
    ///     candidate.model.points.clear();
    ///     Ok(Ok::<(), ()>(()))
    /// });
    /// ```
    pub fn try_append<T, E>(
        &mut self,
        ctx: &DecodeContext<'_>,
        model: Model,
        native: Native,
        admit: impl FnOnce(&Self) -> Result<Result<T, E>, CodecError>,
    ) -> Result<Result<T, E>, CodecError> {
        let native_lengths = if native.0.is_empty() {
            None
        } else {
            Some(ctx.with_scoped_storage("append native checkpoint", || {
                let mut lengths = BTreeMap::new();
                for (format, namespace) in &self.native.0 {
                    ctx.charge_work(1, "append native checkpoint scan")?;
                    let mut arenas = BTreeMap::new();
                    for (arena, records) in namespace.arenas() {
                        admit_append_key(
                            ctx,
                            arenas.len(),
                            arena.len(),
                            "append native checkpoint keys",
                        )?;
                        let key =
                            ctx.copy_retained_text(arena, "append native checkpoint arena")?;
                        ctx.insert_btree_map(
                            &mut arenas,
                            key,
                            records.len(),
                            "append native checkpoint arenas",
                        )?;
                    }
                    admit_append_key(
                        ctx,
                        lengths.len(),
                        format.len(),
                        "append native checkpoint keys",
                    )?;
                    let key =
                        ctx.copy_retained_text(format, "append native checkpoint namespace")?;
                    ctx.insert_btree_map(
                        &mut lengths,
                        key,
                        arenas,
                        "append native checkpoint namespaces",
                    )?;
                }
                Ok::<_, CodecError>(lengths)
            })?)
        };
        let mut speculative_parents = if model.feature_regeneration_parents.0.is_empty() {
            None
        } else {
            Some(ctx.with_scoped_storage("append speculative parents", || {
                let parents = self
                    .model
                    .feature_regeneration_parents
                    .try_clone_for_decode(ctx, "append speculative parent copy")?;
                parents.reserve_append(&model.feature_regeneration_parents, ctx)?;
                Ok::<_, CodecError>(parents)
            })?)
        };
        macro_rules! reserve_arenas {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                if !model.$field.is_empty() {
                    ctx.reserve_vec(&mut self.model.$field, model.$field.len(), "append model arena slots")?;
                    let moved = model.$field.len().checked_mul(std::mem::size_of::<$ty>())
                        .ok_or_else(|| ctx.refuse_codec_limit("append model arena moves", u64::MAX - 1, u64::MAX))?;
                    ctx.charge_work(u64_from_index(moved), "append model arena moves")?;
                    ctx.charge_work(u64_from_index(model.$field.len()), "append model rollback admission")?;
                }
            )*};
        }
        arena_registry!(reserve_arenas);
        let namespace_bound = self
            .native
            .0
            .len()
            .checked_add(native.0.len())
            .ok_or_else(|| {
                ctx.refuse_codec_limit("append native namespace bound", u64::MAX - 1, u64::MAX)
            })?;
        let mut namespace_len = self.native.0.len();
        for (format, incoming) in &native.0 {
            admit_append_key(
                ctx,
                namespace_bound,
                format.len(),
                "append native namespace lookup",
            )?;
            if let Some(destination) = self.native.0.get_mut(format) {
                let arena_bound = destination
                    .arenas()
                    .len()
                    .checked_add(incoming.arenas().len())
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("append native arena bound", u64::MAX - 1, u64::MAX)
                    })?;
                let mut arena_len = destination.arenas().len();
                for (arena, records) in incoming.arenas() {
                    admit_append_key(ctx, arena_bound, arena.len(), "append native arena lookup")?;
                    if let Some(existing) = destination.arenas_mut().get_mut(arena) {
                        ctx.reserve_vec(existing, records.len(), "append native record slots")?;
                        let moved = records
                            .len()
                            .checked_mul(std::mem::size_of::<crate::native::NativeRecord>())
                            .ok_or_else(|| {
                                ctx.refuse_codec_limit(
                                    "append native record moves",
                                    u64::MAX - 1,
                                    u64::MAX,
                                )
                            })?;
                        ctx.charge_work(u64_from_index(moved), "append native record moves")?;
                    } else {
                        ctx.admit_btree_node_storage::<String, Vec<crate::native::NativeRecord>>(
                            arena_len,
                            "append native arena nodes",
                        )?;
                        ctx.charge_collection_items(1, "append native arena nodes")?;
                        ctx.charge_work(1, "append native arena nodes")?;
                        arena_len += 1;
                    }
                }
            } else {
                ctx.admit_btree_node_storage::<String, crate::native::NativeNamespace>(
                    namespace_len,
                    "append native namespace nodes",
                )?;
                ctx.charge_collection_items(1, "append native namespace nodes")?;
                ctx.charge_work(1, "append native namespace nodes")?;
                namespace_len += 1;
            }
        }
        if let Some((lengths, _)) = &native_lengths {
            // Admit the union walk before mutation so rollback can run after a refusal.
            for (format, namespace) in self.native.0.iter().chain(native.0.iter()) {
                ctx.charge_work(1, "append native rollback admission")?;
                admit_append_key(
                    ctx,
                    lengths.len(),
                    format.len(),
                    "append native rollback namespace lookup",
                )?;
                if let Some(arenas) = lengths.get(format) {
                    for (arena, records) in namespace.arenas() {
                        ctx.charge_work(1, "append native rollback admission")?;
                        admit_append_key(
                            ctx,
                            arenas.len(),
                            arena.len(),
                            "append native rollback arena lookup",
                        )?;
                        ctx.charge_work(
                            u64_from_index(records.len()),
                            "append native rollback record admission",
                        )?;
                    }
                }
            }
        }
        let original_parents = speculative_parents.as_mut().map(|(parents, _)| {
            std::mem::replace(
                &mut self.model.feature_regeneration_parents,
                std::mem::take(parents),
            )
        });
        macro_rules! append_and_admit {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {{
                $(let $field = self.model.$field.len();)*
                self.model.append(model);
                for (format, mut namespace) in native.0 {
                    match self.native.0.entry(format) {
                        std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(namespace); },
                        std::collections::btree_map::Entry::Occupied(mut entry) => {
                            let destination = entry.get_mut().arenas_mut();
                            for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
                                match destination.entry(arena) {
                                    std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(records); },
                                    std::collections::btree_map::Entry::Occupied(mut entry) => { entry.get_mut().append(&mut records); },
                                }
                            }
                        },
                    }
                }
                let result = match admit(self) {
                    Ok(Ok(value)) => {
                        match speculative_parents.map(|(_, storage)| storage.commit()).transpose() {
                            Ok(_) => Ok(Ok(value)),
                            Err(error) => Err(error),
                        }
                    },
                    other => other,
                };
                if !result.as_ref().is_ok_and(|inner| inner.is_ok()) {
                    $(self.model.$field.truncate($field);)*
                    if let Some(parents) = original_parents { self.model.feature_regeneration_parents = parents; }
                    if let Some((lengths, _)) = &native_lengths {
                        self.native.0.retain(|format, namespace| {
                            let Some(arenas) = lengths.get(format) else { return false; };
                            namespace.arenas_mut().retain(|arena, records| {
                                let Some(length) = arenas.get(arena) else { return false; };
                                records.truncate(*length);
                                true
                            });
                            true
                        });
                    }
                }
                result
            }};
        }
        arena_registry!(append_and_admit)
    }

    /// Deserialize the reserved `unknowns` arena for `format`.
    pub fn native_unknowns(
        &self,
        format: &str,
    ) -> Result<Vec<NativeUnknownRecord>, crate::native::NativeConvertError> {
        self.native_unknowns_iter(format).collect()
    }

    /// Deserialize the reserved `unknowns` arena for `format` one record at a
    /// time.
    ///
    /// Retained source populations reach hundreds of thousands of records, so a
    /// caller that rebuilds the arena — reducing it for hashing, say — should
    /// consume this rather than [`native_unknowns`](Self::native_unknowns) and
    /// convert each record before the next is read, which keeps the typed
    /// population and the rebuilt one from ever coexisting.
    pub fn native_unknowns_iter<'a>(
        &'a self,
        format: &str,
    ) -> impl Iterator<Item = Result<NativeUnknownRecord, crate::native::NativeConvertError>> + 'a
    {
        self.native
            .namespace(format)
            .into_iter()
            .flat_map(|namespace| namespace.arena_iter_as("unknowns"))
    }

    /// Replace the reserved `unknowns` arena for `format`.
    pub fn set_native_unknowns(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: &str,
        records: &[NativeUnknownRecord],
    ) -> Result<(), crate::native::NativeConvertError> {
        self.set_native_unknowns_from(ctx, format, records.iter())
    }

    /// Replace the reserved `unknowns` arena for `format` one record at a time.
    ///
    /// A caller that derives its product references from a larger population
    /// should derive them inside the iterator: each reference is serialized as
    /// it is produced, so the derived population is never resident alongside the
    /// arena built from it.
    /// Duplicate identities are refused before the document changes.
    ///
    /// Only the reserved product projection can enter through this route.
    ///
    /// ```compile_fail
    /// use cadmpeg_ir::CadIr;
    /// let ctx = cadmpeg_test_support::service_decode_context();
    /// let raw = serde_json::json!({"id": "test:native:unknown#0", "extra": true});
    /// CadIr::empty().set_native_unknowns_from(&ctx, "test", [raw]).unwrap();
    /// ```
    pub fn set_native_unknowns_from<T: Borrow<NativeUnknownRecord>, I: IntoIterator<Item = T>>(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: &str,
        records: I,
    ) -> Result<(), crate::native::NativeConvertError> {
        let records = ctx.with_scoped_storage("native unknown replacement", || {
            crate::native::arena_from(
                ctx,
                records.into_iter().map(|record| {
                    Ok::<_, crate::native::NativeConvertError>(UnknownProjection(record))
                }),
            )
        })?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(records.0.len()),
            "scan native unknown identities",
        )?;
        for pair in records.0.windows(2) {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(pair[0].id().len().min(pair[1].id().len())),
                "compare native unknown identities",
            )?;
            if pair[0].id() == pair[1].id() {
                return Err(crate::native::NativeConvertError::InvalidCollection(
                    ctx.format_retained(
                        format_args!("duplicate native unknown record {}", pair[0].id()),
                        "native unknown identity collision",
                    )?,
                ));
            }
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(format.len())
                .checked_mul(
                    cadmpeg_core::decode::u64_from_index(self.native.0.len())
                        .checked_add(1)
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "native unknown namespace lookup",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?,
                )
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "native unknown namespace lookup",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?,
            "native unknown namespace lookup",
        )?;
        let key = ctx.copy_retained_text("unknowns", "native unknown arena key")?;
        records.1.commit()?;
        if let Some(namespace) = self.native.0.get_mut(format) {
            ctx.insert_btree_map(
                namespace.arenas_mut(),
                key,
                records.0,
                "native unknown arena",
            )?;
        } else {
            let format = ctx.copy_retained_text(format, "native unknown namespace key")?;
            let mut namespace = crate::native::NativeNamespace::default();
            ctx.insert_btree_map(
                namespace.arenas_mut(),
                key,
                records.0,
                "native unknown arena",
            )?;
            ctx.insert_btree_map(
                &mut self.native.0,
                format,
                namespace,
                "native unknown namespace",
            )?;
        }
        Ok(())
    }

    /// Construct an empty document with default tolerances.
    ///
    /// Fixtures and in-progress assembly use this constructor. Decoders that
    /// have classified source metadata use [`Self::decoded`].
    pub fn empty() -> Self {
        Self {
            source: None,
            tolerances: Tolerances::default(),
            model: Model::default(),
            native: Native::default(),
        }
    }

    /// Construct a decoded document with classified source metadata.
    pub fn decoded(source: SourceMeta) -> Self {
        Self {
            source: Some(source),
            tolerances: Tolerances::default(),
            model: Model::default(),
            native: Native::default(),
        }
    }

    /// IR schema version emitted by serialization and accepted on deserialize.
    pub fn ir_version(&self) -> &str {
        IR_VERSION
    }

    /// Serialize a finalized, identity-sorted view as pretty JSON.
    ///
    /// Clones and [`finalize`](Self::finalize)s so callers need not pre-sort.
    ///
    /// The text is written through the same finite adapter the digest uses, so
    /// a non-finite float is refused rather than written as `null`.
    ///
    /// # Errors
    ///
    /// Refuses a document holding a non-finite float.
    pub fn to_canonical_json(&self) -> Result<String, CanonicalJsonError> {
        let mut canonical = self.clone();
        match canonical.finalize_with(&StandardAdmission) {
            Ok(()) => {}
            Err(never) => match never {},
        }
        crate::hash::finite_json::to_canonical_json_string(&canonical)
    }

    /// Parse JSON and reject any unsupported `ir_version`.
    ///
    /// Version is probed first (`ir_version` only) so the full document is
    /// materialized once after the version gate.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        /// Every member but `ir_version` is skipped.
        #[derive(Deserialize)]
        struct VersionProbe {
            ir_version: Option<serde_json::Value>,
        }

        let probe = serde_json::from_str::<VersionProbe>(s)?;
        check_ir_version(probe.ir_version.as_ref())?;
        serde_json::from_str::<CadIrReadWire>(s).map(Into::into)
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        self.finalize_with(ctx)
    }

    fn finalize_with<A: Admission>(&mut self, admission: &A) -> Result<(), A::Error> {
        self.model.finalize_with(admission)?;
        self.native.finalize_with(admission)
    }

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> BTreeMap<CensusKey, usize> {
        crate::index::public_result(census::count(
            &census::StandardStorage,
            crate::native::view::NativeView::new(self, None),
        ))
    }
}

/// Source-container metadata preserved for reporting.
///
/// Attribute keys ending in [`crate::compare::LOCAL_DIGEST_SUFFIX`] hold
/// machine-local digests over decoded content for the write-path edit oracle.
/// Not portable across platforms; not tolerance-aware. Digests over retained
/// source bytes must not use that suffix. See
/// [`crate::hash::document_local_sha256`] and
/// [`crate::compare::is_local_digest_attribute`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceMeta {
    /// Format identity: classified dialect layers, or a bare known format.
    identity: FormatIdentity<DialectLayers>,
    /// Format-specific attributes.
    #[serde(default)]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub attributes: BTreeMap<NonBlankString, String>,
}

impl SourceMeta {
    /// Constructs metadata with dialect layers whose primary format is authoritative.
    #[must_use]
    pub fn classified(
        dialects: DialectLayers,
        attributes: BTreeMap<NonBlankString, String>,
    ) -> Self {
        Self {
            identity: FormatIdentity::classified(dialects),
            attributes,
        }
    }

    pub(crate) fn normalized_digest_copy(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let identity = match &self.identity {
            FormatIdentity::Classified { dialects } => {
                FormatIdentity::classified(dialects.try_clone_for_decode(ctx, operation)?)
            }
            FormatIdentity::Unclassified { format } => {
                FormatIdentity::unclassified(ctx.copy_retained_text(format, operation)?)
            }
        };
        let mut attributes = BTreeMap::new();
        let mut longest = crate::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.len();
        for (key, value) in &self.attributes {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(key.as_str().len()),
                operation,
            )?;
            if key.as_str() == crate::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE {
                continue;
            }
            longest = longest.max(key.as_str().len());
            crate::hash::admit_digest_key(ctx, attributes.len(), longest, operation)?;
            let key = key.try_clone_for_decode(ctx, operation)?;
            let value = ctx.copy_retained_text(value, operation)?;
            ctx.insert_btree_map(&mut attributes, key, value, operation)?;
        }
        Ok(Self {
            identity,
            attributes,
        })
    }

    /// The complete source identity: format plus classified layers, if any.
    #[must_use]
    pub(crate) const fn classification(&self) -> &FormatIdentity<DialectLayers> {
        &self.identity
    }

    /// Registry format namespace of this source's primary layer.
    #[must_use]
    pub fn format(&self) -> &str {
        self.identity.format()
    }

    /// Returns every source dialect layer when the source was classified.
    #[must_use]
    pub const fn dialects(&self) -> Option<&DialectLayers> {
        self.identity.classified_payload()
    }

    /// Returns the primary source dialect match when the source was classified.
    ///
    /// The match contains the registry dialect id, source declarations,
    /// admission, and optional instance discriminator. Its format is also the
    /// source format returned by [`Self::format`].
    #[must_use]
    pub fn dialect(&self) -> Option<&DialectMatch> {
        self.dialects().map(DialectLayers::primary)
    }
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_ir_version, serde_json::Value, "ir_version");
cadmpeg_core::named_optional_field!(deserialize_source, SourceMeta, "source");

mod identity_rewrite;
