// SPDX-License-Identifier: Apache-2.0
use crate::design::geometry::closed_sketch_profiles;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
const EPS_JOINED_LINE_ENDPOINTS: f64 = 1.0e-6;

#[test]
fn duplicate_edge_ids_keep_bounded_faces_in_source_order() {
    let sketch = SketchId::mint("f3d:model:sketch#ordered-profile").unwrap();
    let line = |name: &str, start: (f64, f64), end: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    };
    let entities = [
        line("bottom", (0.0, 0.0), (1.0, 0.0)),
        line("bottom", (1.0, 0.0), (2.0, 0.0)),
        line("right", (2.0, 0.0), (2.0, 1.0)),
        line("top-right", (2.0, 1.0), (1.0, 1.0)),
        line("top-left", (1.0, 1.0), (0.0, 1.0)),
        line("left", (0.0, 1.0), (0.0, 0.0)),
        line("divider", (1.0, 0.0), (1.0, 1.0)),
    ];
    let profiles = crate::test_support::with_decode_context(|ctx| {
        closed_sketch_profiles(ctx, &sketch, &entities, EPS_JOINED_LINE_ENDPOINTS)
    })
    .unwrap();
    let members: Vec<Vec<_>> = profiles
        .iter()
        .map(|profile| {
            profile
                .iter()
                .map(|member| (member.entity.as_str(), member.reversed))
                .collect()
        })
        .collect();
    assert_eq!(
        members,
        [
            vec![
                ("synthetic:test:id#bottom", false),
                ("synthetic:test:id#divider", false),
                ("synthetic:test:id#top-left", false),
                ("synthetic:test:id#left", false)
            ],
            vec![
                ("synthetic:test:id#bottom", false),
                ("synthetic:test:id#right", false),
                ("synthetic:test:id#top-right", false),
                ("synthetic:test:id#divider", true)
            ],
        ]
    );
}
