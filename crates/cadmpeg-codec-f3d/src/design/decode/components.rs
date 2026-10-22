// SPDX-License-Identifier: Apache-2.0
//! Decode fixed local component-occurrence carriers.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::container::ContainerRole;

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::sketch::next_indexed_record_offset;

use crate::records::feature::assembly_features::DesignComponentOccurrence;

use crate::layout::local_component_occurrence_identity_tail as identity_tail;
use crate::layout::local_component_occurrence_matrix_tail as matrix_tail;
use crate::layout::local_component_occurrence_prefix as prefix;
use crate::layout::root_instance_prefix as root_instance;

const BASE_FRAME_LENGTH: usize = prefix::LEN + identity_tail::LEN;
const PLACED_FRAME_LENGTH: usize = prefix::LEN + matrix_tail::LEN;
/// Width of the row-major 4x4 f64 placement matrix.
const MATRIX_BYTES: usize = 16 * 8;
/// UTF-16 code units of a hyphenated GUID.
const GUID_UNITS: u32 = 36;
/// Width of a marked u64 reference: marker byte and value.
const MARKED_U64: usize = 9;

/// Decode exact local component-occurrence records from every Design bulk stream.
pub(crate) fn decode_component_occurrences(
    scan: &ContainerScan,
) -> Result<Vec<DesignComponentOccurrence>, CodecError> {
    let mut occurrences = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope = crate::ids::native_scope(&entry.name);
        let mut at = 0;
        while let Some(start) = next_indexed_record_offset(bytes, at) {
            if let Some(occurrence) = exact_component_occurrence(bytes, start, &scope)
                .or_else(|| attributed_component_occurrence(bytes, start, &scope))
            {
                occurrences.push(occurrence);
            }
            let Some(next_at) = start.checked_add(1) else {
                break;
            };
            at = next_at;
        }
    }
    occurrences.sort_by(|a, b| a.id.cmp(&b.id));
    occurrences.dedup_by(|left, right| left.id == right.id);
    Ok(occurrences)
}

/// Most attributes an attributed occurrence frame can carry.
const MAX_OCCURRENCE_ATTRIBUTES: u32 = 16;
/// The seven bytes between the key reference and the repeated component
/// reference of an attributed frame.
const ATTRIBUTED_KEY_TAIL: [u8; 7] = [0, 1, 1, 0, 0, 0, 0];

