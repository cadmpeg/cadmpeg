// SPDX-License-Identifier: Apache-2.0
//! Design construction transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{
    FeatureDefinition, Length, ShellJoin, ShellMode, SweepOrientation, SweepTransformation,
    SweepTransition,
};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

mod binders;

#[test]
fn transfers_partdesign_refine_and_fuzzy_post_processing() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::AdditiveBox" name="Automatic" id="1"/>
 <Object type="PartDesign::AdditiveBox" name="Explicit" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Automatic"><Properties Count="5">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Refine" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"><Float value="-0.5"/></Property>
 </Properties></Object>
 <Object name="Explicit"><Properties Count="5">
  <Property name="Length" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="6"/></Property>
  <Property name="Refine" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"><Float value="0.01"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("PartDesign post-processing");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Automatic"),
        cadmpeg_ir::features::FeatureDefinition::PostProcess {
            operation,
            refine: true,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::Automatic,
        } if matches!(operation.as_ref(), cadmpeg_ir::features::FeatureDefinition::Primitive { .. })
    ));
    assert!(matches!(
        definition("Explicit"),
        cadmpeg_ir::features::FeatureDefinition::PostProcess {
            operation,
            refine: false,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::Explicit(0.01),
        } if matches!(operation.as_ref(), cadmpeg_ir::features::FeatureDefinition::Primitive { .. })
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn retains_native_for_malformed_post_process_controls() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="PartDesign::AdditiveBox" name="Absent" id="1"/>
 <Object type="PartDesign::AdditiveBox" name="FuzzyOnly" id="2"/>
 <Object type="PartDesign::AdditiveBox" name="RefineOnly" id="3"/>
 <Object type="PartDesign::AdditiveBox" name="WrongRefine" id="4"/>
 <Object type="PartDesign::AdditiveBox" name="WrongFuzzy" id="5"/>
 <Object type="PartDesign::AdditiveBox" name="NestedFuzzy" id="6"/>
 <Object type="PartDesign::AdditiveBox" name="DuplicateFuzzy" id="7"/>
 <Object type="PartDesign::AdditiveBox" name="NonFiniteFuzzy" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="Absent"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
 </Properties></Object>
 <Object name="FuzzyOnly"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"><Float value="0.01"/></Property>
 </Properties></Object>
 <Object name="RefineOnly"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Refine" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="WrongRefine"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Refine" type="App::PropertyInteger"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="WrongFuzzy"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloat"><Float value="0.01"/></Property>
 </Properties></Object>
 <Object name="NestedFuzzy"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"><Wrapper><Float value="0.01"/></Wrapper></Property>
 </Properties></Object>
 <Object name="DuplicateFuzzy"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"><Float value="0.01"/><Float value="0.02"/></Property>
 </Properties></Object>
 <Object name="NonFiniteFuzzy"><Properties Count="4">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloatConstraint"><Float value="NaN"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("post-processing control admission");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Absent"),
        FeatureDefinition::Primitive { .. }
    ));
    assert!(matches!(
        definition("FuzzyOnly"),
        FeatureDefinition::PostProcess {
            operation,
            refine: false,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::Explicit(value),
        } if (*value - 0.01).abs() < f64::EPSILON
            && matches!(operation.as_ref(), FeatureDefinition::Primitive { .. })
    ));
    assert!(matches!(
        definition("RefineOnly"),
        FeatureDefinition::PostProcess {
            operation,
            refine: true,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::KernelDefault,
        } if matches!(operation.as_ref(), FeatureDefinition::Primitive { .. })
    ));
    for name in [
        "WrongRefine",
        "WrongFuzzy",
        "NestedFuzzy",
        "DuplicateFuzzy",
        "NonFiniteFuzzy",
    ] {
        assert!(matches!(
            definition(name),
            FeatureDefinition::Native { kind, .. } if kind.as_str() == "PartDesign::AdditiveBox"
        ));
    }
    assert_eq!(result.report().losses.len(), 5);
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking
    }));
}

#[test]
pub(crate) fn transfers_part_construction_geometry_features() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="Part::Vertex" name="Vertex" id="1"/>
 <Object type="Part::Line" name="Line" id="2"/>
 <Object type="Part::Circle" name="Circle" id="3"/>
 <Object type="Part::Ellipse" name="Ellipse" id="4"/>
 <Object type="Part::Polygon" name="Polyline" id="5"/>
 <Object type="Part::RegularPolygon" name="Regular" id="6"/>
 <Object type="Part::Plane" name="Plane" id="7"/>
 <Object type="Part::Face" name="Face" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="Vertex"><Properties Count="3"><Property name="X" type="App::PropertyDistance"><Float value="1"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="2"/></Property><Property name="Z" type="App::PropertyDistance"><Float value="3"/></Property></Properties></Object>
 <Object name="Line"><Properties Count="6"><Property name="X1" type="App::PropertyDistance"><Float value="0"/></Property><Property name="Y1" type="App::PropertyDistance"><Float value="1"/></Property><Property name="Z1" type="App::PropertyDistance"><Float value="2"/></Property><Property name="X2" type="App::PropertyDistance"><Float value="3"/></Property><Property name="Y2" type="App::PropertyDistance"><Float value="4"/></Property><Property name="Z2" type="App::PropertyDistance"><Float value="5"/></Property></Properties></Object>
 <Object name="Circle"><Properties Count="3"><Property name="Radius" type="App::PropertyLength"><Float value="4"/></Property><Property name="Angle0" type="App::PropertyAngle"><Float value="30"/></Property><Property name="Angle1" type="App::PropertyAngle"><Float value="300"/></Property></Properties></Object>
 <Object name="Ellipse"><Properties Count="4"><Property name="MajorRadius" type="App::PropertyLength"><Float value="6"/></Property><Property name="MinorRadius" type="App::PropertyLength"><Float value="2"/></Property><Property name="Angle1" type="App::PropertyAngle"><Float value="15"/></Property><Property name="Angle2" type="App::PropertyAngle"><Float value="270"/></Property></Properties></Object>
 <Object name="Polyline"><Properties Count="2"><Property name="Nodes" type="App::PropertyVectorList"><VectorList file="Nodes"/></Property><Property name="Close" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Regular"><Properties Count="2"><Property name="Polygon" type="App::PropertyInteger"><Integer value="7"/></Property><Property name="Circumradius" type="App::PropertyLength"><Float value="8"/></Property></Properties></Object>
 <Object name="Plane"><Properties Count="2"><Property name="Length" type="App::PropertyLength"><Float value="9"/></Property><Property name="Width" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object>
 <Object name="Face"><Properties Count="2"><Property name="Sources" type="App::PropertyLinkList"><LinkList count="2"><Link value="Line"/><Link value="Circle"/></LinkList></Property><Property name="FaceMakerClass" type="App::PropertyString"><String value="Part::FaceMakerUnified"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let mut nodes = Vec::new();
    nodes.extend_from_slice(&3_u32.to_le_bytes());
    let points: &[(f64, f64, f64)] = &[(0.0, 0.0, 0.0), (2.0, 0.0, 0.0), (1.0, 1.0, 0.0)];
    for (x, y, z) in points {
        nodes.extend_from_slice(&x.to_le_bytes());
        nodes.extend_from_slice(&y.to_le_bytes());
        nodes.extend_from_slice(&z.to_le_bytes());
    }
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Nodes", &nodes),
            ])),
            &DecodeOptions::default(),
        )
        .expect("Part construction geometry");
    let feature = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert!(
        matches!(feature("Vertex").definition, FeatureDefinition::PointGeometry { position } if position == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
    );
    assert!(
        matches!(feature("Line").definition, FeatureDefinition::LineSegment { start, end } if start == cadmpeg_ir::math::Point3::new(0.0, 1.0, 2.0) && end == cadmpeg_ir::math::Point3::new(3.0, 4.0, 5.0))
    );
    assert!(matches!(
        feature("Circle").definition,
        FeatureDefinition::CircularArc {
            radius: cadmpeg_ir::features::Length(4.0),
            start_angle: cadmpeg_ir::features::Angle(start),
            end_angle: cadmpeg_ir::features::Angle(end),
            ..
        } if (start - 30_f64.to_radians()).abs() < 1.0e-12
            && (end - 300_f64.to_radians()).abs() < 1.0e-12
    ));
    assert!(matches!(
        feature("Ellipse").definition,
        FeatureDefinition::EllipticArc {
            major_radius: cadmpeg_ir::features::Length(6.0),
            minor_radius: cadmpeg_ir::features::Length(2.0),
            ..
        }
    ));
    assert!(
        matches!(&feature("Polyline").definition, FeatureDefinition::Polyline { points, closed: true } if points.len() == 3)
    );
    assert!(matches!(
        feature("Regular").definition,
        FeatureDefinition::RegularPolygonCurve {
            sides: 7,
            circumradius: cadmpeg_ir::features::Length(8.0)
        }
    ));
    assert!(matches!(
        feature("Plane").definition,
        FeatureDefinition::PlanarPatch {
            length: cadmpeg_ir::features::Length(9.0),
            width: cadmpeg_ir::features::Length(10.0)
        }
    ));
    assert!(
        matches!(&feature("Face").definition, FeatureDefinition::FaceFromShapes { sources: cadmpeg_ir::features::BodySelection::Native(source), face_maker } if source.ends_with(":Sources") && *face_maker == cadmpeg_ir::features::FaceMaker::Unified)
    );
    assert_eq!(feature("Face").dependencies.len(), 2);
    assert!(result.report().losses.is_empty());
}

