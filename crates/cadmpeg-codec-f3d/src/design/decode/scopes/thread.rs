// SPDX-License-Identifier: Apache-2.0
//! Exact thread construction scopes and thread payloads.

use crate::bytes::lp_utf16_bounded;
use crate::layout::thread_compact_construction_tail as thread_compact_tail;
use crate::layout::thread_compact_hole_owners_prefix as hole_owners;
use crate::layout::thread_compact_legacy_construction_tail as thread_compact_legacy_tail;
use crate::layout::thread_compact_owned_scope_prefix as compact_owned;
use crate::layout::thread_extended_compact_trailer as extended_compact;
use crate::layout::thread_extended_scope_prefix as thread_extended;
use crate::layout::thread_extended_standard_trailer as extended_standard;
use crate::layout::thread_hole_companion_tail as companion_tail;
use crate::layout::thread_owner_extended_scope_prefix as owner_extended;
use crate::layout::thread_owner_marked_scope_prefix as thread_owner;
use crate::layout::thread_standard_construction_tail as thread_tail;
use crate::layout::thread_standard_legacy_construction_tail as thread_standard_legacy_tail;
use crate::layout::thread_standard_scope_prefix as thread_standard;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread;
use crate::records::feature::thread::DesignThreadConstruction;
use crate::records::feature::thread::DesignThreadForm;
use cadmpeg_core::decode::View;
use cadmpeg_ir::report::loss::LossNote;

/// The 88-byte companion tail of a Thread scope that owns a Hole.
#[derive(Debug, Clone, Copy)]
pub(super) struct HoleCompanionTail {
    /// Tail offset of the preceding history-state identity.
    pub(super) previous_history_state: usize,
    /// The suppression flag.
    pub(super) suppressed: bool,
}

pub(super) fn hole_companion_tail(tail: &[u8], thread: u32) -> Option<HoleCompanionTail> {
    let reference = |at| super::shared_frames::marked_record_reference(tail, at);
    let first = reference(companion_tail::FIRST_COMPANION)?;
    let hole = reference(companion_tail::HOLE)?;
    let duration = View::f64_le_at(tail, companion_tail::EVALUATION_DURATION)?;
    (tail.len() == companion_tail::LEN
        && tail.get(companion_tail::ZERO_PREFIX..companion_tail::FIRST_COMPANION)? == [0; 15]
        && tail.get(companion_tail::NULL_REFERENCE) == Some(&0)
        && first > 2
        && duration.is_finite()
        && duration >= 0.0
        && reference(companion_tail::SECOND_COMPANION)? == first.checked_sub(1)?
        && matches!(
            tail.get(companion_tail::FLAGS..companion_tail::THIRD_COMPANION)?,
            [0 | 1, 0, 0]
        )
        && reference(companion_tail::THIRD_COMPANION)? == first.checked_sub(2)?
        && hole != 0
        && hole < thread
        && tail.get(companion_tail::ZERO_SUFFIX) == Some(&0))
    .then_some(HoleCompanionTail {
        previous_history_state: companion_tail::PREVIOUS_HISTORY_STATE,
        suppressed: tail.get(companion_tail::FLAGS) == Some(&1),
    })
}

