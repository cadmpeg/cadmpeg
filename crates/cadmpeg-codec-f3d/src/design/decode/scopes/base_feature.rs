// SPDX-License-Identifier: Apache-2.0
//! Exact Base Feature constructions.

use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::text::{fixed_guid_end, fixed_relaxed_guid_text};
use crate::layout::base_feature_body_snapshot_body_entry as snapshot_entry;
use crate::layout::base_feature_body_snapshot_compact_preamble as snapshot_compact_preamble;
use crate::layout::base_feature_body_snapshot_expanded_preamble as snapshot_expanded_preamble;
use crate::layout::base_feature_body_snapshot_guid as snapshot_guid;
use crate::layout::base_feature_body_snapshot_linkage_tail as snapshot_tail;
use crate::layout::base_feature_body_snapshot_prefix as snapshot;
use crate::layout::base_feature_body_snapshot_scope_prefix as snapshot_scope;
use crate::layout::base_feature_class_377_prefix as class_377;
use crate::layout::base_feature_class_452_262_compact as class_452_compact;
use crate::layout::base_feature_class_452_262_expanded as class_452_expanded;
use crate::layout::base_feature_compact_repeated_body_entry as compact_entry;
use crate::layout::base_feature_compact_result_body_count as compact_count;
use crate::layout::base_feature_legacy_444_zero_body as legacy_444_zero_body;
use crate::layout::base_feature_legacy_zero_body as legacy_zero_body;
use crate::layout::base_feature_result_body_entry as result_body_entry;
use crate::layout::base_feature_result_body_prefix as result_body;
use crate::records::feature::base_feature::DesignBaseFeatureBodyReferenceForm;
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::base_feature::DesignBaseFeatureEntry;
use crate::records::feature::base_feature::DesignBaseFeatureResultBody;
use crate::records::feature::base_feature::DesignBaseFeatureResults;
use crate::records::feature::base_feature::DesignLegacyBaseFeatureBody;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use super::shared_frames::marked_record_reference;

/// The kind name "Base Feature" as twelve UTF-16LE code units.
const BASE_FEATURE_KIND_UTF16: [u8; 24] = *b"B\0a\0s\0e\0 \0F\0e\0a\0t\0u\0r\0e\0";
/// Code-unit count of [`BASE_FEATURE_KIND_UTF16`].
const BASE_FEATURE_KIND_UNITS: u32 = 12;
/// Length of one repeated result entry: a marker, a 32-bit value and a
/// six-byte field.
const RESULT_ENTRY_LEN: usize = 11;

fn marked_u64_reference(bytes: &[u8], marker_offset: usize, marker_value: u8) -> Option<u64> {
    if bytes.get(marker_offset) != Some(&marker_value) {
        return None;
    }
    View::u64_le_at(bytes, marker_offset + 1)
}

fn exact_body_based_on_faces_property(
    bytes: &[u8],
    start: usize,
    property_marker: usize,
) -> Option<()> {
    if bytes.get(start + property_marker) != Some(&class_377::TAG_BODY_BASED_ON_FACES_MARKER_VALUE)
        || View::u32_le_at(bytes, start + property_marker + 1)?
            != class_377::TAG_BODY_BASED_ON_FACES_COUNT_VALUE
        || View::u32_le_at(bytes, start + property_marker + 5)?
            != class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE
        || bytes.get(start + property_marker + 9..start + property_marker + 28)?
            != b"TagBodyBasedOnFaces"
        || View::u32_le_at(bytes, start + property_marker + 28)?
            != class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE
        || bytes.get(start + property_marker + 32..start + property_marker + 53)?
            != b"IntrinsicMetaTypebool"
        || View::u16_le_at(bytes, start + property_marker + 53)?
            != class_377::TAG_BODY_BASED_ON_FACES_VALUE_VALUE
    {
        return None;
    }
    Some(())
}

#[derive(Clone, Copy)]
struct BaseFeatureScopeTailLayout {
    frame_length: usize,
    reference_count: usize,
    generic_scope_reference_marker: usize,
    generic_scope_reference_record: usize,
    generic_scope_reference_field: usize,
    history_state_id: usize,
    kind_length: usize,
    kind: usize,
    feature_ordinal: usize,
    previous_history_state_id: usize,
}

/// The generic scope tail of a legacy Base Feature frame at `start`: one
/// scope reference, the history states, the "Base Feature" kind and the
/// feature ordinal, each where the scope parse found it.
fn exact_base_feature_scope_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    layout: BaseFeatureScopeTailLayout,
) -> Option<()> {
    let [member] = scope.reference_members().values_array::<1>()?;
    let at = |offset: usize| scope.byte_offset().checked_add(u64::try_from(offset).ok()?);
    if scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset())
        || scope.frame_length() != u64::try_from(layout.frame_length).ok()?
        || Some(scope.reference_count_offset()) != at(layout.reference_count)
        || scope.reference_members().offsets().next().copied()
            != at(layout.generic_scope_reference_record)
        || Some(scope.kind_offset()) != at(layout.kind)
        || Some(scope.feature_ordinal_offset()) != at(layout.feature_ordinal)
        || scope.previous_history_state_id_offset() != at(layout.previous_history_state_id)
        || View::u32_le_at(bytes, start + layout.reference_count)?
            != class_377::REFERENCE_COUNT_VALUE
        || bytes.get(start + layout.generic_scope_reference_marker)
            != Some(&class_377::GENERIC_SCOPE_REFERENCE_MARKER_VALUE)
        || marked_record_reference(bytes, start + layout.generic_scope_reference_marker)
            != Some(*member)
        || !zeros_at::<6>(bytes, start + layout.generic_scope_reference_field)
        || View::u32_le_at(bytes, start + layout.history_state_id)?
            != scope
                .history_state_id()
                .and_then(|id| u32::try_from(id).ok())?
        || View::u32_le_at(bytes, start + layout.kind_length)? != class_377::KIND_LENGTH_VALUE
        || bytes_at::<24>(bytes, start + layout.kind) != Some(&BASE_FEATURE_KIND_UTF16)
        || View::u32_le_at(bytes, start + layout.feature_ordinal)? != scope.feature_ordinal.get()
    {
        return None;
    }
    let previous_state = View::u32_le_at(bytes, start + layout.previous_history_state_id)?;
    let previous_matches = match scope.previous_history_state_id() {
        Some(id) => u32::try_from(id).ok() == Some(previous_state),
        None => previous_state == u32::MAX,
    };
    previous_matches.then_some(())
}

fn exact_base_feature_legacy_body_based_on_faces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    if scope.class_tag.as_str() != "452" || scope.paired_class_tag.as_str() != "262" {
        return Ok(None);
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    match View::u32_le_at(bytes, start + class_452_compact::BODY_COUNT) {
        Some(1) => exact_base_feature_legacy_compact(ctx, bytes, scope, start),
        Some(2) => exact_base_feature_legacy_expanded(ctx, bytes, scope, start),
        _ => Ok(None),
    }
}

