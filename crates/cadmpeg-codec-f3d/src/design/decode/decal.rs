// SPDX-License-Identifier: Apache-2.0
//! Parse exact raster and face bindings owned by Design `Decal` scopes.

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::lp_utf16_bounded;
use crate::bytes::typed_local_reference;
use crate::container::ContainerScan;
use crate::design::decode::image::{embedded_image_asset, image_media_type};
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::next_indexed_record_offset;

use crate::ids;
use crate::layout::design_decal_image_asset_record as decal_asset;
use crate::layout::design_decal_image_name_prefix as decal_name;
use crate::layout::design_decal_scope_extended_prefix as decal_extended_scope;
use crate::loss::F3dLossCode;
use crate::records::{
    decal::{DesignDecalAsset, DesignDecalImage, DesignDecalMappingMode, DesignDecalScopeLayout},
    feature::scope::DesignParameterScope,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    },
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};
use cadmpeg_ir::features::{
    DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureOperation,
};

const TYPED_ASSET_REFERENCE_TYPE: &str = "9A9B75AF-D286-4548-9301-28910E24EA20";
const TYPED_TARGET_GROUP_REFERENCE_TYPE: &str = "2CA5A1CD-C99B-4C9B-91AF-57989148E841";
const TYPED_ASSET_ENTITY_TYPE: &str = "878C70A7-15B8-4B0B-B1B7-C2A7A23CC45C";

const DECAL_TARGET_ROLE: crate::records::topology::extrude_selection::DesignOperandRole =
    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A;

/// Decode every structurally complete Decal image record.
pub(crate) fn decode_decal_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignDecalImage>, CodecError> {
    super::image::decode_scoped_images(
        scan,
        scopes,
        &crate::records::feature::scope::DesignFeatureKind::Decal,
        parse_decal_image,
        |image| image.id.as_str(),
    )
}

pub(crate) fn project_decal_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignDecalImage],
    targets: DecalTargets<'_>,
    features: &mut [Feature],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    let mut assumed_mappings = 0usize;
    let mut unresolved_targets = 0usize;
    let mut unresolved_images = 0usize;
    let mut unresolved_definitions = 0usize;
    for scope in scopes
        .iter()
        .filter(|scope| scope.kind() == crate::records::feature::scope::DesignFeatureKind::Decal)
    {
        let image = images.iter().find(|image| {
            scope.record_index == image.scope_record_index()
                && ids::native_stream(&scope.id) == ids::native_stream(&image.id)
        });
        let feature_id = crate::ids::neutral_feature_id(scope);
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        let (asset, faces) = if let Some(image) = image {
            let asset = match embedded_image_asset(ctx, scan, image.asset.name())? {
                Some(asset) => asset,
                None => {
                    unresolved_images += 1;
                    unresolved_image_asset(Some(image.asset.name()), &image.id)?
                }
            };
            let faces = targets.faces(scope, image).unwrap_or_else(|| {
                unresolved_targets += 1;
                FaceSelection::Native(scope.id.clone())
            });
            assumed_mappings +=
                usize::from(image.mapping_mode != DesignDecalMappingMode::FitToFaces);
            (asset, faces)
        } else {
            unresolved_definitions += 1;
            unresolved_targets += 1;
            assumed_mappings += 1;
            (
                unresolved_image_asset(None, &scope.id)?,
                FaceSelection::Native(scope.id.clone()),
            )
        };
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: asset.id.clone(),
                faces,
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            }));
        assets.push(asset);
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    assets.dedup_by(|a, b| a.id == b.id);
    if assumed_mappings != 0 {
        losses.push(F3dLossCode::DecalMappingAssumed.note(format!(
            "{assumed_mappings} Decal feature(s) have a missing mapping mode or a mode other than 0x60; the decoder assumes the fit-to-faces mapping (DR-481)."
        )));
    }
    if unresolved_targets != 0 {
        losses.push(F3dLossCode::DecalTargetUnresolved.note(format!(
            "{unresolved_targets} Decal feature(s) keep an unresolved face selection because the target group or its members have no complete whole-body recipe faces (DR-255)."
        )));
    }
    if unresolved_images != 0 {
        losses.push(F3dLossCode::DecalImageUnresolved.note(format!(
            "{unresolved_images} Decal feature(s) name an image basename with no unique readable archive entry; the decoder retains an external source identity for that image."
        )));
    }
    if unresolved_definitions != 0 {
        losses.push(F3dLossCode::DecalDefinitionUnresolved.note(format!(
            "{unresolved_definitions} Decal feature(s) have no decoded image definition; the decoder retains the scope identity as the external image source (DR-482)."
        )));
    }
    Ok(assets)
}

