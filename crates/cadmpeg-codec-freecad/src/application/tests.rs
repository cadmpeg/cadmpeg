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
        let id = record["property"]
            .as_str()
            .or_else(|| record["object"].as_str())
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
    let report_layout =
        &result.source_fidelity().record_layout()[report["property"].as_str().unwrap()];
    assert_ne!(report_layout.byte_len(), 0);
    assert_eq!(
        report_layout.byte_len(),
        cadmpeg_core::decode::u64_from_index(xml_bytes(report).len())
    );
    assert_eq!(
        xml_bytes(report),
        &document.as_bytes()[usize::try_from(report_layout.offset()).unwrap()
            ..usize::try_from(report_layout.offset() + report_layout.byte_len()).unwrap()]
    );
    assert!(report.get("byte_len").is_none() && report.get("sha256").is_none());
    assert_eq!(report["payloads"].as_array().unwrap().len(), 1);
    let payload = &report["payloads"][0];
    assert_eq!(payload["name"], "analysis.dat");
    let payload_bytes = crate::test_support::source_fidelity::entry_bytes(
        result.source_fidelity(),
        payload["entry"].as_str().unwrap(),
    );
    assert_eq!(payload_bytes, b"finite-element-results");
    assert_eq!(payload_bytes.len(), b"finite-element-results".len());
    assert_eq!(
        cadmpeg_ir::hash::sha256_hex(&payload_bytes),
        cadmpeg_ir::hash::sha256_hex(b"finite-element-results")
    );
    assert!(payload.get("data").is_none());
    assert!(payload.get("byte_len").is_none() && payload.get("sha256").is_none());
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
        assert!(
            record.get("byte_start").is_none()
                && record.get("byte_end").is_none()
                && record.get("byte_len").is_none()
                && record.get("sha256").is_none()
        );
        let bytes = xml_bytes(record);
        let layout = &result.source_fidelity().record_layout()[record["object"].as_str().unwrap()];
        let start = usize::try_from(layout.pointers()["byte_start"]).unwrap();
        let end = usize::try_from(layout.pointers()["byte_end"]).unwrap();
        assert!(start < end);
        assert_eq!(bytes.len(), end - start);
        assert_eq!(
            cadmpeg_ir::hash::sha256_hex(bytes),
            cadmpeg_ir::hash::sha256_hex(&document.as_bytes()[start..end])
        );
        true
    }));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut altered = records;
    let payload = &mut altered
        .iter_mut()
        .find(|record| record["domain"] == "Fem")
        .unwrap()["property_records"][0]["payloads"][0];
    let replacement = b"different internally consistent bytes";
    payload["byte_len"] = serde_json::json!(replacement.len());
    payload["sha256"] = serde_json::json!(cadmpeg_ir::hash::sha256_hex(replacement));
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
            finding
                .message
                .contains("application preservation records do not match authoritative bytes")
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
    let source_fidelity = cadmpeg_ir::SourceFidelity::default();
    assert!(source_fidelity
        .record_layout()
        .get(objects[0].id())
        .is_none());
    let records = namespace
        .arena_as::<serde_json::Value>("applications")
        .unwrap();
    assert!(records[0].get("data").is_none());
    // #214: authored records without source do not invent zero-length evidence.
    for field in ["byte_start", "byte_end", "byte_len", "sha256"] {
        assert!(records[0].get(field).is_none());
    }
    assert_eq!(records[0]["object"], objects[0].id().as_str());
    assert_eq!(records[0]["type_name"], "Vendor::Feature");
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
    let spans = result
        .source_fidelity()
        .freecad_ledgers()
        .values()
        .next()
        .expect("FCStd source ledgers")
        .logical_ledger
        .clone();
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
        crate::test_support::source_fidelity::entry_bytes(result.source_fidelity(), entry.id()),
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
    let original = archive_entries(&archive);
    let result = FcstdCodec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .expect("registered entries recover locally and keep every independent member");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &original,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let physical = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let mut expected_failures = 0;
    for (name, _, _, payload) in &cases {
        let view = physical.open(&ctx, name).unwrap();
        assert_eq!(view.window(), *payload);
        let entry = assert_whole_named_entry(&result, name, payload);
        if *name == "Distances" {
            assert_distance_slots(&result, &entry);
        }
        let level = match *name {
            "FilletEdges" => Some("freecad.fillet_edges_entry"),
            "Shapes.0.brp" => Some("freecad.brep_text_header"),
            "GreyValues" | "PointNormals" | "MeshNormals" | "PointCurvatures"
            | "MeshCurvatures" => Some("freecad.f32_record_list_entry"),
            _ => None,
        };
        let homes = if *name == "Material" {
            vec![
                ("freecad.mesh_material_binding", 0, 4),
                (
                    "freecad.mesh_material_array",
                    4,
                    u64::try_from(payload.len()).unwrap() - 4,
                ),
            ]
        } else {
            level
                .map(|level| (level, 0, u64::try_from(payload.len()).unwrap()))
                .into_iter()
                .collect()
        };
        if !homes.is_empty() {
            expected_failures += homes.len();
            let capture = ctx.capture_part_extent(view).unwrap();
            let failed = result
                .source_fidelity()
                .parts()
                .values()
                .filter(|part| {
                    part.stream() == capture.stream()
                        && matches!(
                            part.state(),
                            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                        )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                failed.len(),
                homes.len(),
                "exact independent failure homes for {name}"
            );
            let errors = result
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code.local_code() == "record.unreadable"
                        && loss
                            .provenance
                            .as_ref()
                            .is_some_and(|source| source.stream_name() == Some(capture.stream()))
                })
                .collect::<Vec<_>>();
            assert_eq!(errors.len(), homes.len());
            for (level, offset, length) in homes {
                let found = failed
                    .iter()
                    .filter(|part| {
                        part.level().as_str() == level
                            && part.offset() == offset
                            && part.byte_len() == length
                    })
                    .collect::<Vec<_>>();
                assert_eq!(found.len(), 1, "one original bounded {level}");
                let found = errors
                    .iter()
                    .filter(|error| {
                        error.provenance.as_ref().is_some_and(|source| {
                            source.offset == offset && source.byte_len() == Some(length)
                        })
                    })
                    .collect::<Vec<_>>();
                assert_eq!(found.len(), 1);
                assert_eq!(found[0].severity, cadmpeg_ir::report::Severity::Error);
            }
        }
    }
    assert_eq!(
        result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| matches!(
                part.state(),
                cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            ))
            .count(),
        expected_failures
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        expected_failures
    );
    crate::fidelity::b1::assert_classified_spans(&result);
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}
fn assert_whole_named_entry(
    result: &cadmpeg_ir::codec::DecodeResult,
    name: &str,
    payload: &[u8],
) -> crate::native::EntryRecord {
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let entry = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries")
        .into_iter()
        .find(|entry| entry.name() == name)
        .expect("side entry");
    let spans = &result
        .source_fidelity()
        .freecad_ledgers()
        .values()
        .next()
        .expect("FCStd source ledgers")
        .logical_ledger;
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(result.source_fidelity(), entry.id()),
        payload
    );
    let layout = &result.source_fidelity().record_layout()[entry.id()];
    let header = result
        .source_fidelity()
        .parts()
        .values()
        .find(|part| {
            part.level().as_str() == "zip.local_header"
                && part.stream() == layout.stream()
                && part.offset() == layout.offset()
        })
        .expect("entry identity is produced from its actual local header");
    assert_eq!(layout.byte_len(), header.byte_len());
    assert_eq!(
        header.state(),
        &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
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
    entry
}

