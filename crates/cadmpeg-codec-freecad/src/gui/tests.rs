// SPDX-License-Identifier: Apache-2.0
//! GUI document transfer unit tests.

#![allow(clippy::doc_markdown, unused_imports)]

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
pub(crate) fn retains_ordered_document_level_gui_state() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1" active="UnrecognizedRootState">
 <ViewProviderData Count="0"/>
 <Camera settings="OrthographicCamera { position 1 2 3 }"/>
 <ClipPlane enabled="true" file="section.bin"/>
</Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("section.bin", b"section-state"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("GUI state");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let documents = namespace
        .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
        .expect("GUI documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].schema_version, Some(1));
    assert_eq!(documents[0].attributes["active"], "UnrecognizedRootState");
    assert_eq!(
        documents[0]
            .states
            .iter()
            .map(|state| state.kind.as_str())
            .collect::<Vec<_>>(),
        ["Camera", "ClipPlane"]
    );
    assert_eq!(
        documents[0].states[0].attributes["settings"],
        "OrthographicCamera { position 1 2 3 }"
    );
    assert_eq!(documents[0].states[1].side_entries, ["section.bin"]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let section = entries
        .iter()
        .find(|entry| entry.name == "section.bin")
        .expect("section asset");
    assert_eq!(section.referenced_by, [documents[0].states[1].id.clone()]);
    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    let presentation = &result.ir().model.presentation_documents[0];
    assert_eq!(presentation.schema_version, Some(1));
    assert_eq!(presentation.active_view, None);
    let camera = presentation.camera.as_ref().expect("camera state");
    assert_eq!(camera.position, None);
    assert_eq!(camera.orientation, None);
    assert_eq!(
        camera.properties["settings"],
        "OrthographicCamera { position 1 2 3 }"
    );
    assert_eq!(presentation.states[1].assets.len(), 1);
    assert!(presentation.states[1].assets[0].ends_with("section.bin"));
    assert!(result.ir().model.view_presentations.is_empty());
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted.model.presentation_documents[0]
        .camera
        .as_mut()
        .expect("camera state")
        .orientation = Some([0.0; 4]);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid document presentation state"));
}

#[test]
fn requires_one_camera_in_schema_one_gui_document() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    for camera_records in [
        "",
        r#"<Camera settings="first"/><Camera settings="second"/>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/>{camera_records}</Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("schema-one camera cardinality");

        assert!(matches!(
            error,
            cadmpeg_core::CodecError::Malformed(message)
                if message.contains("schema 1 requires one Camera record")
        ));
    }
}

#[test]
fn rejects_noncanonical_gui_schema_and_invalid_camera_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui_documents = [
        r#"<Document schemaVersion="1"><ViewProviderData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="2"><ViewProviderData Count="0"/><Camera settings="first"/><Camera settings="second"/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera><Position x="NaN" y="1" z="2"/></Camera></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera><Position x="0" y="0" z="0"/></Camera></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera orientation="NaN 0 0 1"/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera orientation="0 0 0 0"/></Document>"#,
    ];
    for gui in gui_documents {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid GUI schema or camera value");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn rejects_duplicate_camera_position_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="0"><!-- no providers --></ViewProviderData>
<Camera><Position x="1" y="2" z="3"/><Position x="4" y="5" z="6"/></Camera></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("duplicate camera positions");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::Malformed(message)
            if message.contains("multiple Position values")
    ));
}

