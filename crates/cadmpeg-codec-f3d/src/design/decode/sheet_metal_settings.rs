// SPDX-License-Identifier: Apache-2.0
//! Decode MetaStream-bounded common sheet-metal settings frames.

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::layout::sheet_metal_settings_prefix as settings_prefix;
use crate::metastream::{primary_record_frames, PrimaryRecordFrame};
use crate::records::sheet_metal_settings::{
    DesignSheetMetalSettings, DesignSheetMetalSettingsInputs, DesignSheetMetalSettingsWire,
    V3RuleReferenceTail, PAIRED_GUID, PRESENTATION_GUID, RULE_GUID, SETTINGS_GUID,
};
use cadmpeg_core::container::{ContainerEntry, ContainerRole};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Settings also occur in the flat-pattern asset's Design segment. This does
/// not admit that asset to feature or parameter decoding.
pub(super) fn is_settings_stream(
    scan: &ContainerScan,
    entry: &ContainerEntry,
    role: ContainerRole,
) -> bool {
    if scan.is_design_stream(entry, role) {
        return true;
    }
    let member = match role {
        ContainerRole::Metastream => "MetaStream.dat",
        ContainerRole::Bulkstream => "BulkStream.dat",
        _ => return false,
    };
    entry.role == role
        && entry.name.strip_prefix("FlatPatternAssetSubType/Design1/") == Some(member)
}

pub(super) fn has_supported_settings(meta: &crate::metastream::MetaStream) -> bool {
    meta.types.iter().any(|row| {
        row.type_guid.as_str().eq_ignore_ascii_case(SETTINGS_GUID)
            && matches!(row.version, 2 | 3)
            && row.entities.values().len() != 0
    })
}

pub(crate) fn decode_settings(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignSheetMetalSettings>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| is_settings_stream(scan, entry, ContainerRole::Bulkstream))
    {
        let Some(meta) = super::meta::metadata_for_bulk_stream(ctx, scan, &entry.name)? else {
            continue;
        };
        if !has_supported_settings(&meta) {
            continue;
        }
        let mut by_entity = HashMap::new();
        for row in &meta.types {
            for &id in row.entities.values() {
                if by_entity.contains_key(&id) {
                    return Err(CodecError::Malformed(
                        "F3D settings type registration repeats an entity ID".into(),
                    ));
                }
                ctx.insert_hash_map(
                    &mut by_entity,
                    id,
                    (row.type_guid.as_str(), row.version),
                    "index F3D settings entity types",
                )?;
            }
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let frames = primary_record_frames(ctx, &meta, bytes.len())?;
        let primary = ctx.collect_hash_set(
            frames.iter().map(|frame| frame.entity_id),
            "index F3D settings primary references",
        )?;
        for frame in frames {
            let Some((guid, version)) = by_entity.get(&frame.entity_id) else {
                continue;
            };
            if !guid.eq_ignore_ascii_case(SETTINGS_GUID) || !matches!(version, 2 | 3) {
                continue;
            }
            let Some(fields) =
                parse_settings(bytes, frame, *version, &primary, &by_entity, &meta.types)
            else {
                return Err(CodecError::Malformed(
                    "F3D sheet-metal settings frame or typed link is invalid".into(),
                ));
            };
            let id = super::text::design_record_id_charged(
                ctx,
                &entry.name,
                ":design-sheet-metal-settings#",
                u64_from_index(frame.start),
                "F3D sheet-metal settings identity",
            )?;
            let record = DesignSheetMetalSettings::try_from(DesignSheetMetalSettingsWire {
                id,
                record_index: frame.entity_id,
                byte_offset: u64_from_index(frame.start),
                paired_byte_offset: u64_from_index(frame.member_end),
                mask: fields.mask,
                inputs: fields.inputs,
                rule_record_index: fields.rule,
                presentation_record_index: fields.presentation,
                owner_record_index: fields.owner,
                context_record_index: fields.context,
            })
            .map_err(CodecError::Malformed)?;
            ctx.push_vec(&mut out, record, "retain F3D sheet-metal settings")?;
        }
    }
    Ok(out)
}