/// The fixed fields of a compact one-body legacy envelope around its GUID.
struct LegacyCompactFields {
    mode: crate::records::feature::base_feature::DesignBaseFeatureCompactMode,
    body_entity_suffix: u32,
    body_entity_field: [u8; 6],
    parameter_body_record: u64,
    scope_reference: u64,
    auxiliary_record: u64,
}

fn legacy_compact_fields(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<LegacyCompactFields> {
    if scope.frame_length() != u64::try_from(class_452_compact::LEN).ok()?
        || !zeros_at::<8>(bytes, start + class_452_compact::ZERO_RUN_8)
        || bytes.get(start + class_452_compact::BODY_COUNT_MARKER)
            != Some(&class_452_compact::BODY_COUNT_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_452_compact::BODY_COUNT)?
            != class_452_compact::BODY_COUNT_VALUE
    {
        return None;
    }
    let body_entity_suffix = marked_u64_reference(
        bytes,
        start + class_452_compact::BODY_ENTITY_REFERENCE_MARKER,
        class_452_compact::BODY_ENTITY_REFERENCE_MARKER_VALUE,
    )?;
    if body_entity_suffix == 0 {
        return None;
    }
    let body_entity_field = *bytes_at::<6>(
        bytes,
        start + class_452_compact::BODY_ENTITY_REFERENCE_FIELD,
    )?;
    exact_body_based_on_faces_property(
        bytes,
        start,
        class_452_compact::TAG_BODY_BASED_ON_FACES_MARKER,
    )?;
    let mode = crate::records::feature::base_feature::DesignBaseFeatureCompactMode::try_from(
        *bytes.get(start + class_452_compact::MODE)?,
    )
    .ok()?;
    let parameter_body_record = marked_u64_reference(
        bytes,
        start + class_452_compact::PARAMETER_BODY_REFERENCE_MARKER,
        class_452_compact::PARAMETER_BODY_REFERENCE_MARKER_VALUE,
    )?;
    let scope_reference = marked_u64_reference(
        bytes,
        start + class_452_compact::SCOPE_REFERENCE_MARKER,
        class_452_compact::SCOPE_REFERENCE_MARKER_VALUE,
    )?;
    let auxiliary_record = marked_u64_reference(
        bytes,
        start + class_452_compact::AUXILIARY_REFERENCE_MARKER,
        class_452_compact::AUXILIARY_REFERENCE_MARKER_VALUE,
    )?;
    if bytes.get(start + class_452_compact::PARAMETER_BODY_COUNT)
        != Some(&class_452_compact::PARAMETER_BODY_COUNT_VALUE)
        || !zeros_at::<3>(bytes, start + class_452_compact::PARAMETER_BODY_ZERO_RUN)
        || parameter_body_record == 0
        || !zeros_at::<3>(
            bytes,
            start + class_452_compact::PARAMETER_BODY_REFERENCE_FIELD,
        )
        || scope_reference == 0
        || !zeros_at::<2>(bytes, start + class_452_compact::SCOPE_REFERENCE_FIELD)
        || bytes.get(start + class_452_compact::AUXILIARY_GROUP_MARKER)
            != Some(&class_452_compact::AUXILIARY_GROUP_MARKER_VALUE)
        || !zeros_at::<3>(bytes, start + class_452_compact::AUXILIARY_GROUP_ZERO_RUN)
        || auxiliary_record == 0
        || !zeros_at::<10>(bytes, start + class_452_compact::AUXILIARY_REFERENCE_FIELD)
        || scope.reference_members().values_array::<1>()
            != Some([&u32::try_from(scope_reference).ok()?])
        || !zeros_at::<3>(bytes, start + class_452_compact::ZERO_RUN_AFTER_GUID)
    {
        return None;
    }
    exact_base_feature_scope_tail(
        bytes,
        scope,
        start,
        BaseFeatureScopeTailLayout {
            frame_length: class_452_compact::LEN,
            reference_count: class_452_compact::REFERENCE_COUNT,
            generic_scope_reference_marker: class_452_compact::GENERIC_SCOPE_REFERENCE_MARKER,
            generic_scope_reference_record: class_452_compact::GENERIC_SCOPE_REFERENCE_RECORD,
            generic_scope_reference_field: class_452_compact::GENERIC_SCOPE_REFERENCE_FIELD,
            history_state_id: class_452_compact::HISTORY_STATE_ID,
            kind_length: class_452_compact::KIND_LENGTH,
            kind: class_452_compact::KIND,
            feature_ordinal: class_452_compact::FEATURE_ORDINAL,
            previous_history_state_id: class_452_compact::PREVIOUS_HISTORY_STATE_ID,
        },
    )?;
    Some(LegacyCompactFields {
        mode,
        body_entity_suffix: u32::try_from(body_entity_suffix).ok()?,
        body_entity_field,
        parameter_body_record,
        scope_reference,
        auxiliary_record,
    })
}

fn exact_base_feature_legacy_compact(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let Some(fields) = legacy_compact_fields(bytes, scope, start) else {
        return Ok(None);
    };
    let Some((envelope_guid, guid_end)) = fixed_relaxed_guid_text(
        ctx,
        bytes,
        start + class_452_compact::ENVELOPE_GUID_CODE_UNIT_COUNT,
    )?
    else {
        return Ok(None);
    };
    if guid_end != start + class_452_compact::ZERO_RUN_AFTER_GUID {
        return Ok(None);
    }
    let at = |offset: usize| scope.byte_offset() + u64_from_index(offset);
    Ok(Some(
        DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
            form: DesignBaseFeatureBodyReferenceForm::CompactOneBody {
                mode: Located {
                    value: fields.mode,
                    offset: at(class_452_compact::MODE),
                },
                body: DesignLegacyBaseFeatureBody {
                    entity: DesignBaseFeatureEntry {
                        value: fields.body_entity_suffix,
                        offset: at(class_452_compact::BODY_ENTITY_SUFFIX),
                        field: fields.body_entity_field,
                    },
                    parameter_body: Located {
                        value: fields.parameter_body_record,
                        offset: at(class_452_compact::PARAMETER_BODY_RECORD),
                    },
                    auxiliary: Located {
                        value: fields.auxiliary_record,
                        offset: at(class_452_compact::AUXILIARY_RECORD),
                    },
                },
            },
            scope_reference: fields.scope_reference,
            scope_reference_offset: at(class_452_compact::SCOPE_REFERENCE),
            envelope_guid,
            envelope_guid_offset: at(class_452_compact::ENVELOPE_GUID),
            tag_body_based_on_faces_offset: at(class_452_compact::TAG_BODY_BASED_ON_FACES_VALUE),
        },
    ))
}

/// The fixed fields of an expanded two-body legacy envelope around its GUID.
struct LegacyExpandedFields {
    body_entity_suffixes: [u32; 2],
    body_entity_fields: [[u8; 6]; 2],
    parameter_body_records: [u32; 2],
    scope_reference: u32,
    auxiliary_records: [u32; 2],
}

