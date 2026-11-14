// SPDX-License-Identifier: Apache-2.0
//! Model-space fit bounds selected by an edge's complete parameter map.

use cadmpeg_core::decode::View;

use crate::families::b2::records::B2EdgeNode;
use crate::families::consolidated::allocation::AllocationJournal;
use crate::families::consolidated::records::class25::{
    self, Class25Accuracy, Class25SourceRelation,
};
use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

#[derive(Clone, Copy)]
pub(super) struct Selection {
    pub(super) representations: [usize; 2],
    pub(super) selected: usize,
    pub(super) range: [f64; 2],
}

struct FittedMap {
    operands: [AllocationReference; 2],
    ranges: [[f64; 2]; 2],
    tolerance: f64,
}

fn resolve<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    reference: AllocationReference,
) -> Option<&'a ConsolidatedRecord> {
    match reference.encoding {
        AllocationReferenceEncoding::WidthCoded => {
            journal.resolve_weighted_distance(origin, reference.value)
        }
        AllocationReferenceEncoding::BackwardDistance => {
            journal.resolve_record_distance(origin, reference.value)
        }
        AllocationReferenceEncoding::OwnedChild => {
            journal.resolve_owned_child(origin, reference.value)
        }
        AllocationReferenceEncoding::Selector2
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16 => None,
    }
}

fn interval(view: &mut View<'_>) -> Option<[f64; 2]> {
    let result = [view.f64_le()?, view.f64_le()?];
    (result.into_iter().all(f64::is_finite) && result[0] < result[1]).then_some(result)
}

fn samples(view: &mut View<'_>, count: usize, range: [f64; 2], increasing: bool) -> Option<()> {
    let mut previous = None;
    let mut direction = None;
    for _ in 0..count {
        let value = view.f64_le()?;
        if !value.is_finite() || !(range[0]..=range[1]).contains(&value) {
            return None;
        }
        if let Some(previous) = previous {
            let forward = value > previous;
            if value == previous
                || (increasing && !forward)
                || direction.is_some_and(|direction| direction != forward)
            {
                return None;
            }
            direction = Some(forward);
        }
        previous = Some(value);
    }
    Some(())
}

fn fitted_map(payload: &[u8]) -> Option<FittedMap> {
    if let Some(map) = class25::parse(payload) {
        let range = match map.source {
            Class25SourceRelation::Affine { range, .. }
            | Class25SourceRelation::Sampled { range, .. } => range.endpoints(),
            Class25SourceRelation::Absent => return None,
        };
        let Class25Accuracy::Fitted { fit_tolerance, .. } = map.accuracy else {
            return None;
        };
        return Some(FittedMap {
            operands: [map.source_operand, map.target_operand].map(|operand| AllocationReference {
                value: operand.value,
                encoding: operand.encoding,
            }),
            ranges: [map.target_range.endpoints(), range],
            tolerance: fit_tolerance.get(),
        });
    }
    if payload.first() != Some(&0x82) {
        return None;
    }
    let mut at = 1;
    let operands = [
        allocation_reference(payload, &mut at)?,
        allocation_reference(payload, &mut at)?,
    ];
    let mut view = View::over_retained(payload.get(at..)?);
    if view.u8()? != 0x0a {
        return None;
    }
    let selector = view.u16_le()?;
    if !matches!(selector, 0x1584..=0x1587) {
        return None;
    }
    let target = interval(&mut view)?;
    let signed = view.f64_le()?;
    if !signed.is_finite() {
        return None;
    }
    let source = interval(&mut view)?;
    let count = view.u8()?.checked_sub(0x80)?;
    if count < 2 {
        return None;
    }
    let count = view.counted(u64::from(count), 16)?.get();
    samples(&mut view, count, target, true)?;
    samples(&mut view, count, source, false)?;
    let mut site = || {
        let value = view.f64_le()?;
        (value.is_finite() && (target[0]..=target[1]).contains(&value)).then_some(value)
    };
    match selector & 0xff {
        0x84 => {
            if site()? > site()? {
                return None;
            }
        }
        0x85 | 0x86 => {
            site()?;
        }
        _ => {}
    }
    let tolerance = view.f64_le()?;
    (view.is_empty() && tolerance.is_finite() && tolerance >= signed.abs()).then_some(FittedMap {
        operands,
        ranges: [target, source],
        tolerance,
    })
}

pub(super) fn fit_tolerance(
    data: &[u8],
    journal: &AllocationJournal<'_>,
    node_index: usize,
    node: &B2EdgeNode,
    selection: Selection,
) -> Option<f64> {
    let record = resolve(
        journal,
        node_index,
        AllocationReference {
            value: node.curve_ref,
            encoding: node.reference_encodings[0],
        },
    )?;
    if record.family() != ConsolidatedFamily::A || record.class() != 0x25 {
        return None;
    }
    let origin = journal.index(record.source_index(), record.source_range().start)?;
    let map = fitted_map(data.get(record.payload()?)?)?;
    let mut resolved = [0; 2];
    for (slot, operand) in resolved.iter_mut().zip(map.operands) {
        let representation = resolve(journal, origin, operand)?;
        if !matches!(
            (representation.family(), representation.class()),
            (ConsolidatedFamily::A, 0x20) | (ConsolidatedFamily::B, 0x18 | 0x19 | 0x20)
        ) {
            return None;
        }
        *slot = journal.index(
            representation.source_index(),
            representation.source_range().start,
        )?;
    }
    if resolved[0] == resolved[1] {
        return None;
    }
    let selected = resolved
        .iter()
        .position(|index| *index == selection.selected)?;
    let mut expected = selection.representations;
    expected.sort_unstable();
    resolved.sort_unstable();
    (resolved == expected && map.ranges[selected] == selection.range).then_some(map.tolerance)
}

#[cfg(test)]
mod tests;
