// SPDX-License-Identifier: Apache-2.0

use super::{anonymous, physically_based_payload, texture_payload};
use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::loss::RhinoLossCode;
use crate::presentation::{
    parse_material, parse_physically_based_material, texture_array, MaterialParseInput, TEXTURE,
};
use crate::test_support::test_dump::utf16_bytes;
use crate::wire::Uuid;
use cadmpeg_ir::scalar::FiniteBinary32;

/// One legacy (outer version 2.0) material whose transparent color is the
/// bogus [128, 128, 128] that the pre-2009 rule replaces with `diffuse`.
fn legacy_material_bytes(diffuse: [u8; 4]) -> Vec<u8> {
    let mut body = [[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
    body.extend(utf16_bytes("steel"));
    body.extend([0x22; 16]);
    for color in [
        [1, 2, 3, 4],
        diffuse,
        [9, 10, 11, 12],
        [13, 14, 15, 16],
        [17, 18, 19, 20],
        [128, 128, 128, 24],
    ] {
        body.extend(color);
    }
    for value in [1.5_f64, 0.25, 64.0, 0.1] {
        body.extend(value.to_le_bytes());
    }
    body.extend(anonymous(0, &0_i32.to_le_bytes()));
    body.extend(utf16_bytes(""));
    body.extend(0_i32.to_le_bytes());
    body.extend([1, 0]);
    body.push(1);
    for value in [0.9_f64, 0.8, 1.4] {
        body.extend(value.to_le_bytes());
    }
    body.extend([0x33; 16]);
    body.push(1);
    body.extend([0xaa, 0xbb]);
    let inner = anonymous(7, &body);
    let mut bytes = vec![0x20];
    bytes.extend(inner);
    bytes
}

fn legacy_material_refusal(limit: u64, writer_version: Option<i64>) -> FramingError {
    let bytes = legacy_material_bytes([5, 6, 7, 8]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("legacy material root admitted");
    parse_material(
        &ctx,
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V5,
            writer_version,
            source_offset: 0,
            physically_based: None,
        },
        &mut Vec::new(),
    )
    .expect_err("legacy material retained value exceeds limit")
}

#[test]
fn legacy_material_name_refuses_retained_limit() {
    assert!(
        matches!(legacy_material_refusal(0, Some(200_912_009)), FramingError::Resource(refusal) if refusal.operation == "Rhino material name")
    );
}

#[test]
fn legacy_material_id_refuses_retained_limit() {
    assert!(
        matches!(legacy_material_refusal(5, Some(200_912_009)), FramingError::Resource(refusal) if refusal.operation == "Rhino material ID")
    );
}

#[test]
fn legacy_material_source_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:material#11111111-1111-1111-1111-111111111111".len();
    assert!(
        matches!(legacy_material_refusal(u64::try_from(5 + id_len).expect("budget fits"), Some(200_912_009)), FramingError::Resource(refusal) if refusal.operation == "Rhino material source UUID")
    );
}

#[test]
fn legacy_material_plugin_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:material#11111111-1111-1111-1111-111111111111".len();
    assert!(
        matches!(legacy_material_refusal(u64::try_from(5 + id_len + 36).expect("budget fits"), Some(200_912_009)), FramingError::Resource(refusal) if refusal.operation == "Rhino material plugin UUID")
    );
}

#[test]
fn legacy_material_rdk_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:material#11111111-1111-1111-1111-111111111111".len();
    assert!(
        matches!(legacy_material_refusal(u64::try_from(5 + id_len + 72).expect("budget fits"), Some(200_912_009)), FramingError::Resource(refusal) if refusal.operation == "Rhino material RDK UUID")
    );
}

#[test]
fn unstamped_material_loss_refuses_collection_limit() {
    let bytes = legacy_material_bytes([5, 6, 7, 8]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("legacy material root admitted");
    let error = parse_material(
        &ctx,
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V5,
            writer_version: None,
            source_offset: 0,
            physically_based: None,
        },
        &mut Vec::new(),
    )
    .expect_err("writer-stamp loss exceeds collection limit");
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == "Rhino material writer-stamp losses")
    );
}

#[test]
fn unstamped_material_loss_text_refuses_retained_limit() {
    assert!(
        matches!(legacy_material_refusal(5, None), FramingError::Resource(refusal) if refusal.operation == "Rhino material writer-stamp loss text")
    );
}

