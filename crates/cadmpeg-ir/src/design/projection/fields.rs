// SPDX-License-Identifier: Apache-2.0
//! The fields of the design records a decoder projects and of the model
//! records they restate. Each key is a field name of the record type as
//! written on the wire, and the impls of each enum destructure the record by
//! those names, so every variant names a field and every field has one.

use crate::design::feature::{Feature, FeatureDefinition};
use crate::design::options::NativeOptions;
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::units::ValueKind;
use crate::features::parameters::DesignParameter as ModelParameter;
use crate::features::serialization::FeatureWriteWire;
use crate::ids::ProductDefinitionId;
use crate::sketches::{Sketch as ModelSketch, SketchConstraint as ModelSketchConstraint};

/// A field of a record, named by its wire key.
pub trait Field: Copy + Ord + 'static {
    /// The field's wire key.
    fn key(self) -> &'static str;
    /// The field whose wire key is `key`.
    fn parse(key: &str) -> Option<Self>;
}

/// Whether a design record serializes a field of this type.
trait Stated {
    fn stated(&self) -> bool;
}

impl<T> Stated for Option<T> {
    fn stated(&self) -> bool {
        self.is_some()
    }
}

macro_rules! always_stated {
    ($($type:ty),+) => {
        $(impl Stated for $type {
            fn stated(&self) -> bool {
                true
            }
        })+
    };
}

always_stated!(
    FeatureDefinition,
    ProductDefinitionId,
    ValueKind,
    ParameterDefinition,
    NativeOptions
);

/// Implements [`Field`] for the field enum `$name`, each variant keyed by its
/// field name.
macro_rules! field_keys {
    ($name:ident { $($field:ident => $variant:ident),+ $(,)? }) => {
        impl $name {
            /// Every field, in declared order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
        }

        impl Field for $name {
            fn key(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($field)),+
                }
            }

            fn parse(key: &str) -> Option<Self> {
                match key {
                    $(stringify!($field) => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

/// Implements [`Field`] for the field enum of a design record type, and
/// whether a record states each field.
macro_rules! design_fields {
    ($name:ident of $record:ident { $($field:ident => $variant:ident),+ $(,)? }) => {
        field_keys!($name { $($field => $variant),+ });

        impl $name {
            /// Each field with whether `record` serializes it.
            pub(super) fn stated(record: &$record) -> [(Self, bool); [$(stringify!($field)),+].len()] {
                let $record { id: _, $($field),+ } = record;
                [$((Self::$variant, Stated::stated($field))),+]
            }
        }
    };
}

/// Implements [`Field`] for the field enum of a model record type. A field
/// the type gains or renames fails to compile here.
macro_rules! model_fields {
    ($name:ident of $record:ident $(<$lifetime:lifetime>)? { $($field:ident => $variant:ident),+ $(,)? }) => {
        field_keys!($name { $($field => $variant),+ });

        const _: fn(&$record$(<$lifetime>)?) = |record| {
            let $record { id: _, $($field: _),+ } = record;
        };
    };
}

/// A field of a design feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeatureField {
    /// `name`
    Name,
    /// `suppressed`
    Suppressed,
    /// `definition`
    Definition,
    /// `placement`
    Placement,
    /// `component`
    Component,
    /// `driven_by`
    DrivenBy,
    /// `native_ref`
    NativeRef,
}

design_fields! {
    FeatureField of Feature {
        name => Name,
        suppressed => Suppressed,
        definition => Definition,
        placement => Placement,
        component => Component,
        driven_by => DrivenBy,
        native_ref => NativeRef,
    }
}

/// A field of a design parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParameterField {
    /// `scope`
    Scope,
    /// `owner`
    Owner,
    /// `position`
    Position,
    /// `name`
    Name,
    /// `kind`
    Kind,
    /// `unit`
    Unit,
    /// `definition`
    Definition,
    /// `value`
    Value,
    /// `display`
    Display,
    /// `tolerance`
    Tolerance,
    /// `decimals`
    Decimals,
    /// `comment`
    Comment,
    /// `native_options`
    NativeOptions,
    /// `source_settings`
    SourceSettings,
    /// `native_ref`
    NativeRef,
}

design_fields! {
    ParameterField of DesignParameter {
        scope => Scope,
        owner => Owner,
        position => Position,
        name => Name,
        kind => Kind,
        unit => Unit,
        definition => Definition,
        value => Value,
        display => Display,
        tolerance => Tolerance,
        decimals => Decimals,
        comment => Comment,
        native_options => NativeOptions,
        source_settings => SourceSettings,
        native_ref => NativeRef,
    }
}

/// A field of a model feature, as the wire writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelFeatureField {
    /// `product_definition`
    ProductDefinition,
    /// `ordinal`
    Ordinal,
    /// `name`
    Name,
    /// `suppressed`
    Suppressed,
    /// `regeneration_parent`
    RegenerationParent,
    /// `dependencies`
    Dependencies,
    /// `source_properties`
    SourceProperties,
    /// `source_tag`
    SourceTag,
    /// `source_text`
    SourceText,
    /// `source_content`
    SourceContent,
    /// `outputs`
    Outputs,
    /// `definition`
    Definition,
    /// `native_ref`
    NativeRef,
}

model_fields! {
    ModelFeatureField of FeatureWriteWire<'_> {
        product_definition => ProductDefinition,
        ordinal => Ordinal,
        name => Name,
        suppressed => Suppressed,
        regeneration_parent => RegenerationParent,
        dependencies => Dependencies,
        source_properties => SourceProperties,
        source_tag => SourceTag,
        source_text => SourceText,
        source_content => SourceContent,
        outputs => Outputs,
        definition => Definition,
        native_ref => NativeRef,
    }
}