#[test]
fn rejects_ambiguous_gui_containers_and_names() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    for gui in [
        br#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><ViewProviderData Count="0"/><Camera settings=""/></Document>"#.as_slice(),
        br#"<Document SchemaVersion="1"><ViewProviderData Count="2"><ViewProvider name="Model"><Properties Count="0"/></ViewProvider><ViewProvider name="Model"><Properties Count="0"/></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="0"/><Properties Count="0"/></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="2"><Property name="State" type="Vendor::PropertyState"><Value/></Property><Property name="State" type="Vendor::PropertyState"><Value/></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("ambiguous GUI graph");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn gui_property_counts_ignore_nested_extension_properties() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="0"/><Extension name="Nested"><Properties Count="1"><Property name="NestedValue" type="App::PropertyString"><String value="kept by extension"/></Property></Properties></Extension></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("nested extension properties do not alter the provider's direct count");
    let native = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("FCStd namespace");
    assert_eq!(
        native
            .arena_as::<crate::native::ObjectRecord>("objects")
            .expect("objects")
            .len(),
        1
    );
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_malformed_registered_gui_property_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="LineWidth" type="App::PropertyFloatConstraint"><Integer value="2"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("mismatched GUI value tag");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn validates_gui_link_value_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="8">
<Property name="Link" type="App::PropertyLink"><Link value=""/></Property>
<Property name="LinkList" type="App::PropertyLinkList"><LinkList count="2"><Link value=""/><Link value=""/></LinkList></Property>
<Property name="LinkSub" type="App::PropertyLinkSub"><LinkSub value="" count="1"><Sub value="Face1"/></LinkSub></Property>
<Property name="LinkSubList" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="" sub="Face1"/></LinkSubList></Property>
<Property name="XLink" type="App::PropertyXLink"><XLink name="" file=""/></Property>
<Property name="XLinkSub" type="App::PropertyXLinkSub"><XLink name="" file="" sub="Face1"/></Property>
<Property name="XLinkSubList" type="App::PropertyXLinkSubList"><XLinkSubList count="1"><XLink name="" file="" sub="Face1"/></XLinkSubList></Property>
<Property name="PlacementLink" type="App::PropertyPlacementLink"><Link value=""/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI link grammars");

    for invalid in [
        r#"<Property name="Link" type="App::PropertyLink"><Link value=""><Nested/></Link></Property>"#,
        r#"<Property name="LinkList" type="App::PropertyLinkList"><LinkList count="1"><Link value=""/><Link value=""/></LinkList></Property>"#,
        r#"<Property name="LinkSub" type="App::PropertyLinkSub"><LinkSub value="" count="1"><Sub value="Face1"><Nested/></Sub></LinkSub></Property>"#,
        r#"<Property name="LinkSubList" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="" sub="Face1"><Nested/></Link></LinkSubList></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid GUI link grammar");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn validates_gui_constraint_attribute_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="3">
<Property name="IntegerConstraint" type="App::PropertyIntegerConstraint"><Integer value="4" min="0" max="10" step="2"/></Property>
<Property name="FloatConstraint" type="App::PropertyFloatConstraint"><Float value="1.5" min="0" max="5" step="0.5"/></Property>
<Property name="Length" type="App::PropertyLength"><Float value="12" min="0" max="25" step="0.5"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI constraint attributes");

    for invalid in [
        r#"<Property name="IntegerConstraint" type="App::PropertyIntegerConstraint"><Integer value="4" min="zero"/></Property>"#,
        r#"<Property name="FloatConstraint" type="App::PropertyFloatConstraint"><Float value="1.5" step="not-a-float"/></Property>"#,
        r#"<Property name="Length" type="App::PropertyLength"><Float value="12" max="NaN"/></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid GUI constraint attribute");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn validates_gui_in_memory_list_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="5">
<Property name="Flags" type="App::PropertyBoolList"><BoolList value="101"/></Property>
<Property name="Names" type="App::PropertyStringList"><StringList count="2"><String value="alpha"/><String value="beta"/></StringList></Property>
<Property name="Values" type="App::PropertyIntegerList"><IntegerList count="2"><I v="2"/><I v="4"/></IntegerList></Property>
<Property name="UniqueValues" type="App::PropertyIntegerSet"><IntegerSet count="2"><I v="1"/><I v="4"/></IntegerSet></Property>
<Property name="Mapping" type="App::PropertyMap"><Map count="2"><Item key="alpha" value="one"/><Item key="beta" value="two"/></Map></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI in-memory lists");

    for invalid in [
        r#"<Property name="Flags" type="App::PropertyBoolList"><BoolList value="101"><Nested/></BoolList></Property>"#,
        r#"<Property name="Names" type="App::PropertyStringList"><StringList count="1"><String value="alpha"><Nested/></String></StringList></Property>"#,
        r#"<Property name="Values" type="App::PropertyIntegerList"><IntegerList count="1"><I v="2"><Nested/></I></IntegerList></Property>"#,
        r#"<Property name="UniqueValues" type="App::PropertyIntegerSet"><IntegerSet count="1"><I v="1"><Nested/></I></IntegerSet></Property>"#,
        r#"<Property name="Mapping" type="App::PropertyMap"><Map count="1"><Item key="alpha" value="one"><Nested/></Item></Map></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("nested GUI list value");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn accepts_and_validates_gui_custom_enumerations() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="2"><Property name="Pattern" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="None"/><Enum value="Cross"/></CustomEnumList></Property><Property name="ChildViewProvider" type="App::PropertyPersistentObject"><String value=""/><PersistentObject><State value="retained"/></PersistentObject></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("custom GUI enumeration");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let enumeration = properties
        .iter()
        .find(|property| property.name == "Pattern")
        .expect("enumeration");
    let persistent = properties
        .iter()
        .find(|property| property.name == "ChildViewProvider")
        .expect("persistent object");
    assert_eq!(enumeration.values.len(), 4);
    assert_eq!(persistent.values[0].tag, "String");
    assert_eq!(persistent.values[1].tag, "PersistentObject");
    assert_eq!(persistent.values[2].tag, "State");

    for invalid in [
        br#"<Integer value="1"/><CustomEnumList count="0"/>"#.as_slice(),
        br#"<Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="None"/></CustomEnumList>"#.as_slice(),
    ] {
        let gui = [
            br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="Pattern" type="App::PropertyEnumeration">"#.as_slice(),
            invalid,
            br#"</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        ]
        .concat();
        assert!(FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", &gui),
                ])),
                &DecodeOptions::default(),
            )
            .is_err());
    }
}

