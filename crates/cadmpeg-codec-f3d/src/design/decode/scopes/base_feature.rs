use crate::bytes::{is_guid_relaxed, lp_utf16_bounded};
use crate::layout::base_feature_class_377_prefix as class_377;
use crate::records::{DesignBaseFeatureConstruction, DesignParameterScope};
use cadmpeg_core::decode::View;

use super::marked_record_reference;

pub(super) fn exact_base_feature_body_based_on_faces(
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
        || scope.reference_member_offsets.as_slice()
            != [scope.byte_offset
                + u64::try_from(class_377::GENERIC_SCOPE_REFERENCE_RECORD).ok()?]
        || scope.history_state_id_offset
            != scope.byte_offset + u64::try_from(class_377::HISTORY_STATE_ID).ok()?
        || scope.kind_offset
            != scope.byte_offset + u64::try_from(class_377::KIND_LENGTH + 4).ok()?
        || scope.feature_ordinal_offset
            != scope.byte_offset + u64::try_from(class_377::FEATURE_ORDINAL).ok()?
        || scope.previous_history_state_id_offset
            != scope.byte_offset + u64::try_from(class_377::PREVIOUS_HISTORY_STATE_ID).ok()?
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
            != scope.reference_members[0]
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
            != scope.reference_members[0]
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
        || View::u32_le_at(bytes, start + class_377::FEATURE_ORDINAL)? != scope.feature_ordinal
    {
        return None;
    }
    Some(DesignBaseFeatureConstruction::BodyBasedOnFaces {
        body_entity_suffixes: vec![u64::from(body_entity_suffix)],
        body_entity_suffix_offsets: vec![
            scope.byte_offset + u64::try_from(class_377::BODY_ENTITY_SUFFIX).ok()?,
        ],
        body_reference_records: vec![body_entity_suffix],
        body_reference_record_offsets: vec![
            scope.byte_offset + u64::try_from(class_377::BODY_ENTITY_SUFFIX).ok()?,
        ],
        parameter_body_record,
        parameter_body_record_offset: scope.byte_offset
            + u64::try_from(class_377::PARAMETER_BODY_RECORD).ok()?,
        auxiliary_record,
        auxiliary_record_offset: scope.byte_offset
            + u64::try_from(class_377::AUXILIARY_RECORD).ok()?,
        envelope_guid,
        envelope_guid_offset: scope.byte_offset + u64::try_from(class_377::ENVELOPE_GUID).ok()?,
        tag_body_based_on_faces: true,
        tag_body_based_on_faces_offset: scope.byte_offset
            + u64::try_from(class_377::TAG_BODY_BASED_ON_FACES_VALUE).ok()?,
    })
}
