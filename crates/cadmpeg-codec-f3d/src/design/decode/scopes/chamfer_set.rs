// SPDX-License-Identifier: Apache-2.0
//! Chamfer set lists named by the prefix of a parameter-owned `Chamfer` scope.

use super::fillet_set::DecodedGroupKeys;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::chamfer_scope_prologue as prologue;
use crate::layout::chamfer_set_compact_equal as compact;
use crate::layout::chamfer_set_distance_angle as distance_angle;
use crate::layout::chamfer_set_equal as equal;
use crate::layout::chamfer_set_two_distances as two_distances;
use crate::records::feature::chamfer_set::{
    DesignChamferCornerType, DesignChamferSet, DesignChamferSetList, DesignChamferSetListForm,
    DesignChamferSetSpec,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;

const INDEXED_HEADER_LEN: usize = 11;
const REFERENCE_LEN: usize = 11;
const ENTRY_LEN: usize = 10;
const ENTITY_GUID_CODE_UNITS: u32 = 36;
const CURRENT_RUN: [u8; 2] = [0, 1];

/// Source kinds of an equal-distance or distance-angle distance.
pub(crate) const DISTANCE_KINDS: &[&str] = &["Distance", "leftDistance"];
/// Source kinds of a two-distance first distance.
pub(crate) const FIRST_DISTANCE_KINDS: &[&str] = &["Distance 1", "leftDistance"];
/// Source kinds of a two-distance second distance.
pub(crate) const SECOND_DISTANCE_KINDS: &[&str] = &["Distance 2", "rightDistance"];

/// Whether a two-distance pair of source kinds belongs to one namespace.
pub(crate) fn two_distance_kinds_pair(first: &str, second: &str) -> bool {
    matches!(
        (first, second),
        ("Distance 1", "Distance 2") | ("leftDistance", "rightDistance")
    )
}
/// Source kinds of a distance-angle angle.
pub(crate) const ANGLE_KINDS: &[&str] = &["Angle", "Rotate Angle", "rotateAngle"];

/// The chamfer set list of `scope`, admitted when the prefix, every set frame,
/// and every owner reference match their forms.
pub(super) fn chamfer_set_list(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignChamferSetList> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Chamfer) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = start.checked_add(usize::try_from(scope.frame_length()).ok()?)?;
    let frame = bytes.get(start..end)?;
    if frame.get(prologue::ZERO_RUN_9..prologue::PROLOGUE_BYTE)? != [0; 9] {
        return None;
    }
    let mut cursor = match *frame.get(prologue::PROLOGUE_BYTE)? {
        0 => prologue::LEN,
        1 if View::u32_le_at(frame, prologue::LEN)? == 0 => prologue::LEN + 4,
        _ => return None,
    };
    let corner_type = match View::u32_le_at(frame, cursor)? {
        1 => DesignChamferCornerType::Chamfer,
        2 => DesignChamferCornerType::Miter,
        3 => DesignChamferCornerType::Blend,
        _ => return None,
    };
    cursor += 4;
    let form = if frame.get(cursor..cursor + CURRENT_RUN.len())? == CURRENT_RUN {
        cursor += CURRENT_RUN.len();
        DesignChamferSetListForm::Current
    } else if *frame.get(cursor)? == 1 {
        cursor += 1;
        DesignChamferSetListForm::Compact
    } else {
        return None;
    };
    let entry_count = View::u32_le_at(frame, cursor)?;
    cursor += 4;
    if entry_count == 0 || usize::try_from(entry_count).ok()? > frame.len() / ENTRY_LEN {
        return None;
    }
    let mut entry_sets = Vec::new();
    for ordinal in 1..=u64::from(entry_count) {
        let [set_number, 0] = *frame.get(cursor + 8..cursor + ENTRY_LEN)? else {
            return None;
        };
        if View::u64_le_at(frame, cursor)? != ordinal || set_number == 0 {
            return None;
        }
        entry_sets.push(u32::from(set_number));
        cursor += ENTRY_LEN;
    }
    let count = View::u32_le_at(frame, cursor)?;
    cursor += 4;
    let set_references = references(frame, &mut cursor, count)?;
    if View::u32_le_at(frame, cursor)? != count {
        return None;
    }
    cursor += 4;
    let group_references = references(frame, &mut cursor, count)?;
    if View::u32_le_at(frame, cursor)? != ENTITY_GUID_CODE_UNITS {
        return None;
    }
    let set_entry_counts = set_entry_counts(&entry_sets, count)?;
    let mut sets = Vec::new();
    for (set_number, (set_record_index, group_record_index)) in
        (1_u32..).zip(set_references.into_iter().zip(group_references))
    {
        if !scope
            .reference_members()
            .values()
            .any(|member| *member == group_record_index)
        {
            return None;
        }
        let entries = *set_entry_counts.get(usize::try_from(set_number - 1).ok()?)?;
        let set = chamfer_set(
            bytes,
            records,
            (set_record_index, set_number, entries),
            scope,
            parameters,
            parameter_owners,
        )?;
        if set.group_record_index != group_record_index {
            return None;
        }
        sets.push(set);
    }
    if sets.is_empty() {
        return None;
    }
    Some(DesignChamferSetList {
        form,
        corner_type,
        entry_count,
        sets,
    })
}

