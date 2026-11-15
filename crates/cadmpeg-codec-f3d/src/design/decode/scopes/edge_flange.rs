use crate::bytes::{take_reference, Reference};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::edge_flange_reference_groups_short as short_groups;
use crate::layout::edge_flange_reference_groups_wide as wide_groups;
use crate::layout::edge_flange_reference_prologue as prologue;
use crate::layout::edge_flange_reference_side_short as short_side;
use crate::layout::edge_flange_reference_side_wide as wide_side;
use crate::layout::edge_flange_two_offset_wrapper as wrapper_layout;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::sheet_metal::edge::{
    DesignEdgeFlangeScope, DesignReferenceEdgeFlange, DesignReferenceFlangeEdge,
    DesignReferenceFlangeResult,
};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;
use std::collections::HashMap;

type Types<'a> = HashMap<u64, (&'a str, u32)>;
const OPERATION_GUID: &str = "7AFA0A65-B53C-472C-ACA4-5C7C0A65001C";
const OFFSET_WRAPPER_GUID: &str = "4A557CE2-375C-4F69-AA76-2403109F1491";

fn word(view: &mut View<'_>) -> Option<Located<u32>> {
    Some(Located {
        offset: u64_from_index(view.position()),
        value: view.u32_le()?,
    })
}

fn zeros(view: &mut View<'_>, count: usize) -> Option<()> {
    view.take(count)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some(())
}

enum LocalReference {
    Null,
    Target(Located<u32>),
}

impl LocalReference {
    fn into_target(self) -> Option<Located<u32>> {
        match self {
            Self::Null => None,
            Self::Target(target) => Some(target),
        }
    }
}

fn local(view: &mut View<'_>, types: &Types<'_>) -> Option<LocalReference> {
    let offset = u64_from_index(view.position());
    let mut end = 0;
    let reference = take_reference(view.unread(), &mut end)?;
    let value = match reference {
        Reference::Null => LocalReference::Null,
        Reference::Local {
            target,
            inline_type_guid,
        } => {
            let value = u32::try_from(target).ok()?;
            if inline_type_guid.is_some_and(|inline| {
                types
                    .get(&target)
                    .is_some_and(|(guid, _)| !guid.eq_ignore_ascii_case(inline))
            }) {
                return None;
            }
            LocalReference::Target(Located { value, offset })
        }
        _ => return None,
    };
    view.skip(end)?;
    Some(value)
}

fn member(scope: &DesignParameterScope, reference: Option<&Located<u32>>) -> Option<()> {
    reference
        .is_none_or(|reference| {
            scope
                .reference_members()
                .values()
                .any(|value| *value == reference.value)
        })
        .then_some(())
}

struct Candidate<'a> {
    operation: DesignReferenceEdgeFlange,
    wrappers: View<'a>,
    groups: View<'a>,
    results: View<'a>,
    edge_count: usize,
    result_count: usize,
}

fn candidate<'a>(
    mut view: View<'a>,
    scope: &DesignParameterScope,
    types: &Types<'_>,
    wide_radius: bool,
    wide_group: bool,
) -> Option<Candidate<'a>> {
    let start = view.position();
    let bend_position = word(&mut view)?;
    let edge_count = word(&mut view)?;
    let count = view.counted(u64::from(edge_count.value), 1)?.get();
    if count == 0 || view.position().checked_sub(start)? != prologue::LEN {
        return None;
    }
    let wrappers = view;
    for _ in 0..count {
        member(scope, local(&mut view, types)?.into_target().as_ref())?;
    }
    let settings = local(&mut view, types)?.into_target();
    member(scope, settings.as_ref())?;
    let height_datum = word(&mut view)?;
    let angle_owner = local(&mut view, types)?.into_target();
    let height_owner = local(&mut view, types)?.into_target();
    member(scope, angle_owner.as_ref())?;
    member(scope, height_owner.as_ref())?;
    let (side_padding, radius_padding) = if wide_radius {
        (
            wide_side::RADIUS_CONTROL - wide_side::SIDE_ZERO,
            wide_side::LEN - wide_side::RADIUS_ZERO,
        )
    } else {
        (
            short_side::RADIUS_CONTROL - short_side::SIDE_ZERO,
            short_side::LEN - short_side::RADIUS_ZERO,
        )
    };
    if view.u32_le()? != short_side::REFERENCE_SIDE_VALUE {
        return None;
    }
    zeros(&mut view, side_padding)?;
    let radius_control = Located {
        offset: u64_from_index(view.position()),
        value: PositiveReal::new(view.f64_le()?)?,
    };
    zeros(&mut view, radius_padding)?;
    let result_count = word(&mut view)?;
    let results_count = view.counted(u64::from(result_count.value), 5)?.get();
    let results = view;
    for ordinal in 0..results_count {
        local(&mut view, types)?;
        if view.u32_le()? != u32::from(ordinal + 1 < results_count) {
            return None;
        }
    }
    if view.u32_le()? != 1 {
        return None;
    }
    let aggregate_group = local(&mut view, types)?.into_target();
    member(scope, aggregate_group.as_ref())?;
    zeros(
        &mut view,
        if wide_group {
            wide_groups::GROUP_COUNT
        } else {
            short_groups::GROUP_COUNT
        },
    )?;
    if view.u32_le()? != edge_count.value {
        return None;
    }
    let groups = view;
    for _ in 0..count {
        member(scope, local(&mut view, types)?.into_target().as_ref())?;
    }
    Some(Candidate {
        operation: DesignReferenceEdgeFlange {
            bend_position: Some(bend_position),
            edge_count: Some(edge_count),
            settings,
            height_datum: Some(height_datum),
            angle_owner,
            height_owner,
            radius_control: Some(radius_control),
            result_count: Some(result_count),
            aggregate_group,
            ..Default::default()
        },
        wrappers,
        groups,
        results,
        edge_count: count,
        result_count: results_count,
    })
}