fn assert_distance_slots(
    result: &cadmpeg_ir::codec::DecodeResult,
    entry: &crate::native::EntryRecord,
) {
    let property = crate::native::native_child_id(
        "property",
        &crate::native::native_id("object", "Owner"),
        "Payload0",
    );
    assert!(entry.referenced_by().iter().any(|owner| owner == &property));
    let [crate::native::entry_contents::EntryContents::F32 {
        values: crate::native::entry_contents::F32Values::Scalar(distances),
    }] = entry.contents()
    else {
        panic!("one registered scalar distance interpretation");
    };
    assert_eq!(distances.len(), 2);
    let layout = &result.source_fidelity().record_layout()[entry.id()];
    for (ordinal, (value, expected)) in distances.iter().zip([1.5, 2.5]).enumerate() {
        assert_eq!(value.expect("readable distance slot").get(), expected);
        let offset = 4 + u64::try_from(ordinal).unwrap() * 4;
        let rows = result
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| {
                part.level().as_str() == "freecad.f32_record"
                    && part.offset() == offset
                    && part.byte_len() == 4
                    && layout.reads().contains_key(part.stream())
            })
            .collect::<Vec<_>>();
        let [row] = rows.as_slice() else {
            panic!("one original located distance slot");
        };
        assert_eq!(
            row.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
        );
        assert!(layout.reads()[row.stream()]
            .iter()
            .any(|read| read.start <= offset && offset + 4 <= read.end));
    }
}

#[test]
fn repeated_application_payloads_borrow_entry_identity_without_source_evidence() {
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
    };
    let properties = ["First", "Second"].map(|name| {
        let property = crate::native::PropertyRecord {
            dynamic: None,
            id: format!("fcstd:native:property#Owner:{name}"),
            owner: object.id().clone(),
            name: name.into(),
            type_name: "App::PropertyFileIncluded".into(),
            family: crate::native::PropertyFamily::File,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: Vec::new().into(),
                links: Vec::new(),
                side_entries: vec!["shared.bin".into()],
            }),
            order: 0,
        };
        property
    });
    let entry = crate::test_support::entry_fixture(
        "fcstd:native:entry#shared.bin".into(),
        "shared.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![7; 4096],
    );
    let identity = entry.id().as_ptr();
    let objects = [object];
    let entries = [entry.record];
    let records = super::wire_records(&objects, &properties, &entries, None).unwrap();
    for property in &records[0].property_records {
        assert_eq!(property.payloads[0].entry.as_ptr(), identity);
        assert_eq!(property.payloads[0].entry, entries[0].id());
        assert_eq!(property.payloads[0].name, entries[0].name());
        let wire = serde_json::to_value(&property.payloads[0]).unwrap();
        assert!(wire.get("sha256").is_none());
        assert!(wire.get("byte_len").is_none());
        assert!(wire.get("data").is_none());
    }
}

#[test]
fn external_component_census_matches_its_typed_native_records() {
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/external_component.FCStd"
    ));
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(input), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let mut expected = super::wire_records(&objects, &properties, &entries, None).unwrap();
    expected.sort_by(|left, right| left.id.cmp(&right.id));
    let actual = namespace.arenas().get("applications").unwrap();
    let expected_json = serde_json::to_value(&expected).unwrap();
    let actual_json = serde_json::to_value(actual).unwrap();
    assert_eq!(
        actual_json, expected_json,
        "application census must use only its current typed sources"
    );
    assert!(
        super::matches_native(
            &cadmpeg_test_support::service_decode_context(),
            namespace,
            &objects,
            &properties,
            &entries
        )
        .unwrap(),
        "equal typed census values must match their native serialization: {expected_json:#}"
    );
}