/// Read an attributed local occurrence frame.
///
/// After the indexed header: u32 `0`, an empty LP-ASCII instance name, `u8 1`,
/// u32 path count `1`, the marked u64 component reference at offset 24, two
/// zero bytes, the u32 zero-based occurrence index at offset 35, `u8 1`, and a
/// u32 attribute count at offset 40. Each attribute
/// is an LP-ASCII name, an LP-ASCII type (`IntrinsicMetaTypebool` with one
/// byte, or `IntrinsicMetaTypeHString` with an LP-UTF16 string), and its
/// value. Two bytes, the u32 occurrence ordinal, the LP-UTF16 carrier GUID,
/// and the LP-UTF16 occurrence key follow; then a marked u64 reference, the
/// seven bytes `00 01 01 00 00 00 00`, the repeated marked component
/// reference, two zero bytes, the identity flag, and the common tail.
fn attributed_component_occurrence(
    bytes: &[u8],
    start: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    use crate::records::feature::assembly_features::{
        DesignAttributedOccurrenceFrame, DesignComponentOccurrenceDraft,
        DesignComponentOccurrencePlacement, DesignOccurrenceAttribute,
        DesignOccurrenceAttributeValue,
    };
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    let record_index = View::u32_le_at(bytes, after_tag)?;
    let end = next_indexed_record_offset(bytes, start.checked_add(1)?)?;
    let frame = bytes.get(start..end)?;
    if frame.get(11..19)? != [0; 8]
        || frame.get(19) != Some(&1)
        || View::u32_le_at(frame, 20)? != 1
        || frame.get(24) != Some(&1)
        || frame.get(33..35)? != [0; 2]
        || frame.get(39) != Some(&1)
    {
        return None;
    }
    let component_record_index = View::u64_le_at(frame, 25)?;
    let count = View::u32_le_at(frame, 40)?;
    if !(1..=MAX_OCCURRENCE_ATTRIBUTES).contains(&count) {
        return None;
    }
    let mut at = 44_usize;
    let mut attributes = Vec::new();
    for _ in 0..count {
        let (name, after_name) =
            lp_ascii_filtered_view(frame, at, 1..=64, u8::is_ascii_alphanumeric)?;
        let (kind, after_kind) =
            lp_ascii_filtered_view(frame, after_name, 1..=64, u8::is_ascii_alphanumeric)?;
        let (value, after_value) = match kind {
            "IntrinsicMetaTypebool" => match frame.get(after_kind)? {
                0 => (DesignOccurrenceAttributeValue::Bool(false), after_kind + 1),
                1 => (DesignOccurrenceAttributeValue::Bool(true), after_kind + 1),
                _ => return None,
            },
            "IntrinsicMetaTypeHString" => {
                let (text, after_text) = lp_utf16_bounded(frame, after_kind, 0..=256)?;
                (DesignOccurrenceAttributeValue::Text(text), after_text)
            }
            _ => return None,
        };
        attributes.push(DesignOccurrenceAttribute {
            name: name.to_owned(),
            value,
        });
        at = after_value;
    }
    let ordinal_prefix = View::u16_le_at(frame, at)?;
    let ordinal = std::num::NonZeroU32::new(View::u32_le_at(frame, at + 2)?)?;
    // The zero-based occurrence index at offset 35 precedes the ordinal.
    if View::u32_le_at(frame, 35)?.checked_add(1) != Some(ordinal.get()) {
        return None;
    }
    let component_guid_at = at + 6;
    let (component_guid, after_component) = lp_utf16_bounded(frame, component_guid_at, 36..=36)?;
    let occurrence_key_at = after_component;
    let (occurrence_key, after_key) = lp_utf16_bounded(frame, occurrence_key_at, 1..=36)?;
    if frame.get(after_key) != Some(&1) {
        return None;
    }
    let key_record_index = View::u64_le_at(frame, after_key + 1)?;
    let repeat_at = after_key + 9 + ATTRIBUTED_KEY_TAIL.len();
    if frame.get(after_key + 9..repeat_at)? != ATTRIBUTED_KEY_TAIL
        || frame.get(repeat_at) != Some(&1)
        || View::u64_le_at(frame, repeat_at + 1)? != component_record_index
        || frame.get(repeat_at + 9..repeat_at + 11)? != [0; 2]
    {
        return None;
    }
    let flag_at = repeat_at + 11;
    let (placement, transform_offset, tail_at) = match frame.get(flag_at)? {
        1 if ordinal.get() == 1 => (DesignComponentOccurrencePlacement::Base, None, flag_at + 1),
        0 => {
            let transform = super::scopes::shared_frames::rigid_transform_at(frame, flag_at + 1)?;
            (
                DesignComponentOccurrencePlacement::Explicit { ordinal, transform },
                Some(flag_at + 1),
                flag_at + 1 + MATRIX_BYTES,
            )
        }
        _ => return None,
    };
    if frame.get(tail_at..tail_at + 9)? != [0; 9]
        || frame.get(tail_at + 9) != Some(&1)
        || frame.get(tail_at + 18..)? != [0; 2]
    {
        return None;
    }
    let owner_entity = View::u64_le_at(frame, tail_at + 10)?;
    let absolute = |offset: usize| u64::try_from(start.checked_add(offset)?).ok();
    DesignComponentOccurrence::try_new_attributed(
        DesignComponentOccurrenceDraft {
            id: format!("{stream}:design-component-occurrence#{start}"),
            class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            record_index,
            byte_offset: u64::try_from(start).ok()?,
            component_record_index,
            component_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(component_guid)
                .ok()?,
            occurrence_guid: crate::records::mesh::DesignOccurrenceKey::try_from(occurrence_key)
                .ok()?,
            placement,
            owner_entity,
        },
        DesignAttributedOccurrenceFrame {
            attributes,
            ordinal_prefix,
            key_record_index,
            component_guid_offset: absolute(component_guid_at + 4)?,
            occurrence_guid_offset: absolute(occurrence_key_at + 4)?,
            transform_offset: match transform_offset {
                Some(offset) => Some(absolute(offset)?),
                None => None,
            },
            owner_entity_offset: absolute(tail_at + 10)?,
        },
    )
    .ok()
}

