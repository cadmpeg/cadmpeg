// SPDX-License-Identifier: Apache-2.0
//! Wrappers over internal parsers for the `cadmpeg-fuzz` targets.
//!
//! Each wrapper runs one internal parser over arbitrary bytes as an expression
//! statement. None of the six scanning parsers states a refusal -- each answers
//! a scan, a collection or a fact table for every input -- so there is no
//! `Result` here and nothing is discarded: the answer is a value the wrapper
//! has no use for, and the contract a target checks is only that no input
//! panics. `pmi` states its own reparse invariant instead.
#![doc(hidden)]

/// Exercise outer-container scanning.
pub fn container(data: &[u8]) {
    crate::container::scan_bytes(data);
}

/// Exercise embedded Parasolid stream extraction.
pub fn parasolid(data: &[u8]) {
    crate::parasolid::extract_streams_with_offsets(data);
}

/// Exercise spline-curve carrier scanning.
pub fn spline_curves(data: &[u8]) {
    crate::brep::spline::scan_curve_carriers(data, &mut Vec::new());
}

/// Exercise spline-surface carrier scanning.
pub fn spline_surfaces(data: &[u8]) {
    crate::brep::spline::scan_surface_carriers(data, &mut Vec::new());
}

/// Exercise topology record scanning.
pub fn topology(data: &[u8]) {
    crate::brep::topology::scan(data);
}

/// Exercise entity record scanning.
pub fn entity(data: &[u8]) {
    crate::brep::entity::scan_metadata(data, false);
}

/// Exercise `PMISemanticDataDB` `MessagePack` parse/patch/reparse.
///
/// Invariant: malformed input never panics; successful records keep patch
/// offsets consistent across an in-place value edit and reparse.
pub fn pmi(data: &[u8]) {
    let mut losses = Vec::new();
    let records = crate::pmi::parse_payload(data, &mut losses);
    for record in records {
        if record.item_count != 1 {
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
        let mut patched = data.to_vec();
        let edited = f64::from_bits(record.value.to_bits() ^ 1);
        patched[start..end].copy_from_slice(&edited.to_be_bytes());
        let mut again_losses = Vec::new();
        let again = crate::pmi::parse_payload(&patched, &mut again_losses);
        if let Some(parsed) = again.iter().find(|candidate| candidate.guid == record.guid) {
            assert_eq!(parsed.value.to_bits(), edited.to_bits());
            assert_eq!(parsed.value_offset, record.value_offset);
            assert_eq!(parsed.precision_offset, record.precision_offset);
            assert_eq!(parsed.basic_offset, record.basic_offset);
            assert_eq!(parsed.inspection_offset, record.inspection_offset);
            assert_eq!(parsed.reference_only_offset, record.reference_only_offset);
            assert_eq!(parsed.display_text_offset(), record.display_text_offset());
        }
    }
}
