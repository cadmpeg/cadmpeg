// SPDX-License-Identifier: Apache-2.0
//! Sketch feature dependency bindings for later operations.

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, ProfileRef};
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeMap;

#[test]
fn sketch_inputs_bind_owner_dependencies_after_sketch_conversion() {
    use cadmpeg_ir::features::{BooleanOp, LoftSection, PathRef, SheetMetalThicknessSide};
    use cadmpeg_ir::sketches::SpatialSketchId;

    let feature = |id: &str, ordinal, definition| Feature {
        product_definition: None,
        needs_recompute: false,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    let planar_sketch = SketchId::mint("synthetic:test:id#f3d:sketch:planar").unwrap();
    let spatial_sketch = SpatialSketchId::mint("synthetic:test:id#f3d:sketch:spatial").unwrap();
    let planar_feature = feature(
        "synthetic:test:id#f3d:feature:planar-sketch",
        0,
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(planar_sketch.clone())),
        }),
    );
    let spatial_feature = feature(
        "synthetic:test:id#f3d:feature:spatial-sketch",
        1,
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
            sketch: Some(spatial_sketch.clone()),
        }),
    );
    let base_flange = feature(
        "synthetic:test:id#f3d:feature:base-flange",
        2,
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::PlanarProfileRef::Sketch(planar_sketch.clone()),
            thickness: (cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap()).into(),
            side: SheetMetalThicknessSide::Forward,
        }),
    );
    let loft = feature(
        "synthetic:test:id#f3d:feature:loft",
        3,
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: vec![
                LoftSection::Profile(
                    ProfileRef::spatial_sketch_profiles(spatial_sketch.clone(), vec![2]).unwrap(),
                ),
                LoftSection::Profile(
                    ProfileRef::spatial_sketch_profiles(spatial_sketch.clone(), vec![5]).unwrap(),
                ),
            ],
            guidance: cadmpeg_ir::features::LoftGuidance::Centerline(PathRef::Sketch(
                planar_sketch,
            )),
            op: BooleanOp::Join,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }),
    );
    let expected_dependencies = [spatial_feature.id.clone(), planar_feature.id.clone()];
    let mut features = vec![planar_feature, spatial_feature, base_flange, loft];

    {
        crate::design::feature_project::bind_sketch_feature_geometry(
            &mut features,
            &[],
            &[],
            &[],
            &[],
        )
    }
    .unwrap();

    assert_eq!(
        features[2].dependencies.as_slice(),
        [features[0].id.clone()]
    );
    assert_eq!(features[3].dependencies.as_slice(), expected_dependencies);
}
