// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use super::super::super::{scan_bytes, ScannedSection, Section};

fn assert_selection_peak(section_name: &str, extra_sections: usize) {
    let mut sections = vec![(section_name, b"srf_array\0".to_vec())];
    let opaque_names = ["Other0", "Other1", "Other2", "Other3"];
    for name in opaque_names.iter().take(extra_sections) {
        sections.push((*name, vec![0]));
    }
    let bytes = crate::test_support::build_prt_raw("c", &sections);
    let version_bytes = "#UGC:2 P c".len();
    let name_bytes: usize = sections.iter().map(|(name, _)| name.len()).sum();
    let scanned_slots = sections.len().max(4);
    let retained_slots = sections.len().next_power_of_two().max(4);
    let retained_overlap = if retained_slots > 4 { retained_slots / 2 } else { 0 };
    let scanned_bytes = std::mem::size_of::<ScannedSection<'_>>();
    let section_bytes = std::mem::size_of::<Section>();
    // Original roster, one model/nonvisible roster, and one loop roster.
    // Each selected singleton reserves four slots and copies its name once.
    let selection_peak = version_bytes + name_bytes
        + (scanned_slots + 8) * scanned_bytes + 2 * section_name.len();
    // Final framing growth owns its new slots while the old allocation moves.
    let framing_peak = version_bytes + name_bytes + scanned_slots * scanned_bytes
        + (retained_slots + retained_overlap) * section_bytes;
    let cap = u64::try_from(selection_peak.max(framing_peak)).expect("fixed fixture peak");
    let final_bytes = u64::try_from(version_bytes + name_bytes
        + scanned_slots * scanned_bytes + retained_slots * section_bytes)
        .expect("original roster charge and surviving framing");
    for allowed in [cap - 1, cap] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let result = ctx.with_scoped_storage("selection lifetime parent", ||
            scan_bytes(&ctx, bytes.as_slice()));
        let original = if allowed < cap {
            let Err(CodecError::ResourceLimit(refusal)) = result else {
                panic!("one byte below the actual peak must refuse");
            };
            let (operation, additional) = if selection_peak >= framing_peak {
                ("creo copied section names", section_name.len())
            } else {
                ("creo retained scan sections", retained_overlap * section_bytes)
            };
            assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(refusal.operation, operation);
            assert_eq!((refusal.used, refusal.additional),
                (cap - u64::try_from(additional).expect("peak addition"),
                 u64::try_from(additional).expect("peak addition")));
            refusal
        } else {
            let (scan, storage) = result.expect("last-use refunds admit the actual peak");
            assert_eq!(scan.framing.version_line, "#UGC:2 P c");
            assert_eq!(scan.framing.sections.iter().map(Section::raw_name).collect::<Vec<_>>(),
                sections.iter().map(|(name, _)| *name).collect::<Vec<_>>());
            assert!(scan.loop_arrays.frames.is_empty());
            assert!(scan.surfaces.rows.is_empty());
            assert!(scan.surfaces.nonvisible_rows.is_empty());
            let refusal = ctx.reserve_scoped_limit(cap + 1, "after section selections")
                .expect_err("probe live parent receipt");
            assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(refusal.used, final_bytes);
            drop(scan);
            drop(storage);
            refusal
        };
        assert!(matches!(scan_bytes(&ctx, bytes.as_slice()),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn loop_section_selection_drops_before_later_scan_allocations() {
    for name in ["VisibGeom", "NovisGeom"] {
        assert_selection_peak(name, 0);
    }
}

#[test]
fn geometry_section_selections_drop_before_retained_framing_growth() {
    for name in ["VisibGeom", "NovisGeom"] {
        assert_selection_peak(name, 4);
    }
}
