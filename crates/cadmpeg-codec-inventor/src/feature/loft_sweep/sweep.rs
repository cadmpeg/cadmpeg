// SPDX-License-Identifier: Apache-2.0
//! Circular sweep operands and first-refusal losses.

use cadmpeg_ir::features::{
    FeatureDefinition, FeatureOperation, ParameterValue, SolidSweepOperation, SweepSection,
    SweepShape,
};

use crate::design::PmDcParameter;
use crate::feature::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureLabel, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily, ProjectionIndex,
};
use crate::pmdc::{type_id_string, PmDcReference};

use super::{projection, SWEEP_MODE_TYPE, SWEEP_ORIENTATION_TYPE, SWEEP_SCALING_TYPE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Slot(usize),
    ResultCollection,
    Angle,
    Dimension,
    SectionSelection,
    CircularSection,
    PathSelection,
    Path,
    ResultTopology,
    ResultIdentity,
}

impl NativeReason {
    pub(crate) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let reason = match self {
            Self::Slots => "property vector lacks the sweep operand slots".to_owned(),
            Self::Slot(slot) => format!("property slot {slot} differs from the circular sweep form"),
            Self::ResultCollection => "slot 15 does not select one result body".to_owned(),
            Self::Angle => "slot 3 does not resolve to zero nominal and model angular values".to_owned(),
            Self::Dimension => "slot 7 dimension does not select the slot 3 parameter".to_owned(),
            Self::SectionSelection => "slot 0 does not select one zero-control profile".to_owned(),
            Self::CircularSection => "section does not resolve to one complete consumed circular profile".to_owned(),
            Self::PathSelection => "slot 1 does not select a zero-control path profile".to_owned(),
            Self::Path => "path does not resolve to one complete ordered open wire".to_owned(),
            Self::ResultTopology => "complete generator-owned result does not prove constant-radius circular sweep construction".to_owned(),
            Self::ResultIdentity => "result body identity does not resolve".to_owned(),
        };
        crate::loss::InventorLossCode::FeatureSweepControlsUnresolved
            .note(format!("Retained Inventor sweep natively: {reason}."))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Operands {
    section: PmDcReference,
    path: PmDcReference,
}

pub(crate) fn operands<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
    dimension: impl Fn(PmDcReference) -> Option<&'a crate::feature::revolve::Dimension>,
) -> Result<Operands, NativeReason> {
    let slots = source.properties.references();
    if source.properties.marker() != 2 || slots.len() < 16 {
        return Err(NativeReason::Slots);
    }
    let result = property(slots[15]).ok_or(NativeReason::ResultCollection)?;
    if result.identity.type_id.as_str() != type_id_string(crate::feature::OBJECT_COLLECTION_TYPE)
        || !matches!(&result.kind, PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection, items,
    } if items.references().len() == 1 && items.references()[0].index() != 0)
    {
        return Err(NativeReason::ResultCollection);
    }
    for (slot, reference) in slots.iter().enumerate() {
        if reference.index() != 0 && ![0, 1, 2, 3, 4, 7, 8, 9, 10, 15].contains(&slot) {
            return Err(NativeReason::Slot(slot));
        }
    }
    for (slot, type_id, family, expected_type, expected_value) in [
        (
            2,
            crate::feature::PART_OPERATION_TYPE,
            PmDcFeatureEnumFamily::PartOperation,
            5,
            1,
        ),
        (
            4,
            crate::feature::EXTENT_TYPE,
            PmDcFeatureEnumFamily::Extent,
            if source.save_version_major >= crate::feature::EXTENT_TYPE_12_FIRST_MAJOR {
                12
            } else {
                11
            },
            3,
        ),
    ] {
        let p = property(slots[slot]).ok_or(NativeReason::Slot(slot))?;
        if p.identity.type_id.as_str() != type_id_string(type_id)
            || !matches!(p.kind, PmDcFeaturePropertyKind::Enumeration { family: actual, type_value, value }
                if actual == family && type_value == expected_type && value == expected_value)
        {
            return Err(NativeReason::Slot(slot));
        }
    }
    for (slot, type_id, expected_type) in [
        (8, SWEEP_MODE_TYPE, 3),
        (9, SWEEP_ORIENTATION_TYPE, 2),
        (10, SWEEP_SCALING_TYPE, 2),
    ] {
        if !projection::zero_control(
            property(slots[slot]).ok_or(NativeReason::Slot(slot))?,
            type_id,
            expected_type,
        ) {
            return Err(NativeReason::Slot(slot));
        }
    }
    let (raw, value) = parameter(slots[3]).ok_or(NativeReason::Angle)?;
    if raw.identity.type_id.as_str() != type_id_string(crate::design::PARAMETER_FULL_TYPE)
        || raw.nominal_value.get() != 0.0
        || raw.model_value.get() != 0.0
        || !matches!(value, ParameterValue::Angle(value) if value.get() == 0.0)
    {
        return Err(NativeReason::Angle);
    }
    if slots[7].index() != 0 {
        let p = property(slots[7]).ok_or(NativeReason::Dimension)?;
        if p.identity.type_id.as_str() != type_id_string(crate::feature::FEATURE_DIMENSIONS_TYPE) {
            return Err(NativeReason::Dimension);
        }
        let PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            items,
        } = &p.kind
        else {
            return Err(NativeReason::Dimension);
        };
        let [item] = items.references() else {
            return Err(NativeReason::Dimension);
        };
        if dimension(*item)
            .ok_or(NativeReason::Dimension)?
            .parameter
            .index()
            != slots[3].index()
        {
            return Err(NativeReason::Dimension);
        }
    }
    let boundary = property(slots[0]).ok_or(NativeReason::SectionSelection)?;
    if boundary.identity.type_id.as_str() != type_id_string(crate::feature::BOUNDARY_PATCH_TYPE) {
        return Err(NativeReason::SectionSelection);
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BoundaryPatch,
        items,
    } = &boundary.kind
    else {
        return Err(NativeReason::SectionSelection);
    };
    let [selection] = items.references() else {
        return Err(NativeReason::SectionSelection);
    };
    let section = selection_link(property(*selection).ok_or(NativeReason::SectionSelection)?)
        .ok_or(NativeReason::SectionSelection)?;
    let path = selection_link(property(slots[1]).ok_or(NativeReason::PathSelection)?)
        .ok_or(NativeReason::PathSelection)?;
    Ok(Operands { section, path })
}

