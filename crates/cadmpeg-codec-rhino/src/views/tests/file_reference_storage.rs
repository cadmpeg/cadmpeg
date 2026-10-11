// SPDX-License-Identifier: Apache-2.0
//! Image values keep their backing after temporary diagnostics are dropped.

use crate::chunks::ArchiveVersion;
use crate::loss::{RhinoLossCode, ScratchVec};
use crate::test_support::test_dump::{file_reference, utf16_bytes};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const STORAGE_LIMIT: u64 = 16_384;

fn image_reference_bytes(reference: &super::super::ImageReference) -> usize {
    reference.full_path.len() + reference.relative_path.len() + reference.content_sha1.len()
        + reference.embedded_file_uuid.as_ref().map_or(0, String::len)
}

fn assert_image_diagnostics_released(trace: bool) {
    let archive = ArchiveVersion::V6;
    let mut bytes = if trace { vec![0x14] } else { vec![0x12] };
    bytes.extend(utf16_bytes("image.png"));
    if trace {
        bytes.extend(42.0_f64.to_le_bytes());
        bytes.extend(24.0_f64.to_le_bytes());
        super::serialized_plane(&mut bytes);
        bytes.extend([0, 1, 1]);
    } else {
        bytes.extend([1, 0]);
    }
    let reference_start = bytes.len();
    bytes.extend(file_reference(archive, "/images/source.png", "source.png"));
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = STORAGE_LIMIT;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut staging = ctx.reserve_scoped(0, "image value staging").unwrap();
    let mut losses = ScratchVec::new(&ctx, "image durable losses").unwrap();
    // Keep each admitted image alive while proving that only its final text is scoped.
    if trace {
        let (image, range) = super::super::parse_trace_image(
            &ctx, &bytes, 0..bytes.len(), archive, crate::settings::MillimeterScale::IDENTITY,
            &mut losses, &mut staging,
        ).unwrap();
        assert_eq!(range, Some(reference_start..bytes.len()));
        assert_eq!(image.legacy_file_path, "image.png");
        assert!(image.hidden && image.filtered && !image.grayscale);
        let reference = image.file_reference.as_ref().unwrap();
        assert_eq!(reference.full_path, "/images/source.png");
        let surviving = image.legacy_file_path.len() + image_reference_bytes(reference);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, RhinoLossCode::IntegrityFailure.kind());
        let provenance = losses[0].provenance.as_ref().unwrap();
        assert_eq!(provenance.offset, u64::try_from(reference_start).unwrap());
        assert_eq!(provenance.tag.as_deref(), Some("VIEW/TRACE_IMAGE/FILE_REFERENCE"));
        drop(losses);
        let released = ctx.reserve_scoped(STORAGE_LIMIT - u64::try_from(surviving).unwrap(),
            "trace image temporary diagnostics released").unwrap();
        drop(released);
        drop(image);
    } else {
        let (image, range) = super::super::parse_wallpaper(
            &ctx, &bytes, 0..bytes.len(), archive, &mut losses, &mut staging,
        ).unwrap();
        assert_eq!(range, Some(reference_start..bytes.len()));
        assert_eq!(image.legacy_file_path, "image.png");
        assert!(image.grayscale && !image.hidden);
        let reference = image.file_reference.as_ref().unwrap();
        assert_eq!(reference.full_path, "/images/source.png");
        let surviving = image.legacy_file_path.len() + image_reference_bytes(reference);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, RhinoLossCode::IntegrityFailure.kind());
        let provenance = losses[0].provenance.as_ref().unwrap();
        assert_eq!(provenance.offset, u64::try_from(reference_start).unwrap());
        assert_eq!(provenance.tag.as_deref(), Some("VIEW/WALLPAPER/FILE_REFERENCE"));
        drop(losses);
        let released = ctx.reserve_scoped(STORAGE_LIMIT - u64::try_from(surviving).unwrap(),
            "wallpaper temporary diagnostics released").unwrap();
        drop(released);
        drop(image);
    }
    drop(staging);
    ctx.finish_session().unwrap();
}

#[test]
fn trace_image_file_reference_releases_temporary_diagnostics() {
    assert_image_diagnostics_released(true);
}

#[test]
fn wallpaper_file_reference_releases_temporary_diagnostics() {
    assert_image_diagnostics_released(false);
}
