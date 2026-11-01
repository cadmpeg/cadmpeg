// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn source_native_curves_keep_the_whole_sketch_profile_incomplete() {
    let raw = crate::test_support::sketch_curves::open_cubic_entities();
    let sources = cadmpeg_core::decode::unique_index(raw.iter().map(|source| {
        (
            (
                source.identity.segment_token.as_str(),
                source.identity.record_ordinal,
            ),
            source,
        )
    }));
    for native in [false, true] {
        let geometry = if native {
            SketchGeometry::native(cadmpeg_core::nonblank_literal!("spline"))
        } else {
            crate::sketch::project_geometry(&raw[2], &sources).expect("resolved cubic")
        };
        let projected = [SketchEntity::new(
            SketchEntityId::mint("inventor:design:sketch-entity#generated-7").expect("entity"),
            SketchId::mint("inventor:design:sketch#generated-2").expect("sketch"),
            geometry,
        )
        .with_native_ref(Some(raw[2].id()))];
        let index = ProfileIndex::new(&[], &[], &[], &raw, &projected);
        assert_eq!(
            index.incomplete_sketches.contains_key(&("generated", 2)),
            native
        );
    }
}