#[test]
fn legacy_material_preserves_core_appearance_and_switches() {
    let bytes = legacy_material_bytes([5, 6, 7, 8]);
    let material = parse_material(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V5,
            writer_version: Some(200_912_009),
            source_offset: 0,
            physically_based: None,
        },
        &mut Vec::new(),
    )
    .expect("required invariant");
    assert_eq!(material.name, "steel");
    assert_eq!(material.diffuse, [5, 6, 7, 8]);
    assert_eq!(material.transparent, material.diffuse);
    assert_eq!(
        material.index_of_refraction,
        crate::test_support::finite(1.5)
    );
    assert!(material.shareable);
    assert!(!material.disable_lighting);
}

/// The pre-2009 transparency substitution rests on the stamp.
///
/// The same bytes give diffuse under an old stamp and the stored
/// [128, 128, 128] under none, so an unstamped archive emits a color the
/// archive does not vouch for - unless diffuse already equals the stored
/// color, where both readings agree and nothing was substituted.
#[test]
fn unstamped_legacy_material_charges_the_transparency_stamp_loss() {
    let bytes = legacy_material_bytes([5, 6, 7, 8]);
    let mut losses = Vec::new();
    let material = parse_material(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V5,
            writer_version: None,
            source_offset: 0,
            physically_based: None,
        },
        &mut losses,
    )
    .expect("legacy material without a writer stamp");
    assert_eq!(material.transparent, [128, 128, 128, 24]);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code.local_code(),
        RhinoLossCode::SourceWriterStampUnverified.code()
    );

    let mut stamped_losses = Vec::new();
    let stamped = parse_material(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V5,
            writer_version: Some(200_912_010),
            source_offset: 0,
            physically_based: None,
        },
        &mut stamped_losses,
    )
    .expect("legacy material with a modern writer stamp");
    assert_eq!(stamped.transparent, [128, 128, 128, 24]);
    assert!(stamped_losses.is_empty(), "{stamped_losses:?}");

    // Both readings give the same color, so no color was substituted.
    let agreeing = legacy_material_bytes([128, 128, 128, 24]);
    let mut agreeing_losses = Vec::new();
    let material = parse_material(
        &cadmpeg_test_support::service_decode_context(),
        &agreeing,
        MaterialParseInput {
            range: 0..agreeing.len(),
            archive: ArchiveVersion::V5,
            writer_version: None,
            source_offset: 0,
            physically_based: None,
        },
        &mut agreeing_losses,
    )
    .expect("legacy material whose diffuse equals its transparent color");
    assert_eq!(material.transparent, material.diffuse);
    assert!(agreeing_losses.is_empty(), "{agreeing_losses:?}");
}

fn v2_v3_material_payload(minor: u8) -> Vec<u8> {
    let mut bytes = vec![0x10 | minor];
    for color in [
        [1, 2, 3, 4],
        [5, 6, 7, 8],
        [9, 10, 11, 12],
        [13, 14, 15, 16],
    ] {
        bytes.extend(color);
    }
    for value in [64.0_f64, 0.25] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([21, 22, 23, 24]);
    bytes.extend([25, 26, 27, 28]);
    bytes.extend(3_i16.to_le_bytes());
    bytes.extend(4_i16.to_le_bytes());
    bytes.extend(0.5_f64.to_le_bytes());
    bytes.extend(1.5_f64.to_le_bytes());

    bytes.extend(utf16_bytes("bitmap.png"));
    bytes.extend(2_i32.to_le_bytes());
    bytes.extend(31_i32.to_le_bytes());
    bytes.extend(utf16_bytes("bump.png"));
    bytes.extend(1_i32.to_le_bytes());
    bytes.extend(32_i32.to_le_bytes());
    bytes.extend(2.5_f64.to_le_bytes());
    bytes.extend(utf16_bytes("environment.png"));
    bytes.extend(9_i32.to_le_bytes());
    bytes.extend(33_i32.to_le_bytes());

    bytes.extend(7_i32.to_le_bytes());
    bytes.extend([0x44; 16]);
    bytes.extend(utf16_bytes("obsolete library"));
    bytes.extend(utf16_bytes("old steel"));
    if minor >= 1 {
        bytes.extend([0x55; 16]);
        bytes.extend([41, 42, 43, 44]);
        bytes.extend([45, 46, 47, 48]);
        bytes.extend(1.45_f64.to_le_bytes());
    }
    bytes.extend([0xaa, 0xbb]);
    bytes
}