#[test]
fn rejects_nested_polygon_vector_list_root() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Polygon" name="Polygon" id="1"/></Objects>
<ObjectData Count="1"><Object name="Polygon"><Properties Count="2">
<Property name="Nodes" type="App::PropertyVectorList"><Wrapper><VectorList file="Nodes"/></Wrapper></Property>
<Property name="Close" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let mut nodes = Vec::new();
    nodes.extend_from_slice(&2_u32.to_le_bytes());
    let points: &[(f64, f64, f64)] = &[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)];
    for (x, y, z) in points {
        nodes.extend_from_slice(&x.to_le_bytes());
        nodes.extend_from_slice(&y.to_le_bytes());
        nodes.extend_from_slice(&z.to_le_bytes());
    }
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Nodes", &nodes),
            ])),
            &DecodeOptions::default(),
        )
        .expect("native fallback for nested vector list");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Polygon"))
        .expect("polygon feature");
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Native { .. }
    ));
}

#[test]
fn rejects_malformed_polygon_vector_list_side_streams() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Polygon" name="Polygon" id="1"/></Objects>
<ObjectData Count="1"><Object name="Polygon"><Properties Count="2">
<Property name="Nodes" type="App::PropertyVectorList"><VectorList file="Nodes"/></Property>
<Property name="Close" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let encode = |points: &[(f64, f64, f64)]| {
        let mut bytes = Vec::with_capacity(4 + points.len() * 24);
        bytes.extend_from_slice(&(points.len() as u32).to_le_bytes());
        for (x, y, z) in points {
            bytes.extend_from_slice(&x.to_le_bytes());
            bytes.extend_from_slice(&y.to_le_bytes());
            bytes.extend_from_slice(&z.to_le_bytes());
        }
        bytes
    };
    let mut trailing = encode(&[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)]);
    trailing.push(0xaa);
    let mut non_finite = encode(&[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)]);
    non_finite[4 + 24..4 + 32].copy_from_slice(&f64::NAN.to_le_bytes());

    for nodes in [trailing, non_finite] {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("Nodes", &nodes),
                ])),
                &DecodeOptions::default(),
            )
            .expect("native fallback for malformed vector side stream");
        let feature = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Polygon"))
            .expect("polygon feature");
        assert!(matches!(
            feature.definition,
            FeatureDefinition::Native { .. }
        ));
    }

    let document_with_unowned_entry = document.replace(
        "<VectorList file=\"Nodes\"/>",
        "<VectorList file=\"Nodes\"/><Extra file=\"Other\"/>",
    );
    let nodes = encode(&[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)]);
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document_with_unowned_entry.as_bytes()),
                ("Nodes", &nodes),
                ("Other", &[]),
            ])),
            &DecodeOptions::default(),
        )
        .expect("native fallback for unowned vector side stream");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Polygon"))
        .expect("polygon feature");
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Native { .. }
    ));
}

#[test]
fn transfers_uniform_and_anisotropic_part_scale() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="Part::Scale" name="Uniform" id="2"/>
 <Object type="Part::Scale" name="Anisotropic" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="1"/></Property>
 </Properties></Object>
 <Object name="Uniform"><Properties Count="3">
  <Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="UniformScale" type="App::PropertyFloat"><Float value="-2"/></Property>
 </Properties></Object>
 <Object name="Anisotropic"><Properties Count="5">
  <Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="XScale" type="App::PropertyFloat"><Float value="2"/></Property>
  <Property name="YScale" type="App::PropertyFloat"><Float value="3"/></Property>
  <Property name="ZScale" type="App::PropertyFloat"><Float value="4"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part scale");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Uniform"),
        cadmpeg_ir::features::FeatureDefinition::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::ModelOrigin),
            factors: cadmpeg_ir::features::ScaleFactors::Uniform(-2.0),
            ..
        }
    ));
    assert!(matches!(
        definition("Anisotropic"),
        cadmpeg_ir::features::FeatureDefinition::Scale {
            factors: cadmpeg_ir::features::ScaleFactors::PerAxis(Vector3 {
                x: 2.0,
                y: 3.0,
                z: 4.0,
            }),
            ..
        }
    ));
}

