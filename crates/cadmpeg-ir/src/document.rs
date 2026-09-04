// SPDX-License-Identifier: Apache-2.0
//! Versioned document structure and canonical arena ordering.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{
    de::DeserializeOwned, ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer,
};

use cadmpeg_core::dialect::{DialectLayers, DialectMatch, FormatIdentity};

use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignConfigurationReadWire, DesignParameter, Feature,
    FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{
    Curve, CurveGeometry, Pcurve, ProceduralCurve, ProceduralCurveReadWire, ProceduralSurface,
    ProceduralSurfaceReadWire, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
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
            surfaces: Surface, "Surface arena.", [];
            curves: Curve, "Curve arena.", [];
            subds: SubdSurface, "Subdivision surface arena.", [];
            pcurves: Pcurve, "Pcurve arena.", [];
            procedural_surfaces: ProceduralSurface, "Procedural surface arena.", [];
            procedural_curves: ProceduralCurve, "Procedural curve arena.", [];
            assets: crate::assets::Asset, "Embedded and externally referenced document resources.", [serde(default, skip_serializing_if = "Vec::is_empty")];
            features: Feature, "Feature arena.", [];
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
        state.serialize_field("geometry", self.0.geometry.wire_geometry())?;
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
        state.serialize_field("geometry", self.0.geometry.wire_geometry())?;
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
        let mut state = serializer.serialize_struct("ProceduralSurface", 5)?;
        state.serialize_field("id", &self.procedural.id)?;
        state.serialize_field("surface", owner)?;
        state.serialize_field("definition", self.procedural.definition())?;
        if let Some(cache_fit_tolerance) = self.procedural.cache_fit_tolerance() {
            state.serialize_field("cache_fit_tolerance", &cache_fit_tolerance)?;
        }
        if let Some(record_bounds) = self.procedural.record_bounds {
            state.serialize_field("record_bounds", &record_bounds)?;
        }
        state.end()
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
        let mut state = serializer.serialize_struct("ProceduralCurve", 4)?;
        state.serialize_field("id", &self.procedural.id)?;
        state.serialize_field("curve", owner)?;
        state.serialize_field("definition", self.procedural.definition())?;
        if let Some(cache_fit_tolerance) = self.procedural.cache_fit_tolerance() {
            state.serialize_field("cache_fit_tolerance", &cache_fit_tolerance)?;
        }
        state.end()
    }
}

macro_rules! model_write_type {
    (surfaces, $ty:ty, $l:lifetime) => { Vec<SurfaceWire<$l>> };
    (curves, $ty:ty, $l:lifetime) => { Vec<CurveWire<$l>> };
    (procedural_surfaces, $ty:ty, $l:lifetime) => { Vec<ProceduralSurfaceWire<$l>> };
    (procedural_curves, $ty:ty, $l:lifetime) => { Vec<ProceduralCurveWire<$l>> };
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
    ($model:expr, $field:ident) => {
        &$model.$field
    };
}

macro_rules! model_read_type {
    (procedural_surfaces, $ty:ty) => { Vec<ProceduralSurfaceReadWire> };
    (procedural_curves, $ty:ty) => { Vec<ProceduralCurveReadWire> };
    (configurations, $ty:ty) => { Vec<DesignConfigurationReadWire> };
    ($field:ident, $ty:ty) => { Vec<$ty> };
}

macro_rules! model_read_value {
    ($wire:expr, procedural_surfaces) => {
        Vec::new()
    };
    ($wire:expr, procedural_curves) => {
        Vec::new()
    };
    ($wire:expr, configurations) => {
        Vec::new()
    };
    ($wire:expr, $field:ident) => {
        std::mem::take(&mut $wire.$field)
    };
}

#[derive(Serialize, Deserialize)]
struct SurfaceCacheRewrite {
    geometry: SurfaceGeometry,
}

#[derive(Serialize, Deserialize)]
struct CurveCacheRewrite {
    geometry: CurveGeometry,
}

fn rewrite_surface<R: EntityRewrite>(
    rewrite: &mut R,
    mut surface: Surface,
) -> Result<Surface, R::Error> {
    let cache = match &mut surface.geometry {
        SurfaceGeometry::Procedural { cache, .. } => cache.take(),
        _ => None,
    };
    let cache = match cache {
        Some(cache) => Some(
            rewrite
                .rewrite(SurfaceCacheRewrite {
                    geometry: cache.into_geometry(),
                })?
                .geometry,
        ),
        None => None,
    };
    let mut surface = rewrite.rewrite(surface)?;
    if let (Some(cache), SurfaceGeometry::Procedural { cache: slot, .. }) =
        (cache, &mut surface.geometry)
    {
        *slot = SolvedSurfaceGeometry::new(cache).ok();
    }
    Ok(surface)
}