fn legacy_expanded_fields(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<LegacyExpandedFields> {
    if scope.frame_length() != u64::try_from(class_452_expanded::LEN).ok()?
        || !zeros_at::<8>(bytes, start + class_452_expanded::ZERO_RUN_8)
        || bytes.get(start + class_452_expanded::BODY_COUNT_MARKER)
            != Some(&class_452_expanded::BODY_COUNT_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_452_expanded::BODY_COUNT)?
            != class_452_expanded::BODY_COUNT_VALUE
    {
        return None;
    }
    let body_entity_suffixes = [
        marked_u64_reference(
            bytes,
            start + class_452_expanded::BODY_ENTITY_ONE_MARKER,
            class_452_expanded::BODY_ENTITY_ONE_MARKER_VALUE,
        )?,
        marked_u64_reference(
            bytes,
            start + class_452_expanded::BODY_ENTITY_TWO_MARKER,
            class_452_expanded::BODY_ENTITY_TWO_MARKER_VALUE,
        )?,
    ];
    if body_entity_suffixes.contains(&0) {
        return None;
    }
    let body_entity_fields = [
        *bytes_at::<6>(bytes, start + class_452_expanded::BODY_ENTITY_ONE_FIELD)?,
        *bytes_at::<6>(bytes, start + class_452_expanded::BODY_ENTITY_TWO_FIELD)?,
    ];
    exact_body_based_on_faces_property(
        bytes,
        start,
        class_452_expanded::TAG_BODY_BASED_ON_FACES_MARKER,
    )?;
    let parameter_body_records = [
        marked_record_reference(bytes, start + class_452_expanded::PARAMETER_BODY_ONE_MARKER)?,
        marked_record_reference(bytes, start + class_452_expanded::PARAMETER_BODY_TWO_MARKER)?,
    ];
    let scope_reference =
        marked_record_reference(bytes, start + class_452_expanded::SCOPE_REFERENCE_MARKER)?;
    let auxiliary_records = [
        marked_record_reference(bytes, start + class_452_expanded::AUXILIARY_BODY_ONE_MARKER)?,
        marked_record_reference(bytes, start + class_452_expanded::AUXILIARY_BODY_TWO_MARKER)?,
    ];
    if parameter_body_records.contains(&0)
        || auxiliary_records.contains(&0)
        || bytes.get(start + class_452_expanded::PARAMETER_BODY_GROUP_MARKER)
            != Some(&class_452_expanded::PARAMETER_BODY_GROUP_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_452_expanded::PARAMETER_BODY_COUNT)?
            != class_452_expanded::PARAMETER_BODY_COUNT_VALUE
        || !zeros_at::<6>(bytes, start + class_452_expanded::PARAMETER_BODY_ONE_FIELD)
        || !zeros_at::<6>(bytes, start + class_452_expanded::PARAMETER_BODY_TWO_FIELD)
        || bytes.get(start + class_452_expanded::PARAMETER_BODY_SEPARATOR)
            != Some(&class_452_expanded::PARAMETER_BODY_SEPARATOR_VALUE)
        || Some(&scope_reference) != scope.reference_members().values().next()
        || !zeros_at::<6>(bytes, start + class_452_expanded::SCOPE_REFERENCE_FIELD)
        || View::u32_le_at(bytes, start + class_452_expanded::AUXILIARY_BODY_COUNT)?
            != class_452_expanded::AUXILIARY_BODY_COUNT_VALUE
        || !zeros_at::<6>(bytes, start + class_452_expanded::AUXILIARY_BODY_ONE_FIELD)
        || !zeros_at::<6>(bytes, start + class_452_expanded::AUXILIARY_BODY_TWO_FIELD)
        || !zeros_at::<8>(bytes, start + class_452_expanded::AUXILIARY_BODY_ZERO_RUN)
        || !zeros_at::<3>(bytes, start + class_452_expanded::ZERO_RUN_AFTER_GUID)
    {
        return None;
    }
    exact_base_feature_scope_tail(
        bytes,
        scope,
        start,
        BaseFeatureScopeTailLayout {
            frame_length: class_452_expanded::LEN,
            reference_count: class_452_expanded::REFERENCE_COUNT,
            generic_scope_reference_marker: class_452_expanded::GENERIC_SCOPE_REFERENCE_MARKER,
            generic_scope_reference_record: class_452_expanded::GENERIC_SCOPE_REFERENCE_RECORD,
            generic_scope_reference_field: class_452_expanded::GENERIC_SCOPE_REFERENCE_FIELD,
            history_state_id: class_452_expanded::HISTORY_STATE_ID,
            kind_length: class_452_expanded::KIND_LENGTH,
            kind: class_452_expanded::KIND,
            feature_ordinal: class_452_expanded::FEATURE_ORDINAL,
            previous_history_state_id: class_452_expanded::PREVIOUS_HISTORY_STATE_ID,
        },
    )?;
    Some(LegacyExpandedFields {
        body_entity_suffixes: [
            u32::try_from(body_entity_suffixes[0]).ok()?,
            u32::try_from(body_entity_suffixes[1]).ok()?,
        ],
        body_entity_fields,
        parameter_body_records,
        scope_reference,
        auxiliary_records,
    })
}

fn exact_base_feature_legacy_expanded(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let Some(fields) = legacy_expanded_fields(bytes, scope, start) else {
        return Ok(None);
    };
    let Some((envelope_guid, guid_end)) = fixed_relaxed_guid_text(
        ctx,
        bytes,
        start + class_452_expanded::ENVELOPE_GUID_CODE_UNIT_COUNT,
    )?
    else {
        return Ok(None);
    };
    if guid_end != start + class_452_expanded::ZERO_RUN_AFTER_GUID {
        return Ok(None);
    }
    let at = |offset: usize| scope.byte_offset() + u64_from_index(offset);
    let body = |ordinal: usize, suffix: usize, parameter_body: usize, auxiliary: usize| {
        DesignLegacyBaseFeatureBody {
            entity: DesignBaseFeatureEntry {
                value: fields.body_entity_suffixes[ordinal],
                offset: at(suffix),
                field: fields.body_entity_fields[ordinal],
            },
            parameter_body: Located {
                value: u64::from(fields.parameter_body_records[ordinal]),
                offset: at(parameter_body),
            },
            auxiliary: Located {
                value: u64::from(fields.auxiliary_records[ordinal]),
                offset: at(auxiliary),
            },
        }
    };
    Ok(Some(
        DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
            form: DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody {
                bodies: [
                    body(
                        0,
                        class_452_expanded::BODY_ENTITY_ONE_SUFFIX,
                        class_452_expanded::PARAMETER_BODY_ONE_RECORD,
                        class_452_expanded::AUXILIARY_BODY_ONE_RECORD,
                    ),
                    body(
                        1,
                        class_452_expanded::BODY_ENTITY_TWO_SUFFIX,
                        class_452_expanded::PARAMETER_BODY_TWO_RECORD,
                        class_452_expanded::AUXILIARY_BODY_TWO_RECORD,
                    ),
                ],
            },
            scope_reference: u64::from(fields.scope_reference),
            scope_reference_offset: at(class_452_expanded::SCOPE_REFERENCE),
            envelope_guid,
            envelope_guid_offset: at(class_452_expanded::ENVELOPE_GUID),
            tag_body_based_on_faces_offset: at(class_452_expanded::TAG_BODY_BASED_ON_FACES_VALUE),
        },
    ))
}