/// Entries per set, in set-number order, when every entry names a set from
/// one to `set_count` and every set has at least one entry.
pub(super) fn set_entry_counts(entry_sets: &[u32], set_count: u32) -> Option<Vec<u32>> {
    let mut counts = vec![0_u32; usize::try_from(set_count).ok()?.min(entry_sets.len())];
    if counts.len() != usize::try_from(set_count).ok()? {
        return None;
    }
    for set_number in entry_sets {
        let slot = counts.get_mut(usize::try_from(set_number.checked_sub(1)?).ok()?)?;
        *slot = slot.checked_add(1)?;
    }
    counts.iter().all(|count| *count > 0).then_some(counts)
}

/// `count` consecutive distinct marked references from `cursor`.
fn references(frame: &[u8], cursor: &mut usize, count: u32) -> Option<Vec<u32>> {
    let count = usize::try_from(count).ok()?;
    if count > frame.len() / REFERENCE_LEN {
        return None;
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let value = marked_record_reference(frame, *cursor)?;
        if out.contains(&value) {
            return None;
        }
        out.push(value);
        *cursor += REFERENCE_LEN;
    }
    Some(out)
}

/// Dimensional specification a set form stores.
#[derive(Clone, Copy)]
enum SpecForm {
    EqualDistance,
    TwoDistances,
    DistanceAngle,
}

/// Byte layout of one chamfer set form.
struct SetForm {
    len: usize,
    tangent_chain: usize,
    spec: SpecForm,
    owners: [usize; 2],
    owner_count: usize,
    kind: (usize, u32),
    group: usize,
    set_number: usize,
    one_byte: Option<usize>,
    auxiliary: usize,
    scope: usize,
}

const SET_FORMS: [SetForm; 4] = [
    SetForm {
        len: equal::LEN,
        tangent_chain: equal::TANGENT_CHAIN,
        spec: SpecForm::EqualDistance,
        owners: [equal::DISTANCE_OWNER_REFERENCE, 0],
        owner_count: 1,
        kind: (equal::SPEC_KIND, equal::SPEC_KIND_VALUE),
        group: equal::EDGE_GROUP_REFERENCE,
        set_number: equal::SET_NUMBER,
        one_byte: Some(equal::FIXED_BYTE),
        auxiliary: equal::AUXILIARY_REFERENCE,
        scope: equal::SCOPE_REFERENCE,
    },
    SetForm {
        len: two_distances::LEN,
        tangent_chain: two_distances::FLAG,
        spec: SpecForm::TwoDistances,
        owners: [
            two_distances::FIRST_DISTANCE_OWNER_REFERENCE,
            two_distances::SECOND_DISTANCE_OWNER_REFERENCE,
        ],
        owner_count: 2,
        kind: (two_distances::SPEC_KIND, two_distances::SPEC_KIND_VALUE),
        group: two_distances::EDGE_GROUP_REFERENCE,
        set_number: two_distances::SET_NUMBER,
        one_byte: Some(two_distances::FIXED_BYTE),
        auxiliary: two_distances::AUXILIARY_REFERENCE,
        scope: two_distances::SCOPE_REFERENCE,
    },
    SetForm {
        len: distance_angle::LEN,
        tangent_chain: distance_angle::FLAG,
        spec: SpecForm::DistanceAngle,
        owners: [
            distance_angle::DISTANCE_OWNER_REFERENCE,
            distance_angle::ANGLE_OWNER_REFERENCE,
        ],
        owner_count: 2,
        kind: (distance_angle::SPEC_KIND, distance_angle::SPEC_KIND_VALUE),
        group: distance_angle::EDGE_GROUP_REFERENCE,
        set_number: distance_angle::SET_NUMBER,
        one_byte: Some(distance_angle::FIXED_BYTE),
        auxiliary: distance_angle::AUXILIARY_REFERENCE,
        scope: distance_angle::SCOPE_REFERENCE,
    },
    SetForm {
        len: compact::LEN,
        tangent_chain: compact::FLAG,
        spec: SpecForm::EqualDistance,
        owners: [compact::DISTANCE_OWNER_REFERENCE, 0],
        owner_count: 1,
        kind: (compact::SPEC_KIND, compact::SPEC_KIND_VALUE),
        group: compact::EDGE_GROUP_REFERENCE,
        set_number: compact::SET_NUMBER,
        one_byte: None,
        auxiliary: compact::AUXILIARY_REFERENCE,
        scope: compact::SCOPE_REFERENCE,
    },
];

