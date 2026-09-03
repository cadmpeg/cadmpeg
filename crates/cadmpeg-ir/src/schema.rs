// SPDX-License-Identifier: Apache-2.0
//! Compile-time schema contract shared by every neutral model arena.

use std::cell::Cell;
use std::fmt;

use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Serialize, Serializer};

/// Marks fields that current writers always serialize as required while their
/// deserializers remain omission-tolerant for older artifacts.
#[cfg(feature = "schema")]
pub(crate) fn require_object_fields(
    schema: &mut schemars::Schema,
    names: impl IntoIterator<Item = &'static str>,
) {
    let object = schema
        .as_object_mut()
        .expect("a derived struct schema is an object");
    let required = object
        .entry("required")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .expect("a derived struct schema has an array of required fields");
    for name in names {
        if !required.iter().any(|field| field == name) {
            required.push(serde_json::Value::String(name.to_owned()));
        }
    }
}

const REFERENCE_ID_MARKER: &str = "cadmpeg::reference_id";

thread_local! {
    static REFERENCE_WALK_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

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

/// Serializes a typed reference ID while preserving its ordinary string wire shape.
pub(crate) fn serialize_reference_id<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if REFERENCE_WALK_ACTIVE.get() {
        serializer.serialize_newtype_struct(REFERENCE_ID_MARKER, value)
    } else {
        serializer.serialize_str(value)
    }
}

#[derive(Debug)]
struct ReferenceWalkError(String);

impl fmt::Display for ReferenceWalkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ReferenceWalkError {}

impl serde::ser::Error for ReferenceWalkError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

struct ReferenceSerializer<'a> {
    identity: &'a str,
    visitor: &'a mut dyn FnMut(Reference),
}

macro_rules! ignore_scalar {
    ($($method:ident($type:ty)),* $(,)?) => {
        $(fn $method(self, _value: $type) -> Result<Self::Ok, Self::Error> { Ok(()) })*
    };
}

impl Serializer for &mut ReferenceSerializer<'_> {
    type Ok = ();
    type Error = ReferenceWalkError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore_scalar!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8]),
    );

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        if name == REFERENCE_ID_MARKER {
            let value = serde_json::to_value(value).map_err(serde::ser::Error::custom)?;
            let target = value.as_str().ok_or_else(|| {
                serde::ser::Error::custom("typed reference ID did not serialize as a string")
            })?;
            if target != self.identity {
                (self.visitor)(Reference {
                    target: target.to_owned(),
                });
            }
            Ok(())
        } else {
            value.serialize(self)
        }
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(self)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(self)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(self)
    }
}

macro_rules! serialize_element {
    ($trait:ident, $method:ident) => {
        impl $trait for &mut ReferenceSerializer<'_> {
            type Ok = ();
            type Error = ReferenceWalkError;

            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
                value.serialize(&mut **self)
            }

            fn end(self) -> Result<Self::Ok, Self::Error> {
                Ok(())
            }
        }
    };
}

serialize_element!(SerializeSeq, serialize_element);
serialize_element!(SerializeTuple, serialize_element);
serialize_element!(SerializeTupleStruct, serialize_field);

impl SerializeTupleVariant for &mut ReferenceSerializer<'_> {
    type Ok = ();
    type Error = ReferenceWalkError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeMap for &mut ReferenceSerializer<'_> {
    type Ok = ();
    type Error = ReferenceWalkError;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        key.serialize(&mut **self)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStruct for &mut ReferenceSerializer<'_> {
    type Ok = ();
    type Error = ReferenceWalkError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStructVariant for &mut ReferenceSerializer<'_> {
    type Ok = ();
    type Error = ReferenceWalkError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

/// Visits typed-ID newtypes without interpreting arbitrary strings as references.
pub(crate) fn visit_typed_references<T: EntitySchema>(
    entity: &T,
    visitor: &mut dyn FnMut(Reference),
) {
    struct WalkScope(bool);

    impl Drop for WalkScope {
        fn drop(&mut self) {
            REFERENCE_WALK_ACTIVE.set(self.0);
        }
    }

    let scope = WalkScope(REFERENCE_WALK_ACTIVE.replace(true));
    let mut serializer = ReferenceSerializer {
        identity: entity.identity(),
        visitor,
    };
    let _ = entity.serialize(&mut serializer);
    drop(scope);
}

