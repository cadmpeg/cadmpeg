// SPDX-License-Identifier: Apache-2.0
//! Design history transfer unit tests.
#![allow(unused_imports)]

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{FeatureDefinition, Length};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn distinguishes_stored_base_and_application_owned_features() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Source" id="1"/>
 <Object type="PartDesign::FeatureBase" name="BaseFeature" id="2"/>
 <Object type="Part::FeaturePython" name="PartExtension" id="3"/>
 <Object type="PartDesign::FeaturePython" name="DesignExtension" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Source"><Properties Count="0"/></Object>
 <Object name="BaseFeature"><Properties Count="1"><Property name="BaseFeature" type="App::PropertyLink"><Link value="Source"/></Property></Properties></Object>
 <Object name="PartExtension"><Properties Count="1"><Property name="ProxyState" type="App::PropertyString"><String value="part-owned"/></Property></Properties></Object>
 <Object name="DesignExtension"><Properties Count="1"><Property name="ProxyState" type="App::PropertyString"><String value="design-owned"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("stored and derived features");
    let source = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Source"))
        .expect("stored source");
    let base = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("BaseFeature"))
        .expect("base feature");
    assert!(matches!(
        source.definition,
        cadmpeg_ir::features::FeatureDefinition::StoredGeometry
    ));
    assert!(matches!(
        &base.definition,
        cadmpeg_ir::features::FeatureDefinition::DerivedGeometry { source }
            if source.0 == "fcstd:design:feature#Source"
    ));
    assert_eq!(base.dependencies, std::slice::from_ref(&source.id));
    assert!(result.ir().model.features.iter().all(|feature| {
        !matches!(
            feature.name.as_deref(),
            Some("PartExtension" | "DesignExtension")
        )
    }));
    let namespace = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert!(objects
        .iter()
        .any(|object| object.type_name == "Part::FeaturePython"));
    assert!(objects
        .iter()
        .any(|object| object.type_name == "PartDesign::FeaturePython"));
    let census = namespace
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 2);
    assert!(census.iter().all(|record| record.neutral));
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
    let mut corrupted = result.ir().clone();
    let derived = corrupted
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("BaseFeature"))
        .expect("derived feature");
    derived.definition = cadmpeg_ir::features::FeatureDefinition::DerivedGeometry {
        source: cadmpeg_ir::features::FeatureId("fcstd:design:feature#Missing".into()),
    };
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("source feature")));
}

#[test]
fn rejects_noncanonical_feature_base_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Feature" name="Source" id="1"/>
 <Object type="Part::Feature" name="Alternate" id="2"/>
 <Object type="PartDesign::FeatureBase" name="MultiBase" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="0"/></Object>
 <Object name="Alternate"><Properties Count="0"/></Object>
 <Object name="MultiBase"><Properties Count="1"><Property name="BaseFeature" type="App::PropertyLinkList"><LinkList count="2"><Link value="Source"/><Link value="Alternate"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("noncanonical feature base carriers");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("MultiBase"))
        .expect("feature base");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Native { kind, .. } if kind == "PartDesign::FeatureBase"
    ));
    assert_eq!(result.report().losses.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking));
}

#[test]
fn keeps_extension_types_out_of_exact_design_dispatch() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
<Object type="Part::Fillet" name="PartFillet"/>
<Object type="PartDesign::Fillet" name="DesignFillet"/>
<Object type="Vendor::Fillet" name="VendorFillet"/>
<Object type="Vendor::PartDesign::PadLike" name="PadLike"/>
<Object type="Vendor::PartDesign::RevolutionLike" name="RevolutionLike"/>
<Object type="Vendor::PartDesign::BodyLike" name="BodyLike"/>
<Object type="Vendor::Spreadsheet::SheetLike" name="SheetLike"/>
</Objects>
<ObjectData Count="7">
<Object name="PartFillet"><Properties Count="0"/></Object>
<Object name="DesignFillet"><Properties Count="0"/></Object>
<Object name="VendorFillet"><Properties Count="0"/></Object>
<Object name="PadLike"><Properties Count="0"/></Object>
<Object name="RevolutionLike"><Properties Count="0"/></Object>
<Object name="BodyLike"><Properties Count="0"/></Object>
<Object name="SheetLike"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("exact dress-up dispatch");
    let feature_names = result
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| feature.name.as_deref())
        .collect::<Vec<_>>();
    assert!(feature_names.contains(&"PartFillet"));
    assert!(feature_names.contains(&"DesignFillet"));
    assert!(!feature_names.contains(&"VendorFillet"));
    assert!(!feature_names.contains(&"PadLike"));
    assert!(!feature_names.contains(&"RevolutionLike"));
    assert!(!feature_names.contains(&"BodyLike"));
    assert!(!feature_names.contains(&"SheetLike"));
    assert_valid_document(result.ir());
}

