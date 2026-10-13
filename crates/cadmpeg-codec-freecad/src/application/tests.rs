// SPDX-License-Identifier: Apache-2.0
//! Application-domain census unit tests.

use crate::test_support::test_archive::{archive_entries, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fmt::Write as _;
use std::io::Cursor;

#[test]
fn application_census_without_objects_skips_source_indexes() {
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#Data.bin".into(),
        "Data.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![1; 4096],
    );
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Owner:Data".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: "Data".into(),
        type_name: "App::PropertyFileIncluded".into(),
        family: crate::native::PropertyFamily::File,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["Data.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    assert!(super::wire_records(
        &ctx,
        &[],
        std::slice::from_ref(&property),
        std::slice::from_ref(&entry),
    )
    .expect("no application consumer")
    .is_empty());
}

#[test]
fn empty_application_comparison_skips_exhausted_actual_source() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let namespace = cadmpeg_ir::native::NativeNamespace::default();
    assert!(super::matches_native(&ctx, &namespace, &[], &[], &[]).expect("empty census"));
}

#[test]
fn missing_application_arena_skips_unexecuted_expected_and_actual_visits() {
    let objects = [crate::native::ObjectRecord {
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
    }];
    let complete_work = crate::test_support::with_service_context(&[], |ctx| {
        let mut records = super::wire_records(ctx, &objects, &[], &[]).expect("census prefix");
        ctx.stable_sort_by(
            &mut records,
            |record| &record.id,
            Ord::cmp,
            "FreeCAD application records sort",
        )
        .expect("census order");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
            .charge_work(u64::MAX, "completed application comparison prefix")
            .expect_err("work overflow")
        else {
            panic!("resource refusal")
        };
        limit.used
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = complete_work;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    assert!(!super::matches_native(
        &ctx,
        &cadmpeg_ir::native::NativeNamespace::default(),
        &objects,
        &[],
        &[]
    )
    .expect("known missing actual arena"));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn application_census_without_side_entries_skips_entry_index() {
    let mut object = crate::native::ObjectRecord {
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
    let entry = crate::test_support::entry_record(
        "fcstd:native:entry#Data.bin".into(),
        "Data.bin".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![1; 4096],
    );
    let complete_work = crate::test_support::with_service_context(&[], |ctx| {
        super::wire_records(ctx, std::slice::from_ref(&object), &[], &[])
            .expect("short application census");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
            .charge_work(u64::MAX, "completed application census work")
            .expect_err("work overflow")
        else {
            panic!("work refusal")
        };
        limit.used
    });
    object.type_name = format!("Vendor::{}", "UnvisitedFeature".repeat(4096));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = complete_work;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let records = super::wire_records(
        &ctx,
        std::slice::from_ref(&object),
        &[],
        std::slice::from_ref(&entry),
    )
    .expect("entry index has no consumer");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].object, object.id());
    assert_eq!(records[0].domain, "Vendor");
    assert!(records[0].side_entries.is_empty());
    assert!(records[0].property_records.is_empty());
}

#[test]
fn application_inert_classification_stops_at_first_fixed_marker() {
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Owner:Data".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: "Data".into(),
        type_name: format!("PropertyPythonObject{}", "Unvisited".repeat(4096)),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    assert!(super::is_inert(&ctx, &property).expect("first marker"));
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn application_records_refuse_on_collection_limit() {
    let objects = [crate::native::ObjectRecord {
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
    }];
    crate::test_support::assert_collection_refusal_at(&[], "FreeCAD application records", |ctx| {
        super::wire_records(ctx, &objects, &[], &[]).map(|_| ())
    });
}

#[test]
fn application_identity_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Owner".into(),
            "Owner".into(),
        )
        .expect("object identity"),
        type_name: "Vendor::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD native identity", |ctx| {
        super::wire_records(ctx, std::slice::from_ref(&object), &[], &[]).map(|_| ())
    });
}