fn rewrite_curve<R: EntityRewrite>(rewrite: &mut R, mut curve: Curve) -> Result<Curve, R::Error> {
    let cache = match &mut curve.geometry {
        CurveGeometry::Procedural { cache, .. } => cache.take(),
        _ => None,
    };
    let cache = match cache {
        Some(cache) => Some(
            rewrite
                .rewrite(CurveCacheRewrite {
                    geometry: cache.into_geometry(),
                })?
                .geometry,
        ),
        None => None,
    };
    let mut curve = rewrite.rewrite(curve)?;
    if let (Some(cache), CurveGeometry::Procedural { cache: slot, .. }) =
        (cache, &mut curve.geometry)
    {
        *slot = SolvedCurveGeometry::new(cache).ok();
    }
    Ok(curve)
}

macro_rules! model_rewrite_entity {
    ($rewrite:expr, surfaces, $entity:expr) => {
        rewrite_surface($rewrite, $entity)
    };
    ($rewrite:expr, curves, $entity:expr) => {
        rewrite_curve($rewrite, $entity)
    };
    ($rewrite:expr, $field:ident, $entity:expr) => {
        $rewrite.rewrite($entity)
    };
}

macro_rules! sorted_model_type {
    (surfaces, $ty:ty, $l:lifetime) => { Vec<SurfaceWire<$l>> };
    (curves, $ty:ty, $l:lifetime) => { Vec<CurveWire<$l>> };
    (procedural_surfaces, $ty:ty, $l:lifetime) => { Vec<ProceduralSurfaceWire<$l>> };
    (procedural_curves, $ty:ty, $l:lifetime) => { Vec<ProceduralCurveWire<$l>> };
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
    ($model:expr, $field:ident) => {
        sorted_refs(&$model.$field)
    };
}

macro_rules! declare_model {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        /// Format-neutral entity arenas connected by typed IDs.
        #[derive(Debug, Clone, Default, PartialEq)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub struct Model {
            $(
                $(#[$attribute])*
                #[doc = $doc]
                pub $field: Vec<$ty>,
            )*
        }

        #[derive(Serialize)]
        struct ModelWriteWire<'a> {
            $(
                $(#[$attribute])*
                $field: model_write_type!($field, $ty, 'a),
            )*
        }

        #[derive(Deserialize)]
        struct ModelReadWire {
            $(
                $(#[$attribute])*
                $field: model_read_type!($field, $ty),
            )*
        }

        impl Serialize for Model {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
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
                let configurations = std::mem::take(&mut wire.configurations);
                let mut model = Self {
                    $($field: model_read_value!(wire, $field),)*
                };
                for wire in procedural_surfaces {
                    let (owner, procedural) = wire.into_parts().map_err(serde::de::Error::custom)?;
                    let owner = owner.ok_or_else(|| serde::de::Error::custom(
                        "procedural surface wire is missing surface",
                    ))?;
                    model
                        .add_procedural_surface(owner, procedural)
                        .map_err(serde::de::Error::custom)?;
                }
                for wire in procedural_curves {
                    let (owner, procedural) = wire.into_parts().map_err(serde::de::Error::custom)?;
                    let owner = owner.ok_or_else(|| serde::de::Error::custom(
                        "procedural curve wire is missing curve",
                    ))?;
                    model
                        .add_procedural_curve(owner, procedural)
                        .map_err(serde::de::Error::custom)?;
                }
                for wire in configurations {
                    model.configurations.push(
                        wire.into_configuration(&model.features)
                            .map_err(serde::de::Error::custom)?,
                    );
                }
                Ok(model)
            }
        }

        impl Model {
            /// Arena field names in canonical order.
            pub fn arena_names() -> &'static [&'static str] {
                &[$(stringify!($field)),*]
            }

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

            /// Visits every typed identity reference in canonical arena order.
            pub fn visit_references(
                &self,
                visitor: &mut dyn FnMut(crate::schema::Reference),
            ) {
                $(for entity in &self.$field {
                    crate::schema::EntitySchema::visit_references(entity, visitor);
                })*
            }

            /// Sort each arena lexicographically by its entity identity.
            pub fn finalize(&mut self) {
                $(self.$field.sort_by(|left, right| {
                    crate::schema::EntitySchema::identity(left)
                        .cmp(crate::schema::EntitySchema::identity(right))
                });)*
            }

            /// Append every arena of `other` onto the matching arena of this
            /// model, passing each entity through `rewrite`.
            ///
            /// Derived from the same `arena_registry!` declaration as
            /// [`finalize`](Self::finalize), so a new arena is merged without
            /// editing any call site. One entity is handed to `rewrite` at a
            /// time, which bounds a rewriting caller's transient storage by the
            /// largest single entity rather than by the whole model.
            pub fn extend_rewritten<R: EntityRewrite>(
                &mut self,
                other: Self,
                rewrite: &mut R,
            ) -> Result<(), R::Error> {
                $(
                    self.$field.reserve(other.$field.len());
                    for entity in other.$field {
                        self.$field.push(model_rewrite_entity!(rewrite, $field, entity)?);
                    }
                )*
                Ok(())
            }
        }
    };
}