/// Decode one fixed component-occurrence carrier. The class tag is a per-file
/// dynamic value, so the fixed frame identifies the carrier.
fn exact_component_occurrence(
    bytes: &[u8],
    start: usize,
    stream: &str,
) -> Option<DesignComponentOccurrence> {
    const SUFFIX: &str = ":design-component-occurrence#";

    let parsed = (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if after_tag != start.checked_add(7)? {
            return None;
        }
        let record_index = View::u32_le_at(bytes, after_tag)?;
        let end = next_indexed_record_offset(bytes, start.checked_add(1)?)?;
        let frame_length = end.checked_sub(start)?;
        let at = |offset: usize| start + offset;
        if !matches!(frame_length, BASE_FRAME_LENGTH | PLACED_FRAME_LENGTH)
            || bytes.get(at(prefix::EMPTY_INSTANCE_NAME)..at(prefix::PATH_MARKER))? != [0; 8]
            || bytes.get(at(prefix::PATH_MARKER)) != Some(&prefix::PATH_MARKER_VALUE)
            || View::u32_le_at(bytes, at(prefix::PATH_COUNT))? != prefix::PATH_COUNT_VALUE
            || bytes.get(at(prefix::COMPONENT_REFERENCE)) != Some(&1)
            || bytes.get(at(prefix::COMPONENT_REFERENCE_REPEAT) - 1) != Some(&0)
            || bytes.get(at(prefix::COMPONENT_REFERENCE_REPEAT)) != Some(&1)
        {
            return None;
        }
        let component_record_index = View::u64_le_at(bytes, at(prefix::COMPONENT_REFERENCE) + 1)?;
        if View::u64_le_at(bytes, at(prefix::COMPONENT_REFERENCE_REPEAT) + 1)?
            != component_record_index
        {
            return None;
        }
        let occurrence_ordinal =
            std::num::NonZeroU32::new(View::u32_le_at(bytes, at(prefix::OCCURRENCE_ORDINAL))?)?;
        if View::u32_le_at(bytes, at(prefix::CARRIER_GUID)) != Some(GUID_UNITS)
            || bytes
                .get(at(prefix::CARRIER_GUID) + 4..at(prefix::OCCURRENCE_GUID))
                .is_none()
            || View::u32_le_at(bytes, at(prefix::OCCURRENCE_GUID)) != Some(GUID_UNITS)
            || bytes
                .get(at(prefix::OCCURRENCE_GUID) + 4..at(prefix::COMPONENT_REFERENCE_REPEAT) - 1)
                .is_none()
            || bytes.get(
                at(prefix::COMPONENT_REFERENCE_REPEAT) + MARKED_U64..at(prefix::IDENTITY_FLAG),
            )? != [0; 2]
        {
            return None;
        }
        let tail = at(prefix::LEN);
        let (placement, owner_entity) = match frame_length {
            BASE_FRAME_LENGTH => {
                if occurrence_ordinal.get() != 1
                    || bytes.get(at(prefix::IDENTITY_FLAG)) != Some(&1)
                    || bytes.get(tail..tail + identity_tail::OWNER_MARKER)? != [0; 9]
                    || bytes.get(tail + identity_tail::OWNER_MARKER)
                        != Some(&identity_tail::OWNER_MARKER_VALUE)
                    || bytes.get(tail + identity_tail::OWNER_ENTITY + 8..at(BASE_FRAME_LENGTH))?
                        != [0; 2]
                {
                    return None;
                }
                (
                    crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
                    View::u64_le_at(bytes, tail + identity_tail::OWNER_ENTITY)?,
                )
            }
            PLACED_FRAME_LENGTH => {
                if (class_tag == "256" && occurrence_ordinal.get() < 2)
                    || bytes.get(at(prefix::IDENTITY_FLAG)) != Some(&0)
                    || bytes.get(tail + MATRIX_BYTES..tail + matrix_tail::OWNER_MARKER)? != [0; 9]
                    || bytes.get(tail + matrix_tail::OWNER_MARKER)
                        != Some(&matrix_tail::OWNER_MARKER_VALUE)
                    || bytes.get(tail + matrix_tail::OWNER_ENTITY + 8..at(PLACED_FRAME_LENGTH))?
                        != [0; 2]
                {
                    return None;
                }
                let transform = super::scopes::shared_frames::rigid_transform_at(bytes, tail)?;
                (
                    crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Explicit {
                        ordinal: occurrence_ordinal,
                        transform,
                    },
                    View::u64_le_at(bytes, tail + matrix_tail::OWNER_ENTITY)?,
                )
            }
            _ => return None,
        };
        let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
        let byte_offset = u64::try_from(start).ok()?;
        Some((
            class_tag,
            record_index,
            byte_offset,
            component_record_index,
            placement,
            owner_entity,
        ))
    })();
    let (class_tag, record_index, byte_offset, component_record_index, placement, owner_entity) =
        parsed?;
    let (component_guid, after_component) =
        lp_utf16_bounded(bytes, start + prefix::CARRIER_GUID, 36..=36)?;
    let (occurrence_guid, after_occurrence) =
        lp_utf16_bounded(bytes, start + prefix::OCCURRENCE_GUID, 36..=36)?;
    if after_component != start + prefix::OCCURRENCE_GUID
        || after_occurrence != start + prefix::COMPONENT_REFERENCE_REPEAT - 1
    {
        return None;
    }
    let Ok(component_guid) = crate::records::mesh::DesignRelaxedGuidText::try_from(component_guid)
    else {
        return None;
    };
    let Ok(occurrence_guid) =
        crate::records::mesh::DesignRelaxedGuidText::try_from(occurrence_guid)
    else {
        return None;
    };
    let id = format!("{stream}{SUFFIX}{start}");
    DesignComponentOccurrence::try_new(
        crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
            id,
            class_tag,
            record_index,
            byte_offset,
            component_record_index,
            component_guid,
            occurrence_guid: occurrence_guid.into(),
            placement,
            owner_entity,
        },
    )
    .ok()
}

