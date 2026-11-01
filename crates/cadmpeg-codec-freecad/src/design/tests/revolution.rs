// SPDX-License-Identifier: Apache-2.0
//! Angular bounds and sense of revolution properties.

use std::io::Cursor;

use cadmpeg_ir::features::{AngularTermination, BooleanOp, FeatureOperation, RevolveExtent};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::test_support::test_archive::archive;
use crate::FcstdCodec;

const EPS_PROPERTY_ANGLE: f64 = 1.0e-12;

#[test]
fn revolve_and_groove_keep_reversed_axis_and_each_side() {
    for kind in ["PartDesign::Revolution", "PartDesign::Groove"] {
        for reversed in [false, true] {
            for (mode, midplane, first, second, admitted) in [
                (0, false, 270, 0, true),
                (0, true, 180, 0, true),
                (0, false, 360, 60, true),
                (0, false, 361, 0, false),
                (0, false, 0, 0, false),
                (0, false, -90, 0, false),
                (1, false, 90, 45, true),
                (2, false, 90, 45, true),
                (3, false, 90, 45, true),
                (4, false, 90, 45, true),
                (4, false, 270, 90, true),
                (4, false, 270, 91, false),
                (4, false, 90, -45, false),
                (4, false, 90, 0, false),
                (4, true, 90, 45, false),
                (5, false, 90, 45, false),
            ] {
                let document = format!(
                    r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="{kind}" name="Sweep"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="0"/></Object>
<Object name="Sweep"><Properties Count="10">
<Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
<Property name="Base" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
<Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="2"/></Property>
<Property name="Type" type="App::PropertyEnumeration"><Integer value="{mode}"/></Property>
<Property name="Angle" type="App::PropertyAngle"><Float value="{first}"/></Property>
<Property name="Angle2" type="App::PropertyAngle"><Float value="{second}"/></Property>
<Property name="Midplane" type="App::PropertyBool"><Bool value="{midplane}"/></Property>
<Property name="Reversed" type="App::PropertyBool"><Bool value="{reversed}"/></Property>
<Property name="ReferenceAxis" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="V_Axis"/></LinkSub></Property>
<Property name="UpToFace" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="Face1"/></LinkSub></Property>
</Properties></Object></ObjectData></Document>"#
                );
                let result = FcstdCodec
                    .decode(
                        &mut Cursor::new(archive(&document)),
                        &DecodeOptions::default(),
                    )
                    .expect("angular property decode");
                let feature = result
                    .ir()
                    .model
                    .features
                    .iter()
                    .find(|f| f.name.as_deref() == Some("Sweep"))
                    .expect("sweep feature");
                let operation = feature.evaluation.definition().operation();
                if !admitted {
                    assert!(matches!(operation, FeatureOperation::Native { .. }));
                    assert!(result
                        .report()
                        .losses
                        .iter()
                        .any(|loss| loss.code.namespace() == "fcstd"
                            && loss.code.local_code() == "feature.revolution-not-transferred"));
                    continue;
                }
                assert!(result.report().losses.is_empty());
                let FeatureOperation::Revolve { construction, op } = operation else {
                    panic!("revolve")
                };
                assert_eq!(
                    *op,
                    if kind.ends_with("Groove") {
                        BooleanOp::Cut
                    } else {
                        BooleanOp::Join
                    }
                );
                let axis = construction.axis().expect("axis");
                assert_eq!(axis.direction.z, if reversed { -1.0 } else { 1.0 });
                assert_eq!(
                    axis.origin.get(),
                    cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                );
                assert!(axis.reference.is_some());
                let angle_matches = |termination: &AngularTermination, degrees: i32| {
                    matches!(termination, AngularTermination::Angle { angle }
                        if result.ir().dimension_value(angle).is_some_and(|angle| (angle.get() - f64::from(degrees).to_radians()).abs() < EPS_PROPERTY_ANGLE))
                };
                match construction.extent().expect("extent") {
                    RevolveExtent::OneSided {
                        termination: termination @ AngularTermination::Angle { .. },
                    } => {
                        assert!(!midplane);
                        assert!(angle_matches(termination, first));
                    }
                    RevolveExtent::Symmetric { termination } => {
                        assert!(midplane);
                        assert!(angle_matches(termination, first));
                    }
                    RevolveExtent::TwoSided {
                        first: a,
                        second: b,
                    } => {
                        assert!(angle_matches(a, first));
                        assert!(angle_matches(b, second));
                    }
                    RevolveExtent::OneSided { .. } => {}
                }
                if mode == 1 {
                    assert!(matches!(
                        (kind, construction.extent()),
                        (
                            "PartDesign::Groove",
                            Some(RevolveExtent::OneSided {
                                termination: AngularTermination::ThroughAll {}
                            })
                        ) | (
                            "PartDesign::Revolution",
                            Some(RevolveExtent::OneSided {
                                termination: AngularTermination::ToLast {}
                            })
                        )
                    ));
                    continue;
                }
                assert!(matches!(
                    (mode, construction.extent()),
                    (
                        0,
                        Some(
                            RevolveExtent::OneSided {
                                termination: AngularTermination::Angle { .. }
                            } | RevolveExtent::Symmetric { .. }
                        )
                    ) | (
                        2,
                        Some(RevolveExtent::OneSided {
                            termination: AngularTermination::ToFirst {}
                        })
                    ) | (
                        3,
                        Some(RevolveExtent::OneSided {
                            termination: AngularTermination::ToFace { .. }
                        })
                    ) | (4, Some(RevolveExtent::TwoSided { .. }))
                ));
            }
        }
    }
}

#[test]
fn revolve_absent_base_defaults_but_malformed_base_remains_native() {
    for (base, admitted) in [
        ("", true),
        (
            r#"<Property name="Base" type="App::PropertyString"><String value="origin"/></Property>"#,
            false,
        ),
        (
            r#"<Property name="Base" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2"/></Property>"#,
            false,
        ),
        (
            r#"<Property name="Base" type="App::PropertyVector"><PropertyVector valueX="NaN" valueY="2" valueZ="3"/></Property>"#,
            false,
        ),
    ] {
        let count = if base.is_empty() { 3 } else { 4 };
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Revolution" name="Sweep"/></Objects>
<ObjectData Count="1"><Object name="Sweep"><Properties Count="{count}">{base}
<Property name="Axis" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
<Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>
<Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></Object></ObjectData></Document>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("base property decode");
        let operation = result.ir().model.features[0]
            .evaluation
            .definition()
            .operation();
        if admitted {
            let FeatureOperation::Revolve { construction, .. } = operation else {
                panic!("revolve")
            };
            assert_eq!(
                construction.axis().expect("axis").origin,
                cadmpeg_ir::features::FinitePoint3::ZERO
            );
        } else {
            assert!(matches!(operation, FeatureOperation::Native { .. }));
            assert!(result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.namespace() == "fcstd"
                    && loss.code.local_code() == "feature.revolution-not-transferred"));
        }
    }
}