#[test]
fn distinguishes_absent_and_malformed_part_scale_uniform_flag() {
    fn definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    fn document(uniform_property: Option<&str>) -> String {
        let uniform = uniform_property.unwrap_or_default();
        let count = 5 + usize::from(!uniform.is_empty());
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Box" name="Source"/><Object type="Part::Scale" name="Scale"/></Objects>
<ObjectData Count="2"><Object name="Source"><Properties Count="3">
<Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
<Property name="Width" type="App::PropertyLength"><Float value="1"/></Property>
<Property name="Height" type="App::PropertyLength"><Float value="1"/></Property>
</Properties></Object>
<Object name="Scale"><Properties Count="{count}">
<Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
{uniform}
<Property name="UniformScale" type="App::PropertyFloat"><Float value="2"/></Property>
<Property name="XScale" type="App::PropertyFloat"><Float value="3"/></Property>
<Property name="YScale" type="App::PropertyFloat"><Float value="4"/></Property>
<Property name="ZScale" type="App::PropertyFloat"><Float value="5"/></Property>
</Properties></Object></ObjectData></Document>"#
        )
    }

    let absent = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document(None))),
            &DecodeOptions::default(),
        )
        .expect("absent Part scale flag");
    assert!(matches!(
        definition(&absent, "Scale"),
        FeatureDefinition::Scale {
            factors: cadmpeg_ir::features::ScaleFactors::Uniform(2.0),
            ..
        }
    ));
    assert_valid_document(absent.ir());

    let valid =
        r#"<Property name="Uniform" type="App::PropertyBool"><Bool value="false"/></Property>"#;
    let valid = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document(Some(valid)))),
            &DecodeOptions::default(),
        )
        .expect("valid Part scale flag");
    assert!(matches!(
        definition(&valid, "Scale"),
        FeatureDefinition::Scale {
            factors: cadmpeg_ir::features::ScaleFactors::PerAxis(Vector3 {
                x: 3.0,
                y: 4.0,
                z: 5.0,
            }),
            ..
        }
    ));
    assert_valid_document(valid.ir());

    let malformed_values = [
        r#"<Property name="TARGET" type="App::PropertyString"><String value="false"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyInteger"><Integer value="0"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="1"/></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Wrapper><Bool value="false"/></Wrapper></Property>"#,
        r#"<Property name="TARGET" type="App::PropertyBool"><Bool value="false"/><Bool value="true"/></Property>"#,
    ];
    for malformed in malformed_values {
        let replacement = malformed.replace("TARGET", "Uniform");
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document(Some(&replacement)))),
                &DecodeOptions::default(),
            )
            .expect("malformed Part scale flag");
        assert!(matches!(
            definition(&result, "Scale"),
            FeatureDefinition::Native { kind, .. } if kind.as_str() == "Part::Scale"
        ));
        assert_eq!(result.report().losses.len(), 1);
        assert_valid_document(result.ir());
    }
}