macro_rules! impl_entity_schema {
    ($type:ty, $kind:ident, $identity:ident . 0; $($field:ident),+ $(,)?) => {
        impl EntitySchema for $type {
            const KIND: EntityKind = EntityKind::$kind;

            fn identity(&self) -> &str {
                self.$identity.0.as_str()
            }

            fn visit_references(&self, visitor: &mut dyn FnMut(Reference)) {
                let Self { $($field: _),+ } = self;
                visit_typed_references(self, visitor);
            }
        }
    };
    ($type:ty, $kind:ident, $identity:ident; $($field:ident),+ $(,)?) => {
        impl EntitySchema for $type {
            const KIND: EntityKind = EntityKind::$kind;

            fn identity(&self) -> &str {
                self.$identity.as_str()
            }

            fn visit_references(&self, visitor: &mut dyn FnMut(Reference)) {
                let Self { $($field: _),+ } = self;
                visit_typed_references(self, visitor);
            }
        }
    };
}

impl_entity_schema!(crate::topology::Body, Body, id.0; id, kind, regions, transform, name, color, visible);
impl_entity_schema!(crate::topology::Region, Region, id.0; id, body, shells);
impl_entity_schema!(crate::topology::Shell, Shell, id.0; id, region, faces, wire_edges, free_vertices);
impl_entity_schema!(crate::topology::Face, Face, id.0; id, shell, surface, sense, loops, name, color, tolerance);
impl_entity_schema!(crate::topology::Loop, Loop, id.0; id, face, boundary_role, coedges, vertex_uses);
impl_entity_schema!(crate::topology::Coedge, Coedge, id.0; id, owner_loop, edge, next, previous, radial_next, sense, pcurves, use_curve);
impl_entity_schema!(crate::topology::Edge, Edge, id.0; id, curve, start, end, param_range, tolerance);
impl_entity_schema!(crate::topology::Vertex, Vertex, id.0; id, point, tolerance);
impl_entity_schema!(crate::topology::Point, Point, id.0; id, position, source_object);
impl_entity_schema!(crate::geometry::Surface, Surface, id.0; id, geometry, source_object);
impl_entity_schema!(crate::geometry::Curve, Curve, id.0; id, geometry, source_object);
impl_entity_schema!(crate::subd::SubdSurface, SubdSurface, id.0; id, scheme, vertices, edges, faces, symmetries, source_object);
impl_entity_schema!(crate::geometry::Pcurve, Pcurve, id.0; id, geometry, wrapper_reversed, native_tail_flags, parameter_range, fit_tolerance);
impl EntitySchema for crate::geometry::ProceduralSurface {
    const KIND: EntityKind = EntityKind::ProceduralSurface;

    fn identity(&self) -> &str {
        self.id.0.as_str()
    }

    fn visit_references(&self, visitor: &mut dyn FnMut(Reference)) {
        visit_typed_references(self, visitor);
    }
}

impl EntitySchema for crate::geometry::ProceduralCurve {
    const KIND: EntityKind = EntityKind::ProceduralCurve;

    fn identity(&self) -> &str {
        self.id.0.as_str()
    }

