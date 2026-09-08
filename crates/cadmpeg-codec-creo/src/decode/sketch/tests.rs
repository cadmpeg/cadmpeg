// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::sketches::SketchGeometry;

use super::*;

#[test]
fn coincident_endpoint_conic_materializes_as_a_full_ellipse() {
    let entity = crate::feature::FeatureSavedEntity::Conic(crate::feature::FeatureSavedConic {
        entity_id: 2,
        endpoints: [[Some(0.0), Some(1.0), Some(0.0)]; 2],
        parameters: [Some(0.0), None],
        coefficients: [Some(35.0), Some(27.0)],
        local_system: Some([
            0.8, -0.6, 0.0, 0.6, 0.8, 0.0, 0.0, 0.0, 1.0, 128.0, 75.0, 0.0,
        ]),
        body: Vec::new(),
        offset: 40,
    });

    let Some((2, SketchGeometry::Ellipse { bounds, .. }, 40)) =
        saved_section_entity_geometry(&entity)
    else {
        panic!("full ellipse");
    };
    assert_eq!(bounds, None);
}