macro_rules! assert_entity_schemas {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        const _: fn() = || {
            fn assert_schema<T: crate::schema::EntitySchema>() {}
            $(assert_schema::<$ty>();)*
        };
    };
}

/// Per-entity rewrite applied by [`Model::extend_rewritten`].
pub trait EntityRewrite {
    /// Failure raised while rewriting one entity.
    type Error;

    /// Rewrite one arena entity.
    fn rewrite<T: Serialize + DeserializeOwned>(&mut self, entity: T) -> Result<T, Self::Error>;
}

macro_rules! declare_model_view {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*];)*) => {
        /// Every model arena borrowed in canonical identity order.
        #[derive(Serialize)]
        pub(crate) struct SortedModel<'a> {
            $($(#[$attribute])* $field: sorted_model_type!($field, $ty, 'a),)*
        }

        impl Model {
            /// Borrow every arena in canonical identity order.
            pub(crate) fn sorted(&self) -> SortedModel<'_> {
                SortedModel {
                    $($field: sorted_model_value!(self, $field),)*
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

/// The IR schema version this build produces and accepts.
pub const IR_VERSION: &str = "5";

arena_registry!(declare_model);
arena_registry!(assert_entity_schemas);
arena_registry!(declare_model_view);

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
    pub fn add_procedural_surface(
        &mut self,
        owner: SurfaceId,
        procedural: ProceduralSurface,
    ) -> Result<(), ProceduralCarrierError> {
        if self
            .procedural_surfaces
            .iter()
            .any(|existing| existing.id == procedural.id)
        {
            return Err(ProceduralCarrierError::new(format!(
                "procedural surface construction {} already exists",
                procedural.id
            )));
        }
        if let Some(existing_owner) = self.surfaces.iter().find(|surface| {
            surface.id != owner
                && surface.geometry.procedural_construction() == Some(&procedural.id)
        }) {
            return Err(ProceduralCarrierError::new(format!(
                "procedural surface construction {} already owns surface {}",
                procedural.id, existing_owner.id
            )));
        }
        let surface = self
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == owner)
            .ok_or_else(|| {
                ProceduralCarrierError::new(format!(
                    "procedural surface {} references missing surface {owner}",
                    procedural.id
                ))
            })?;
        match &surface.geometry {
            SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {
                if procedural.cache_fit_tolerance().is_some() {
                    return Err(ProceduralCarrierError::new(format!(
                        "direct procedural surface {owner} cannot carry a solved-cache tolerance"
                    )));
                }
            }
            SurfaceGeometry::Procedural { construction, .. } => {
                return Err(ProceduralCarrierError::new(format!(
                    "surface {owner} is already owned by procedural construction {construction}"
                )));
            }
            geometry => {
                let cache = SolvedSurfaceGeometry::new(geometry.clone()).map_err(|_| {
                    ProceduralCarrierError::new(format!(
                        "surface {owner} already has a procedural construction"
                    ))
                })?;
                surface.geometry = SurfaceGeometry::Procedural {
                    construction: procedural.id.clone(),
                    cache: Some(cache),
                };
            }
        }
        self.procedural_surfaces.push(procedural);
        Ok(())
    }

    /// Attaches one procedural curve construction to its carrier.
    pub fn add_procedural_curve(
        &mut self,
        owner: CurveId,
        procedural: ProceduralCurve,
    ) -> Result<(), ProceduralCarrierError> {
        if self
            .procedural_curves
            .iter()
            .any(|existing| existing.id == procedural.id)
        {
            return Err(ProceduralCarrierError::new(format!(
                "procedural curve construction {} already exists",
                procedural.id
            )));
        }
        if let Some(existing_owner) = self.curves.iter().find(|curve| {
            curve.id != owner && curve.geometry.procedural_construction() == Some(&procedural.id)
        }) {
            return Err(ProceduralCarrierError::new(format!(
                "procedural curve construction {} already owns curve {}",
                procedural.id, existing_owner.id
            )));
        }
        let curve = self
            .curves
            .iter_mut()
            .find(|curve| curve.id == owner)
            .ok_or_else(|| {
                ProceduralCarrierError::new(format!(
                    "procedural curve {} references missing curve {owner}",
                    procedural.id
                ))
            })?;
        match &curve.geometry {
            CurveGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {
                if procedural.cache_fit_tolerance().is_some() {
                    return Err(ProceduralCarrierError::new(format!(
                        "direct procedural curve {owner} cannot carry a solved-cache tolerance"
                    )));
                }
            }
            CurveGeometry::Procedural { construction, .. } => {
                return Err(ProceduralCarrierError::new(format!(
                    "curve {owner} is already owned by procedural construction {construction}"
                )));
            }
            geometry => {
                let cache = SolvedCurveGeometry::new(geometry.clone()).map_err(|_| {
                    ProceduralCarrierError::new(format!(
                        "curve {owner} already has a procedural construction"
                    ))
                })?;
                curve.geometry = CurveGeometry::Procedural {
                    construction: procedural.id.clone(),
                    cache: Some(cache),
                };
            }
        }
        self.procedural_curves.push(procedural);
        Ok(())
    }
}

fn deserialize_ir_version<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    let version = String::deserialize(deserializer)?;
    if version != IR_VERSION {
        return Err(serde::de::Error::custom(format!(
            "unsupported ir_version {version:?}; expected {IR_VERSION}"
        )));
    }
    Ok(())
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
/// [`crate::ValidationReport`] is produced separately by `validate_neutral` and
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
    /// Independently versioned native namespaces.
    pub native: Native,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CadIrWriteWire<'a> {
    #[cfg_attr(feature = "schema", schemars(schema_with = "ir_version_schema"))]
    ir_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    model: &'a Model,
    native: &'a Native,
}

#[derive(Deserialize)]
struct CadIrReadWire {
    #[serde(rename = "ir_version", deserialize_with = "deserialize_ir_version")]
    _ir_version: (),
    #[serde(flatten)]
    payload: CadIrPayload,
}

#[derive(Deserialize)]
struct CadIrPayload {
    #[serde(default)]
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
            ir_version: IR_VERSION,
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
        Ok(wire.payload.into())
    }
}

