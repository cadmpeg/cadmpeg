use crate::bytes::{lp_utf16_bounded, take_reference};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use std::collections::HashMap;

pub(super) type Types<'a> = HashMap<u64, (&'a str, u32)>;
pub(super) const UNFOLD_GUID: &str = "EC187027-FF97-48A3-8031-F923807E630B";
pub(super) const SETTINGS_GUID: &str = "E75C86D6-76D8-4F1F-BC16-AB8CB74958B0";
pub(super) const RULE_GUID: &str = "B42ED662-8999-4C1B-A744-759D1D446D55";
pub(super) const PRESENTATION_GUID: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";

pub(super) fn typed(types: &Types<'_>, index: u32, guid: &str, version: u32) -> bool {
    types
        .get(&u64::from(index))
        .is_some_and(|(found, v)| found.eq_ignore_ascii_case(guid) && *v == version)
}

pub(super) fn local(
    view: &mut View<'_>,
    types: &Types<'_>,
    expected: Option<&str>,
) -> Option<Located<u32>> {
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

pub(super) fn settings_rule(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    index: u32,
    owner: u32,
    owner_guid: &str,
) -> Option<(u32, bool)> {
    if let Some(index) = exact_settings_rule(bytes, records, types, index, owner) {
        return Some((index, false));
    }
    let (guid, version) = *types.get(&u64::from(index))?;
    if !guid.eq_ignore_ascii_case(SETTINGS_GUID) || !matches!(version, 1..=3) {
        return None;
    }
    let mut result = None;
    for (start, end) in records.frames(index) {
        let Some(frame) = View::over_retained(bytes).child(start, end) else {
            continue;
        };
        let mut prefix = frame;
        if prefix.skip(11).is_none() || prefix.take(10) != Some(&[0; 10][..]) {
            continue;
        }
        let mut owner_at = None;
        for width in [11, crate::bytes::TYPED_LOCAL_REFERENCE_LEN] {
            let Some(at) = end.checked_sub(width) else {
                continue;
            };
            let mut tail = frame;
            if tail.seek(at).is_none() {
                continue;
            }
            let Some(target) = local(&mut tail, types, Some(owner_guid)) else {
                continue;
            };
            if target.value != owner || !tail.is_empty() {
                continue;
            }
            if owner_at.replace(at).is_some() {
                return None;
            }
        }
        let Some(owner_at) = owner_at else {
            continue;
        };
        for at in prefix.position()..owner_at {
            let mut candidate = frame;
            candidate.seek(at)?;
            let Some(reference) = local(&mut candidate, types, Some(RULE_GUID)) else {
                continue;
            };
            if candidate.position() > owner_at
                || !(typed(types, reference.value, RULE_GUID, 0)
                    || typed(types, reference.value, RULE_GUID, 1))
            {
                continue;
            }
            if result.replace(reference.value).is_some() {
                return None;
            }
        }
    }
    result.map(|index| (index, true))
}

fn exact_settings_rule(
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

pub(super) struct RuleValues {
    pub(super) first_value: Located<FiniteReal>,
    pub(super) thickness: Located<PositiveReal>,
    pub(super) material_scalar: Located<FiniteReal>,
}

pub(super) fn rule_values(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    index: u32,
) -> Option<RuleValues> {
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
        let (_, after_name) = lp_utf16_bounded(bytes, view.position(), 1..=64)?;
        if after_name > end {
            continue;
        }
        view.seek(after_name)?;
        let Some(first) = FiniteReal::new(view.f64_le()?) else {
            continue;
        };
        for at in view.position()..end {
            let Some((material, after_material)) = lp_utf16_bounded(bytes, at, 1..=128) else {
                continue;
            };
            if material.is_empty() || after_material > end {
                continue;
            }
            let Some((name, after_thickness)) = lp_utf16_bounded(bytes, after_material, 1..=64)
            else {
                continue;
            };
            if name != "Thickness" || after_thickness.checked_add(8)? > end {
                continue;
            }
            let mut scalar_view = frame;
            scalar_view.seek(after_thickness)?;
            let thickness = scalar_view.f64_le()?;
            let thickness = PositiveReal::new(thickness)?;
            let offset = at.checked_sub(8)?;
            scalar_view.seek(offset)?;
            let scalar = FiniteReal::new(scalar_view.f64_le()?)?;
            if result
                .replace(RuleValues {
                    first_value: Located {
                        value: first,
                        offset: u64_from_index(after_name),
                    },
                    thickness: Located {
                        value: thickness,
                        offset: u64_from_index(after_thickness),
                    },
                    material_scalar: Located {
                        value: scalar,
                        offset: u64_from_index(offset),
                    },
                })
                .is_some()
            {
                return None;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
