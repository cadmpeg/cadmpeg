// SPDX-License-Identifier: Apache-2.0
//! Parse exact raster and face bindings owned by Design `Decal` scopes.

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::lp_utf16_bounded;
use crate::container::ContainerScan;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::scopes::shared_frames::marked_reference;

use crate::ids;
use crate::layout::design_decal_image_asset_record as decal_asset;
use crate::layout::design_decal_image_name_prefix as decal_name;
use crate::layout::design_decal_scope_prefix as decal_scope;
use crate::records::{
    decal::{DesignDecalAsset, DesignDecalImage},
    feature::scope::DesignParameterScope,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    },
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{
    DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureOperation,
};

const DECAL_TARGET_ROLE: crate::records::topology::extrude_selection::DesignOperandRole =
    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A;

/// Decode indexed Decal scope, asset, and name records as independent parts.
pub(crate) fn decode_decal_images(
    ctx: &DecodeContext<'_>,
    output: &mut cadmpeg_ir::source_fidelity::parts::PartOutput,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignDecalImage>, CodecError> {
    let mut images = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let source = scan
            .entry_view(&entry.name)
            .ok_or_else(|| CodecError::malformed("Decal stream has no source view"))?;
        let bytes = source.window();
        let frames = super::meta::design_primary_frames(bytes, &meta)?;
        let native_stream = ids::native_scope(&entry.name);
        let mut assets = std::collections::HashMap::new();
        for scope in scopes.iter().filter(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal
                && ids::native_stream(&scope.id) == Some(native_stream.as_str())
        }) {
            let Some(scope_at) = scope.byte_offset().and_then(|at| usize::try_from(at).ok()) else {
                continue;
            };
            let Some(scope_frame) = frames.iter().find(|frame| {
                frame.start == scope_at && frame.entity_id == u64::from(scope.record_index)
            }) else {
                continue;
            };
            let Some((asset_index, mapping, target)) =
                decal_part(ctx, output, source, &entry.name, scope_frame, || {
                    decal_scope_fields(
                        bytes
                            .get(scope_at..scope_frame.member_end)
                            .unwrap_or_default(),
                    )
                    .ok_or_else(|| CodecError::malformed("invalid Decal scope members"))
                })?
            else {
                continue;
            };
            if let std::collections::hash_map::Entry::Vacant(slot) = assets.entry(asset_index) {
                let asset =
                    decode_decal_asset(ctx, output, source, &entry.name, &frames, asset_index)?;
                slot.insert(asset);
            }
            let Some(asset) = assets.get(&asset_index).and_then(Option::as_ref).cloned() else {
                continue;
            };
            let image = DesignDecalImage::new(
                ids::native_design_decal_image_id(&entry.name, scope_at),
                crate::records::identity::Located {
                    value: scope.record_index,
                    offset: u64::try_from(scope_at).map_err(CodecError::malformed)?,
                },
                crate::records::decal::DesignDecalMappingMode::from_code(mapping),
                target,
                asset,
            )
            .map_err(CodecError::malformed)?;
            images.push(image);
        }
    }
    images.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(images)
}

fn decal_part<T>(
    ctx: &DecodeContext<'_>,
    output: &mut cadmpeg_ir::source_fidelity::parts::PartOutput,
    source: View<'_>,
    stream: &str,
    frame: &super::meta::DesignPrimaryFrame<'_>,
    parse: impl FnOnce() -> Result<T, CodecError>,
) -> Result<Option<T>, CodecError> {
    use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartSpec};
    let start = source
        .start()
        .checked_add(frame.start)
        .ok_or_else(|| CodecError::malformed("Decal part start overflows"))?;
    let end = source
        .start()
        .checked_add(frame.end)
        .ok_or_else(|| CodecError::malformed("Decal part end overflows"))?;
    let extent = source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("Decal part exceeds its stream"))?;
    let spec = PartSpec::new("f3d.bulk_record").holding([
        cadmpeg_ir::document::ArenaClass::Model,
        cadmpeg_ir::document::ArenaClass::Design,
    ]);
    let spec = if extent.location().space == cadmpeg_core::decode::SpaceId::ROOT {
        spec
    } else {
        spec.in_stream(
            cadmpeg_ir::StreamName::try_from(stream.to_owned()).map_err(CodecError::malformed)?,
        )
    };
    ctx.decode_part(output, extent, spec, |_| parse())
}