#[test]
fn validates_sketcher_visual_layer_list_with_the_producer_type_token() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Sketch"><Properties Count="1"><Property name="VisualLayerList" type="BadType">
<VisualLayerList count="2"><VisualLayer visible="true" linePattern="65535" lineWidth="3.0"/><VisualLayer visible="false" linePattern="32382" lineWidth="1.5"/></VisualLayerList>
</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("Sketcher visual layers");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let property = properties
        .iter()
        .find(|property| property.name == "VisualLayerList")
        .expect("visual layers");
    assert_eq!(property.type_name, "BadType");
    assert_eq!(
        property
            .values
            .iter()
            .map(|value| value.tag.as_str())
            .collect::<Vec<_>>(),
        ["VisualLayerList", "VisualLayer", "VisualLayer"]
    );
    let logical = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = logical
        .iter()
        .find(|span| span.owner.as_deref() == Some(property.id.as_str()))
        .expect("visual layer span");
    assert_eq!(span.classification, "typed");
    assert!(crate::validate_native(result.ir()).is_empty());

    for value in [
        br#"<VisualLayerList count="1"><VisualLayer visible="maybe" linePattern="1" lineWidth="1"/></VisualLayerList>"#.as_slice(),
        br#"<VisualLayerList count="2"><VisualLayer visible="true" linePattern="1" lineWidth="1"/></VisualLayerList>"#.as_slice(),
        br#"<VisualLayerList count="1"><VisualLayer visible="true" linePattern="1" lineWidth="NaN"/></VisualLayerList>"#.as_slice(),
    ] {
        let gui = [
            br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Sketch"><Properties Count="1"><Property name="VisualLayerList" type="BadType">"#.as_slice(),
            value,
            br#"</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        ]
        .concat();
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", &gui),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid visual layer list");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    }
}

