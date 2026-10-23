// SPDX-License-Identifier: Apache-2.0

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::{Codec, DecodeOptions, DecodeResult};
use std::fmt::Write;
use std::io::Cursor;

fn pcurve_brep(
    locations: &str,
    representations: &str,
    face_support: usize,
    wire_location: usize,
) -> String {
    format!(
        "CASCADE Topology V1, (c) Matra-Datavision
{locations}
Curve2ds 3
1 0 0 1 0
1 3 0 1 0
1 7 0 1 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 2
1 0 0 0 0 0 1 1 0 0 0 1 0
1 0 0 1 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 5
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 {representations} 0 1001000 +5 0 -4 0 *
Wi 1001000 +3 0 -3 0 *
Fa 0 0.001 1 {face_support} 1001000 +2 {wire_location} *
+1 0 *"
    )
}

fn decode_shapes(shapes: &[(&str, String)]) -> DecodeResult {
    let mut objects = String::new();
    let mut object_data = String::new();
    for (index, (name, _)) in shapes.iter().enumerate() {
        write!(
            objects,
            "<Object type=\"Part::Feature\" name=\"{name}\" id=\"{}\"/>",
            index + 1
        )
        .expect("fixture object declaration");
        write!(object_data,"<Object name=\"{name}\"><Properties Count=\"1\"><Property name=\"Shape\" type=\"Part::PropertyPartShape\"><Part file=\"{name}.brp\"/></Property></Properties></Object>").expect("fixture object properties");
    }
    let document = format!("<Document SchemaVersion=\"4\" FileVersion=\"1\"><Objects Count=\"{count}\">{objects}</Objects><ObjectData Count=\"{count}\">{object_data}</ObjectData></Document>", count=shapes.len());
    let names: Vec<_> = shapes
        .iter()
        .map(|(name, _)| format!("{name}.brp"))
        .collect();
    let mut entries = vec![("Document.xml", document.as_bytes())];
    entries.extend(
        names
            .iter()
            .zip(shapes)
            .map(|(name, (_, brep))| (name.as_str(), brep.as_bytes())),
    );
    let bytes = archive_entries(&entries);
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("pcurve representations decode")
}

fn decode_pcurves(locations: &str, representations: &str) -> DecodeResult {
    decode_shapes(&[("Shape", pcurve_brep(locations, representations, 0, 0))])
}

fn selected_origins(result: &DecodeResult) -> Vec<f64> {
    result
        .ir()
        .model
        .coedges
        .iter()
        .map(|coedge| {
            let reference = &coedge.pcurves[0];
            let pcurve = result
                .ir()
                .model
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == reference.pcurve)
                .expect("selected pcurve exists");
            let PcurveGeometry::Line(line) = &pcurve.geometry else {
                panic!("line pcurve expected");
            };
            line.origin().as_raw().u
        })
        .collect()
}

#[test]
fn later_matching_pcurve_replaces_curve_and_range() {
    for (representations, origin, range) in [
        ("2 1 1 0 0 1 2 2 1 0 2 5", 3.0, [2.0, 5.0]),
        ("2 2 1 0 2 5 2 1 1 0 0 1", 0.0, [0.0, 1.0]),
        ("2 1 1 0 0 1 2 2 1 0 2 5 2 3 2 0 0 1", 3.0, [2.0, 5.0]),
    ] {
        let result = decode_pcurves("Locations 0", representations);
        assert_eq!(selected_origins(&result), [origin, origin]);
        for coedge in &result.ir().model.coedges {
            assert_eq!(
                coedge.pcurves[0].parameter_range.unwrap().endpoints(),
                range,
            );
        }
    }
}

#[test]
fn replacement_between_single_and_paired_pcurves_keeps_orientation() {
    let single_then_pair = decode_pcurves("Locations 0", "2 1 1 0 0 1 3 2 3 C0 1 0 0 1");
    assert_eq!(selected_origins(&single_then_pair), [3.0, 7.0]);
    let pair_then_single = decode_pcurves("Locations 0", "3 2 3 C0 1 0 0 1 2 1 1 0 0 1");
    assert_eq!(selected_origins(&pair_then_single), [0.0, 0.0]);
    let pair_then_pair = decode_pcurves("Locations 0", "3 1 2 C0 1 0 0 1 3 2 3 C0 1 0 0 1");
    assert_eq!(selected_origins(&pair_then_pair), [3.0, 7.0]);
}