/// The fixed fields of a class-365/class-377 direct envelope, before its GUID.
struct DirectFields {
    start: usize,
    parameter_body_record: u32,
    body_entity_suffix: u32,
    auxiliary_record: u32,
}

fn direct_body_based_on_faces_fields(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DirectFields> {
    let [member] = scope.reference_members().values_array::<1>()?;
    let at = |offset: usize| scope.byte_offset().checked_add(u64::try_from(offset).ok()?);
    if !matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("365", "262") | ("377", "259")
    ) || scope.frame_length() != u64::try_from(class_377::LEN).ok()?
        || scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset())
        || Some(scope.reference_count_offset()) != at(class_377::REFERENCE_COUNT)
        || scope.reference_members().offsets().next().copied()
            != at(class_377::GENERIC_SCOPE_REFERENCE_RECORD)
        || Some(scope.kind_offset()) != at(class_377::KIND_LENGTH + 4)
        || Some(scope.feature_ordinal_offset()) != at(class_377::FEATURE_ORDINAL)
        || scope.previous_history_state_id_offset() != at(class_377::PREVIOUS_HISTORY_STATE_ID)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if !zeros_at::<8>(bytes, start + class_377::ZERO_RUN_8)
        || bytes.get(start + class_377::BODY_REFERENCE_COUNT_MARKER)
            != Some(&class_377::BODY_REFERENCE_COUNT_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_377::BODY_REFERENCE_COUNT)?
            != class_377::BODY_REFERENCE_COUNT_VALUE
    {
        return None;
    }
    let parameter_body_record =
        marked_record_reference(bytes, start + class_377::PARAMETER_BODY_REFERENCE_MARKER)?;
    let body_entity_suffix =
        marked_record_reference(bytes, start + class_377::BODY_ENTITY_REFERENCE_MARKER)?;
    if parameter_body_record == 0
        || body_entity_suffix == 0
        || View::u32_le_at(bytes, start + class_377::PARAMETER_BODY_RECORD)?
            != parameter_body_record
        || View::u32_le_at(bytes, start + class_377::BODY_ENTITY_SUFFIX)? != body_entity_suffix
        || !zeros_at::<10>(bytes, start + class_377::PARAMETER_BODY_REFERENCE_FIELD)
        || !zeros_at::<10>(bytes, start + class_377::BODY_ENTITY_REFERENCE_FIELD)
    {
        return None;
    }
    exact_body_based_on_faces_property(bytes, start, class_377::TAG_BODY_BASED_ON_FACES_MARKER)?;
    if bytes.get(start + class_377::PARAMETER_REFERENCE_GROUP_MARKER)
        != Some(&class_377::PARAMETER_REFERENCE_GROUP_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_377::PARAMETER_REFERENCE_GROUP_COUNT)?
            != class_377::PARAMETER_REFERENCE_GROUP_COUNT_VALUE
        || marked_record_reference(bytes, start + class_377::PARAMETER_REFERENCE_MARKER)?
            != parameter_body_record
        || !zeros_at::<7>(bytes, start + class_377::PARAMETER_REFERENCE_FIELD)
        || marked_record_reference(bytes, start + class_377::SCOPE_REFERENCE_MEMBER_MARKER)?
            != *member
        || !zeros_at::<6>(bytes, start + class_377::SCOPE_REFERENCE_MEMBER_FIELD)
        || bytes.get(start + class_377::AUXILIARY_GROUP_MARKER)
            != Some(&class_377::AUXILIARY_GROUP_MARKER_VALUE)
        || !zeros_at::<3>(bytes, start + class_377::AUXILIARY_GROUP_ZERO_RUN)
    {
        return None;
    }
    let auxiliary_record =
        marked_record_reference(bytes, start + class_377::AUXILIARY_REFERENCE_MARKER)?;
    if auxiliary_record == 0
        || !zeros_at::<14>(bytes, start + class_377::AUXILIARY_REFERENCE_FIELD)
        || View::u32_le_at(bytes, start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT)?
            != class_377::ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE
    {
        return None;
    }
    // The generic scope tail after the GUID.
    let previous_history_state_id =
        View::u32_le_at(bytes, start + class_377::PREVIOUS_HISTORY_STATE_ID)?;
    let previous_history_state_matches = match scope.previous_history_state_id() {
        Some(id) => u32::try_from(id).ok() == Some(previous_history_state_id),
        None => previous_history_state_id == u32::MAX,
    };
    if !zeros_at::<3>(bytes, start + class_377::ZERO_RUN_3)
        || View::u32_le_at(bytes, start + class_377::REFERENCE_COUNT)?
            != class_377::REFERENCE_COUNT_VALUE
        || marked_record_reference(bytes, start + class_377::GENERIC_SCOPE_REFERENCE_MARKER)?
            != *member
        || View::u32_le_at(bytes, start + class_377::HISTORY_STATE_ID)?
            != scope
                .history_state_id()
                .and_then(|id| u32::try_from(id).ok())?
        || !previous_history_state_matches
        || View::u32_le_at(bytes, start + class_377::KIND_LENGTH)? != class_377::KIND_LENGTH_VALUE
        || bytes_at::<24>(bytes, start + class_377::KIND) != Some(&BASE_FEATURE_KIND_UTF16)
        || View::u32_le_at(bytes, start + class_377::FEATURE_ORDINAL)?
            != scope.feature_ordinal.get()
    {
        return None;
    }
    Some(DirectFields {
        start,
        parameter_body_record,
        body_entity_suffix,
        auxiliary_record,
    })
}

fn exact_base_feature_direct_body_based_on_faces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let Some(fields) = direct_body_based_on_faces_fields(bytes, scope) else {
        return Ok(None);
    };
    let Some((envelope_guid, guid_end)) = fixed_relaxed_guid_text(
        ctx,
        bytes,
        fields.start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT,
    )?
    else {
        return Ok(None);
    };
    if guid_end != fields.start + class_377::ZERO_RUN_3 {
        return Ok(None);
    }
    let at = |offset: usize| scope.byte_offset() + u64_from_index(offset);
    Ok(Some(DesignBaseFeatureConstruction::BodyBasedOnFaces {
        body: Located {
            value: fields.body_entity_suffix,
            offset: at(class_377::BODY_ENTITY_SUFFIX),
        },
        parameter_body_record: fields.parameter_body_record,
        parameter_body_record_offset: at(class_377::PARAMETER_BODY_RECORD),
        auxiliary_record: fields.auxiliary_record,
        auxiliary_record_offset: at(class_377::AUXILIARY_RECORD),
        envelope_guid,
        envelope_guid_offset: at(class_377::ENVELOPE_GUID),
        tag_body_based_on_faces_offset: at(class_377::TAG_BODY_BASED_ON_FACES_VALUE),
    }))
}