fn v2_v3_material_refusal(limit: u64) -> FramingError {
    let bytes = v2_v3_material_payload(1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("V2 material root admitted");
    parse_material(
        &ctx,
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V2,
            writer_version: None,
            source_offset: 77,
            physically_based: None,
        },
        &mut Vec::new(),
    )
    .expect_err("V2 material retained value exceeds limit")
}

macro_rules! v2_material_text_limit {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert!(matches!(
                v2_v3_material_refusal($limit),
                FramingError::Resource(refusal) if refusal.operation == $operation
            ));
        }
    };
}

v2_material_text_limit!(
    v2_bitmap_path_refuses_retained_limit,
    0,
    "Rhino V2/V3 texture path"
);
v2_material_text_limit!(
    v2_bump_path_refuses_retained_limit,
    10,
    "Rhino V2/V3 texture path"
);
v2_material_text_limit!(
    v2_environment_path_refuses_retained_limit,
    18,
    "Rhino V2/V3 texture path"
);
v2_material_text_limit!(
    v2_material_name_refuses_retained_limit,
    33,
    "Rhino V2/V3 material name"
);
v2_material_text_limit!(
    v2_material_id_refuses_retained_limit,
    42,
    "Rhino material ID"
);

#[test]
fn v2_material_source_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:material#55555555-5555-5555-5555-555555555555".len();
    assert!(matches!(
        v2_v3_material_refusal(u64::try_from(42 + id_len).expect("budget fits")),
        FramingError::Resource(refusal) if refusal.operation == "Rhino material source UUID"
    ));
}

#[test]
fn v2_material_plugin_uuid_refuses_retained_limit() {
    let id_len = "rhino:presentation:material#55555555-5555-5555-5555-555555555555".len();
    assert!(matches!(
        v2_v3_material_refusal(u64::try_from(42 + id_len + 36).expect("budget fits")),
        FramingError::Resource(refusal) if refusal.operation == "Rhino material plugin UUID"
    ));
}

#[test]
fn v2_v3_material_reads_direct_prefix_and_legacy_textures() {
    for archive in [ArchiveVersion::V2, ArchiveVersion::V3] {
        let bytes = v2_v3_material_payload(1);
        let material = parse_material(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            MaterialParseInput {
                range: 0..bytes.len(),
                archive,
                writer_version: None,
                source_offset: 77,
                physically_based: None,
            },
            &mut Vec::new(),
        )
        .expect("V2/V3 material payload");
        assert_eq!(material.archive_index, Some(7));
        assert_eq!(material.name, "old steel");
        assert_eq!(
            material.plugin_uuid,
            Uuid::from_wire([0x44; 16]).to_string()
        );
        assert_eq!(material.ambient, [1, 2, 3, 4]);
        assert_eq!(material.diffuse, [5, 6, 7, 8]);
        assert_eq!(material.shine, crate::test_support::finite(64.0));
        assert_eq!(material.transparency, crate::test_support::finite(0.25));
        assert_eq!(material.reflection, [41, 42, 43, 44]);
        assert_eq!(material.transparent, [45, 46, 47, 48]);
        assert_eq!(
            material.index_of_refraction,
            crate::test_support::finite(1.45)
        );
        assert_eq!(
            material.source_uuid,
            Some(Uuid::from_wire([0x55; 16]).to_string())
        );
        assert_eq!(material.textures.len(), 3);
        assert_eq!(material.textures[0].legacy_file_path, "bitmap.png");
        assert_eq!(material.textures[0].texture_type, 1);
        assert_eq!(material.textures[0].mode, 2);
        assert_eq!(material.textures[1].legacy_file_path, "bump.png");
        assert_eq!(material.textures[1].texture_type, 2);
        assert_eq!(material.textures[1].mode, 1);
        assert_eq!(
            material.textures[1].bump_scale,
            crate::test_support::finite_array([0.0, 2.5])
        );
        assert_eq!(material.textures[2].legacy_file_path, "environment.png");
        assert_eq!(material.textures[2].texture_type, 86);
        assert_eq!(material.textures[2].mode, 1);
        assert_eq!(material.textures[0].source_offset, 77);
        assert_eq!(material.textures[0].wrap, [0, 0, 0]);
        assert_eq!(
            material.textures[0].uvw_transform[0],
            crate::test_support::finite_array([1.0, 0.0, 0.0, 0.0])
        );
    }
}