impl From<CadIrPayload> for CadIr {
    fn from(payload: CadIrPayload) -> Self {
        Self {
            source: payload.source,
            tolerances: payload.tolerances,
            model: payload.model,
            native: payload.native,
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

    /// Deserialize every reserved native `unknowns` arena one record at a time.
    ///
    /// Each record is converted as it is read, so a caller that only scans the
    /// population — collecting link targets or record ids — keeps just what it
    /// retains resident rather than the whole typed population at once.
    pub fn all_native_unknowns_iter(
        &self,
    ) -> impl Iterator<Item = Result<NativeUnknownRecord, crate::native::NativeConvertError>> + '_
    {
        self.native
            .0
            .values()
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
    pub fn set_native_unknowns_from<T: Serialize, I: IntoIterator<Item = T>>(
        &mut self,
        format: &str,
        records: I,
    ) -> Result<(), crate::native::NativeConvertError> {
        self.unknowns_namespace_mut(format)
            .set_arena_from("unknowns", records)
    }

    /// Return the `format` namespace used for version-1 unknown records.
    fn unknowns_namespace_mut(&mut self, format: &str) -> &mut crate::native::NativeNamespace {
        self.native.namespace_mut(format)
    }

    /// Construct an empty current-version document with default tolerances.
    pub fn empty() -> Self {
        Self {
            source: None,
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
    pub fn to_canonical_json(&self) -> Result<String, serde_json::Error> {
        let mut canonical = self.clone();
        canonical.finalize();
        serde_json::to_string_pretty(&canonical)
    }

    /// Parse JSON and reject any unsupported `ir_version`.
    ///
    /// Version is probed first (`ir_version` only) so the full document is
    /// materialized once after the version gate.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        /// Every member but `ir_version` is skipped, and the version stays a
        /// [`serde_json::Value`] so a non-string one is reported as an
        /// unsupported version rather than as a type error.
        #[derive(Deserialize)]
        struct VersionProbe {
            ir_version: Option<serde_json::Value>,
        }

        let probe = serde_json::from_str::<VersionProbe>(s)?;
        let version = probe
            .ir_version
            .as_ref()
            .and_then(serde_json::Value::as_str);
        if version != Some(IR_VERSION) {
            return Err(<serde_json::Error as serde::de::Error>::custom(format!(
                "unsupported ir_version {version:?}; expected {IR_VERSION}"
            )));
        }
        serde_json::from_str::<CadIrPayload>(s).map(Into::into)
    }

    /// Sort model, native, and unknown-record arenas by identity.
    pub fn finalize(&mut self) {
        self.model.finalize();
        self.native.finalize();
    }

    /// Count arena rows and native loss tallies without running validation.
    pub fn census(&self) -> std::collections::BTreeMap<String, usize> {
        entity_census(self)
    }
}

macro_rules! define_registered_entity_census {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*]; )*) => {
        fn registered_entity_census(ir: &CadIr) -> BTreeMap<String, usize> {
            BTreeMap::from([
                $((stringify!($field).into(), ir.model.$field.len())),*
            ])
        }
    };
}
arena_registry!(define_registered_entity_census);

/// Count the records represented by the IR arenas without running validation.
pub fn entity_census(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = registered_entity_census(ir);
    counts.insert(
        "surfaces_unknown_geometry".into(),
        ir.model
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(
                    surface.geometry,
                    crate::geometry::SurfaceGeometry::Unknown { .. }
                )
            })
            .count(),
    );
    for loss in ir.native.loss_counts() {
        counts.insert(format!("native.{}.{}", loss.format, loss.kind), loss.count);
    }
    counts
}

