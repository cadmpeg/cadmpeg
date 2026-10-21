// SPDX-License-Identifier: Apache-2.0
//! Application-domain census unit tests.

use crate::test_support::test_archive::{archive_entries, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fmt::Write as _;
use std::io::Cursor;

#[test]
fn censuses_application_domains_and_keeps_python_payloads_inert() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5" Dependencies="1">
 <ObjectDeps Name="Mesh" Count="1"><Dep Name="Points"/></ObjectDeps>
 <ObjectDeps Name="Points" Count="0"/>
 <ObjectDeps Name="Analysis" Count="0"/>
 <ObjectDeps Name="Toolpath" Count="0"/>
 <ObjectDeps Name="Local" Count="0"/>
 <Object type="Mesh::Feature" name="Mesh" id="1"/>
 <Object type="Points::Feature" name="Points" id="2"/>
 <Object type="Fem::FemAnalysis" name="Analysis" id="3"/>
 <Object type="Path::FeaturePython" name="Toolpath" id="4"/>
 <Object type="LocalType" name="Local" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Mesh"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Points"/></Property></Properties></Object>
 <Object name="Points"><Properties Count="0"/></Object>
 <Object name="Analysis"><Properties Count="1"><Property name="Report" type="App::PropertyFileIncluded"><FileIncluded file="analysis.dat"/></Property></Properties></Object>
 <Object name="Toolpath"><Properties Count="1"><Property name="Proxy" type="App::PropertyPythonObject"><PythonObject class="ToolController">serialized-but-inert</PythonObject></Property></Properties></Object>
 <Object name="Local"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("analysis.dat", b"finite-element-results"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("application census");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let records = namespace
        .arena_as::<serde_json::Value>("applications")
        .expect("applications");
    let xml_bytes = |record: &serde_json::Value| {
        let id = record
            .get("property")
            .unwrap_or(&record["object"])
            .as_str()
            .unwrap();
        crate::test_support::source_fidelity::bytes(result.source_fidelity(), id)
    };
    assert_eq!(records.len(), 5);
    let by_domain = records
        .iter()
        .map(|record| (record["domain"].as_str().unwrap(), record))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_domain["Mesh"]["dependencies"],
        serde_json::json!(["fcstd:native:object#Points"])
    );
    assert_eq!(
        by_domain["Fem"]["side_entries"],
        serde_json::json!(["analysis.dat"])
    );
    assert_eq!(by_domain["Path"]["inert_payload"], true);
    assert_eq!(by_domain["Mesh"]["inert_payload"], false);
    assert_eq!(by_domain["Unqualified"]["type_name"], "LocalType");
    let report = &by_domain["Fem"]["property_records"][0];
    assert_eq!(report["object"], by_domain["Fem"]["object"]);
    let report_source = result
        .source_fidelity()
        .retained_record(report["property"].as_str().unwrap())
        .expect("property source");
    assert!(report_source.offset() < report_source.end_offset());
    assert_eq!(
        report_source.byte_len(),
        cadmpeg_core::decode::u64_from_index(xml_bytes(report).len())
    );
    assert_eq!(
        report_source.sha256(),
        cadmpeg_ir::hash::sha256_hex(xml_bytes(report))
    );
    assert_eq!(report["payloads"].as_array().unwrap().len(), 1);
    let payload = &report["payloads"][0];
    assert_eq!(payload["name"], "analysis.dat");
    let payload_source = result
        .source_fidelity()
        .retained_record(payload["entry"].as_str().unwrap())
        .expect("payload source");
    assert_eq!(
        payload_source.byte_len(),
        cadmpeg_core::decode::u64_from_index(b"finite-element-results".len())
    );
    assert!(payload.get("data").is_none());
    assert_eq!(
        payload_source.sha256(),
        cadmpeg_ir::hash::sha256_hex(b"finite-element-results")
    );
    let python = &by_domain["Path"]["property_records"][0];
    assert_eq!(python["inert"], true);
    assert!(String::from_utf8_lossy(xml_bytes(python)).contains("serialized-but-inert"));
    assert!(records.iter().all(|record| {
        assert!(record.get("data").is_none());
        assert!(record["property_records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|property| property.get("data").is_none()));
        let source = result
            .source_fidelity()
            .retained_record(record["object"].as_str().unwrap())
            .expect("object source");
        source.offset() < source.end_offset()
            && source.byte_len() == cadmpeg_core::decode::u64_from_index(xml_bytes(record).len())
            && source.sha256() == cadmpeg_ir::hash::sha256_hex(xml_bytes(record))
    }));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut altered = records;
    let payload = &mut altered
        .iter_mut()
        .find(|record| record["domain"] == "Fem")
        .unwrap()["property_records"][0]["payloads"][0];
    payload["entry"] = serde_json::json!("fcstd:native:entry#different-payload");
    let mut edited = result.ir().clone();
    edited
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "applications",
            &altered,
        )
        .unwrap();
    assert!(crate::test_support::validate_native(&edited)
        .iter()
        .any(|finding| {
            finding.message
                == "FCStd application records do not match their typed objects and entries"
        }));
}

#[test]
fn absent_object_data_keeps_empty_ranges_and_digest() {
    let objects = [crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Absent".into(),
            "Absent".into(),
        )
        .expect("object identity"),
        type_name: "Vendor::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    }];
    let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
    super::install(
        &cadmpeg_test_support::service_decode_context(),
        &mut namespace,
        &objects,
        &[],
        &[],
    )
    .unwrap();
    assert!(objects[0].data.is_none());
    let records = namespace
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    assert!(records[0].get("data").is_none());
    // §2.4: absence of source data does not put invented layout facts in CADIR.
    for field in ["byte_start", "byte_end", "byte_len", "sha256"] {
        assert!(records[0].get(field).is_none());
    }
    assert!(super::matches_native(
        &cadmpeg_test_support::service_decode_context(),
        &namespace,
        &objects,
        &[],
        &[]
    )
    .unwrap());
}

#[test]
fn unregistered_application_payloads_remain_whole_named_opaque_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Vendor::Feature" name="Owner"/></Objects>
<ObjectData Count="1"><Object name="Owner"><Properties Count="1">
<Property name="Payload" type="Vendor::PropertyMeshLike"><Mesh file="Payload.bin"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let payload = [
        0xd0, 0xc0, 0xb0, 0xa0, 0x00, 0x00, 0x01, 0x00, 0x7f, 0x45, 0x4c, 0x46,
    ];
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Payload.bin", &payload),
            ])),
            &DecodeOptions::default(),
        )
        .expect("opaque vendor payload");
    assert!(result.ir().model.tessellations.is_empty());
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(properties[0].type_name, "Vendor::PropertyMeshLike");
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.name() == "Payload.bin")
        .expect("payload entry");
    let spans: Vec<crate::native::LogicalSpan> =
        crate::test_support::source_fidelity::layout(result.source_fidelity(), "logical-ledger");
    let span = spans
        .iter()
        .find(|span| span.entry == entry.name())
        .expect("payload span");
    assert_eq!(span.span.start(), 0);
    assert_eq!(
        span.span.end(),
        cadmpeg_core::decode::u64_from_index(payload.len())
    );
    assert_eq!(span.classification.as_str(), "named_opaque");
    assert_eq!(span.classification.owner(), Some(entry.id()));
    assert_eq!(
        crate::test_support::source_fidelity::bytes(result.source_fidelity(), entry.id()),
        payload
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}

