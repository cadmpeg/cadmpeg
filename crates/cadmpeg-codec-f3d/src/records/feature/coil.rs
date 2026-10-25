// SPDX-License-Identifier: Apache-2.0
//! Coil extents, sections, placements, transforms and the coil scope.

use super::extrude::DesignExtrudeOperation;
use super::scope::DesignFaceRecipeKind;
use crate::records::identity::{
    DesignSecondaryIdentity, MaybeRecordedValue, RecordedValue, Sourced,
};
use crate::records::mesh::DesignRelaxedGuidText;
use crate::records::recipes::{ConstructionRecipeDesign, ConstructionRecipeSelector};
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use serde::{Deserialize, Deserializer, Serialize};

cadmpeg_core::named_optional_field!(deserialize_coil_clockwise, bool, "coil_clockwise");
cadmpeg_core::named_optional_field!(deserialize_coil_extent, DesignCoilExtent, "coil_extent");
cadmpeg_core::named_optional_field!(
    deserialize_coil_operation,
    DesignExtrudeOperation,
    "coil_operation"
);
cadmpeg_core::named_optional_field!(
    deserialize_coil_placement,
    DesignCoilPlacement,
    "coil_placement"
);
cadmpeg_core::named_optional_field!(deserialize_coil_section, DesignCoilSection, "coil_section");
cadmpeg_core::named_optional_field!(
    deserialize_coil_section_placement,
    DesignCoilSectionPlacement,
    "coil_section_placement"
);
cadmpeg_core::named_optional_field!(
    deserialize_coil_transform,
    DesignCoilTransform,
    "coil_transform"
);
cadmpeg_core::named_optional_field!(
    deserialize_curve_secondary_identity,
    u64,
    "curve_secondary_identity"
);
cadmpeg_core::named_optional_field!(deserialize_design_id, String, "design_id");
cadmpeg_core::named_optional_field!(deserialize_transform, SketchPlacementMatrix, "transform");
cadmpeg_core::named_optional_field!(
    deserialize_design_selector,
    ConstructionRecipeSelector,
    "design_selector"
);
cadmpeg_core::named_optional_field!(deserialize_secondary_identity, u64, "secondary_identity");
/// Driving-dimension mode stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCoilExtent {
    /// Revolution count and total height are independent.
    RevolutionsHeight,
    /// Revolution count and pitch are independent.
    RevolutionsPitch,
    /// Total height and pitch are independent.
    HeightPitch,
    /// Revolution count and radial pitch define a planar spiral.
    Spiral,
}

/// Generated section family stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCoilSection {
    /// Circular section.
    Circular,
    /// Square section.
    Square,
    /// Triangular section pointing away from the axis.
    ExternalTriangle,
    /// Triangular section pointing toward the axis.
    InternalTriangle,
}

/// Radial section placement stored by a Coil parameter scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignCoilSectionPlacement {
    /// Section inside the reference trajectory.
    Inside,
    /// Section centered on the reference trajectory.
    Center,
    /// Section outside the reference trajectory.
    Outside,
}

fn deserialize_coil_secondary_identity<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DesignSecondaryIdentity<u64>>, D::Error> {
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default, deserialize_with = "deserialize_secondary_identity")]
        secondary_identity: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_curve_secondary_identity")]
        curve_secondary_identity: Option<u64>,
    }
    let wire = Wire::deserialize(deserializer)?;
    DesignSecondaryIdentity::from_wire(wire.secondary_identity, wire.curve_secondary_identity)
        .map_err(serde::de::Error::custom)
}

fn deserialize_coil_recipe_design<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ConstructionRecipeDesign<String>>, D::Error> {
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default, deserialize_with = "deserialize_design_id")]
        design_id: Option<String>,
        #[serde(default, deserialize_with = "deserialize_design_selector")]
        design_selector: Option<ConstructionRecipeSelector>,
    }
    let wire = Wire::deserialize(deserializer)?;
    match (wire.design_id, wire.design_selector) {
        (Some(id), selector) => Ok(Some(ConstructionRecipeDesign { id, selector })),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(serde::de::Error::custom(
            "design_selector requires design_id",
        )),
    }
}

