use std::io::{Cursor, Write};

use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

#[test]
fn transfers_recursive_exact_parameter_curve_geometry() {
    let source = crate::brep::TextCurve2d::Offset {
        distance: 0.25,
        basis: Box::new(crate::brep::TextCurve2d::Trimmed {
            parameter_range: [0.0, std::f64::consts::PI],
            basis: Box::new(crate::brep::TextCurve2d::Circle {
                center: cadmpeg_ir::math::Point2::new(1.0, 2.0),
                x_axis: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                y_axis: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                radius: 3.0,
            }),
        }),
    };
    let cadmpeg_ir::geometry::PcurveGeometry::Offset { distance, basis } =
        crate::text_pcurve_geometry(&source)
    else {
        panic!("expected offset pcurve");
    };
    assert_eq!(distance, 0.25);
    assert!(matches!(
        basis.as_ref(),
        cadmpeg_ir::geometry::PcurveGeometry::Trimmed { basis, .. }
            if matches!(basis.as_ref(), cadmpeg_ir::geometry::PcurveGeometry::Circle { radius: 3.0, .. })
    ));
}

#[test]
fn transfers_binary_exact_curve_and_surface_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.bin"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let mut brep = b"\nOpen CASCADE Topology V3 (c)\nLocations 0\nCurve2ds 0\nCurves 1\n".to_vec();
    brep.push(1);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n");
    brep.push(1);
    for value in [
        0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Triangulations 1\n");
    brep.extend_from_slice(&3_i32.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.push(0);
    for value in [0.01_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    for node in [1_i32, 2, 3] {
        brep.extend_from_slice(&node.to_le_bytes());
    }
    brep.extend_from_slice(b"TShapes 7\n");
    let flags = |brep: &mut Vec<u8>| brep.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0]);
    let child = |brep: &mut Vec<u8>, orientation: u8, reverse_index: i32| {
        brep.push(orientation);
        brep.extend_from_slice(&reverse_index.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
    };
    brep.push(7);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.push(0);
    flags(&mut brep);
    brep.push(b'*');
    brep.push(6);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&[1, 1, 1, 0]);
    flags(&mut brep);
    child(&mut brep, 0, 7);
    child(&mut brep, 1, 7);
    brep.push(b'*');
    brep.push(5);
    flags(&mut brep);
    child(&mut brep, 0, 6);
    brep.push(b'*');
    brep.push(4);
    brep.push(0);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.push(0);
    flags(&mut brep);
    child(&mut brep, 0, 5);
    brep.push(b'*');
    for (kind, reverse_index) in [(3_u8, 4_i32), (2, 3), (0, 2)] {
        brep.push(kind);
        flags(&mut brep);
        child(&mut brep, 0, reverse_index);
        brep.push(b'*');
    }
    brep.extend_from_slice(&7_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.bin", &brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("binary curve carrier");
    assert_eq!(result.ir.model.curves.len(), 1);
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { .. }
    ));
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].triangles, [[0, 1, 2]]);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(result.report.geometry_transferred);
}

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

fn streaming_archive(document: &str) -> Vec<u8> {
    streaming_archive_with_options(document, SimpleFileOptions::default())
}

fn streaming_archive_with_options(document: &str, options: SimpleFileOptions) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_stream(Vec::new());
    zip.start_file(
        "Document.xml",
        options.compression_method(zip::CompressionMethod::Deflated),
    )
    .expect("start streamed entry");
    zip.write_all(document.as_bytes()).expect("write entry");
    zip.finish().expect("finish ZIP").into_inner()
}

#[test]
fn frames_zip64_streaming_descriptor_and_local_extra() {
    let bytes = streaming_archive_with_options(
        "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>",
        SimpleFileOptions::default().large_file(true),
    );
    let scan = crate::container::scan(&mut Cursor::new(bytes)).expect("ZIP64 streaming ZIP");
    assert!(scan
        .ledger
        .iter()
        .any(|span| span.role == "local-extra" && span.end > span.start));
    let descriptor = scan
        .ledger
        .iter()
        .find(|span| span.role == "data-descriptor")
        .expect("ZIP64 descriptor");
    assert_eq!(descriptor.end - descriptor.start, 24);
}

#[test]
fn frames_streaming_data_descriptor_separately_from_padding() {
    let bytes = streaming_archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>");
    let scan = crate::container::scan(&mut Cursor::new(bytes)).expect("streaming ZIP");
    let descriptors = scan
        .ledger
        .iter()
        .filter(|span| span.role == "data-descriptor")
        .collect::<Vec<_>>();
    assert_eq!(descriptors.len(), 1);
    assert!(matches!(descriptors[0].end - descriptors[0].start, 16 | 24));
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
fn transfers_connected_text_brep_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("connected topology");
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 2);
    assert_eq!(result.ir.model.edges.len(), 2);
    assert_eq!(result.ir.model.vertices.len(), 2);
    assert_eq!(result.ir.model.pcurves.len(), 2);
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurve.is_some()));
    let report = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity),
        "{:#?}",
        report.findings
    );
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
            b"\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 4\n1 10 20 30 1 0 0\n7 0 0 2 3 2 0 0 0 5 0 0 10 0 0 0 3 1 3\n8 0 5 1 0 0 0 1 0 0\n9 2 0 0 1 1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 5\n1 0 0 0 0 0 1 1 0 0 0 1 0\n9 0 0 0 0 1 1 2 2 2 2 0 0 0 0 1 0 1 0 0 1 1 0 0 2 1 2 0 2 1 2\n6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 1\n3 1 0 0.01 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*",
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
    assert_eq!(result.ir.model.curves.len(), 8);
    match &result.ir.model.curves[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } => {
            assert_eq!([origin.x, origin.y, origin.z], [10.0, 20.0, 30.0]);
            assert_eq!([direction.x, direction.y, direction.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected curve {other:?}"),
    }
    match &result.ir.model.curves[1].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) => {
            assert_eq!(nurbs.degree, 2);
            assert_eq!(nurbs.control_points.len(), 3);
            assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            assert!(nurbs.weights.is_none());
        }
        other => panic!("unexpected curve {other:?}"),
    }
    assert_eq!(result.ir.model.procedural_curves.len(), 2);
    match &result.ir.model.procedural_curves[0].definition {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } => assert_eq!(*parameter_range, [0.0, 5.0]),
        other => panic!("unexpected trimmed construction {other:?}"),
    }
    match &result.ir.model.procedural_curves[1].definition {
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
    assert_eq!(result.ir.model.surfaces.len(), 7);
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
    assert_eq!(result.ir.model.procedural_surfaces.len(), 4);
    assert!(matches!(
        result.ir.model.procedural_surfaces[0].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { .. }
    ));
    assert!(matches!(
        result.ir.model.procedural_surfaces[1].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            parameter_interval: None,
            ..
        }
    ));
    assert!(matches!(
        result.ir.model.procedural_surfaces[2].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
            u_sense: None,
            v_sense: None,
            ..
        }
    ));
    assert!(matches!(
        result.ir.model.procedural_surfaces[3].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset { .. }
    ));
    match &result.ir.model.surfaces[1].geometry {
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
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].vertices.len(), 3);
    assert_eq!(result.ir.model.tessellations[0].triangles, [[0, 1, 2]]);
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
