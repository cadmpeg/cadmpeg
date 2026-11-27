// SPDX-License-Identifier: Apache-2.0
//! Pad and Pocket sweep-direction unit tests.

use crate::design::tests::definition;
use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_ir::features::extents::ExtrudeExtent;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

/// A Pad sweeps along the sketch normal and a Pocket against it, each reversed
/// by `Reversed`. A symmetric extent has no direction to flip, a custom
/// `Direction` is used as stored, and a `ReferenceAxis` direction is stored
/// already negated for a Pocket.
#[test]
fn pocket_sweeps_against_the_sketch_normal() {
    let extent = |kind: &str, properties: &str| {
        let all = format!(
            r#"<Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Length2" type="App::PropertyLength"><Float value="3"/></Property>{properties}"#
        );
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch"/><Object type="{kind}" name="Feature"/></Objects>
<ObjectData Count="2">
<Object name="Sketch"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="10" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
<Object name="Feature"><Properties Count="{count}">{all}</Properties></Object></ObjectData></Document>"#,
            count = all.matches("<Property ").count(),
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("extrusion");
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            direction, extent, ..
        }) = definition(&result, "Feature").clone()
        else {
            panic!("{kind}: not a neutral extrusion");
        };
        let cadmpeg_ir::features::extents::ExtrudeDirection::Explicit { vector, .. } = direction
        else {
            panic!("{kind}: direction is not explicit");
        };
        (vector.z, extent)
    };
    let prop = |name: &str, kind: &str, value: &str| {
        format!(r#"<Property name="{name}" type="App::{kind}"><{value}/></Property>"#)
    };
    let type_prop = |value: u32| {
        prop(
            "Type",
            "PropertyEnumeration",
            &format!(r#"Integer value="{value}""#),
        )
    };
    let side_prop = |value: u32| {
        prop(
            "SideType",
            "PropertyEnumeration",
            &format!(r#"Integer value="{value}""#),
        )
    };
    let reversed = prop("Reversed", "PropertyBool", r#"Bool value="true""#);

    // Blind length.
    assert_eq!(extent("PartDesign::Pad", &type_prop(0)).0, 1.0);
    assert_eq!(extent("PartDesign::Pocket", &type_prop(0)).0, -1.0);
    assert_eq!(
        extent("PartDesign::Pad", &format!("{}{reversed}", type_prop(0))).0,
        -1.0
    );
    assert_eq!(
        extent("PartDesign::Pocket", &format!("{}{reversed}", type_prop(0))).0,
        1.0
    );
    // Through all, up to first, up to face.
    for termination in [1, 2, 3] {
        let (z, _) = extent(
            "PartDesign::Pocket",
            &format!(
                "{}{}",
                type_prop(termination),
                r#"<Property name="UpToFace" type="App::PropertyLinkSub"><LinkSub value="Sketch" count="1"><Sub value="Face1"/></LinkSub></Property>"#
            ),
        );
        assert_eq!(z, -1.0, "Pocket termination {termination}");
    }
    // Two lengths: the second length runs opposite the first, so both flip.
    let (z, two) = extent(
        "PartDesign::Pocket",
        &format!("{}{}", side_prop(1), type_prop(0)),
    );
    assert_eq!(z, -1.0);
    assert!(matches!(two, ExtrudeExtent::TwoSided { .. }));
    // Symmetric: no direction to flip.
    let symmetric = format!("{}{}", side_prop(2), type_prop(0));
    assert_eq!(extent("PartDesign::Pocket", &symmetric).0, 1.0);
    assert_eq!(extent("PartDesign::Pad", &symmetric).0, 1.0);
    // Custom direction is used as stored.
    let custom = format!(
        "{}{}{}",
        type_prop(0),
        prop("UseCustomVector", "PropertyBool", r#"Bool value="true""#),
        prop(
            "Direction",
            "PropertyVector",
            r#"PropertyVector valueX="0" valueY="0" valueZ="1""#
        )
    );
    assert_eq!(extent("PartDesign::Pocket", &custom).0, 1.0);
}
