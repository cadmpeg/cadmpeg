// SPDX-License-Identifier: Apache-2.0
//! Thicken operands, selection properties, and control properties.

use std::collections::HashSet;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{FaceMotion, FaceSelection, FeatureOperation, ParameterValue},
    scalar::Length,
};

use super::{
    parse_enumeration, property, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcFeatureReferenceFamily,
    PropertyParser,
};
use crate::pmdc::{content_header, Cursor, PmDcReference};

pub(super) const SELECTION_TYPE: [u8; 16] = [
    0x68, 0x88, 0x8b, 0xbf, 0xd1, 0x11, 0x05, 0xaf, 0x00, 0x08, 0x47, 0xb0, 0x05, 0x24, 0xdc, 0x09,
];
pub(super) const CONTROL_13_TYPE: [u8; 16] = [
    0x4c, 0x40, 0xb0, 0xa3, 0xac, 0x4b, 0x95, 0x04, 0xe7, 0x19, 0xb2, 0x8a, 0x70, 0xe0, 0xe8, 0x18,
];
pub(super) const CONTROL_14_TYPE: [u8; 16] = [
    0xc1, 0x1c, 0x7b, 0x63, 0xea, 0x4d, 0x84, 0x0b, 0x68, 0xb7, 0x6e, 0x8c, 0x67, 0xbc, 0xbc, 0x48,
];

pub(super) fn property_parser(type_id: [u8; 16]) -> Option<PropertyParser> {
    match type_id {
        SELECTION_TYPE => Some(parse_selection),
        CONTROL_13_TYPE => Some(parse_control_13),
        CONTROL_14_TYPE => Some(parse_control_14),
        _ => None,
    }
}

fn parse_selection(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let faces = cursor.reference("thicken selected faces")?;
    let parameter = cursor.reference("thicken selection parameter")?;
    cursor.finish("thicken selection")?;
    Ok(property(
        version,
        header,
        PmDcFeaturePropertyKind::ThickenSelection { faces, parameter },
    ))
}

fn parse_control_13(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    parse_enumeration(source, version, PmDcFeatureEnumFamily::ThickenControl13)
}

fn parse_control_14(
    _: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcFeaturePropertyPayload, CodecError> {
    parse_enumeration(source, version, PmDcFeatureEnumFamily::ThickenControl14)
}

pub(crate) fn property_kind_matches_class(type_id: &str, kind: &PmDcFeaturePropertyKind) -> bool {
    match type_id {
        "68888bbfd11105af000847b00524dc09" => {
            matches!(kind, PmDcFeaturePropertyKind::ThickenSelection { .. })
        }
        "4c40b0a3ac4b9504e719b28a70e0e818" => matches!(
            kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::ThickenControl13,
                ..
            }
        ),
        "c11c7b63ea4d840b68b76e8c67bcbc48" => matches!(
            kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::ThickenControl14,
                ..
            }
        ),
        _ => !matches!(
            kind,
            PmDcFeaturePropertyKind::ThickenSelection { .. }
                | PmDcFeaturePropertyKind::Enumeration {
                    family: PmDcFeatureEnumFamily::ThickenControl13
                        | PmDcFeatureEnumFamily::ThickenControl14,
                    ..
                }
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Version,
    Slot(usize),
    OffsetControl,
    Result,
}

impl NativeReason {
    pub(super) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let reason = match self {
            Self::Version => "segment version has unresolved construction semantics".to_owned(),
            Self::Slots => "property vector has unresolved arity".to_owned(),
            Self::Slot(slot) => format!("slot {slot} has unresolved construction operands"),
            Self::OffsetControl => {
                "slot 16 is false; its offset construction meaning is unresolved".to_owned()
            }
            Self::Result => "result-body topology is unresolved".to_owned(),
        };
        crate::loss::InventorLossCode::FeatureThickenConstructionNative
            .note(format!("Retained Inventor Thicken natively: {reason}."))
    }
}

