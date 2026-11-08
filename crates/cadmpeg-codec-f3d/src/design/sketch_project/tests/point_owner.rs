use super::*;

fn point(owner_reference: Option<u32>) -> SketchPoint {
    SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:Design/BulkStream.dat:point#14".into(),
        record_index: 14,
        owner_reference,
        class_tag: "413".to_owned().try_into().unwrap(),
        byte_offset: Some(14),
        coordinate_offset: Some(0),
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            14,
            crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
            None,
            0.0,
        ),
        paired_reference: 15,
        coordinates: Point2::new(0.0, 0.0),
    })
    .unwrap()
}

#[test]
fn point_without_owner_placement_projects_into_an_unresolved_sketch() {
    let result = crate::test_support::with_decode_context(|ctx| {
        project_sketch_design(
            ctx,
            &SketchProjectInputs::new(&[], &[point(Some(42))], &[], &[], &[]),
            EPS_POINT_PROJECTION,
        )
    })
    .unwrap();
    let sketch = crate::ids::neutral_owner_sketch_id("f3d:Design/BulkStream.dat", 42);
    assert_eq!(result.sketches.len(), 1);
    assert_eq!(result.sketches[0].id, sketch);
    assert_eq!(
        result.sketches[0].placement,
        cadmpeg_ir::sketches::SketchPlacement::Unresolved {}
    );
    assert_eq!(result.entities.len(), 1);
    assert_eq!(result.entities[0].sketch, sketch);
    assert_eq!(
        result.entities[0].native_ref.as_deref(),
        Some("f3d:Design/BulkStream.dat:point#14")
    );
    assert!(result.losses.is_empty());
}

#[test]
fn unowned_point_stays_native() {
    let result = crate::test_support::with_decode_context(|ctx| {
        project_sketch_design(
            ctx,
            &SketchProjectInputs::new(&[], &[point(None)], &[], &[], &[]),
            EPS_POINT_PROJECTION,
        )
    })
    .unwrap();
    assert!(result.sketches.is_empty());
    assert!(result.entities.is_empty());
    assert!(result.losses.is_empty());
}
