// SPDX-License-Identifier: Apache-2.0
//! Source-less planar sketch NURBS.

use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchPlacement, SketchProfiles};

#[test]
fn rational_sketch_nurbs_survives_source_less_synthesis() {
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
    let curve = PcurveNurbs::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 5.0),
            Point2::new(20.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .unwrap();
    let geometry = SketchGeometry::nurbs(curve);
    let entity = SketchEntity::new(
        "authored:model:sketch-entity#spline".try_into().unwrap(),
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
    let mut edited = output.ir().clone();
    let edited_curve = PcurveNurbs::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 5.0),
            Point2::new(30.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .unwrap();
    let edited_geometry = SketchGeometry::nurbs(edited_curve);
    edited.model.sketch_entities[0].geometry = edited_geometry.clone();
    let (edited_output, _) = super::super::source_less_analytic::synthesize_document(&edited, None);
    assert_eq!(edited_output.ir().model.sketch_entities.len(), 1);
    assert_eq!(
        edited_output.ir().model.sketch_entities[0].geometry,
        edited_geometry
    );
    let native = crate::test_support::native_test::f3d_native(edited_output.ir());
    assert!(!native.sketch_curve_identities.is_empty());
    for record in &native.sketch_curve_identities {
        if let Some(crate::records::sketch_geometry::SketchCurveGeometry::Nurbs {
            geometry, ..
        }) = &record.geometry
        {
            assert_eq!(geometry.poles().points().last().unwrap().x, 30.0);
        }
    }
    let invalid_weights = PcurveNurbs::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(30.0, 0.0),
            Point2::new(40.0, 5.0),
            Point2::new(50.0, 0.0),
        ],
        Some(vec![1.0, -0.5, 1.0]),
        false,
    )
    .unwrap();
    let omitted = SketchEntity::new(
        "authored:model:sketch-entity#invalid-weight"
            .try_into()
            .unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::nurbs(invalid_weights),
    );
    let omitted_id = omitted.id().clone();
    ir.model.sketch_entities.push(omitted);
    let text = cadmpeg_ir::sketches::SketchGeometry::try_from(
        cadmpeg_ir::sketches::SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::new("Label").unwrap(),
            font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
            font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
            height: cadmpeg_ir::scalar::Length::new(4.0).unwrap(),
            width_factor: None,
            placement: Some(cadmpeg_ir::sketches::TextPlacement {
                anchor: Point2::new(10.0, 20.0),
                rotation: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            }),
            horizontal_alignment: Some(cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Left),
            vertical_alignment: Some(cadmpeg_ir::sketches::SketchTextVerticalAlignment::Top),
        },
    )
    .unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#text".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        text.clone(),
    ));
    let (output, report) = super::super::source_less_analytic::synthesize_document(&ir, None);
    assert_eq!(output.ir().model.sketch_entities.len(), 2);
    assert!(output
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|row| row.geometry == geometry));
    assert!(output
        .ir()
        .model
        .sketch_entities
        .iter()
        .any(|row| row.geometry == text));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains(omitted_id.as_str())));
}