#[test]
fn application_property_identity_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            "fcstd:native:object#Owner".into(),
            "Owner".into(),
        )
        .expect("object identity"),
        type_name: "Vendor::Feature".into(),
        persistent_id: None,
        view_type: None,
        attributes: std::collections::BTreeMap::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Owner:Value".into(),
        owner: object.id().clone(),
        name: "Value".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD native child identity", |ctx| {
        super::wire_records(
            ctx,
            std::slice::from_ref(&object),
            std::slice::from_ref(&property),
            &[],
        )
        .map(|_| ())
    });
}

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
    let bytes = |record: &serde_json::Value| {
        serde_json::from_value::<Vec<u8>>(record["data"].clone()).expect("wire bytes")
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
    assert!(report["byte_start"].as_u64().unwrap() < report["byte_end"].as_u64().unwrap());
    assert_eq!(
        report["byte_len"],
        cadmpeg_core::decode::u64_from_index(bytes(report).len())
    );
    assert_eq!(
        report["sha256"],
        cadmpeg_ir::hash::sha256_hex(&bytes(report))
    );
    assert_eq!(report["payloads"].as_array().unwrap().len(), 1);
    let payload = &report["payloads"][0];
    assert_eq!(payload["name"], "analysis.dat");
    assert_eq!(bytes(payload), b"finite-element-results");
    assert_eq!(
        payload["sha256"],
        cadmpeg_ir::hash::sha256_hex(&bytes(payload))
    );
    let python = &by_domain["Path"]["property_records"][0];
    assert_eq!(python["inert"], true);
    assert!(String::from_utf8_lossy(&bytes(python)).contains("serialized-but-inert"));
    assert!(records.iter().all(|record| {
        record["byte_start"].as_u64().unwrap() < record["byte_end"].as_u64().unwrap()
            && record["byte_len"] == cadmpeg_core::decode::u64_from_index(bytes(record).len())
            && record["sha256"] == cadmpeg_ir::hash::sha256_hex(&bytes(record))
    }));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut altered = records;
    let payload = &mut altered
        .iter_mut()
        .find(|record| record["domain"] == "Fem")
        .unwrap()["property_records"][0]["payloads"][0];
    let replacement = b"different internally consistent bytes";
    payload["data"] = serde_json::json!(replacement.as_slice());
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
fn absent_object_data_keeps_the_legacy_empty_wire_without_a_domain_sentinel() {
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
    assert_eq!(records[0]["data"], serde_json::json!([]));
    assert_eq!(records[0]["byte_start"], 0);
    assert_eq!(records[0]["byte_end"], 0);
    assert_eq!(records[0]["byte_len"], 0);
    assert_eq!(records[0]["sha256"], cadmpeg_ir::hash::sha256_hex(&[]));
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
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
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
    assert_eq!(entry.data(), payload);
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
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    for (name, _, _, payload) in cases {
        let entry = entries
            .iter()
            .find(|entry| entry.name() == name)
            .expect("side entry");
        assert_eq!(entry.data(), payload);
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
fn application_hashes_refuse_work_before_digest_allocation() {
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
        data: Some(crate::native::RetainedXml::from_text("<Object/>".into(), 0).expect("XML")),
    };
    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &[],
        "FreeCAD application object digest",
        |ctx| super::wire_records(ctx, std::slice::from_ref(&object), &[], &[]).map(|_| ()),
    );
}

#[test]
fn application_property_hash_refuses_work_and_digest_storage() {
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
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Owner:Value".into(),
        owner: object.id().clone(),
        name: "Value".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML"),
    };
    crate::test_support::refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        &[],
        "FreeCAD application property digest",
        |ctx| {
            super::wire_records(
                ctx,
                std::slice::from_ref(&object),
                std::slice::from_ref(&property),
                &[],
            )
            .map(|_| ())
        },
    );
    crate::test_support::assert_retained_refusal_at(
        &[],
        "FreeCAD application property digest",
        |ctx| {
            super::wire_records(
                ctx,
                std::slice::from_ref(&object),
                std::slice::from_ref(&property),
                &[],
            )
            .map(|_| ())
        },
    );
    crate::test_support::assert_retained_refusal_at(
        &[],
        "FreeCAD application object digest",
        |ctx| super::wire_records(ctx, std::slice::from_ref(&object), &[], &[]).map(|_| ()),
    );
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
    let digest = entry.sha256().as_ptr();
    let objects = [object];
    let entries = [entry];
    let small_entries = [crate::test_support::entry_record(
        entries[0].id().into(),
        entries[0].name().into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        vec![7],
    )];
    let complete_work = crate::test_support::with_service_context(&[], |ctx| {
        super::wire_records(ctx, &objects, &properties, &small_entries)
            .expect("small application payloads");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
            .charge_work(u64::MAX, "completed application payload work")
            .expect_err("work overflow")
        else {
            panic!("work refusal")
        };
        limit.used
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = complete_work;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let records = super::wire_records(&ctx, &objects, &properties, &entries)
        .expect("only property XML is hashed");
    for property in &records[0].property_records {
        assert_eq!(property.payloads[0].sha256.as_ptr(), digest);
        assert_eq!(property.payloads[0].sha256, entries[0].sha256());
    }
}

#[test]
fn application_comparison_refuses_actual_arena_visits() {
    let objects = [crate::native::ObjectRecord {
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
    }];
    let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
    crate::test_support::with_service_context(&[], |ctx| {
        super::install(ctx, &mut namespace, &objects, &[], &[]).expect("application arena");
    });
    for (operation, expected) in [
        ("FreeCAD actual application record", objects.as_slice()),
        ("FreeCAD actual application tail", &[][..]),
    ] {
        crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            operation,
            |ctx| {
                super::matches_native(ctx, &namespace, expected, &[], &[])
                    .map_err(cadmpeg_core::CodecError::from)
            },
        );
    }
}

