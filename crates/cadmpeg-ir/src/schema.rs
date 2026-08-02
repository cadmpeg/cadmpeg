// SPDX-License-Identifier: Apache-2.0
//! Compile-time schema contract shared by every neutral model arena.

use serde::Serialize;

/// Canonical neutral arena kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    /// Body.
    Body,
    /// Region.
    Region,
    /// Shell.
    Shell,
    /// Face.
    Face,
    /// Loop.
    Loop,
    /// Coedge.
    Coedge,
    /// Edge.
    Edge,
    /// Vertex.
    Vertex,
    /// Point.
    Point,
    /// Surface.
    Surface,
    /// Curve.
    Curve,
    /// Subdivision surface.
    SubdSurface,
    /// Parameter-space curve.
    Pcurve,
    /// Procedural surface.
    ProceduralSurface,
    /// Procedural curve.
    ProceduralCurve,
    /// Feature.
    Feature,
    /// Feature input topology.
    FeatureInputTopology,
    /// Design configuration.
    DesignConfiguration,
    /// Design parameter.
    DesignParameter,
    /// Planar sketch.
    Sketch,
    /// Planar sketch entity.
    SketchEntity,
    /// Planar sketch constraint.
    SketchConstraint,
    /// Spatial sketch.
    SpatialSketch,
    /// Spatial sketch entity.
    SpatialSketchEntity,
    /// Spatial sketch constraint.
    SpatialSketchConstraint,
    /// Spreadsheet.
    Spreadsheet,
    /// Product definition.
    ProductDefinition,
    /// Product occurrence.
    Occurrence,
    /// Assembly joint.
    AssemblyJoint,
    /// Drawing.
    Drawing,
    /// Semantic annotation.
    SemanticAnnotation,
    /// Presentation document.
    PresentationDocument,
    /// View presentation.
    ViewPresentation,
    /// Tessellation.
    Tessellation,
    /// Appearance asset.
    Appearance,
    /// Appearance binding.
    AppearanceBinding,
    /// Source attribute.
    SourceAttribute,
    /// Product-manufacturing annotation.
    PmiAnnotation,
    /// Presentation layer.
    PresentationLayer,
}

impl EntityKind {
    /// Every registered entity kind in canonical arena order.
    pub const ALL: [Self; 39] = [
        Self::Body,
        Self::Region,
        Self::Shell,
        Self::Face,
        Self::Loop,
        Self::Coedge,
        Self::Edge,
        Self::Vertex,
        Self::Point,
        Self::Surface,
        Self::Curve,
        Self::SubdSurface,
        Self::Pcurve,
        Self::ProceduralSurface,
        Self::ProceduralCurve,
        Self::Feature,
        Self::FeatureInputTopology,
        Self::DesignConfiguration,
        Self::DesignParameter,
        Self::Sketch,
        Self::SketchEntity,
        Self::SketchConstraint,
        Self::SpatialSketch,
        Self::SpatialSketchEntity,
        Self::SpatialSketchConstraint,
        Self::Spreadsheet,
        Self::ProductDefinition,
        Self::Occurrence,
        Self::AssemblyJoint,
        Self::Drawing,
        Self::SemanticAnnotation,
        Self::PresentationDocument,
        Self::ViewPresentation,
        Self::Tessellation,
        Self::Appearance,
        Self::AppearanceBinding,
        Self::SourceAttribute,
        Self::PmiAnnotation,
        Self::PresentationLayer,
    ];
}

/// One typed identity reference emitted by an entity schema walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Referenced globally unique identity.
    pub target: String,
}

/// Schema behavior required for every entity admitted to a model arena.
pub trait EntitySchema: Serialize {
    /// Entity's canonical arena kind.
    const KIND: EntityKind;

    /// Globally unique entity identity.
    fn identity(&self) -> &str;

    /// Visits every typed identity reference held by this entity.
    fn visit_references(&self, visitor: &mut dyn FnMut(Reference));
}

fn looks_like_identity(value: &str) -> bool {
    let Some((namespace, key)) = value.split_once('#') else {
        return false;
    };
    !key.is_empty() && namespace.split(':').count() >= 3
}

fn visit_value(value: &serde_json::Value, identity: &str, visitor: &mut dyn FnMut(Reference)) {
    match value {
        serde_json::Value::String(target) if target != identity && looks_like_identity(target) => {
            visitor(Reference {
                target: target.clone(),
            });
        }
        serde_json::Value::Array(values) => {
            for value in values {
                visit_value(value, identity, visitor);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                visit_value(value, identity, visitor);
            }
        }
        _ => {}
    }
}