pub(super) fn exact_base_feature_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    if !matches!(scope.payload(), scope::DesignScopePayload::BaseFeature(_)) {
        return Ok(None);
    }
    if let Some(snapshot) = exact_base_feature_body_snapshot(ctx, bytes, scope)? {
        return Ok(Some(snapshot));
    }
    if let Some(value) = exact_base_feature_legacy_body_based_on_faces(ctx, bytes, scope)? {
        return Ok(Some(value));
    }
    if let Some(value) = exact_base_feature_direct_body_based_on_faces(ctx, bytes, scope)? {
        return Ok(Some(value));
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let tags = (scope.class_tag.as_str(), scope.paired_class_tag.as_str());
    if scope.frame_length() == 267 {
        return exact_base_feature_267_zero_body(ctx, bytes, scope, start);
    }
    if tags == ("409", "262") && scope.frame_length() == 258 {
        return exact_base_feature_409_zero_body(ctx, bytes, scope, start);
    }
    if tags == ("444", "263") && scope.frame_length() == 258 {
        return exact_base_feature_444_zero_body(ctx, bytes, scope, start);
    }
    exact_base_feature_result_bodies(ctx, bytes, scope, start)
}

fn exact_base_feature_267_zero_body(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let (Some(metadata_record), Some(metadata_field)) = (
        View::u32_le_at(bytes, start + 37),
        bytes.get(start + 45..start + 51),
    ) else {
        return Ok(None);
    };
    Ok(Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
        metadata_record,
        metadata_record_offset: scope.byte_offset() + 37,
        metadata_field: ctx
            .copy_slice(metadata_field, "f3d BaseFeature 267-byte metadata field")?,
    }))
}

/// The metadata record, metadata field and zero padding of a class-409
/// zero-body frame.
fn legacy_409_zero_body_fields<'a>(
    bytes: &'a [u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<(u32, &'a [u8], &'a [u8])> {
    if scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset()) {
        return None;
    }
    let metadata_record = u32::try_from(View::u64_le_at(
        bytes,
        start + legacy_zero_body::SHARED_METADATA_RECORD,
    )?)
    .ok()?;
    if !zeros_at::<9>(bytes, start + legacy_zero_body::ZERO_RUN_9)
        || bytes.get(start + legacy_zero_body::ZERO_BODY_MARKER) != Some(&1)
        || !zeros_at::<11>(bytes, start + legacy_zero_body::ZERO_RUN_11)
        || bytes.get(start + legacy_zero_body::SHARED_METADATA_MARKER) != Some(&1)
        || scope.reference_members().values_array::<1>() != Some([&metadata_record])
    {
        return None;
    }
    // The zero padding runs from the fixed prefix to the GUID before the
    // scope kind.
    let uuid_offset = usize::try_from(scope.kind_offset())
        .ok()?
        .checked_sub(102)?;
    let padding = bytes.get(start + legacy_zero_body::ZERO_PADDING_8..uuid_offset)?;
    let metadata_field = bytes.get(
        start + legacy_zero_body::SHARED_METADATA_FIELD..start + legacy_zero_body::ZERO_PADDING_8,
    )?;
    Some((metadata_record, metadata_field, padding))
}

fn exact_base_feature_409_zero_body(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let Some((metadata_record, metadata_field, padding)) =
        legacy_409_zero_body_fields(bytes, scope, start)
    else {
        return Ok(None);
    };
    if !ctx.all_by(
        padding,
        |byte| Ok(*byte == 0),
        "validate F3D legacy BaseFeature zero padding",
    )? {
        return Ok(None);
    }
    Ok(Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
        metadata_record,
        metadata_record_offset: scope.byte_offset()
            + u64_from_index(legacy_zero_body::SHARED_METADATA_RECORD),
        metadata_field: ctx.copy_slice(metadata_field, "f3d BaseFeature 409/262 metadata field")?,
    }))
}

/// The metadata record of a class-444 zero-body frame whose fixed fields
/// around the GUID match.
fn legacy_444_zero_body_metadata(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<u32> {
    let at = |offset: usize| scope.byte_offset().checked_add(u64::try_from(offset).ok()?);
    if scope.byte_offset().checked_add(scope.frame_length()) != Some(scope.paired_byte_offset())
        || scope.reference_members().len() != 1
        || Some(scope.reference_count_offset()) != at(legacy_444_zero_body::REFERENCE_COUNT)
        || scope.reference_members().offsets().next().copied()
            != at(legacy_444_zero_body::SCOPE_REFERENCE_RECORD)
        || Some(scope.kind_offset()) != at(legacy_444_zero_body::KIND_LENGTH + 4)
    {
        return None;
    }
    let metadata_record = u32::try_from(View::u64_le_at(
        bytes,
        start + legacy_444_zero_body::SHARED_METADATA_RECORD,
    )?)
    .ok()?;
    if !zeros_at::<9>(bytes, start + legacy_444_zero_body::ZERO_RUN_9)
        || bytes.get(start + legacy_444_zero_body::ZERO_BODY_MARKER)
            != Some(&legacy_444_zero_body::ZERO_BODY_MARKER_VALUE)
        || !zeros_at::<11>(bytes, start + legacy_444_zero_body::ZERO_RUN_11)
        || bytes.get(start + legacy_444_zero_body::SHARED_METADATA_MARKER)
            != Some(&legacy_444_zero_body::SHARED_METADATA_MARKER_VALUE)
        || scope.reference_members().values_array::<1>() != Some([&metadata_record])
        || !zeros_at::<14>(
            bytes,
            start + legacy_444_zero_body::SHARED_METADATA_ZERO_TAIL,
        )
        || !zeros_at::<3>(bytes, start + legacy_444_zero_body::ZERO_RUN_3)
        || View::u32_le_at(bytes, start + legacy_444_zero_body::REFERENCE_COUNT)?
            != legacy_444_zero_body::REFERENCE_COUNT_VALUE
        || bytes.get(start + legacy_444_zero_body::SCOPE_REFERENCE_MARKER)
            != Some(&legacy_444_zero_body::SCOPE_REFERENCE_MARKER_VALUE)
        || View::u32_le_at(bytes, start + legacy_444_zero_body::SCOPE_REFERENCE_RECORD)?
            != metadata_record
        || !zeros_at::<
            {
                legacy_444_zero_body::HISTORY_STATE_ID - legacy_444_zero_body::SCOPE_REFERENCE_FIELD
            },
        >(bytes, start + legacy_444_zero_body::SCOPE_REFERENCE_FIELD)
        || View::u32_le_at(bytes, start + legacy_444_zero_body::KIND_LENGTH)?
            != legacy_444_zero_body::KIND_LENGTH_VALUE
    {
        return None;
    }
    Some(metadata_record)
}

fn exact_base_feature_444_zero_body(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let Some(metadata_record) = legacy_444_zero_body_metadata(bytes, scope, start) else {
        return Ok(None);
    };
    if fixed_guid_end(
        ctx,
        bytes,
        start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
    )? != Some(start + legacy_444_zero_body::ZERO_RUN_3)
    {
        return Ok(None);
    }
    let Some(metadata_field) = bytes.get(
        start + legacy_444_zero_body::SHARED_METADATA_ZERO_TAIL
            ..start + legacy_444_zero_body::GUID_CODE_UNIT_COUNT,
    ) else {
        return Ok(None);
    };
    Ok(Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithoutRepeatedFields(Vec::new()),
        metadata_record,
        metadata_record_offset: scope.byte_offset()
            + u64_from_index(legacy_444_zero_body::SHARED_METADATA_RECORD),
        metadata_field: ctx.copy_slice(metadata_field, "f3d BaseFeature 444/263 metadata tail")?,
    }))
}