fn decal_scope_fields(prefix: &[u8]) -> Option<(u32, u8, u32)> {
    if prefix.get(decal_scope::ZERO_RUN_10..decal_scope::ASSET_REFERENCE)? != [0; 10]
        || prefix.get(decal_scope::ASSET_REFERENCE_ZERO_RUN..decal_scope::MAPPING_MODE)? != [0; 6]
        || prefix.get(decal_scope::TARGET_REFERENCE_ZERO_RUN..decal_scope::LEN)? != [0; 6]
    {
        return None;
    }
    Some((
        marked_reference(prefix, decal_scope::ASSET_REFERENCE)?,
        *prefix.get(decal_scope::MAPPING_MODE)?,
        marked_reference(prefix, decal_scope::TARGET_GROUP_REFERENCE)?,
    ))
}

fn decode_decal_asset(
    ctx: &DecodeContext<'_>,
    output: &mut cadmpeg_ir::source_fidelity::parts::PartOutput,
    source: View<'_>,
    stream: &str,
    frames: &[super::meta::DesignPrimaryFrame<'_>],
    asset_index: u32,
) -> Result<Option<DesignDecalAsset>, CodecError> {
    let Some(asset_frame) = frames
        .iter()
        .find(|frame| frame.entity_id == u64::from(asset_index))
    else {
        return Ok(None);
    };
    let bytes = source.window();
    let Some((asset_class, entity_suffix)) =
        decal_part(ctx, output, source, stream, asset_frame, || {
            let asset = bytes
                .get(asset_frame.start..asset_frame.end)
                .unwrap_or_default();
            decal_asset_fields(asset, asset_index)
                .ok_or_else(|| CodecError::malformed("invalid Decal asset members"))
        })?
    else {
        return Ok(None);
    };
    let Some(name_frame) = frames.iter().find(|frame| frame.start == asset_frame.end) else {
        return Ok(None);
    };
    decal_part(ctx, output, source, stream, name_frame, || {
        let name = bytes
            .get(name_frame.start..name_frame.member_end)
            .unwrap_or_default();
        let (name_class, name_index, asset_name) = decal_name_fields(name)
            .ok_or_else(|| CodecError::malformed("invalid Decal name members"))?;
        DesignDecalAsset::new(
            [asset_class, name_class],
            [asset_index, name_index],
            u64::try_from(asset_frame.start).map_err(CodecError::malformed)?,
            entity_suffix,
            asset_name,
        )
        .map_err(CodecError::malformed)
    })
}

fn decal_asset_fields(asset: &[u8], asset_index: u32) -> Option<(String, u32)> {
    if asset.len() != decal_asset::LEN {
        return None;
    }
    let (class_tag, after_tag) = lp_ascii_filtered_view(asset, 0, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(asset, after_tag)? != asset_index
        || asset.get(decal_asset::ZERO_RUN_8..decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE)?
            != [0; 8]
        || asset.get(decal_asset::ZERO_RUN_6..decal_asset::LEN)? != [0; 6]
    {
        return None;
    }
    Some((
        class_tag.to_owned(),
        marked_reference(asset, decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE)?,
    ))
}

fn decal_name_fields(name: &[u8]) -> Option<(String, u32, String)> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(name, 0, 0..=2000, u8::is_ascii_graphic)?;
    let index = View::u32_le_at(name, after_tag)?;
    if name.get(decal_name::ZERO_RUN_10..decal_name::ASSET_NAME_CODE_UNIT_COUNT)? != [0; 10] {
        return None;
    }
    let (asset_name, end) =
        lp_utf16_bounded(name, decal_name::ASSET_NAME_CODE_UNIT_COUNT, 1..=1024)?;
    (end == name.len()).then(|| (class_tag.to_owned(), index, asset_name))
}

/// Project exact Decal image and face bindings into neutral features.
pub(crate) fn project_decal_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignDecalImage],
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignBodyRecipeOperand],
    features: &mut [Feature],
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    for image in images {
        let Some(mut asset) = embedded_image_asset(ctx, scan, image.asset.name())? else {
            continue;
        };
        asset.native_ref = Some(image.id.clone());
        let asset_id = asset.id.clone();
        assets.push(asset);
        if image.mapping_mode != crate::records::decal::DesignDecalMappingMode::FitToFaces {
            continue;
        }
        let native_stream = ids::native_stream(&image.id);
        let Some(scope) = scopes.iter().find(|scope| {
            scope.record_index == image.scope_record_index()
                && ids::native_stream(&scope.id) == native_stream
        }) else {
            continue;
        };
        let Some(group) = groups.iter().find(|group| {
            group.scope_record_index == scope.record_index
                && group.record_index == image.target_group_record_index
                && group.role() == DECAL_TARGET_ROLE
                && group.members().len() == 1
                && ids::native_stream(&group.id) == native_stream
        }) else {
            continue;
        };
        let Some(operand) = operands.iter().find(|operand| {
            operand.scope_record_index == scope.record_index
                && operand.owner.group() == Some((group.record_index, 0))
                && operand.record_index() == group.members()[0].value
                && ids::native_stream(&operand.id) == native_stream
        }) else {
            continue;
        };
        if operand
            .references()
            .iter()
            .any(|reference| reference.candidate_faces.is_empty())
        {
            continue;
        }
        let mut faces: Vec<_> = operand
            .references()
            .iter()
            .flat_map(|reference| reference.candidate_faces.iter())
            .cloned()
            .collect();
        faces.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        faces.dedup();
        if faces.is_empty() {
            continue;
        }
        let feature_id = crate::ids::neutral_feature_id(scope);
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: asset_id,
                faces: FaceSelection::Resolved {
                    faces,
                    native: operand.id.clone(),
                },
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            }));
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    assets.dedup_by(|a, b| a.id == b.id);
    Ok(assets)
}