/// Visits serialized typed-ID newtypes without imposing a second hand-maintained field walk.
pub(crate) fn visit_serialized_references<T: EntitySchema>(
    entity: &T,
    visitor: &mut dyn FnMut(Reference),
) {
    if let Ok(value) = serde_json::to_value(entity) {
        visit_value(&value, entity.identity(), visitor);
    }
}

macro_rules! impl_entity_schema {
    ($type:ty, $kind:ident, $($identity:tt)+) => {
        impl EntitySchema for $type {
            const KIND: EntityKind = EntityKind::$kind;

            fn identity(&self) -> &str {
                self.$($identity)+.as_str()
            }

            fn visit_references(&self, visitor: &mut dyn FnMut(Reference)) {
                visit_serialized_references(self, visitor);
            }
        }
    };
}

impl_entity_schema!(crate::topology::Body, Body, id.0);
impl_entity_schema!(crate::topology::Region, Region, id.0);
impl_entity_schema!(crate::topology::Shell, Shell, id.0);
impl_entity_schema!(crate::topology::Face, Face, id.0);
impl_entity_schema!(crate::topology::Loop, Loop, id.0);
impl_entity_schema!(crate::topology::Coedge, Coedge, id.0);
impl_entity_schema!(crate::topology::Edge, Edge, id.0);
impl_entity_schema!(crate::topology::Vertex, Vertex, id.0);
impl_entity_schema!(crate::topology::Point, Point, id.0);
impl_entity_schema!(crate::geometry::Surface, Surface, id.0);
impl_entity_schema!(crate::geometry::Curve, Curve, id.0);
impl_entity_schema!(crate::subd::SubdSurface, SubdSurface, id.0);
impl_entity_schema!(crate::geometry::Pcurve, Pcurve, id.0);
impl_entity_schema!(crate::geometry::ProceduralSurface, ProceduralSurface, id.0);
impl_entity_schema!(crate::geometry::ProceduralCurve, ProceduralCurve, id.0);
impl_entity_schema!(crate::features::Feature, Feature, id.0);
impl_entity_schema!(
    crate::features::FeatureInputTopology,
    FeatureInputTopology,
    id.0
);
impl_entity_schema!(
    crate::features::DesignConfiguration,
    DesignConfiguration,
    id.0
);
impl_entity_schema!(crate::features::DesignParameter, DesignParameter, id.0);
impl_entity_schema!(crate::sketches::Sketch, Sketch, id.0);
impl_entity_schema!(crate::sketches::SketchEntity, SketchEntity, id.0);
impl_entity_schema!(crate::sketches::SketchConstraint, SketchConstraint, id.0);
impl_entity_schema!(crate::sketches::SpatialSketch, SpatialSketch, id.0);
impl_entity_schema!(
    crate::sketches::SpatialSketchEntity,
    SpatialSketchEntity,
    id.0
);
impl_entity_schema!(
    crate::sketches::SpatialSketchConstraint,
    SpatialSketchConstraint,
    id.0
);
impl_entity_schema!(crate::spreadsheets::Spreadsheet, Spreadsheet, id.0);
impl_entity_schema!(crate::products::ProductDefinition, ProductDefinition, id.0);
impl_entity_schema!(crate::products::Occurrence, Occurrence, id.0);
impl_entity_schema!(crate::products::AssemblyJoint, AssemblyJoint, id.0);
impl_entity_schema!(crate::drawings::Drawing, Drawing, id.0);
impl_entity_schema!(
    crate::semantic_annotations::SemanticAnnotation,
    SemanticAnnotation,
    id.0
);
impl_entity_schema!(
    crate::presentation::PresentationDocument,
    PresentationDocument,
    id.0
);
impl_entity_schema!(
    crate::presentation::ViewPresentation,
    ViewPresentation,
    id.0
);
impl_entity_schema!(crate::tessellation::Tessellation, Tessellation, id);
impl_entity_schema!(crate::appearance::Appearance, Appearance, id.0);
impl_entity_schema!(crate::appearance::AppearanceBinding, AppearanceBinding, id);
impl_entity_schema!(crate::attributes::SourceAttribute, SourceAttribute, id.0);
impl_entity_schema!(crate::pmi::PmiAnnotation, PmiAnnotation, id.0);
impl_entity_schema!(
    crate::presentation::PresentationLayer,
    PresentationLayer,
    id.0
);
