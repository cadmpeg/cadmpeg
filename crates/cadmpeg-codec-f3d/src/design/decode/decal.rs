// SPDX-License-Identifier: Apache-2.0
//! Parse exact raster and face bindings owned by Design `Decal` scopes.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::ids;
use crate::layout::design_decal_image_asset_record as decal_asset;
use crate::layout::design_decal_image_name_prefix as decal_name;
use crate::layout::design_decal_scope_prefix as decal_scope;
use crate::records::{
    DesignBodyRecipeOperand, DesignConstructionOperandGroup, DesignDecalImage, DesignParameterScope,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{DecalMapping, FaceSelection, Feature, FeatureDefinition};

const DECAL_TARGET_ROLE: u64 = 0x0000_0004_0000_0000;

struct DecalAssetRecord {
    asset_class_tag: String,
    asset_at: usize,
    asset_entity_suffix: u32,
    asset_entity_reference_at: usize,
    name_class_tag: String,
    name_record_index: u32,
    name_at: usize,
    next_at: usize,
    asset_name: String,
}

/// Decode every structurally complete Decal image record.
pub fn decode_decal_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignDecalImage>, CodecError> {
    let mut images = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        images.extend(
            scopes
                .iter()
                .filter(|scope| {
                    scope.kind == crate::records::DesignFeatureKind::Decal
                        && ids::native_stream(&scope.id) == Some(stream.as_str())
                })
                .filter_map(|scope| parse_decal_image(bytes, &entry.name, scope)),
        );
    }
    images.sort_by(|a, b| a.id.cmp(&b.id));
    images.dedup_by(|a, b| a.id == b.id);
    Ok(images)
}

/// Project exact Decal image and face bindings into neutral features.
pub fn project_decal_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignDecalImage],
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignBodyRecipeOperand],
    features: &mut [Feature],
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    for image in images {
        if image.mapping_mode != crate::records::DesignDecalMappingMode::FitToFaces {
            continue;
        }
        let native_stream = ids::native_stream(&image.id);
        let Some(scope) = scopes.iter().find(|scope| {
            scope.record_index == image.scope_record_index
                && ids::native_stream(&scope.id) == native_stream
        }) else {
            continue;
        };
        let Some(group) = groups.iter().find(|group| {
            group.scope_record_index == scope.record_index
                && group.record_index == image.target_group_record_index
                && group.role == DECAL_TARGET_ROLE
                && group.members.len() == 1
                && ids::native_stream(&group.id) == native_stream
        }) else {
            continue;
        };
        let Some(operand) = operands.iter().find(|operand| {
            operand.scope_record_index == scope.record_index
                && operand.owner.group() == Some((group.record_index, 0))
                && operand.record_index == group.members[0]
                && ids::native_stream(&operand.id) == native_stream
        }) else {
            continue;
        };
        let mut faces = operand
            .references
            .iter()
            .flat_map(|reference| reference.candidate_faces.iter().cloned())
            .collect::<Vec<_>>();
        faces.sort_by(|a, b| a.0.cmp(&b.0));
        faces.dedup();
        if faces.is_empty() {
            continue;
        }
        let Some(asset) = embedded_image_asset(scan, &image.asset_name)? else {
            continue;
        };
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.id == ids::neutral_feature_id(scope))
        else {
            continue;
        };
        feature.definition = FeatureDefinition::Decal {
            asset: asset.id.clone(),
            faces: FaceSelection::Resolved {
                faces,
                native: operand.id.clone(),
            },
            mapping: DecalMapping::FitToFaces,
            opacity: None,
        };
        assets.push(asset);
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    assets.dedup_by(|a, b| a.id == b.id);
    Ok(assets)
}

fn parse_decal_image(
    bytes: &[u8],
    stream: &str,
    scope: &DesignParameterScope,
) -> Option<DesignDecalImage> {
    parse_decal_image_frame(
        bytes,
        stream,
        scope.record_index,
        usize::try_from(scope.byte_offset).ok()?,
    )
}

