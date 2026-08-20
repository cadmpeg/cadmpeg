// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use super::*;

#[test]
fn sketch_coordinate_pairs_are_retained_as_native_entities_without_roles() {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-9".to_string(),
        section_link: "section".to_string(),
        ordinal: 9,
        value: "SKETCH".to_string(),
        object_indices: [None; 4],
        raw_object_indices: Default::default(),
        source_offset: 40,
    };
    let pair = crate::native::features::FeatureSketchPayloadCoordinatePair {
        id: "nx:feature-history:sketch-payload-coordinate-pair#section-9-0000000000".to_string(),
        operation_label: label.id.clone(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        values: [12.5, -3.0],
        raw_values: [[0; 8]; 2],
        payload_offset: 12,
        value_payload_offsets: [20, 28],
        source_offset: 51,
        value_source_offsets: [59, 67],
        discriminator: vec![8, 2, 3, 1, 3, 1],
    };
    let coordinate_pairs = [&pair];
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = AnnotationBuilder::new();
    let stream = annotations.stream("nx:container");
    let sketch = super::super::attach_sketch_graph(
        &mut ir,
        &label,
        &super::super::SketchSources {
            point_uses: &[],
            point_groups: &[],
            points: &[],
            payload_scalars: &[],
            coordinate_pairs: &coordinate_pairs,
        },
        &mut annotations,
        stream,
    )
    .expect("one complete coordinate pair retains a native sketch graph");

    assert_eq!(ir.model.sketches[0].id, sketch);
    assert!(matches!(
        ir.model.sketches[0].placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved
    ));
    assert_eq!(ir.model.sketch_entities.len(), 1);
    assert_eq!(
        ir.model.sketch_entities[0].id.0,
        "nx:feature-history:sketch-entity#coordinate-pair-section-9-0000000000"
    );
    assert!(cadmpeg_ir::ids::is_valid_identity(
        &ir.model.sketch_entities[0].id.0
    ));
    assert_eq!(
        ir.model.sketch_entities[0].native_ref.as_deref(),
        Some(pair.id.as_str())
    );
    assert!(matches!(
        &ir.model.sketch_entities[0].geometry,
        SketchGeometry::Native { native_kind } if native_kind == "nx-coordinate-pair"
    ));
}