/// Selection carrier used by a compact Coil placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignCoilSelection {
    /// Nested entity-selection frame with one or two persistent identities.
    Persistent {
        /// Asset UUID qualifying the persistent selection namespace.
        asset_id: DesignRelaxedGuidText,
        /// Context UUID qualifying the persistent selection namespace.
        context_id: DesignRelaxedGuidText,
        /// Indexed nested record carrying the persistent identity pair.
        identity_record_index: u32,
        /// First persistent identity value.
        primary_identity: u64,
        /// Secondary identity and any dependent curve identity.
        #[serde(flatten, deserialize_with = "deserialize_coil_secondary_identity")]
        secondary: Option<DesignSecondaryIdentity<u64>>,
    },
    /// Face construction recipe carried by a placement selection frame.
    FaceRecipe {
        /// Asset UUID qualifying the recipe selection namespace.
        asset_id: DesignRelaxedGuidText,
        /// Context UUID qualifying the recipe selection namespace.
        context_id: DesignRelaxedGuidText,
        /// Indexed record containing the face recipe.
        recipe_record_index: u32,
        /// Byte offset of the face recipe record header of a located scope.
        #[serde(skip)]
        recipe_record_byte_offset: Option<u64>,
        /// Native construction-recipe arena identity.
        recipe_id: String,
        /// Exact face-recipe family.
        recipe_kind: DesignFaceRecipeKind,
        /// Recipe Design identity and its optional selector.
        #[serde(flatten, deserialize_with = "deserialize_coil_recipe_design")]
        design: Option<ConstructionRecipeDesign<String>>,
    },
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(try_from = "DesignCoilPlacementWire")]
pub(crate) struct DesignCoilPlacement {
    /// First ordered placement-construction reference.
    pub(crate) selection_record_index: u32,
    /// Byte offset of the support selection frame header of a located scope.
    pub(crate) selection_record_byte_offset: Option<u64>,
    /// Dynamic class tag of the support selection frame.
    pub(crate) selection_class_tag: DesignClassTag,
    /// Exact selection semantics carried by the first placement reference.
    pub(crate) selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    pub(crate) transform_record_index: u32,
    /// Byte offset of the frame carrier header of a located scope.
    pub(crate) transform_record_byte_offset: Option<u64>,
    /// Dynamic class tag of the frame carrier.
    pub(crate) transform_class_tag: DesignClassTag,
    /// Explicit matrix; absent for the encoded identity form.
    pub(crate) explicit_transform: Option<Sourced<SketchPlacementMatrix>>,
}

#[cfg(test)]
thread_local! {
    pub(super) static COIL_PLACEMENT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCoilPlacement {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COIL_PLACEMENT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            selection_record_index: self.selection_record_index,
            selection_record_byte_offset: self.selection_record_byte_offset,
            selection_class_tag: self.selection_class_tag.clone(),
            selection: self.selection.clone(),
            transform_record_index: self.transform_record_index,
            transform_record_byte_offset: self.transform_record_byte_offset,
            transform_class_tag: self.transform_class_tag.clone(),
            explicit_transform: self.explicit_transform,
        }
    }
}

#[derive(Serialize)]
struct DesignCoilPlacementRef<'a> {
    selection_record_index: u32,
    selection_class_tag: &'a str,
    selection: &'a DesignCoilSelection,
    transform_record_index: u32,
    transform_class_tag: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    transform: Option<&'a SketchPlacementMatrix>,
}

impl Serialize for DesignCoilPlacement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCoilPlacementRef {
            selection_record_index: self.selection_record_index,
            selection_class_tag: self.selection_class_tag.as_str(),
            selection: &self.selection,
            transform_record_index: self.transform_record_index,
            transform_class_tag: self.transform_class_tag.as_str(),
            transform: self.explicit_transform.as_ref().map(|matrix| &matrix.value),
        }
        .serialize(serializer)
    }
}

