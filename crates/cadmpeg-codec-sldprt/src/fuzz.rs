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
pub fn spline_curves(data: &[u8]) {
    drop(crate::brep::spline::scan_curve_carriers(
        data,
        &mut Vec::new(),
    ));
}

/// Exercise spline-surface carrier scanning.
pub fn spline_surfaces(data: &[u8]) {
    drop(crate::brep::spline::scan_surface_carriers(
        data,
        &mut Vec::new(),
    ));
}

/// Exercise topology record scanning.
pub fn topology(data: &[u8]) {
    drop(crate::brep::topology::scan(data));
}

/// Exercise entity record scanning.
pub fn entity(data: &[u8]) {
    drop(crate::brep::entity::scan_metadata(data, false));
}

/// Exercise `PMISemanticDataDB` `MessagePack` parse/patch/reparse.
///
/// Invariant: malformed input never panics; successful records keep patch
/// offsets consistent across an in-place value edit and reparse.
pub fn pmi(data: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())?;
    let mut losses = Vec::new();
    let parsed = crate::pmi::parse_payload(&ctx, root, &mut losses)?;
    let mut fidelity = cadmpeg_ir::SourceFidelity::default();
    crate::pmi::layout::install(&ctx, parsed.pointers, &mut fidelity)?;
    for record in parsed.records {
        if record.item_count.get() != 1 {
            continue;
        }
        let pointers = fidelity
            .record_layout()
            .get(&record.id)
            .ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("PMI fuzz record lacks source layout")
            })?
            .pointers();
        let Some(start) = pointers
            .get("value_offset")
            .copied()
            .and_then(|offset| usize::try_from(offset).ok())
        else {
            continue;
        };
        let Some(end) = start.checked_add(8) else {
            continue;
        };
        if data.get(start..end).is_none() {
            continue;
        }
        let mut patched = data.to_vec();
        let edited = f64::from_bits(record.value.get().to_bits() ^ 1);
        patched[start..end].copy_from_slice(&edited.to_be_bytes());
        let mut again_losses = Vec::new();
        let (again_ctx, again_root) =
            DecodeContext::from_root_bytes(&patched, &arena, &DecodePolicy::service())?;
        let again = crate::pmi::parse_payload(&again_ctx, again_root, &mut again_losses)?;
        let mut again_fidelity = cadmpeg_ir::SourceFidelity::default();
        crate::pmi::layout::install(&again_ctx, again.pointers, &mut again_fidelity)?;
        if let Some(parsed) = again
            .records
            .iter()
            .find(|candidate| candidate.guid == record.guid)
        {
            let again_layout = again_fidelity.record_layout().get(&parsed.id);
            assert!(again_layout.is_some(), "reparsed PMI layout");
            let again_pointers = again_layout.map(|layout| layout.pointers());
            assert_eq!(parsed.value.get().to_bits(), edited.to_bits());
            assert_eq!(
                again_pointers.and_then(|pointers| pointers.get("value_offset")),
                pointers.get("value_offset")
            );
            assert_eq!(
                again_pointers.and_then(|pointers| pointers.get("precision_offset")),
                pointers.get("precision_offset")
            );
            assert_eq!(
                again_pointers.and_then(|pointers| pointers.get("basic_offset")),
                pointers.get("basic_offset")
            );
            assert_eq!(
                again_pointers.and_then(|pointers| pointers.get("inspection_offset")),
                pointers.get("inspection_offset")
            );
            assert_eq!(
                again_pointers.and_then(|pointers| pointers.get("reference_only_offset")),
                pointers.get("reference_only_offset")
            );
            assert_eq!(
                again_pointers.and_then(|pointers| pointers.get("display_text_offset")),
                pointers.get("display_text_offset")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn parasolid_fuzz_wrapper_skips_frame_above_local_cap() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&crate::test_support::parasolid::parasolid_payload(
                "partition body",
                "SCH_SW_33103_11000",
            ))
            .expect("compressed frame fixture");
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
        super::parasolid(&payload).expect("a frame above the local cap is skipped");
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::service())
            .expect("root");
        let streams = crate::parasolid::extract_streams_with_offsets(&payload, &ctx)
            .expect("a frame above the local cap is skipped");
        assert!(streams.streams.is_empty());
        assert!(!streams.probe_capped);
    }
}