#[test]
fn transfers_ordered_body_membership_and_active_tip() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="PartDesign::Feature" name="First" id="2"/>
 <Object type="PartDesign::Feature" name="Second" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Body"><Properties Count="2">
  <Property name="Model" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Second"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("body state");
    let body = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    let cadmpeg_ir::features::FeatureDefinition::TreeNode {
        children,
        active_child,
        ..
    } = &body.definition
    else {
        panic!("body tree node");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.0.as_str())
            .collect::<Vec<_>>(),
        ["fcstd:design:feature#First", "fcstd:design:feature#Second"]
    );
    assert_eq!(active_child.as_ref(), children.get(1));
    for child in children {
        assert_eq!(
            result
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.id == *child)
                .and_then(|feature| feature.parent.as_ref()),
            Some(&body.id)
        );
    }
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    let body = corrupted
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    let cadmpeg_ir::features::FeatureDefinition::TreeNode { active_child, .. } =
        &mut body.definition
    else {
        panic!("body tree node");
    };
    *active_child = Some(cadmpeg_ir::features::FeatureId(
        "fcstd:design:feature#Outside".into(),
    ));
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("active tree child")));
}

#[test]
fn rejects_ambiguous_body_history_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="PartDesign::Body" name="TipList" id="1"/>
 <Object type="PartDesign::Feature" name="First" id="2"/>
 <Object type="PartDesign::Feature" name="Second" id="3"/>
 <Object type="PartDesign::Body" name="MembershipAliases" id="4"/>
 <Object type="PartDesign::Feature" name="Third" id="5"/>
 <Object type="PartDesign::Feature" name="Fourth" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="TipList"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
 <Object name="MembershipAliases"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Third"/></LinkList></Property>
  <Property name="Model" type="App::PropertyLinkList"><LinkList count="1"><Link value="Fourth"/></LinkList></Property>
 </Properties></Object>
 <Object name="Third"><Properties Count="0"/></Object>
 <Object name="Fourth"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("ambiguous body carriers");
    for name in ["TipList", "MembershipAliases"] {
        assert!(matches!(
            result
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some(name))
                .map(|feature| &feature.definition)
                .expect("body feature"),
            FeatureDefinition::Native { kind, .. } if kind == "PartDesign::Body"
        ));
    }
    assert_eq!(result.report().losses.len(), 2);
    assert!(result.report().losses.iter().all(|loss| {
        loss.code.namespace == "fcstd"
            && loss.code.code == "feature.native-kind-retained"
            && loss.severity == cadmpeg_ir::Severity::Blocking
    }));
    assert_valid_document(result.ir());
}

#[test]
fn transfers_stored_and_external_part_feature_families() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="9">
 <Object type="Part::FeatureExt" name="Extended" id="1"/>
 <Object type="Part::FeatureGeometrySet" name="GeometrySet" id="2"/>
 <Object type="Part::Spline" name="Spline" id="3"/>
 <Object type="Part::Part2DObject" name="Planar" id="4"/>
 <Object type="Part::ImportStep" name="Step" id="5"/>
 <Object type="Part::ImportIges" name="Iges" id="6"/>
 <Object type="Part::ImportBrep" name="Brep" id="7"/>
 <Object type="Part::CurveNet" name="CurveNet" id="8"/>
 <Object type="Part::Part2DObjectPython" name="PlanarExtension" id="9"/>