#[test]
fn producer_specific_side_entries_remain_whole_until_their_grammar_is_registered() {
    let cases = [
        (
            "Distances",
            "Inspection::PropertyDistanceList",
            r#"<FloatList file="Distances"/>"#,
            b"\x02\x00\x00\x00\x00\x00\xc0?\x00\x00 @".as_slice(),
        ),
        (
            "FilletEdges",
            "Part::PropertyFilletEdges",
            r#"<FilletEdges file="FilletEdges"/>"#,
            b"fillet-side-entry".as_slice(),
        ),
        (
            "Shapes.0.brp",
            "Part::PropertyTopoShapeList",
            r#"<ShapeList count="1"><TopoShape file="Shapes.0.brp"/></ShapeList>"#,
            b"shape-side-entry".as_slice(),
        ),
        (
            "GreyValues",
            "Points::PropertyGreyValueList",
            r#"<FloatList file="GreyValues"/>"#,
            b"grey-side-entry".as_slice(),
        ),
        (
            "PointNormals",
            "Points::PropertyNormalList",
            r#"<VectorList file="PointNormals"/>"#,
            b"point-normal-side-entry".as_slice(),
        ),
        (
            "MeshNormals",
            "Mesh::PropertyNormalList",
            r#"<VectorList file="MeshNormals"/>"#,
            b"mesh-normal-side-entry".as_slice(),
        ),
        (
            "PointCurvatures",
            "Points::PropertyCurvatureList",
            r#"<CurvatureList file="PointCurvatures"/>"#,
            b"point-curvature-side-entry".as_slice(),
        ),
        (
            "MeshCurvatures",
            "Mesh::PropertyCurvatureList",
            r#"<CurvatureList file="MeshCurvatures"/>"#,
            b"mesh-curvature-side-entry".as_slice(),
        ),
        (
            "Material",
            "Mesh::PropertyMaterial",
            r#"<Material file="Material"/>"#,
            b"material-side-entry".as_slice(),
        ),
        (
            "FemMesh.unv",
            "Fem::PropertyFemMesh",
            r#"<FemMesh file="FemMesh.unv"/>"#,
            b"fem-side-entry".as_slice(),
        ),
        (
            "Data.vtu",
            "Fem::PropertyPostDataObject",
            r#"<Data file="Data.vtu"/>"#,
            b"data-side-entry".as_slice(),
        ),
        (
            "Toolpath.nc",
            "Path::PropertyPath",
            r#"<Path file="Toolpath.nc" version="1"><Center x="0" y="0" z="0"/></Path>"#,
            b"path-side-entry".as_slice(),
        ),
    ];
    let properties = cases.iter().enumerate().fold(
        String::new(),
        |mut properties, (index, (_, type_name, value, _))| {
            write!(
                properties,
                "<Property name=\"Payload{index}\" type=\"{type_name}\">{value}</Property>"
            )
            .expect("writing a String cannot fail");
            properties
        },
    );
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Vendor::Feature" name="Owner"/></Objects>
<ObjectData Count="1"><Object name="Owner"><Properties Count="{}">{properties}</Properties></Object></ObjectData>
</Document>"#,
        cases.len()
    );
    let mut archive = vec![("Document.xml", document.as_bytes())];
    archive.extend(cases.iter().map(|(name, _, _, payload)| (*name, *payload)));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&archive)),
            &DecodeOptions::default(),
        )
        .expect("producer-specific opaque side entries");

    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let spans: Vec<crate::native::LogicalSpan> =
        crate::test_support::source_fidelity::layout(result.source_fidelity(), "logical-ledger");
    for (name, _, _, payload) in cases {
        let entry = entries
            .iter()
            .find(|entry| entry.name() == name)
            .expect("side entry");
        assert_eq!(
            crate::test_support::source_fidelity::bytes(result.source_fidelity(), entry.id()),
            payload
        );
        assert_eq!(
            entry.byte_len(),
            cadmpeg_core::decode::u64_from_index(payload.len())
        );
        let span = spans
            .iter()
            .find(|span| span.entry == name)
            .expect("side-entry span");
        assert_eq!(span.span.start(), 0);
        assert_eq!(
            span.span.end(),
            cadmpeg_core::decode::u64_from_index(payload.len())
        );
        assert_eq!(span.classification.as_str(), "named_opaque");
        assert_eq!(span.classification.owner(), Some(entry.id()));
    }
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}

#[test]
fn application_repeated_payloads_borrow_the_cached_digest() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Owner".into(),
            "Owner".into(),
        )
        .expect("object identity"),
        type_name: "Vendor::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::new(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let properties = ["First", "Second"].map(|name| crate::native::PropertyRecord {
        id: format!("fcstd:native:property#Owner:{name}"),
        owner: object.id().clone(),
        name: name.into(),
        type_name: "App::PropertyFileIncluded".into(),
        family: crate::native::PropertyFamily::File,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["shared.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML"),
    });
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#shared.bin".into(),
        "shared.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![7; 4096],
    );
    let identity = entry.id().as_ptr();
    let objects = [object];
    let entries = [entry];
    let records = super::wire_records(&objects, &properties, &entries);
    for property in &records[0].property_records {
        // §2.4 moves the digest to fidelity; the census borrows the typed entry identity.
        assert_eq!(property.payloads[0].entry.as_ptr(), identity);
        assert_eq!(property.payloads[0].entry, entries[0].id());
    }
}
