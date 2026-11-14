use super::parameter_scope::ParameterScopeSources;
use super::sheet_metal_rule::{local, rule_values, settings_rule, typed, Types, SETTINGS_GUID};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::native_stream;
use crate::layout::{
    sheet_metal_contour_compact_carrier as carrier_layout,
    sheet_metal_contour_short_prefix as prefix, sheet_metal_contour_vector_owner as vector_layout,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::sheet_metal::contour::{
    DesignContourFlangeOperation, DesignContourRule, DesignContourTravel,
};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::{features::FeatureDirection3, math::Vector3, scalar::PositiveReal};

const CONTOUR_GUID: &str = "7AFA0A65-B53C-472C-ACA4-5C7C0A65001C";
const CARRIER_GUID: &str = "CAA55628-037B-4D01-9C74-26118F70B63E";
const EPS_DIRECTION_SQUARED_NORM: f64 = 1.0e-9;

fn slot(
    frame: View<'_>,
    scope: &DesignParameterScope,
    types: &Types<'_>,
    guid: &str,
    versions: &[u32],
) -> Option<(Located<u32>, usize)> {
    let mut result = None;
    for at in frame.position()..usize::try_from(scope.reference_count_offset()).ok()? {
        let mut view = frame;
        view.seek(at)?;
        let Some(reference) = local(&mut view, types, Some(guid)) else {
            continue;
        };
        if !versions
            .iter()
            .any(|version| typed(types, reference.value, guid, *version))
            || !scope
                .reference_members()
                .values()
                .any(|value| *value == reference.value)
        {
            continue;
        }
        if result.replace((reference, view.position())).is_some() {
            return None;
        }
    }
    result
}

fn distance_owner(
    scope: &DesignParameterScope,
    sources: &ParameterScopeSources<'_>,
    types: &Types<'_>,
    index: u32,
) -> Option<bool> {
    let stream = native_stream(&scope.id);
    let mut definitions = sources
        .parameters
        .iter()
        .filter(|parameter| {
            native_stream(&parameter.id) == stream && parameter.owner_record_index() == Some(index)
        })
        .peekable();
    if definitions.peek().is_some() {
        return definitions
            .all(|parameter| parameter.source_kind() == "AlongDistance")
            .then_some(false);
    }
    if sources.parameter_owners.iter().any(|owner| {
        owner.record_index() == index
            && owner.scope_record_index() == scope.record_index
            && native_stream(owner.id()) == stream
            && !sources.parameters.iter().any(|parameter| {
                native_stream(&parameter.id) == stream
                    && parameter.record_index == owner.parameter_record_index()
            })
    }) {
        return Some(false);
    }
    types
        .get(&u64::from(index))
        .is_some_and(|(guid, _)| guid.eq_ignore_ascii_case("D91D429C-1BB5-4640-B49F-69AF14F463E9"))
        .then_some(true)
}

fn travel(
    frame: View<'_>,
    scope: &DesignParameterScope,
    types: &Types<'_>,
    sources: &ParameterScopeSources<'_>,
    end: u64,
) -> Option<DesignContourTravel> {
    let mut result = None;
    for at in frame
        .position()
        .checked_add(vector_layout::OWNER_REFERENCE_PREFIX)?
        ..usize::try_from(end).ok()?
    {
        let mut owner_view = frame;
        owner_view.seek(at)?;
        let Some(owner) = local(&mut owner_view, types, None) else {
            continue;
        };
        if owner_view.position() > usize::try_from(end).ok()?
            || !scope
                .reference_members()
                .values()
                .any(|value| *value == owner.value)
        {
            continue;
        }
        let Some(owner_role_assumed) = distance_owner(scope, sources, types, owner.value) else {
            continue;
        };
        let offset = at.checked_sub(vector_layout::OWNER_REFERENCE_PREFIX)?;
        let mut vector = frame;
        vector.seek(offset)?;
        let value = Vector3::new(vector.f64_le()?, vector.f64_le()?, vector.f64_le()?);
        if vector.u8()? != 0 || (value.dot(value) - 1.0).abs() > EPS_DIRECTION_SQUARED_NORM {
            continue;
        }
        let Some(value) = FeatureDirection3::new(value) else {
            continue;
        };
        if result
            .replace(DesignContourTravel {
                owner,
                owner_role_assumed,
                direction: Located {
                    value,
                    offset: u64_from_index(offset),
                },
            })
            .is_some()
        {
            return None;
        }
    }
    result
}

fn operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &Types<'_>,
    sources: &ParameterScopeSources<'_>,
) -> Option<DesignContourFlangeOperation> {
    if scope.kind() != DesignFeatureKind::ContourFlange
        || ![3, 4, 6]
            .into_iter()
            .any(|version| typed(types, scope.record_index, CONTOUR_GUID, version))
    {
        return None;
    }
    let mut frame = View::over_retained(bytes).child(
        usize::try_from(scope.byte_offset()).ok()?,
        usize::try_from(scope.reference_count_offset()).ok()?,
    )?;
    frame.skip(prefix::ZERO_RUN)?;
    if frame.take(prefix::EXTENSION - prefix::ZERO_RUN)? != [0; 9] {
        return None;
    }
    match frame.u8()? {
        0 => {}
        1 if frame.u32_le()? == 0 => {}
        _ => return None,
    }
    if frame.u8()? != 1 {
        return None;
    }
    let operation = Located {
        offset: u64_from_index(frame.position()),
        value: frame.u32_le()?,
    };
    let carrier_slot = slot(frame, scope, types, CARRIER_GUID, &[0]);
    let settings_slot = slot(frame, scope, types, SETTINGS_GUID, &[1, 2, 3]);
    if settings_slot
        .as_ref()
        .zip(carrier_slot.as_ref())
        .is_some_and(|((_, after_settings), (carrier, _))| {
            u64_from_index(*after_settings) > carrier.offset
        })
    {
        return None;
    }
    let profile_control = carrier_slot.as_ref().and_then(|(carrier, _)| {
        let mut control = frame;
        control.seek(
            usize::try_from(carrier.offset)
                .ok()?
                .checked_sub(carrier_layout::CARRIER_REFERENCE)?,
        )?;
        let value = Located {
            offset: u64_from_index(control.position()),
            value: control.u32_le()?,
        };
        (control.u8()? == 0).then_some(value)
    });
    let inline_thickness = carrier_slot.as_ref().and_then(|(_, after_carrier)| {
        let mut thickness = frame;
        thickness.seek(*after_carrier)?;
        Some(Located {
            offset: u64_from_index(*after_carrier),
            value: PositiveReal::new(thickness.f64_le()?)?,
        })
    });
    let carrier = carrier_slot.map(|(carrier, _)| carrier);
    let settings = settings_slot.map(|(settings, _)| settings);
    let travel = travel(
        frame,
        scope,
        types,
        sources,
        settings
            .as_ref()
            .map_or(scope.reference_count_offset(), |settings| settings.offset),
    );
    let rule = settings
        .as_ref()
        .and_then(|settings| {
            settings_rule(
                bytes,
                records,
                types,
                settings.value,
                scope.record_index,
                CONTOUR_GUID,
            )
        })
        .and_then(|(record_index, reference_assumed)| {
            let values = rule_values(bytes, records, types, record_index)?;
            Some(DesignContourRule {
                record_index,
                reference_assumed,
                first_value: values.first_value,
                thickness: values.thickness,
                material_scalar: values.material_scalar,
            })
        });
    Some(DesignContourFlangeOperation {
        operation,
        carrier,
        settings,
        profile_control,
        inline_thickness,
        travel,
        rule,
    })
}

pub(super) fn bind(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    sources: &ParameterScopeSources<'_>,
    scopes: &mut [DesignParameterScope],
) {
    for scope in scopes {
        if scope.kind() != DesignFeatureKind::ContourFlange {
            continue;
        }
        let value = operation(bytes, records, scope, types, sources);
        if let DesignScopePayloadMut::ContourFlange(slot) = scope.payload_mut() {
            *slot = value;
        }
    }
}

#[cfg(test)]
mod tests;