</Objects>
<ObjectData Count="9">
 <Object name="Extended"><Properties Count="0"/></Object>
 <Object name="GeometrySet"><Properties Count="0"/></Object>
 <Object name="Spline"><Properties Count="0"/></Object>
 <Object name="Planar"><Properties Count="0"/></Object>
 <Object name="Step"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/source.step"/></Property></Properties></Object>
 <Object name="Iges"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/source.igs"/></Property></Properties></Object>
 <Object name="Brep"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/source.brep"/></Property></Properties></Object>
 <Object name="CurveNet"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/network.brep"/></Property></Properties></Object>
 <Object name="PlanarExtension"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("remaining Part feature families");
    for name in ["Extended", "GeometrySet", "Spline", "Planar"] {
        assert!(matches!(
            result
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some(name))
                .expect("stored feature")
                .definition,
            cadmpeg_ir::features::FeatureDefinition::StoredGeometry
        ));
    }
    for (name, format) in [
        ("Step", cadmpeg_ir::features::GeometryImportFormat::Step),
        ("Iges", cadmpeg_ir::features::GeometryImportFormat::Iges),
        ("Brep", cadmpeg_ir::features::GeometryImportFormat::Brep),
        ("CurveNet", cadmpeg_ir::features::GeometryImportFormat::Brep),
    ] {
        assert!(matches!(
            &result
                .ir()
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some(name))
                .expect("import feature")
                .definition,
            cadmpeg_ir::features::FeatureDefinition::ImportedGeometry { path, format: actual }
                if path.starts_with("models/") && *actual == format
        ));
    }
    assert!(result
        .ir()
        .model
        .features
        .iter()
        .all(|feature| feature.name.as_deref() != Some("PlanarExtension")));
    let census = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 8);
    assert!(census.iter().all(|record| record.neutral));
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn transfers_datum_frames_from_persisted_placements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Plane" name="Plane" id="1"/>
 <Object type="PartDesign::Line" name="Axis" id="2"/>
 <Object type="PartDesign::Point" name="Point" id="3"/>
 <Object type="PartDesign::CoordinateSystem" name="Frame" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Plane"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Axis"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Point"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="7" Py="8" Pz="9" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Frame"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="11" Pz="12" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("datums");
    let definition = |name: &str| {
        &result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("datum")
            .definition
    };
    assert!(matches!(
        definition("Plane"),
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { origin, normal, u_axis }
            if *origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *normal == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                && *u_axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    ));
    assert!(matches!(
        definition("Axis"),
        cadmpeg_ir::features::FeatureDefinition::DatumAxis { origin, direction }
            if *origin == cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
                && *direction == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    ));
    assert!(matches!(
        definition("Point"),
        cadmpeg_ir::features::FeatureDefinition::DatumPoint { position, .. }
            if *position == cadmpeg_ir::math::Point3::new(7.0, 8.0, 9.0)
    ));
    assert!(matches!(
        definition("Frame"),
        cadmpeg_ir::features::FeatureDefinition::DatumCoordinateSystem { origin, x_axis, y_axis, z_axis }
            if *origin == cadmpeg_ir::math::Point3::new(10.0, 11.0, 12.0)
                && *x_axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && *y_axis == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
                && *z_axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
fn reports_attributable_native_design_blockers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::FeatureBase" name="Custom" id="1"/></Objects>
<ObjectData Count="1"><Object name="Custom"><Properties Count="2"><Property name="Refine" type="App::PropertyBool"><Bool value="true"/></Property><Property name="FuzzyTolerance" type="App::PropertyFloat"><Float value="0"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("native feature");
    assert_eq!(result.report().losses.len(), 1);
    assert_eq!(
        result.report().losses[0].severity,
        cadmpeg_ir::Severity::Blocking
    );
    assert_eq!(
        result.report().losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("fcstd:native:object#Custom")
    );
}

#[test]
fn transfers_spreadsheet_cells_aliases_and_parameter_dependencies() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2" Dependencies="1">
 <ObjectDeps Name="Pad" Count="0"/>
 <ObjectDeps Name="Sheet" Count="0"/>
 <Object type="PartDesign::Pad" name="Pad" id="2"/>
 <Object type="Spreadsheet::Sheet" name="Sheet" id="1"/>
</Objects>
<ObjectData Count="2">
 <Object name="Sheet"><Properties Count="3"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="2" xlink="1">
  <Cell address="A2" content="=width * 3" alias="height" style="bold"/>
  <Cell address="A1" content="5" alias="width" displayUnit="mm" rowSpan="1" colSpan="2"/>
 </Cells></Property>
 <Property name="columnWidths" type="Spreadsheet::PropertyColumnWidths"><ColumnInfo Count="2"><Column name="A" width="120"/><Column name="B" width="80"/></ColumnInfo></Property>
 <Property name="rowHeights" type="Spreadsheet::PropertyRowHeights"><RowInfo Count="1"><Row name="2" height="45"/></RowInfo></Property>
 </Properties></Object>
 <Object name="Pad"><Properties Count="2">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="Sheet.width * 2"/></ExpressionEngine></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("spreadsheet");
    let width = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "width")
        .expect("width cell");
    assert_eq!(
        width.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(
        width.properties.get("address").map(String::as_str),
        Some("A1")
    );
    let pad = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .expect("pad");
    let length = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&pad.id) && parameter.name == "Length")
        .expect("pad length");
    assert_eq!(length.dependencies, vec![width.id.clone()]);
    let width_position = result
        .ir()
        .model
        .parameters
        .iter()
        .position(|parameter| parameter.name == "width")
        .expect("width position");
    let height_position = result
        .ir()
        .model
        .parameters
        .iter()
        .position(|parameter| parameter.name == "height")
        .expect("height position");
    assert!(width_position < height_position);
    let sheet = result.ir().model.spreadsheets.first().expect("sheet state");
    assert_eq!(sheet.feature.0, "fcstd:design:feature#Sheet");
    assert_eq!(sheet.cells.len(), 2);
    assert_eq!(
        sheet.column_widths,
        [
            cadmpeg_ir::SpreadsheetDimension {
                name: "A".into(),
                pixels: 120,
            },
            cadmpeg_ir::SpreadsheetDimension {
                name: "B".into(),
                pixels: 80,
            },
        ]
    );
    assert_eq!(
        sheet.row_heights,
        [cadmpeg_ir::SpreadsheetDimension {
            name: "2".into(),
            pixels: 45,
        }]
    );
    assert_eq!(
        sheet.merged_ranges,
        [cadmpeg_ir::SpreadsheetRange {
            start: "A1".into(),
            end: "B1".into(),
        }]
    );
    assert_valid_document(result.ir());
    let mut corrupted = result.ir().clone();
    corrupted.model.spreadsheets[0]
        .merged_ranges
        .push(cadmpeg_ir::SpreadsheetRange {
            start: "A1".into(),
            end: "A2".into(),
        });
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("merged ranges overlap")));
}

