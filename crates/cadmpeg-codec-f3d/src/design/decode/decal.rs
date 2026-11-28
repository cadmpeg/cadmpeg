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
use crate::layout::design_decal_placement_prefix as decal_placement;
use crate::layout::design_decal_scope_extended_prefix as decal_extended_scope;
use crate::loss::F3dLossCode;
use crate::records::{
    decal::{DesignDecalAsset, DesignDecalImage, DesignDecalPlacement, DesignDecalScopeLayout},
    feature::scope::DesignParameterScope,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    },
};
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};
use cadmpeg_ir::features::datums::DecalMapping;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};

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
    let mut native_placements = 0usize;
    let mut assumed_layouts = 0usize;
    let mut assumed_normals = 0usize;
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
            if let Some(placement) = &image.placement {
                native_placements += 1;
                assumed_layouts += usize::from(!placement.has_stated_layout());
                assumed_normals += usize::from(!placement.normal_is_parallel_to_axes());
                let target_faces = match targets.faces(scope, image.target_group_record_index) {
                    Some(FaceSelection::Resolved { faces, .. }) => Some(
                        faces
                            .iter()
                            .map(cadmpeg_ir::ids::FaceId::as_str)
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => {
                        unresolved_targets += 1;
                        None
                    }
                };
                for (key, value) in [
                    ("decal:asset", asset.id.to_string()),
                    (
                        "decal:asset-record-index",
                        image.asset.record_index().to_string(),
                    ),
                    (
                        "decal:target-group-record-index",
                        image.target_group_record_index.to_string(),
                    ),
                ]
                .into_iter()
                .chain(target_faces.map(|faces| ("decal:target-faces", faces)))
                {
                    feature.source_properties.extend(
                        cadmpeg_core::text::NonBlankString::new(key).map(|key| (key, value)),
                    );
                }
                assets.push(asset);
                continue;
            }
            let faces = targets
                .faces(scope, image.target_group_record_index)
                .unwrap_or_else(|| {
                    unresolved_targets += 1;
                    FaceSelection::Native(scope.id.clone())
                });
            (asset, faces)
        } else {
            unresolved_definitions += 1;
            let prefix = scope_prefix(scan, scope)?;
            let faces = prefix
                .as_ref()
                .and_then(|prefix| targets.faces(scope, prefix.target_group_record_index))
                .unwrap_or_else(|| {
                    unresolved_targets += 1;
                    FaceSelection::Native(scope.id.clone())
                });
            if let Some(prefix) = prefix {
                for (key, value) in [
                    (
                        "decal:asset-record-index",
                        prefix.asset_record_index.to_string(),
                    ),
                    ("decal:scope-code", prefix.scope_code.to_string()),
                    (
                        "decal:target-group-record-index",
                        prefix.target_group_record_index.to_string(),
                    ),
                ] {
                    feature.source_properties.extend(
                        cadmpeg_core::text::NonBlankString::new(key).map(|key| (key, value)),
                    );
                }
            }
            (unresolved_image_asset(None, &scope.id)?, faces)
        };
        assumed_mappings += 1;
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
            "{assumed_mappings} Decal feature(s) have no decoded placement record; the decoder assumes the fit-to-faces mapping (DR-484)."
        )));
    }
    if native_placements != 0 {
        losses.push(F3dLossCode::DecalPlacementNative.note(format!(
            "{native_placements} Decal feature(s) keep their placement frame in the native Decal image record and name their image, target group, and resolved target faces in source properties; the feature definition stays native."
        )));
    }
    if assumed_layouts != 0 {
        losses.push(F3dLossCode::DecalPlacementLayoutAssumed.note(format!(
            "{assumed_layouts} Decal placement record(s) store adjacent trailing references or five zero bytes before the normal; the decoder reads the frame at the one position that closes the record (DR-484)."
        )));
    }
    if assumed_normals != 0 {
        losses.push(F3dLossCode::DecalPlacementNormalAssumed.note(format!(
            "{assumed_normals} Decal placement record(s) store a normal that is not parallel to U x V; the decoder keeps the stored frame (DR-484)."
        )));
    }
    if unresolved_targets != 0 {
        losses.push(F3dLossCode::DecalTargetUnresolved.note(format!(
            "{unresolved_targets} Decal feature(s) have unresolved target faces because the target group or its members have no complete whole-body recipe faces (DR-255)."
        )));
    }
    if unresolved_images != 0 {
        losses.push(F3dLossCode::DecalImageUnresolved.note(format!(
            "{unresolved_images} Decal feature(s) name an image basename with no unique readable archive entry; the decoder retains an external source identity for that image."
        )));
    }
    if unresolved_definitions != 0 {
        losses.push(F3dLossCode::DecalDefinitionUnresolved.note(format!(
            "{unresolved_definitions} Decal feature(s) have no decoded image definition; the decoder retains the scope identity as the external image source (DR-485)."
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
        target_group_record_index: u32,
    ) -> Option<FaceSelection> {
        let native_stream = ids::native_stream(&scope.id);
        let group = self.groups.iter().find(|group| {
            group.scope_record_index == scope.record_index
                && group.record_index == target_group_record_index
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
    scope_code: u8,
    target_group_record_index: u32,
}

fn scope_prefix(
    scan: &ContainerScan,
    scope: &DesignParameterScope,
) -> Result<Option<DecalScopePrefix>, CodecError> {
    let Some(entry) = scan.entries.iter().find(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
            && ids::native_stream(&scope.id) == Some(ids::native_scope(&entry.name).as_str())
    }) else {
        return Ok(None);
    };
    let bytes = scan.entry_bytes(&entry.name)?;
    let (Ok(start), Ok(end)) = (
        usize::try_from(scope.byte_offset()),
        usize::try_from(scope.reference_count_offset()),
    ) else {
        return Ok(None);
    };
    let Some(bytes) = bytes.get(..end) else {
        return Ok(None);
    };
    Ok(unique_scope_prefix(bytes, start))
}

fn unique_scope_prefix(bytes: &[u8], scope_at: usize) -> Option<DecalScopePrefix> {
    let mut prefixes = DesignDecalScopeLayout::ALL
        .into_iter()
        .filter_map(|layout| parse_decal_scope_prefix(bytes, scope_at, layout));
    let prefix = prefixes.next()?;
    prefixes.next().is_none().then_some(prefix)
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
        scope_code: *bytes.get(scope_at + layout.scope_code())?,
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
        let prefix = unique_scope_prefix(bytes, scope_at)?;

        let mut position = 0;
        let mut asset_record = None;
        let mut placements = Vec::new();
        while let Some(asset_at) = next_indexed_record_offset(bytes, position) {
            position = asset_at.checked_add(1)?;
            if View::u32_le_at(bytes, asset_at + 7) != Some(prefix.asset_record_index) {
                continue;
            }
            if !prefix.layout.typed()
                && decal_placement_prefix(bytes, asset_at, prefix.asset_record_index).is_some()
            {
                placements.push(parse_decal_placement_record(
                    bytes,
                    asset_at,
                    prefix.asset_record_index,
                ));
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
        let mut image = DesignDecalImage::new(
            ids::native_design_decal_image_id(stream, scope_at),
            crate::records::identity::Located {
                value: scope_record_index,
                offset: u64::try_from(scope_at).ok()?,
            },
            prefix.layout,
            prefix.scope_code,
            prefix.target_group_record_index,
            asset_record?,
        )
        .ok()?;
        let asset_at = image.asset.byte_offset();
        if let [Some((placement, end))] = placements.as_slice() {
            if u64::try_from(*end).ok() == Some(asset_at) && placement.admit(asset_at).is_ok() {
                image.placement = Some(placement.clone());
            }
        }
        Some(Ok(image))
    })();
    parsed.transpose()
}

/// Zero-run lengths between the two trailing references of a Decal placement record.
const PLACEMENT_REFERENCE_SEPARATORS: [usize; 2] = [0, 1];

/// Zero-run lengths between the second trailing reference and the normal.
const PLACEMENT_NORMAL_GAPS: [usize; 2] = [4, 5];

/// Byte length of a marked record reference: `01`, a u32 record index, and six zero bytes.
const MARKED_REFERENCE_LEN: usize = 11;

/// The class tag of a record at `at` that carries the image-asset record index
/// and the fixed prefix of a Decal placement record.
fn decal_placement_prefix(bytes: &[u8], at: usize, asset_record_index: u32) -> Option<&str> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag.checked_add(4)? != at.checked_add(decal_placement::ZERO_RUN_10)?
        || View::u32_le_at(bytes, after_tag)? != asset_record_index
        || bytes.get(at + decal_placement::ZERO_RUN_10..at + decal_placement::LEADING_REFERENCE)?
            != [0; 10]
    {
        return None;
    }
    marked_record_reference(bytes, at + decal_placement::LEADING_REFERENCE)?;
    Some(class_tag)
}

/// The placement frame of the record at `at` and the offset of the record that follows it.
fn parse_decal_placement_record(
    bytes: &[u8],
    at: usize,
    asset_record_index: u32,
) -> Option<(DesignDecalPlacement, usize)> {
    let class_tag = decal_placement_prefix(bytes, at, asset_record_index)?;
    let byte_offset = u64::try_from(at).ok()?;
    let vertex_count = View::u32_le_at(bytes, at + decal_placement::VERTEX_COUNT)?;
    let pairs_at = at.checked_add(decal_placement::LEN)?;
    let pairs = bounded_len(
        u64::from(vertex_count),
        8,
        bytes.len().checked_sub(pairs_at)?,
    )?;
    let mut cursor = pairs_at.checked_add(pairs.checked_mul(8)?)?;
    let ordinal = View::u32_le_at(bytes, cursor)?;
    cursor += 4;
    let axis = |at: usize| -> Option<[f64; 3]> {
        let mut axis = [0.0; 3];
        for (lane, value) in axis.iter_mut().enumerate() {
            let component = View::f32_le_at(bytes, at.checked_add(lane * 4)?)?;
            if !component.is_finite() {
                return None;
            }
            *value = f64::from(component);
        }
        Some(axis)
    };
    let u_axis = axis(cursor)?;
    let v_axis = axis(cursor + 12)?;
    cursor += 24;
    let flag_bytes: [u8; 3] = bytes.get(cursor..cursor + 3)?.try_into().ok()?;
    cursor += 3;
    marked_record_reference(bytes, cursor)?;
    cursor += MARKED_REFERENCE_LEN;
    if !View::f64_le_at(bytes, cursor)?.is_finite()
        || !View::f64_le_at(bytes, cursor + 8)?.is_finite()
        || bytes.get(cursor + 16) != Some(&0)
        || !View::f32_le_at(bytes, cursor + 17)?.is_finite()
    {
        return None;
    }
    cursor += 21;
    marked_record_reference(bytes, cursor)?;
    cursor += MARKED_REFERENCE_LEN;
    let vector = |at: usize| -> Option<[f64; 3]> {
        let mut vector = [0.0; 3];
        for (lane, value) in vector.iter_mut().enumerate() {
            *value = View::f64_le_at(bytes, at.checked_add(lane * 8)?)?;
            if !value.is_finite() {
                return None;
            }
        }
        Some(vector)
    };
    let mut frames = PLACEMENT_REFERENCE_SEPARATORS
        .into_iter()
        .flat_map(|separator| PLACEMENT_NORMAL_GAPS.map(|gap| (separator, gap)))
        .filter_map(|(separator, gap)| {
            let reference_at = cursor.checked_add(separator)?;
            let gap_at = reference_at.checked_add(MARKED_REFERENCE_LEN)?;
            let normal_at = gap_at.checked_add(gap)?;
            marked_record_reference(bytes, reference_at)?;
            if bytes
                .get(cursor..reference_at)?
                .iter()
                .any(|byte| *byte != 0)
                || bytes.get(gap_at..normal_at)?.iter().any(|byte| *byte != 0)
            {
                return None;
            }
            let normal = vector(normal_at)?;
            let origin = vector(normal_at + 24)?;
            let (image_name, after_name) = lp_utf16_bounded(bytes, normal_at + 48, 1..=1024)?;
            let triangle_index_count = View::u32_le_at(bytes, after_name)?;
            let indices_at = after_name.checked_add(4)?;
            let indices = bounded_len(
                u64::from(triangle_index_count),
                4,
                bytes.len().checked_sub(indices_at)?,
            )?;
            let end = indices_at.checked_add(indices.checked_mul(4)?)?;
            if next_indexed_record_offset(bytes, end) != Some(end)
                || View::u32_le_at(bytes, end + 7) != Some(asset_record_index)
            {
                return None;
            }
            Some((
                DesignDecalPlacement {
                    byte_offset,
                    class_tag: class_tag.to_owned(),
                    vertex_count,
                    ordinal,
                    u_axis,
                    v_axis,
                    flag_bytes: flag_bytes.into(),
                    reference_separator: u8::try_from(separator).ok()?,
                    normal_gap: u8::try_from(gap).ok()?,
                    normal,
                    origin,
                    image_name,
                    triangle_index_count,
                },
                end,
            ))
            .filter(|(placement, _)| placement.admit(u64::MAX).is_ok())
        });
    let frame = frames.next()?;
    frames.next().is_none().then_some(frame)
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
    use crate::records::decal::{DesignDecalImage, DesignDecalScopeLayout};
    use crate::records::feature::scope::DesignParameterScope;
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_ir::assets::{Asset, AssetContent};
    use cadmpeg_ir::features::datums::DecalMapping;
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::selections::FaceSelection;
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureOperation, SketchFeatureBinding,
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

    fn fixture(layout: DesignDecalScopeLayout, scope_code: u8) -> (Vec<u8>, usize) {
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
        bytes.push(scope_code);
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

    /// A placement record with record index 17 for a 200 by 100 pixel image
    /// centred at (1, 2, 0) with U = (0.5, 0, 0) and V = (0, -0.5, 0).
    fn placement_record(mirror: u8, separator: usize, gap: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        header(&mut bytes, *b"267", 17);
        bytes.extend_from_slice(&[0; 10]);
        reference(&mut bytes, false, 15, "");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        for pair in [[0.25f32, 0.75], [3.0e7, -6.0e7], [0.5, 0.5]] {
            for value in pair {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for value in [0.5f32, 0.0, 0.0, 0.0, -0.5, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, mirror, 0]);
        reference(&mut bytes, false, 19, "");
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        reference(&mut bytes, false, 21, "");
        bytes.extend(std::iter::repeat_n(0, separator));
        reference(&mut bytes, false, 18, "");
        bytes.extend(std::iter::repeat_n(0, gap));
        for value in [0.0f64, 0.0, -1.0, 1.0, 2.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&4u32.to_le_bytes());
        for unit in "mark".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        for index in [0u32, 2, 1] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        bytes
    }

    fn placed_image(scope_code: u8, placement: &[u8]) -> Option<DesignDecalImage> {
        let (frame, scope_at) = fixture(DesignDecalScopeLayout::Compact, scope_code);
        let mut bytes = placement.to_vec();
        bytes.extend_from_slice(&frame);
        parse_decal_image_frame(&bytes, STREAM, 23, placement.len() + scope_at).unwrap()
    }

    #[test]
    fn decal_placement_record_states_the_frame_for_every_scope_code() {
        for (separator, gap) in [(1u8, 4u8), (1, 5), (0, 4), (0, 5)] {
            for mirror in [0, 1, 2] {
                for scope_code in [0x00, 0x01, 0x60, 0xE2] {
                    let image = placed_image(
                        scope_code,
                        &placement_record(mirror, usize::from(separator), usize::from(gap)),
                    )
                    .expect("Decal image with a placement record");
                    assert_eq!(image.scope_code, scope_code);
                    let placement = image.placement.expect("placement frame");
                    assert_eq!(placement.byte_offset, 0);
                    assert_eq!(placement.class_tag, "267");
                    assert_eq!(placement.vertex_count, 3);
                    assert_eq!(placement.ordinal, 1);
                    assert_eq!(placement.u_axis, [0.5, 0.0, 0.0]);
                    assert_eq!(placement.v_axis, [0.0, -0.5, 0.0]);
                    assert_eq!(placement.flag_bytes, [1, mirror, 0]);
                    assert_eq!(
                        (placement.reference_separator, placement.normal_gap),
                        (separator, gap)
                    );
                    assert_eq!(placement.has_stated_layout(), (separator, gap) == (1, 4));
                    assert_eq!(placement.normal, [0.0, 0.0, -1.0]);
                    assert_eq!(placement.origin, [1.0, 2.0, 0.0]);
                    assert_eq!(placement.image_name, "mark");
                    assert_eq!(placement.triangle_index_count, 3);
                }
            }
        }
    }

    #[test]
    fn decal_placement_record_refuses_deviations_and_keeps_the_image() {
        let base = placement_record(0, 1, 4);
        let flags_at = 36 + 3 * 8 + 4 + 24;
        let normal_at = flags_at + 3 + 11 + 21 + 11 + 1 + 11 + 4;
        let mut cases = Vec::new();
        let mut gap = base.clone();
        gap[normal_at - 1] = 1;
        cases.push(("nonzero gap", gap));
        let mut normal = base.clone();
        normal[normal_at + 16..normal_at + 24].copy_from_slice(&(-2.0f64).to_le_bytes());
        cases.push(("normal of length 2", normal));
        let mut count = base.clone();
        count[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(("vertex count past the stream", count));
        let mut indices = base.clone();
        indices.extend_from_slice(&[0; 4]);
        cases.push(("index run short of the image-asset record", indices));
        cases.push(("gap of three bytes", placement_record(0, 0, 3)));
        cases.push(("gap of six bytes", placement_record(0, 0, 6)));
        cases.push(("separator of two bytes", placement_record(0, 2, 4)));
        let mut twice = base.clone();
        twice.extend_from_slice(&base);
        cases.push(("two placement records", twice));
        let mut unreadable_second = base.clone();
        unreadable_second.extend_from_slice(&placement_record(0, 2, 4));
        cases.push((
            "readable record before an unreadable one",
            unreadable_second,
        ));
        let mut separator = base.clone();
        separator[normal_at - 4 - 11 - 1] = 0x7F;
        cases.push(("nonzero separator", separator));
        let mut prefix_run = base.clone();
        prefix_run[15] = 1;
        cases.push(("nonzero byte in the ten-byte zero run", prefix_run));
        let mut marker = base.clone();
        marker[21] = 0;
        cases.push(("leading reference without its marker", marker));
        let mut u_axis = base.clone();
        u_axis[flags_at - 24..flags_at - 20].copy_from_slice(&f32::NAN.to_le_bytes());
        cases.push(("U component that is not a number", u_axis));
        let mut origin = base.clone();
        origin[normal_at + 24..normal_at + 32].copy_from_slice(&f64::INFINITY.to_le_bytes());
        cases.push(("infinite centre", origin));
        let mut scalar_flag = base.clone();
        scalar_flag[flags_at + 3 + 11 + 16] = 1;
        cases.push(("nonzero byte before the f32 value", scalar_flag));
        for (name, placement) in cases {
            let image = placed_image(0x01, &placement).expect(name);
            assert_eq!(image.placement, None, "{name}");
            assert_eq!(image.asset.name(), "mark.png", "{name}");
        }
    }

    #[test]
    fn decal_with_a_placement_record_keeps_its_definition_native() {
        for scope_code in [0x01, 0x60] {
            let image = placed_image(scope_code, &placement_record(0, 1, 4)).unwrap();
            let native = feature(&scope()).evaluation.definition().clone();
            let (assets, definition, losses) = project(&archive(true), &image, &[], &[]);
            assert_eq!(assets.len(), 1);
            assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
            assert_eq!(definition, native);
            assert_eq!(
                loss_codes(&losses),
                [
                    "appearance.decal-placement-native",
                    "appearance.decal-target-unresolved"
                ]
            );
        }
    }

    #[test]
    fn decal_placement_record_outside_the_stated_layout_charges_the_assumed_loss() {
        for (separator, gap, assumed) in [(1, 4, false), (0, 4, true), (0, 5, true), (1, 5, true)] {
            let image = placed_image(0x01, &placement_record(0, separator, gap)).unwrap();
            let (_, _, losses) = project(&archive(true), &image, &[], &[]);
            assert_eq!(
                loss_codes(&losses).contains(&"appearance.decal-placement-layout-assumed"),
                assumed,
                "separator {separator} gap {gap}"
            );
        }
    }

    #[test]
    fn decal_placement_normal_at_an_angle_to_the_axes_charges_the_assumed_loss() {
        let base = placement_record(0, 1, 4);
        let normal_at = 36 + 3 * 8 + 4 + 24 + 3 + 11 + 21 + 11 + 1 + 11 + 4;
        for (normal, charged) in [
            ([0.0f64, 0.0, -1.0], false),
            ([0.0, 0.0, 1.0], false),
            ([0.0, 0.6, -0.8], true),
            ([1.0, 0.0, 0.0], true),
        ] {
            let mut bytes = base.clone();
            for (lane, value) in normal.iter().enumerate() {
                bytes[normal_at + lane * 8..normal_at + lane * 8 + 8]
                    .copy_from_slice(&value.to_le_bytes());
            }
            let image = placed_image(0x01, &bytes).unwrap();
            let placement = image.placement.as_ref().expect("placement frame");
            assert_eq!(placement.normal, normal);
            assert_eq!(placement.u_axis, [0.5, 0.0, 0.0]);
            assert_eq!(placement.v_axis, [0.0, -0.5, 0.0]);
            let (_, _, losses) = project(&archive(true), &image, &[], &[]);
            assert_eq!(
                loss_codes(&losses).contains(&"appearance.decal-placement-normal-assumed"),
                charged,
                "{normal:?}"
            );
        }
    }

    #[test]
    fn decal_with_a_placement_record_names_its_resolved_target_faces() {
        let image = placed_image(0x01, &placement_record(0, 1, 4)).unwrap();
        let (group, operand, face) = resolved_target();
        let scope = scope();
        let mut features = [feature(&scope)];
        let mut losses = Vec::new();
        crate::test_support::zip_test::with_scan(&archive(true), |scan| {
            super::project_decal_images(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                super::DecalTargets {
                    groups: std::slice::from_ref(&group),
                    operands: std::slice::from_ref(&operand),
                },
                &mut features,
                &mut losses,
            )
            .unwrap()
        });
        assert_eq!(
            features[0]
                .source_properties
                .iter()
                .find(|(key, _)| key.as_str() == "decal:target-faces")
                .map(|(_, value)| value.as_str()),
            Some(face.as_str())
        );
        assert_eq!(loss_codes(&losses), ["appearance.decal-placement-native"]);
    }

    #[test]
    fn decal_with_a_placement_record_names_its_image_and_target_group() {
        let image = placed_image(0x01, &placement_record(0, 1, 4)).unwrap();
        let scope = scope();
        let mut features = [feature(&scope)];
        let mut losses = Vec::new();
        crate::test_support::zip_test::with_scan(&archive(true), |scan| {
            super::project_decal_images(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                super::DecalTargets {
                    groups: &[],
                    operands: &[],
                },
                &mut features,
                &mut losses,
            )
            .unwrap()
        });
        let properties = features[0]
            .source_properties
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let asset = crate::ids::neutral_asset_id(ENTRY).to_string();
        assert_eq!(
            properties,
            [
                ("decal:asset", asset.as_str()),
                ("decal:asset-record-index", "17"),
                ("decal:target-group-record-index", "24"),
            ]
        );
    }

    #[test]
    fn typed_decal_keeps_the_fit_to_faces_mapping_beside_a_placement_record() {
        let (frame, scope_at) = fixture(DesignDecalScopeLayout::TypedCompact, 0x01);
        let placement = placement_record(0, 1, 4);
        let mut bytes = placement.clone();
        bytes.extend_from_slice(&frame);
        let image = parse_decal_image_frame(&bytes, STREAM, 23, placement.len() + scope_at)
            .unwrap()
            .expect("typed Decal image");
        assert_eq!(image.placement, None);
    }

    fn image(layout: DesignDecalScopeLayout, scope_code: u8) -> DesignDecalImage {
        let (bytes, scope_at) = fixture(layout, scope_code);
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
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
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
                &cadmpeg_core::decode::test_support::service_decode_context(),
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
    fn decal_frame_decodes_image_scope_code_and_target() {
        let image = image(DesignDecalScopeLayout::Compact, 0x60);
        assert_eq!(image.asset.record_index(), 17);
        assert_eq!(image.asset.entity_suffix(), 50);
        assert_eq!(image.asset.name(), "mark.png");
        assert_eq!(image.scope_code, 0x60);
        assert_eq!(image.placement, None);
        assert_eq!(image.target_group_record_index, 24);
        assert_eq!(image.asset.primary_frame_length(), 30);
        assert_eq!(image.asset.name_frame_length(), 41);
    }

    #[test]
    fn decal_frame_decodes_every_prefix_layout() {
        for (layout, asset_frame_length, asset_reference, scope_code, target_reference) in [
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
            assert_eq!(image.scope_code, 0x01);
            assert_eq!(image.asset.record_index(), 17);
            assert_eq!(image.asset.entity_suffix(), 50);
            assert_eq!(image.asset.name(), "mark.png");
            assert_eq!(image.target_group_record_index, 24);
            assert_eq!(image.asset.primary_frame_length(), asset_frame_length);
            let wire = json::to_value(&image).unwrap();
            let offset = |relative: usize| cadmpeg_core::json!(scope_at + relative);
            assert_eq!(wire["asset_reference_offset"], offset(asset_reference + 1));
            assert_eq!(wire["scope_code_offset"], offset(scope_code));
            assert_eq!(
                wire["target_group_reference_offset"],
                offset(target_reference + 1)
            );
            let decoded: DesignDecalImage = json::from_value(wire).unwrap();
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

    fn resolved_target() -> (
        crate::records::topology::construction::DesignConstructionOperandGroup,
        crate::records::topology::body_recipe::DesignBodyRecipeOperand,
        cadmpeg_ir::ids::FaceId,
    ) {
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
        (group, operand, face)
    }

    #[test]
    fn decal_projection_emits_one_asset() {
        let image = image(DesignDecalScopeLayout::Compact, 0x60);
        let (group, operand, face) = resolved_target();
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
        assert_eq!(loss_codes(&losses), ["appearance.decal-mapping-assumed"]);
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
                "appearance.decal-mapping-assumed",
                "appearance.decal-target-unresolved",
                "appearance.decal-image-unresolved"
            ]
        );
    }
    #[test]
    fn missing_image_definition_preserves_scope_bindings() {
        let entry = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut scope = scope();
        scope
            .try_edit(|draft| {
                draft.id = format!("f3d:{entry}:scope#23");
                draft.reference_count_offset = 64;
                draft.reference_members = crate::records::identity::ReferenceRun::located(vec![
                    crate::records::identity::Located {
                        value: 24,
                        offset: 69,
                    },
                ]);
                draft.kind_offset = 87;
                draft.feature_ordinal_offset = 103;
                draft.frame_length = 183;
                draft.paired_byte_offset = 183;
            })
            .unwrap();
        let (bytes, at) = fixture(DesignDecalScopeLayout::Compact, 0x60);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(entry, stored).unwrap();
        zip.write_all(&bytes[at..at + 44]).unwrap();
        zip.write_all(&[0; 139]).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let mut features = [feature(&scope)];
        let mut losses = Vec::new();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            super::project_decal_images(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                scan,
                &[scope],
                &[],
                super::DecalTargets {
                    groups: &[],
                    operands: &[],
                },
                &mut features,
                &mut losses,
            )
            .unwrap();
        });
        let properties: Vec<_> = features[0]
            .source_properties
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            properties,
            [
                ("decal:asset-record-index", "17"),
                ("decal:scope-code", "96"),
                ("decal:target-group-record-index", "24"),
            ]
        );
        assert_eq!(
            loss_codes(&losses),
            [
                "appearance.decal-mapping-assumed",
                "appearance.decal-target-unresolved",
                "appearance.decal-definition-unresolved",
            ]
        );
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Decal { .. })
        ));
    }

    #[test]
    fn decal_projection_retains_a_missing_definition_as_an_external_source() {
        let scope = scope();
        let mut features = [feature(&scope)];
        let mut losses = Vec::new();
        let assets = crate::test_support::zip_test::with_scan(&archive(false), |scan| {
            super::project_decal_images(
                &cadmpeg_core::decode::test_support::service_decode_context(),
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
