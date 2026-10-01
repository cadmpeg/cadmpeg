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

use cadmpeg_core::decode::DecodeContext;
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
    pcurve::Pcurve, Curve, CurveGeometry, ProceduralCurve, ProceduralCurveRow, ProceduralSurface,
    ProceduralSurfaceRow, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FeatureRegenerationParents(
    BTreeMap<crate::features::FeatureId, crate::features::FeatureId>,
);

impl FeatureRegenerationParents {
    /// Admit the nodes rebuilt when two nonempty parent tables are merged.
    pub(crate) fn reserve_append(
        &self,
        incoming: &Self,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if !self.0.is_empty() && !incoming.0.is_empty() {
            for _edge in self.0.iter().chain(incoming.0.iter()) {
                ctx.charge_collection_items(1, "append feature regeneration parents")?;
                ctx.charge_retained(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                        crate::features::FeatureId,
                        crate::features::FeatureId,
                    )>()),
                    "append feature regeneration parents",
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn try_clone_for_decode(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut parents = BTreeMap::new();
        for (child, parent) in &self.0 {
            ctx.charge_work(1, operation)?;
            ctx.insert_btree_map(
                &mut parents,
                child.try_clone_for_decode(ctx, operation)?,
                parent.try_clone_for_decode(ctx, operation)?,
                operation,
            )?;
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
    ($model:expr, procedural_surfaces) => {
        $model
            .procedural_surfaces
            .iter()
            .map(|procedural| ProceduralSurfaceWire {
                owner: $model.procedural_surface_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, procedural_curves) => {
        $model
            .procedural_curves
            .iter()
            .map(|procedural| ProceduralCurveWire {
                owner: $model.procedural_curve_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
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
    ($model:expr, surfaces) => {
        sorted_refs(&$model.surfaces)
            .into_iter()
            .map(SurfaceWire)
            .collect()
    };
    ($model:expr, curves) => {
        sorted_refs(&$model.curves)
            .into_iter()
            .map(CurveWire)
            .collect()
    };
    ($model:expr, procedural_surfaces) => {
        sorted_refs(&$model.procedural_surfaces)
            .into_iter()
            .map(|procedural| ProceduralSurfaceWire {
                owner: $model.procedural_surface_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, procedural_curves) => {
        sorted_refs(&$model.procedural_curves)
            .into_iter()
            .map(|procedural| ProceduralCurveWire {
                owner: $model.procedural_curve_owner(&procedural.id),
                procedural,
            })
            .collect()
    };
    ($model:expr, features) => {
        sorted_refs(&$model.features)
            .into_iter()
            .map(|feature| {
                FeatureWriteWire::new(feature, $model.feature_regeneration_parent(&feature.id))
            })
            .collect()
    };
    ($model:expr, $field:ident) => {
        sorted_refs(&$model.$field)
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
                validate_feature_parents(&[self]).map_err(serde::ser::Error::custom)?;
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
                validate_feature_parents(&[&model]).map_err(serde::de::Error::custom)?;
                for wire in procedural_surfaces {
                    let (owner, procedural) = wire.into_parts();
                    model
                        .add_procedural_surface(&owner, procedural)
                        .map_err(serde::de::Error::custom)?;
                }
                for wire in procedural_curves {
                    let (owner, procedural) = wire.into_parts();
                    model
                        .add_procedural_curve(&owner, procedural)
                        .map_err(serde::de::Error::custom)?;
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
                $(ctx.stable_sort_by(&mut self.$field, |left, right| {
                    crate::schema::EntitySchema::identity(left)
                        .cmp(crate::schema::EntitySchema::identity(right))
                }, |entity| crate::schema::EntitySchema::identity(entity).len(), "finalize model arena")?;)*
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
                    ctx.reserve_retained_capacity_limit(&mut self.$field, other.$field.len(), operation).map_err(|limit| ModelRewriteError::Resource(limit.into()))?;
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
    fn rewrite<T: crate::schema::rewrite::typed::RewriteIdentities>(&mut self, entity: T) -> Result<T, Self::Error>;
}

macro_rules! declare_model_view {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// Every model arena borrowed in canonical identity order.
        #[derive(Serialize)]
        #[serde(remote = "Self")]
        pub(crate) struct SortedModel<'a> {
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
            #[serde(skip)]
            owner: &'a Model,
        }

        impl Serialize for SortedModel<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                validate_feature_parents(&[self.owner]).map_err(serde::ser::Error::custom)?;
                Self::serialize(self, serializer)
            }
        }

        impl Model {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedModel<'_> {
                SortedModel {
                    $($field: sorted_model_value!(self, $field),)*
                    owner: self,
                }
            }
        }
    };
}

fn sorted_refs<T: crate::schema::EntitySchema>(entities: &[T]) -> Vec<&T> {
    let mut refs = entities.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.identity().cmp(right.identity()));
    refs
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
}

impl Model {
    /// Serializes the geometry and topology arenas without staging owned rows.
    pub fn geometry_snapshot<'a>(&'a self, kind: &'a str) -> GeometrySnapshot<'a> {
        GeometrySnapshot { model: self, kind }
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

struct ProceduralSurfaceRows<'a>(&'a Model);

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
        let mut sequence = serializer.serialize_seq(Some(self.0.procedural_surfaces.len()))?;
        for procedural in &self.0.procedural_surfaces {
            let owner = self
                .0
                .procedural_surface_owner(&procedural.id)
                .ok_or_else(|| {
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

struct ProceduralCurveRows<'a>(&'a Model);

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
        let mut sequence = serializer.serialize_seq(Some(self.0.procedural_curves.len()))?;
        for procedural in &self.0.procedural_curves {
            let owner = self
                .0
                .procedural_curve_owner(&procedural.id)
                .ok_or_else(|| {
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
        validate_feature_parents(&[model]).map_err(serde::ser::Error::custom)?;
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
        map.serialize_entry("procedural_curves", &ProceduralCurveRows(model))?;
        map.serialize_entry("procedural_surfaces", &ProceduralSurfaceRows(model))?;
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

/// A model-owned feature relation that cannot survive document admission.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct FeatureParentError {
    pub(crate) owner: crate::features::FeatureId,
    pub(crate) message: String,
}

/// Checks structural ownership and predecessor ordering across the supplied
/// models as one graph. Draft references may resolve in the destination model.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FeatureParentValidationError {
    #[error("{0}")]
    Invalid(FeatureParentError),
    #[error("resource refusal: {0:?}")]
    Resource(cadmpeg_core::decode::ResourceLimit),
    #[error("{0}")]
    Admission(String),
}

impl From<cadmpeg_core::CodecError> for FeatureParentValidationError {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        match error {
            cadmpeg_core::CodecError::ResourceLimit(limit) => Self::Resource(limit),
            error => Self::Admission(error.to_string()),
        }
    }
}

pub(crate) fn validate_feature_parents(
    models: &[&Model],
) -> Result<(), FeatureParentValidationError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    validate_feature_parents_for_decode(models, &ctx)?
        .map_err(FeatureParentValidationError::Invalid)
}

pub(crate) fn validate_feature_parents_for_decode(
    models: &[&Model],
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Result<(), FeatureParentError>, cadmpeg_core::CodecError> {
    use crate::features::{FeatureDefinition, FeatureOperation};
    use crate::index::{DecodeStorage, IndexStorage};
    let storage = DecodeStorage(ctx);
    let mut reservation = ctx.reserve_scoped(0, "feature parent validation storage")?;

    let mut features = std::collections::HashMap::new();
    for feature in models.iter().flat_map(|model| &model.features) {
        ctx.charge_work(1, "feature parent validation scan")?;
        reservation.with_storage_limit(|| {
            storage.entry(&mut features, &(&feature.id), "feature parent identities")
        })?;
        if features.insert(&feature.id, feature).is_some() {
            return Ok(Err(FeatureParentError {
                owner: feature
                    .id
                    .try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                message: ctx.format_retained(
                    format_args!("feature identity `{}` is repeated", feature.id),
                    "feature parent diagnostic",
                )?,
            }));
        }
    }
    let mut tree_parents = std::collections::HashMap::new();
    for parent in models.iter().flat_map(|model| &model.features) {
        let FeatureDefinition::Operation(FeatureOperation::TreeNode { children, .. }) =
            parent.evaluation.definition()
        else {
            continue;
        };
        for child in children {
            ctx.charge_work(1, "feature tree parent scan")?;
            reservation.with_storage_limit(|| {
                storage.entry(&mut tree_parents, &child, "feature tree parent entries")
            })?;
            if let Some(previous) = tree_parents.insert(child, &parent.id) {
                return Ok(Err(FeatureParentError {
                    owner: child.try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                    message: ctx.format_retained(
                        format_args!(
                            "feature `{child}` has two tree parents `{previous}` and `{}`",
                            parent.id
                        ),
                        "feature parent diagnostic",
                    )?,
                }));
            }
        }
    }
    let mut regeneration_parents = std::collections::HashMap::new();
    for (child, parent) in models
        .iter()
        .flat_map(|model| &model.feature_regeneration_parents.0)
    {
        ctx.charge_work(1, "feature regeneration parent scan")?;
        reservation.with_storage_limit(|| {
            storage.entry(
                &mut regeneration_parents,
                &child,
                "feature regeneration parent entries",
            )
        })?;
        if let Some(previous) = regeneration_parents.insert(child, parent) {
            return Ok(Err(FeatureParentError {
                owner: child.try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                message: ctx.format_retained(format_args!("feature `{child}` has two regeneration parent entries `{previous}` and `{parent}`"), "feature parent diagnostic")?,
            }));
        }
        let Some(child_feature) = features.get(child) else {
            return Ok(Err(FeatureParentError {
                owner: child.try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                message: ctx.format_retained(
                    format_args!("regeneration relation names missing child feature `{child}`"),
                    "feature parent diagnostic",
                )?,
            }));
        };
        if let Some(existing) = tree_parents.get(child) {
            return Ok(Err(FeatureParentError {
                owner: child.try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                message: ctx.format_retained(format_args!("tree child `{child}` is owned by `{existing}` and states no regeneration parent"), "feature parent diagnostic")?,
            }));
        }
        let Some(parent_feature) = features.get(parent) else {
            return Ok(Err(FeatureParentError {
                owner: child.try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                message: ctx.format_retained(
                    format_args!("feature `{child}` names missing regeneration parent `{parent}`"),
                    "feature parent diagnostic",
                )?,
            }));
        };
        if parent_feature.ordinal >= child_feature.ordinal {
            return Ok(Err(FeatureParentError {
                owner: child.try_clone_for_decode(ctx, "feature parent diagnostic owner")?,
                message: ctx.format_retained(
                    format_args!("regeneration parent `{parent}` does not precede child `{child}`"),
                    "feature parent diagnostic",
                )?,
            }));
        }
    }
    Ok(Ok(()))
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
        for candidate in &self.features {
            ctx.charge_work(1, OPERATION)?;
            if let crate::features::FeatureDefinition::Operation(
                crate::features::FeatureOperation::TreeNode { children, .. },
            ) = candidate.evaluation.definition()
            {
                for member in children {
                    ctx.charge_work(1, OPERATION)?;
                    if member == child {
                        return Ok(Err(RegenerationParentError::TreeChild(child)));
                    }
                }
            }
        }
        let mut child_ordinal = None;
        for feature in &self.features {
            ctx.charge_work(1, OPERATION)?;
            if feature.id == *child {
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
            if feature.id == *parent {
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

    /// Set a regeneration predecessor without asserting structural tree membership.
    pub fn set_feature_regeneration_parent(
        &mut self,
        child: crate::features::FeatureId,
        parent: crate::features::FeatureId,
    ) -> Result<(), FeatureRegenerationError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)
            .map_err(FeatureRegenerationError::Resource)?;
        let result = self
            .set_feature_regeneration_parent_for_decode(&ctx, &child, &parent)
            .map_err(FeatureRegenerationError::from);
        drop(child);
        drop(parent);
        result
    }

    /// Set a decoded regeneration predecessor with charged text and map admission.
    pub fn set_feature_regeneration_parent_for_decode(
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
        let parent = parent.try_clone_for_decode(ctx, OPERATION)?;
        if let Some(existing) = self.feature_regeneration_parents.0.get_mut(child) {
            *existing = parent;
        } else {
            let child = child.try_clone_for_decode(ctx, OPERATION)?;
            ctx.insert_btree_map(&mut self.feature_regeneration_parents.0, child, parent, OPERATION)?;
        }
        Ok(())
    }
}

/// Refusal while validating or storing a regeneration parent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeatureRegenerationError {
    /// The parent relation failed its model invariant.
    #[error("{0}")]
    Invalid(String),
    /// The operation exceeded its resource limit.
    #[error("resource refusal: {0:?}")]
    Resource(cadmpeg_core::decode::ResourceLimit),
}

impl From<CodecError> for FeatureRegenerationError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(error) => Self::Resource(error),
            CodecError::Malformed(message) => Self::Invalid(message),
            error => Self::Invalid(error.to_string()),
        }
    }
}

/// Refusal while attaching or admitting a procedural construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProceduralAttachmentError {
    /// The construction failed its carrier invariant.
    #[error(transparent)]
    Invalid(ProceduralCarrierError),
    /// The operation exceeded its resource limit.
    #[error("resource refusal: {0:?}")]
    Resource(cadmpeg_core::decode::ResourceLimit),
}

impl From<CodecError> for ProceduralAttachmentError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::ResourceLimit(error) => Self::Resource(error),
            CodecError::Malformed(message) => Self::Invalid(ProceduralCarrierError::new(message)),
            error => Self::Invalid(ProceduralCarrierError::new(error.to_string())),
        }
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

    /// Attaches one procedural surface construction to its carrier.
    // Attachment accepts the owner ID and its construction at the same ownership boundary.
    pub fn add_procedural_surface(
        &mut self,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<(), ProceduralAttachmentError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)
            .map_err(ProceduralAttachmentError::Resource)?;
        self.add_procedural_surface_for_decode(&ctx, owner, procedural)
            .map_err(ProceduralAttachmentError::from)?
            .map_err(ProceduralAttachmentError::Invalid)
    }

    /// Attach a procedural surface using the caller's retained-byte budget.
    pub fn add_procedural_surface_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        owner: &SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        for existing in &self.procedural_surfaces {
            ctx.charge_work(1, "scan procedural surface constructions")?;
            if existing.id == procedural.id {
                return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                    format_args!(
                        "procedural surface construction {} already exists",
                        procedural.id
                    ),
                    "procedural surface refusal",
                )?)));
            }
        }
        let mut owner_index = None;
        for (index, carrier) in self.surfaces.iter().enumerate() {
            ctx.charge_work(1, "scan procedural surface carriers")?;
            if &carrier.id == owner && owner_index.is_none() {
                owner_index = Some(index);
            }
            if &carrier.id != owner
                && carrier.geometry.procedural_construction() == Some(&procedural.id)
            {
                return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                    format_args!(
                        "procedural surface construction {} already owns surface {}",
                        procedural.id, carrier.id
                    ),
                    "procedural surface refusal",
                )?)));
            }
        }
        let Some(surface) = owner_index.and_then(|index| self.surfaces.get_mut(index)) else {
            return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                format_args!(
                    "procedural surface {} references missing surface {owner}",
                    procedural.id
                ),
                "procedural surface refusal",
            )?)));
        };
        match &surface.geometry {
            SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {
                if procedural.cache_fit_tolerance().is_some() {
                    return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                        format_args!(
                        "direct procedural surface {owner} cannot carry a solved-cache tolerance"
                    ),
                        "procedural surface refusal",
                    )?)));
                }
                ctx.reserve_retained_vec(
                    &mut self.procedural_surfaces,
                    1,
                    "store procedural surface constructions",
                )?;
            }
            SurfaceGeometry::Procedural { construction, .. } => {
                return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                    format_args!(
                    "surface {owner} is already owned by procedural construction {construction}"
                ),
                    "procedural surface refusal",
                )?)));
            }
            SurfaceGeometry::Solved(_) => {
                let construction = procedural
                    .id
                    .try_clone_for_decode(ctx, "procedural surface owner identity")?;
                ctx.reserve_retained_vec(
                    &mut self.procedural_surfaces,
                    1,
                    "store procedural surface constructions",
                )?;
                let previous = std::mem::replace(
                    &mut surface.geometry,
                    SurfaceGeometry::Procedural {
                        construction,
                        cache: None,
                    },
                );
                if let (
                    SurfaceGeometry::Solved(geometry),
                    SurfaceGeometry::Procedural { cache, .. },
                ) = (previous, &mut surface.geometry)
                {
                    *cache = Some(geometry);
                }
            }
        }
        self.procedural_surfaces.push(procedural);
        Ok(Ok(()))
    }

    /// Attaches one procedural curve construction to its carrier.
    // Attachment accepts the owner ID and its construction at the same ownership boundary.
    pub fn add_procedural_curve(
        &mut self,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<(), ProceduralAttachmentError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(&[], &arena, &policy)
            .map_err(ProceduralAttachmentError::Resource)?;
        self.add_procedural_curve_for_decode(&ctx, owner, procedural)
            .map_err(ProceduralAttachmentError::from)?
            .map_err(ProceduralAttachmentError::Invalid)
    }

    /// Attach a procedural curve using the caller's retained-byte budget.
    pub fn add_procedural_curve_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        owner: &CurveId,
        procedural: ProceduralCurve,
    ) -> Result<Result<(), ProceduralCarrierError>, CodecError> {
        for existing in &self.procedural_curves {
            ctx.charge_work(1, "scan procedural curve constructions")?;
            if existing.id == procedural.id {
                return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                    format_args!(
                        "procedural curve construction {} already exists",
                        procedural.id
                    ),
                    "procedural curve refusal",
                )?)));
            }
        }
        let mut owner_index = None;
        for (index, carrier) in self.curves.iter().enumerate() {
            ctx.charge_work(1, "scan procedural curve carriers")?;
            if &carrier.id == owner && owner_index.is_none() {
                owner_index = Some(index);
            }
            if &carrier.id != owner
                && carrier.geometry.procedural_construction() == Some(&procedural.id)
            {
                return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                    format_args!(
                        "procedural curve construction {} already owns curve {}",
                        procedural.id, carrier.id
                    ),
                    "procedural curve refusal",
                )?)));
            }
        }
        let Some(curve) = owner_index.and_then(|index| self.curves.get_mut(index)) else {
            return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                format_args!(
                    "procedural curve {} references missing curve {owner}",
                    procedural.id
                ),
                "procedural curve refusal",
            )?)));
        };
        match &mut curve.geometry {
            CurveGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {
                if procedural.cache_fit_tolerance().is_some() {
                    return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                        format_args!(
                            "direct procedural curve {owner} cannot carry a solved-cache tolerance"
                        ),
                        "procedural curve refusal",
                    )?)));
                }
                ctx.reserve_retained_vec(
                    &mut self.procedural_curves,
                    1,
                    "store procedural curve constructions",
                )?;
            }
            CurveGeometry::Procedural { construction, .. } => {
                return Ok(Err(ProceduralCarrierError::new(ctx.format_retained(
                    format_args!(
                        "curve {owner} is already owned by procedural construction {construction}"
                    ),
                    "procedural curve refusal",
                )?)));
            }
            CurveGeometry::Solved(_) => {
                let construction = procedural
                    .id
                    .try_clone_for_decode(ctx, "ir_procedural_curve_construction_id")?;
                ctx.reserve_retained_vec(
                    &mut self.procedural_curves,
                    1,
                    "store procedural curve constructions",
                )?;
                let previous = std::mem::replace(
                    &mut curve.geometry,
                    CurveGeometry::Procedural {
                        construction,
                        cache: None,
                    },
                );
                if let (CurveGeometry::Solved(geometry), CurveGeometry::Procedural { cache, .. }) =
                    (previous, &mut curve.geometry)
                {
                    *cache = Some(geometry);
                }
            }
        }
        self.procedural_curves.push(procedural);
        Ok(Ok(()))
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
    /// ir.try_append(Model::default(), Native::default(), |candidate| {
    ///     candidate.model.points.clear();
    ///     Ok::<(), ()>(())
    /// });
    /// ```
    pub fn try_append<T, E>(
        &mut self,
        model: Model,
        native: Native,
        admit: impl FnOnce(&Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let native_lengths = self
            .native
            .0
            .iter()
            .map(|(format, namespace)| {
                (
                    format.clone(),
                    namespace
                        .arenas()
                        .iter()
                        .map(|(arena, records)| (arena.clone(), records.len()))
                        .collect::<BTreeMap<_, _>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let parents = self.model.feature_regeneration_parents.clone();
        macro_rules! append_and_admit {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {{
                $(let $field = self.model.$field.len();)*
                self.model.append(model);
                for (format, mut namespace) in native.0 {
                    let destination = self.native.namespace_mut(format).arenas_mut();
                    for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
                        destination.entry(arena).or_default().append(&mut records);
                    }
                }
                let result = admit(self);
                if result.is_err() {
                    $(self.model.$field.truncate($field);)*
                    self.model.feature_regeneration_parents = parents;
                    self.native.0.retain(|format, namespace| {
                        let Some(lengths) = native_lengths.get(format) else {
                            return false;
                        };
                        namespace.arenas_mut().retain(|arena, records| {
                            let Some(length) = lengths.get(arena) else {
                                return false;
                            };
                            records.truncate(*length);
                            true
                        });
                        true
                    });
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
        format: &str,
        records: &[NativeUnknownRecord],
    ) -> Result<(), crate::native::NativeConvertError> {
        self.set_native_unknowns_from(format, records.iter())
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
    /// let raw = serde_json::json!({"id": "test:native:unknown#0", "extra": true});
    /// CadIr::empty().set_native_unknowns_from("test", [raw]).unwrap();
    /// ```
    pub fn set_native_unknowns_from<T: Borrow<NativeUnknownRecord>, I: IntoIterator<Item = T>>(
        &mut self,
        format: &str,
        records: I,
    ) -> Result<(), crate::native::NativeConvertError> {
        let mut records: Vec<_> = records
            .into_iter()
            .map(|record| {
                let record: &NativeUnknownRecord = record.borrow();
                crate::native::NativeRecord::from(record)
            })
            .collect();
        records.sort_by(|left, right| left.id().cmp(right.id()));
        if let Some(pair) = records.windows(2).find(|pair| pair[0].id() == pair[1].id()) {
            return Err(crate::native::NativeConvertError::InvalidCollection(
                format!("duplicate native unknown record {}", pair[0].id()),
            ));
        }
        self.native
            .namespace_mut(format)
            .arenas_mut()
            .insert("unknowns".into(), records);
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::default())?;
        canonical.finalize(&ctx)?;
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
        self.model.finalize(ctx)?;
        self.native.finalize(ctx)
    }

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> BTreeMap<CensusKey, usize> {
        entity_census(self)
    }
}

macro_rules! define_registered_entity_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?; )*) => {
        fn registered_entity_census(ir: &CadIr) -> BTreeMap<CensusKey, usize> {
            BTreeMap::from([
                $((CensusKey::model(ArenaName::registered(stringify!($field))), ir.model.$field.len())),*
            ])
        }
    };
}
arena_registry!(define_registered_entity_census);

/// Count the records represented by the IR arenas without running validation.
pub fn entity_census(ir: &CadIr) -> BTreeMap<CensusKey, usize> {
    let mut counts = registered_entity_census(ir);
    counts.insert(
        CensusKey::surfaces_unknown_geometry(),
        ir.model
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry,
                    crate::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                )
            })
            .count(),
    );
    for loss in ir.native.loss_counts() {
        counts.insert(
            CensusKey::native(&loss.format, &loss.kind),
            loss.count.get(),
        );
    }
    counts
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
