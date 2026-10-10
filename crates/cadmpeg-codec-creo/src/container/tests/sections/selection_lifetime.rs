// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use super::super::super::{scan_bytes, Section};

fn assert_selection_peak(section_name: &str, extra_sections: usize) {
    let mut sections = vec![(section_name, b"srf_array\0".to_vec())];
    let opaque_names = ["Other0", "Other1", "Other2", "Other3"];
    for name in opaque_names.iter().take(extra_sections) {
        sections.push((*name, vec![0]));
    }
    let bytes = crate::test_support::build_prt_raw("c", &sections);
    let cap = crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |allowed| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = allowed;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        ctx.with_scoped_storage("selection lifetime parent", || scan_bytes(&ctx, bytes.as_slice()))
            .map(|(scan, storage)| drop((scan, storage)))
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    let (scan, storage) = ctx.with_scoped_storage("selection lifetime parent", ||
        scan_bytes(&ctx, bytes.as_slice())).expect("actual section-selection peak");
    assert_eq!(scan.framing.version_line, "#UGC:2 P c");
    assert_eq!(scan.framing.sections.iter().map(Section::raw_name).collect::<Vec<_>>(),
        sections.iter().map(|(name, _)| *name).collect::<Vec<_>>());
    assert!(scan.loop_arrays.frames.is_empty());
    assert!(scan.surfaces.rows.is_empty());
    assert!(scan.surfaces.nonvisible_rows.is_empty());
    let live_bytes = scan.framing.version_line.capacity()
        + scan.framing.sections.iter().map(|section| section.raw_name.capacity()).sum::<usize>()
        + scan.framing.sections.capacity() * std::mem::size_of::<Section>();
    let live_bytes = u64::try_from(live_bytes).expect("live framing storage");
    let probe = ctx.reserve_scoped(cap.checked_sub(live_bytes).expect("peak includes live framing"),
        "after section selections").expect("only surviving framing storage remains");
    drop(probe);
    drop(scan);
    drop(storage);
    ctx.reserve_scoped(cap, "after framing release").expect("every section allocation released");
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