impl SetForm {
    /// Whether every byte outside the named fields of `frame` is zero.
    fn unnamed_bytes_are_zero(&self, frame: &[u8]) -> bool {
        let mut fields = vec![
            (0, INDEXED_HEADER_LEN),
            (self.tangent_chain, 1),
            (self.kind.0, 4),
            (self.group, REFERENCE_LEN),
            (self.set_number, 4),
            (self.auxiliary, REFERENCE_LEN),
            (self.scope, REFERENCE_LEN),
        ];
        fields.extend(
            self.owners
                .iter()
                .take(self.owner_count)
                .map(|at| (*at, REFERENCE_LEN)),
        );
        fields.extend(self.one_byte.map(|at| (at, 1)));
        frame.iter().enumerate().all(|(at, byte)| {
            *byte == 0
                || fields
                    .iter()
                    .any(|(start, width)| (*start..start + width).contains(&at))
        })
    }

    /// Source kinds each owner reference of this form must carry.
    fn owner_kinds(&self) -> [&'static [&'static str]; 2] {
        match self.spec {
            SpecForm::EqualDistance => [DISTANCE_KINDS, &[]],
            SpecForm::TwoDistances => [FIRST_DISTANCE_KINDS, SECOND_DISTANCE_KINDS],
            SpecForm::DistanceAngle => [DISTANCE_KINDS, ANGLE_KINDS],
        }
    }
}

