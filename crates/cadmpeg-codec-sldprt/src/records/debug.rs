// SPDX-License-Identifier: Apache-2.0
//! Legacy debug field projections used by persisted native state hashes.
//!
//! The history and sketch hash contracts digest Debug text. These projections
//! derive removed fields at formatting time and preserve their original order.

use std::fmt::{Debug, Formatter, Result};

impl Debug for super::Feature {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("Feature")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("xml_tag", &self.xml_tag)
            .field("tree_parent", &self.tree_parent_record_id())
            .field("source_id", &self.source_id.map(String::from))
            .field(
                "parent_source_id",
                &self.parent_source_id().map(String::from),
            )
            .field("ordinal", &self.ordinal)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("input_class", &self.input_class)
            .field("suppressed", &self.suppressed)
            .field("parameters", &self.parameters)
            .field("dimension_properties", &self.dimension_properties)
            .field("properties", &self.properties)
            .field("text", &self.text)
            .field("content", &self.content)
            .finish()
    }
}

impl Debug for super::FeatureInputName {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("FeatureInputName")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("ordinal", &self.ordinal)
            .field("offset", &self.offset)
            .field("object_id", &self.object_id.map(u32::from))
            .field("value", &self.value)
            .finish()
    }
}

impl Debug for super::FeatureInputClass {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("FeatureInputClass")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("ordinal", &self.ordinal)
            .field("offset", &self.offset)
            .field("name", &self.name)
            .field("role", &self.role())
            .finish()
    }
}

impl Debug for super::FeatureInputScalar {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("FeatureInputScalar")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("feature_ref", &self.feature_ref)
            .field("ordinal", &self.ordinal)
            .field("offset", &self.offset)
            .field("object_id", &self.object_id)
            .field("name", &self.name)
            .field("value", &self.value)
            .field("role", &self.role)
            .field("entity_indices", &self.entity_indices())
            .field("operands", &self.operands)
            .finish()
    }
}

// The scalar carrier is rendered through its legacy projections for persisted hashes.
#[allow(clippy::missing_fields_in_debug)]
impl Debug for super::FeatureInputRelationInstance {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("FeatureInputRelationInstance")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("ordinal", &self.ordinal)
            .field("offset", &self.offset)
            .field("family", &self.family)
            .field("class_ref", &self.class_ref)
            .field("feature_ref", &self.feature_ref)
            .field("scalar_refs", &self.scalar_refs())
            .field("parameter_scalar_ref", &self.parameter_scalar_ref())
            .field("display_scalar_ref", &self.display_scalar_ref())
            .field("operands", &self.operands)
            .finish()
    }
}

// The kind is rendered as its legacy endpoint selector for persisted hashes.
#[allow(clippy::missing_fields_in_debug)]
impl Debug for super::FeatureInputSurfaceSelection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("FeatureInputSurfaceSelection")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("ordinal", &self.ordinal)
            .field("offset", &self.offset)
            .field("selector", &self.selector)
            .field("endpoint_selector", &self.endpoint_selector())
            .field("object_name_ref", &self.object_name_ref)
            .field("feature_ref", &self.feature_ref)
            .field("producer_feature_refs", &self.producer_feature_refs)
            .field("terminal_feature_ref", &self.terminal_feature_ref)
            .field("components", &self.components)
            .finish()
    }
}

impl Debug for super::SketchInputEntity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter
            .debug_struct("SketchInputEntity")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("feature_ref", &self.feature_ref)
            .field("ordinal", &self.ordinal)
            .field("offset", &self.offset)
            .field("object_index", &self.object_index)
            .field("local_id", &self.local_id)
            .field("kind", &self.kind)
            .field("state_value", &self.state_value)
            .field("coordinates_m", &self.coordinates_m)
            .field("links", &self.links())
            .field(
                "link_selector",
                &self.links.as_ref().map(super::SketchInputLinks::selector),
            )
            .finish()
    }
}

impl Debug for super::SketchInputKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        match self {
            Self::Point => formatter.write_str("Point"),
            Self::LineOrCircle => formatter.write_str("LineOrCircle"),
            Self::Arc => formatter.write_str("Arc"),
            Self::ConstrainedPoint => formatter.write_str("ConstrainedPoint"),
            Self::Relation(relation) => formatter.debug_tuple("Relation").field(relation).finish(),
            Self::Native(_) | Self::NativeHandle(_) => formatter
                .debug_tuple("Native")
                .field(&self.native_code())
                .finish(),
        }
    }
}
