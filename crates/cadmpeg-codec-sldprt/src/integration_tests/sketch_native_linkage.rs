// SPDX-License-Identifier: Apache-2.0
//! Nested Parasolid topology is neutral; only its actual source lane is native.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::units::FinitePoint2;

use crate::test_support::history::{
    sldprt_with_compressed_nested_sketch_profile, sldprt_with_nested_sketch_profiles,
};
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

fn assert_profile_owners(source: Vec<u8>, count: usize) {
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("actual nested Parasolid source");
    let ir = decoded.ir();
    assert_eq!(ir.model.sketches.len(), count);
    assert_eq!(ir.model.sketch_entities.len(), 3 * count);
    assert_eq!(ir.model.sketch_constraints.len(), 3 * count);
    let native = ir.native.namespace("sldprt").expect("held native source");
    let lanes = &native.arenas()["feature_input_lanes"];
    assert_eq!(lanes.len(), 1);
    let native_ids = native
        .arenas()
        .values()
        .flatten()
        .map(|record| record.id())
        .collect::<std::collections::BTreeSet<_>>();
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(1000.0, 0.0),
        Point2::new(0.0, 1000.0),
    ]
    .map(|point| FinitePoint2::new(point).expect("finite original triangle coordinates"));
    for sketch in &ir.model.sketches {
        assert_eq!(sketch.native_ref.as_deref(), Some(lanes[0].id()));
        assert!(native_ids.contains(sketch.native_ref.as_deref().expect("actual lane")));
        assert_eq!(sketch.configuration.as_deref(), Some("0"));
        assert_eq!(sketch.profiles.len(), 1);
        assert_eq!(sketch.profiles[0].len(), 3);
        let entities = ir
            .model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>();
        assert_eq!(entities.len(), 3);
        for (index, entity) in entities.into_iter().enumerate() {
            assert!(entity.native_ref.is_none());
            assert_eq!(
                entity.geometry.definition(),
                &SketchGeometryDefinition::Line {
                    start: points[index],
                    end: points[(index + 1) % 3],
                }
            );
            assert_eq!(entity.endpoint_refs.len(), 2);
            assert!(entity
                .endpoint_refs
                .iter()
                .all(|reference| reference.contains(":sldprt:brep:point#")));
        }
    }
    let audit = cadmpeg_test_support::native_linkage::assert_native_linkage(ir);
    assert!(audit.checked_native_refs >= count);
    assert!(audit.findings.is_empty());
}

#[test]
fn nested_profile_parent_links_resolve_without_invented_native_edge_records() {
    assert_profile_owners(sldprt_with_nested_sketch_profiles(&triangle_body(), 2), 2);
}

#[test]
fn compressed_profile_parent_links_resolve_without_invented_native_edge_records() {
    assert_profile_owners(
        sldprt_with_compressed_nested_sketch_profile(&triangle_body()),
        1,
    );
}
