// SPDX-License-Identifier: Apache-2.0
//! Compile-time schema contract shared by every neutral model arena.

use serde::Serialize;

pub mod rewrite;
/// Caller-accounted structural value projection and reconstruction.
pub mod structural;

/// Canonical neutral arena kind.
#[repr(usize)]
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
    /// Embedded or externally referenced document resource.
    Asset,
    /// Feature.
    Feature,
    /// Feature input topology.
    FeatureInputTopology,
    /// Feature result topology.
    FeatureResultTopology,
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
    /// Position of this kind in the canonical arena registry.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Body => 0,
            Self::Region => 1,
            Self::Shell => 2,
            Self::Face => 3,
            Self::Loop => 4,
            Self::Coedge => 5,
            Self::Edge => 6,
            Self::Vertex => 7,
            Self::Point => 8,
            Self::Surface => 9,
            Self::Curve => 10,
            Self::SubdSurface => 11,
            Self::Pcurve => 12,
            Self::ProceduralSurface => 13,
            Self::ProceduralCurve => 14,
            Self::Asset => 15,
            Self::Feature => 16,
            Self::FeatureInputTopology => 17,
            Self::FeatureResultTopology => 18,
            Self::DesignConfiguration => 19,
            Self::DesignParameter => 20,
            Self::Sketch => 21,
            Self::SketchEntity => 22,
            Self::SketchConstraint => 23,
            Self::SpatialSketch => 24,
            Self::SpatialSketchEntity => 25,
            Self::SpatialSketchConstraint => 26,
            Self::Spreadsheet => 27,
            Self::ProductDefinition => 28,
            Self::Occurrence => 29,
            Self::AssemblyJoint => 30,
            Self::Drawing => 31,
            Self::SemanticAnnotation => 32,
            Self::PresentationDocument => 33,
            Self::ViewPresentation => 34,
            Self::Tessellation => 35,
            Self::Appearance => 36,
            Self::AppearanceBinding => 37,
            Self::SourceAttribute => 38,
            Self::PmiAnnotation => 39,
            Self::PresentationLayer => 40,
        }
    }

    /// Every registered entity kind in canonical arena order.
    pub const ALL: [Self; 41] = [
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
        Self::Asset,
        Self::Feature,
        Self::FeatureInputTopology,
        Self::FeatureResultTopology,
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

/// Schema behavior required for every entity admitted to a model arena.
pub trait EntitySchema: Serialize + rewrite::typed::RewriteIdentities {
    /// Entity's canonical arena kind.
    const KIND: EntityKind;

    /// Globally unique entity identity.
    fn identity(&self) -> &str;

    /// Visit borrowed typed references through fields under the caller's context.
    fn visit_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError> {
        rewrite::typed::RewriteIdentities::visit_identity_references(self, ctx, &mut |target| {
            let identity = self.identity();
            if ctx.equal_bytes(
                identity.as_bytes(),
                target.as_bytes(),
                "typed reference owner comparison",
            )? {
                return Ok(());
            }
            visitor(target)
        })
    }
}

macro_rules! impl_entity_schema {
    ($type:ty, $kind:ident, $identity:ident $(.$inner:tt)?; $($field:ident),+ $(,)?) => {
        impl EntitySchema for $type {
            const KIND: EntityKind = EntityKind::$kind;

            fn identity(&self) -> &str {
                let Self { $($field: _),+ } = self;
                self.$identity $(.$inner)?.as_str()
            }
        }
    };
}

impl_entity_schema!(crate::topology::Body, Body, id; id, kind, regions, transform, name, color, visible);
impl_entity_schema!(crate::topology::Region, Region, id; id, body, shells);
impl EntitySchema for crate::topology::Shell {
    const KIND: EntityKind = EntityKind::Shell;
    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::topology::Face, Face, id; id, shell, surface, sense, loops, name, color, tolerance);
impl_entity_schema!(crate::topology::Loop, Loop, id; id, face, boundary);
impl_entity_schema!(crate::topology::Coedge, Coedge, id; id, owner_loop, edge, radial_next, sense, pcurves, use_curve);
impl_entity_schema!(crate::topology::Edge, Edge, id; id, carrier, start, end, tolerance);
impl_entity_schema!(crate::topology::Vertex, Vertex, id; id, point, tolerance);
impl EntitySchema for crate::topology::Point {
    const KIND: EntityKind = EntityKind::Point;
    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::geometry::Surface, Surface, id; id, geometry, source_object);
impl_entity_schema!(crate::geometry::Curve, Curve, id; id, geometry, source_object);
impl_entity_schema!(crate::subd::SubdSurface, SubdSurface, id; id, scheme, cage, source_object);
impl_entity_schema!(crate::geometry::pcurve::Pcurve, Pcurve, id; id, geometry, metadata);
impl EntitySchema for crate::geometry::ProceduralSurface {
    const KIND: EntityKind = EntityKind::ProceduralSurface;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}

impl EntitySchema for crate::geometry::ProceduralCurve {
    const KIND: EntityKind = EntityKind::ProceduralCurve;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::assets::Asset, Asset, id; id, name, media_type, content, native_ref);
impl_entity_schema!(crate::features::Feature, Feature, id; id, ordinal, name, suppressed, dependencies, source_properties, source_tag, source_text, source_content, evaluation, native_ref);
impl_entity_schema!(
    crate::features::FeatureInputTopology,
    FeatureInputTopology,
    id;
    id, input_of, bodies, faces, edges, vertices, native_ref
);
impl EntitySchema for crate::features::FeatureResultTopology {
    const KIND: EntityKind = EntityKind::FeatureResultTopology;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(
    crate::features::DesignConfiguration,
    DesignConfiguration,
    id;
    id, ordinal, active, source_index, name, material, properties, parameter_overrides,
    bodies, parameter_values, feature_states, native_ref
);
impl_entity_schema!(crate::features::DesignParameter, DesignParameter, id; id, owner, ordinal, name, expression, display, value, dependencies, properties, pmi, native_ref);
impl_entity_schema!(crate::sketches::Sketch, Sketch, id; id, name, configuration, visible, placement, profiles, native_ref);
impl EntitySchema for crate::sketches::SketchEntity {
    const KIND: EntityKind = EntityKind::SketchEntity;

    fn identity(&self) -> &str {
        self.id().as_str()
    }
}
impl_entity_schema!(crate::sketches::SketchConstraint, SketchConstraint, id; id, sketch, definition, name, driving, active, virtual_space, visible, orientation, label_distance, label_position, metadata, native_ref);
impl_entity_schema!(crate::sketches::SpatialSketch, SpatialSketch, id; id, name, configuration, visible, profiles, native_ref);
impl EntitySchema for crate::sketches::SpatialSketchEntity {
    const KIND: EntityKind = EntityKind::SpatialSketchEntity;

    fn identity(&self) -> &str {
        self.id().as_str()
    }
}
impl_entity_schema!(
    crate::sketches::SpatialSketchConstraint,
    SpatialSketchConstraint,
    id;
    id, sketch, definition, native_ref
);
impl EntitySchema for crate::spreadsheets::Spreadsheet {
    const KIND: EntityKind = EntityKind::Spreadsheet;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::products::ProductDefinition, ProductDefinition, id; id, kind, source_name, label, description, part_number, bom_properties, bodies, native_ref);
impl_entity_schema!(crate::products::Occurrence, Occurrence, id; id, prototype, parent, ordinal, transform, linked_prototype, scale, name, visible, link, native_ref);
impl EntitySchema for crate::products::AssemblyJoint {
    const KIND: EntityKind = EntityKind::AssemblyJoint;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::drawings::Drawing, Drawing, id; id, object, kind, runtime_type, order, visible, relationships, template, position, scale, direction, rotation_degrees, parameters, assets, native_ref);
impl_entity_schema!(
    crate::semantic_annotations::SemanticAnnotation,
    SemanticAnnotation,
    id;
    id, object, kind, runtime_type, order, text, references, value, format, position,
    parameters, assets, native_ref
);
impl EntitySchema for crate::presentation::PresentationDocument {
    const KIND: EntityKind = EntityKind::PresentationDocument;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(
    crate::presentation::ViewPresentation,
    ViewPresentation,
    id;
    id, object, order, expanded, visible, display_mode, selection_style, line_width,
    point_size, properties, native_ref
);
impl EntitySchema for crate::tessellation::Tessellation {
    const KIND: EntityKind = EntityKind::Tessellation;

    fn identity(&self) -> &str {
        self.id.as_str()
    }
}
impl_entity_schema!(crate::appearance::Appearance, Appearance, id; id, name, asset_guid, library_id, visual_guid, physical_token, schema, category, base_color, properties, textures);
impl_entity_schema!(crate::appearance::AppearanceBinding, AppearanceBinding, id; id, target, appearance, source_entity_id, object_type, visible, channels);
impl_entity_schema!(crate::attributes::SourceAttribute, SourceAttribute, id; id, target, name, values);
impl_entity_schema!(crate::pmi::PmiAnnotation, PmiAnnotation, id; id, name, visible, targets, definition);
impl_entity_schema!(
    crate::presentation::PresentationLayer,
    PresentationLayer,
    id;
    id, name, description, visible, items
);

#[cfg(test)]
mod tests;
