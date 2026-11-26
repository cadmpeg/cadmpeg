// SPDX-License-Identifier: Apache-2.0
//! The fields of the design records a decoder projects and of the model
//! records they restate. Each enum is generated from the field names of its
//! record type, and each key is its field name as written on the wire.

use crate::design::feature::{Feature, FeatureDefinition};
use crate::design::options::NativeOptions;
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::units::ValueKind;
use crate::features::parameters::DesignParameter as ModelParameter;
use crate::features::serialization::FeatureWriteWire;
use crate::ids::ProductDefinitionId;

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

macro_rules! field_enum {
    ($(#[$meta:meta])* $name:ident { $($field:ident => $variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum $name {
            $(#[doc = concat!("`", stringify!($field), "`")] $variant),+
        }

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

/// A design record type's field enum, with whether a record states each field.
macro_rules! design_fields {
    ($(#[$meta:meta])* $name:ident of $record:ident { $($field:ident => $variant:ident),+ $(,)? }) => {
        field_enum!($(#[$meta])* $name { $($field => $variant),+ });

        impl $name {
            /// Each field with whether `record` serializes it.
            pub(super) fn stated(record: &$record) -> [(Self, bool); [$(stringify!($field)),+].len()] {
                let $record { id: _, $($field),+ } = record;
                [$((Self::$variant, Stated::stated($field))),+]
            }
        }
    };
}

/// A model record type's field enum. A field the type gains or renames fails
/// to compile here.
macro_rules! model_fields {
    ($(#[$meta:meta])* $name:ident of $record:ident $(<$lifetime:lifetime>)? { $($field:ident => $variant:ident),+ $(,)? }) => {
        field_enum!($(#[$meta])* $name { $($field => $variant),+ });

        const _: fn(&$record$(<$lifetime>)?) = |record| {
            let $record { id: _, $($field: _),+ } = record;
        };
    };
}

design_fields! {
    /// A field of a design feature.
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

design_fields! {
    /// A field of a design parameter.
    ParameterField of DesignParameter {
        scope => Scope,
        owner => Owner,
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

model_fields! {
    /// A field of a model feature, as the wire writes it.
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

model_fields! {
    /// A field of a model parameter.
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

#[cfg(test)]
mod tests;
