use crate::bytes::{lp_utf16_bounded, take_reference};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::sheet_metal_fold_scope_prefix as prefix;
use crate::layout::sheet_metal_unfold_refold_group as group_layout;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::sheet_metal::fold::{DesignRefoldOperation, DesignUnfoldOperation};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::HashMap;

type Types<'a> = HashMap<u64, (&'a str, u32)>;
const UNFOLD_GUID: &str = "EC187027-FF97-48A3-8031-F923807E630B";
const REFOLD_GUID: &str = "483D584F-29AA-46DA-ABA2-F139B0ED8100";
const GROUP_GUID: &str = "99F375B3-8331-47DE-9E99-9A0882F2ED29";
const SELECTION_GUID: &str = "2CA5A1CD-C99B-4C9B-91AF-57989148E841";
const WRAPPER_GUID: &str = "2E3AC990-483D-4A55-8662-C064B976A957";
const SETTINGS_GUID: &str = "E75C86D6-76D8-4F1F-BC16-AB8CB74958B0";
const RULE_GUID: &str = "B42ED662-8999-4C1B-A744-759D1D446D55";
const PRESENTATION_GUID: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";

fn typed(types: &Types<'_>, index: u32, guid: &str, version: u32) -> bool {
    types
        .get(&u64::from(index))
        .is_some_and(|(found, v)| found.eq_ignore_ascii_case(guid) && *v == version)
}

fn local(view: &mut View<'_>, types: &Types<'_>, expected: Option<&str>) -> Option<Located<u32>> {
    let offset = u64_from_index(view.position());
    let mut at = 0;
    let reference = take_reference(view.unread(), &mut at)?;
    let (target, inline) = reference.local()?;
    let target = u32::try_from(target).ok()?;
    if let Some(inline) = inline {
        if expected.is_some_and(|guid| !guid.eq_ignore_ascii_case(inline))
            || types
                .get(&u64::from(target))
                .is_some_and(|(guid, _)| !guid.eq_ignore_ascii_case(inline))
        {
            return None;
        }
    }
    if expected.is_some_and(|expected| {
        types
            .get(&u64::from(target))
            .is_some_and(|(guid, _)| !guid.eq_ignore_ascii_case(expected))
    }) {
        return None;
    }
    view.skip(at)?;
    Some(Located {
        value: target,
        offset,
    })
}

fn scope_prefix<'a>(bytes: &'a [u8], scope: &DesignParameterScope) -> Option<View<'a>> {
    let mut view = View::over_retained(bytes).child(
        usize::try_from(scope.byte_offset()).ok()?,
        usize::try_from(scope.paired_byte_offset()).ok()?,
    )?;
    view.skip(prefix::ZERO_RUN)?;
    if view
        .take(prefix::EXTENSION - prefix::ZERO_RUN)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return None;
    }
    match view.u8()? {
        0 => {}
        1 if view.u32_le()? == 0 => {}
        _ => return None,
    }
    Some(view)
}

fn unfold(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &Types<'_>,
) -> Option<DesignUnfoldOperation> {
    if !typed(types, scope.record_index, UNFOLD_GUID, 0) {
        return None;
    }
    let mut view = scope_prefix(bytes, scope)?;
    let control = Located {
        offset: u64_from_index(view.position()),
        value: view.u8()?,
    };
    let bend_group = local(&mut view, types, Some(SELECTION_GUID))?;
    let settings = local(&mut view, types, Some(SETTINGS_GUID))?;
    let wrapper = local(&mut view, types, Some(WRAPPER_GUID))?;
    let stationary_group = local(&mut view, types, Some(SELECTION_GUID))?;
    if [bend_group.value, settings.value, stationary_group.value]
        .iter()
        .any(|id| !scope.reference_members().values().any(|value| value == id))
    {
        return None;
    }
    let rule_record_index =
        settings_rule(bytes, records, types, settings.value, scope.record_index);
    let rule_material_scalar =
        rule_record_index.and_then(|rule| material_scalar(bytes, records, types, rule));
    Some(DesignUnfoldOperation {
        control,
        prefix_end: u64_from_index(view.position()),
        bend_group,
        settings,
        wrapper,
        stationary_group,
        rule_record_index,
        rule_material_scalar,
    })
}

fn settings_rule(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    index: u32,
    owner: u32,
) -> Option<u32> {
    let (guid, version) = *types.get(&u64::from(index))?;
    if !guid.eq_ignore_ascii_case(SETTINGS_GUID) {
        return None;
    }
    let slots = match version {
        1 => 6,
        2 => 7,
        3 => 9,
        _ => return None,
    };
    let mut result = None;
    for (start, end) in records.frames(index) {
        let mut view = View::over_retained(bytes).child(start, end)?;
        view.skip(11)?;
        if view.take(10)?.iter().any(|byte| *byte != 0) {
            continue;
        }
        view.u32_le()?;
        let parsed = (|| {
            for _ in 0..slots {
                if view.unread().first() == Some(&0) {
                    view.skip(1)?;
                } else {
                    local(&mut view, types, None)?;
                }
            }
            let rule = local(&mut view, types, Some(RULE_GUID))?.value;
            if version == 3 {
                match view.u8()? {
                    0 if view.u8()? == 0 => {}
                    1 if view.take(2)? == [0, 0] => {}
                    _ => return None,
                }
            } else if view.take(4)? != [0; 4] {
                return None;
            }
            local(&mut view, types, Some(PRESENTATION_GUID))?;
            if view.u8()? != 0 || local(&mut view, types, None)?.value != owner || !view.is_empty()
            {
                return None;
            }
            Some(rule)
        })();
        if let Some(rule) = parsed {
            if result.replace(rule).is_some() {
                return None;
            }
        }
    }
    result
}