/// The marked entry of `result_body_entry` layout at `at`: a 64-bit value
/// and a six-byte field.
fn base_feature_entry_at(bytes: &[u8], at: usize) -> Option<DesignBaseFeatureEntry<u64>> {
    if bytes.get(at) != Some(&1) {
        return None;
    }
    let value_at = at.checked_add(result_body_entry::REFERENCE_VALUE)?;
    Some(DesignBaseFeatureEntry {
        value: View::u64_le_at(bytes, value_at)?,
        offset: u64::try_from(value_at).ok()?,
        field: *bytes_at::<6>(bytes, at.checked_add(result_body_entry::REFERENCE_FIELD)?)?,
    })
}

/// Start offsets of the four parallel per-body runs of a result-body frame.
struct ResultBodyRuns {
    entities: usize,
    references: usize,
    repeated: usize,
    results: usize,
    /// The repeated entries name the body entity rather than the passive
    /// reference.
    repeated_names_entity: bool,
}

impl ResultBodyRuns {
    /// Body `ordinal`, read from its entry in each run, and the field of its
    /// repeated entry. `result_row` is its entry in the result run.
    fn body_at(
        &self,
        bytes: &[u8],
        ordinal: usize,
        result_row: &[u8; RESULT_ENTRY_LEN],
    ) -> Option<(DesignBaseFeatureResultBody, [u8; 6])> {
        let entity = base_feature_entry_at(
            bytes,
            self.entities
                .checked_add(ordinal.checked_mul(result_body_entry::LEN)?)?,
        )?;
        let reference = base_feature_entry_at(
            bytes,
            self.references
                .checked_add(ordinal.checked_mul(result_body_entry::LEN)?)?,
        )?;
        let reference = DesignBaseFeatureEntry {
            value: u32::try_from(reference.value).ok()?,
            offset: reference.offset,
            field: reference.field,
        };
        let repeated = self
            .repeated
            .checked_add(ordinal.checked_mul(compact_entry::LEN)?)?;
        let expected = if self.repeated_names_entity {
            u32::try_from(entity.value).ok()?
        } else {
            reference.value
        };
        if bytes.get(repeated + compact_entry::BODY_MARKER) != Some(&1)
            || View::u32_le_at(bytes, repeated + compact_entry::BODY_ENTITY_SUFFIX)? != expected
            || result_row[0] != 1
        {
            return None;
        }
        let repeated_field = *bytes_at::<6>(bytes, repeated + compact_entry::BODY_FIELD)?;
        let result_at = self
            .results
            .checked_add(ordinal.checked_mul(RESULT_ENTRY_LEN)?)?;
        let result = DesignBaseFeatureEntry {
            value: View::u32_le_at(result_row, 1)?,
            offset: u64::try_from(result_at.checked_add(1)?).ok()?,
            field: *bytes_at::<6>(result_row, 5)?,
        };
        Some((
            DesignBaseFeatureResultBody {
                entity,
                reference,
                result,
            },
            repeated_field,
        ))
    }
}

/// The fixed layout of a counted result-body frame: its runs, metadata and
/// the zero padding before the scope kind.
struct ResultBodyLayout<'a> {
    runs: ResultBodyRuns,
    result_rows: &'a [[u8; RESULT_ENTRY_LEN]],
    metadata_record: u32,
    metadata_record_offset: u64,
    metadata_field: &'a [u8],
    padding: &'a [u8],
}

fn result_body_layout<'a>(
    bytes: &'a [u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<ResultBodyLayout<'a>> {
    if !zeros_at::<8>(bytes, start + result_body::ZERO_RUN_8)
        || bytes.get(start + result_body::BODY_COUNT_MARKER) != Some(&1)
    {
        return None;
    }
    let combined_count = usize::try_from(View::u32_le_at(
        bytes,
        start + result_body::COMBINED_BODY_REFERENCE_COUNT,
    )?)
    .ok()?;
    if combined_count == 0 || combined_count > 200_000 || !combined_count.is_multiple_of(2) {
        return None;
    }
    let body_count = combined_count / 2;
    let tags = (scope.class_tag.as_str(), scope.paired_class_tag.as_str());
    let legacy_290_261 = tags == ("290", "261");
    let legacy_444_263 = tags == ("444", "263");
    let expanded = matches!(
        tags,
        ("290", "261") | ("360", "258") | ("384", "264") | ("409", "262")
    );
    let compact = matches!(tags, ("420", "258") | ("452", "266"));
    let base_length = if legacy_290_261 {
        261
    } else if expanded || compact || legacy_444_263 {
        262
    } else {
        271
    };
    if scope.frame_length() != base_length + u64::try_from(body_count.checked_mul(52)?).ok()? {
        return None;
    }
    let entry_run_length = body_count.checked_mul(result_body_entry::LEN)?;
    let entities = start + result_body::LEN;
    let references = entities.checked_add(entry_run_length)?;
    let mut cursor = references.checked_add(entry_run_length)?;
    if expanded {
        if bytes.get(cursor) != Some(&1)
            || !zeros_at::<6>(bytes, cursor + 1)
            || usize::try_from(View::u32_le_at(bytes, cursor + 7)?).ok()? != body_count
        {
            return None;
        }
        cursor += 11;
    } else if compact || legacy_444_263 {
        // The class-444 form repeats with marker 0, the compact forms with 1.
        if bytes.get(cursor + compact_count::COUNT_MARKER) != Some(&1)
            || !zeros_at::<5>(bytes, cursor + compact_count::ZERO_RUN_5)
            || bytes.get(cursor + compact_count::REPEAT_MARKER) != Some(&u8::from(compact))
            || usize::try_from(View::u32_le_at(bytes, cursor + compact_count::BODY_COUNT)?).ok()?
                != body_count
        {
            return None;
        }
        cursor += compact_count::LEN;
    } else {
        if bytes.get(cursor) != Some(&1)
            || !zeros_at::<10>(bytes, cursor + 1)
            || usize::try_from(View::u32_le_at(bytes, cursor + 11)?).ok()? != body_count
        {
            return None;
        }
        cursor += 15;
    }
    let repeated = cursor;
    cursor = cursor.checked_add(body_count.checked_mul(compact_entry::LEN)?)?;
    if bytes.get(cursor) != Some(&0) || bytes.get(cursor + 1) != Some(&1) {
        return None;
    }
    cursor += 1;
    let metadata_record = u32::try_from(View::u64_le_at(bytes, cursor + 1)?).ok()?;
    let metadata_record_offset = u64::try_from(cursor + 1).ok()?;
    let metadata_field_width = if expanded || compact || legacy_444_263 {
        2
    } else {
        6
    };
    let metadata_field = bytes.get(cursor + 9..cursor + 9 + metadata_field_width)?;
    cursor += 9 + metadata_field_width;
    if usize::try_from(View::u32_le_at(bytes, cursor)?).ok()? != body_count {
        return None;
    }
    cursor += 4;
    let results = cursor;
    let result_rows = bytes
        .get(results..)?
        .as_chunks::<RESULT_ENTRY_LEN>()
        .0
        .get(..body_count)?;
    cursor = results.checked_add(body_count.checked_mul(RESULT_ENTRY_LEN)?)?;
    // The zero padding runs from the result run to the GUID before the scope
    // kind.
    let uuid_offset = usize::try_from(scope.kind_offset())
        .ok()?
        .checked_sub(102)?;
    let padding = bytes.get(cursor..uuid_offset)?;
    Some(ResultBodyLayout {
        runs: ResultBodyRuns {
            entities,
            references,
            repeated,
            results,
            repeated_names_entity: compact,
        },
        result_rows,
        metadata_record,
        metadata_record_offset,
        metadata_field,
        padding,
    })
}

