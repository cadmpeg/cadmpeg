// SPDX-License-Identifier: Apache-2.0

use crate::brep::emit::emit_carrier_surface;
use crate::brep::AsmBrep;
use crate::brep::Carriers;
use crate::brep::Reachable;
use crate::nurbs;
use crate::sab::Record;
use crate::sab::Token;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::SurfaceGeometry;

fn record(revision: bool, ranges: [f64; 4], subtransform: Vec<Token>, knot: f64) -> Record {
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("t_spl_sur".into())];
    if revision {
        tokens.extend([Token::Long(23_100), Token::Enum(2)]);
        tokens.extend([Token::False, Token::False, Token::False, Token::False]);
        tokens.extend([
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
        ]);
    } else {
        tokens.extend([Token::Ident("nubs".into()), Token::Long(1), Token::Long(1)]);
        tokens.extend([
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
        ]);
        tokens.extend([Token::Long(2), Token::Long(2)]);
        for _ in 0..2 {
            for value in [0.0, 1.0] {
                tokens.extend([Token::Double(value), Token::Long(1)]);
            }
        }
        for point in [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ] {
            tokens.extend(point.map(Token::Double));
        }
        tokens.push(Token::Double(0.125));
    }
    tokens.extend([Token::Long(1), Token::Double(knot)]);
    for _ in 0..5 {
        tokens.push(Token::Long(0));
    }
    tokens.push(Token::False);
    tokens.extend(ranges.map(Token::Double));
    tokens.push(if revision {
        Token::Enum(7)
    } else {
        Token::Long(7)
    });
    tokens.push(Token::SubtypeOpen);
    tokens.extend(subtransform);
    tokens.extend([Token::SubtypeClose, Token::Long(9), Token::SubtypeClose]);
    Record {
        index: 0,
        name: "spline".into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn inline(program: &str) -> Vec<Token> {
    vec![
        Token::Ident("t_spl_subtrans_object".into()),
        Token::Str(program.into()),
        Token::False,
        Token::Str("100verts 1 2\n".into()),
    ]
}

fn emit(record: &Record) -> Result<(), cadmpeg_core::CodecError> {
    let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &asm_decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let table =
        nurbs::toks::SubtypeTable::from_records(&asm_decode_ctx, std::slice::from_ref(record))?;
    let decoded = nurbs::proc_surface::procedural_surface_resolving_refs(
        &asm_decode_ctx,
        &record.tokens,
        &table,
    )
    .transpose()
    .expect("resource allocation did not fail")
    .expect("syntactically complete T-spline reaches surface admission");
    let mut carriers = Carriers::default();
    carriers.surface_geo.insert(
        0,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
    );
    carriers.procedural_surface_defs.insert(0, decoded);
    emit_carrier_surface(
        &asm_decode_ctx,
        &mut AsmBrep::default(),
        record,
        0,
        &mut carriers,
        &Reachable::default(),
        crate::asm_format!("f3d"),
    )
}

#[test]
fn tspline_admission_errors_reach_the_surface_error_channel() {
    for revision in [false, true] {
        let ranges = [0.0, 1.0, 0.0, 1.0];
        assert!(emit(&record(revision, ranges, inline("v 1 0 0 0\n"), 0.25)).is_ok());
        for index in [1, 999] {
            let unresolved = record(
                revision,
                ranges,
                vec![Token::Ident("ref".into()), Token::Long(index)],
                0.25,
            );
            let error = emit(&unresolved).unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message)
                if message == "T-spline subtransform is unresolved"));
        }
        for invalid in [
            record(revision, ranges, inline(""), 0.25),
            record(revision, ranges, inline("program"), f64::INFINITY),
        ] {
            assert!(matches!(
                emit(&invalid),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        }
    }
}

#[test]
fn decreasing_tspline_ranges_keep_face_and_native_construction() {
    for revision in [false, true] {
        for ranges in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 1.0, 0.0]] {
            let mut spline = record(revision, ranges, inline("program"), 0.25);
            spline.index = 1;
            spline.len = 6;
            let face = Record {
                index: 0,
                name: "face".into(),
                tokens: vec![
                    Token::Ref(-1),
                    Token::Long(-1),
                    Token::Ref(-1),
                    Token::Ref(-1),
                    Token::Ref(-1),
                    Token::Ref(2),
                    Token::Ref(-1),
                    Token::Ref(1),
                ]
                .into(),
                offset: 0,
                len: 0,
            };
            let container = |index, name: &str, refs: &[i64]| Record {
                index,
                name: name.into(),
                tokens: refs
                    .iter()
                    .copied()
                    .map(Token::Ref)
                    .collect::<Vec<_>>()
                    .into(),
                offset: 0,
                len: 0,
            };
            let records = [
                face,
                spline,
                container(2, "shell", &[-1, -1, -1, -1, -1, 0, -1, 3]),
                container(3, "region", &[-1, -1, -1, -1, 2, 4]),
                container(4, "body", &[-1, -1, -1, 3]),
            ];
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                b"native",
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )
            .unwrap();
            let out = crate::brep::decode_with_header(
                &ctx,
                &records,
                b"native",
                None,
                "synthetic",
                crate::asm_format!("f3d"),
                crate::brep::DecodePurpose::Model,
            )
            .expect("unrepresentable range omits only the construction");
            assert_eq!(out.bodies.len(), 1);
            assert_eq!(out.regions.len(), 1);
            assert_eq!(out.shells.len(), 1);
            assert_eq!(out.faces.len(), 1);
            assert_eq!(out.surfaces.len(), 1);
            assert_eq!(out.unknowns.len(), 1);
            assert_eq!(out.unknowns[0].data().unwrap(), b"native");
            if revision {
                assert!(matches!(
                    out.surfaces[0].geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                ));
                assert_eq!(out.stats.unknown_surface_faces(), 1);
            } else {
                assert!(matches!(
                    out.surfaces[0].geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
                ));
                assert_eq!(out.procedural_surfaces.len(), 1);
                assert_eq!(
                    out.stats
                        .other_record_kinds
                        .get("cached-procedural-surface-untyped"),
                    Some(&1)
                );
                assert!(matches!(
                    out.procedural_surfaces[0].1.definition(),
                    cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Unknown { .. }
                ));
            }
        }
    }
}