fn unresolved_image_asset(name: Option<&str>, native: &str) -> Result<Asset, CodecError> {
    Asset::try_new(
        ids::neutral_asset_id(native),
        name.map(str::to_owned),
        name.and_then(image_media_type),
        AssetContent::External {
            uri: cadmpeg_core::text::NonBlankString::new(native).ok_or_else(|| {
                CodecError::Malformed("Decal source identity must be nonempty".into())
            })?,
        },
        Some(native.to_owned()),
    )
    .map_err(CodecError::Malformed)
}

#[derive(Clone, Copy)]
pub(crate) struct DecalTargets<'a> {
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a [DesignBodyRecipeOperand],
}

impl DecalTargets<'_> {
    pub(crate) fn faces(
        self,
        scope: &DesignParameterScope,
        image: &DesignDecalImage,
    ) -> Option<FaceSelection> {
        let native_stream = ids::native_stream(&image.id);
        let group = self.groups.iter().find(|group| {
            group.scope_record_index == scope.record_index
                && group.record_index == image.target_group_record_index
                && group.role() == DECAL_TARGET_ROLE
                && ids::native_stream(&group.id) == native_stream
        })?;
        let mut faces = Vec::new();
        let mut native = None;
        for (ordinal, member) in group.members().iter().enumerate() {
            let ordinal = u32::try_from(ordinal).ok()?;
            let operand = self.operands.iter().find(|operand| {
                operand.scope_record_index == scope.record_index
                    && operand.owner.group() == Some((group.record_index, ordinal))
                    && operand.record_index() == member.value
                    && ids::native_stream(&operand.id) == native_stream
            })?;
            let resolved = faces.len();
            faces.extend(
                operand
                    .references()
                    .iter()
                    .flat_map(|reference| reference.candidate_faces.iter())
                    .cloned(),
            );
            if faces.len() == resolved {
                return None;
            }
            native.get_or_insert(&operand.id);
        }
        let native = if group.members().len() == 1 {
            native?.clone()
        } else {
            native?;
            group.id.clone()
        };
        faces.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        faces.dedup();
        Some(FaceSelection::Resolved { faces, native })
    }
}

