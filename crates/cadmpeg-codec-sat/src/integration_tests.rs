// SPDX-License-Identifier: Apache-2.0
use crate::loss::SatLossCode;
use crate::SatCodec;
use cadmpeg_ir::codec::{Codec, DecodeResult};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

const EPS_UNIT_CONVERSION: f64 = 1e-12;
fn decode_bytes(bytes: &[u8]) -> DecodeResult {
    SatCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap()
}
#[test]
fn concatenated_sat_tables_preserve_each_independent_sphere() {
    let sphere = |x, radius, terminator: &str| {
        format!(
        "104 5 0 0\nbody $-1 $1 $-1 $-1 #\nlump $-1 $-1 $2 $0 #\nshell $-1 $-1 $-1 $3 $1 #\nface $-1 $-1 $-1 $2 $-1 $4 0 #\nsphere-surface $-1 {x} 0 0 {radius} 1 0 0 0 0 1 0 #\n{terminator}"
    )
    };
    for terminator in ["", "End-of-ACIS-data\n"] {
        let source = format!(
            "{}result\n{}",
            sphere(0, 2, terminator),
            sphere(10, 3, "End-of-ACIS-data\n")
        );
        let decoded = decode_bytes(source.as_bytes());
        assert_eq!(decoded.ir().model.bodies.len(), 2);
        assert_eq!(decoded.ir().model.faces.len(), 2);
        let surfaces = &decoded.ir().model.surfaces;
        assert_eq!(surfaces.len(), 2);
        let radii: Vec<_> = surfaces
            .iter()
            .map(|surface| match surface.geometry.solved().unwrap() {
                SolvedSurfaceGeometry::Sphere(sphere) => sphere.radius().get(),
                _ => panic!("sphere carrier"),
            })
            .collect();
        assert_eq!(radii.len(), 2);
        for (actual, expected) in radii.iter().zip([2.0, 3.0]) {
            assert!((actual - expected).abs() < EPS_UNIT_CONVERSION);
        }
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == SatLossCode::SourceConcatenatedStreamsRecovered.kind()));
        let editable = cadmpeg_test_support::EditableDecodeResult::from(decoded);
        assert_eq!(
            editable
                .source_fidelity()
                .retained_record("sat:source:concatenated-streams#0")
                .unwrap()
                .data(),
            Some(source.as_bytes())
        );
        let report = cadmpeg_ir::validate_neutral(editable.ir(), Vec::new()).unwrap();
        assert!(report.is_ok(), "{report:?}");
    }
}

#[test]
fn concatenated_sat_streams_use_their_own_length_unit() {
    let sphere = |scale, radius| {
        format!("600 5 0 0\n1 T 4 ACIS 1 D\n{scale} 0.01 0.001\nbody $-1 $1 $-1 $-1 #\nlump $-1 $-1 $2 $0 #\nshell $-1 $-1 $-1 $3 $-1 $1 #\nface $-1 $-1 $-1 $2 $-1 $4 forward single #\nsphere-surface $-1 0 0 0 {radius} 1 0 0 0 0 1 forward_v I I I I #\nEnd-of-ACIS-data\n")
    };
    let decoded = decode_bytes(format!("{}{}", sphere(1, 2), sphere(10, 3)).as_bytes());
    assert_eq!(decoded.ir().model.bodies.len(), 2);
    let radii: Vec<_> = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .map(|surface| match surface.geometry.solved().unwrap() {
            SolvedSurfaceGeometry::Sphere(sphere) => sphere.radius().get(),
            _ => panic!("sphere carrier"),
        })
        .collect();
    assert_eq!(radii, [2.0, 30.0]);
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn concatenated_sat_different_save_format_retains_the_later_stream_unread() {
    let first = crate::test_support::test_streams::text_sphere_stream(1.0);
    let second = crate::test_support::test_streams::acis_text_sphere_stream(21_800);
    for terminator in [true, false] {
        let first = if terminator {
            first.clone()
        } else {
            String::from_utf8(first.clone())
                .unwrap()
                .replace("End-of-ASM-data\n", "")
                .into_bytes()
        };
        let mut source = first.clone();
        source.extend(&second);
        let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(&source));
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == SatLossCode::SourceStreamLayoutUnprojected.kind()));
        assert_eq!(
            result
                .source_fidelity()
                .retained_record("sat:source:unread#0")
                .unwrap()
                .data(),
            Some(second.as_slice())
        );
    }
}