/// Type GUID shared by occurrence-placement records, including the root instance.
const OCCURRENCE_PLACEMENT_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";
/// Instance name of the occurrence-placement record that places the root component.
const ROOT_INSTANCE_NAME: &[u8] = b"rootInstance";

/// Naming space of one local component definition.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentDefinition {
    /// Root entity of the component naming space.
    pub(crate) root_entity: u64,
    /// UUID that selects the component naming space.
    pub(crate) context_uuid: crate::records::mesh::DesignRelaxedGuidText,
    /// The component's name, when its definition record stores one.
    pub(crate) name: Option<String>,
}

/// Local component definitions of one Design stream.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StreamComponents {
    /// Root entity of every component naming space in the stream.
    pub(crate) root_entities: std::collections::BTreeSet<u64>,
    /// Component definition record to the naming space whose binding it stores.
    pub(crate) roots: std::collections::BTreeMap<u64, ComponentDefinition>,
    /// Component definition placed by the root instance, when exactly one names it.
    pub(crate) root_component: Option<u64>,
}

/// Join every Design stream's component definition records to their naming
/// spaces and identify the root component. The map key is the native stream
/// scope of the Design `BulkStream`.
pub(crate) fn decode_local_components(
    scan: &ContainerScan,
    naming_spaces: &[crate::records::recipes::DesignComponentNamingSpace],
) -> Result<std::collections::BTreeMap<String, StreamComponents>, CodecError> {
    let mut out = std::collections::BTreeMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let scope = crate::ids::native_scope(&entry.name);
        let spaces = naming_spaces
            .iter()
            .filter(|space| crate::ids::native_stream(&space.id) == Some(scope.as_str()))
            .collect::<Vec<_>>();
        if spaces.is_empty() {
            continue;
        }
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Ok(frames) = super::meta::design_primary_frames(bytes, &meta) else {
            continue;
        };
        let mut components = StreamComponents::default();
        for space in spaces {
            components
                .root_entities
                .insert(space.component_record_index);
            let Ok(offset) = usize::try_from(space.byte_offset) else {
                continue;
            };
            let Some(frame) = frames
                .get(frames.partition_point(|frame| frame.end <= offset))
                .filter(|frame| frame.start <= offset)
            else {
                continue;
            };
            components.roots.insert(
                frame.entity_id,
                ComponentDefinition {
                    root_entity: space.component_record_index,
                    context_uuid: space.context_uuid.clone(),
                    name: space.name.clone(),
                },
            );
        }
        let mut root_components = frames
            .iter()
            .filter(|frame| {
                frame.design_type.module == "Component"
                    && frame
                        .design_type
                        .type_guid
                        .as_str()
                        .eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)
            })
            .filter_map(|frame| root_instance_component(bytes, frame.start, frame.end));
        components.root_component = root_components.next();
        if root_components.any(|other| Some(other) != components.root_component) {
            components.root_component = None;
        }
        out.insert(scope, components);
    }
    Ok(out)
}