/// Parse one Decal image from its indexed scope and asset records.
#[cfg(test)]
fn parse_decal_image_frame(
    stream: &str,
    scope_record_index: u32,
    scope_at: usize,
    read: &super::image::ImageRead<'_, '_>,
    frames: &[super::meta::DesignPrimaryFrame<'_>],
) -> Result<Option<DesignDecalImage>, CodecError> {
    let parsed = (|| {
        let prefix = read(scope_at..scope_at.checked_add(decal_scope::LEN)?)?;
        if prefix.get(decal_scope::ZERO_RUN_10..decal_scope::ASSET_REFERENCE)? != [0; 10] {
            return None;
        }
        let asset_record_index = marked_reference(prefix, decal_scope::ASSET_REFERENCE)?;
        if prefix.get(decal_scope::ASSET_REFERENCE_ZERO_RUN..decal_scope::MAPPING_MODE)? != [0; 6] {
            return None;
        }
        let mapping_mode = *prefix.get(decal_scope::MAPPING_MODE)?;
        let target_group_record_index =
            marked_reference(prefix, decal_scope::TARGET_GROUP_REFERENCE)?;
        if prefix.get(decal_scope::TARGET_REFERENCE_ZERO_RUN..decal_scope::LEN)? != [0; 6] {
            return None;
        }

        let scope_frame = frames.iter().find(|frame| {
            frame.start == scope_at && frame.entity_id == u64::from(scope_record_index)
        })?;
        if scope_at.checked_add(decal_scope::LEN)? > scope_frame.member_end {
            return None;
        }
        let asset_frame = frames
            .iter()
            .find(|frame| frame.entity_id == u64::from(asset_record_index))?;
        let name_frame = frames.iter().find(|frame| frame.start == asset_frame.end)?;
        let asset_record =
            match parse_decal_asset_record(asset_frame, name_frame, asset_record_index, read) {
                Ok(Some(candidate)) => candidate,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        DesignDecalImage::new(
            ids::native_design_decal_image_id(stream, scope_at),
            crate::records::identity::Located {
                value: scope_record_index,
                offset: u64::try_from(scope_at).ok()?,
            },
            crate::records::decal::DesignDecalMappingMode::from_code(mapping_mode),
            target_group_record_index,
            asset_record,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

/// Parse the indexed asset and image-name records.
#[cfg(test)]
fn parse_decal_asset_record(
    asset_frame: &super::meta::DesignPrimaryFrame<'_>,
    name_frame: &super::meta::DesignPrimaryFrame<'_>,
    asset_record_index: u32,
    read: &super::image::ImageRead<'_, '_>,
) -> Result<Option<DesignDecalAsset>, CodecError> {
    let parsed = (|| {
        let asset_at = asset_frame.start;
        let name_at = name_frame.start;
        if name_at != asset_at + decal_asset::LEN {
            return None;
        }
        let asset = read(asset_at..name_at)?;
        let (asset_class_tag, after_asset_tag) =
            lp_ascii_filtered_view(asset, 0, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(asset, after_asset_tag)? != asset_record_index
            || asset.get(decal_asset::ZERO_RUN_8..decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE)?
                != [0; 8]
        {
            return None;
        }
        let asset_entity_suffix =
            marked_reference(asset, decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE)?;
        if asset.get(decal_asset::ZERO_RUN_6..decal_asset::LEN)? != [0; 6] {
            return None;
        }
        let name_prefix = read(name_at..name_at + decal_name::LEN)?;
        let (name_class_tag, after_name_tag) =
            lp_ascii_filtered_view(name_prefix, 0, 0..=2000, u8::is_ascii_graphic)?;
        let name_record_index = View::u32_le_at(name_prefix, after_name_tag)?;
        if name_prefix.get(decal_name::ZERO_RUN_10..decal_name::ASSET_NAME_CODE_UNIT_COUNT)?
            != [0; 10]
        {
            return None;
        }
        let units = usize::try_from(View::u32_le_at(
            name_prefix,
            decal_name::ASSET_NAME_CODE_UNIT_COUNT,
        )?)
        .ok()?;
        if !(1..=1024).contains(&units) {
            return None;
        }
        let name_end = name_at
            .checked_add(decal_name::LEN)?
            .checked_add(units.checked_mul(2)?)?;
        let next_at = name_frame.member_end;
        if name_end != next_at {
            return None;
        }
        let name = read(name_at..name_end)?;
        let (asset_name, after_asset_name) =
            lp_utf16_bounded(name, decal_name::ASSET_NAME_CODE_UNIT_COUNT, 1..=1024)?;
        if after_asset_name != name.len() {
            return None;
        }
        let asset_class_tag = asset_class_tag.to_owned();
        let name_class_tag = name_class_tag.to_owned();

        DesignDecalAsset::new(
            [asset_class_tag, name_class_tag],
            [asset_record_index, name_record_index],
            u64::try_from(asset_at).ok()?,
            asset_entity_suffix,
            asset_name,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

#[cfg(test)]
mod tests {
    use super::parse_decal_image_frame;
    use crate::test_support::decal::{fixture, header};
    use crate::test_support::write_marked_reference;

    #[test]
    fn decal_frame_decodes_image_mode_and_target() {
        let (bytes, scope_at, meta) = fixture();
        let frames = crate::design::decode::meta::design_primary_frames(&bytes, &meta).unwrap();
        let image = parse_decal_image_frame(
            "Design/BulkStream.dat",
            23,
            scope_at,
            &|range| bytes.get(range),
            &frames,
        )
        .unwrap()
        .expect("complete synthetic Decal frame");
        assert_eq!(image.asset.record_index(), 17);
        assert_eq!(image.asset.entity_suffix(), 50);
        assert_eq!(image.asset.name(), "mark.png");
        assert_eq!(
            image.mapping_mode,
            crate::records::decal::DesignDecalMappingMode::FitToFaces
        );
        assert_eq!(image.target_group_record_index, 24);
        assert_eq!(
            crate::records::decal::DesignDecalAsset::primary_frame_length(),
            30
        );
        assert_eq!(image.asset.name_frame_length(), 41);
    }

    #[test]
    fn decal_frame_rejects_an_unframed_name() {
        let (mut bytes, scope_at, meta) = fixture();
        bytes[scope_at..scope_at + 4].fill(0);
        let frames = crate::design::decode::meta::design_primary_frames(&bytes, &meta).unwrap();
        assert!(parse_decal_image_frame(
            "Design/BulkStream.dat",
            23,
            scope_at,
            &|range| { bytes.get(range) },
            &frames
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn damaged_decal_members_give_up_the_indexed_record() {
        use std::io::{Cursor, Write};

        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let (bytes, scope_at, _) = fixture();
        let meta =
            crate::test_support::zip_test::indexed_fixture_metastream(&bytes, &[17, 18, 23], &[23]);
        let scope =
            crate::records::feature::test_support::located_scope_from_fixture(serde_json::json!({
                "id": format!("{}:scope#23", crate::ids::native_scope(ENTRY)),
                "byte_offset": scope_at,
                "class_tag": "301",
                "record_index": 23,
                "frame_length": 129,
                "kind": "Decal",
                "kind_offset": scope_at + 32,
                "feature_ordinal": 1,
                "feature_ordinal_offset": 128,
                "reference_count_offset": scope_at + 9,
                "reference_members": [23],
                "reference_member_offsets": [scope_at + 14],
                "paired_class_tag": "302",
                "paired_byte_offset": 200
            }));
        for (damaged_at, expected_start, expected_len) in
            [(11, 0, 30), (41, 30, 41), (scope_at + 11, scope_at, 169)]
        {
            let mut damaged = bytes.clone();
            damaged[damaged_at] = 2;
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
            crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
            zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
                .unwrap();
            zip.write_all(&meta).unwrap();
            zip.start_file(ENTRY, stored).unwrap();
            zip.write_all(&damaged).unwrap();
            let archive = zip.finish().unwrap().into_inner();
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let ctx = cadmpeg_test_support::service_decode_context();
                let mut output = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                    cadmpeg_ir::CodecFormat::F3d,
                    &ctx,
                );
                assert!(super::decode_decal_images(
                    &ctx,
                    &mut output,
                    scan,
                    std::slice::from_ref(&scope)
                )
                .unwrap()
                .is_empty());
                let (_, native, fidelity, losses) = output.into_parts();
                assert_eq!(losses.len(), 1);
                assert_eq!(
                    losses[0].code,
                    cadmpeg_ir::codec::DecodeLossCode::RecordUnreadable
                        .note(String::new())
                        .code
                );
                assert!(losses[0].message.contains("f3d.bulk_record"));
                assert!(losses[0].message.contains("Design1/BulkStream.dat"));
                assert!(fidelity
                    .unrepresented()
                    .contains(&cadmpeg_ir::document::ArenaClass::Model));
                assert!(fidelity
                    .unrepresented()
                    .contains(&cadmpeg_ir::document::ArenaClass::Design));
                let unknowns = &native.namespace("f3d").unwrap().arenas()["unknowns"];
                assert_eq!(unknowns.len(), 1);
                let retained = fidelity.retained_records().values().next().unwrap();
                assert_eq!(retained.offset(), u64::try_from(expected_start).unwrap());
                assert_eq!(retained.byte_len(), expected_len);
            });
        }
    }

    #[test]
    fn damaged_decal_asset_keeps_an_independent_sibling() {
        use std::io::{Cursor, Write};

        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let (mut bytes, scope_at, _) = fixture();
        let sibling_at = bytes.len();
        bytes.extend_from_within(..);
        header(&mut bytes, sibling_at, *b"258", 27);
        header(&mut bytes, sibling_at + 30, *b"279", 28);
        header(&mut bytes, sibling_at + scope_at, *b"301", 33);
        header(&mut bytes, sibling_at + 200, *b"302", 33);
        write_marked_reference(&mut bytes, sibling_at + scope_at + 21, 27);
        let meta = crate::test_support::zip_test::indexed_fixture_metastream(
            &bytes,
            &[17, 18, 23, 27, 28, 33],
            &[23, 33],
        );
        bytes[11] = 2;
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
            .unwrap();
        zip.write_all(&meta).unwrap();
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(&bytes).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let scope = |at: usize, index: u32| {
            crate::records::feature::test_support::located_scope_from_fixture(serde_json::json!({
                "id": format!("{}:scope#{index}", crate::ids::native_scope(ENTRY)),
                "byte_offset": at,
                "class_tag": "301",
                "record_index": index,
                "frame_length": 129,
                "kind": "Decal",
                "kind_offset": at + 32,
                "feature_ordinal": 1,
                "feature_ordinal_offset": at + 57,
                "reference_count_offset": at + 9,
                "reference_members": [index],
                "reference_member_offsets": [at + 14],
                "paired_class_tag": "302",
                "paired_byte_offset": at + 129
            }))
        };
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let ctx = cadmpeg_test_support::service_decode_context();
            let mut output = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                &ctx,
            );
            let scopes = [scope(scope_at, 23), scope(sibling_at + scope_at, 33)];
            let images = super::decode_decal_images(&ctx, &mut output, scan, &scopes).unwrap();
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].scope_record_index(), 33);
            assert_eq!(images[0].asset.record_index(), 27);
            let (_, _, fidelity, losses) = output.into_parts();
            assert_eq!(losses.len(), 1);
            assert_eq!(fidelity.retained_records().len(), 1);
        });
    }

    #[test]
    fn decal_projection_emits_one_asset() {
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
        };
        use std::collections::BTreeMap;
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const STREAM: &str = "Design/BulkStream.dat";
        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        let (bytes, scope_at, meta) = fixture();
        let frames = crate::design::decode::meta::design_primary_frames(&bytes, &meta).unwrap();
        let image =
            parse_decal_image_frame(STREAM, 23, scope_at, &|range| bytes.get(range), &frames)
                .unwrap()
                .unwrap();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#23",
            crate::records::feature::scope::DesignFeatureKind::Decal,
            23,
        );
        let group = crate::records::topology::construction::DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: "f3d:Design/BulkStream.dat:operand-group#24".into(),
                scope_record_index: 23,
                scope_reference_ordinal: 0,
                record_index: 24,
                byte_offset: 900,
                class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
                members: vec![crate::records::identity::Located { value: 100, offset: 926 }],
                lost_edge_references: Vec::new(),
                frame: crate::records::topology::construction::DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 921,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: vec![crate::records::identity::Located { value: 200, offset: 943 }],
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: 971,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: 975,
                        variant: false,
                        closing_flag: false,
                    },
                ).unwrap(),
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
                ),
                role_offset: 953,
                paired_class_tag: crate::records::references::DesignClassTag::try_from("265".to_owned()).unwrap(),
                paired_byte_offset: 1024,
            },
        ).unwrap();
        let face = cadmpeg_ir::ids::FaceId::mint("test:model:face#1").unwrap();
        let second_face = cadmpeg_ir::ids::FaceId::mint("test:model:face#2").unwrap();
        let operand = crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: "f3d:Design/BulkStream.dat:body-operand#100".into(),
                scope_record_index: 23,
                owner: crate::records::topology::body_recipe::DesignOperandOwner::Group {
                    group_record_index: 24,
                    group_member_ordinal: 0,
                },
                record_index: 100,
                byte_offset: 1000,
                class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 1068,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "11111111-2222-4333-8444-555555555555".to_owned(),
                )
                .unwrap(),
                context_id_offset: 1102,
                selector_tail: None,
                references: vec![
                    crate::records::topology::body_recipe::DesignBodyRecipeReference {
                        design_reference: 1,
                        design_reference_offset: Some(1025),
                        form: 1,
                        form_offset: Some(1033),
                        candidate_faces: vec![face.clone()],
                        preceding_candidate_faces: Vec::new(),
                        preceding_body_slots: Vec::new(),
                    },
                    crate::records::topology::body_recipe::DesignBodyRecipeReference {
                        design_reference: 2,
                        design_reference_offset: Some(1037),
                        form: 1,
                        form_offset: Some(1045),
                        candidate_faces: vec![second_face.clone()],
                        preceding_candidate_faces: Vec::new(),
                        preceding_body_slots: Vec::new(),
                    },
                ],
                nested_record_index: 103,
                nested_record_index_offset: 1050,
                recipe_id: "f3d:Design/BulkStream.dat:recipe#1".into(),
                resolved_face_slot: None,
                resolved_body_state_id: None,
                resolved_body_slot: None,
                resolved_body_face_slots: Vec::new(),
                next_record_index: 104,
                next_byte_offset: 1200,
            },
        )
        .unwrap();
        let feature = || Feature {
            product_definition: None,
            needs_recompute: false,
            id: crate::ids::neutral_feature_id(&scope),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Unresolved,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut features = [feature()];
            let assets = super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                std::slice::from_ref(&group),
                std::slice::from_ref(&operand),
                &mut features,
            )
            .unwrap();
            assert_eq!(assets.len(), 1);
            assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
            assert!(matches!(features[0].definition(),
                FeatureDefinition::Operation(FeatureOperation::Decal {
                    asset, faces: cadmpeg_ir::features::FaceSelection::Resolved { faces, native }, ..
                }) if *asset == assets[0].id && faces.as_slice() == [face.clone(), second_face.clone()] && native == &operand.id));
            let mut partial_operand = operand.clone();
            partial_operand
                .reference_bindings_mut()
                .nth(1)
                .unwrap()
                .candidate_faces
                .clear();
            let mut partial_feature = [feature()];
            let partial_assets = super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                std::slice::from_ref(&group),
                std::slice::from_ref(&partial_operand),
                &mut partial_feature,
            )
            .unwrap();
            assert_eq!(partial_assets.len(), 1);
            assert!(matches!(
                partial_feature[0].definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            ));
            let unbound = super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                &[],
                &[],
                &mut [feature()],
            )
            .unwrap();
            assert_eq!(unbound.len(), 1);
            assert_eq!(unbound[0].id, assets[0].id);
        });
    }
}