struct SettingsFields {
    mask: u32,
    inputs: DesignSheetMetalSettingsInputs,
    rule: u64,
    presentation: u64,
    owner: u64,
    context: u64,
}

fn header<'a>(view: &mut View<'a>, record_index: u64) -> Option<&'a str> {
    let (tag, length) = lp_ascii_filtered_view(view.unread(), 0, 1..=10, u8::is_ascii_digit)?;
    view.skip(length)?;
    (u64::from(view.u32_le()?) == record_index).then_some(tag)
}
fn local(view: &mut View<'_>) -> Result<Option<u64>, ()> {
    match view.u8().ok_or(())? {
        0 => Ok(None),
        1 => {
            let value = view.u64_le().ok_or(())?;
            (view.take(2).ok_or(())? == [0, 0])
                .then_some(Some(value))
                .ok_or(())
        }
        _ => Err(()),
    }
}
fn parse_settings(
    bytes: &[u8],
    frame: PrimaryRecordFrame,
    version: u32,
    primary: &HashSet<u64>,
    types: &HashMap<u64, (&str, u32)>,
    registrations: &[crate::records::entity_header::SegmentTypeData],
) -> Option<SettingsFields> {
    let mut view = View::over_retained(bytes.get(frame.start..frame.end)?);
    let primary_class = header(&mut view, frame.entity_id)?;
    let ordinal = primary_class.parse::<usize>().ok()?.checked_sub(256)?;
    let registration = registrations.get(ordinal)?;
    if !registration
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(SETTINGS_GUID)
        || registration.version != version
    {
        return None;
    }
    if view.take(settings_prefix::OVERRIDE_MASK)? != [0; settings_prefix::OVERRIDE_MASK] {
        return None;
    }
    let mask = view.u32_le()?;
    let (inputs, rule) = match version {
        2 => {
            let mut slots = [None; 7];
            for slot in &mut slots {
                *slot = local(&mut view).ok()?;
            }
            let rule = local(&mut view).ok()??;
            if view.take(4)? != [0; 4] {
                return None;
            }
            (DesignSheetMetalSettingsInputs::V2 { slots }, rule)
        }
        3 => {
            let mut slots = [None; 9];
            for slot in &mut slots {
                *slot = local(&mut view).ok()?;
            }
            let rule = local(&mut view).ok()??;
            let rule_reference_tail = match view.u8()? {
                0 if view.u8()? == 0 => V3RuleReferenceTail::ZeroPair,
                1 if view.take(2)? == [0, 0] => V3RuleReferenceTail::OneZeroPair,
                _ => return None,
            };
            (
                DesignSheetMetalSettingsInputs::V3 {
                    slots,
                    rule_reference_tail,
                },
                rule,
            )
        }
        _ => return None,
    };
    if inputs
        .slots()
        .iter()
        .flatten()
        .any(|id| !primary.contains(id))
    {
        return None;
    }
    if !primary.contains(&rule) || !types.get(&rule)?.0.eq_ignore_ascii_case(RULE_GUID) {
        return None;
    }
    let presentation = local(&mut view).ok()??;
    if !primary.contains(&presentation)
        || !types
            .get(&presentation)?
            .0
            .eq_ignore_ascii_case(PRESENTATION_GUID)
        || view.u8()? != 0
    {
        return None;
    }
    let owner = local(&mut view).ok()??;
    if !primary.contains(&owner) || frame.start.checked_add(view.position())? != frame.member_end {
        return None;
    }
    let paired_class = header(&mut view, frame.entity_id)?;
    let ordinal = paired_class.parse::<usize>().ok()?.checked_sub(256)?;
    if !registrations
        .get(ordinal)?
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(PAIRED_GUID)
    {
        return None;
    }
    if view.take(8)? != [0; 8] {
        return None;
    }
    let context = local(&mut view).ok()??;
    if !primary.contains(&context) || view.remaining() != 0 {
        return None;
    }
    Some(SettingsFields {
        mask,
        inputs,
        rule,
        presentation,
        owner,
        context,
    })
}

#[cfg(test)]
mod tests;