#[test]
fn transfers_part_compound_refine_and_reverse_operations() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Part::Box" name="A" id="1"/>
 <Object type="Part::Box" name="B" id="2"/>
 <Object type="Part::Compound" name="Compound" id="3"/>
 <Object type="Part::Compound2" name="Compound2" id="4"/>
 <Object type="Part::Refine" name="Refine" id="5"/>
 <Object type="Part::Reverse" name="Reverse" id="6"/>
 <Object type="Part::Compound" name="CachedCompound" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Compound"><Properties Count="1"><Property name="Links" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property></Properties></Object>
 <Object name="Compound2"><Properties Count="1"><Property name="Links" type="App::PropertyLinkList"><LinkList count="2"><Link value="B"/><Link value="A"/></LinkList></Property></Properties></Object>
 <Object name="Refine"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Compound"/></Property></Properties></Object>
 <Object name="Reverse"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Refine"/></Property></Properties></Object>
 <Object name="CachedCompound"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"/></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("derived Part shapes");
    let feature = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert!(matches!(
        &feature("Compound").definition,
        cadmpeg_ir::features::FeatureDefinition::Compound {
            members: cadmpeg_ir::features::BodySelection::Native(reference)
        } if reference.ends_with(":Links")
    ));
    assert!(matches!(
        feature("Refine").definition,
        cadmpeg_ir::features::FeatureDefinition::RefineShape { .. }
    ));
    assert!(matches!(
        feature("Reverse").definition,
        cadmpeg_ir::features::FeatureDefinition::ReverseShape { .. }
    ));
    assert!(matches!(
        feature("CachedCompound").definition,
        cadmpeg_ir::features::FeatureDefinition::StoredGeometry
    ));
    assert_eq!(feature("Compound").dependencies.len(), 2);
    assert_eq!(feature("Compound2").dependencies.len(), 2);
    assert_eq!(feature("Refine").dependencies.len(), 1);
    assert_eq!(feature("Reverse").dependencies.len(), 1);
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_part_ruled_surface_and_section_intersection() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="First" id="1"/>
 <Object type="Part::Box" name="Second" id="2"/>
 <Object type="Part::RuledSurface" name="Ruled" id="3"/>
 <Object type="Part::Section" name="Section" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="First"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Second"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Ruled"><Properties Count="3">
  <Property name="Curve1" type="App::PropertyLinkSub"><LinkSub value="First" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Curve2" type="App::PropertyLinkSub"><LinkSub value="Second" count="1"><Sub value="Wire1"/></LinkSub></Property>
  <Property name="Orientation" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="Section"><Properties Count="3">
  <Property name="Base" type="App::PropertyLink"><Link value="First"/></Property>
  <Property name="Tool" type="App::PropertyLink"><Link value="Second"/></Property>
  <Property name="Approximation" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part surface constructions");
    let feature = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert!(matches!(
        &feature("Ruled").definition,
        cadmpeg_ir::features::FeatureDefinition::RuledBetweenCurves {
            first: cadmpeg_ir::features::PathRef::Native(first),
            second: cadmpeg_ir::features::PathRef::Native(second),
            orientation: cadmpeg_ir::features::RuledCurveOrientation::Reversed,
        } if first.ends_with(":Curve1") && second.ends_with(":Curve2")
    ));
    assert!(matches!(
        feature("Section").definition,
        cadmpeg_ir::features::FeatureDefinition::SectionShape {
            approximate: Some(true),
            ..
        }
    ));
    assert_eq!(feature("Ruled").dependencies.len(), 2);
    assert_eq!(feature("Section").dependencies.len(), 2);
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_standalone_part_mirror_plane_semantics() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="Part::Box" name="PlaneCarrier" id="2"/>
 <Object type="Part::Mirroring" name="Mirror" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="3"/></Property></Properties></Object>
 <Object name="PlaneCarrier"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="4"/></Property><Property name="Width" type="App::PropertyLength"><Float value="5"/></Property><Property name="Height" type="App::PropertyLength"><Float value="6"/></Property></Properties></Object>
 <Object name="Mirror"><Properties Count="4">
  <Property name="Source" type="App::PropertyLink"><Link value="Source"/></Property>
 <Property name="Base" type="App::PropertyVector"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
 <Property name="Normal" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="4"/></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><LinkSub value="PlaneCarrier" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("standalone Part mirror");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Mirror"))
        .expect("mirror feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::MirrorShape {
            source: cadmpeg_ir::features::BodySelection::Native(source),
            plane_origin: cadmpeg_ir::math::Point3 { x: 1.0, y: 2.0, z: 3.0 },
            plane_normal: cadmpeg_ir::math::Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            plane_reference: Some(cadmpeg_ir::features::FaceSelection::Native(reference)),
        } if source.ends_with(":Source") && reference.ends_with(":MirrorPlane")
    ));
    assert_eq!(feature.dependencies.len(), 2);
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_part_projection_on_surface_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="First" id="1"/>
 <Object type="Part::Box" name="Second" id="2"/>
 <Object type="Part::Box" name="Support" id="3"/>
 <Object type="Part::ProjectOnSurface" name="Projection" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="First"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Second"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Support"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="3"/></Property><Property name="Width" type="App::PropertyLength"><Float value="3"/></Property><Property name="Height" type="App::PropertyLength"><Float value="3"/></Property></Properties></Object>
 <Object name="Projection"><Properties Count="6">
  <Property name="Projection" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="First" sub="Wire1"/><Link obj="Second" sub="Face2"/></LinkSubList></Property>
  <Property name="SupportFace" type="App::PropertyLinkSub"><LinkSub value="Support" count="1"><Sub value="Face1"/></LinkSub></Property>
 <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="5"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Offset" type="App::PropertyDistance"><Float value="-1.5"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("projection on surface");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Projection"))
        .expect("projection feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::ProjectOnSurface {
            sources: cadmpeg_ir::features::PathRef::Native(sources),
            support_face: cadmpeg_ir::features::FaceSelection::Native(support),
            direction: cadmpeg_ir::math::Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            mode: cadmpeg_ir::features::SurfaceProjectionMode::Faces,
            height: cadmpeg_ir::features::Length(8.0),
            offset: cadmpeg_ir::features::Length(-1.5),
        } if sources.ends_with(":Projection")
            && support.ends_with(":SupportFace")
    ));
    assert_eq!(feature.dependencies.len(), 3);
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_ordered_loft_sections_and_subtractive_pipe_path() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Sketcher::SketchObject" name="Section1" id="1"/>
 <Object type="Sketcher::SketchObject" name="Section2" id="2"/>
 <Object type="Sketcher::SketchObject" name="Path" id="3"/>
 <Object type="PartDesign::AdditiveLoft" name="Loft" id="4"/>
 <Object type="PartDesign::SubtractivePipe" name="Pipe" id="5"/>
 <Object type="Part::Loft" name="SurfaceLoft" id="6"/>
 <Object type="Part::Sweep" name="SurfaceSweep" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Section1"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Section2"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Path"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Loft"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section1"/></Property>
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="Section2"/></LinkList></Property>
  <Property name="Closed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Ruled" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="Pipe"><Properties Count="11">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section1"/></Property>
  <Property name="Sections" type="App::PropertyLinkSubList"><LinkSubList count="2"><Link obj="Section1" sub=""/><Link obj="Section2" sub=""/></LinkSubList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="SpineTangent" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AuxiliarySpine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge2"/></LinkSub></Property>
  <Property name="AuxiliarySpineTangent" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AuxiliaryCurvilinear" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="SurfaceLoft"><Properties Count="4">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Section1"/><Link value="Section2"/></LinkList></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="MaxDegree" type="App::PropertyInteger"><Integer value="7"/></Property>
  <Property name="CheckCompatibility" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="SurfaceSweep"><Properties Count="6">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Section1"/><Link value="Section2"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Frenet" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Linearize" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("loft and pipe");
    let feature = |name: &str| {
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
    };
    assert!(matches!(
        &feature("Loft").definition,
        cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            closed: true,
            solid: true,
            ruled: true,
            allow_multi_profile_faces: Some(false),
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        } if matches!(sections.as_slice(), [
            cadmpeg_ir::features::LoftSection::Profile(cadmpeg_ir::features::ProfileRef::Sketch(first)),
            cadmpeg_ir::features::LoftSection::Profile(cadmpeg_ir::features::ProfileRef::Sketch(second)),
        ] if first.0.ends_with("#Section1") && second.0.ends_with("#Section2"))
    ));
    assert!(matches!(
        &feature("SurfaceLoft").definition,
        cadmpeg_ir::features::FeatureDefinition::Loft {
            solid: false,
            ruled: false,
            max_degree: Some(7),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            ..
        }
    ));
    let native_properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("native properties");
    let compatibility_properties = native_properties
        .iter()
        .filter(|property| property.name == "CheckCompatibility")
        .collect::<Vec<_>>();
    assert_eq!(compatibility_properties.len(), 1);
    assert_eq!(compatibility_properties[0].type_name, "App::PropertyBool");
    assert!(compatibility_properties[0]
        .raw_xml
        .contains("<Bool value=\"false\"/>"));
    assert!(matches!(
        &feature("Pipe").definition,
        cadmpeg_ir::features::FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Sketch(_),
            ),
            sections,
            path: Some(cadmpeg_ir::features::PathRef::Native(path)),
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: cadmpeg_ir::features::BooleanKind::Cut,
            },
            orientation: Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
                tangent: true,
                curvilinear: false,
                ..
            }),
            transition: Some(cadmpeg_ir::features::SweepTransition::RoundCorner),
            transformation: Some(cadmpeg_ir::features::SweepTransformation::MultiSection),
            path_tangent: true,
            allow_multi_profile_faces: Some(true),
            ..
        } if path.ends_with(":Spine") && sections.len() == 1
    ));
    assert!(matches!(
        &feature("SurfaceSweep").definition,
        cadmpeg_ir::features::FeatureDefinition::Sweep {
            sections,
            mode: cadmpeg_ir::features::SweepMode::Surface,
            orientation: Some(cadmpeg_ir::features::SweepOrientation::CorrectedFrenet),
            transition: Some(cadmpeg_ir::features::SweepTransition::RoundCorner),
            transformation: Some(cadmpeg_ir::features::SweepTransformation::Constant),
            linearize: true,
            ..
        } if sections.len() == 1
    ));
    assert_eq!(feature("Loft").dependencies.len(), 2);
    assert_eq!(feature("Pipe").dependencies.len(), 3);
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_remaining_pipe_orientation_and_transformation_modes() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="Sketcher::SketchObject" name="Section" id="1"/>
 <Object type="Sketcher::SketchObject" name="Path" id="2"/>
 <Object type="PartDesign::AdditivePipe" name="Fixed" id="3"/>
 <Object type="PartDesign::AdditivePipe" name="Frenet" id="4"/>
 <Object type="PartDesign::AdditivePipe" name="Binormal" id="5"/>
 <Object type="PartDesign::AdditivePipe" name="Linear" id="6"/>
 <Object type="PartDesign::AdditivePipe" name="SShape" id="7"/>
 <Object type="PartDesign::AdditivePipe" name="Interpolation" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="Section"><Properties Count="0"/></Object>
 <Object name="Path"><Properties Count="0"/></Object>
 <Object name="Fixed"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Frenet"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Binormal"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="4"/></Property>
 <Property name="Binormal" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="4"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Linear"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="SShape"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Interpolation"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="4"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("pipe modes");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Fixed"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Fixed),
            transition: Some(SweepTransition::Transformed),
            ..
        }
    ));
    assert!(matches!(
        definition("Frenet"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Frenet),
            transition: Some(SweepTransition::RightCorner),
            ..
        }
    ));
    assert!(matches!(
        definition("Binormal"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Binormal { direction }),
            transition: Some(SweepTransition::RoundCorner),
            ..
        } if direction.z == 1.0
    ));
    for (name, expected) in [
        ("Linear", SweepTransformation::Linear),
        ("SShape", SweepTransformation::SShape),
        ("Interpolation", SweepTransformation::Interpolation),
    ] {
        assert!(matches!(
            definition(name),
            FeatureDefinition::Sweep {
                transformation: Some(actual),
                ..
            } if *actual == expected
        ));
    }
}