    fn visit_references(&self, visitor: &mut dyn FnMut(Reference)) {
        visit_typed_references(self, visitor);
    }
}
impl_entity_schema!(crate::assets::Asset, Asset, id.0; id, name, media_type, content, native_ref);
impl_entity_schema!(crate::features::Feature, Feature, id.0; id, ordinal, name, suppressed, parent, dependencies, source_properties, source_tag, source_text, source_content, outputs, definition, native_ref);
impl_entity_schema!(
    crate::features::FeatureInputTopology,
    FeatureInputTopology,
    id.0;
    id, input_of, bodies, faces, edges, vertices, native_ref
);
impl_entity_schema!(
    crate::features::FeatureResultTopology,
    FeatureResultTopology,
    id.0;
    id, output_of, bodies, faces, edges, vertices, native_ref
);
impl_entity_schema!(
    crate::features::DesignConfiguration,
    DesignConfiguration,
    id.0;
    id, ordinal, active, source_index, name, material, properties, parameter_overrides,
    suppressed_features, bodies, parameter_values, feature_states, native_ref
);
impl_entity_schema!(crate::features::DesignParameter, DesignParameter, id.0; id, owner, ordinal, name, expression, display, value, dependencies, properties, pmi, native_ref);
impl_entity_schema!(crate::sketches::Sketch, Sketch, id.0; id, name, configuration, visible, placement, profiles, native_ref);
impl_entity_schema!(crate::sketches::SketchEntity, SketchEntity, id.0; id, sketch, construction, native_ref, geometry_ref, endpoint_refs, geometry);
impl_entity_schema!(crate::sketches::SketchConstraint, SketchConstraint, id.0; id, sketch, definition, name, driving, active, virtual_space, visible, orientation, label_distance, label_position, metadata, native_ref);
impl_entity_schema!(crate::sketches::SpatialSketch, SpatialSketch, id.0; id, name, configuration, visible, profiles, native_ref);
impl_entity_schema!(
    crate::sketches::SpatialSketchEntity,
    SpatialSketchEntity,
    id.0;
    id, sketch, construction, native_ref, geometry_ref, endpoint_refs, geometry
);
impl_entity_schema!(
    crate::sketches::SpatialSketchConstraint,
    SpatialSketchConstraint,
    id.0;
    id, sketch, definition, native_ref
);
impl_entity_schema!(crate::spreadsheets::Spreadsheet, Spreadsheet, id.0; id, feature, cells, column_widths, row_heights, merged_ranges, native_ref);
impl_entity_schema!(crate::products::ProductDefinition, ProductDefinition, id.0; id, kind, source_name, label, description, part_number, bom_properties, bodies, native_ref);
impl_entity_schema!(crate::products::Occurrence, Occurrence, id.0; id, prototype, parent, ordinal, transform, linked_prototype, scale, name, linked_subelements, visible, element_component, claim_child, copy_on_change, copy_on_change_source, copy_on_change_group, copy_on_change_touched, native_ref);
impl EntitySchema for crate::products::AssemblyJoint {
    const KIND: EntityKind = EntityKind::AssemblyJoint;

    fn identity(&self) -> &str {
        self.id.0.as_str()
    }

    fn visit_references(&self, visitor: &mut dyn FnMut(Reference)) {
        visit_typed_references(self, visitor);
    }
}
impl_entity_schema!(crate::drawings::Drawing, Drawing, id.0; id, object, kind, runtime_type, order, visible, relationships, template, position, scale, direction, rotation_degrees, parameters, assets, native_ref);
impl_entity_schema!(
    crate::semantic_annotations::SemanticAnnotation,
    SemanticAnnotation,
    id.0;
    id, object, kind, runtime_type, order, text, references, value, format, position,
    parameters, assets, native_ref
);
impl_entity_schema!(
    crate::presentation::PresentationDocument,
    PresentationDocument,
    id.0;
    id, schema_version, active_view, camera, states, native_ref
);
impl_entity_schema!(
    crate::presentation::ViewPresentation,
    ViewPresentation,
    id.0;
    id, object, order, expanded, visible, display_mode, selection_style, line_width,
    point_size, properties, native_ref
);
impl_entity_schema!(crate::tessellation::Tessellation, Tessellation, id; id, body, faces, chordal_deflection, source_object, vertices, triangles, feature_edges, strip_lengths, normals, corner_normals, triangle_groups, texture_assignments, channels);
impl_entity_schema!(crate::appearance::Appearance, Appearance, id.0; id, name, asset_guid, library_id, visual_guid, physical_token, schema, category, base_color, properties, textures);
impl_entity_schema!(crate::appearance::AppearanceBinding, AppearanceBinding, id; id, target, appearance, source_entity_id, object_type, visible, channels);
impl_entity_schema!(crate::attributes::SourceAttribute, SourceAttribute, id.0; id, target, name, values);
impl_entity_schema!(crate::pmi::PmiAnnotation, PmiAnnotation, id.0; id, name, visible, targets, definition);
impl_entity_schema!(
    crate::presentation::PresentationLayer,
    PresentationLayer,
    id.0;
    id, name, description, visible, items
);

#[cfg(test)]
mod tests;
