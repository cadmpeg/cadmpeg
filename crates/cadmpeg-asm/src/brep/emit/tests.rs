// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn face_sidedness_retains_the_decode_time_carrier_flip() {
    for (cosine, native, normalized) in [
        (1.0, Sense::Forward, Sense::Forward),
        (-1.0, Sense::Forward, Sense::Reversed),
        (1.0, Sense::Reversed, Sense::Reversed),
        (-1.0, Sense::Reversed, Sense::Forward),
    ] {
        let record = |index, name: &str, tokens: Vec<Token>| Record {
            index,
            name: name.into(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        };
        let records = [
            record(
                0,
                "face",
                vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Ref(-1),
                    Token::Ref(-1),
                    Token::Ref(2),
                    Token::Ref(-1),
                    Token::Ref(1),
                    if native == Sense::Forward {
                        Token::False
                    } else {
                        Token::True
                    },
                    Token::False,
                ],
            ),
            record(
                1,
                "cone",
                vec![
                    Token::Position([0.0; 3]),
                    Token::Vector3([0.0, 0.0, 1.0]),
                    Token::Vector3([1.0, 0.0, 0.0]),
                    Token::Double(1.0),
                    Token::False,
                    Token::False,
                    Token::Double(0.0),
                    Token::Double(cosine),
                    Token::Double(1.0),
                ],
            ),
        ];
        let by_index = records
            .iter()
            .map(|record| (record.index as i64, record))
            .collect();
        let (_, inward) = super::super::topology::decode_analytic_carriers(&records);
        let reach = Reachable {
            faces: HashSet::from([0]),
            ..Reachable::default()
        };
        let mut out = AsmBrep::default();
        emit_faces(
            &mut out,
            &records,
            &by_index,
            &reach,
            &inward,
            IdFormat("f3d"),
        );
        assert_eq!(out.faces.len(), 1);
        assert_eq!(out.face_sidedness.len(), 1);
        assert_eq!(out.faces[0].sense, normalized);
        assert_eq!(out.face_sidedness[0].native_sense, native);
        assert_eq!(out.face_sidedness[0].normalized_sense, normalized);
        out.faces[0].sense = match normalized {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        };
        assert_eq!(out.face_sidedness[0].normalized_sense, normalized);
    }
}

#[test]
fn tolerant_coedge_extension_retains_the_release_band() {
    for (major, suffix, expected) in [
        (214, vec![], TolerantCoedgeExtension::None),
        (
            215,
            vec![Token::Ref(-1)],
            TolerantCoedgeExtension::Reference { target: None },
        ),
        (
            219,
            vec![Token::Ref(-1)],
            TolerantCoedgeExtension::Reference { target: None },
        ),
        (
            220,
            vec![Token::Ref(-1), Token::Long(0), Token::Long(0)],
            TolerantCoedgeExtension::Empty { target: None },
        ),
    ] {
        let mut tokens = vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(0),
            Token::Ref(0),
            Token::Ref(-1),
            Token::Ref(1),
            Token::False,
            Token::Ref(2),
            Token::Long(0),
            Token::Ref(-1),
            Token::Double(0.0),
            Token::Double(1.0),
        ];
        tokens.extend(suffix);
        let records = [Record {
            index: 0,
            name: "tcoedge".into(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        }];
        let table = nurbs::toks::SubtypeTable::from_records(&records);
        let reach = Reachable {
            coedges: HashSet::from([0]),
            edges: HashSet::from([1]),
            loops: HashSet::from([2]),
            ..Reachable::default()
        };
        let mut out = AsmBrep::default();
        emit_coedges(
            &mut out,
            &records,
            &table,
            Some(major),
            &Carriers::default(),
            &reach,
            IdFormat("f3d"),
        );
        assert_eq!(out.tolerant_coedge_parameters.len(), 1);
        assert_eq!(out.tolerant_coedge_parameters[0].extension, expected);
    }
}

#[test]
fn tolerant_vertex_uses_the_third_double_for_evaluation_and_unset_state() {
    for width in [
        crate::kernel_header::RefWidth::Four,
        crate::kernel_header::RefWidth::Eight,
    ] {
        for (evaluated, tolerance, unset) in [(0.125f64, Some(1.25), false), (-1.0, None, true)] {
            let mut bytes = b"\x0d\x07tvertex".to_vec();
            for (tag, value) in [
                (0x0c, -1i64),
                (0x04, -1),
                (0x0c, -1),
                (0x0c, -1),
                (0x04, 0),
                (0x0c, 1),
            ] {
                bytes.push(tag);
                bytes.extend_from_slice(&value.to_le_bytes()[..width.bytes()]);
            }
            for value in [0.03f64, 0.07, evaluated] {
                bytes.push(0x06);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.push(0x11);
            let records = crate::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            assert_eq!(records[0].chunk(6), Some(&Token::Double(0.03)));
            assert_eq!(records[0].chunk(7), Some(&Token::Double(0.07)));
            let by_index = records
                .iter()
                .map(|record| (record.index as i64, record))
                .collect();
            let reach = Reachable {
                vertices: HashSet::from([0]),
                points: HashSet::from([1]),
                ..Reachable::default()
            };
            let mut out = AsmBrep::default();
            emit_vertices(&mut out, &records, &by_index, &reach, IdFormat("f3d"));
            assert_eq!(out.vertices.len(), 1);
            assert_eq!(out.vertices[0].tolerance, tolerance);
            assert_eq!(out.tolerant_vertex_tails.len(), 1);
            assert_eq!(
                out.tolerant_vertex_tails[0].leading_tolerances,
                [0.03, 0.07]
            );
            assert_eq!(out.tolerant_vertex_tails[0].evaluated_unset, unset);
        }
    }
}
