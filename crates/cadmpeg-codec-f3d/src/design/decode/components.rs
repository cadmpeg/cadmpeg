// SPDX-License-Identifier: Apache-2.0
//! Decode fixed local component-occurrence carriers.

use crate::bytes::lp_utf16_bounded_charged;
use cadmpeg_core::container::ContainerRole;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::container::ContainerScan;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::sketch::{
    native_scope_scoped, next_indexed_record_header, IndexedRecordHeader,
};

use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrencePlacement,
};

const BASE_FRAME_LENGTH: usize = 229;
const PLACED_FRAME_LENGTH: usize = 357;

/// Decode exact local component-occurrence records from every Design bulk stream.
/// Each stream is searched forward once; every indexed header found closes the
/// frame of the header before it.
pub(crate) fn decode_component_occurrences(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignComponentOccurrence>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "f3d component occurrence candidates")?;
    let mut candidates = Vec::new();
    for entry in ctx.admit_iter(&scan.entries, "scan F3D component occurrence streams")? {
        if !scan.is_design_stream(ctx, entry, ContainerRole::Bulkstream)? {
            continue;
        }
        let bytes = scan.entry_bytes(ctx, &entry.name)?;
        let (_scope_reservation, scope) = native_scope_scoped(ctx, &entry.name)?;
        let mut current = next_indexed_record_header(ctx, bytes, 0, |_| true)?;
        while let Some(header) = current {
            // A header offset indexes `bytes`, so the successor stays in range.
            let next = next_indexed_record_header(ctx, bytes, header.offset + 1, |_| true)?;
            if let Some(end) = next.map(|next| next.offset) {
                let (occurrence, storage) = ctx
                    .with_scoped_storage("f3d decoded component occurrence", || {
                        exact_component_occurrence(ctx, bytes, &header, end, &scope)
                    })?;
                if let Some(occurrence) = occurrence {
                    ctx.push_scoped_vec(
                        &mut scratch,
                        &mut candidates,
                        (occurrence, storage),
                        "f3d component occurrence candidates",
                    )?;
                }
            }
            current = next;
        }
    }
    ctx.stable_sort_by(
        &mut candidates[..],
        |(value, _)| &value.id,
        Ord::cmp,
        "sort f3d design components 1",
    )?;
    // The first occurrence of each identity becomes retained.
    let mut occurrences = Vec::new();
    for (occurrence, storage) in candidates {
        ctx.charge_work(1, "dedup f3d design components")?;
        if let Some(kept) = occurrences.last() {
            let kept: &DesignComponentOccurrence = kept;
            if ctx.equal_bytes(
                kept.id.as_bytes(),
                occurrence.id.as_bytes(),
                "dedup f3d design components",
            )? {
                continue;
            }
        }
        storage.commit()?;
        ctx.push_vec(
            &mut occurrences,
            occurrence,
            "f3d decoded component occurrence",
        )?;
    }
    Ok(occurrences)
}