fn exact_base_feature_result_bodies(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    let Some(layout) = result_body_layout(bytes, scope, start) else {
        return Ok(None);
    };
    if !ctx.all_by(
        layout.padding,
        |byte| Ok(*byte == 0),
        "validate F3D BaseFeature result padding",
    )? {
        return Ok(None);
    }
    let Some((first_row, later_rows)) = layout.result_rows.split_first() else {
        return Ok(None);
    };
    let Some(first) = layout.runs.body_at(bytes, 0, first_row) else {
        return Ok(None);
    };
    let metadata_field = ctx.copy_slice(
        layout.metadata_field,
        "f3d BaseFeature result-body metadata field",
    )?;
    let mut rest =
        ctx.vector_storage(later_rows.len(), "f3d BaseFeature remaining result bodies")?;
    // Each later body is read and kept in run order; the scan stops at the
    // first malformed body.
    let malformed = ctx.position_by(
        later_rows,
        |result_row| {
            let ordinal = rest.len() + 1;
            let Some(body) = layout.runs.body_at(bytes, ordinal, result_row) else {
                return Ok(true);
            };
            ctx.push_vec(&mut rest, body, "f3d BaseFeature remaining result bodies")?;
            Ok(false)
        },
        "scan F3D BaseFeature result bodies",
    )?;
    if malformed.is_some() {
        return Ok(None);
    }
    Ok(Some(DesignBaseFeatureConstruction::ResultBodies {
        bodies: DesignBaseFeatureResults::WithRepeatedFields { first, rest },
        metadata_record: layout.metadata_record,
        metadata_record_offset: layout.metadata_record_offset,
        metadata_field,
    }))
}

/// The body rows of a class-314/class-259 body snapshot frame at `start`.
/// The scope kind is "Base Feature", so its UTF-16 payload has fixed width.
fn snapshot_body_rows<'a>(
    bytes: &'a [u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<&'a [[u8; snapshot_entry::LEN]]> {
    // Fixed prefix, linkage and GUID blocks, generic scope prefix, kind
    // prefix, ordinal, and closing tail; the kind payload adds 2L bytes.
    const FIXED_FRAME_LENGTH: u64 = 431;
    let body_count = usize::try_from(View::u32_le_at(bytes, start + snapshot::BODY_COUNT)?).ok()?;
    let expected_frame_length = FIXED_FRAME_LENGTH
        .checked_add(u64::try_from(body_count.checked_mul(snapshot_entry::LEN)?).ok()?)?
        .checked_add(u64_from_index(BASE_FEATURE_KIND_UTF16.len()))?;
    if !(1..=200_000).contains(&body_count)
        || scope.frame_length() != expected_frame_length
        || !zeros_at::<8>(bytes, start + snapshot::ZERO_RUN_8)
        || bytes.get(start + snapshot::BODY_COUNT_MARKER) != Some(&1)
    {
        return None;
    }
    bytes
        .get(start + snapshot::LEN..)?
        .as_chunks::<{ snapshot_entry::LEN }>()
        .0
        .get(..body_count)
}

/// The snapshot body of the marked row `row` that starts at `at`.
fn snapshot_body(
    row: &[u8; snapshot_entry::LEN],
    at: usize,
) -> Option<DesignBaseFeatureEntry<u64>> {
    if row[0] != 1 {
        return None;
    }
    Some(DesignBaseFeatureEntry {
        value: View::u64_le_at(row, snapshot_entry::BODY_ENTITY_SUFFIX)?,
        offset: u64::try_from(at.checked_add(snapshot_entry::BODY_ENTITY_SUFFIX)?).ok()?,
        field: *bytes_at::<6>(row, snapshot_entry::BODY_ENTITY_FIELD)?,
    })
}

/// The linkage and auxiliary records of the snapshot linkage tail at
/// `after_guids`. The tail repeats the first body and names the scope's
/// only reference.
fn snapshot_linkage_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    after_guids: usize,
    first_body: u64,
) -> Option<(u32, u32)> {
    let [member] = scope.reference_members().values_array::<1>()?;
    if bytes_at::<{ snapshot_tail::FIRST_BODY_MARKER }>(bytes, after_guids)
        != Some(&[0, 0, 1, 1, 0, 0, 0])
        || bytes.get(after_guids + snapshot_tail::FIRST_BODY_MARKER) != Some(&1)
        || View::u64_le_at(bytes, after_guids + snapshot_tail::FIRST_BODY_ENTITY_SUFFIX)?
            != first_body
        || !zeros_at::<3>(bytes, after_guids + snapshot_tail::ZERO_RUN_3)
        || bytes.get(after_guids + snapshot_tail::LINKAGE_MARKER) != Some(&1)
    {
        return None;
    }
    let linkage_record = u32::try_from(View::u64_le_at(
        bytes,
        after_guids + snapshot_tail::LINKAGE_RECORD,
    )?)
    .ok()?;
    if linkage_record != *member
        || !zeros_at::<6>(bytes, after_guids + snapshot_tail::ZERO_RUN_6)
        || View::u32_le_at(bytes, after_guids + snapshot_tail::RELATION_COUNT)? != 1
        || bytes.get(after_guids + snapshot_tail::AUXILIARY_MARKER) != Some(&1)
    {
        return None;
    }
    let auxiliary_record = u32::try_from(View::u64_le_at(
        bytes,
        after_guids + snapshot_tail::AUXILIARY_RECORD,
    )?)
    .ok()?;
    if !zeros_at::<6>(bytes, after_guids + snapshot_tail::TRAILING_ZERO_RUN_6)
        || !zeros_at::<4>(bytes, after_guids + snapshot_tail::TRAILING_ZERO_RUN_4)
    {
        return None;
    }
    Some((linkage_record, auxiliary_record))
}