#[test]
fn distinguishes_absent_and_malformed_loft_sweep_boolean_flags() {
    fn definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="17">
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
 <Object type="Sketcher::SketchObject" name="Section" id="2"/>
 <Object type="Sketcher::SketchObject" name="Path" id="3"/>
 <Object type="Part::Loft" name="LoftAbsent" id="4"/>
 <Object type="PartDesign::AdditiveLoft" name="LoftValid" id="5"/>
 <Object type="Part::Sweep" name="SweepAbsent" id="6"/>
 <Object type="Part::Sweep" name="SweepValid" id="7"/>
 <Object type="PartDesign::AdditivePipe" name="PipeAbsent" id="8"/>
 <Object type="PartDesign::AdditivePipe" name="PipeValid" id="9"/>
 <Object type="Part::Loft" name="LoftBadSolid" id="10"/>
 <Object type="PartDesign::AdditiveLoft" name="LoftBadAllow" id="11"/>
 <Object type="PartDesign::AdditivePipe" name="PipeBadSpine" id="12"/>
 <Object type="PartDesign::AdditivePipe" name="PipeBadAux" id="13"/>
 <Object type="Part::Sweep" name="SweepBadSolid" id="14"/>
 <Object type="Part::Sweep" name="SweepBadFrenet" id="15"/>
 <Object type="Part::Loft" name="LoftLinearized" id="16"/>
 <Object type="Part::Loft" name="LoftBadLinearize" id="17"/>
</Objects>
<ObjectData Count="17">
 <Object name="Profile"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Section"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Path"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="LoftAbsent"><Properties Count="1">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Profile"/><Link value="Section"/></LinkList></Property>
 </Properties></Object>
 <Object name="LoftValid"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Sections" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Section" sub=""/></LinkSubList></Property>
  <Property name="Ruled" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Closed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Linearize" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="SweepAbsent"><Properties Count="2">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="Profile"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="SweepValid"><Properties Count="6">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="Profile"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Frenet" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Linearize" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="PipeAbsent"><Properties Count="2">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="PipeValid"><Properties Count="9">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="SpineTangent" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AuxiliarySpine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge2"/></LinkSub></Property>
  <Property name="AuxiliarySpineTangent" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AuxiliaryCurvilinear" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="LoftBadSolid"><Properties Count="2">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Profile"/><Link value="Section"/></LinkList></Property>
  <Property name="Solid" type="App::PropertyInteger"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="LoftBadAllow"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Sections" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Section" sub=""/></LinkSubList></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Wrapper><Bool value="true"/></Wrapper></Property>
 </Properties></Object>
 <Object name="PipeBadSpine"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="SpineTangent" type="App::PropertyBool"><Bool value="1"/></Property>
 </Properties></Object>
 <Object name="PipeBadAux"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="AuxiliarySpine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge2"/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="AuxiliaryCurvilinear" type="App::PropertyInteger"><Integer value="1"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="SweepBadSolid"><Properties Count="3">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="Profile"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Solid" type="App::PropertyString"><String value="true"/></Property>
 </Properties></Object>
 <Object name="SweepBadFrenet"><Properties Count="3">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="Profile"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><LinkSub value="Path" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Frenet" type="App::PropertyBool"><Bool value="false"/><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="LoftLinearized"><Properties Count="2">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Profile"/><Link value="Section"/></LinkList></Property>
  <Property name="Linearize" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="LoftBadLinearize"><Properties Count="2">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Profile"/><Link value="Section"/></LinkList></Property>
  <Property name="Linearize" type="App::PropertyInteger"><Integer value="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("loft and sweep flags");

    assert!(matches!(
        definition(&result, "LoftAbsent"),
        FeatureDefinition::Loft {
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            allow_multi_profile_faces: None,
            ..
        }
    ));
    assert!(matches!(
        definition(&result, "LoftValid"),
        FeatureDefinition::Loft {
            closed: true,
            solid: true,
            ruled: true,
            linearize: false,
            allow_multi_profile_faces: Some(false),
            ..
        }
    ));
    assert!(matches!(
        definition(&result, "LoftLinearized"),
        FeatureDefinition::Loft {
            closed: false,
            solid: true,
            ruled: false,
            linearize: true,
            allow_multi_profile_faces: None,
            ..
        }
    ));
    assert!(matches!(
        definition(&result, "SweepAbsent"),
        FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Solid { .. },
            orientation: Some(SweepOrientation::Frenet),
            path_tangent: false,
            linearize: false,
            allow_multi_profile_faces: None,
            ..
        }
    ));
    assert!(matches!(
        definition(&result, "SweepValid"),
        FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Surface,
            orientation: Some(SweepOrientation::CorrectedFrenet),
            path_tangent: false,
            linearize: true,
            allow_multi_profile_faces: None,
            ..
        }
    ));
    assert!(matches!(
        definition(&result, "PipeAbsent"),
        FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Solid { .. },
            orientation: Some(SweepOrientation::CorrectedFrenet),
            path_tangent: false,
            allow_multi_profile_faces: Some(false),
            ..
        }
    ));
    assert!(matches!(
        definition(&result, "PipeValid"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Auxiliary {
                tangent: true,
                curvilinear: false,
                ..
            }),
            path_tangent: true,
            allow_multi_profile_faces: Some(false),
            ..
        }
    ));
    for (name, kind) in [
        ("LoftBadSolid", "Part::Loft"),
        ("LoftBadAllow", "PartDesign::AdditiveLoft"),
        ("PipeBadSpine", "PartDesign::AdditivePipe"),
        ("PipeBadAux", "PartDesign::AdditivePipe"),
        ("SweepBadSolid", "Part::Sweep"),
        ("SweepBadFrenet", "Part::Sweep"),
        ("LoftBadLinearize", "Part::Loft"),
    ] {
        assert!(matches!(
            definition(&result, name),
            FeatureDefinition::Native { kind: actual, .. } if actual.as_str() == kind
        ));
    }
    assert_eq!(result.report().losses.len(), 7);
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking
    }));
}

