// SPDX-License-Identifier: Apache-2.0
//! Document.xml persistence-graph unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
pub(crate) fn schema_three_uses_the_object_envelope_and_defaults_file_version() {
    let document = r#"<Document SchemaVersion="3">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Legacy"/></Property></Properties>
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Thing"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let bytes = archive(document);
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("legacy inspection");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=3"));
    assert!(summary.notes.iter().any(|note| note == "FileVersion=0"));
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("schema-three decode");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].type_name, "App::FeaturePython");
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[1].links[0].object.as_deref(),
        Some(objects[0].id.as_str())
    );
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
pub(crate) fn schema_two_uses_the_feature_envelope_and_common_property_grammar() {
    let document = r#"<Document SchemaVersion="2" ProgramVersion="0.13">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Document"/></Property></Properties>
<Features Count="2"><Feature type="App::Feature" name="First"/><Feature type="App::FeaturePython" name="Second"/></Features>
<FeatureData Count="2"><Feature name="First"><Properties Count="0"/></Feature><Feature name="Second"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="First"/></Property></Properties></Feature></FeatureData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("schema-two decode");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(
        objects
            .iter()
            .map(|object| object.name.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[1].links[0].object.as_deref(),
        Some(objects[0].id.as_str())
    );
    assert!(objects.iter().all(|object| object.persistent_id.is_none()));
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_duplicate_root_property_containers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="First" type="App::PropertyString"><String value="one"/></Property></Properties>
<Properties Count="1"><Property name="Second" type="App::PropertyString"><String value="two"/></Property></Properties>
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("duplicate root Properties containers");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
pub(crate) fn legacy_schema_dispatch_rejects_wrong_envelopes_and_inconsistent_counts() {
    let cases = [
        r#"<Document SchemaVersion="2"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="3"><Features Count="0"/><FeatureData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="2"><Features Count="1"><Feature type="App::Feature" name="A"/></Features><FeatureData Count="0"><Feature name="A"/></FeatureData></Document>"#,
        r#"<Document SchemaVersion="2"><Features Count="1"><Feature type="App::Feature" name="A"/></Features><FeatureData Count="1"><Feature name="B"/></FeatureData></Document>"#,
    ];
    for document in cases {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default()
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn recovers_objects_dynamic_properties_links_and_side_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Demo"/></Property></Properties>
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="Body" Count="1" AllowPartial="2"><Dep Name="Sketch"/></ObjectDeps>
<ObjectDeps Name="Sketch" Count="0"/>
<Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
<Object type="PartDesign::Feature" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
<Object name="Body" Extensions="True"><Extensions Count="1"><Extension type="Demo::Extension" name="Demo"><Properties Count="1"><Property name="ExtensionValue" type="App::PropertyString"><String value="kept"/></Property></Properties></Extension></Extensions><Properties Count="4" TransientCount="1">
<_Property name="TransientState" type="App::PropertyInteger" status="8"/>
<Property name="Support" type="App::PropertyLinkSub" status="4" group="Attachment" doc="Support object" attr="2" ro="1" hide="0"><LinkSub value="Sketch" count="1"><Sub value="Face1"/></LinkSub></Property>
<Property name="Members" type="App::PropertyLinkList"><LinkList count="2"><Link value="Sketch"/><Link value=""/></LinkList></Property>
<Property name="Payload" type="App::PropertyFileIncluded"><File file="Payload.bin"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="" file="Shape.brp"/></Property>
</Properties></Object>
<Object name="Sketch"><Properties Count="0"></Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Payload.bin", b"payload"),
        (
            "Shape.brp",
            b"\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 4\n1 10 20 30 1 0 0\n7 0 0 2 3 2 0 0 0 5 0 0 10 0 0 0 3 1 3\n8 0 5 1 0 0 0 1 0 0\n9 2 0 0 1 1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 5\n1 0 0 0 0 0 1 1 0 0 0 1 0\n9 0 0 0 0 1 1 2 2 2 2 0 0 0 0 1 0 1 0 0 1 1 0 0 2 1 2 0 2 1 2\n6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 1\n3 1 0 0.01 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*",
        ),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode graph");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let extensions = namespace
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions");
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].dependency_allow_partial, Some(2));
    assert_eq!(objects[1].dependency_allow_partial, None);
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].owner, "fcstd:native:object#Body");
    let extension_value = properties
        .iter()
        .find(|property| property.name == "ExtensionValue")
        .expect("extension property");
    assert_eq!(extension_value.owner, extensions[0].id);
    assert_eq!(objects[0].dependencies, vec!["fcstd:native:object#Sketch"]);
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.owner, "fcstd:native:object#Body");
    assert_eq!(
        support.links[0].object.as_deref(),
        Some("fcstd:native:object#Sketch")
    );
    assert_eq!(support.family, crate::native::PropertyFamily::Link);
    assert_eq!(support.links[0].subelements, vec!["Face1"]);
    assert_eq!(
        support.dynamic.as_ref().and_then(|meta| meta.read_only),
        Some(true)
    );
    let members = properties
        .iter()
        .find(|property| property.name == "Members")
        .expect("members");
    assert_eq!(members.links.len(), 2);
    assert_eq!(
        members.links[0].object.as_deref(),
        Some("fcstd:native:object#Sketch")
    );
    assert_eq!(members.links[1].object.as_deref(), Some(""));
    let transient = properties
        .iter()
        .find(|property| property.name == "TransientState")
        .expect("transient");
    assert!(transient.transient);
    assert_eq!(transient.status, Some(8));
    let payload = properties
        .iter()
        .find(|property| property.name == "Payload")
        .expect("payload");
    assert_eq!(payload.side_entries, vec!["Payload.bin"]);
    let shape = properties
        .iter()
        .find(|property| property.name == "Shape")
        .expect("shape");
    assert_eq!(shape.family, crate::native::PropertyFamily::Geometry);
    assert_eq!(shape.side_entries, vec!["Shape.brp"]);
    let shape_payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .expect("shape payloads");
    assert_eq!(shape_payloads.len(), 1);
    assert_eq!(
        shape_payloads[0]
            .text
            .as_ref()
            .map(|facts| facts.topology_version),
        Some(1)
    );
    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.curves.len(), 8);
    match &result.ir().model.curves[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } => {
            assert_eq!([origin.x, origin.y, origin.z], [10.0, 20.0, 30.0]);
            assert_eq!([direction.x, direction.y, direction.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected curve {other:?}"),
    }
    match &result.ir().model.curves[1].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) => {
            assert_eq!(nurbs.degree, 2);
            assert_eq!(nurbs.control_points.len(), 3);
            assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            assert!(nurbs.weights.is_none());
        }
        other => panic!("unexpected curve {other:?}"),
    }
    assert_eq!(result.ir().model.procedural_curves.len(), 2);
    match &result.ir().model.procedural_curves[0].definition {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } => assert_eq!(*parameter_range, [0.0, 5.0]),
        other => panic!("unexpected trimmed construction {other:?}"),
    }
    match &result.ir().model.procedural_curves[1].definition {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
            distance,
            direction,
            ..
        } => {
            assert_eq!(*distance, 2.0);
            let direction = direction.expect("offset direction");
            assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 1.0]);
        }
        other => panic!("unexpected offset construction {other:?}"),
    }
    assert_eq!(result.ir().model.surfaces.len(), 7);
    match &result.ir().model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!([origin.x, origin.y, origin.z], [0.0, 0.0, 0.0]);
            assert_eq!([normal.x, normal.y, normal.z], [0.0, 0.0, 1.0]);
            assert_eq!([u_axis.x, u_axis.y, u_axis.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir().model.procedural_surfaces.len(), 4);
    assert!(matches!(
        result.ir().model.procedural_surfaces[0].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { .. }
    ));
    assert!(matches!(
        result.ir().model.procedural_surfaces[1].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            parameter_interval: None,
            ..
        }
    ));
    assert!(matches!(
        result.ir().model.procedural_surfaces[2].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
            u_sense: None,
            v_sense: None,
            ..
        }
    ));
    assert!(matches!(
        result.ir().model.procedural_surfaces[3].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset { .. }
    ));
    match &result.ir().model.surfaces[1].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) => {
            assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
            assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
            assert_eq!(nurbs.control_points.len(), 4);
            assert_eq!(nurbs.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
            assert!(nurbs.weights.is_none());
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices.len(), 3);
    assert_eq!(result.ir().model.tessellations[0].triangles, [[0, 1, 2]]);
    assert!(result.ir().model.tessellations[0].body.is_none());
    assert!(result.ir().model.tessellations[0].faces.is_empty());
    assert_eq!(
        result.ir().model.tessellations[0].chordal_deflection,
        Some(0.01)
    );
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let payload_entry = entries
        .iter()
        .find(|entry| entry.name == "Payload.bin")
        .expect("payload entry");
    assert_eq!(payload_entry.referenced_by, vec![payload.id.clone()]);
    assert_eq!(payload_entry.data, b"payload");
    let ledger = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    for entry in &entries {
        let mut spans = ledger
            .iter()
            .filter(|span| span.entry == entry.name)
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.start);
        assert_eq!(spans.first().map(|span| span.start), Some(0));
        assert_eq!(spans.last().map(|span| span.end), Some(entry.byte_len));
        assert!(spans.windows(2).all(|pair| pair[0].end == pair[1].start));
    }
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Shape.brp")
        .all(|span| span.classification == "typed"));
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Payload.bin")
        .all(|span| span.classification == "named_opaque"));
    assert!(ledger
        .iter()
        .any(|span| span.entry == "Document.xml" && span.classification == "typed"));
    assert!(ledger
        .iter()
        .any(|span| span.entry == "Document.xml" && span.classification == "structural"));
    let coverage = namespace
        .arena_as::<crate::native::ByteCoverageRecord>("byte_coverage")
        .expect("byte coverage");
    assert_eq!(coverage.len(), 1);
    assert!(coverage[0].exact);
    assert_eq!(coverage[0].logical_entry_count, entries.len());
    assert_eq!(
        coverage[0].logical_byte_len,
        entries.iter().map(|entry| entry.byte_len).sum::<u64>()
    );
    assert_eq!(
        coverage[0].classification_bytes.values().sum::<u64>(),
        coverage[0].logical_byte_len
    );
    assert!(coverage[0]
        .named_opaque_entries
        .contains(&"Payload.bin".to_owned()));
    let findings = crate::validate_native(result.ir());
    assert!(findings.is_empty(), "{findings:#?}");

    let mut corrupted = result.ir().clone();
    let missing_payload = ledger
        .iter()
        .filter(|span| span.entry != "Payload.bin")
        .cloned()
        .collect::<Vec<_>>();
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("logical_ledger", &missing_payload)
        .expect("replace logical ledger");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding
            .message
            .contains("logical ledger omits nonempty entry Payload.bin")));

    let mut corrupted = result.ir().clone();
    let mut invalid_owner = ledger.clone();
    invalid_owner
        .iter_mut()
        .find(|span| span.classification == "typed")
        .expect("typed span")
        .owner = None;
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("logical_ledger", &invalid_owner)
        .expect("replace logical ledger");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding.message.contains("invalid logical entry or owner")));

    let mut corrupted = result.ir().clone();
    let mut invalid_objects = objects.clone();
    invalid_objects[0].dependency_allow_partial = Some(0);
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("objects", &invalid_objects)
        .expect("replace objects");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding.message.contains("invalid partial-load capability")));
}

