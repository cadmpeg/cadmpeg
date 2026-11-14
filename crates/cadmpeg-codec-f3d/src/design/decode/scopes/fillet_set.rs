// SPDX-License-Identifier: Apache-2.0
//! Fillet set lists named by the prefix of a parameter-owned `Fillet` scope.

use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::fillet_scope_prologue as prologue;
use crate::layout::fillet_set_chordal as chordal;
use crate::layout::fillet_set_compact_constant as compact;
use crate::layout::fillet_set_constant as constant;
use crate::records::feature::fillet_set::{
    DesignFilletSet, DesignFilletSetLaw, DesignFilletSetList, DesignFilletSetListForm,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::fillet::{DesignFilletRadiusGroup, DesignFilletRadiusLaw};
use cadmpeg_core::decode::View;

const INDEXED_HEADER_LEN: usize = 11;
const REFERENCE_LEN: usize = 11;
const ENTITY_GUID_CODE_UNITS: u32 = 36;
const CURRENT_FLAG_RUN_TAIL: [u8; 11] = [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const COMPACT_FLAG_RUN: [u8; 11] = [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// The fillet set list of `scope`, admitted when the prefix, every set frame,
/// and every owner reference match their forms.
pub(super) fn fillet_set_list(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFilletSetList> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Fillet) {
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
    let count = View::u32_le_at(frame, cursor)?;
    cursor += 4;
    let set_references = references(frame, &mut cursor, count)?;
    if View::u32_le_at(frame, cursor)? != 0 {
        return None;
    }
    cursor += 4;
    let rolling_ball = *frame.get(cursor)?;
    let form = if matches!(rolling_ball, 0 | 1)
        && frame.get(cursor + 1..cursor + 1 + CURRENT_FLAG_RUN_TAIL.len())? == CURRENT_FLAG_RUN_TAIL
    {
        cursor += 1 + CURRENT_FLAG_RUN_TAIL.len();
        DesignFilletSetListForm::Current
    } else if frame.get(cursor..cursor + COMPACT_FLAG_RUN.len())? == COMPACT_FLAG_RUN {
        cursor += COMPACT_FLAG_RUN.len();
        DesignFilletSetListForm::Compact
    } else {
        return None;
    };
    if View::u32_le_at(frame, cursor)? != count {
        return None;
    }
    cursor += 4;
    let group_references = references(frame, &mut cursor, count)?;
    if View::u32_le_at(frame, cursor)? != ENTITY_GUID_CODE_UNITS {
        return None;
    }
    let mut sets = Vec::new();
    for (position, (set_record_index, group_record_index)) in
        (0_u32..).zip(set_references.into_iter().zip(group_references))
    {
        if !scope
            .reference_members()
            .values()
            .any(|member| *member == group_record_index)
        {
            return None;
        }
        let set = fillet_set(
            bytes,
            records,
            (set_record_index, position),
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
    Some(DesignFilletSetList {
        form,
        rolling_ball_corner: rolling_ball == 1,
        sets,
    })
}

/// `count` consecutive marked references from `cursor`, each distinct.
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

/// Radius law a set form stores at its law-owner offset.
#[derive(Clone, Copy)]
enum LawRole {
    Radius,
    ChordLength,
}

impl LawRole {
    fn source_kind(self) -> &'static str {
        match self {
            Self::Radius => "Radius",
            Self::ChordLength => "ChordLen",
        }
    }
}

/// Byte layout of one fillet set form.
struct SetForm {
    len: usize,
    marker: usize,
    tangent_chain: usize,
    curvature: Option<usize>,
    group: usize,
    law: (usize, LawRole),
    ordinal: usize,
    tangency_weight: usize,
}

const SET_FORMS: [SetForm; 3] = [
    SetForm {
        len: constant::LEN,
        marker: constant::FORM_MARKER,
        tangent_chain: constant::TANGENT_CHAIN,
        curvature: Some(constant::CURVATURE_CONTINUITY),
        group: constant::EDGE_GROUP_REFERENCE,
        law: (constant::RADIUS_OWNER_REFERENCE, LawRole::Radius),
        ordinal: constant::SET_ORDINAL,
        tangency_weight: constant::TANGENCY_WEIGHT_OWNER_REFERENCE,
    },
    SetForm {
        len: chordal::LEN,
        marker: chordal::FORM_MARKER,
        tangent_chain: chordal::FLAG,
        curvature: None,
        group: chordal::EDGE_GROUP_REFERENCE,
        law: (chordal::CHORD_LENGTH_OWNER_REFERENCE, LawRole::ChordLength),
        ordinal: chordal::SET_ORDINAL,
        tangency_weight: chordal::TANGENCY_WEIGHT_OWNER_REFERENCE,
    },
    SetForm {
        len: compact::LEN,
        marker: compact::FORM_MARKER,
        tangent_chain: compact::FLAG,
        curvature: None,
        group: compact::EDGE_GROUP_REFERENCE,
        law: (compact::RADIUS_OWNER_REFERENCE, LawRole::Radius),
        ordinal: compact::SET_ORDINAL,
        tangency_weight: compact::TANGENCY_WEIGHT_OWNER_REFERENCE,
    },
];

impl SetForm {
    /// Whether every byte outside the named fields of `frame` is zero.
    fn unnamed_bytes_are_zero(&self, frame: &[u8]) -> bool {
        let fields = [
            (0, INDEXED_HEADER_LEN),
            (self.marker, 1),
            (self.tangent_chain, 1),
            (
                self.curvature.unwrap_or(0),
                usize::from(self.curvature.is_some()),
            ),
            (self.group, REFERENCE_LEN),
            (self.law.0, REFERENCE_LEN),
            (self.ordinal, 4),
            (self.tangency_weight, REFERENCE_LEN),
        ];
        frame.iter().enumerate().all(|(at, byte)| {
            *byte == 0
                || fields
                    .iter()
                    .any(|(start, width)| (*start..start + width).contains(&at))
        })
    }
}

/// The fillet set record `record_index` at `position` in the prefix list,
/// admitted in exactly one form.
fn fillet_set(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    (record_index, position): (u32, u32),
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFilletSet> {
    let [start] = records.offsets(record_index) else {
        return None;
    };
    let start = *start;
    let mut matches = SET_FORMS.iter().filter_map(|form| {
        let end = start.checked_add(form.len)?;
        let frame = bytes.get(start..end)?;
        lp_ascii_filtered_view(bytes, end, 3..=3, u8::is_ascii_digit)?;
        if frame.get(form.marker) != Some(&1)
            || !matches!(frame.get(form.tangent_chain), Some(0 | 1))
            || form
                .curvature
                .is_some_and(|at| !matches!(frame.get(at), Some(0 | 1)))
            || View::u32_le_at(frame, form.ordinal)? != position
            || !form.unnamed_bytes_are_zero(frame)
        {
            return None;
        }
        let owner = |at: usize, source_kind: &str| {
            let owner = marked_record_reference(frame, at)?;
            (owned_source_kind(scope, owner, parameters, parameter_owners)? == source_kind)
                .then_some(owner)
        };
        let law_owner = owner(form.law.0, form.law.1.source_kind())?;
        let law = match form.law.1 {
            LawRole::ChordLength => DesignFilletSetLaw::Chordal {
                chord_length_owner_record_index: law_owner,
            },
            LawRole::Radius => DesignFilletSetLaw::Constant {
                radius_owner_record_index: law_owner,
            },
        };
        Some(DesignFilletSet {
            record_index,
            byte_offset: u64::try_from(start).ok()?,
            frame_length: u64::try_from(form.len).ok()?,
            group_record_index: marked_record_reference(frame, form.group)?,
            law,
            tangency_weight_owner_record_index: owner(form.tangency_weight, "TangencyWeight")?,
            tangent_chain: frame.get(form.tangent_chain) == Some(&1),
            curvature_continuity: form.curvature.map(|at| frame.get(at) == Some(&1)),
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

/// Whether radius group `group` uses the parameters that the owners of `set`
/// reference; `owned_parameter` maps an owner record to its parameter record.
pub(crate) fn fillet_set_matches_radius_group(
    set: &DesignFilletSet,
    group: &DesignFilletRadiusGroup,
    owned_parameter: impl Fn(u32) -> Option<u32>,
) -> bool {
    let law = match (set.law, &group.law) {
        (
            DesignFilletSetLaw::Constant {
                radius_owner_record_index,
            },
            DesignFilletRadiusLaw::Constant {
                radius_parameter_record_index,
            },
        ) => owned_parameter(radius_owner_record_index) == Some(*radius_parameter_record_index),
        (
            DesignFilletSetLaw::Chordal {
                chord_length_owner_record_index,
            },
            DesignFilletRadiusLaw::Chordal {
                chord_length_parameter_record_index,
            },
        ) => {
            owned_parameter(chord_length_owner_record_index)
                == Some(*chord_length_parameter_record_index)
        }
        _ => false,
    };
    law && group.tangency_weight_parameter_record_index.is_some()
        && owned_parameter(set.tangency_weight_owner_record_index)
            == group.tangency_weight_parameter_record_index
}

/// Construction-operand groups decoded for each scope, keyed by stream, scope
/// record, and group record.
pub(crate) type DecodedGroupKeys = std::collections::HashSet<(String, u32, u32)>;

/// Keys of `groups` before any group retention pass.
pub(crate) fn decoded_group_keys(groups: &[DesignConstructionOperandGroup]) -> DecodedGroupKeys {
    groups
        .iter()
        .filter_map(|group| {
            Some((
                native_stream(&group.id)?.to_owned(),
                group.scope_record_index,
                group.record_index,
            ))
        })
        .collect()
}

/// Clear each fillet set list that names an edge group outside the decoded
/// construction-operand groups of its scope, or whose owners disagree with the
/// radius group that the indexed-owner path assigns to the same edge group.
/// Returns the scopes cleared for disagreement with a radius group.
pub(crate) fn retain_consistent_fillet_sets(
    scopes: &mut [DesignParameterScope],
    decoded_groups: &DecodedGroupKeys,
    radius_groups: &[DesignFilletRadiusGroup],
    parameter_owners: &[DesignParameterOwner],
) -> Vec<String> {
    let mut conflicts = Vec::new();
    for scope in scopes {
        let Some(sets) = scope.fillet_sets() else {
            continue;
        };
        let stream = native_stream(&scope.id);
        let owned_parameter = |owner_record_index: u32| {
            let mut owners = parameter_owners.iter().filter(|owner| {
                native_stream(owner.id()) == stream
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == owner_record_index
            });
            let owner = owners.next()?;
            owners
                .next()
                .is_none()
                .then(|| owner.parameter_record_index())
        };
        let grouped = sets.sets.iter().all(|set| {
            stream.is_some_and(|stream| {
                decoded_groups.contains(&(
                    stream.to_owned(),
                    scope.record_index,
                    set.group_record_index,
                ))
            })
        });
        let agreeing = sets.sets.iter().all(|set| {
            radius_groups
                .iter()
                .filter(|group| {
                    native_stream(&group.id) == stream
                        && group.scope_record_index == scope.record_index
                        && group.group_record_index == set.group_record_index
                })
                .all(|group| fillet_set_matches_radius_group(set, group, owned_parameter))
        });
        if grouped && agreeing {
            continue;
        }
        if grouped {
            conflicts.push(scope.id.clone());
        }
        if let crate::records::feature::scope::DesignScopePayloadMut::Fillet(slot) =
            scope.payload_mut()
        {
            slot.sets = None;
        }
    }
    conflicts
}

/// The loss that names the Fillet scopes whose set lists were dropped for
/// disagreeing with their radius groups.
pub(crate) fn fillet_set_conflict_loss(
    conflicts: &[String],
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (!conflicts.is_empty()).then(|| {
        crate::loss::F3dLossCode::FeatureFilletSetConflict.note(format!(
            "{} Fillet scope(s) store set records whose owners differ from the radius groups paired by owner order; the radius groups are kept and the set lists are dropped: {}.",
            conflicts.len(),
            conflicts.join(", ")
        ))
    })
}