#[test]
fn preserves_cached_loft_and_chamfer_without_construction_inputs() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Loft" name="Loft" id="1"/>
 <Object type="Part::Chamfer" name="Chamfer" id="2"/>
 <Object type="Part::MultiFuse" name="Fusion" id="3"/>
 <Object type="Part::Fillet" name="Fillet" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Loft"><Properties Count="2">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Loft.Shape.brp"/></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Chamfer"><Properties Count="2">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Chamfer.Shape.brp"/></Property>
  <Property name="Base" type="App::PropertyLink"><Link value="Loft"/></Property>
 </Properties></Object>
 <Object name="Fusion"><Properties Count="1">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Fusion.Shape.brp"/></Property>
 </Properties></Object>
 <Object name="Fillet"><Properties Count="2">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Fillet.Shape.brp"/></Property>
  <Property name="Base" type="App::PropertyLink"><Link value="Fusion"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 0
*";
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Loft.Shape.brp", brep),
        ("Chamfer.Shape.brp", brep),
        ("Fusion.Shape.brp", brep),
        ("Fillet.Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("cached operations");
    assert!(result
        .ir()
        .model
        .features
        .iter()
        .all(|feature| matches!(feature.definition, FeatureDefinition::StoredGeometry)));
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_shape_and_subshape_binder_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="PartDesign::CoordinateSystem" name="Context" id="2"/>
 <Object type="PartDesign::ShapeBinder" name="ShapeBind" id="3"/>
 <Object type="PartDesign::SubShapeBinder" name="SubBind" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Context"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="ShapeBind"><Properties Count="2"><Property name="Support" type="App::PropertyLinkSubListGlobal"><LinkSubList count="1"><Link obj="Source" sub="Face1 Face2"/></LinkSubList></Property><Property name="TraceSupport" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="SubBind"><Properties Count="15">
  <Property name="Support" type="App::PropertyXLinkSubList"><XLinkSubList count="2"><XLink name="Source" sub="Edge1"/><XLink file="library.FCStd" name="RemotePart" sub="Face3"/></XLinkSubList></Property>
  <Property name="Context" type="App::PropertyXLink"><XLink name="Context"/></Property>
  <Property name="ClaimChildren" type="App::PropertyBool"><Bool value="true"/></Property><Property name="Relative" type="App::PropertyBool"><Bool value="false"/></Property><Property name="Fuse" type="App::PropertyBool"><Bool value="true"/></Property><Property name="MakeFace" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="BindMode" type="App::PropertyEnumeration"><Integer value="1"/></Property><Property name="PartialLoad" type="App::PropertyBool"><Bool value="true"/></Property><Property name="BindCopyOnChange" type="App::PropertyEnumeration"><Integer value="2"/></Property><Property name="Refine" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Offset" type="App::PropertyFloat"><Float value="-2.5"/></Property><Property name="OffsetJoinType" type="App::PropertyEnumeration"><Integer value="2"/></Property><Property name="OffsetFill" type="App::PropertyBool"><Bool value="true"/></Property><Property name="OffsetOpenResult" type="App::PropertyBool"><Bool value="true"/></Property><Property name="OffsetIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("binders");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(
        matches!(definition("ShapeBind"), cadmpeg_ir::features::FeatureDefinition::Binder { sources, construction: cadmpeg_ir::features::BinderConstruction::Shape { trace_support: true } } if sources.len() == 1 && sources[0].subelements == ["Face1", "Face2"])
    );
    let cadmpeg_ir::features::FeatureDefinition::PostProcess {
        operation,
        refine: false,
        ..
    } = definition("SubBind")
    else {
        panic!("subshape binder post-processing");
    };
    let cadmpeg_ir::features::FeatureDefinition::Binder {
        sources,
        construction:
            cadmpeg_ir::features::BinderConstruction::SubShape {
                lifecycle,
                placement,
                copy_on_change,
                claim_children,
                fuse,
                make_face,
                partial_load,
                refine,
                offset: Some(offset),
                context: Some(context),
            },
    } = operation.as_ref()
    else {
        panic!("subshape binder");
    };
    assert_eq!(sources.len(), 2);
    assert!(
        matches!(sources[1].target, cadmpeg_ir::features::BinderTarget::External { ref document, ref object } if document == "library.FCStd" && object == "RemotePart")
    );
    assert_eq!(*lifecycle, cadmpeg_ir::features::BinderLifecycle::Frozen);
    assert_eq!(*placement, cadmpeg_ir::features::BinderPlacement::Global);
    assert_eq!(
        *copy_on_change,
        cadmpeg_ir::features::BinderCopyOnChange::Mutated
    );
    assert!(*claim_children && *fuse && !*make_face && *partial_load && !*refine);
    assert_eq!(offset.distance.0, -2.5);
    assert_eq!(
        offset.join,
        cadmpeg_ir::features::BinderOffsetJoin::Intersection
    );
    assert!(matches!(
        context,
        cadmpeg_ir::features::BinderTarget::Feature { .. }
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn rejects_noncanonical_subshape_binder_context_carrier() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="PartDesign::CoordinateSystem" name="Context" id="2"/>
 <Object type="PartDesign::SubShapeBinder" name="SubBind" id="3"/>
 <Object type="PartDesign::SubShapeBinder" name="SubBindSelector" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Context"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="SubBind"><Properties Count="2">
  <Property name="Support" type="App::PropertyXLinkSubList"><XLinkSubList count="1"><XLink name="Source" sub="Face1"/></XLinkSubList></Property>
  <Property name="Context" type="App::PropertyXLinkList"><XLinkSubList count="2"><XLink name="Context"/><XLink name="OtherContext"/></XLinkSubList></Property>
 </Properties></Object>
 <Object name="SubBindSelector"><Properties Count="2">
  <Property name="Support" type="App::PropertyXLinkSubList"><XLinkSubList count="1"><XLink name="Source" sub="Face1"/></XLinkSubList></Property>
  <Property name="Context" type="App::PropertyXLink"><XLink name="Context" sub="Face1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("noncanonical binder context");
    for name in ["SubBind", "SubBindSelector"] {
        let definition = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("subshape binder feature");
        assert!(matches!(
            &definition.definition,
            FeatureDefinition::Native { kind, .. } if kind.as_str() == "PartDesign::SubShapeBinder"
        ));
    }
    assert_eq!(result.report().losses.len(), 2);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking));
}