fn material_scalar(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    index: u32,
) -> Option<Located<FiniteReal>> {
    let (guid, version) = *types.get(&u64::from(index))?;
    if !guid.eq_ignore_ascii_case(RULE_GUID) || !matches!(version, 0 | 1) {
        return None;
    }
    let mut result = None;
    for (start, end) in records.frames(index) {
        let frame = View::over_retained(bytes).child(start, end)?;
        let mut view = frame;
        view.skip(11)?;
        if view.take(10)? != [0; 10] {
            continue;
        }
        if version == 0 {
            let count = view.u32_le()?;
            view.counted(u64::from(count), 11)?;
            for _ in 0..count {
                local(&mut view, types, Some(SETTINGS_GUID))?;
            }
        }
        view.u32_le()?;
        let (name, after_name) = lp_utf16_bounded(bytes, view.position(), 1..=64)?;
        if name != "Thickness" || after_name > end {
            continue;
        }
        view.seek(after_name)?;
        let thickness = view.f64_le()?;
        if !thickness.is_finite() || thickness <= 0.0 {
            continue;
        }
        for at in view.position()..end {
            let Some((material, after_material)) = lp_utf16_bounded(bytes, at, 1..=128) else {
                continue;
            };
            if material.is_empty() || after_material > end {
                continue;
            }
            let Some((name, after_name)) = lp_utf16_bounded(bytes, after_material, 1..=64) else {
                continue;
            };
            if name != "Thickness" || after_name.checked_add(8)? > end {
                continue;
            }
            let mut scalar_view = frame;
            scalar_view.seek(after_name)?;
            if scalar_view.f64_le()? != thickness {
                continue;
            }
            let offset = at.checked_sub(8)?;
            scalar_view.seek(offset)?;
            let scalar = FiniteReal::new(scalar_view.f64_le()?)?;
            if result
                .replace(Located {
                    value: scalar,
                    offset: u64_from_index(offset),
                })
                .is_some()
            {
                return None;
            }
        }
    }
    result
}

fn group(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    types: &Types<'_>,
    index: u32,
) -> Result<Option<DesignRefoldOperation>, CodecError> {
    let mut view = match View::over_retained(bytes).child(start, end) {
        Some(view) => view,
        None => return Ok(None),
    };
    let parsed = (|| {
        view.skip(group_layout::ZERO_RUN)?;
        if view.take(group_layout::NAME - group_layout::ZERO_RUN)? != [0; 10] {
            return None;
        }
        let (name, after_name) = lp_utf16_bounded(bytes, view.position(), 21..=21)?;
        if name != "UnfoldRefoldGroupMode" || after_name != start.checked_add(group_layout::MODE)? {
            return None;
        }
        view.seek(after_name)?;
        view.u32_le()?;
        if view.u8()? != 1 {
            return None;
        }
        let count = view.u32_le()?;
        view.counted(u64::from(count), 11)
    })();
    let Some(count) = parsed else {
        return Ok(None);
    };
    let mut members = ctx.alloc_filled(
        count.get(),
        Located {
            value: 0,
            offset: 0,
        },
        "unfold/refold group members",
    )?;
    for member in &mut members {
        let Some(value) = local(&mut view, types, None) else {
            return Ok(None);
        };
        *member = value;
    }
    if view.take(5) != Some(&[0; 5][..]) || !view.is_empty() {
        return Ok(None);
    }
    Ok(Some(DesignRefoldOperation {
        group_record_index: index,
        group_byte_offset: u64_from_index(start),
        group_paired_byte_offset: u64_from_index(end),
        members,
        preceding_unfolds: Vec::new(),
    }))
}

pub(super) fn bind(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    if !scopes.iter().any(|scope| {
        matches!(
            scope.kind(),
            DesignFeatureKind::Unfold | DesignFeatureKind::Refold
        )
    }) {
        return Ok(());
    }
    let mut groups = Vec::new();
    for (&index, &(guid, version)) in types {
        if !guid.eq_ignore_ascii_case(GROUP_GUID) || version != 0 {
            continue;
        }
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        for (start, end) in records.frames(index) {
            if let Some(group) = group(ctx, bytes, start, end, types, index)? {
                groups.push(group);
            }
        }
    }
    groups.sort_by_key(|group| (group.group_record_index, group.group_byte_offset));
    for scope in scopes {
        if scope.kind() == DesignFeatureKind::Unfold {
            let value = unfold(bytes, records, scope, types);
            if let DesignScopePayloadMut::Unfold(slot) = scope.payload_mut() {
                *slot = value;
            }
        } else if typed(types, scope.record_index, REFOLD_GUID, 1) {
            let mut candidates = groups.iter().filter(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.value == scope.record_index)
            });
            let first = candidates.next();
            let value = first
                .filter(|first| {
                    candidates.all(|candidate| {
                        candidate
                            .members
                            .iter()
                            .map(|member| member.value)
                            .eq(first.members.iter().map(|member| member.value))
                    })
                })
                .cloned();
            let value = value.map(|mut group| {
                group.preceding_unfolds = group
                    .members
                    .iter()
                    .map(|member| member.value)
                    .take_while(|index| *index != scope.record_index)
                    .filter(|index| typed(types, *index, UNFOLD_GUID, 0))
                    .collect();
                group
            });
            if let DesignScopePayloadMut::Refold(slot) = scope.payload_mut() {
                *slot = value;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