pub(crate) fn operation<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a crate::design::PmDcParameter, &'a ParameterValue)>,
    dimension: impl Fn(PmDcReference) -> Option<&'a super::revolve::Dimension>,
    record_type_at: impl Fn(u32) -> Option<&'a str>,
) -> Result<FeatureOperation, NativeReason> {
    if source.properties.references().len() != 18 {
        return Err(NativeReason::Slots);
    }
    let property_at = |slot: usize| property(source.properties.references()[slot]);
    let parameter_at = |slot: usize| parameter(source.properties.references()[slot]);
    for slot in [5, 8, 11] {
        if source.properties.references()[slot].index() != 0 {
            return Err(NativeReason::Slot(slot));
        }
    }
    if !matches!(
        property_at(1).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::Enumeration {
            family: PmDcFeatureEnumFamily::PartOperation,
            type_value: 5,
            value: 3,
        })
    ) {
        return Err(NativeReason::Slot(1));
    }
    let offset = match property_at(2).map(|property| &property.kind) {
        Some(PmDcFeaturePropertyKind::Boolean { value, .. }) => !value,
        _ => return Err(NativeReason::Slot(2)),
    };
    for (slot, expected) in [(3, false), (4, false), (7, false), (10, false), (17, false)] {
        if !matches!(
            property_at(slot).map(|property| &property.kind),
            Some(PmDcFeaturePropertyKind::Boolean { value, .. }) if *value == expected
        ) {
            return Err(NativeReason::Slot(slot));
        }
    }
    match property_at(16).map(|property| &property.kind) {
        Some(PmDcFeaturePropertyKind::Boolean { value: true, .. }) => {}
        Some(PmDcFeaturePropertyKind::Boolean { value: false, .. }) if offset => {
            return Err(NativeReason::OffsetControl);
        }
        _ => return Err(NativeReason::Slot(16)),
    }
    if !matches!(
        property_at(9).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            ..
        })
    ) {
        return Err(NativeReason::Slot(9));
    }
    for (slot, expected) in [
        (13, "4c40b0a3ac4b9504e719b28a70e0e818"),
        (14, "c11c7b63ea4d840b68b76e8c67bcbc48"),
    ] {
        let actual = source.properties.references()[slot]
            .record_ordinal()
            .and_then(&record_type_at);
        if actual != Some(expected) {
            return Err(NativeReason::Slot(slot));
        }
    }
    let (raw_thickness, value) = parameter_at(6).ok_or(NativeReason::Slot(6))?;
    let ParameterValue::Length(thickness) = value else {
        return Err(NativeReason::Slot(6));
    };
    if Length::new(raw_thickness.model_value.get() * 10.0) != Some(*thickness) {
        return Err(NativeReason::Slot(6));
    }
    let (raw_control, control) = parameter_at(12).ok_or(NativeReason::Slot(12))?;
    if !matches!(control, ParameterValue::Real(value) if *value == raw_control.model_value) {
        return Err(NativeReason::Slot(12));
    }
    let selections = property_at(0).ok_or(NativeReason::Slot(0))?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::SelectionList,
        items,
    } = &selections.kind
    else {
        return Err(NativeReason::Slot(0));
    };
    let mut distinct = HashSet::new();
    if items.references().is_empty()
        || items.references().iter().any(|reference| {
            reference
                .record_ordinal()
                .and_then(&record_type_at)
                .is_none()
                || !distinct.insert(reference.index())
        })
    {
        return Err(NativeReason::Slot(0));
    }
    let thickness = cadmpeg_ir::scalar::PositiveLength::try_from(*thickness)
        .map_err(|_| NativeReason::Slot(6))?;
    if offset {
        let distance = offset_distance(source, property, parameter, dimension)?;
        return Ok(FeatureOperation::MoveFace {
            faces: FaceSelection::Native(selections.id()),
            motion: FaceMotion::Offset { distance },
        });
    }
    Ok(FeatureOperation::Thicken {
        faces: FaceSelection::Native(selections.id()),
        thickness: Some(thickness),
        side: None,
    })
}