#[test]
fn preserves_forward_declared_feature_dependencies() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="First" Count="1"><Dep Name="Second"/></ObjectDeps>
<ObjectDeps Name="Second" Count="0"/>
<Object type="PartDesign::Feature" name="First"/><Object type="PartDesign::Feature" name="Second"/>
</Objects><ObjectData Count="2"><Object name="First"><Properties Count="0"/></Object><Object name="Second"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("forward dependency");
    let first = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("First"))
        .expect("first feature");
    let second = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Second"))
        .expect("second feature");

    assert_eq!(first.dependencies, std::slice::from_ref(&second.id));
    assert!(second.ordinal < first.ordinal);
    assert_valid_document(result.ir());
}

#[test]
fn orders_forward_linked_sketches_before_profile_consumers() {
    for property_name in ["Profile", "Sketch", "Base", "Source"] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="PartDesign::Pad" name="Pad"/><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="2"><Object name="Pad"><Properties Count="1"><Property name="{property_name}" type="App::PropertyLink"><Link value="Sketch"/></Property></Properties></Object><Object name="Sketch"><Properties Count="0"/></Object></ObjectData>
</Document>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("forward-linked sketch");
        let features = &result.ir().model.features;
        let pad = features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Pad"))
            .expect("pad feature");
        let sketch = features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Sketch"))
            .expect("sketch feature");

        assert!(sketch.ordinal < pad.ordinal, "property {property_name}");
        assert_valid_document(result.ir());
    }
}

#[test]
fn retains_native_dependency_cycles_as_a_stable_acyclic_feature_projection() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="First" Count="1"><Dep Name="Second"/></ObjectDeps>
<ObjectDeps Name="Second" Count="1"><Dep Name="First"/></ObjectDeps>
<Object type="PartDesign::Feature" name="First"/><Object type="PartDesign::Feature" name="Second"/>
</Objects><ObjectData Count="2"><Object name="First"><Properties Count="0"/></Object><Object name="Second"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("cyclic native dependency graph");
    let features = &result.ir().model.features;
    assert_eq!(features.len(), 2);
    assert_eq!(features[0].ordinal, 0);
    assert_eq!(features[1].ordinal, 1);
    assert!(features
        .iter()
        .all(|feature| matches!(feature.definition, FeatureDefinition::Native { .. })));
    assert!(features[0].dependencies.is_empty());
    assert_eq!(features[1].dependencies, [features[0].id.clone()]);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.code == "feature.cyclic-history")
            .count(),
        2
    );
    let objects = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert_eq!(objects[0].dependencies, [objects[1].id.clone()]);
    assert_eq!(objects[1].dependencies, [objects[0].id.clone()]);
    assert_valid_document(result.ir());
    assert!(crate::validate_native(result.ir()).is_empty());
}
