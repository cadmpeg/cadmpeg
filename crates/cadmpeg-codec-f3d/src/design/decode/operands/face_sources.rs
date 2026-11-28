// SPDX-License-Identifier: Apache-2.0
//! Face source groups, carriers, and reference headers.

use super::extrude_selections::parse_extrude_identity_member;
use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::layout::{
    indexed_design_record_header as indexed_header,
    sheet_metal_region_carrier_prefix as region_carrier,
    sheet_metal_region_carrier_scalar_lane as region_scalar,
    sheet_metal_region_identity_reference as region_reference,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::SegmentType;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::records::topology::construction::DesignConstructionPersistentIdentity;
use crate::records::topology::face::{DesignFaceSourceGroup, DesignFaceSourceMember};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode ordered sheet-metal source-region identities.
pub(crate) fn decode_face_source_groups(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    types: &[SegmentType],
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignFaceSourceGroup>, CodecError> {
    let mut out = Vec::new();
    for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == crate::records::feature::scope::DesignFeatureKind::Face)
    {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        if !face_source_record_is_registered(
            types,
            &scope.id,
            scope.record_index,
            scope.class_tag.as_str(),
            FaceSourceType::Scope,
        ) {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = scan.design_record_offsets(&entry.name)?;
        let Ok(scope_start) = usize::try_from(scope.byte_offset()) else {
            continue;
        };
        let reference_headers = face_source_reference_headers(
            bytes,
            scope_start,
            scope.reference_members().values(),
            &records,
        );
        for (carrier_ordinal, carrier) in reference_headers.iter().enumerate() {
            let Some(FaceSourceReferenceHeader {
                record_index: carrier_record_index,
                byte_offset: carrier_byte_offset,
                class_tag: carrier_class_tag,
            }) = carrier
            else {
                continue;
            };
            if !face_source_record_is_registered(
                types,
                &scope.id,
                *carrier_record_index,
                carrier_class_tag,
                FaceSourceType::Carrier,
            ) {
                continue;
            }
            let Some(FaceSourceReferenceHeader {
                record_index: paired_record_index,
                byte_offset: paired_byte_offset,
                class_tag: paired_class_tag,
            }) = reference_headers
                .get(carrier_ordinal + 1)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            if !face_source_record_is_registered(
                types,
                &scope.id,
                *paired_record_index,
                paired_class_tag,
                FaceSourceType::Settings,
            ) {
                continue;
            }
            let Some(source_reference_offsets) = parse_face_source_carrier_prefix(
                ctx,
                bytes,
                *carrier_byte_offset,
                *paired_byte_offset,
                scope.record_index,
            )?
            else {
                continue;
            };
            let pair_count = face_source_profile_pair_count(types, scope, |record| {
                reference_headers
                    .iter()
                    .flatten()
                    .find(|header| header.record_index == record)
                    .map(|header| header.class_tag)
            });
            if pair_count != Some(source_reference_offsets.len()) {
                continue;
            }
            let parsed_source_members = source_reference_offsets.into_iter()
                .map(|(offset, source_record_index)| -> Result<Option<_>, CodecError> {
                    let Some(source_byte_offset) = carrier_byte_offset.checked_add(indexed_header::LEN).and_then(|at| records.first_at_or_after(at, source_record_index)) else { return Ok(None); };
                    let Some((source_class_tag, _)) =
                        lp_ascii_filtered_view(bytes, source_byte_offset, 3..=3, u8::is_ascii_digit) else { return Ok(None); };
                    let source_class = source_class_tag;
                    if !face_source_record_is_registered(types, &scope.id, source_record_index, source_class, FaceSourceType::Identity) { return Ok(None); }
                    let Some(member) = parse_extrude_identity_member(bytes, source_byte_offset, None) else { return Ok(None); };
                    let Ok(source_byte_offset_u64) = u64::try_from(source_byte_offset) else { return Ok(None); };
                    let Ok(offset) = u64::try_from(offset) else { return Ok(None); };
                    let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(source_class_tag) else { return Ok(None); };
                    let (Ok(asset_id), Ok(context_id)) = (member.asset_id.try_into(), member.context_id.try_into()) else { return Ok(None); };
                    let Some(persistent_identity) = DesignConstructionPersistentIdentity::try_new(crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                        local_id: member.local_id,
                        local_id_offset: member.local_id_offset,
                        asset_id,
                        asset_id_offset: member.asset_id_offset,
                        context_id,
                        context_id_offset: member.context_id_offset,
                        tail_slot_present: member.tail_slot_present,
                        tail_slot_offset: member.tail_slot_offset,
                        next_record_index: member.next_record_index,
                        next_byte_offset: member.next_byte_offset,
                    }).ok() else { return Ok(None); };
                    Ok(Some(crate::records::identity::Located {
                        offset,
                        value: DesignFaceSourceMember {
                            record_index: source_record_index,
                            byte_offset: source_byte_offset_u64,
                            class_tag,
                            persistent_identity,
                        },
                    }))
                });
            let mut source_members = Vec::new();
            let mut complete = true;
            for member in parsed_source_members {
                match member? {
                    Some(member) => source_members.push(member),
                    None => complete = false,
                }
            }
            if !complete {
                continue;
            }
            let Ok(carrier_reference_ordinal) = u32::try_from(carrier_ordinal) else {
                continue;
            };
            let (Ok(carrier_start), Ok(carrier_end)) = (
                u64::try_from(*carrier_byte_offset),
                u64::try_from(*paired_byte_offset),
            ) else {
                continue;
            };
            let Some(carrier_span) =
                crate::records::identity::NonEmptyByteSpan::new(carrier_start, carrier_end)
            else {
                continue;
            };
            let (Ok(carrier_class_tag), Ok(paired_class_tag)) = (
                crate::design::decode::text::class_tag_from_view(carrier_class_tag),
                crate::design::decode::text::class_tag_from_view(paired_class_tag),
            ) else {
                continue;
            };
            out.push(DesignFaceSourceGroup {
                id: design_record_id(&entry.name, ":design-face-source-group#", carrier_start),
                scope_record_index: scope.record_index,
                carrier_reference_ordinal,
                carrier_record_index: *carrier_record_index,
                carrier_span,
                carrier_class_tag,
                paired_record_index: *paired_record_index,
                paired_class_tag,
                source_members,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(super) struct FaceSourceReferenceHeader<'a> {
    pub(super) record_index: u32,
    pub(super) byte_offset: usize,
    pub(super) class_tag: &'a str,
}

pub(super) fn face_source_reference_headers<'a, 'r>(
    bytes: &'a [u8],
    scope_start: usize,
    references: impl Iterator<Item = &'r u32>,
    records: &IndexedRecordOffsets,
) -> Vec<Option<FaceSourceReferenceHeader<'a>>> {
    references
        .map(|record_index| {
            scope_start
                .checked_add(indexed_header::LEN)
                .and_then(|at| records.first_at_or_after(at, *record_index))
                .and_then(|byte_offset| {
                    lp_ascii_filtered_view(bytes, byte_offset, 3..=3, u8::is_ascii_digit).map(
                        |(class_tag, _)| FaceSourceReferenceHeader {
                            record_index: *record_index,
                            byte_offset,
                            class_tag,
                        },
                    )
                })
        })
        .collect()
}

/// Counted source-reference run followed by an opaque scalar lane.
#[derive(Clone, Copy)]
pub(crate) struct FaceSourceCarrierLayout {
    pub(crate) source_count: usize,
    pub(crate) source_reference_offset: usize,
    pub(crate) scalar_offset: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum FaceSourceType {
    Scope,
    Carrier,
    Settings,
    Identity,
    SourceGroup,
    Profile,
}

impl FaceSourceType {
    pub(super) fn matches(
        self,
        registration: &crate::records::entity_header::SegmentTypeData,
    ) -> bool {
        let (guid, module, versions): (&str, &str, &[u32]) = match self {
            Self::Scope => ("7AFA0A65-B53C-472C-ACA4-5C7C0A65001C", "Fusion", &[3, 4, 7]),
            Self::Carrier => ("CAA55628-037B-4D01-9C74-26118F70B63E", "Fusion", &[0]),
            Self::Settings => ("E75C86D6-76D8-4F1F-BC16-AB8CB74958B0", "Fusion", &[2, 3]),
            Self::Identity => (
                "C46D3EEB-40E1-43FA-83DC-9CB204335417",
                "EntityTracking",
                &[1],
            ),
            Self::SourceGroup => ("2CA5A1CD-C99B-4C9B-91AF-57989148E841", "Fusion", &[1]),
            Self::Profile => ("4BD53E5A-0B3E-45E5-AE8B-02044306485A", "Fusion", &[0]),
        };
        registration.type_guid.as_str().eq_ignore_ascii_case(guid)
            && registration.module == module
            && versions.contains(&registration.version)
    }
}

pub(crate) fn face_source_record_is_registered(
    types: &[SegmentType],
    source_id: &str,
    record_index: u32,
    class_tag: &str,
    role: FaceSourceType,
) -> bool {
    let Some(segment) = crate::ids::design_segment(source_id) else {
        return false;
    };
    let mut matches = types.iter().filter(|row| {
        crate::ids::design_segment(row.id()) == Some(segment)
            && role.matches(row)
            && row
                .entities
                .values()
                .any(|entity| *entity == u64::from(record_index))
    });
    let Some(row) = matches.next() else {
        return false;
    };
    if matches.next().is_some() {
        return false;
    }
    let ordinal = types
        .iter()
        .filter(|other| {
            crate::ids::design_segment(other.id()) == Some(segment)
                && other.byte_offset < row.byte_offset
        })
        .count();
    ordinal
        .checked_add(256)
        .is_some_and(|value| value.to_string() == class_tag)
}

pub(crate) fn face_source_profile_pair_count<'a>(
    types: &[SegmentType],
    scope: &DesignParameterScope,
    mut class_for: impl FnMut(u32) -> Option<&'a str>,
) -> Option<usize> {
    let mut count = 0usize;
    let mut references = scope.reference_members().values().peekable();
    while let Some(group) = references.next() {
        let Some(class) = class_for(*group) else {
            continue;
        };
        if !face_source_record_is_registered(
            types,
            &scope.id,
            *group,
            class,
            FaceSourceType::SourceGroup,
        ) {
            continue;
        }
        let member = **references.peek()?;
        if !face_source_record_is_registered(
            types,
            &scope.id,
            member,
            class_for(member)?,
            FaceSourceType::Profile,
        ) {
            return None;
        }
        count = count.checked_add(1)?;
    }
    (count != 0).then_some(count)
}

pub(crate) fn face_source_carrier_spec(source_count: usize) -> Option<FaceSourceCarrierLayout> {
    (source_count != 0).then_some(())?;
    Some(FaceSourceCarrierLayout {
        source_count,
        source_reference_offset: region_carrier::LEN,
        scalar_offset: region_carrier::LEN
            .checked_add(source_count.checked_mul(region_reference::LEN)?)?,
    })
}

pub(super) fn parse_face_source_carrier_prefix(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    scope_record_index: u32,
) -> Result<Option<Vec<(usize, u32)>>, CodecError> {
    let layout = (|| {
        if end > bytes.len() {
            return None;
        }
        let remaining = end.checked_sub(
            start
                .checked_add(region_carrier::LEN)?
                .checked_add(region_scalar::LEN)?,
        )?;
        let count = View::u32_le_at(bytes, start.checked_add(region_carrier::IDENTITY_COUNT)?)?;
        let count = bounded_len(u64::from(count), region_reference::LEN, remaining)?;
        let layout = face_source_carrier_spec(count)?;
        let scalar = start.checked_add(layout.scalar_offset)?;
        if bytes.get(
            start.checked_add(region_carrier::ZERO_RUN_10)?
                ..start.checked_add(region_carrier::OWNER_MARKER)?,
        )? != [0; 10]
            || marked_face_source_reference(
                bytes,
                start.checked_add(region_carrier::OWNER_MARKER)?,
            )? != scope_record_index
            || !View::f64_le_at(bytes, scalar.checked_add(region_scalar::SCALAR)?)?.is_finite()
            || View::u32_le_at(bytes, scalar.checked_add(region_scalar::DISCRIMINATOR)?)?
                != View::u32_le_at(
                    bytes,
                    scalar.checked_add(region_scalar::REPEATED_DISCRIMINATOR)?,
                )?
        {
            return None;
        }
        Some(layout)
    })();
    let Some(layout) = layout else {
        return Ok(None);
    };
    let mut references = ctx.alloc_filled(
        layout.source_count,
        (0, 0),
        "F3D sheet-metal source-region references",
    )?;
    for (ordinal, slot) in references.iter_mut().enumerate() {
        let offset = start + layout.source_reference_offset + ordinal * region_reference::LEN;
        let Some(record) = marked_face_source_reference(bytes, offset) else {
            return Ok(None);
        };
        *slot = (offset, record);
    }
    Ok(Some(references))
}

pub(super) fn marked_face_source_reference(bytes: &[u8], offset: usize) -> Option<u32> {
    if bytes.get(offset.checked_add(region_reference::MARKER)?) != Some(&1)
        || bytes.get(
            offset.checked_add(region_reference::ZERO_TRAILER)?
                ..offset.checked_add(region_reference::LEN)?,
        )? != [0; 6]
    {
        return None;
    }
    View::u32_le_at(bytes, offset.checked_add(region_reference::RECORD_INDEX)?)
        .filter(|record| *record != 0)
}

/// Index recipes by their final record for each native recipe identity.
pub(super) fn indexed_operand_recipes(
    recipes: &[ConstructionRecipe],
) -> HashMap<&str, &ConstructionRecipe> {
    let mut indexed = HashMap::new();
    for recipe in recipes {
        indexed.insert(recipe.id.as_str(), recipe);
    }
    indexed
}

pub(super) fn indexed_operand_headers(
    headers: &[DesignRecordHeader],
) -> HashMap<(&str, u32), &DesignRecordHeader> {
    let mut indexed = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        indexed.insert(key, header);
    }
    indexed
}

pub(super) fn referenced_operand_faces(
    references: &[crate::records::dimensions::DesignRecipeReference],
    design_reference: i64,
) -> HashSet<&cadmpeg_ir::ids::FaceId> {
    let mut referenced = HashSet::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == design_reference)
        .flat_map(|reference| &reference.candidate_faces)
    {
        referenced.insert(face);
    }
    referenced
}