#[test]
fn application_releases_consumed_owner_buffer_before_later_entry_index() {
    let objects = ["First", "Second"].map(|name| crate::native::ObjectRecord {
        identity: crate::native::object_identity::ObjectIdentity::try_new(
            format!("fcstd:native:object#{name}"),
            name.into(),
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
    });
    let entries: Vec<_> = (0..1024)
        .map(|index| {
            crate::test_support::entry_record(
                format!("fcstd:native:entry#Payload{index}.bin"),
                format!("Payload{index}.bin"),
                cadmpeg_core::container::ContainerRole::Auxiliary,
                Vec::new(),
                Vec::new(),
            )
        })
        .collect();
    let mut properties: Vec<_> = (0..1024)
        .map(|index| crate::native::PropertyRecord {
            id: format!("fcstd:native:property#First:Value{index}"),
            owner: objects[0].id().clone(),
            name: format!("Value{index}"),
            type_name: "App::PropertyString".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Transient,
            order: index,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
        })
        .collect();
    properties.push(crate::native::PropertyRecord {
        id: "fcstd:native:property#Second:Data".into(),
        owner: objects[1].id().clone(),
        name: "Data".into(),
        type_name: "App::PropertyFileIncluded".into(),
        family: crate::native::PropertyFamily::File,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec![entries[0].name().into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML span"),
    });
    // Core operations are the storage oracle for the live owner and entry
    // trees. The second owner still holds the four-reference minimum vector;
    // the first owner's 1024-reference buffer has already been consumed.
    let tree_bytes = crate::test_support::with_service_context(&[], |ctx| {
        let _owners = ctx
            .collect_scoped_btree_map(
                objects
                    .iter()
                    .map(|object| (object.id().as_str(), None::<super::OwnerProperties<'_, '_>>)),
                "application lifetime owner-tree oracle",
            )
            .expect("owner tree");
        let _entries = ctx
            .collect_scoped_btree_map(
                entries.iter().map(|entry| (entry.name(), entry)),
                "application lifetime entry-tree oracle",
            )
            .expect("entry tree");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
            .reserve_scoped(u64::MAX, "measure live application trees")
            .expect_err("materialized overflow")
        else {
            panic!("materialized refusal")
        };
        limit.used
    });
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = tree_bytes
        + cadmpeg_core::decode::u64_from_index(
            4 * std::mem::size_of::<&crate::native::PropertyRecord>(),
        );
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("context");
    let records = super::wire_records(&ctx, &objects, &properties, &entries)
        .expect("consumed first-owner storage is released");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].object, objects[0].id());
    assert_eq!(
        records[0].properties,
        properties[..1024]
            .iter()
            .map(|property| property.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(records[1].object, objects[1].id());
    assert_eq!(records[1].property_records.len(), 1);
    assert_eq!(records[1].property_records[0].payloads.len(), 1);
    assert_eq!(
        records[1].property_records[0].payloads[0].entry,
        entries[0].id()
    );
    assert_eq!(ctx.resource_refusal(), None);
}