fn offset_distance<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    parameter: impl Fn(PmDcReference) -> Option<(&'a crate::design::PmDcParameter, &'a ParameterValue)>,
    dimension: impl Fn(PmDcReference) -> Option<&'a super::revolve::Dimension>,
) -> Result<Length, NativeReason> {
    if !matches!(source.save_version_major, 19 | 25) {
        return Err(NativeReason::Version);
    }
    let slots = source.properties.references();
    for (slot, family, expected_type, expected_value) in [
        (13, PmDcFeatureEnumFamily::ThickenControl13, 3, 3),
        (14, PmDcFeatureEnumFamily::ThickenControl14, 1, 0),
    ] {
        if !matches!(property(slots[slot]).map(|p| &p.kind),
            Some(PmDcFeaturePropertyKind::Enumeration { family: actual, type_value, value })
                if *actual == family && *type_value == expected_type && *value == expected_value)
        {
            return Err(NativeReason::Slot(slot));
        }
    }
    let (raw, value) = parameter(slots[12]).ok_or(NativeReason::Slot(12))?;
    if raw.model_value.get() != 10.0
        || !matches!(value, ParameterValue::Real(value) if value.get() == 10.0)
    {
        return Err(NativeReason::Slot(12));
    }
    let members = |reference, family| match &property(reference)?.kind {
        PmDcFeaturePropertyKind::References {
            family: actual,
            items,
        } if *actual == family => Some(items.references()),
        _ => None,
    };
    let selection = members(slots[0], PmDcFeatureReferenceFamily::SelectionList)
        .ok_or(NativeReason::Slot(0))?;
    let [selection] = selection else {
        return Err(NativeReason::Slot(0));
    };
    let Some(PmDcFeaturePropertyKind::ThickenSelection {
        faces,
        parameter: selected_parameter,
    }) = property(*selection).map(|p| &p.kind)
    else {
        return Err(NativeReason::Slot(0));
    };
    let faces =
        members(*faces, PmDcFeatureReferenceFamily::FaceCollection).ok_or(NativeReason::Slot(0))?;
    let mut indices = HashSet::new();
    let mut references = HashSet::new();
    if faces.is_empty()
        || faces.iter().any(|reference| {
            if !references.insert(reference.index()) {
                return true;
            }
            let Some(PmDcFeaturePropertyKind::FaceItem { index_references }) =
                property(*reference).map(|p| &p.kind)
            else {
                return true;
            };
            let [index] = index_references.values() else {
                return true;
            };
            index_references.marker() != 2
                || *index == 0
                || !indices.insert(*index)
                || index_references.metadata()
                    != Some(&crate::pmdc::PmDcListMetadata::U32([1, 0x1000_0000]))
        })
    {
        return Err(NativeReason::Slot(0));
    }
    let dimensions = members(slots[9], PmDcFeatureReferenceFamily::FeatureDimensions)
        .ok_or(NativeReason::Slot(9))?;
    let [reference] = dimensions else {
        return Err(NativeReason::Slot(9));
    };
    let dimension = dimension(*reference).ok_or(NativeReason::Slot(9))?;
    if dimension.parameter.index() != selected_parameter.index() {
        return Err(NativeReason::Slot(9));
    }
    let (raw, value) = parameter(dimension.parameter).ok_or(NativeReason::Slot(9))?;
    let ParameterValue::Length(distance) = value else {
        return Err(NativeReason::Slot(9));
    };
    if distance.get() <= 0.0 || Length::new(raw.model_value.get() * 10.0) != Some(*distance) {
        return Err(NativeReason::Slot(9));
    }
    let results = members(slots[15], PmDcFeatureReferenceFamily::ObjectCollection)
        .ok_or(NativeReason::Slot(15))?;
    let [result] = results else {
        return Err(NativeReason::Slot(15));
    };
    if !matches!(
        property(*result).map(|p| &p.kind),
        Some(PmDcFeaturePropertyKind::SurfaceBody { .. })
    ) {
        return Err(NativeReason::Slot(15));
    }
    Ok(*distance)
}

#[cfg(test)]
mod tests;