#[test]
fn rejects_inconsistent_object_dependency_envelopes() {
    let cases = [
        r#"<Document SchemaVersion="4"><Objects Count="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="1"/><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="B" Count="0"/><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
    ];

    for document in cases {
        assert!(matches!(
            crate::persistence::parse(document.as_bytes()),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_ambiguous_persistence_carriers() {
    let cases = [
        r#"<Document schemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4" schemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="0"/><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><ObjectDeps Name="B" Count="0"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="2"><Property name="Same" type="App::PropertyString"/><Property name="Same" type="App::PropertyString"/></Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="0"/><Extensions Count="0"/><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2"><Extension type="Vendor::First" name="Same"/><Extension type="Vendor::Second" name="Same"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2"><Extension type="Vendor::Same" name="First"/><Extension type="Vendor::Same" name="Second"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Properties Count="0"/><Extensions Count="0"/></Object></ObjectData></Document>"#,
    ];

    for document in cases {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default()
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn binds_nested_extension_properties_to_their_enclosing_record() {
    let document = r#"<Document SchemaVersion="4">
<Objects Count="1"><Object type="App::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2">
<Extension type="Vendor::First" name="First"><Properties Count="1"><Property name="FirstValue" type="App::PropertyString"><String value="first"/></Property></Properties></Extension>
<Extension type="Vendor::Second" name="Second"><Properties Count="1"><Property name="SecondValue" type="App::PropertyString"><String value="second"/></Property></Properties></Extension>
</Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    let graph = crate::persistence::parse(document.as_bytes()).expect("extension graph");
    let first = graph
        .extensions
        .iter()
        .find(|extension| extension.name == "First")
        .expect("first extension");
    let second = graph
        .extensions
        .iter()
        .find(|extension| extension.name == "Second")
        .expect("second extension");
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "FirstValue")
            .expect("first property")
            .owner,
        first.id
    );
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "SecondValue")
            .expect("second property")
            .owner,
        second.id
    );
}

#[test]
fn native_validation_rejects_duplicate_extension_identity() {
    let document = r#"<Document SchemaVersion="4">
<Objects Count="1"><Object type="App::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="1"><Extension type="Vendor::Extension" name="Extension"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("extension graph");
    let mut corrupted = result.ir().clone();
    let mut extensions = corrupted
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions")
        .clone();
    extensions.push(extensions[0].clone());
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("extensions", &extensions)
        .expect("replace extensions");
    let findings = crate::validate_native(&corrupted);
    assert!(findings.iter().any(|finding| {
        finding.message.contains("duplicate FCStd native identity")
            || finding.message.contains("duplicates extension name")
    }));
}

#[test]
fn unknown_property_runtime_names_do_not_select_a_family_by_substring() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="1"><Property name="Custom" type="Vendor::PropertyLinkAndPropertyString"><Link value="A"/></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("unknown property runtime type is retained");
    let property = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .into_iter()
        .find(|property| property.name == "Custom")
        .expect("custom property");
    assert_eq!(property.family, crate::native::PropertyFamily::Unknown);
    assert!(property.links.is_empty());
}
