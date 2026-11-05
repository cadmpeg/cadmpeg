// SPDX-License-Identifier: Apache-2.0
//! Source-less sketch text authoring.

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchFontWeight, SketchGeometry, SketchGeometryDefinition,
    SketchPlacement, SketchProfiles, SketchTextHorizontalAlignment, SketchTextVerticalAlignment,
    TextPlacement,
};

#[test]
fn neutral_sketch_text_survives_source_less_synthesis() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.sketches.push(Sketch {
        id: "authored:model:sketch#1".try_into().unwrap(),
        name: Some("Sketch_1".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: cadmpeg_core::text::NonBlankString::new("Hello").unwrap(),
        font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
        font_weight: SketchFontWeight::Regular,
        height: Length::new(4.0).unwrap(),
        width_factor: None,
        placement: Some(TextPlacement {
            anchor: Point2::new(10.0, 20.0),
            rotation: Angle::new(0.0).unwrap(),
        }),
        horizontal_alignment: Some(SketchTextHorizontalAlignment::Left),
        vertical_alignment: Some(SketchTextVerticalAlignment::Top),
    })
    .unwrap();
    let entity = SketchEntity::new(
        "authored:model:sketch-entity#text".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        geometry.clone(),
    );
    let id = entity.id().clone();
    ir.model.sketch_entities.push(entity);
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.sketch_entities.len(), 1);
    assert_eq!(output.ir().model.sketch_entities[0].geometry, geometry);
    assert!(report.losses.iter().any(|loss| {
        loss.message.contains(id.as_str())
            && loss.code.local_code() == "record.residual-unavailable"
    }));
    let rotated = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: cadmpeg_core::text::NonBlankString::new("Rotated").unwrap(),
        font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
        font_weight: SketchFontWeight::Regular,
        height: Length::new(4.0).unwrap(),
        width_factor: None,
        placement: Some(TextPlacement {
            anchor: Point2::new(10.0, 20.0),
            rotation: Angle::new(0.25).unwrap(),
        }),
        horizontal_alignment: Some(SketchTextHorizontalAlignment::Left),
        vertical_alignment: Some(SketchTextVerticalAlignment::Top),
    })
    .unwrap();
    let omitted = SketchEntity::new(
        "authored:model:sketch-entity#rotated".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        rotated,
    );
    let omitted_id = omitted.id().clone();
    ir.model.sketch_entities.push(omitted);
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.sketch_entities.len(), 1);
    assert_eq!(output.ir().model.sketch_entities[0].geometry, geometry);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(omitted_id.as_str())));
}
