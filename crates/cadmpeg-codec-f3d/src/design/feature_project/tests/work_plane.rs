// SPDX-License-Identifier: Apache-2.0

#[test]
fn work_plane_projects_three_point_vertex_recipes() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::feature::work_geometry::{DesignVertexRecipe, DesignWorkPlaneConstruction};

    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let recipe = |record_index, vertex| {
        DesignVertexRecipe::try_new(
            crate::records::feature::work_geometry::DesignVertexRecipeDraft {
                record_index,
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                    .unwrap(),
                paired_byte_offset: u64::from(record_index) + 16,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                recipe_record_index: record_index + 3,
                recipe_record_byte_offset: u64::from(record_index) + 32,
                recipe_id: format!("f3d:test:recipe#{record_index}"),
                recipe_prefix_offset: u64::from(record_index) + 43,
                recipe_prefix_bytes: Vec::new(),
                recipe_references: Vec::new(),
                recipe_program_offset: 4,
                recipe_program: vec![0],
                resolution: Some(
                    crate::records::feature::work_geometry::DesignVertexResolution::new(4, vertex)
                        .unwrap(),
                ),
                next_record_index: record_index + 5,
                next_byte_offset: u64::from(record_index) + 200,
            },
        )
        .unwrap()
    };
    let mut plane =
        DesignParameterScope::empty("f3d:test:scope#20", DesignFeatureKind::WorkPlane, 20);
    plane.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    if let Some(frame) = plane.work_plane_frame_mut() {
        frame.work_plane_construction = Some(
            DesignWorkPlaneConstruction::try_new(
                21,
                Box::new([recipe(22, 43), recipe(27, 64), recipe(32, 84)]),
            )
            .unwrap(),
        );
    }
    let transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.into();
    let FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { points, .. }) =
        super::super::project_work_plane(&plane, transform).unwrap()
    else {
        panic!("expected a three-point datum plane");
    };
    // Each recipe resolves vertex `slot` of input state 4 and keeps its recipe ID.
    let feature = crate::ids::neutral_feature_id(&plane);
    let prefix = crate::ids::feature_input_prefix(&feature, 4);
    let expected = [(22, 43), (27, 64), (32, 84)].map(|(record_index, slot)| {
        cadmpeg_ir::features::VertexSelection::historical(
            crate::ids::feature_input_topology_id(&feature, 4),
            crate::ids::history_input_vertex_id(&prefix, slot),
            format!("f3d:test:recipe#{record_index}"),
        )
        .unwrap()
    });
    assert_eq!(*points, expected);
}
