// SPDX-License-Identifier: Apache-2.0
//! Wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper runs one internal parser over arbitrary bytes. The fuzz target
//! checks that no input panics. Resource-aware scanners can return a refusal.
#![doc(hidden)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Exercise outer-container scanning.
pub fn container(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    drop(crate::container::scan(&ctx, root)?);
    Ok(())
}

/// Exercise embedded Parasolid stream extraction.
pub fn parasolid(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    drop(crate::parasolid::extract_streams_with_offsets(data, &ctx)?);
    Ok(())
}

/// Exercise spline-curve carrier scanning.
pub fn spline_curves(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    drop(crate::brep::spline::scan_curve_carriers(
        &ctx,
        data,
        &mut Vec::new(),
    )?);
    Ok(())
}

/// Exercise spline-surface carrier scanning.
pub fn spline_surfaces(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    drop(crate::brep::spline::scan_surface_carriers(
        &ctx,
        data,
        &mut Vec::new(),
    )?);
    Ok(())
}

/// Exercise topology record scanning.
pub fn topology(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    drop(crate::brep::topology::scan(&ctx, data)?);
    Ok(())
}

/// Exercise entity record scanning.
pub fn entity(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    drop(crate::brep::entity::scan_metadata(&ctx, data, false)?);
    Ok(())
}

/// Exercise `PMISemanticDataDB` `MessagePack` parse/patch/reparse.
///
/// Invariant: malformed input never panics; successful records keep patch
/// offsets consistent across an in-place value edit and reparse.
pub fn pmi(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    let mut losses = Vec::new();
    let records = crate::pmi::parse_payload(&ctx, data, &mut losses)?;
    for record in records {
        if record.item_count.get() != 1 {
            continue;
        }
        let Ok(start) = usize::try_from(record.value_offset) else {
            continue;
        };
        let Some(end) = start.checked_add(8) else {
            continue;
        };
        if data.get(start..end).is_none() {
            continue;
        }
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(data.len()),
            "patch SLDPRT PMI fuzz payload",
        )?;
        let _patched_reservation = ctx.reserve_scoped(
            cadmpeg_core::decode::u64_from_index(data.len()),
            "patch SLDPRT PMI fuzz payload",
        )?;
        let mut patched = Vec::new();
        patched.try_reserve_exact(data.len()).map_err(|_| {
            ctx.refuse_codec_limit("patch SLDPRT PMI fuzz payload", u64::MAX - 1, u64::MAX)
        })?;
        patched.extend_from_slice(data);
        let edited = f64::from_bits(record.value.get().to_bits() ^ 1);
        patched[start..end].copy_from_slice(&edited.to_be_bytes());
        let mut again_losses = Vec::new();
        let again = crate::pmi::parse_payload(&ctx, &patched, &mut again_losses)?;
        if let Some(parsed) = again.iter().find(|candidate| candidate.guid == record.guid) {
            assert_eq!(parsed.value.get().to_bits(), edited.to_bits());
            assert_eq!(parsed.value_offset, record.value_offset);
            assert_eq!(parsed.precision_offset, record.precision_offset);
            assert_eq!(parsed.basic_offset, record.basic_offset);
            assert_eq!(parsed.inspection_offset, record.inspection_offset);
            assert_eq!(parsed.reference_only_offset, record.reference_only_offset);
            assert_eq!(parsed.display_text_offset(), record.display_text_offset());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    #[test]
    fn parasolid_fuzz_wrapper_returns_declared_expansion_refusal() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"x").expect("compressed frame fixture");
        let member = encoder.finish().expect("finish compressed frame fixture");
        let mut payload = vec![
            0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89,
            0xef, 0x99,
        ];
        payload.extend_from_slice(&(512_u32 * 1024 * 1024 + 1).to_le_bytes());
        payload.extend_from_slice(
            &u32::try_from(member.len())
                .expect("small compressed fixture")
                .to_le_bytes(),
        );
        payload.extend_from_slice(&member);
        let error =
            super::parasolid(&payload).expect_err("declared expansion exceeds service policy");
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::DecompressedBytes
        ));
    }
}
