// SPDX-License-Identifier: Apache-2.0

use crate::test_support::test_archive::archive_entries;
use crate::FcstdCodec;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::{Codec, DecodeOptions, DecodeResult};
use std::io::Cursor;

fn decode_pcurves(locations: &str, representations: &str) -> DecodeResult {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = format!(
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
Fa 0 0.001 1 0 1001000 +2 0 *
+1 0 *"
    );
    let bytes = archive_entries(&[("Document.xml", document), ("Shape.brp", brep.as_bytes())]);
    FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("pcurve representations decode")
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