#[test]
fn transfers_complete_thickness_construction_controls() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Box" name="Base" id="1"/>
 <Object type="PartDesign::Thickness" name="Wall" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Base"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
 </Properties></Object>
 <Object name="Wall"><Properties Count="7">
  <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face2 Face4"/></LinkSub></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="2.5"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="SelfIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("thickness");
    let wall = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Wall"))
        .expect("wall");
    assert!(matches!(
        &wall.definition,
        cadmpeg_ir::features::FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Native(selection),
            thickness: Some(cadmpeg_ir::features::Length(2.5)),
            outward: Some(false),
            mode: Some(cadmpeg_ir::features::ShellMode::BothSides),
            join: Some(cadmpeg_ir::features::ShellJoin::Intersection),
            resolve_intersections: Some(true),
            allow_self_intersections: Some(true),
            ..
        } if selection.ends_with(":Base")
    ));
    assert_eq!(wall.dependencies.len(), 1);
    assert!(result.report().losses.is_empty());
}

#[test]
fn transfers_part_thickness_and_shape_offset_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Base" id="1"/>
 <Object type="Part::Thickness" name="Thickness" id="2"/>
 <Object type="Part::Offset" name="Offset" id="3"/>
 <Object type="Part::Offset2D" name="Offset2D" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Base"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
 </Properties></Object>
 <Object name="Thickness"><Properties Count="6">
  <Property name="Faces" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face1 Face3"/></LinkSub></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="-2"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="SelfIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Offset"><Properties Count="7">
  <Property name="Source" type="App::PropertyLink"><Link value="Base"/></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="-1.5"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="SelfIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Fill" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Offset2D"><Properties Count="6">
  <Property name="Source" type="App::PropertyLink"><Link value="Base"/></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Fill" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part offsets");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Thickness"),
        FeatureDefinition::Shell {
            thickness: Some(Length(2.0)),
            outward: Some(false),
            mode: Some(ShellMode::Pipe),
            join: Some(ShellJoin::Intersection),
            resolve_intersections: Some(true),
            allow_self_intersections: Some(true),
            ..
        }
    ));
    assert!(matches!(
        definition("Offset"),
        FeatureDefinition::OffsetShape {
            distance: Length(-1.5),
            mode: ShellMode::BothSides,
            join: ShellJoin::Tangent,
            resolve_intersections: true,
            allow_self_intersections: true,
            fill: true,
            planar: false,
            ..
        }
    ));
    assert!(matches!(
        definition("Offset2D"),
        FeatureDefinition::OffsetShape {
            distance: Length(3.0),
            mode: ShellMode::Pipe,
            join: ShellJoin::Arc,
            fill: true,
            planar: true,
            ..
        }
    ));
}