impl DesignCoilPlacement {
    /// Row-major local-to-model matrix with translation in source centimetres.
    #[must_use]
    pub(crate) fn transform(&self) -> &SketchPlacementMatrix {
        self.explicit_transform
            .as_ref()
            .map_or(&SketchPlacementMatrix::IDENTITY, |matrix| &matrix.value)
    }
}

/// Exact placement construction carried by a compact Coil scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DesignCoilPlacementWire {
    /// First ordered placement-construction reference.
    selection_record_index: u32,
    /// Dynamic class tag of the support selection frame.
    selection_class_tag: String,
    /// Exact selection semantics carried by the first placement reference.
    selection: DesignCoilSelection,
    /// Second ordered placement-construction reference: the frame carrier.
    transform_record_index: u32,
    /// Dynamic class tag of the frame carrier.
    transform_class_tag: String,
    /// Explicit row-major local-to-model rigid transform; absent for the
    /// encoded identity form. Matrix values are in source centimetres for the
    /// translation column.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_transform"
    )]
    transform: Option<SketchPlacementMatrix>,
}

impl TryFrom<DesignCoilPlacementWire> for DesignCoilPlacement {
    type Error = String;
    fn try_from(wire: DesignCoilPlacementWire) -> Result<Self, Self::Error> {
        let explicit_transform = wire.transform.map(|value| Sourced {
            value,
            offset: None,
        });
        Ok(Self {
            selection_record_index: wire.selection_record_index,
            selection_record_byte_offset: None,
            selection_class_tag: wire.selection_class_tag.try_into()?,
            selection: wire.selection,
            transform_record_index: wire.transform_record_index,
            transform_record_byte_offset: None,
            transform_class_tag: wire.transform_class_tag.try_into()?,
            explicit_transform,
        })
    }
}

#[cfg(test)]
impl From<DesignCoilPlacement> for DesignCoilPlacementWire {
    fn from(record: DesignCoilPlacement) -> Self {
        Self {
            selection_record_index: record.selection_record_index,
            selection_class_tag: record.selection_class_tag.into(),
            selection: record.selection,
            transform_record_index: record.transform_record_index,
            transform_class_tag: record.transform_class_tag.into(),
            transform: record.explicit_transform.map(|matrix| matrix.value),
        }
    }
}

/// Direct rigid placement carried by the long ten-reference Coil form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignCoilTransform {
    /// Row-major local-to-model rigid transform. Translation is in source
    /// centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar of a located scope.
    #[serde(skip)]
    pub(crate) transform_offset: Option<u64>,
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, PartialEq, Default, Deserialize)]
#[serde(try_from = "DesignCoilScopeWire")]
// Field names are the native record serialized keys.
pub(crate) struct DesignCoilScope {
    pub(crate) operation: Option<RecordedValue<DesignExtrudeOperation, Option<u64>>>,
    pub(crate) extent: Option<MaybeRecordedValue<DesignCoilExtent>>,
    pub(crate) section: Option<MaybeRecordedValue<DesignCoilSection>>,
    pub(crate) section_placement: Option<MaybeRecordedValue<DesignCoilSectionPlacement>>,
    pub(crate) clockwise: Option<MaybeRecordedValue<bool>>,
    pub(crate) placement: Option<DesignCoilPlacement>,
    pub(crate) transform: Option<DesignCoilTransform>,
}

#[cfg(test)]
thread_local! {
    pub(super) static COIL_SCOPE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clone for DesignCoilScope {
    fn clone(&self) -> Self {
        #[cfg(test)]
        COIL_SCOPE_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            operation: self.operation,
            extent: self.extent,
            section: self.section,
            section_placement: self.section_placement,
            clockwise: self.clockwise,
            placement: self.placement.clone(),
            transform: self.transform.clone(),
        }
    }
}