/// Source-container metadata preserved for reporting.
///
/// Attribute keys ending in [`cadmpeg_ir::compare::LOCAL_DIGEST_SUFFIX`] hold
/// machine-local digests over decoded content for the write-path edit oracle.
/// Not portable across platforms; not tolerance-aware. Digests over retained
/// source bytes must not use that suffix. See
/// [`crate::hash::document_local_sha256`] and
/// [`cadmpeg_ir::compare::is_local_digest_attribute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMeta {
    classification: FormatIdentity<DialectLayers>,
    /// Format-specific attributes.
    pub attributes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SourceMetaReadWire {
    format: String,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
    #[serde(default)]
    dialects: Option<DialectLayers>,
    #[serde(default)]
    dialect: Option<DialectMatch>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct SourceMetaWriteWire<'a> {
    format: &'a str,
    attributes: &'a BTreeMap<String, String>,
    dialects: Option<&'a DialectLayers>,
}

impl Serialize for SourceMeta {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SourceMetaWriteWire {
            format: self.format(),
            attributes: &self.attributes,
            dialects: self.dialects(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SourceMeta {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SourceMetaReadWire::deserialize(deserializer)?;
        let dialects = match (wire.dialects, wire.dialect) {
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "source metadata cannot contain both dialects and legacy dialect fields",
                ));
            }
            (Some(dialects), None) => Some(dialects),
            (None, Some(dialect)) => Some(DialectLayers::of(dialect)),
            (None, None) => None,
        };
        let classification =
            FormatIdentity::from_wire(wire.format, dialects).map_err(serde::de::Error::custom)?;
        Ok(Self {
            classification,
            attributes: wire.attributes,
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for SourceMeta {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SourceMeta".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::SourceMeta").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = SourceMetaWriteWire::json_schema(generator);
        crate::schema::require_object_fields(&mut schema, ["dialects"]);
        schema
    }
}

impl SourceMeta {
    /// Constructs metadata with dialect layers whose primary format is authoritative.
    #[must_use]
    pub fn classified(dialects: DialectLayers, attributes: BTreeMap<String, String>) -> Self {
        Self {
            classification: FormatIdentity::classified(dialects),
            attributes,
        }
    }

    /// The complete source identity: format plus classified layers, if any.
    #[must_use]
    pub(crate) fn classification(&self) -> &FormatIdentity<DialectLayers> {
        &self.classification
    }

    /// Registry format namespace of this source's primary layer.
    #[must_use]
    pub fn format(&self) -> &str {
        self.classification.format()
    }

    /// Returns every source dialect layer when the source was classified.
    #[must_use]
    pub fn dialects(&self) -> Option<&DialectLayers> {
        self.classification.classified_payload()
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