pub(super) fn exact_thread_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignThreadConstruction> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let bytes = &bytes[..end.min(bytes.len())];
    if scope.kind() != scope::DesignFeatureKind::Thread
        || scope.reference_members().len() < 2
        || !scope.reference_members().len().is_multiple_of(2)
    {
        return None;
    }
    let (prefix_form, designation_delta) = bytes.get(start..).and_then(exact_thread_prefix)?;
    let designation_at = start.checked_add(designation_delta)?;
    let reference_count = scope.reference_members().len();

    let mut face_group_record_indices = Vec::new();
    match prefix_form {
        ThreadPrefix::Standard => {
            let first = scope.reference_members().values().next()?;
            face_group_record_indices.push(*first);
        }
        ThreadPrefix::Compact => {
            face_group_record_indices
                .extend(scope.reference_members().values().step_by(2).copied());
        }
        ThreadPrefix::Extended(_)
        | ThreadPrefix::CompactOwned
        | ThreadPrefix::CompactHoleOwners => {}
    }
    let mut construction = parse_thread_payload(
        bytes,
        designation_at,
        &prefix_form,
        face_group_record_indices,
    )?;
    construction.angle = bytes.get(start..).and_then(prologue_thread_angle)?;
    if matches!(
        construction.form,
        DesignThreadForm::CompactExtended(_)
            | DesignThreadForm::CompactOwnersLeading
            | DesignThreadForm::CompactOwnersTrailing
    ) {
        let references = scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>();
        construction.face_group_record_indices = construction.form.face_groups(&references);
        if matches!(
            (&prefix_form, construction.form),
            (ThreadPrefix::Extended(prefix), DesignThreadForm::CompactExtended(_))
                if prefix.modeled && prefix.entry_count != construction.face_group_record_indices.len()
        ) {
            return None;
        }
    }
    if construction.form == DesignThreadForm::StandardExtended {
        let entries = match &prefix_form {
            ThreadPrefix::Extended(prefix) => prefix.entry_count,
            _ => return None,
        };
        construction.face_group_record_indices =
            extended_standard_face_groups(bytes, scope, owners, designation_at, entries)?;
    } else if matches!(
        construction.form,
        DesignThreadForm::CompactExtended(_)
            | DesignThreadForm::CompactOwnersLeading
            | DesignThreadForm::CompactOwnersTrailing
    ) {
        let (_, after_designation) = lp_utf16_bounded(bytes, designation_at, 1..=128)?;
        let (_, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
        let (_, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
        let trailer = after_profile.checked_add(thread_tail::STANDARD_TRAILER)?;
        let (length_at, offset_at, length_ordinal, offset_ordinal) = match construction.form {
            DesignThreadForm::CompactOwnersTrailing => (
                extended_standard::LENGTH_REFERENCE,
                extended_standard::OFFSET_REFERENCE,
                reference_count.checked_sub(1)?,
                reference_count.checked_sub(2)?,
            ),
            DesignThreadForm::CompactOwnersLeading => (
                extended_standard::LENGTH_REFERENCE,
                extended_standard::OFFSET_REFERENCE,
                1,
                0,
            ),
            DesignThreadForm::CompactExtended(_) => (
                extended_compact::LENGTH_REFERENCE,
                extended_compact::OFFSET_REFERENCE,
                1,
                0,
            ),
            _ => return None,
        };
        if super::shared_frames::marked_record_reference(bytes, trailer + length_at)
            != scope
                .reference_members()
                .values()
                .nth(length_ordinal)
                .copied()
            || super::shared_frames::marked_record_reference(bytes, trailer + offset_at)
                != scope
                    .reference_members()
                    .values()
                    .nth(offset_ordinal)
                    .copied()
        {
            return None;
        }
    }
    if !construction.form.owners_hold(scope, owners)
        || !construction
            .form
            .class_pair_holds(scope.class_tag.as_str(), scope.paired_class_tag.as_str())
        || !construction
            .form
            .designation_offset_holds(construction.designation_offset, scope.byte_offset())
        || !construction
            .form
            .reference_count_holds(scope.reference_members().len())
    {
        return None;
    }
    Some(construction)
}

/// The face-selection groups of an extended standard scope.
///
/// The trailer's length and offset references each name one scope reference,
/// the owner with local ordinal `1` and `0`. The other references are the
/// ordered `(group, member)` pairs.
fn extended_standard_face_groups(
    bytes: &[u8],
    scope: &DesignParameterScope,
    owners: &[crate::records::parameters::DesignParameterOwner],
    designation_at: usize,
    entries: usize,
) -> Option<Vec<u32>> {
    let (_, after_designation) = lp_utf16_bounded(bytes, designation_at, 1..=128)?;
    let (_, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
    let (_, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
    let trailer = after_profile.checked_add(thread_tail::STANDARD_TRAILER)?;
    let references = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let ordinal_of = |at: usize| {
        let record =
            super::shared_frames::marked_record_reference(bytes, trailer.checked_add(at)?)?;
        let mut ordinals = references
            .iter()
            .enumerate()
            .filter(|(_, reference)| **reference == record)
            .map(|(ordinal, _)| ordinal);
        let ordinal = ordinals.next()?;
        ordinals.next().is_none().then_some(ordinal)
    };
    let length = ordinal_of(extended_standard::LENGTH_REFERENCE)?;
    let offset = ordinal_of(extended_standard::OFFSET_REFERENCE)?;
    if length == offset
        || DesignThreadForm::StandardExtended.owner_ordinals(scope, owners)
            != Some((offset, length))
    {
        return None;
    }
    let groups = DesignThreadForm::extended_standard_face_groups(&references, (offset, length));
    (entries == 0 || entries == groups.len()).then_some(groups)
}

pub(super) fn thread_modeled_entries_loss(
    bytes: &[u8],
    scope: &DesignParameterScope,
    construction: &DesignThreadConstruction,
) -> Option<LossNote> {
    if !construction.modeled || construction.extended.is_none() {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let (ThreadPrefix::Extended(prefix), _) =
        exact_thread_prefix(bytes.get(start..end.min(bytes.len()))?)?
    else {
        return None;
    };
    (!prefix.modeled).then(|| {
        crate::loss::F3dLossCode::ThreadModeledEntriesAbsent.note(format!(
            "Thread scope {} stores modeled flag 1 and no extended-prefix face entries; it is read as a modeled thread (DR-205).",
            scope.id
        ))
    })
}

pub(super) fn thread_modeled_flag_loss(
    scope: &DesignParameterScope,
    construction: &DesignThreadConstruction,
) -> Option<LossNote> {
    (construction.extended.is_none()
        && !construction.modeled
        && scope.history_state_id().is_none())
    .then(|| {
        crate::loss::F3dLossCode::ThreadModeledFlagAssumed.note(format!(
            "Thread scope {} has no extended prefix, commits no history state, and stores modeled flag 0; it is read as a cosmetic thread, and a modeled thread stores the same value in such a scope (DR-142).",
            scope.id
        ))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ThreadPrefix {
    Standard,
    Compact,
    CompactOwned,
    CompactHoleOwners,
    /// The extended prefix and the fields it states before the designation.
    Extended(ExtendedThreadPrefix),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExtendedThreadPrefix {
    external: bool,
    modeled: bool,
    /// Count of the 16-byte entries.
    entry_count: usize,
    class: String,
    designation_copy: String,
}

/// Most 16-byte entries that an extended Thread prefix admits.
const EXTENDED_PREFIX_ENTRY_LIMIT: usize = 4096;

/// Byte length of each counted entry in an extended Thread prefix.
const EXTENDED_PREFIX_ENTRY_LEN: usize = 16;

/// Longest thread class, in UTF-16 code units, that the extended prefix admits.
const THREAD_CLASS_UNITS: usize = 16;

fn exact_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    let direct_prologue = bytes.get(thread_standard::ZERO_RUN_10..thread_standard::FIXED_SCALAR)?
        == [0; 10]
        && thread_angle_at(bytes, thread_standard::FIXED_SCALAR).is_some();
    let direct = if direct_prologue {
        thread_form(
            bytes,
            thread_standard::STANDARD_MARKER,
            thread_standard::STANDARD_PREFIX_TAIL,
        )
        .map(|form| (form, thread_standard::LEN))
    } else {
        None
    };
    let compact_hole_owners = (direct_prologue
        && View::u32_le_at(bytes, hole_owners::PREFIX_WORD) == Some(0)
        && thread_form(bytes, hole_owners::FORM_MARKER, hole_owners::FORM_TOKEN)
            == Some(ThreadPrefix::Compact))
    .then_some((ThreadPrefix::CompactHoleOwners, hole_owners::LEN))
    .filter(|(prefix, designation)| {
        parse_thread_payload(bytes, *designation, prefix, Vec::new()).is_some()
    });
    let owner_marked = if bytes.get(thread_owner::ZERO_RUN_9..thread_owner::OWNER_MARKER)? == [0; 9]
        && View::u32_le_at(bytes, thread_owner::OWNER_MARKER)? == 1
        && bytes.get(thread_owner::SEPARATOR) == Some(&0)
        && thread_angle_at(bytes, thread_owner::FIXED_SCALAR).is_some()
    {
        thread_form(bytes, thread_owner::FORM_MARKER, thread_owner::FORM_TOKEN)
            .map(|form| (form, thread_owner::LEN))
    } else {
        None
    };
    let compact_owned = owner_prologue(bytes)
        .then(|| {
            (View::u32_le_at(bytes, compact_owned::PREFIX_WORD) == Some(0)
                && thread_form(bytes, compact_owned::FORM_MARKER, compact_owned::FORM_TOKEN)
                    == Some(ThreadPrefix::Compact))
            .then_some((ThreadPrefix::CompactOwned, compact_owned::LEN))
        })
        .flatten()
        .filter(|(prefix, designation)| {
            parse_thread_payload(bytes, *designation, prefix, Vec::new()).is_some()
        });
    let extended = extended_thread_prefix(bytes).filter(|(prefix, designation)| {
        parse_thread_payload(bytes, *designation, prefix, Vec::new()).is_some()
    });
    let mut candidates = [
        direct,
        owner_marked,
        compact_hole_owners,
        compact_owned,
        extended,
    ]
    .into_iter()
    .flatten();
    let prefix = candidates.next()?;
    candidates.next().is_none().then_some(prefix)
}

/// The extended prefix: the external flag, the counted entries, the LP-UTF16
/// thread class, and the LP-UTF16 designation copy before the designation.
fn extended_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    let (external_at, count_at, entries_at) = if owner_prologue(bytes) {
        (
            owner_extended::EXTERNAL_FLAG,
            owner_extended::ENTRY_COUNT,
            owner_extended::LEN,
        )
    } else if bytes.get(thread_extended::ZERO_RUN_10..thread_extended::FIXED_SCALAR)? == [0; 10]
        && thread_angle_at(bytes, thread_extended::FIXED_SCALAR).is_some()
    {
        (
            thread_extended::EXTERNAL_FLAG,
            thread_extended::ENTRY_COUNT,
            thread_extended::LEN,
        )
    } else {
        return None;
    };
    let external = match bytes.get(external_at)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let entry_count = usize::try_from(View::u32_le_at(bytes, count_at)?).ok()?;
    if entry_count > EXTENDED_PREFIX_ENTRY_LIMIT {
        return None;
    }
    let modeled = entry_count != 0;
    let class_at = entries_at.checked_add(entry_count.checked_mul(EXTENDED_PREFIX_ENTRY_LEN)?)?;
    let (class, copy_at) = lp_utf16_bounded(bytes, class_at, 1..=THREAD_CLASS_UNITS)?;
    let (designation_copy, designation_at) = lp_utf16_bounded(bytes, copy_at, 0..=128)?;
    if !designation_copy.is_empty()
        && lp_utf16_bounded(bytes, designation_at, 1..=128)?.0 != designation_copy
    {
        return None;
    }
    Some((
        ThreadPrefix::Extended(ExtendedThreadPrefix {
            external,
            modeled,
            entry_count,
            class,
            designation_copy,
        }),
        designation_at,
    ))
}

/// The thread profile angle stored at `at`.
fn thread_angle_at(bytes: &[u8], at: usize) -> Option<thread::DesignThreadAngle> {
    thread::DesignThreadAngle::new(View::f64_le_at(bytes, at)?)
}

/// The thread profile angle of the direct or owner-marked prologue.
fn prologue_thread_angle(bytes: &[u8]) -> Option<thread::DesignThreadAngle> {
    if owner_prologue(bytes) {
        thread_angle_at(bytes, thread_owner::FIXED_SCALAR)
    } else if bytes.get(thread_standard::ZERO_RUN_10..thread_standard::FIXED_SCALAR)? == [0; 10] {
        thread_angle_at(bytes, thread_standard::FIXED_SCALAR)
    } else {
        None
    }
}

fn owner_prologue(bytes: &[u8]) -> bool {
    bytes.get(thread_owner::ZERO_RUN_9..thread_owner::OWNER_MARKER) == Some(&[0; 9])
        && View::u32_le_at(bytes, thread_owner::OWNER_MARKER) == Some(1)
        && bytes.get(thread_owner::SEPARATOR) == Some(&0)
        && thread_angle_at(bytes, thread_owner::FIXED_SCALAR).is_some()
}

fn thread_form(bytes: &[u8], marker_at: usize, token_at: usize) -> Option<ThreadPrefix> {
    match (
        bytes.get(marker_at..marker_at + 5)?,
        bytes.get(token_at..token_at + 4)?,
    ) {
        ([1, 2, 0, 0, 0], [0x36, 0, 0x67, 0]) => Some(ThreadPrefix::Standard),
        ([0, 2, 0, 0, 0], [0x36, 0, 0x48, 0]) => Some(ThreadPrefix::Compact),
        _ => None,
    }
}

pub(super) fn parse_thread_payload(
    bytes: &[u8],
    designation_at: usize,
    expected_form: &ThreadPrefix,
    face_group_record_indices: Vec<u32>,
) -> Option<DesignThreadConstruction> {
    let (designation, after_designation) = lp_utf16_bounded(bytes, designation_at, 1..=128)?;
    let (nominal_size_text, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
    let (profile, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
    (|| {
        let extended = match expected_form {
            ThreadPrefix::Extended(prefix) => Some(prefix),
            ThreadPrefix::Standard
            | ThreadPrefix::Compact
            | ThreadPrefix::CompactOwned
            | ThreadPrefix::CompactHoleOwners => None,
        };
        if extended.is_some_and(|prefix| {
            !prefix.designation_copy.is_empty() && prefix.designation_copy != designation
        }) {
            return None;
        }
        let zero_flag_trailer = bytes.get(after_profile + thread_tail::STANDARD_TRAILER)
            == Some(&extended_standard::ZERO_FLAG_VALUE);
        let stated_modeled = match bytes.get(after_profile + thread_tail::MODELED_FLAG)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let modeled = match extended {
            Some(prefix) => prefix.modeled || stated_modeled,
            None => stated_modeled,
        };
        let entries_absent = extended.is_some_and(|prefix| stated_modeled && !prefix.modeled);
        let (trailer_kind, full_length) =
            match (expected_form, bytes.get(after_profile..after_profile + 5)?) {
                (ThreadPrefix::Extended(_), [flag @ (0 | 1), 1 | 2, 0, 0, 0])
                    if zero_flag_trailer =>
                {
                    (ThreadTrailerKind::ExtendedStandard, Some(*flag == 1))
                }
                (ThreadPrefix::CompactOwned, [0, 1, 0, 0, 0]) => {
                    (ThreadTrailerKind::CompactOwnersLeading, None)
                }
                (ThreadPrefix::CompactOwned, [0, 2, 0, 0, 0]) => {
                    (ThreadTrailerKind::CompactOwnersTrailing, None)
                }
                (ThreadPrefix::Standard, [0, 1, 0, 0, 0]) => (ThreadTrailerKind::Standard, None),
                (ThreadPrefix::Standard, [1, 1, 0, 0, 0]) => {
                    (ThreadTrailerKind::StandardLegacy, None)
                }
                (ThreadPrefix::Standard, [1, 2, 0, 0, 0]) => {
                    (ThreadTrailerKind::StandardCompact, None)
                }
                (ThreadPrefix::Compact, [1, 2, 0, 0, 0]) => (ThreadTrailerKind::Compact, None),
                (ThreadPrefix::Compact, [0, 2, 0, 0, 0])
                | (ThreadPrefix::CompactHoleOwners, [0 | 1, 2, 0, 0, 0]) => {
                    (ThreadTrailerKind::CompactWithOwners, None)
                }
                (ThreadPrefix::Extended(prefix), [0, 2, 0, 0, 0]) if !prefix.external => {
                    (ThreadTrailerKind::CompactWithOwners, None)
                }
                (ThreadPrefix::Extended(prefix), [1, 2, 0, 0, 0]) if !prefix.modeled => {
                    (ThreadTrailerKind::Compact, None)
                }
                (ThreadPrefix::Compact, [1, 1, 0, 0, 0]) => {
                    (ThreadTrailerKind::CompactLegacy, None)
                }
                _ => return None,
            };
        if entries_absent && trailer_kind != ThreadTrailerKind::ExtendedStandard {
            return None;
        }
        let nominal_size = thread::DesignThreadNominalSize::try_from(nominal_size_text).ok()?;
        let major_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MAJOR_DIAMETER)?;
        let minor_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MINOR_DIAMETER)?;
        let pitch = View::f64_le_at(bytes, after_profile + thread_tail::PITCH)?;
        let pitch_diameter = View::f64_le_at(bytes, after_profile + thread_tail::PITCH_DIAMETER)?;
        let trailer_at = match trailer_kind {
            ThreadTrailerKind::Standard => thread_tail::STANDARD_TRAILER,
            ThreadTrailerKind::Compact | ThreadTrailerKind::CompactWithOwners => {
                thread_compact_tail::COMPACT_TRAILER
            }
            ThreadTrailerKind::StandardLegacy => thread_standard_legacy_tail::LEGACY_TRAILER,
            ThreadTrailerKind::CompactLegacy => thread_compact_legacy_tail::LEGACY_TRAILER,
            ThreadTrailerKind::StandardCompact
            | ThreadTrailerKind::ExtendedStandard
            | ThreadTrailerKind::CompactOwnersLeading
            | ThreadTrailerKind::CompactOwnersTrailing => thread_tail::STANDARD_TRAILER,
        };
        let trailer_offset = after_profile.checked_add(trailer_at)?;
        let right_handed = match trailer_kind {
            ThreadTrailerKind::ExtendedStandard => {
                match bytes.get(trailer_offset + extended_standard::RIGHT_HANDED)? {
                    0 => Some(false),
                    1 => Some(true),
                    _ => return None,
                }
            }
            _ => None,
        };
        let form = match trailer_kind {
            ThreadTrailerKind::ExtendedStandard
                if bytes.get(trailer_offset + extended_standard::ZERO_FLAG)
                    == Some(&extended_standard::ZERO_FLAG_VALUE) =>
            {
                DesignThreadForm::StandardExtended
            }
            ThreadTrailerKind::CompactOwnersLeading | ThreadTrailerKind::CompactOwnersTrailing
                if bytes.get(trailer_offset + extended_standard::ZERO_FLAG)
                    == Some(&extended_standard::ZERO_FLAG_VALUE)
                    && bytes.get(trailer_offset + extended_standard::RIGHT_HANDED) == Some(&1) =>
            {
                match trailer_kind {
                    ThreadTrailerKind::CompactOwnersLeading => {
                        DesignThreadForm::CompactOwnersLeading
                    }
                    ThreadTrailerKind::CompactOwnersTrailing => {
                        DesignThreadForm::CompactOwnersTrailing
                    }
                    _ => DesignThreadForm::StandardExtended,
                }
            }
            ThreadTrailerKind::Standard
                if bytes.get(trailer_offset..trailer_offset + 2)? == [0, 1] =>
            {
                DesignThreadForm::Standard
            }
            ThreadTrailerKind::StandardLegacy
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::StandardLegacy
            }
            ThreadTrailerKind::CompactLegacy
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::CompactLegacy
            }
            ThreadTrailerKind::StandardCompact
                if standard_compact_trailer_is_valid(
                    bytes,
                    trailer_offset,
                    face_group_record_indices.first().copied()?,
                ) =>
            {
                DesignThreadForm::StandardCompact
            }
            ThreadTrailerKind::Compact
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::Compact(None)
            }
            ThreadTrailerKind::Compact | ThreadTrailerKind::CompactWithOwners
                if bytes.get(trailer_offset) == Some(&1)
                    && bytes.get(trailer_offset + 5..trailer_offset + 11)? == [0; 6] =>
            {
                let reference_offset = trailer_offset.checked_add(1)?;
                let record_index =
                    std::num::NonZeroU32::new(View::u32_le_at(bytes, reference_offset)?)?;
                let reference = crate::records::identity::Located {
                    value: record_index,
                    offset: u64::try_from(reference_offset).ok()?,
                };
                if extended.is_some() || trailer_kind == ThreadTrailerKind::CompactWithOwners {
                    (bytes.get(trailer_offset + extended_compact::END_FLAG)
                        == Some(&extended_compact::END_FLAG_VALUE))
                    .then_some(())?;
                    DesignThreadForm::CompactExtended(reference)
                } else {
                    DesignThreadForm::Compact(Some(reference))
                }
            }
            _ => return None,
        };
        if extended.is_some()
            && !matches!(
                form,
                DesignThreadForm::CompactExtended(_) | DesignThreadForm::StandardExtended
            )
        {
            return None;
        }
        Some(DesignThreadConstruction {
            angle: crate::records::feature::thread::DesignThreadAngle::SIXTY,
            form,
            designation_offset: u64::try_from(designation_at).ok()?,
            designation: cadmpeg_core::text::NonBlankString::new(designation)?,
            nominal_size,
            profile: cadmpeg_core::text::NonBlankString::new(profile)?,
            pitch: cadmpeg_ir::scalar::PositiveReal::new(pitch)?,
            modeled,
            face_group_record_indices,
            diameters: thread::DesignThreadDiameters::new(
                major_diameter,
                minor_diameter,
                pitch_diameter,
            )?,
            extended: extended.map(|prefix| thread::DesignThreadExtended {
                class: prefix.class.clone(),
                external: prefix.external,
                full_length,
                right_handed,
            }),
        })
    })()
}

/// Admit the trailer of the standard-prefix compact-marker form: a run of two
/// or three zero bytes, the byte `01`, u32 `1`, and a marked reference to the
/// face-selection group followed by four zero bytes.
fn standard_compact_trailer_is_valid(
    bytes: &[u8],
    trailer_offset: usize,
    face_group_record_index: u32,
) -> bool {
    let Some(tail) = bytes.get(trailer_offset..) else {
        return false;
    };
    let zero_run = tail.iter().take_while(|byte| **byte == 0).count();
    if !STANDARD_COMPACT_ZERO_RUNS.contains(&zero_run) {
        return false;
    }
    let Some(flag_at) = trailer_offset.checked_add(zero_run) else {
        return false;
    };
    let Some(count_at) = flag_at.checked_add(1) else {
        return false;
    };
    let Some(marker_at) = count_at.checked_add(4) else {
        return false;
    };
    let Some(index_at) = marker_at.checked_add(1) else {
        return false;
    };
    bytes.get(flag_at) == Some(&1)
        && View::u32_le_at(bytes, count_at) == Some(1)
        && bytes.get(marker_at) == Some(&1)
        && View::u32_le_at(bytes, index_at) == Some(face_group_record_index)
        && index_at
            .checked_add(4)
            .and_then(|padding_at| {
                bytes.get(padding_at..padding_at.checked_add(REFERENCE_PADDING)?)
            })
            .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
}

/// Zero bytes after the u32 record index of the trailer's marked reference.
const REFERENCE_PADDING: usize = 4;

/// Zero-run lengths before the trailer `01` of the standard-prefix compact-marker form.
const STANDARD_COMPACT_ZERO_RUNS: std::ops::RangeInclusive<usize> = 2..=3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadTrailerKind {
    Standard,
    Compact,
    CompactWithOwners,
    StandardLegacy,
    CompactLegacy,
    StandardCompact,
    ExtendedStandard,
    CompactOwnersLeading,
    CompactOwnersTrailing,
}