#[derive(Serialize)]
struct DesignCoilScopeRef<'a> {
    #[serde(rename = "coil_operation")]
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<DesignExtrudeOperation>,
    #[serde(rename = "coil_extent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    extent: Option<DesignCoilExtent>,
    #[serde(rename = "coil_extent_stored")]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    extent_stored: bool,
    #[serde(rename = "coil_section")]
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<DesignCoilSection>,
    #[serde(rename = "coil_section_stored")]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    section_stored: bool,
    #[serde(rename = "coil_section_placement")]
    #[serde(skip_serializing_if = "Option::is_none")]
    section_placement: Option<DesignCoilSectionPlacement>,
    #[serde(rename = "coil_section_placement_stored")]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    section_placement_stored: bool,
    #[serde(rename = "coil_clockwise")]
    #[serde(skip_serializing_if = "Option::is_none")]
    clockwise: Option<bool>,
    #[serde(rename = "coil_clockwise_stored")]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    clockwise_stored: bool,
    #[serde(rename = "coil_placement")]
    #[serde(skip_serializing_if = "Option::is_none")]
    placement: Option<&'a DesignCoilPlacement>,
    #[serde(rename = "coil_transform")]
    #[serde(skip_serializing_if = "Option::is_none")]
    transform: Option<&'a DesignCoilTransform>,
}

impl Serialize for DesignCoilScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DesignCoilScopeRef {
            operation: self.operation.map(|field| field.value),
            extent: self.extent.map(|field| field.value()),
            extent_stored: self.extent.is_some_and(|field| field.is_stored()),
            section: self.section.map(|field| field.value()),
            section_stored: self.section.is_some_and(|field| field.is_stored()),
            section_placement: self.section_placement.map(|field| field.value()),
            section_placement_stored: self
                .section_placement
                .is_some_and(|field| field.is_stored()),
            clockwise: self.clockwise.map(|field| field.value()),
            clockwise_stored: self.clockwise.is_some_and(|field| field.is_stored()),
            placement: self.placement.as_ref(),
            transform: self.transform.as_ref(),
        }
        .serialize(serializer)
    }
}

/// Coil-specific records carried by a Coil parameter scope.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// Field names are the native record serialized keys.
struct DesignCoilScopeWire {
    /// Coil result operation from the fixed scope prologue.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_operation"
    )]
    #[serde(rename = "coil_operation")]
    operation: Option<DesignExtrudeOperation>,
    /// Coil driving-dimension mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_extent"
    )]
    #[serde(rename = "coil_extent")]
    extent: Option<DesignCoilExtent>,
    /// Whether the form stores the Coil mode enum as a member.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[serde(rename = "coil_extent_stored")]
    extent_stored: bool,
    /// Generated Coil section family.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_section"
    )]
    #[serde(rename = "coil_section")]
    section: Option<DesignCoilSection>,
    /// Whether the form stores the Coil section enum as a member.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[serde(rename = "coil_section_stored")]
    section_stored: bool,
    /// Radial placement of the generated Coil section.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_section_placement"
    )]
    #[serde(rename = "coil_section_placement")]
    section_placement: Option<DesignCoilSectionPlacement>,
    /// Whether the form stores the Coil section-placement enum as a member.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[serde(rename = "coil_section_placement_stored")]
    section_placement_stored: bool,
    /// Whether Coil angular travel is clockwise.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_clockwise"
    )]
    #[serde(rename = "coil_clockwise")]
    clockwise: Option<bool>,
    /// Whether the form stores the Coil direction enum as a member.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[serde(rename = "coil_clockwise_stored")]
    clockwise_stored: bool,
    /// Exact placement construction carried by a compact Coil scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_placement"
    )]
    #[serde(rename = "coil_placement")]
    placement: Option<DesignCoilPlacement>,
    /// Direct rigid placement carried by the long ten-reference Coil form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_coil_transform"
    )]
    #[serde(rename = "coil_transform")]
    transform: Option<DesignCoilTransform>,
}