fn parse_decal_image_frame(
    bytes: &[u8],
    stream: &str,
    scope_record_index: u32,
    scope_at: usize,
) -> Option<DesignDecalImage> {
    if bytes.get(scope_at + decal_scope::ZERO_RUN_10..scope_at + decal_scope::ASSET_REFERENCE)?
        != [0; 10]
    {
        return None;
    }
    let asset_reference_at = scope_at + decal_scope::ASSET_REFERENCE;
    let asset_record_index = marked_reference(bytes, asset_reference_at)?;
    if bytes.get(
        scope_at + decal_scope::ASSET_REFERENCE_ZERO_RUN..scope_at + decal_scope::MAPPING_MODE,
    )? != [0; 6]
    {
        return None;
    }
    let mapping_mode_at = scope_at + decal_scope::MAPPING_MODE;
    let mapping_mode = *bytes.get(mapping_mode_at)?;
    let target_group_reference_at = scope_at + decal_scope::TARGET_GROUP_REFERENCE;
    let target_group_record_index = marked_reference(bytes, target_group_reference_at)?;
    if bytes.get(scope_at + decal_scope::TARGET_REFERENCE_ZERO_RUN..scope_at + decal_scope::LEN)?
        != [0; 6]
    {
        return None;
    }

    let mut position = 0;
    let mut asset_record = None;
    while let Some(asset_at) = next_indexed_record_offset(bytes, position) {
        position = asset_at.checked_add(1)?;
        if View::u32_le_at(bytes, asset_at + 7) != Some(asset_record_index) {
            continue;
        }
        let Some(candidate) = parse_decal_asset_record(bytes, asset_at, asset_record_index) else {
            continue;
        };
        if asset_record.replace(candidate).is_some() {
            return None;
        }
    }
    let DecalAssetRecord {
        asset_class_tag,
        asset_at,
        asset_entity_suffix,
        asset_entity_reference_at,
        name_class_tag,
        name_record_index,
        name_at,
        next_at,
        asset_name,
    } = asset_record?;

    Some(DesignDecalImage {
        id: ids::native_design_decal_image_id(stream, scope_at),
        scope_record_index,
        asset_reference_offset: u64::try_from(asset_reference_at + 1).ok()?,
        mapping_mode: crate::records::DesignDecalMappingMode::from_code(mapping_mode),
        mapping_mode_offset: u64::try_from(mapping_mode_at).ok()?,
        target_group_record_index,
        target_group_reference_offset: u64::try_from(target_group_reference_at + 1).ok()?,
        asset_class_tag,
        asset_record_index,
        asset_byte_offset: u64::try_from(asset_at).ok()?,
        asset_frame_length: u64::try_from(name_at.checked_sub(asset_at)?).ok()?,
        asset_entity_suffix,
        asset_entity_reference_offset: u64::try_from(asset_entity_reference_at + 1).ok()?,
        name_class_tag,
        name_record_index,
        name_byte_offset: u64::try_from(name_at).ok()?,
        name_frame_length: u64::try_from(next_at.checked_sub(name_at)?).ok()?,
        asset_name,
        asset_name_offset: u64::try_from(name_at + decal_name::LEN).ok()?,
    })
}