/// The chamfer set record `record_index` with 1-based `set_number` and
/// `entry_count` prefix entries, admitted in exactly one form.
fn chamfer_set(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    (record_index, set_number, entry_count): (u32, u32, u32),
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignChamferSet> {
    let mut frames = records.frames(record_index);
    let (start, end) = frames.next()?;
    if frames.next().is_some() {
        return None;
    }
    let frame = bytes.get(start..end)?;
    let mut matches = SET_FORMS.iter().filter_map(|form| {
        if frame.len() != form.len
            || !matches!(frame.get(form.tangent_chain), Some(0 | 1))
            || View::u32_le_at(frame, form.kind.0)? != form.kind.1
            || View::u32_le_at(frame, form.set_number)? != set_number
            || form.one_byte.is_some_and(|at| frame.get(at) != Some(&1))
            || marked_record_reference(frame, form.scope)? != scope.record_index
            || !form.unnamed_bytes_are_zero(frame)
        {
            return None;
        }
        if records
            .offsets(marked_record_reference(frame, form.auxiliary)?)
            .is_empty()
        {
            return None;
        }
        let kinds = form.owner_kinds();
        let owned_kind = |slot: usize| {
            let owner = marked_record_reference(frame, form.owners[slot])?;
            let kind = owned_source_kind(scope, owner, parameters, parameter_owners)?;
            kinds[slot].contains(&kind).then_some((owner, kind))
        };
        let owner = |slot: usize| owned_kind(slot).map(|(owner, _)| owner);
        if matches!(form.spec, SpecForm::TwoDistances)
            && !two_distance_kinds_pair(owned_kind(0)?.1, owned_kind(1)?.1)
        {
            return None;
        }
        let spec = match form.spec {
            SpecForm::EqualDistance => DesignChamferSetSpec::EqualDistance {
                distance_owner_record_index: owner(0)?,
            },
            SpecForm::TwoDistances => DesignChamferSetSpec::TwoDistances {
                first_owner_record_index: owner(0)?,
                second_owner_record_index: owner(1)?,
            },
            SpecForm::DistanceAngle => DesignChamferSetSpec::DistanceAngle {
                distance_owner_record_index: owner(0)?,
                angle_owner_record_index: owner(1)?,
            },
        };
        Some(DesignChamferSet {
            record_index,
            byte_offset: u64::try_from(start).ok()?,
            frame_length: u64::try_from(form.len).ok()?,
            group_record_index: marked_record_reference(frame, form.group)?,
            spec,
            auxiliary_record_index: marked_record_reference(frame, form.auxiliary)?,
            entry_count,
            tangent_chain: frame.get(form.tangent_chain) == Some(&1),
        })
    });
    let set = matches.next()?;
    matches.next().is_none().then_some(set)
}

/// Source kind of the one Design parameter that owner `owner_record_index`
/// of `scope` references and that references the owner back.
fn owned_source_kind<'a>(
    scope: &DesignParameterScope,
    owner_record_index: u32,
    parameters: &'a [DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<&'a str> {
    let stream = native_stream(&scope.id)?;
    let mut owners = parameter_owners.iter().filter(|owner| {
        native_stream(owner.id()) == Some(stream)
            && owner.scope_record_index() == scope.record_index
            && owner.record_index() == owner_record_index
    });
    let owner = owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    let mut owned = parameters.iter().filter(|parameter| {
        native_stream(&parameter.id) == Some(stream)
            && parameter.record_index == owner.parameter_record_index()
            && parameter.owner_record_index() == Some(owner.record_index())
    });
    let parameter = owned.next()?;
    owned.next().is_none().then(|| parameter.source_kind_str())
}

/// Owners of one set's specification in slot order: the distance or first
/// distance, then the second distance or angle.
fn spec_owners(spec: DesignChamferSetSpec) -> [Option<u32>; 2] {
    match spec {
        DesignChamferSetSpec::EqualDistance {
            distance_owner_record_index,
        } => [Some(distance_owner_record_index), None],
        DesignChamferSetSpec::TwoDistances {
            first_owner_record_index,
            second_owner_record_index,
        } => [
            Some(first_owner_record_index),
            Some(second_owner_record_index),
        ],
        DesignChamferSetSpec::DistanceAngle {
            distance_owner_record_index,
            angle_owner_record_index,
        } => [
            Some(distance_owner_record_index),
            Some(angle_owner_record_index),
        ],
    }
}

/// Whether the sets of `list`, taken in the reference-table order of their
/// edge groups, name owners in increasing owner-local order in each slot, so
/// that they assign the same specification to each group as owner order does.
fn agrees_with_owner_order(
    scope: &DesignParameterScope,
    list: &DesignChamferSetList,
    parameter_owners: &[DesignParameterOwner],
) -> bool {
    let stream = native_stream(&scope.id);
    let local_ordinal = |owner_record_index: u32| {
        let mut owners = parameter_owners.iter().filter(|owner| {
            native_stream(owner.id()) == stream
                && owner.scope_record_index() == scope.record_index
                && owner.record_index() == owner_record_index
        });
        let owner = owners.next()?;
        owners.next().is_none().then(|| owner.local_ordinal())
    };
    let mut ordered = Vec::new();
    for set in &list.sets {
        let Some(position) = scope
            .reference_members()
            .values()
            .position(|member| *member == set.group_record_index)
        else {
            return false;
        };
        ordered.push((position, spec_owners(set.spec)));
    }
    ordered.sort_by_key(|(position, _)| *position);
    (0..2).all(|slot| {
        let mut previous = None;
        ordered.iter().all(|(_, owners)| match owners[slot] {
            None => true,
            Some(owner) => {
                let Some(ordinal) = local_ordinal(owner) else {
                    return false;
                };
                let increasing = previous.is_none_or(|previous| ordinal > previous);
                previous = Some(ordinal);
                increasing
            }
        })
    })
}

/// Clear each chamfer set list that names an edge group outside the decoded
/// construction-operand groups of its scope, or whose owners assign the
/// specifications to the groups in another order than owner-local order.
/// Returns the scopes cleared for disagreeing with owner order.
pub(crate) fn retain_consistent_chamfer_sets(
    scopes: &mut [DesignParameterScope],
    decoded_groups: &DecodedGroupKeys,
    parameter_owners: &[DesignParameterOwner],
) -> Vec<String> {
    let mut conflicts = Vec::new();
    for scope in scopes {
        let Some(sets) = scope.chamfer_sets() else {
            continue;
        };
        let stream = native_stream(&scope.id);
        let grouped = sets.sets.iter().all(|set| {
            stream.is_some_and(|stream| {
                decoded_groups.contains(&(
                    stream.to_owned(),
                    scope.record_index,
                    set.group_record_index,
                ))
            })
        });
        let agreeing = agrees_with_owner_order(scope, sets, parameter_owners);
        if grouped && agreeing {
            continue;
        }
        if grouped {
            conflicts.push(scope.id.clone());
        }
        if let crate::records::feature::scope::DesignScopePayloadMut::Chamfer(slot) =
            scope.payload_mut()
        {
            slot.sets = None;
        }
    }
    conflicts
}

/// The loss that names the Chamfer scopes whose set lists were dropped for
/// disagreeing with owner order.
pub(crate) fn chamfer_set_conflict_loss(
    conflicts: &[String],
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (!conflicts.is_empty()).then(|| {
        crate::loss::F3dLossCode::FeatureChamferSetConflict.note(format!(
            "{} Chamfer scope(s) store set records that pair edge groups and specifications in another order than owner-local order; the owner-order pairing is kept and the set lists are dropped: {}.",
            conflicts.len(),
            conflicts.join(", ")
        ))
    })
}