impl TryFrom<DesignCoilScopeWire> for DesignCoilScope {
    type Error = String;
    fn try_from(wire: DesignCoilScopeWire) -> Result<Self, Self::Error> {
        Ok(Self {
            operation: wire.operation.map(|value| RecordedValue {
                value,
                offset: None,
            }),
            extent: MaybeRecordedValue::from_wire(wire.extent, wire.extent_stored, "coil_extent")?,
            section: MaybeRecordedValue::from_wire(
                wire.section,
                wire.section_stored,
                "coil_section",
            )?,
            section_placement: MaybeRecordedValue::from_wire(
                wire.section_placement,
                wire.section_placement_stored,
                "coil_section_placement",
            )?,
            clockwise: MaybeRecordedValue::from_wire(
                wire.clockwise,
                wire.clockwise_stored,
                "coil_clockwise",
            )?,
            placement: wire.placement,
            transform: wire.transform,
        })
    }
}

#[cfg(test)]
impl From<DesignCoilScope> for DesignCoilScopeWire {
    fn from(value: DesignCoilScope) -> Self {
        Self {
            operation: value.operation.map(|field| field.value),
            extent: value.extent.map(|field| field.value()),
            extent_stored: value.extent.is_some_and(|field| field.is_stored()),
            section: value.section.map(|field| field.value()),
            section_stored: value.section.is_some_and(|field| field.is_stored()),
            section_placement: value.section_placement.map(|field| field.value()),
            section_placement_stored: value
                .section_placement
                .is_some_and(|field| field.is_stored()),
            clockwise: value.clockwise.map(|field| field.value()),
            clockwise_stored: value.clockwise.is_some_and(|field| field.is_stored()),
            placement: value.placement,
            transform: value.transform,
        }
    }
}

impl crate::records::source_offsets::SourceOffsets for DesignCoilScope {
    fn visit_source_offsets(
        &mut self,
        offsets: &mut crate::records::source_offsets::OffsetVisitor<'_>,
    ) {
        if let Some(operation) = &mut self.operation {
            offsets.offset("coil_operation_offset", &mut operation.offset);
        }
        if let Some(slot) = self
            .extent
            .as_mut()
            .and_then(MaybeRecordedValue::offset_mut)
        {
            offsets.offset("coil_extent_offset", slot);
        }
        if let Some(slot) = self
            .section
            .as_mut()
            .and_then(MaybeRecordedValue::offset_mut)
        {
            offsets.offset("coil_section_offset", slot);
        }
        if let Some(slot) = self
            .section_placement
            .as_mut()
            .and_then(MaybeRecordedValue::offset_mut)
        {
            offsets.offset("coil_section_placement_offset", slot);
        }
        if let Some(slot) = self
            .clockwise
            .as_mut()
            .and_then(MaybeRecordedValue::offset_mut)
        {
            offsets.offset("coil_clockwise_offset", slot);
        }
        if let Some(placement) = &mut self.placement {
            offsets.nested("coil_placement", |offsets| {
                offsets.offset(
                    "selection_record_byte_offset",
                    &mut placement.selection_record_byte_offset,
                );
                if let DesignCoilSelection::FaceRecipe {
                    recipe_record_byte_offset,
                    ..
                } = &mut placement.selection
                {
                    offsets.offset(
                        "selection.recipe_record_byte_offset",
                        recipe_record_byte_offset,
                    );
                }
                offsets.offset(
                    "transform_record_byte_offset",
                    &mut placement.transform_record_byte_offset,
                );
                if let Some(transform) = &mut placement.explicit_transform {
                    offsets.sourced("transform_offset", transform);
                }
            });
        }
        if let Some(transform) = &mut self.transform {
            offsets.offset(
                "coil_transform.transform_offset",
                &mut transform.transform_offset,
            );
        }
    }
}

#[cfg(test)]
mod tests;

mod identity_rewrite;