fn parse_decal_image(
    bytes: &[u8],
    stream: &str,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDecalImage>, CodecError> {
    let Ok(scope_at) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    parse_decal_image_frame(bytes, stream, scope.record_index, scope_at)
}

fn decal_reference(bytes: &[u8], at: usize, typed: bool, type_guid: &str) -> Option<u32> {
    if typed {
        let (target, guid) = typed_local_reference(bytes, at)?;
        guid.eq_ignore_ascii_case(type_guid).then_some(target)
    } else {
        marked_record_reference(bytes, at)
    }
}

struct DecalScopePrefix {
    layout: DesignDecalScopeLayout,
    asset_record_index: u32,
    mapping_mode: u8,
    target_group_record_index: u32,
}

fn parse_decal_scope_prefix(
    bytes: &[u8],
    scope_at: usize,
    layout: DesignDecalScopeLayout,
) -> Option<DecalScopePrefix> {
    let header = bytes.get(scope_at + 11..scope_at + layout.asset_reference())?;
    if layout.extended() {
        let code_at = decal_extended_scope::EXTRA_CODE - 11;
        if header.get(..code_at)?.iter().any(|byte| *byte != 0)
            || View::u32_le_at(header, code_at)? != decal_extended_scope::EXTRA_CODE_VALUE
            || *header.get(decal_extended_scope::ZERO - 11)? != 0
        {
            return None;
        }
    } else if header.iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(DecalScopePrefix {
        layout,
        asset_record_index: decal_reference(
            bytes,
            scope_at + layout.asset_reference(),
            layout.typed(),
            TYPED_ASSET_REFERENCE_TYPE,
        )?,
        mapping_mode: *bytes.get(scope_at + layout.mapping_mode())?,
        target_group_record_index: decal_reference(
            bytes,
            scope_at + layout.target_group_reference(),
            layout.typed(),
            TYPED_TARGET_GROUP_REFERENCE_TYPE,
        )?,
    })
}

fn parse_decal_image_frame(
    bytes: &[u8],
    stream: &str,
    scope_record_index: u32,
    scope_at: usize,
) -> Result<Option<DesignDecalImage>, CodecError> {
    let parsed = (|| {
        let mut prefixes = DesignDecalScopeLayout::ALL
            .into_iter()
            .filter_map(|layout| parse_decal_scope_prefix(bytes, scope_at, layout));
        let prefix = prefixes.next()?;
        if prefixes.next().is_some() {
            return None;
        }

        let mut position = 0;
        let mut asset_record = None;
        while let Some(asset_at) = next_indexed_record_offset(bytes, position) {
            position = asset_at.checked_add(1)?;
            if View::u32_le_at(bytes, asset_at + 7) != Some(prefix.asset_record_index) {
                continue;
            }
            let candidate = match parse_decal_asset_record(
                bytes,
                asset_at,
                prefix.asset_record_index,
                prefix.layout,
            ) {
                Ok(Some(candidate)) => candidate,
                Ok(None) => continue,
                Err(error) => return Some(Err(error)),
            };
            if asset_record.replace(candidate).is_some() {
                return None;
            }
        }
        DesignDecalImage::new(
            ids::native_design_decal_image_id(stream, scope_at),
            crate::records::identity::Located {
                value: scope_record_index,
                offset: u64::try_from(scope_at).ok()?,
            },
            prefix.layout,
            DesignDecalMappingMode::from_code(prefix.mapping_mode),
            prefix.target_group_record_index,
            asset_record?,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

fn parse_decal_asset_record(
    bytes: &[u8],
    asset_at: usize,
    asset_record_index: u32,
    layout: DesignDecalScopeLayout,
) -> Result<Option<DesignDecalAsset>, CodecError> {
    let parsed = (|| {
        let (asset_class_tag, after_asset_tag) =
            lp_ascii_filtered_view(bytes, asset_at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_asset_tag)? != asset_record_index
            || bytes.get(
                asset_at + decal_asset::ZERO_RUN_8
                    ..asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE,
            )? != [0; 8]
        {
            return None;
        }
        let asset_entity_suffix = decal_reference(
            bytes,
            asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE,
            layout.typed(),
            TYPED_ASSET_ENTITY_TYPE,
        )?;
        let name_at = next_indexed_record_offset(bytes, asset_at + decal_asset::ZERO_RUN_8)?;
        if name_at != asset_at + layout.asset_frame_length() {
            return None;
        }
        let (name_class_tag, after_name_tag) =
            lp_ascii_filtered_view(bytes, name_at, 0..=2000, u8::is_ascii_graphic)?;
        let name_record_index = View::u32_le_at(bytes, after_name_tag)?;
        if bytes.get(
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
        let asset_class_tag = asset_class_tag.to_owned();
        let name_class_tag = name_class_tag.to_owned();

        DesignDecalAsset::new(
            layout,
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
    use crate::records::decal::{DesignDecalImage, DesignDecalMappingMode, DesignDecalScopeLayout};
    use crate::records::feature::scope::DesignParameterScope;
    use cadmpeg_ir::assets::{Asset, AssetContent};
    use cadmpeg_ir::features::{
        DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureEvaluation,
        FeatureOperation, SketchFeatureBinding,
    };
    use cadmpeg_ir::report::loss::LossNote;
    use std::collections::BTreeMap;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    const STREAM: &str = "Design/BulkStream.dat";
    const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";

    fn header(bytes: &mut Vec<u8>, tag: [u8; 3], index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&tag);
        bytes.extend_from_slice(&index.to_le_bytes());
    }

    fn reference(bytes: &mut Vec<u8>, typed: bool, target: u32, type_guid: &str) {
        bytes.push(1);
        if typed {
            bytes.extend_from_slice(&u64::from(target).to_le_bytes());
            bytes.extend_from_slice(&36u32.to_le_bytes());
            bytes.extend_from_slice(type_guid.as_bytes());
            bytes.extend_from_slice(&[0; 2]);
        } else {
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
    }

    fn fixture(layout: DesignDecalScopeLayout, mapping_mode: u8) -> (Vec<u8>, usize) {
        let mut bytes = Vec::new();
        header(&mut bytes, *b"258", 17);
        bytes.extend_from_slice(&[0; 8]);
        reference(
            &mut bytes,
            layout.typed(),
            50,
            "878C70A7-15B8-4B0B-B1B7-C2A7A23CC45C",
        );
        header(&mut bytes, *b"279", 18);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for unit in "mark.png".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let scope_at = bytes.len();
        header(&mut bytes, *b"301", 23);
        bytes.extend_from_slice(&[0; 9]);
        if layout.extended() {
            bytes.extend_from_slice(&1u32.to_le_bytes());
        }
        bytes.push(0);
        reference(
            &mut bytes,
            layout.typed(),
            17,
            "9A9B75AF-D286-4548-9301-28910E24EA20",
        );
        bytes.push(mapping_mode);
        reference(
            &mut bytes,
            layout.typed(),
            24,
            "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
        );
        bytes.extend_from_slice(&[0; 20]);
        header(&mut bytes, *b"440", 17);
        bytes.extend_from_slice(&[0; 20]);
        header(&mut bytes, *b"302", 23);
        bytes.extend_from_slice(&[0; 20]);
        (bytes, scope_at)
    }

    fn image(layout: DesignDecalScopeLayout, mapping_mode: u8) -> DesignDecalImage {
        let (bytes, scope_at) = fixture(layout, mapping_mode);
        parse_decal_image_frame(&bytes, STREAM, 23, scope_at)
            .unwrap()
            .expect("complete synthetic Decal frame")
    }

    fn scope() -> DesignParameterScope {
        DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#23",
            crate::records::feature::scope::DesignFeatureKind::Decal,
            23,
        )
    }

    fn feature(scope: &DesignParameterScope) -> Feature {
        Feature {
            product_definition: None,
            id: crate::ids::neutral_feature_id(scope),
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
        }
    }

    fn archive(with_image: bool) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        if with_image {
            zip.start_file(ENTRY, stored).unwrap();
            zip.write_all(b"PNG").unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn project(
        archive: &[u8],
        image: &DesignDecalImage,
        groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
        operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    ) -> (Vec<Asset>, FeatureDefinition, Vec<LossNote>) {
        let scope = scope();
        let mut features = [feature(&scope)];
        let mut losses = Vec::new();
        let assets = crate::test_support::zip_test::with_scan(archive, |scan| {
            super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(image),
                super::DecalTargets { groups, operands },
                &mut features,
                &mut losses,
            )
            .unwrap()
        });
        let [feature] = features;
        (assets, feature.evaluation.definition().clone(), losses)
    }

    fn loss_codes(losses: &[LossNote]) -> Vec<&str> {
        losses.iter().map(|note| note.code.local_code()).collect()
    }

    #[test]
    fn decal_frame_decodes_image_mode_and_target() {
        let image = image(DesignDecalScopeLayout::Compact, 0x60);
        assert_eq!(image.asset.record_index(), 17);
        assert_eq!(image.asset.entity_suffix(), 50);
        assert_eq!(image.asset.name(), "mark.png");
        assert_eq!(image.mapping_mode, DesignDecalMappingMode::FitToFaces);
        assert_eq!(image.target_group_record_index, 24);
        assert_eq!(image.asset.primary_frame_length(), 30);
        assert_eq!(image.asset.name_frame_length(), 41);
    }

    #[test]
    fn decal_frame_decodes_every_prefix_layout() {
        for (layout, asset_frame_length, asset_reference, mapping_mode, target_reference) in [
            (DesignDecalScopeLayout::Compact, 30, 21, 32, 33),
            (DesignDecalScopeLayout::Extended, 30, 25, 36, 37),
            (DesignDecalScopeLayout::TypedCompact, 70, 21, 72, 73),
            (DesignDecalScopeLayout::TypedExtended, 70, 25, 76, 77),
        ] {
            let (bytes, scope_at) = fixture(layout, 0x01);
            let image = parse_decal_image_frame(&bytes, STREAM, 23, scope_at)
                .unwrap()
                .expect("complete synthetic Decal frame");
            assert_eq!(image.layout(), layout);
            assert_eq!(image.mapping_mode.code(), 0x01);
            assert_eq!(image.asset.record_index(), 17);
            assert_eq!(image.asset.entity_suffix(), 50);
            assert_eq!(image.asset.name(), "mark.png");
            assert_eq!(image.target_group_record_index, 24);
            assert_eq!(image.asset.primary_frame_length(), asset_frame_length);
            let wire = serde_json::to_value(&image).unwrap();
            let offset = |relative: usize| serde_json::json!(scope_at + relative);
            assert_eq!(wire["asset_reference_offset"], offset(asset_reference + 1));
            assert_eq!(wire["mapping_mode_offset"], offset(mapping_mode));
            assert_eq!(
                wire["target_group_reference_offset"],
                offset(target_reference + 1)
            );
            let decoded: DesignDecalImage = serde_json::from_value(wire).unwrap();
            assert_eq!(decoded, image);
        }
    }

    #[test]
    fn decal_frame_rejects_an_unframed_name() {
        let (mut bytes, scope_at) = fixture(DesignDecalScopeLayout::Compact, 0x60);
        bytes[scope_at..scope_at + 4].fill(0);
        assert!(parse_decal_image_frame(&bytes, STREAM, 23, scope_at)
            .unwrap()
            .is_none());
    }

    #[test]
    fn typed_decal_frame_requires_its_reference_type_guids() {
        let layout = DesignDecalScopeLayout::TypedExtended;
        let (bytes, scope_at) = fixture(layout, 0x01);
        let type_guid_at = 13;
        for reference_at in [
            scope_at + 25 + type_guid_at,
            scope_at + 77 + type_guid_at,
            19 + type_guid_at,
        ] {
            let mut bytes = bytes.clone();
            bytes[reference_at] = b'0';
            assert!(parse_decal_image_frame(&bytes, STREAM, 23, scope_at)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn decal_projection_emits_one_asset() {
        let image = image(DesignDecalScopeLayout::Compact, 0x60);
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
                asset_id_offset: 1056,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "11111111-2222-4333-8444-555555555555".to_owned(),
                )
                .unwrap(),
                context_id_offset: 1090,
                selector_tail: None,
                references: vec![
                    crate::records::topology::body_recipe::DesignBodyRecipeReference {
                        design_reference: 1,
                        design_reference_offset: 1025,
                        form: 1,
                        form_offset: 1033,
                        candidate_faces: vec![face.clone()],
                        preceding_candidate_faces: Vec::new(),
                        preceding_body_slots: Vec::new(),
                    },
                ],
                nested_record_index: 103,
                nested_record_index_offset: 1038,
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
        let (assets, definition, losses) = project(
            &archive(true),
            &image,
            std::slice::from_ref(&group),
            std::slice::from_ref(&operand),
        );
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
        assert_eq!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: crate::ids::neutral_asset_id(ENTRY),
                faces: FaceSelection::Resolved {
                    faces: vec![face],
                    native: operand.id.clone(),
                },
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            })
        );
        assert!(losses.is_empty());
    }

    #[test]
    fn decal_projection_keeps_an_unresolved_target_and_assumes_the_mapping() {
        let image = image(DesignDecalScopeLayout::TypedExtended, 0x01);
        let (assets, definition, losses) = project(&archive(true), &image, &[], &[]);
        assert_eq!(assets.len(), 1);
        assert_eq!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: crate::ids::neutral_asset_id(ENTRY),
                faces: FaceSelection::Native(scope().id),
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            })
        );
        assert_eq!(
            loss_codes(&losses),
            [
                "appearance.decal-mapping-assumed",
                "appearance.decal-target-unresolved"
            ]
        );
    }

    #[test]
    fn decal_projection_assumes_an_external_image_without_an_archive_entry() {
        let image = image(DesignDecalScopeLayout::Compact, 0x60);
        let (assets, definition, losses) = project(&archive(false), &image, &[], &[]);
        let [asset] = assets.as_slice() else {
            panic!("one external Decal image asset");
        };
        assert_eq!(
            asset
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str),
            Some("mark.png")
        );
        assert_eq!(
            asset
                .media_type
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str),
            Some("image/png")
        );
        assert!(matches!(
            &asset.content,
            AssetContent::External { uri } if uri.as_str() == image.id
        ));
        assert!(matches!(
            definition,
            FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: ref id,
                faces: FaceSelection::Native(ref native),
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            }) if *id == asset.id && native == &scope().id
        ));
        assert_eq!(
            loss_codes(&losses),
            [
                "appearance.decal-target-unresolved",
                "appearance.decal-image-unresolved"
            ]
        );
    }
    #[test]
    fn decal_projection_retains_a_missing_definition_as_an_external_source() {
        let scope = scope();
        let mut features = [feature(&scope)];
        let mut losses = Vec::new();
        let assets = crate::test_support::zip_test::with_scan(&archive(false), |scan| {
            super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                &[],
                super::DecalTargets {
                    groups: &[],
                    operands: &[],
                },
                &mut features,
                &mut losses,
            )
            .unwrap()
        });
        assert_eq!(assets.len(), 1);
        assert!(
            matches!(&assets[0].content, AssetContent::External { uri } if uri.as_str() == scope.id)
        );
        assert!(matches!(features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Decal {
                asset, faces: FaceSelection::Native(native), mapping: DecalMapping::FitToFaces, opacity: None,
            }) if *asset == assets[0].id && native == &scope.id));
        assert_eq!(
            loss_codes(&losses),
            [
                "appearance.decal-mapping-assumed",
                "appearance.decal-target-unresolved",
                "appearance.decal-definition-unresolved",
            ]
        );
    }
}