/// The component definition named by a root-instance placement record.
fn root_instance_component(bytes: &[u8], start: usize, end: usize) -> Option<u64> {
    let at = |offset: usize| start.checked_add(offset);
    if at(root_instance::LEN)? > end
        || View::u32_le_at(bytes, at(root_instance::LEADING_ZERO)?)?
            != root_instance::LEADING_ZERO_VALUE
        || View::u32_le_at(bytes, at(root_instance::INSTANCE_NAME_LENGTH)?)?
            != root_instance::INSTANCE_NAME_LENGTH_VALUE
        || bytes.get(at(root_instance::INSTANCE_NAME)?..at(root_instance::PATH_MARKER)?)?
            != ROOT_INSTANCE_NAME
        || bytes.get(at(root_instance::PATH_MARKER)?) != Some(&root_instance::PATH_MARKER_VALUE)
        || View::u32_le_at(bytes, at(root_instance::PATH_COUNT)?)?
            != root_instance::PATH_COUNT_VALUE
        || bytes.get(at(root_instance::COMPONENT_MARKER)?)
            != Some(&root_instance::COMPONENT_MARKER_VALUE)
    {
        return None;
    }
    View::u64_le_at(bytes, at(root_instance::COMPONENT_REFERENCE)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::trivially_copy_pass_by_ref)]

    use super::{
        attributed_component_occurrence, exact_component_occurrence, root_instance_component,
    };
    use crate::test_support::indexed_header;

    const COMPONENT: &str = "a989beb9-467b-4afa-9e90-a9329a2ca258";
    const OCCURRENCE: &str = "f2371d14-7339-4f5c-82a1-50ec8fca5597";

    fn guid(bytes: &mut [u8], at: usize, value: &str) {
        bytes[at..at + 4].copy_from_slice(&36_u32.to_le_bytes());
        for (ordinal, unit) in value.encode_utf16().enumerate() {
            bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    fn common(frame_length: usize, ordinal: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 20);
        bytes.resize(frame_length, 0);
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[24] = 1;
        bytes[25..33].copy_from_slice(&10_u64.to_le_bytes());
        bytes[40..44].copy_from_slice(&ordinal.to_le_bytes());
        guid(&mut bytes, 44, COMPONENT);
        guid(&mut bytes, 120, OCCURRENCE);
        bytes[197] = 1;
        bytes[198..206].copy_from_slice(&10_u64.to_le_bytes());
        bytes
    }

    #[test]
    fn fixed_component_occurrence_frames_distinguish_seed_and_generated_placements() {
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        let seed = exact_component_occurrence(&seed, 0, "f3d:Design/BulkStream.dat")
            .expect("seed occurrence");
        assert_eq!(seed.component_guid.as_str(), COMPONENT);
        assert_eq!(seed.occurrence_guid.as_str(), OCCURRENCE);
        assert_eq!(seed.occurrence_ordinal(), 1);
        assert_eq!(seed.transform(), None);

        let mut generated = common(357, 2);
        let transform: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            generated[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        generated[346] = 1;
        indexed_header(&mut generated, *b"325", 21);
        let generated = exact_component_occurrence(&generated, 0, "f3d:Design/BulkStream.dat")
            .expect("generated occurrence");
        assert_eq!(generated.occurrence_ordinal(), 2);
        assert_eq!(
            generated.transform().map(|frame| frame.value),
            Some(transform.try_into().unwrap())
        );
        assert_eq!(generated.transform().map(|frame| frame.offset), Some(209));

        let mut legacy = common(229, 1);
        legacy[4..7].copy_from_slice(b"327");
        legacy[208] = 1;
        legacy[218] = 1;
        indexed_header(&mut legacy, *b"333", 21);
        let legacy = exact_component_occurrence(&legacy, 0, "f3d:Design/BulkStream.dat")
            .expect("legacy occurrence");
        assert_eq!(legacy.component_guid.as_str(), COMPONENT);
        assert_eq!(legacy.occurrence_guid.as_str(), OCCURRENCE);

        let mut legacy_placed = common(357, 1);
        legacy_placed[4..7].copy_from_slice(b"327");
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            legacy_placed[209 + ordinal * 8..217 + ordinal * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        legacy_placed[346] = 1;
        indexed_header(&mut legacy_placed, *b"325", 21);
        let legacy_placed =
            exact_component_occurrence(&legacy_placed, 0, "f3d:Design/BulkStream.dat")
                .expect("legacy placed occurrence");
        assert_eq!(legacy_placed.occurrence_ordinal(), 1);
        assert_eq!(
            legacy_placed.transform().map(|frame| frame.value),
            Some(transform.try_into().unwrap())
        );

        // The carrier class tag is a per-file dynamic value, so the fixed frame
        // alone identifies the carrier and a third tag reads the same members.
        let mut dynamic_tag = common(357, 1);
        dynamic_tag[4..7].copy_from_slice(b"336");
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            dynamic_tag[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        dynamic_tag[346] = 1;
        indexed_header(&mut dynamic_tag, *b"325", 21);
        let dynamic_tag = exact_component_occurrence(&dynamic_tag, 0, "f3d:Design/BulkStream.dat")
            .expect("dynamic-tag placed occurrence");
        assert_eq!(dynamic_tag.class_tag.as_str(), "336");
        assert_eq!(dynamic_tag.occurrence_ordinal(), 1);
        assert_eq!(
            dynamic_tag.transform().map(|frame| frame.value),
            Some(transform.try_into().unwrap())
        );

        // A class-256 carrier still cannot use a placed frame for ordinal one.
        let mut placed_seed = common(357, 1);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            placed_seed[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        placed_seed[346] = 1;
        indexed_header(&mut placed_seed, *b"325", 21);
        assert!(exact_component_occurrence(&placed_seed, 0, "f3d:Design/BulkStream.dat").is_none());
    }

    #[test]
    fn component_occurrence_frames_store_their_containing_component_root() {
        let mut identity = common(229, 1);
        identity[208] = 1;
        identity[218] = 1;
        identity[219..227].copy_from_slice(&277_u64.to_le_bytes());
        indexed_header(&mut identity, *b"333", 21);
        let identity = exact_component_occurrence(&identity, 0, "f3d:Design/BulkStream.dat")
            .expect("identity occurrence");
        assert_eq!(identity.owner_entity, 277);

        let mut placed = common(357, 2);
        for (ordinal, value) in [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .into_iter()
        .enumerate()
        {
            placed[209 + ordinal * 8..217 + ordinal * 8].copy_from_slice(&f64::to_le_bytes(value));
        }
        placed[346] = 1;
        placed[347..355].copy_from_slice(&277_u64.to_le_bytes());
        indexed_header(&mut placed, *b"325", 21);
        let placed = exact_component_occurrence(&placed, 0, "f3d:Design/BulkStream.dat")
            .expect("placed occurrence");
        assert_eq!(placed.owner_entity, 277);
    }

    #[test]
    fn root_instance_record_names_the_root_component_definition() {
        let mut record = Vec::new();
        indexed_header(&mut record, *b"339", 548);
        record.extend_from_slice(&0_u32.to_le_bytes());
        record.extend_from_slice(&12_u32.to_le_bytes());
        record.extend_from_slice(b"rootInstance");
        record.push(1);
        record.extend_from_slice(&1_u32.to_le_bytes());
        record.push(1);
        record.extend_from_slice(&276_u64.to_le_bytes());
        record.extend_from_slice(&[0; 7]);
        assert_eq!(root_instance_component(&record, 0, record.len()), Some(276));

        let mut renamed = record.clone();
        renamed[19..31].copy_from_slice(b"rootInstancf");
        assert_eq!(root_instance_component(&renamed, 0, renamed.len()), None);
        let mut two_elements = record;
        two_elements[32..36].copy_from_slice(&2_u32.to_le_bytes());
        assert_eq!(
            root_instance_component(&two_elements, 0, two_elements.len()),
            None
        );
    }

    fn push_ascii(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(
            &u32::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes(),
        );
        for unit in value.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }

    /// An attributed occurrence frame with a decimal occurrence key, its
    /// second occurrence (index 1), and a matrix.
    fn attributed(key: &str, index: u32, ordinal: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"373", 20);
        bytes.extend_from_slice(&[0; 8]);
        bytes.push(1);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&187_u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        push_ascii(&mut bytes, "GatedByParent");
        push_ascii(&mut bytes, "IntrinsicMetaTypebool");
        bytes.push(0);
        push_ascii(&mut bytes, "ResolutionTime");
        push_ascii(&mut bytes, "IntrinsicMetaTypeHString");
        push_utf16(&mut bytes, "");
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(&ordinal.to_le_bytes());
        push_utf16(&mut bytes, COMPONENT);
        push_utf16(&mut bytes, key);
        bytes.push(1);
        bytes.extend_from_slice(&139_u64.to_le_bytes());
        bytes.extend_from_slice(&[0, 1, 1, 0, 0, 0, 0]);
        bytes.push(1);
        bytes.extend_from_slice(&187_u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.push(0);
        let transform: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 2.0],
            [0.0, 1.0, 0.0, 3.0],
            [0.0, 0.0, 1.0, 4.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for value in transform.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&6_u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        indexed_header(&mut bytes, *b"262", 20);
        bytes
    }

    #[test]
    fn attributed_component_occurrence_frame_reads_a_decimal_occurrence_key() {
        let bytes = attributed("34", 1, 2);
        let occurrence = attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .expect("attributed occurrence");
        assert_eq!(occurrence.occurrence_guid.as_str(), "34");
        assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
        assert_eq!(occurrence.component_record_index, 187);
        assert_eq!(occurrence.occurrence_ordinal(), 2);
        assert_eq!(occurrence.owner_entity, 6);
        let transform = occurrence.transform().expect("matrix");
        assert_eq!(transform.value.rows()[2][3], 4.0);
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(wire["attributed"]["key_record_index"], 139);
        assert_eq!(wire["attributed"]["attributes"][0]["name"], "GatedByParent");
        let round_trip: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_value(wire).unwrap();
        assert_eq!(round_trip, occurrence);

        // A GUID key reads too; the zero-based index must match the ordinal.
        let bytes = attributed(OCCURRENCE, 1, 2);
        assert!(attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").is_some());
        let bytes = attributed("34", 0, 2);
        assert!(attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat").is_none());
    }
}
