// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::{
    DistinctMembers, FeatureContent, FeatureCoordinateFrame, FeatureDefinition, FeatureEvaluation,
    FeatureId,
};
use cadmpeg_ir::math::{Point3, Vector3};

fn feature(operation: LegacyOperation) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:model:feature#joint-origin:10").unwrap(),
        ordinal: 0,
        name: Some("Coordinate System 1".to_owned()),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: [
            (
                cadmpeg_core::nonblank_literal!("reference:0"),
                "11".to_owned(),
            ),
            (
                cadmpeg_core::nonblank_literal!("parameter:d8"),
                "2.5 mm".to_owned(),
            ),
        ]
        .into(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some("f3d:Design1/BulkStream.dat:design-parameter-scope#100".to_owned()),
    }
}

#[test]
fn solved_frame_keeps_origin_axes_and_native_construction() {
    let frame = FeatureCoordinateFrame::new(
        Point3::new(12.5, -25.0, 37.5),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let source = feature(LegacyOperation::DatumCoordinateSystem { frame });
    let mut losses = Vec::new();
    let FeatureOperation::Datum(datum) = project(&source, &mut losses).unwrap() else {
        panic!("datum frame")
    };
    let DatumElement::Frame(construction) = datum.element else {
        panic!("coordinate frame")
    };
    let resolved = construction.resolved.unwrap();
    assert_eq!(resolved.origin, frame.origin());
    assert_eq!(resolved.x_axis, frame.x_axis());
    assert_eq!(resolved.y_axis, frame.y_axis());
    assert_eq!(resolved.z_axis, frame.z_axis());
    assert_eq!(construction.inputs.len(), 1);
    assert!(construction.inputs[0].role.is_none());
    assert!(matches!(
        &construction.inputs[0].reference,
        Some(InputReference::Picks(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            ..
        }))
    ));
    assert!(construction.after.is_empty());
    assert!(construction.mode.is_none());
    for (key, expected) in [("f3d:reference:0", "11"), ("f3d:parameter:d8", "2.5 mm")] {
        assert_eq!(
            datum.native_options[&NonBlankString::new(key.to_owned()).unwrap()],
            NativeOption::Text(Dimension::Literal(
                NonBlankString::new(expected.to_owned()).unwrap()
            ))
        );
    }
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::CoordinateSystemConstructionNative.kind()
    );
    assert_eq!(
        losses[0].code.local_code(),
        "feature.coordinate-system-construction-native"
    );
}

#[test]
fn unresolved_frames_stay_native_and_each_charge_a_loss() {
    let mut losses = Vec::new();
    for kind in ["JointOrigin", "Coordinate System"] {
        let source = feature(LegacyOperation::Native {
            kind: kind.to_owned().into(),
            parameters: BTreeMap::default(),
        });
        assert!(project(&source, &mut losses).is_none());
    }
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code == F3dLossCode::CoordinateSystemFrameUnresolved.kind()));
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "feature.coordinate-system-frame-unresolved"));
}