/// The generic scope tail after the third snapshot GUID: one reference, the
/// history state, the "Base Feature" kind and the feature ordinal, each where
/// the scope parse found it, and no previous history state.
fn snapshot_scope_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    after_third_guid: usize,
) -> Option<()> {
    let [member] = scope.reference_members().values_array::<1>()?;
    let reference_count_at = after_third_guid.checked_add(snapshot_scope::REFERENCE_COUNT)?;
    let reference_marker = after_third_guid.checked_add(snapshot_scope::REFERENCE_MARKER)?;
    let state_at = after_third_guid.checked_add(snapshot_scope::HISTORY_STATE_ID)?;
    let kind_at = after_third_guid.checked_add(snapshot_scope::KIND_CODE_UNIT_COUNT)?;
    if !zeros_at::<3>(bytes, after_third_guid)
        || View::u32_le_at(bytes, reference_count_at)? != 1
        || bytes.get(reference_marker) != Some(&1)
        || View::u32_le_at(bytes, reference_marker + 1)? != *member
        || !zeros_at::<6>(bytes, reference_marker + 5)
        || scope.reference_count_offset() != u64::try_from(reference_count_at).ok()?
        || scope.reference_members().offsets().next().copied()
            != Some(u64::try_from(reference_marker + 1).ok()?)
        || scope.kind_offset() != u64::try_from(kind_at + 4).ok()?
    {
        return None;
    }
    let state = View::u32_le_at(bytes, state_at)?;
    let state_matches = match scope.history_state_id() {
        Some(history_state_id) => u32::try_from(history_state_id).ok() == Some(state),
        None => state == u32::MAX,
    };
    let kind_end = kind_at
        .checked_add(4)?
        .checked_add(BASE_FEATURE_KIND_UTF16.len())?;
    if !state_matches
        || View::u32_le_at(bytes, kind_at)? != BASE_FEATURE_KIND_UNITS
        || bytes_at::<24>(bytes, kind_at + 4) != Some(&BASE_FEATURE_KIND_UTF16)
        || View::u32_le_at(bytes, kind_end)? != scope.feature_ordinal.get()
        || scope.feature_ordinal_offset() != u64::try_from(kind_end).ok()?
        || scope.previous_history_state_id().is_some()
        || scope.previous_history_state_id_offset().is_some()
        || scope.paired_byte_offset()
            != u64::try_from(start.checked_add(usize::try_from(scope.frame_length()).ok()?)?)
                .ok()?
    {
        return None;
    }
    Some(())
}

/// A class-314/class-259 body snapshot. The caller has matched the scope
/// payload, so the scope kind is "Base Feature".
fn exact_base_feature_body_snapshot(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignBaseFeatureConstruction>, CodecError> {
    if scope.class_tag.as_str() != "314"
        || scope.paired_class_tag.as_str() != "259"
        || scope.reference_members().len() != 1
    {
        return Ok(None);
    }
    let Ok(start) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    let Some(body_rows) = snapshot_body_rows(bytes, scope, start) else {
        return Ok(None);
    };
    let body_run = start + snapshot::LEN;
    let mut bodies = ctx.vector_storage(body_rows.len(), "f3d BaseFeature snapshot bodies")?;
    // Each body is read and kept in run order; the scan stops at the first
    // unmarked body.
    let malformed = ctx.position_by(
        body_rows,
        |row| {
            let at = body_run + bodies.len() * snapshot_entry::LEN;
            let Some(body) = snapshot_body(row, at) else {
                return Ok(true);
            };
            ctx.push_vec(&mut bodies, body, "f3d BaseFeature snapshot bodies")?;
            Ok(false)
        },
        "scan F3D BaseFeature snapshot bodies",
    )?;
    if malformed.is_some() {
        return Ok(None);
    }
    let Some(first_body) = bodies.first().map(|body| body.value) else {
        return Ok(None);
    };
    let mut cursor = body_run + body_rows.len() * snapshot_entry::LEN;
    let Some(preamble) = bytes_at::<{ snapshot_expanded_preamble::LEN }>(bytes, cursor) else {
        return Ok(None);
    };
    let packed_guid_preamble = if preamble == &[1, 0, 0, 0, 0, 1, 0, 0, 0] {
        true
    } else if preamble.first_chunk::<{ snapshot_compact_preamble::LEN }>()
        == Some(&[1, 0, 0, 0, 1, 0, 0, 0])
    {
        false
    } else {
        return Ok(None);
    };
    cursor += if packed_guid_preamble {
        snapshot_expanded_preamble::LEN
    } else {
        snapshot_compact_preamble::LEN
    };
    let first_guid_offset = cursor + snapshot_guid::GUID_UTF16;
    let Some((first_guid, after_first_guid)) = fixed_relaxed_guid_text(ctx, bytes, cursor)? else {
        return Ok(None);
    };
    let second_guid_offset = after_first_guid + snapshot_guid::GUID_UTF16;
    let Some((second_guid, after_second_guid)) =
        fixed_relaxed_guid_text(ctx, bytes, after_first_guid)?
    else {
        return Ok(None);
    };
    // The nine-byte preamble carries the linkage anchor in the final zero
    // byte of the second GUID's UTF-16 payload. Keep the full GUID for the
    // native record, but anchor the fixed tail at that shared byte.
    let after_guids = after_second_guid - usize::from(packed_guid_preamble);
    let Some((linkage_record, auxiliary_record)) =
        snapshot_linkage_tail(bytes, scope, after_guids, first_body)
    else {
        return Ok(None);
    };
    let third_guid_at = after_guids + snapshot_tail::LEN;
    let third_guid_offset = third_guid_at + snapshot_guid::GUID_UTF16;
    let Some((third_guid, after_third_guid)) = fixed_relaxed_guid_text(ctx, bytes, third_guid_at)?
    else {
        return Ok(None);
    };
    if snapshot_scope_tail(bytes, scope, start, after_third_guid).is_none() {
        return Ok(None);
    }
    Ok(Some(DesignBaseFeatureConstruction::BodySnapshot {
        bodies,
        related_guids: [first_guid, second_guid, third_guid],
        related_guid_offsets: [
            u64_from_index(first_guid_offset),
            u64_from_index(second_guid_offset),
            u64_from_index(third_guid_offset),
        ],
        linkage_record,
        linkage_record_offset: u64_from_index(after_guids + snapshot_tail::LINKAGE_RECORD),
        auxiliary_record,
        auxiliary_record_offset: u64_from_index(after_guids + snapshot_tail::AUXILIARY_RECORD),
    }))
}

#[cfg(test)]
mod tests;
