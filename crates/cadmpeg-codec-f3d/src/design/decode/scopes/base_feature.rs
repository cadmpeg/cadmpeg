use crate::bytes::{is_guid_relaxed, lp_utf16_bounded};
use crate::layout::base_feature_class_377_prefix as class_377;
use crate::layout::base_feature_class_452_262_compact as class_452_compact;
use crate::layout::base_feature_class_452_262_expanded as class_452_expanded;
use crate::records::{
    DesignBaseFeatureBodyReferenceForm, DesignBaseFeatureConstruction, DesignBaseFeatureEntry,
    DesignLegacyBaseFeatureBody, DesignParameterScope, Located,
};
use cadmpeg_core::decode::View;

use super::marked_record_reference;

pub(super) fn exact_base_feature_body_based_on_faces(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignBaseFeatureConstruction> {
    exact_base_feature_legacy_body_based_on_faces(bytes, scope)
        .or_else(|| exact_base_feature_direct_body_based_on_faces(bytes, scope))
}

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

fn exact_base_feature_scope_tail(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
    layout: BaseFeatureScopeTailLayout,
) -> Option<()> {
    if scope.reference_members.len() != 1
        || scope.byte_offset.checked_add(scope.frame_length) != Some(scope.paired_byte_offset)
        || scope.frame_length != u64::try_from(layout.frame_length).ok()?
        || scope.reference_count_offset
            != scope.byte_offset + u64::try_from(layout.reference_count).ok()?
        || !scope.reference_members.offsets().copied().eq([scope.byte_offset + u64::try_from(layout.generic_scope_reference_record).ok()?])
        || scope.history_state_id_offset
            != scope.byte_offset + u64::try_from(layout.history_state_id).ok()?
        || scope.kind_offset != scope.byte_offset + u64::try_from(layout.kind).ok()?
        || scope.feature_ordinal_offset
            != scope.byte_offset + u64::try_from(layout.feature_ordinal).ok()?
        || scope.previous_history_state_id_offset
            != Some(scope.byte_offset + u64::try_from(layout.previous_history_state_id).ok()?)
        || View::u32_le_at(bytes, start + layout.reference_count)?
            != class_377::REFERENCE_COUNT_VALUE
        || bytes.get(start + layout.generic_scope_reference_marker)
            != Some(&class_377::GENERIC_SCOPE_REFERENCE_MARKER_VALUE)
        || marked_record_reference(bytes, start + layout.generic_scope_reference_marker)
            != Some(*scope.reference_members.values().next()?)
        || bytes
            .get(start + layout.generic_scope_reference_field..start + layout.history_state_id)?
            != [0; 6]
        || View::u32_le_at(bytes, start + layout.history_state_id)?
            != scope
                .history_state_id
                .and_then(|id| u32::try_from(id).ok())?
        || View::u32_le_at(bytes, start + layout.kind_length)? != class_377::KIND_LENGTH_VALUE
    {
        return None;
    }
    let kind_code_units = usize::try_from(class_377::KIND_LENGTH_VALUE).ok()?;
    let (kind_text, kind_end) = lp_utf16_bounded(
        bytes,
        start + layout.kind_length,
        kind_code_units..=kind_code_units,
    )?;
    if kind_text != "Base Feature"
        || kind_end != start + layout.feature_ordinal
        || View::u32_le_at(bytes, start + layout.feature_ordinal)? != scope.feature_ordinal.get()
    {
        return None;
    }
    let previous_state = View::u32_le_at(bytes, start + layout.previous_history_state_id)?;
    let previous_matches = match scope.previous_history_state_id {
        Some(id) => u32::try_from(id).ok() == Some(previous_state),
        None => previous_state == u32::MAX,
    };
    previous_matches.then_some(())
}

fn exact_base_feature_legacy_body_based_on_faces(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignBaseFeatureConstruction> {
    if scope.class_tag != "452" || scope.paired_class_tag != "262" {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    let body_count = View::u32_le_at(bytes, start + class_452_compact::BODY_COUNT)?;
    match body_count {
        1 => exact_base_feature_legacy_compact(bytes, scope, start),
        2 => exact_base_feature_legacy_expanded(bytes, scope, start),
        _ => None,
    }
}

fn exact_base_feature_legacy_compact(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<DesignBaseFeatureConstruction> {
    if scope.frame_length != u64::try_from(class_452_compact::LEN).ok()? {
        return None;
    }
    if bytes
        .get(start + class_452_compact::ZERO_RUN_8..start + class_452_compact::BODY_COUNT_MARKER)?
        != [0; 8]
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
    let body_entity_field = bytes
        .get(
            start + class_452_compact::BODY_ENTITY_REFERENCE_FIELD
                ..start + class_452_compact::TAG_BODY_BASED_ON_FACES_MARKER,
        )?
        .try_into()
        .ok()?;
    exact_body_based_on_faces_property(
        bytes,
        start,
        class_452_compact::TAG_BODY_BASED_ON_FACES_MARKER,
    )?;
    let mode = *bytes.get(start + class_452_compact::MODE)?;
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
    if mode > 1
        || bytes.get(start + class_452_compact::PARAMETER_BODY_COUNT)
            != Some(&class_452_compact::PARAMETER_BODY_COUNT_VALUE)
        || bytes.get(
            start + class_452_compact::PARAMETER_BODY_ZERO_RUN
                ..start + class_452_compact::PARAMETER_BODY_REFERENCE_MARKER,
        )? != [0; 3]
        || parameter_body_record == 0
        || bytes.get(
            start + class_452_compact::PARAMETER_BODY_REFERENCE_FIELD
                ..start + class_452_compact::SCOPE_REFERENCE_MARKER,
        )? != [0; 3]
        || scope_reference == 0
        || bytes.get(
            start + class_452_compact::SCOPE_REFERENCE_FIELD
                ..start + class_452_compact::AUXILIARY_GROUP_MARKER,
        )? != [0; 2]
        || bytes.get(start + class_452_compact::AUXILIARY_GROUP_MARKER)
            != Some(&class_452_compact::AUXILIARY_GROUP_MARKER_VALUE)
        || bytes.get(
            start + class_452_compact::AUXILIARY_GROUP_ZERO_RUN
                ..start + class_452_compact::AUXILIARY_REFERENCE_MARKER,
        )? != [0; 3]
        || auxiliary_record == 0
        || bytes.get(
            start + class_452_compact::AUXILIARY_REFERENCE_FIELD
                ..start + class_452_compact::ENVELOPE_GUID_CODE_UNIT_COUNT,
        )? != [0; 10]
        || !scope.reference_members.values().copied().eq([u32::try_from(scope_reference).ok()?])
    {
        return None;
    }
    let guid_code_units =
        usize::try_from(class_452_compact::ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE).ok()?;
    let (envelope_guid, guid_end) = lp_utf16_bounded(
        bytes,
        start + class_452_compact::ENVELOPE_GUID_CODE_UNIT_COUNT,
        guid_code_units..=guid_code_units,
    )?;
    if !is_guid_relaxed(&envelope_guid)
        || guid_end != start + class_452_compact::ZERO_RUN_AFTER_GUID
        || bytes.get(
            start + class_452_compact::ZERO_RUN_AFTER_GUID
                ..start + class_452_compact::REFERENCE_COUNT,
        )? != [0; 3]
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
    Some(DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
        form: DesignBaseFeatureBodyReferenceForm::CompactOneBody {
            mode: Located { value: mode, offset: scope.byte_offset + u64::try_from(class_452_compact::MODE).ok()? },
            body: DesignLegacyBaseFeatureBody {
                entity: DesignBaseFeatureEntry { value: u32::try_from(body_entity_suffix).ok()?, offset: scope.byte_offset + u64::try_from(class_452_compact::BODY_ENTITY_SUFFIX).ok()?, field: body_entity_field },
                parameter_body: Located { value: parameter_body_record, offset: scope.byte_offset + u64::try_from(class_452_compact::PARAMETER_BODY_RECORD).ok()? },
                auxiliary: Located { value: auxiliary_record, offset: scope.byte_offset + u64::try_from(class_452_compact::AUXILIARY_RECORD).ok()? },
            },
        },
        scope_reference,
        scope_reference_offset: scope.byte_offset
            + u64::try_from(class_452_compact::SCOPE_REFERENCE).ok()?,
        envelope_guid,
        envelope_guid_offset: scope.byte_offset
            + u64::try_from(class_452_compact::ENVELOPE_GUID).ok()?,
        tag_body_based_on_faces_offset: scope.byte_offset
            + u64::try_from(class_452_compact::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
    })
}

fn exact_base_feature_legacy_expanded(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<DesignBaseFeatureConstruction> {
    if scope.frame_length != u64::try_from(class_452_expanded::LEN).ok()? {
        return None;
    }
    if bytes.get(
        start + class_452_expanded::ZERO_RUN_8..start + class_452_expanded::BODY_COUNT_MARKER,
    )? != [0; 8]
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
        bytes
            .get(
                start + class_452_expanded::BODY_ENTITY_ONE_FIELD
                    ..start + class_452_expanded::BODY_ENTITY_TWO_MARKER,
            )?
            .try_into()
            .ok()?,
        bytes
            .get(
                start + class_452_expanded::BODY_ENTITY_TWO_FIELD
                    ..start + class_452_expanded::TAG_BODY_BASED_ON_FACES_MARKER,
            )?
            .try_into()
            .ok()?,
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
        || bytes.get(
            start + class_452_expanded::PARAMETER_BODY_ONE_FIELD
                ..start + class_452_expanded::PARAMETER_BODY_TWO_MARKER,
        )? != [0; 6]
        || bytes.get(
            start + class_452_expanded::PARAMETER_BODY_TWO_FIELD
                ..start + class_452_expanded::PARAMETER_BODY_SEPARATOR,
        )? != [0; 6]
        || bytes.get(start + class_452_expanded::PARAMETER_BODY_SEPARATOR)
            != Some(&class_452_expanded::PARAMETER_BODY_SEPARATOR_VALUE)
        || scope_reference != scope.reference_members.values().next().copied()?
        || bytes.get(
            start + class_452_expanded::SCOPE_REFERENCE_FIELD
                ..start + class_452_expanded::AUXILIARY_BODY_COUNT,
        )? != [0; 6]
        || View::u32_le_at(bytes, start + class_452_expanded::AUXILIARY_BODY_COUNT)?
            != class_452_expanded::AUXILIARY_BODY_COUNT_VALUE
        || bytes.get(
            start + class_452_expanded::AUXILIARY_BODY_ONE_FIELD
                ..start + class_452_expanded::AUXILIARY_BODY_TWO_MARKER,
        )? != [0; 6]
        || bytes.get(
            start + class_452_expanded::AUXILIARY_BODY_TWO_FIELD
                ..start + class_452_expanded::AUXILIARY_BODY_ZERO_RUN,
        )? != [0; 6]
        || bytes.get(
            start + class_452_expanded::AUXILIARY_BODY_ZERO_RUN
                ..start + class_452_expanded::ENVELOPE_GUID_CODE_UNIT_COUNT,
        )? != [0; 8]
    {
        return None;
    }
    let guid_code_units =
        usize::try_from(class_452_expanded::ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE).ok()?;
    let (envelope_guid, guid_end) = lp_utf16_bounded(
        bytes,
        start + class_452_expanded::ENVELOPE_GUID_CODE_UNIT_COUNT,
        guid_code_units..=guid_code_units,
    )?;
    if !is_guid_relaxed(&envelope_guid)
        || guid_end != start + class_452_expanded::ZERO_RUN_AFTER_GUID
        || bytes.get(
            start + class_452_expanded::ZERO_RUN_AFTER_GUID
                ..start + class_452_expanded::REFERENCE_COUNT,
        )? != [0; 3]
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
    Some(DesignBaseFeatureConstruction::LegacyBodyBasedOnFaces {
        form: DesignBaseFeatureBodyReferenceForm::ExpandedTwoBody { bodies: [DesignLegacyBaseFeatureBody {
                entity: DesignBaseFeatureEntry { value: u32::try_from(body_entity_suffixes[0]).ok()?, offset: scope.byte_offset + u64::try_from(class_452_expanded::BODY_ENTITY_ONE_SUFFIX).ok()?, field: body_entity_fields[0] },
                parameter_body: Located { value: u64::from(parameter_body_records[0]), offset: scope.byte_offset + u64::try_from(class_452_expanded::PARAMETER_BODY_ONE_RECORD).ok()? },
                auxiliary: Located { value: u64::from(auxiliary_records[0]), offset: scope.byte_offset + u64::try_from(class_452_expanded::AUXILIARY_BODY_ONE_RECORD).ok()? },
            },
DesignLegacyBaseFeatureBody {
                entity: DesignBaseFeatureEntry { value: u32::try_from(body_entity_suffixes[1]).ok()?, offset: scope.byte_offset + u64::try_from(class_452_expanded::BODY_ENTITY_TWO_SUFFIX).ok()?, field: body_entity_fields[1] },
                parameter_body: Located { value: u64::from(parameter_body_records[1]), offset: scope.byte_offset + u64::try_from(class_452_expanded::PARAMETER_BODY_TWO_RECORD).ok()? },
                auxiliary: Located { value: u64::from(auxiliary_records[1]), offset: scope.byte_offset + u64::try_from(class_452_expanded::AUXILIARY_BODY_TWO_RECORD).ok()? },
            }] },
        scope_reference: u64::from(scope_reference),
        scope_reference_offset: scope.byte_offset
            + u64::try_from(class_452_expanded::SCOPE_REFERENCE).ok()?,
        envelope_guid,
        envelope_guid_offset: scope.byte_offset
            + u64::try_from(class_452_expanded::ENVELOPE_GUID).ok()?,
        tag_body_based_on_faces_offset: scope.byte_offset
            + u64::try_from(class_452_expanded::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
    })
}

fn exact_base_feature_direct_body_based_on_faces(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignBaseFeatureConstruction> {
    if !matches!(
        (scope.class_tag.as_str(), scope.paired_class_tag.as_str()),
        ("365", "262") | ("377", "259")
    ) || scope.frame_length != u64::try_from(class_377::LEN).ok()?
        || scope.byte_offset.checked_add(scope.frame_length) != Some(scope.paired_byte_offset)
        || scope.reference_members.len() != 1
        || scope.reference_count_offset
            != scope.byte_offset + u64::try_from(class_377::REFERENCE_COUNT).ok()?
        || !scope.reference_members.offsets().copied().eq([scope.byte_offset
                + u64::try_from(class_377::GENERIC_SCOPE_REFERENCE_RECORD).ok()?])
        || scope.history_state_id_offset
            != scope.byte_offset + u64::try_from(class_377::HISTORY_STATE_ID).ok()?
        || scope.kind_offset
            != scope.byte_offset + u64::try_from(class_377::KIND_LENGTH + 4).ok()?
        || scope.feature_ordinal_offset
            != scope.byte_offset + u64::try_from(class_377::FEATURE_ORDINAL).ok()?
        || scope.previous_history_state_id_offset
            != Some(scope.byte_offset + u64::try_from(class_377::PREVIOUS_HISTORY_STATE_ID).ok()?)
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset).ok()?;
    if bytes.get(start + class_377::ZERO_RUN_8..start + class_377::BODY_REFERENCE_COUNT_MARKER)?
        != [0; 8]
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
        || bytes.get(
            start + class_377::PARAMETER_BODY_REFERENCE_FIELD
                ..start + class_377::BODY_ENTITY_REFERENCE_MARKER,
        )? != [0; 10]
        || bytes.get(
            start + class_377::BODY_ENTITY_REFERENCE_FIELD
                ..start + class_377::TAG_BODY_BASED_ON_FACES_MARKER,
        )? != [0; 10]
    {
        return None;
    }
    if bytes.get(start + class_377::TAG_BODY_BASED_ON_FACES_MARKER)
        != Some(&class_377::TAG_BODY_BASED_ON_FACES_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_377::TAG_BODY_BASED_ON_FACES_COUNT)?
            != class_377::TAG_BODY_BASED_ON_FACES_COUNT_VALUE
        || View::u32_le_at(bytes, start + class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH)?
            != class_377::TAG_BODY_BASED_ON_FACES_KEY_LENGTH_VALUE
        || bytes.get(
            start + class_377::TAG_BODY_BASED_ON_FACES_KEY
                ..start + class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH,
        )? != b"TagBodyBasedOnFaces"
        || View::u32_le_at(
            bytes,
            start + class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH,
        )? != class_377::TAG_BODY_BASED_ON_FACES_TYPE_LENGTH_VALUE
        || bytes.get(
            start + class_377::TAG_BODY_BASED_ON_FACES_TYPE
                ..start + class_377::TAG_BODY_BASED_ON_FACES_VALUE,
        )? != b"IntrinsicMetaTypebool"
        || View::u16_le_at(bytes, start + class_377::TAG_BODY_BASED_ON_FACES_VALUE)?
            != class_377::TAG_BODY_BASED_ON_FACES_VALUE_VALUE
    {
        return None;
    }
    if bytes.get(start + class_377::PARAMETER_REFERENCE_GROUP_MARKER)
        != Some(&class_377::PARAMETER_REFERENCE_GROUP_MARKER_VALUE)
        || View::u32_le_at(bytes, start + class_377::PARAMETER_REFERENCE_GROUP_COUNT)?
            != class_377::PARAMETER_REFERENCE_GROUP_COUNT_VALUE
        || marked_record_reference(bytes, start + class_377::PARAMETER_REFERENCE_MARKER)?
            != parameter_body_record
        || bytes.get(
            start + class_377::PARAMETER_REFERENCE_FIELD
                ..start + class_377::SCOPE_REFERENCE_MEMBER_MARKER,
        )? != [0; 7]
        || marked_record_reference(bytes, start + class_377::SCOPE_REFERENCE_MEMBER_MARKER)?
            != *scope.reference_members.values().next()?
        || bytes.get(
            start + class_377::SCOPE_REFERENCE_MEMBER_FIELD
                ..start + class_377::AUXILIARY_GROUP_MARKER,
        )? != [0; 6]
        || bytes.get(start + class_377::AUXILIARY_GROUP_MARKER)
            != Some(&class_377::AUXILIARY_GROUP_MARKER_VALUE)
        || bytes.get(
            start + class_377::AUXILIARY_GROUP_ZERO_RUN
                ..start + class_377::AUXILIARY_REFERENCE_MARKER,
        )? != [0; 3]
    {
        return None;
    }
    let auxiliary_record =
        marked_record_reference(bytes, start + class_377::AUXILIARY_REFERENCE_MARKER)?;
    if auxiliary_record == 0
        || bytes.get(
            start + class_377::AUXILIARY_REFERENCE_FIELD
                ..start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT,
        )? != [0; 14]
        || View::u32_le_at(bytes, start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT)?
            != class_377::ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE
    {
        return None;
    }
    let guid_code_units = usize::try_from(class_377::ENVELOPE_GUID_CODE_UNIT_COUNT_VALUE).ok()?;
    let (envelope_guid, guid_end) = lp_utf16_bounded(
        bytes,
        start + class_377::ENVELOPE_GUID_CODE_UNIT_COUNT,
        guid_code_units..=guid_code_units,
    )?;
    let previous_history_state_id =
        View::u32_le_at(bytes, start + class_377::PREVIOUS_HISTORY_STATE_ID)?;
    let previous_history_state_matches = match scope.previous_history_state_id {
        Some(id) => u32::try_from(id).ok() == Some(previous_history_state_id),
        None => previous_history_state_id == u32::MAX,
    };
    if !is_guid_relaxed(&envelope_guid)
        || guid_end != start + class_377::ZERO_RUN_3
        || bytes.get(start + class_377::ZERO_RUN_3..start + class_377::REFERENCE_COUNT)? != [0; 3]
        || View::u32_le_at(bytes, start + class_377::REFERENCE_COUNT)?
            != class_377::REFERENCE_COUNT_VALUE
        || marked_record_reference(bytes, start + class_377::GENERIC_SCOPE_REFERENCE_MARKER)?
            != *scope.reference_members.values().next()?
        || View::u32_le_at(bytes, start + class_377::HISTORY_STATE_ID)?
            != scope
                .history_state_id
                .and_then(|id| u32::try_from(id).ok())?
        || !previous_history_state_matches
        || View::u32_le_at(bytes, start + class_377::KIND_LENGTH)? != class_377::KIND_LENGTH_VALUE
    {
        return None;
    }
    let kind_code_units = usize::try_from(class_377::KIND_LENGTH_VALUE).ok()?;
    let (kind, kind_end) = lp_utf16_bounded(
        bytes,
        start + class_377::KIND_LENGTH,
        kind_code_units..=kind_code_units,
    )?;
    if kind != "Base Feature"
        || kind_end != start + class_377::FEATURE_ORDINAL
        || View::u32_le_at(bytes, start + class_377::FEATURE_ORDINAL)? != scope.feature_ordinal.get()
    {
        return None;
    }
    Some(DesignBaseFeatureConstruction::BodyBasedOnFaces {
        body: crate::records::Located {
            value: body_entity_suffix,
            offset: scope.byte_offset + u64::try_from(class_377::BODY_ENTITY_SUFFIX).ok()?,
        },
        parameter_body_record,
        parameter_body_record_offset: scope.byte_offset
            + u64::try_from(class_377::PARAMETER_BODY_RECORD).ok()?,
        auxiliary_record,
        auxiliary_record_offset: scope.byte_offset
            + u64::try_from(class_377::AUXILIARY_RECORD).ok()?,
        envelope_guid,
        envelope_guid_offset: scope.byte_offset + u64::try_from(class_377::ENVELOPE_GUID).ok()?,
        tag_body_based_on_faces_offset: scope.byte_offset
            + u64::try_from(class_377::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
    })
}