#[test]
fn distinct_location_references_with_equal_transforms_report_ambiguity() {
    for locations in [
        "Locations 2\n1\n1 0 0 0\n0 1 0 0\n0 0 1 0\n1\n1 0 0 0\n0 1 0 0\n0 0 1 0",
        "Locations 2\n1\n1 0 0 0\n0 1 0 0\n0 0 1 0\n2\n1 1 0",
    ] {
        let result = decode_pcurves(locations, "2 1 1 1 0 1 2 2 1 2 0 1");
        assert_eq!(result.ir().model.edges.len(), 1);
        assert!(result
            .ir()
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        assert!(result.ir().model.pcurves.is_empty());
        assert!(result.report().losses.iter().any(|loss| {
            loss.code.local_code() == "pcurve.location-ambiguous"
                && loss.severity == Severity::Blocking
        }));
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).unwrap();
        assert!(
            validation.findings.iter().all(|finding| {
                finding.severity != Severity::Error
                    || finding.check == cadmpeg_ir::report::check::Check::Identity
            }),
            "{:?}",
            validation.findings
        );
    }
}

#[test]
fn canonical_location_aliases_select_last_exact_chain_and_ignore_equal_matrices() {
    let locations =
        "Locations 3\n1\n1 0 0 7\n0 1 0 0\n0 0 1 0\n1\n1 0 0 7\n0 1 0 0\n0 0 1 0\n2\n1 1 0";
    for representations in [
        "2 1 1 1 0 1 2 2 1 3 2 5 2 3 1 2 0 1",
        "2 3 1 2 0 1 2 1 1 1 0 1 2 2 1 3 2 5",
    ] {
        let result = decode_shapes(&[("Shape", pcurve_brep(locations, representations, 1, 0))]);
        assert_eq!(selected_origins(&result), [3.0, 3.0]);
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "pcurve.location-ambiguous"));
        assert!(result
            .ir()
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves[0].parameter_range.unwrap().endpoints() == [2.0, 5.0]));
    }
}

#[test]
fn edge_placement_and_inverse_representation_cancel_before_pcurve_matching() {
    let locations =
        "Locations 3\n1\n1 0 0 7\n0 1 0 0\n0 0 1 0\n1\n1 0 0 -7\n0 1 0 0\n0 0 1 0\n2\n1 -1 0";
    let result = decode_shapes(&[(
        "Shape",
        pcurve_brep(locations, "2 2 1 3 0 1 2 3 1 2 0 1", 0, 1),
    )]);
    assert_eq!(selected_origins(&result), [3.0, 3.0]);
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "pcurve.location-ambiguous"));
}

#[test]
fn distinct_equal_matrix_datums_do_not_merge_topology_but_aliases_do() {
    let locations =
        "Locations 3\n1\n1 0 0 7\n0 1 0 0\n0 0 1 0\n1\n1 0 0 7\n0 1 0 0\n0 0 1 0\n2\n1 1 0";
    for (second_location, expected_edges) in [(2, 2), (3, 1)] {
        let brep = format!(
            "CASCADE Topology V1, (c) Matra-Datavision
{locations}
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 4
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +4 0 -3 0 *
Co 1001000 +2 1 +2 {second_location} *
+1 0 *"
        );
        let result = decode_shapes(&[("Shape", brep)]);
        assert_eq!(result.ir().model.edges.len(), expected_edges);
        assert_eq!(result.ir().model.vertices.len(), 2 * expected_edges);
        assert_eq!(result.ir().model.points.len(), 2 * expected_edges);
    }
}

#[test]
fn bounded_location_expansion_withholds_only_its_payload_before_topology_mutation() {
    let locations = "Locations 4\n1\n1 0 0 0\n0 1 0 0\n0 0 1 0\n1\n1 0 0 0\n0 1 0 0\n0 0 1 0\n2\n1 1 2 1 0\n2\n3 32769 0";
    let result = decode_shapes(&[
        ("Bounded", pcurve_brep(locations, "2 1 1 0 0 1", 0, 0)),
        ("Complete", pcurve_brep("Locations 0", "2 1 1 0 0 1", 0, 0)),
    ]);
    let payloads = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    assert_eq!(payloads.len(), 2);
    let bounded = payloads
        .iter()
        .find(|payload| payload.id.contains("Bounded"))
        .unwrap();
    let tables = crate::brep::Tables::from_payload(bounded).unwrap();
    assert_eq!(tables.locations.len(), 4);
    assert_eq!(tables.tshapes.len(), 5);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert!(result
        .ir()
        .model
        .faces
        .iter()
        .all(|face| face.id.as_str().contains("Complete")));
    assert!(result
        .report()
        .losses
        .iter()
        .any(
            |loss| loss.code.local_code() == "topology.location-identity-bounded"
                && loss.severity == Severity::Blocking
        ));
    assert!(result
        .ir()
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
}
