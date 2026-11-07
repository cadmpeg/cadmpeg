use super::*;

#[test]
fn point_without_owner_placement_reports_the_dropped_point() {
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:Design/BulkStream.dat:point#14".into(),
        record_index: 14,
        owner_reference: Some(42),
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
    .unwrap();
    let result = crate::test_support::with_decode_context(|ctx| {
        project_sketch_design(
            ctx,
            &SketchProjectInputs::new(&[], &[point], &[], &[], &[]),
            EPS_POINT_PROJECTION,
        )
    })
    .unwrap();
    assert!(result.entities.is_empty());
    assert_eq!(result.losses.len(), 1);
    assert_eq!(
        result.losses[0].code,
        crate::loss::F3dLossCode::SketchPointOwnerUnresolved.kind()
    );
    assert!(result.losses[0].message.contains("point#14"));
}
