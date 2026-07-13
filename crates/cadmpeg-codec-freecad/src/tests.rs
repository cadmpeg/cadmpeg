use std::io::{Cursor, Write};

use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

fn archive(document: &str) -> Vec<u8> {
    archive_entries(&[("Document.xml", document.as_bytes())])
}

fn archive_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut bytes);
    for (name, data) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .expect("start entry");
        zip.write_all(data).expect("write entry");
    }
    zip.finish().expect("finish ZIP");
    bytes.into_inner()
}

#[test]
fn rejects_unsafe_names() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let unsafe_name = archive_entries(&[("../Document.xml", xml), ("Document.xml", xml)]);
    let error = FcstdCodec
        .inspect(&mut Cursor::new(unsafe_name))
        .expect_err("unsafe path must fail");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn legacy_layout_is_inspectable_but_explicitly_refused_for_decode() {
    let bytes = archive("<Document SchemaVersion=\"3\" FileVersion=\"1\"/>");
    let summary = FcstdCodec
        .inspect(&mut Cursor::new(&bytes))
        .expect("legacy inspection");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=3"));
    let error = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect_err("legacy decode must fail");
    assert!(error
        .to_string()
        .contains("FCStd SchemaVersion=3 FileVersion=1 persistence layout"));
}

#[test]
fn thumbnail_bytes_are_retained_with_digest() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let bytes = archive_entries(&[("Document.xml", xml), ("thumbnails/Thumbnail.png", b"png")]);
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        )
        .expect("decode");
    let unknowns = result.ir.native_unknowns("fcstd").expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].data.as_deref(), Some(b"png".as_slice()));
}

#[test]
fn recovers_objects_dynamic_properties_links_and_side_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Demo"/></Property></Properties>
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="Body" Count="1"><Dep Name="Sketch"/></ObjectDeps>
<ObjectDeps Name="Sketch" Count="0"/>
<Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
<Object type="PartDesign::Feature" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
<Object name="Body" Extensions="True"><Extensions Count="1"><Extension type="Demo::Extension" name="Demo"><Properties Count="1"><Property name="ExtensionValue" type="App::PropertyString"><String value="kept"/></Property></Properties></Extension></Extensions><Properties Count="4" TransientCount="1">
<_Property name="TransientState" type="App::PropertyInteger" status="8"/>
<Property name="Support" type="App::PropertyLinkSub" status="4" group="Attachment" doc="Support object" attr="2" ro="1" hide="0"><Link object="Sketch" sub="Face1"/></Property>
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
            b"\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 1\n1 10 20 30 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 0\n*",
        ),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode graph");
    let namespace = result.ir.native.namespace("fcstd").expect("namespace");
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
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].owner, "fcstd:object:Body");
    let extension_value = properties
        .iter()
        .find(|property| property.name == "ExtensionValue")
        .expect("extension property");
    assert_eq!(extension_value.owner, extensions[0].id);
    assert_eq!(objects[0].dependencies, vec!["fcstd:object:Sketch"]);
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.owner, "fcstd:object:Body");
    assert_eq!(
        support.links[0].object.as_deref(),
        Some("fcstd:object:Sketch")
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
        Some("fcstd:object:Sketch")
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
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.curves.len(), 1);
    match &result.ir.model.curves[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } => {
            assert_eq!([origin.x, origin.y, origin.z], [10.0, 20.0, 30.0]);
            assert_eq!([direction.x, direction.y, direction.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected curve {other:?}"),
    }
    assert_eq!(result.ir.model.surfaces.len(), 1);
    match &result.ir.model.surfaces[0].geometry {
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
    let findings = crate::validate_native(&result.ir);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn detects_marker_but_not_arbitrary_zip() {
    assert_eq!(
        FcstdCodec.detect(&archive(
            "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>"
        )),
        Confidence::High
    );
    assert_eq!(FcstdCodec.detect(b"PK\x03\x04 unrelated"), Confidence::Low);
    assert_eq!(FcstdCodec.detect(b"not zip"), Confidence::No);
}

#[test]
fn inspects_and_closes_physical_ledger() {
    let bytes = archive("<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>");
    let archive_len = bytes.len() as u64;
    let summary = FcstdCodec
        .inspect(&mut Cursor::new(&bytes))
        .expect("inspect");
    assert_eq!(summary.format, "fcstd");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=4"));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        )
        .expect("decode");
    assert!(result.report.losses.is_empty());
    let ledger = result
        .ir
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .expect("ledger");
    assert_eq!(ledger.first().map(|span| span.start), Some(0));
    assert_eq!(ledger.last().map(|span| span.end), Some(archive_len));
    assert!(ledger.windows(2).all(|pair| pair[0].end == pair[1].start));
    for role in [
        "local-signature",
        "local-fields",
        "local-name",
        "compressed-payload",
        "central-signature",
        "central-fields",
        "central-name",
        "end-record",
    ] {
        assert!(
            ledger.iter().any(|span| span.role == role),
            "missing {role}"
        );
    }
}