fn parse_decal_asset_record(
    bytes: &[u8],
    asset_at: usize,
    asset_record_index: u32,
) -> Option<DecalAssetRecord> {
    let (asset_class_tag, after_asset_tag) =
        lp_ascii_filtered(bytes, asset_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_asset_tag)? != asset_record_index
        || bytes.get(
            asset_at + decal_asset::ZERO_RUN_8
                ..asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE,
        )? != [0; 8]
    {
        return None;
    }
    let asset_entity_reference_at = asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE;
    let asset_entity_suffix = marked_reference(bytes, asset_entity_reference_at)?;
    if bytes.get(asset_at + decal_asset::ZERO_RUN_6..asset_at + decal_asset::LEN)? != [0; 6] {
        return None;
    }
    let name_at = next_indexed_record_offset(bytes, asset_at + decal_asset::ZERO_RUN_8)?;
    if name_at != asset_at + decal_asset::LEN {
        return None;
    }
    let (name_class_tag, after_name_tag) =
        lp_ascii_filtered(bytes, name_at, 0..=2000, u8::is_ascii_graphic)?;
    let name_record_index = View::u32_le_at(bytes, after_name_tag)?;
    if name_record_index != asset_record_index.checked_add(1)?
        || bytes.get(
            name_at + decal_name::ZERO_RUN_10..name_at + decal_name::ASSET_NAME_CODE_UNIT_COUNT,
        )? != [0; 10]
    {
        return None;
    }
    let (asset_name, after_asset_name) = lp_utf16_bounded(
        bytes,
        name_at + decal_name::ASSET_NAME_CODE_UNIT_COUNT,
        1..=1024,
    )?;
    let next_at = next_indexed_record_offset(bytes, name_at + decal_name::ZERO_RUN_10)?;
    if after_asset_name != next_at {
        return None;
    }

    Some(DecalAssetRecord {
        asset_class_tag,
        asset_at,
        asset_entity_suffix,
        asset_entity_reference_at,
        name_class_tag,
        name_record_index,
        name_at,
        next_at,
        asset_name,
    })
}

fn marked_reference(bytes: &[u8], at: usize) -> Option<u32> {
    (bytes.get(at) == Some(&1)).then(|| View::u32_le_at(bytes, at + 1))?
}

#[cfg(test)]
mod tests {
    use super::parse_decal_image_frame;
    use crate::records::DesignDecalMappingMode;

    fn header(bytes: &mut [u8], at: usize, tag: [u8; 3], index: u32) {
        bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&tag);
        bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
    }

    fn marked(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture() -> (Vec<u8>, usize) {
        let mut bytes = vec![0; 240];
        let asset_at = 0;
        let name_at = 30;
        let scope_at = 71;
        let end_at = 200;
        header(&mut bytes, asset_at, *b"258", 17);
        marked(&mut bytes, asset_at + 19, 50);
        header(&mut bytes, name_at, *b"279", 18);
        let name = "mark.png".encode_utf16().collect::<Vec<_>>();
        bytes[name_at + 21..name_at + 25]
            .copy_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        for (ordinal, unit) in name.into_iter().enumerate() {
            let at = name_at + 25 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        header(&mut bytes, scope_at, *b"301", 23);
        marked(&mut bytes, scope_at + 21, 17);
        bytes[scope_at + 32] = DesignDecalMappingMode::FitToFaces.code();
        marked(&mut bytes, scope_at + 33, 24);
        header(&mut bytes, 150, *b"440", 17);
        header(&mut bytes, end_at, *b"302", 23);
        (bytes, scope_at)
    }

    #[test]
    fn decal_frame_decodes_image_mode_and_target() {
        let (bytes, scope_at) = fixture();
        let image = parse_decal_image_frame(&bytes, "Design/BulkStream.dat", 23, scope_at)
            .expect("complete synthetic Decal frame");
        assert_eq!(image.asset_record_index, 17);
        assert_eq!(image.asset_entity_suffix, 50);
        assert_eq!(image.asset_name, "mark.png");
        assert_eq!(
            image.mapping_mode,
            crate::records::DesignDecalMappingMode::FitToFaces
        );
        assert_eq!(image.target_group_record_index, 24);
        assert_eq!(image.asset_frame_length, 30);
        assert_eq!(image.name_frame_length, 41);
    }

    #[test]
    fn decal_frame_rejects_an_unframed_name() {
        let (mut bytes, scope_at) = fixture();
        bytes[scope_at..scope_at + 4].fill(0);
        assert!(parse_decal_image_frame(&bytes, "Design/BulkStream.dat", 23, scope_at).is_none());
    }
}