#[test]
fn validates_dynamic_gui_property_registry_and_side_lists() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="12">
<Property name="FloatList" type="App::PropertyFloatList"><FloatList file="FloatList"/></Property>
<Property name="VectorList" type="App::PropertyVectorList"><VectorList file="VectorList"/></Property>
<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>
<Property name="BoolList" type="App::PropertyBoolList"><BoolList value="101"/></Property>
<Property name="IntegerSet" type="App::PropertyIntegerSet"><IntegerSet count="3"><I v="1"/><I v="4"/><I v="9"/></IntegerSet></Property>
<Property name="Strings" type="App::PropertyStringList"><StringList count="2"><String value="alpha"/><String value="beta"/></StringList></Property>
<Property name="Map" type="App::PropertyMap"><Map count="2"><Item key="alpha" value="one"/><Item key="beta" value="two"/></Map></Property>
<Property name="Matrix" type="App::PropertyMatrix"><PropertyMatrix a11="1" a12="0" a13="0" a14="0" a21="0" a22="1" a23="0" a24="0" a31="0" a32="0" a33="1" a34="0" a41="0" a42="0" a43="0" a44="1"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="Rotation" type="App::PropertyRotation"><PropertyRotation A="0" Ox="0" Oy="0" Oz="1"/></Property>
<Property name="Uuid" type="App::PropertyUUID"><Uuid value="01234567-89ab-cdef-0123-456789abcdef"/></Property>
<Property name="Path" type="App::PropertyPath"><Path value="/var/tmp/path"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let mut float_list = Vec::new();
    float_list.extend_from_slice(&2_u32.to_le_bytes());
    float_list.extend_from_slice(&1.25_f64.to_le_bytes());
    float_list.extend_from_slice(&2.5_f64.to_le_bytes());
    let mut vector_list = Vec::new();
    vector_list.extend_from_slice(&1_u32.to_le_bytes());
    for value in [1.0_f64, 2.0, 3.0] {
        vector_list.extend_from_slice(&value.to_le_bytes());
    }
    let mut placement_list = Vec::new();
    placement_list.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        placement_list.extend_from_slice(&value.to_le_bytes());
    }
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("FloatList", &float_list),
                ("VectorList", &vector_list),
                ("PlacementList", &placement_list),
            ])),
            &DecodeOptions::default(),
        )
        .expect("dynamic GUI registry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties.len(), 12);
    assert!(properties.iter().all(|property| {
        crate::gui::has_registered_property_grammar(&property.name, &property.type_name)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());

    let bad_float_list = [0_u32.to_le_bytes().as_slice(), &[0xff]].concat();
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("FloatList", &bad_float_list),
                ("VectorList", &vector_list),
                ("PlacementList", &placement_list),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("trailing dynamic float-list bytes");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn validates_the_complete_loaded_dynamic_gui_registry() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="14">
<Property name="Precision" type="App::PropertyPrecision"><Float value="0.001"/></Property>
<Property name="VectorDistance" type="App::PropertyVectorDistance"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
<Property name="Position" type="App::PropertyPosition"><PropertyVector valueX="4" valueY="5" valueZ="6"/></Property>
<Property name="Direction" type="App::PropertyDirection"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
<Property name="PlacementLink" type="App::PropertyPlacementLink"><Link value=""/></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="2"/></ExpressionEngine></Property>
<Property name="MaterialReference" type="Materials::PropertyMaterial"><PropertyMaterial uuid="6b80c8f7-cf5f-4e7d-a6e3-88b3cd1db5a3"/></Property>
<Property name="PartShape" type="Part::PropertyPartShape"><Part file="ProbeShape"/><ElementMap/></Property>
<Property name="GeometryList" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="ShapeHistory" type="Part::PropertyShapeHistory"/>
<Property name="FilletEdges" type="Part::PropertyFilletEdges"><FilletEdges file="ProbeFillet"/></Property>
<Property name="ShapeCache" type="Part::PropertyShapeCache"/>
<Property name="TopoShapeList" type="Part::PropertyTopoShapeList"><ShapeList count="2"><TopoShape file="ProbeShapeList.0.brp"/><TopoShape file="ProbeShapeList.1.brp"/></ShapeList></Property>
<Property name="ConstraintList" type="Sketcher::PropertyConstraintList"><ConstraintList count="0"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("ProbeShape", b"shape-side-entry"),
                ("ProbeFillet", &0_u32.to_le_bytes()),
                ("ProbeShapeList.0.brp", b"shape-list-0"),
                ("ProbeShapeList.1.brp", b"shape-list-1"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("complete dynamic GUI registry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties.len(), 14);
    assert!(properties.iter().all(|property| {
        crate::gui::has_registered_property_grammar(&property.name, &property.type_name)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());

    let logical = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    assert!(properties.iter().all(|property| {
        logical.iter().any(|span| {
            span.owner.as_deref() == Some(property.id.as_str()) && span.classification == "typed"
        })
    }));
}

#[test]
fn rejects_truncated_gui_material_list_payload() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="3"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("ShapeAppearance", &1_u32.to_le_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("truncated GUI material list");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn retains_unregistered_gui_property_values_without_semantic_dispatch() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ExtensionState" type="Vendor::PropertyState"><VendorState mode="custom"><Nested value="kept"/></VendorState></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("unregistered GUI property");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties[0].type_name, "Vendor::PropertyState");
    assert_eq!(properties[0].values[0].tag, "VendorState");
    assert_eq!(properties[0].values[1].tag, "Nested");
}

#[test]
fn retains_unregistered_gui_side_entries_as_opaque_archive_members() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ExtensionState" type="Vendor::PropertyState"><VendorState file="state.bin" mode="custom"><Nested value="kept"/></VendorState></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let payload = [
        0xd0, 0xc0, 0xb0, 0xa0, 0x00, 0x00, 0x01, 0x00, 0x7f, 0x45, 0x4c, 0x46,
    ];
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("state.bin", &payload),
            ])),
            &DecodeOptions::default(),
        )
        .expect("unregistered GUI side entry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let property = properties
        .iter()
        .find(|property| property.name == "ExtensionState")
        .expect("extension property");
    assert_eq!(property.side_entries, ["state.bin"]);

    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.name == "state.bin")
        .expect("state entry");
    assert_eq!(
        entry.referenced_by.as_slice(),
        std::slice::from_ref(&property.id)
    );
    assert_eq!(entry.data, payload);

    let logical = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = logical
        .iter()
        .find(|span| span.entry == entry.name)
        .expect("state span");
    assert_eq!(span.classification, "named_opaque");
    assert_eq!(span.owner.as_deref(), Some(entry.id.as_str()));
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn does_not_treat_gui_external_links_as_archive_members() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="Remote" type="App::PropertyXLink"><XLink file="External.FCStd" name="Body"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("external GUI link");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let property = properties
        .iter()
        .find(|property| property.name == "Remote")
        .expect("external-link property");
    assert!(property.side_entries.is_empty());
    assert!(namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries")
        .iter()
        .all(|entry| entry.name != "External.FCStd"));
    assert!(crate::validate_native(result.ir()).is_empty());
}