#[test]
fn distinguishes_absent_and_malformed_shell_and_surface_selectors() {
    fn feature_definition<'a>(
        result: &'a cadmpeg_ir::codec::DecodeResult,
        name: &str,
    ) -> &'a FeatureDefinition {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    }

    fn assert_native(result: &cadmpeg_ir::codec::DecodeResult, kind: &str) {
        assert!(matches!(
            feature_definition(result, "Target"),
            FeatureDefinition::Native { kind: actual, .. } if actual.as_str() == kind
        ));
        assert_eq!(result.report().losses.len(), 1);
        assert!(result.report().losses.iter().all(|loss| {
            loss.code.namespace == "fcstd"
                && loss.code.code == "feature.native-kind-retained"
                && loss.severity == cadmpeg_ir::Severity::Blocking
        }));
    }

    let shell_document = |kind: &str, mode: &str, join: &str| {
        let (base_properties, base_count) = match kind {
            "PartDesign::Thickness" => (
                r#"<Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face1"/></LinkSub></Property><Property name="Value" type="App::PropertyLength"><Float value="2"/></Property><Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property><Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property>"#,
                4,
            ),
            "Part::Thickness" => (
                r#"<Property name="Faces" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face1"/></LinkSub></Property><Property name="Value" type="App::PropertyLength"><Float value="2"/></Property><Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property><Property name="SelfIntersection" type="App::PropertyBool"><Bool value="false"/></Property>"#,
                4,
            ),
            "Part::Offset" => (
                r#"<Property name="Source" type="App::PropertyLink"><Link value="Base"/></Property><Property name="Value" type="App::PropertyLength"><Float value="2"/></Property><Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property><Property name="SelfIntersection" type="App::PropertyBool"><Bool value="false"/></Property><Property name="Fill" type="App::PropertyBool"><Bool value="false"/></Property>"#,
                5,
            ),
            "Part::Offset2D" => (
                r#"<Property name="Source" type="App::PropertyLink"><Link value="Base"/></Property><Property name="Value" type="App::PropertyLength"><Float value="2"/></Property><Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property><Property name="Fill" type="App::PropertyBool"><Bool value="false"/></Property>"#,
                4,
            ),
            _ => panic!("unexpected shell kind {kind}"),
        };
        let count = base_count + usize::from(!mode.is_empty()) + usize::from(!join.is_empty());
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Box" name="Base" id="1"/><Object type="{kind}" name="Target" id="2"/></Objects>
<ObjectData Count="2">
 <Object name="Base"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property><Property name="Width" type="App::PropertyLength"><Float value="10"/></Property><Property name="Height" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object>
 <Object name="Target"><Properties Count="{count}">{base_properties}{mode}{join}</Properties></Object>
</ObjectData></Document>"#
        )
    };
    let decode_shell = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("shell selector document")
    };

    for kind in [
        "PartDesign::Thickness",
        "Part::Thickness",
        "Part::Offset",
        "Part::Offset2D",
    ] {
        let result = decode_shell(&shell_document(kind, "", ""));
        let expected_mode = if kind == "Part::Offset2D" {
            ShellMode::Pipe
        } else {
            ShellMode::Skin
        };
        assert!(
            matches!(
                feature_definition(&result, "Target"),
                FeatureDefinition::Shell {
                    mode: Some(mode),
                    join: Some(ShellJoin::Arc),
                    ..
                } if *mode == expected_mode
            ) || matches!(
                feature_definition(&result, "Target"),
                FeatureDefinition::OffsetShape {
                    mode,
                    join: ShellJoin::Arc,
                    ..
                } if *mode == expected_mode
            )
        );
        assert!(result.report().losses.is_empty());

        let malformed_values = [
            "<Integer value=\"bad\"/>",
            "<String value=\"0\"/>",
            "<Wrapper><Integer value=\"0\"/></Wrapper>",
            "<Integer value=\"0\"/><Integer value=\"1\"/>",
            "<Integer value=\"-1\"/>",
            "<Integer value=\"99\"/>",
        ];
        for selector in ["Mode", "Join"] {
            for value in malformed_values {
                let property = if value.starts_with("<String") {
                    format!(
                        r#"<Property name="{selector}" type="App::PropertyString">{value}</Property>"#
                    )
                } else {
                    format!(
                        r#"<Property name="{selector}" type="App::PropertyEnumeration">{value}</Property>"#
                    )
                };
                let mode = if selector == "Mode" {
                    property.as_str()
                } else {
                    r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#
                };
                let join = if selector == "Join" {
                    property.as_str()
                } else {
                    r#"<Property name="Join" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#
                };
                let result = decode_shell(&shell_document(kind, mode, join));
                assert_native(&result, kind);
            }
        }
    }

    let offset_2d_unsupported = decode_shell(&shell_document(
        "Part::Offset2D",
        r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>"#,
        r#"<Property name="Join" type="App::PropertyEnumeration"><Integer value="0"/></Property>"#,
    ));
    assert_native(&offset_2d_unsupported, "Part::Offset2D");

    let surface_document = |mode: &str| {
        let count = 5 + usize::from(!mode.is_empty());
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3"><Object type="Part::Box" name="Source" id="1"/><Object type="Part::Box" name="Support" id="2"/><Object type="Part::ProjectOnSurface" name="Target" id="3"/></Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Support"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Target"><Properties Count="{count}"><Property name="Projection" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Source" sub="Edge1"/></LinkSubList></Property><Property name="SupportFace" type="App::PropertyLinkSub"><LinkSub value="Support" count="1"><Sub value="Face1"/></LinkSub></Property><Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="4"/></Property><Property name="Offset" type="App::PropertyDistance"><Float value="-0.5"/></Property>{mode}</Properties></Object>
</ObjectData></Document>"#
        )
    };
    let decode_surface = |document: &str| {
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("surface selector document")
    };
    for (mode, expected) in [
        ("", cadmpeg_ir::features::SurfaceProjectionMode::All),
        (
            r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>"#,
            cadmpeg_ir::features::SurfaceProjectionMode::Faces,
        ),
        (
            r#"<Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>"#,
            cadmpeg_ir::features::SurfaceProjectionMode::Edges,
        ),
    ] {
        let result = decode_surface(&surface_document(mode));
        assert!(matches!(
            feature_definition(&result, "Target"),
            FeatureDefinition::ProjectOnSurface { mode: actual, .. } if *actual == expected
        ));
        assert!(result.report().losses.is_empty());
    }
    for value in [
        "<Integer value=\"bad\"/>",
        "<String value=\"0\"/>",
        "<Wrapper><Integer value=\"0\"/></Wrapper>",
        "<Integer value=\"0\"/><Integer value=\"1\"/>",
        "<Integer value=\"-1\"/>",
        "<Integer value=\"99\"/>",
    ] {
        let mode = if value.starts_with("<String") {
            format!(r#"<Property name="Mode" type="App::PropertyString">{value}</Property>"#)
        } else {
            format!(r#"<Property name="Mode" type="App::PropertyEnumeration">{value}</Property>"#)
        };
        let result = decode_surface(&surface_document(&mode));
        assert!(matches!(
            feature_definition(&result, "Target"),
            FeatureDefinition::Native { kind, .. } if kind.as_str() == "Part::ProjectOnSurface"
        ));
        assert_eq!(result.report().losses.len(), 1);
    }
}

#[test]
fn transfers_draft_with_resolved_neutral_plane_and_pull_direction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="Base" id="1"/>
 <Object type="PartDesign::Plane" name="Neutral" id="2"/>
 <Object type="PartDesign::Line" name="Pull" id="3"/>
 <Object type="PartDesign::Draft" name="Draft" id="4"/>
 <Object type="PartDesign::Draft" name="FaceDraft" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Base"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property><Property name="Width" type="App::PropertyLength"><Float value="10"/></Property><Property name="Height" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object>
 <Object name="Neutral"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="2" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Pull"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0.7071067811865476" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property></Properties></Object>
 <Object name="Draft"><Properties Count="5">
  <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face1 Face3"/></LinkSub></Property>
  <Property name="NeutralPlane" type="App::PropertyLinkSub"><LinkSub value="Neutral" count="1"><Sub value=""/></LinkSub></Property>
  <Property name="PullDirection" type="App::PropertyLinkSub"><LinkSub value="Pull" count="1"><Sub value=""/></LinkSub></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="5"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="FaceDraft"><Properties Count="4">
  <Property name="Base" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face2"/></LinkSub></Property>
  <Property name="NeutralPlane" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face1"/></LinkSub></Property>
  <Property name="PullDirection" type="App::PropertyLinkSub"><LinkSub value="Base" count="1"><Sub value="Face2"/></LinkSub></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="3"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("draft");
    let draft = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Draft"))
        .expect("draft feature");
    assert!(matches!(
        &draft.definition,
        cadmpeg_ir::features::FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Native(faces),
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane {
                plane: cadmpeg_ir::features::FaceSelection::Native(plane),
                pull: Some(cadmpeg_ir::features::DraftPull {
                    direction: pull_direction,
                    plane: None,
                }),
            },
            angle: Some(cadmpeg_ir::features::Angle(angle)),
            outward: Some(true),
        } if faces.ends_with(":Base")
            && plane.ends_with(":NeutralPlane")
            && (pull_direction.x - 0.0).abs() < 1.0e-12
            && (pull_direction.y + 1.0).abs() < 1.0e-12
            && pull_direction.z.abs() < 1.0e-12
            && (*angle + 5f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(draft.dependencies.len(), 3);
    let face_draft = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("FaceDraft"))
        .expect("face draft");
    assert!(matches!(
        face_draft.definition,
        FeatureDefinition::Draft {
            anchor: cadmpeg_ir::features::DraftAnchor::NeutralPlane { pull: None, .. },
            ..
        }
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn rejects_ambiguous_single_source_design_operands() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="Part::Box" name="A" id="1"/>
 <Object type="Part::Box" name="B" id="2"/>
 <Object type="Part::Box" name="C" id="3"/>
 <Object type="Part::Scale" name="Scale" id="4"/>
 <Object type="Part::Offset" name="Offset" id="5"/>
 <Object type="Part::Cut" name="Cut" id="6"/>
 <Object type="Part::Compound" name="Compound" id="7"/>
 <Object type="Part::Sweep" name="Sweep" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="C"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="3"/></Property><Property name="Width" type="App::PropertyLength"><Float value="3"/></Property><Property name="Height" type="App::PropertyLength"><Float value="3"/></Property></Properties></Object>
 <Object name="Scale"><Properties Count="3">
  <Property name="Base" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="UniformScale" type="App::PropertyFloat"><Float value="2"/></Property>
 </Properties></Object>
 <Object name="Offset"><Properties Count="3">
  <Property name="Source" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property>
  <Property name="Value" type="App::PropertyDistance"><Float value="1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Cut"><Properties Count="2">
  <Property name="Base" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property>
  <Property name="Tool" type="App::PropertyLink"><Link value="C"/></Property>
 </Properties></Object>
 <Object name="Compound"><Properties Count="1">
  <Property name="Links" type="App::PropertyLinkList"><LinkList count="0"/></Property>
 </Properties></Object>
 <Object name="Sweep"><Properties Count="4">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="1"><Link value="A"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkList"><LinkList count="2"><Link value="B"/><Link value="C"/></LinkList></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Frenet" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("ambiguous single-source operands");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    for name in ["Scale", "Offset", "Cut", "Compound", "Sweep"] {
        assert!(matches!(definition(name), FeatureDefinition::Native { .. }));
    }
    assert_eq!(result.report().losses.len(), 5);
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking
    }));
}