/// A field of a model parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelParameterField {
    /// `product_definition`
    ProductDefinition,
    /// `owner`
    Owner,
    /// `ordinal`
    Ordinal,
    /// `name`
    Name,
    /// `expression`
    ExpressionText,
    /// `display`
    Display,
    /// `value`
    Value,
    /// `dependencies`
    Dependencies,
    /// `properties`
    Properties,
    /// `pmi`
    Pmi,
    /// `native_ref`
    NativeRef,
}

model_fields! {
    ModelParameterField of ModelParameter {
        product_definition => ProductDefinition,
        owner => Owner,
        ordinal => Ordinal,
        name => Name,
        expression => ExpressionText,
        display => Display,
        value => Value,
        dependencies => Dependencies,
        properties => Properties,
        pmi => Pmi,
        native_ref => NativeRef,
    }
}

/// A field of a model sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelSketchField {
    /// `name`
    Name,
    /// `configuration`
    Configuration,
    /// `visible`
    Visible,
    /// `placement`
    Placement,
    /// `profiles`
    Profiles,
    /// `native_ref`
    NativeRef,
}

model_fields! {
    ModelSketchField of ModelSketch {
        name => Name,
        configuration => Configuration,
        visible => Visible,
        placement => Placement,
        profiles => Profiles,
        native_ref => NativeRef,
    }
}

/// A field of a model sketch entity. The type keeps its identity private,
/// so the serialization test checks these keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelSketchEntityField {
    /// `sketch`
    Sketch,
    /// `construction`
    Construction,
    /// `native_ref`
    NativeRef,
    /// `geometry_ref`
    GeometryRef,
    /// `endpoint_refs`
    EndpointRefs,
    /// `center_ref`
    CenterRef,
    /// `geometry`
    Geometry,
}

field_keys!(ModelSketchEntityField {
    sketch => Sketch,
    construction => Construction,
    native_ref => NativeRef,
    geometry_ref => GeometryRef,
    endpoint_refs => EndpointRefs,
    center_ref => CenterRef,
    geometry => Geometry,
});

/// A field of a model sketch constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelSketchConstraintField {
    /// `sketch`
    Sketch,
    /// `definition`
    Definition,
    /// `name`
    Name,
    /// `driving`
    Driving,
    /// `active`
    Active,
    /// `virtual_space`
    VirtualSpace,
    /// `visible`
    Visible,
    /// `orientation`
    Orientation,
    /// `label_distance`
    LabelDistance,
    /// `label_position`
    LabelPosition,
    /// `metadata`
    Metadata,
    /// `native_ref`
    NativeRef,
}

model_fields! {
    ModelSketchConstraintField of ModelSketchConstraint {
        sketch => Sketch,
        definition => Definition,
        name => Name,
        driving => Driving,
        active => Active,
        virtual_space => VirtualSpace,
        visible => Visible,
        orientation => Orientation,
        label_distance => LabelDistance,
        label_position => LabelPosition,
        metadata => Metadata,
        native_ref => NativeRef,
    }
}

#[cfg(test)]
mod tests;
