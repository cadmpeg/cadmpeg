// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::sketch_placement::{DesignSketchFrame, DesignSketchFrameForm};

#[test]
fn planar_line_endpoints_define_the_profile_for_each_auxiliary_normal() {
    let placement = DesignSketchPlacement {
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeCompact).unwrap(),
        id: "f3d:Design/BulkStream.dat:placement#100".into(),
        scope_record_index: None,
        entity_id: "Sketch_42".to_owned().try_into().unwrap(),
        visibility: None,
        display_name: None,
        class_tag: "300".to_owned().try_into().unwrap(),
        record_index: 100,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
    };
    let corners = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(2.0, 3.0, 0.0),
        Point3::new(0.0, 3.0, 0.0),
    ];
    for normal_sign in [-1.0, 1.0] {
        let curves: Vec<_> = (0..4)
            .map(|index| {
                let start = corners[index];
                let end = corners[(index + 1) % corners.len()];
                let direction = if index % 2 == 0 {
                    Vector3::new((end.x - start.x) / 2.0, 0.0, 0.0)
                } else {
                    Vector3::new(0.0, (end.y - start.y) / 3.0, 0.0)
                };
                let normal = if index % 2 == 0 {
                    Vector3::new(0.0, normal_sign, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, normal_sign)
                };
                let record_index = u32::try_from(index).unwrap() + 101;
                SketchCurveIdentity {
                    planar_z_noise: None,
                    tail_marker: None,
                    id: format!("f3d:Design/BulkStream.dat:curve#{record_index}"),
                    record_index,
                    owner_reference: Some(42),
                    geometry_group: None,
                    class_tag: "308".to_owned().try_into().unwrap(),
                    byte_offset: u64::from(record_index),
                    geometry_offset: 133,
                    entity_genesis: None,
                    primary_id: u64::from(record_index).try_into().unwrap(),
                    secondary_id: 0,
                    geometry: Some(
                        SketchCurveGeometry::line(start, end, direction, normal).unwrap(),
                    ),
                }
            })
            .collect();
        let projected = crate::test_support::with_decode_context(|ctx| {
            project_sketch_design(
                ctx,
                &SketchProjectInputs::new(std::slice::from_ref(&placement), &[], &curves, &[], &[]),
                EPS_POINT_PROJECTION,
            )
        })
        .unwrap();
        let (sketches, entities) = projected.into_tuple();
        assert_eq!(entities.len(), 4);
        assert_eq!(sketches.len(), 1);
        assert_eq!(sketches[0].profiles.len(), 1);
        assert_eq!(sketches[0].profiles[0].len(), 4);
        for (entity, index) in entities.iter().zip(0..4) {
            let start = corners[index];
            let end = corners[(index + 1) % corners.len()];
            let expected = cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                    start: Point2::new(start.x, start.y),
                    end: Point2::new(end.x, end.y),
                },
            )
            .unwrap();
            assert_eq!(entity.geometry, expected);
        }
    }
}
