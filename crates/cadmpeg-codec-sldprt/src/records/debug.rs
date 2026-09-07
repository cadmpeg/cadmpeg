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
            .field("source_id", &self.source_id)
            .field("parent_source_id", &self.parent_source_id())
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