#[test]
fn rejected_later_stream_retains_intervening_descriptive_lines() {
    for terminator in ["", "End-of-ACIS-data\n"] {
        let first = format!("104 5 0 0\nbody $-1 $1 $-1 $-1 #\nlump $-1 $-1 $2 $0 #\nshell $-1 $-1 $-1 $3 $1 #\nface $-1 $-1 $-1 $2 $-1 $4 0 #\nsphere-surface $-1 0 0 0 2 1 0 0 0 0 1 0 #\n{terminator}");
        for second in [
            crate::test_support::test_streams::acis_text_sphere_stream(21_800),
            b"9999999999 5 0 0\n".to_vec(),
        ] {
            let mut unread = b"result\nnext model\n".to_vec();
            unread.extend(second);
            let mut source = first.as_bytes().to_vec();
            source.extend(&unread);
            let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(&source));
            assert_eq!(result.ir().model.bodies.len(), 1);
            assert_eq!(result.ir().model.faces.len(), 1);
            assert!(
                result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code == SatLossCode::SourceStreamLayoutUnprojected.kind()),
                "terminator {terminator:?}, unread {unread:?}, losses {:?}",
                result.report().losses
            );
            assert_eq!(
                result
                    .source_fidelity()
                    .retained_record("sat:source:unread#0")
                    .unwrap()
                    .data(),
                Some(unread.as_slice())
            );
        }
    }
}

#[test]
fn concatenated_sat_unreadable_later_header_keeps_the_first_model() {
    let first = crate::test_support::test_streams::text_sphere_stream(1.0);
    for second in [b"23200 5 0 0\n".as_slice(), b"9999999999 5 0 0\n"] {
        let mut source = first.clone();
        source.extend(second);
        let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(&source));
        assert_eq!(result.ir().model.bodies.len(), 1);
        assert_eq!(result.ir().model.faces.len(), 1);
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == SatLossCode::SourceStreamLayoutUnprojected.kind()));
        assert_eq!(
            result
                .source_fidelity()
                .retained_record("sat:source:unread#0")
                .unwrap()
                .data(),
            Some(second)
        );
    }
}

#[test]
fn standalone_face_children_are_retained_without_entering_the_model() {
    let source = b"600 0 1 0\n1 T 4 ACIS 1 D\n1 0.01 0.001\n\
face $-1 $-1 $1 $-1 $-1 $2 forward single #\n\
loop $-1 $-1 $3 $0 #\n\
plane-surface $-1 0 0 0 0 0 1 1 0 0 forward_v I I I I #\n\
coedge $-1 $3 $3 $-1 $4 forward $1 $-1 #\n\
edge $-1 $5 0 $6 1 $3 $7 forward 0 #\n\
vertex $-1 $4 $8 #\nvertex $-1 $4 $9 #\n\
straight-curve $-1 0 0 0 1 0 0 I I #\n\
point $-1 0 0 0 #\npoint $-1 1 0 0 #\n\
body $-1 $11 $-1 $-1 #\n\
lump $-1 $-1 $12 $10 #\n\
shell $-1 $-1 $-1 $13 $-1 $11 #\n\
face $-1 $-1 $-1 $12 $-1 $14 forward single #\n\
sphere-surface $-1 0 0 0 2 1 0 0 0 0 1 forward_v I I I I #\n\
End-of-ACIS-data\n";
    let result = cadmpeg_test_support::EditableDecodeResult::from(decode_bytes(source));
    let model = &result.ir().model;
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.faces.len(), 1);
    assert_eq!(model.surfaces.len(), 1);
    assert_eq!(model.faces[0].id.as_str(), "sat:brep:entity#13");
    assert!(model.loops.is_empty());
    assert!(model.coedges.is_empty());
    assert!(model.edges.is_empty());
    assert!(model.vertices.is_empty());
    assert!(model.points.is_empty());
    assert!(model.curves.is_empty());
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .unwrap()
        .is_ok());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == SatLossCode::TopologyFaceOwnerUnprojected.kind()));
    assert_eq!(
        result
            .source_fidelity()
            .retained_record("sat:source:standalone-faces#0")
            .unwrap()
            .data(),
        Some(source.as_slice())
    );
}