#[test]
fn v2_v3_material_minor_zero_uses_source_defaults_without_fabricating_identity() {
    let bytes = v2_v3_material_payload(0);
    let material = parse_material(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V2,
            writer_version: None,
            source_offset: 77,
            physically_based: None,
        },
        &mut Vec::new(),
    )
    .expect("V2 minor-zero material payload");
    assert_eq!(material.id, "rhino:presentation:material#record-77");
    assert_eq!(material.source_uuid, None);
    assert_eq!(material.reflection, [255, 255, 255, 0]);
    assert_eq!(material.transparent, [255, 255, 255, 0]);
    assert_eq!(
        material.index_of_refraction,
        crate::test_support::finite(1.0)
    );
}

#[test]
fn physically_based_material_reads_versioned_prefix_and_suffix() {
    let bytes = physically_based_payload(2, &[0xaa, 0xbb]);
    let payload = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
        .expect("outer userdata payload");
    let material = parse_physically_based_material(&bytes, payload.body(), ArchiveVersion::V8)
        .expect("physically based material");
    assert_eq!(material.revision.version(), 2);
    assert_eq!(
        material.base_color.map(FiniteBinary32::get),
        [0.1, 0.2, 0.3, 0.4]
    );
    assert_eq!(material.brdf, 1);
    assert_eq!(material.subsurface, crate::test_support::finite(0.5));
    assert_eq!(
        material
            .subsurface_scattering_color
            .map(FiniteBinary32::get),
        [0.6, 0.7, 0.8, 0.9]
    );
    assert_eq!(
        material.subsurface_scattering_radius,
        crate::test_support::finite(1.0)
    );
    assert_eq!(material.metallic, crate::test_support::finite(2.0));
    assert_eq!(material.specular, crate::test_support::finite(3.0));
    assert_eq!(material.specular_tint, crate::test_support::finite(4.0));
    assert_eq!(material.roughness, crate::test_support::finite(5.0));
    assert_eq!(material.anisotropic, crate::test_support::finite(6.0));
    assert_eq!(
        material.anisotropic_rotation,
        crate::test_support::finite(7.0)
    );
    assert_eq!(material.sheen, crate::test_support::finite(8.0));
    assert_eq!(material.sheen_tint, crate::test_support::finite(9.0));
    assert_eq!(material.clearcoat, crate::test_support::finite(10.0));
    assert_eq!(
        material.clearcoat_roughness,
        crate::test_support::finite(11.0)
    );
    assert_eq!(material.opacity_ior, crate::test_support::finite(12.0));
    assert_eq!(material.opacity, crate::test_support::finite(13.0));
    assert_eq!(
        material.opacity_roughness,
        crate::test_support::finite(14.0)
    );
    assert_eq!(
        material.emission.map(FiniteBinary32::get),
        [0.11, 0.22, 0.33, 0.44]
    );
    assert_eq!(material.revision.alpha(), crate::test_support::finite(0.77));
}

#[test]
fn physically_based_material_version_one_defaults_alpha() {
    let bytes = physically_based_payload(1, &[0xcc, 0xdd]);
    let payload = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
        .expect("outer userdata payload");
    let material = parse_physically_based_material(&bytes, payload.body(), ArchiveVersion::V8)
        .expect("version one physically based material");
    assert_eq!(material.revision.version(), 1);
    assert_eq!(material.revision.alpha(), crate::test_support::finite(1.0));
}

#[test]
fn material_texture_array_refuses_collection_limit() {
    let archive = ArchiveVersion::V8;
    let texture = crate::test_support::test_dump::class_wrapper(
        archive,
        TEXTURE.to_wire(),
        &texture_payload(0, &[]),
    );
    let mut body = 1_i32.to_le_bytes().to_vec();
    body.extend(texture);
    let bytes = anonymous(0, &body);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("texture array bounds");
    let error = texture_array(&ctx, &bytes, &mut reader, archive, &mut Vec::new())
        .expect_err("texture exceeds collection limit");
    assert!(matches!(
        error,
        FramingError::Resource(refusal) if refusal.operation == "Rhino material textures"
    ));
}

#[test]
fn v2_v3_material_textures_refuse_collection_limit() {
    let bytes = v2_v3_material_payload(1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("root bytes admitted");
    let error = parse_material(
        &ctx,
        &bytes,
        MaterialParseInput {
            range: 0..bytes.len(),
            archive: ArchiveVersion::V2,
            writer_version: None,
            source_offset: 0,
            physically_based: None,
        },
        &mut Vec::new(),
    )
    .expect_err("legacy texture exceeds collection limit");
    assert!(matches!(
        error,
        FramingError::Resource(refusal)
            if refusal.operation == "Rhino V2/V3 material textures"
    ));
}
