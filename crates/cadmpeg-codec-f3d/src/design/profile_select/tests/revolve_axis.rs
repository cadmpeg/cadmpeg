// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn revolve_axis_reference_selects_the_named_sketch_line() {
    let mut group = group();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X21,
        );
    group
        .try_set_members(vec![crate::records::identity::Located {
            value: 10,
            offset: 0,
        }])
        .unwrap();
    let placements = [placement()];
    let sketch = neutral_sketch_id(&placements[0]);
    let curves = [curve(31, 11, 0)];
    let entities = [SketchEntity::new(
        neutral_sketch_curve_id(&sketch, 11, 0),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 1.0),
        })
        .unwrap(),
    )];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let operands = [operand(10, 0, 11)];
    let resolution = planar_resolution(&operands, &placements, &curves, &sketches, &entities);
    assert_eq!(
        super::super::resolved_revolve_axis_reference(
            &operands[0].id,
            std::slice::from_ref(&group),
            &resolution,
        ),
        Some(
            PathRef::sketch_curves(
                sketch.clone(),
                vec![neutral_sketch_curve_id(&sketch, 11, 0),]
            )
            .unwrap()
        ),
    );
    assert!(super::super::resolved_revolve_axis_reference(
        "stream:another-selection",
        std::slice::from_ref(&group),
        &resolution,
    )
    .is_none());
    assert!(super::super::resolved_revolve_axis_reference(
        &operands[0].id,
        &[group.clone(), group],
        &resolution,
    )
    .is_none());
}
