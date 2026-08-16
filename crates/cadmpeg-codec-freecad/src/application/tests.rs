// SPDX-License-Identifier: Apache-2.0
//! Application-domain census unit tests.

#![allow(unused_imports)]

use crate::test_support::*;
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
        .arena_as::<crate::native::ApplicationRecord>("applications")
        .expect("applications");
    assert_eq!(records.len(), 5);
    let by_domain = records
        .iter()
        .map(|record| (record.domain.as_str(), record))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_domain["Mesh"].dependencies,
        ["fcstd:native:object#Points"]
    );
    assert_eq!(by_domain["Fem"].side_entries, ["analysis.dat"]);
    assert!(by_domain["Path"].inert_payload);
    assert!(!by_domain["Mesh"].inert_payload);
    assert_eq!(by_domain["Unqualified"].type_name, "LocalType");
    let report = &by_domain["Fem"].property_records[0];
    assert_eq!(report.object, by_domain["Fem"].object);
    assert!(report.byte_start < report.byte_end);
    assert_eq!(report.byte_len, report.data.len() as u64);
    assert_eq!(report.sha256, cadmpeg_ir::hash::sha256_hex(&report.data));
    assert_eq!(report.payloads.len(), 1);
    assert_eq!(report.payloads[0].name, "analysis.dat");
    assert_eq!(report.payloads[0].data, b"finite-element-results");
    assert_eq!(
        report.payloads[0].sha256,
        cadmpeg_ir::hash::sha256_hex(&report.payloads[0].data)
    );
    let python = &by_domain["Path"].property_records[0];
    assert!(python.inert);
    assert!(String::from_utf8_lossy(&python.data).contains("serialized-but-inert"));
    assert!(records.iter().all(|record| {
        record.byte_start < record.byte_end
            && record.byte_len == record.data.len() as u64
            && record.sha256 == cadmpeg_ir::hash::sha256_hex(&record.data)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    let mut stale_records = records.clone();
    stale_records[0].property_records[0].sha256 = "0".repeat(64);
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("applications", &stale_records)
        .expect("replace application records");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding
            .message
            .contains("application preservation records do not match authoritative bytes")));
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
        .find(|entry| entry.name == "Payload.bin")
        .expect("payload entry");
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = spans
        .iter()
        .find(|span| span.entry == entry.name)
        .expect("payload span");
    assert_eq!(span.start, 0);
    assert_eq!(span.end, payload.len() as u64);
    assert_eq!(span.classification, "named_opaque");
    assert_eq!(span.owner.as_deref(), Some(entry.id.as_str()));
    assert_eq!(entry.data, payload);
    assert!(crate::validate_native(result.ir()).is_empty());
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
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    for (name, _, _, payload) in cases {
        let entry = entries
            .iter()
            .find(|entry| entry.name == name)
            .expect("side entry");
        assert_eq!(entry.data, payload);
        assert_eq!(entry.byte_len, payload.len() as u64);
        let span = spans
            .iter()
            .find(|span| span.entry == name)
            .expect("side-entry span");
        assert_eq!(span.start, 0);
        assert_eq!(span.end, payload.len() as u64);
        assert_eq!(span.classification, "named_opaque");
        assert_eq!(span.owner.as_deref(), Some(entry.id.as_str()));
    }
    assert!(crate::validate_native(result.ir()).is_empty());
}