/// The fixed members of a component-occurrence carrier whose indexed header
/// is `header` and whose frame ends at `end`: the component-definition record
/// index and the placement. The class tag is a per-file dynamic value, so the
/// fixed frame identifies the carrier. The test reads a constant number of
/// bytes.
fn component_occurrence_layout(
    bytes: &[u8],
    header: &IndexedRecordHeader<'_>,
    end: usize,
) -> Option<(u64, DesignComponentOccurrencePlacement)> {
    let start = header.offset;
    let frame_length = end.checked_sub(start)?;
    if !matches!(frame_length, BASE_FRAME_LENGTH | PLACED_FRAME_LENGTH)
        || !zeros_at::<8>(bytes, start + 11)
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20)? != 1
        || bytes.get(start + 24) != Some(&1)
        || bytes.get(start + 196) != Some(&0)
        || bytes.get(start + 197) != Some(&1)
    {
        return None;
    }
    let component_record_index = View::u64_le_at(bytes, start + 25)?;
    if View::u64_le_at(bytes, start + 198)? != component_record_index {
        return None;
    }
    let occurrence_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, start + 40)?)?;
    if View::u32_le_at(bytes, start + 44) != Some(36)
        || bytes.get(start + 48..start + 120).is_none()
        || View::u32_le_at(bytes, start + 120) != Some(36)
        || bytes.get(start + 124..start + 196).is_none()
    {
        return None;
    }
    let placement = match frame_length {
        BASE_FRAME_LENGTH => {
            if occurrence_ordinal.get() != 1
                || bytes_at::<12>(bytes, start + 206) != Some(&[0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                || bytes.get(start + 218) != Some(&1)
                || !zeros_at::<2>(bytes, start + 227)
            {
                return None;
            }
            DesignComponentOccurrencePlacement::Base
        }
        PLACED_FRAME_LENGTH => {
            if (header.class_tag == b"256" && occurrence_ordinal.get() < 2)
                || !zeros_at::<3>(bytes, start + 206)
                || !zeros_at::<9>(bytes, start + 337)
                || bytes.get(start + 346) != Some(&1)
                || !zeros_at::<2>(bytes, start + 355)
            {
                return None;
            }
            let transform = super::scopes::shared_frames::rigid_transform_at(bytes, start + 209)?;
            DesignComponentOccurrencePlacement::Explicit {
                ordinal: occurrence_ordinal,
                transform,
            }
        }
        _ => return None,
    };
    Some((component_record_index, placement))
}

/// Decode one fixed component-occurrence carrier whose indexed header is
/// `header` and whose frame ends at `end`.
fn exact_component_occurrence(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &IndexedRecordHeader<'_>,
    end: usize,
    stream: &str,
) -> Result<Option<DesignComponentOccurrence>, CodecError> {
    const SUFFIX: &str = ":design-component-occurrence#";

    let start = header.offset;
    let Some((component_record_index, placement)) = component_occurrence_layout(bytes, header, end)
    else {
        return Ok(None);
    };
    let Ok(byte_offset) = u64::try_from(start) else {
        return Ok(None);
    };
    let Some((component_guid, after_component)) =
        lp_utf16_bounded_charged(ctx, bytes, start + 44, 36..=36, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    let Some((occurrence_guid, after_occurrence)) =
        lp_utf16_bounded_charged(ctx, bytes, start + 120, 36..=36, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    if after_component != start + 120 || after_occurrence != start + 196 {
        return Ok(None);
    }
    let Ok(component_guid) = crate::records::mesh::DesignRelaxedGuidText::try_from(component_guid)
    else {
        return Ok(None);
    };
    let Ok(occurrence_guid) =
        crate::records::mesh::DesignRelaxedGuidText::try_from(occurrence_guid)
    else {
        return Ok(None);
    };
    let class_tag = header.retain_class_tag(ctx, "copy F3D class tag")?;
    let id = ctx.format_retained(
        format_args!("{stream}{SUFFIX}{start}"),
        "f3d component occurrence id",
    )?;
    Ok(DesignComponentOccurrence::try_new(
        crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
            id,
            class_tag,
            record_index: header.record_index,
            byte_offset,
            component_record_index,
            component_guid,
            occurrence_guid,
            placement,
        },
    )
    .ok())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::trivially_copy_pass_by_ref)]

    use crate::records::feature::assembly_features::DesignComponentOccurrence;
    use crate::test_support::indexed_header;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    const COMPONENT: &str = "a989beb9-467b-4afa-9e90-a9329a2ca258";
    const OCCURRENCE: &str = "f2371d14-7339-4f5c-82a1-50ec8fca5597";

    fn guid(bytes: &mut [u8], at: usize, value: &str) {
        bytes[at..at + 4].copy_from_slice(&36_u32.to_le_bytes());
        for (ordinal, unit) in value.encode_utf16().enumerate() {
            bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }

    /// Parse the carrier at offset zero of a frame closed by the trailing
    /// eleven-byte indexed header.
    fn exact_component_occurrence(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        start: usize,
        stream: &str,
    ) -> Result<Option<DesignComponentOccurrence>, CodecError> {
        let header =
            crate::design::decode::sketch::indexed_record_header_at(bytes, start).expect("carrier");
        super::exact_component_occurrence(ctx, bytes, &header, bytes.len() - 11, stream)
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
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        let seed = exact_component_occurrence(&ctx, &seed, 0, "f3d:Design/BulkStream.dat")
            .unwrap()
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
        let generated =
            exact_component_occurrence(&ctx, &generated, 0, "f3d:Design/BulkStream.dat")
                .unwrap()
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
        let legacy = exact_component_occurrence(&ctx, &legacy, 0, "f3d:Design/BulkStream.dat")
            .unwrap()
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
            exact_component_occurrence(&ctx, &legacy_placed, 0, "f3d:Design/BulkStream.dat")
                .unwrap()
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
        let dynamic_tag =
            exact_component_occurrence(&ctx, &dynamic_tag, 0, "f3d:Design/BulkStream.dat")
                .unwrap()
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
        assert!(
            exact_component_occurrence(&ctx, &placed_seed, 0, "f3d:Design/BulkStream.dat")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn component_occurrence_id_refuses_retained_limit() {
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        let stream = "f3d:synthetic";
        let id_bytes = stream.len() + ":design-component-occurrence#".len() + 1;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(72 + id_bytes - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = exact_component_occurrence(&ctx, &seed, 0, stream)
            .expect_err("one native occurrence ID exceeds the retained-byte limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "f3d component occurrence id")
        );
    }

    #[test]
    fn component_occurrence_guid_text_refuses_each_retained_limit() {
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        for limit in [35, 71] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error = exact_component_occurrence(&ctx, &seed, 0, "f3d:synthetic")
                .err()
                .unwrap();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "f3d Design UTF-16 text")
            );
        }
    }

    #[test]
    fn decoded_component_occurrence_refuses_collection_limit() {
        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        let arena = DecodeArena::new();
        let (parse_ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
        let occurrence = exact_component_occurrence(&parse_ctx, &seed, 0, "f3d:synthetic")
            .unwrap()
            .expect("valid fixed component occurrence");
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx
            .push_vec(
                &mut Vec::new(),
                occurrence,
                "f3d decoded component occurrence",
            )
            .expect_err("one decoded occurrence needs one collection item");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d decoded component occurrence")
        );
    }
    #[test]
    fn component_occurrence_stream_search_charges_each_visited_byte() {
        use std::io::{Cursor, Write};

        let mut seed = common(229, 1);
        seed[208] = 1;
        seed[218] = 1;
        indexed_header(&mut seed, *b"333", 21);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
        archive
            .start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
            .expect("Design stream entry");
        archive
            .write_all(&seed)
            .expect("component occurrence frame");
        let archive = archive.finish().expect("synthetic archive").into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let output = crate::test_support::with_decode_context(|ctx| {
                super::decode_component_occurrences(ctx, scan)
            })
            .expect("valid fixed component occurrence");
            let [occurrence] = output.as_slice() else {
                panic!("expected one component occurrence");
            };
            assert_eq!(
                occurrence.id,
                "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-component-occurrence#0"
            );
            assert_eq!(occurrence.class_tag.as_str(), "256");
            assert_eq!(occurrence.record_index, 20);
            assert_eq!(occurrence.byte_offset(), 0);
            assert_eq!(
                serde_json::to_value(occurrence).expect("native component record")
                    ["component_record_index"],
                10,
            );
            assert_eq!(occurrence.component_guid.as_str(), COMPONENT);
            assert_eq!(occurrence.occurrence_guid.as_str(), OCCURRENCE);
            assert_eq!(occurrence.occurrence_ordinal(), 1);
            assert_eq!(occurrence.transform(), None);
            // The header searches visit each of the 240 stream bytes once.
            let operation = "find F3D indexed record header";
            for skip in [0, 239] {
                let error = crate::test_support::resource_refusal_at(
                    ResourceDimension::WorkUnits,
                    operation,
                    skip,
                    |ctx| super::decode_component_occurrences(ctx, scan).map(|_| ()),
                );
                assert!(matches!(
                    error,
                    cadmpeg_core::CodecError::ResourceLimit(refusal)
                        if refusal.dimension == ResourceDimension::WorkUnits
                            && refusal.operation == operation
                            && refusal.additional == 1
                ));
            }
        });
    }
}