fn offset_wrapper(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    scope: &DesignParameterScope,
    edge: &DesignReferenceFlangeEdge,
) -> Option<DesignReferenceFlangeEdge> {
    let index = edge.wrapper.as_ref()?.value;
    let (guid, version) = *types.get(&u64::from(index))?;
    if !guid.eq_ignore_ascii_case(OFFSET_WRAPPER_GUID) || version != 1 {
        return None;
    }
    let mut found = None;
    for (start, end) in records.frames(index) {
        let read = || -> Option<_> {
            let mut view = View::over_retained(bytes).child(start, end)?;
            view.skip(wrapper_layout::ZERO_PREFIX)?;
            zeros(
                &mut view,
                wrapper_layout::MEMBER_COUNT - wrapper_layout::ZERO_PREFIX,
            )?;
            let count = word(&mut view)?;
            if count.value != wrapper_layout::MEMBER_COUNT_VALUE
                || view.u32_le()? != wrapper_layout::CONTROL_VALUE
            {
                return None;
            }
            let group = local(&mut view, types)?.into_target();
            if group.as_ref().map(|value| value.value)
                != edge.group.as_ref().map(|value| value.value)
            {
                return None;
            }
            let first_boundary = local(&mut view, types)?.into_target();
            let second_boundary = local(&mut view, types)?.into_target();
            let first_owner = local(&mut view, types)?.into_target();
            let second_owner = local(&mut view, types)?.into_target();
            zeros(
                &mut view,
                wrapper_layout::NESTED - wrapper_layout::MIDDLE_ZERO,
            )?;
            local(&mut view, types)?.into_target()?;
            if view.u8()? != wrapper_layout::CLOSING_ZERO_VALUE
                || local(&mut view, types)?.into_target()?.value != scope.record_index
                || !view.is_empty()
            {
                return None;
            }
            for reference in [
                &first_boundary,
                &second_boundary,
                &first_owner,
                &second_owner,
            ] {
                member(scope, reference.as_ref())?;
            }
            Some(DesignReferenceFlangeEdge {
                first_boundary,
                second_boundary,
                first_owner,
                second_owner,
                wrapper_member_count: Some(count),
                ..edge.clone()
            })
        };
        if let Some(value) = read() {
            if found.replace(value).is_some() {
                return None;
            }
        }
    }
    found
}

pub(super) fn bind(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    types: &Types<'_>,
    scopes: &mut [DesignParameterScope],
) -> Result<(), CodecError> {
    for scope in scopes {
        if scope.kind() != DesignFeatureKind::EdgeFlange || scope.edge_flange_operation().is_some()
        {
            continue;
        }
        let Some((guid, version)) = types.get(&u64::from(scope.record_index)) else {
            continue;
        };
        if !guid.eq_ignore_ascii_case(OPERATION_GUID) || !matches!(version, 4 | 6) {
            continue;
        }
        let Some((start, end)) = usize::try_from(scope.byte_offset())
            .ok()
            .zip(usize::try_from(scope.reference_count_offset()).ok())
        else {
            continue;
        };
        let Some(frame) = View::over_retained(bytes).child(start, end) else {
            continue;
        };
        let mut found = None;
        let mut ambiguous = false;
        for at in start..end {
            let mut view = frame;
            if view.seek(at).is_none() {
                continue;
            }
            for (wide_radius, wide_group) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                if let Some(candidate) = candidate(view, scope, types, wide_radius, wide_group) {
                    if found.replace(candidate).is_some() {
                        ambiguous = true;
                    }
                }
            }
        }
        let Some(mut found) = found.filter(|_| !ambiguous) else {
            continue;
        };
        let mut edges = ctx.alloc_filled(
            found.edge_count,
            DesignReferenceFlangeEdge::default(),
            "reading EdgeFlange edge slots",
        )?;
        for edge in &mut edges {
            edge.wrapper = local(&mut found.wrappers, types).and_then(LocalReference::into_target);
            edge.group = local(&mut found.groups, types).and_then(LocalReference::into_target);
            if let Some(width) = offset_wrapper(bytes, records, types, scope, edge) {
                *edge = width;
            }
        }
        let mut results = ctx.alloc_filled(
            found.result_count,
            DesignReferenceFlangeResult::default(),
            "reading EdgeFlange result slots",
        )?;
        for result in &mut results {
            result.record = local(&mut found.results, types).and_then(LocalReference::into_target);
            result.trailer = word(&mut found.results);
        }
        found.operation.edges = edges;
        found.operation.results = results;
        if let DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
            *slot = DesignEdgeFlangeScope::ReferenceFramed(found.operation);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