fn selection_link(property: &PmDcFeatureProperty) -> Option<PmDcReference> {
    if ![
        crate::feature::PROFILE_SELECTION_TYPE,
        crate::feature::PLATE_PROFILE_SELECTION_TYPE,
    ]
    .into_iter()
    .any(|type_id| property.identity.type_id.as_str() == type_id_string(type_id))
    {
        return None;
    }
    match property.kind {
        PmDcFeaturePropertyKind::ProfileSelection {
            entity_link,
            value: 0,
        } if entity_link.index() != 0 => Some(entity_link),
        _ => None,
    }
}

pub(super) fn project(
    source: &PmDcFeature,
    label: &PmDcFeatureLabel,
    index: &ProjectionIndex<'_>,
) -> Result<FeatureOperation, NativeReason> {
    let token = source.identity.segment_token.as_str();
    let links = operands(
        source,
        |r| crate::feature::resolve_property(token, r.index(), index),
        |r| {
            let raw = index
                .parameters
                .get(&(token, r.record_ordinal()?))?
                .as_ref()?;
            let value = index
                .parameter_values
                .get(raw.id().as_str())?
                .as_ref()?
                .value
                .as_ref()?;
            Some((*raw, value))
        },
        |r| {
            index
                .dimensions
                .get(&(token, r.record_ordinal()?))?
                .as_ref()
                .copied()
        },
    )?;
    let sketches = projection::participant_sketches(source, label, index);
    let mut circles = sketches.iter().filter_map(|(ordinal, sketch)| {
        index
            .profiles
            .circle(token, *ordinal, sketch, links.section.record_ordinal()?)
    });
    let (profile, radius, name) = circles.next().ok_or(NativeReason::CircularSection)?;
    if circles.next().is_some() {
        return Err(NativeReason::CircularSection);
    }
    let path_property =
        crate::feature::slot_property(source, 1, index).ok_or(NativeReason::PathSelection)?;
    let path = index
        .profiles
        .selected_path(
            token,
            links.path.record_ordinal().ok_or(NativeReason::Path)?,
            path_property.id(),
            &sketches,
        )
        .ok_or(NativeReason::Path)?;
    projection::evidence::sweep(
        index.result_model.ok_or(NativeReason::ResultTopology)?,
        source.header.source_index,
        radius,
        name,
        &path.names,
        path.endpoints,
    )
    .ok_or(NativeReason::ResultTopology)?;
    Ok(FeatureOperation::Sweep {
        shape: SweepShape::Solid {
            op: SolidSweepOperation::NewBody,
            section: SweepSection::Profile(profile),
            sections: Vec::new(),
        },
        path: Some(path.reference),
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    })
}

pub(crate) fn validates<'a>(
    source: &PmDcFeature,
    definition: &FeatureDefinition,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a PmDcParameter, &'a ParameterValue)>,
    dimension: impl Fn(PmDcReference) -> Option<&'a crate::feature::revolve::Dimension>,
) -> bool {
    matches!(definition, FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape: SweepShape::Solid { op: SolidSweepOperation::NewBody, section: SweepSection::Profile(_), sections },
        path: Some(_), orientation: None, transition: None, transformation: None,
        path_tangent: false, linearize: false, twist: None, path_extent: None, guide_rail: None,
        taper: None, scale: None, allow_multi_profile_faces: None,
    }) if sections.is_empty())
        && operands(source, property, parameter, dimension).is_ok()
}

#[cfg(test)]
mod tests;
