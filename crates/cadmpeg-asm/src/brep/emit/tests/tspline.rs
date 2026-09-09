// SPDX-License-Identifier: Apache-2.0

use super::*;

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
    let table = nurbs::toks::SubtypeTable::from_records(std::slice::from_ref(record));
    let decoded = nurbs::proc_surface::procedural_surface_resolving_refs(&record.tokens, &table)
        .expect("syntactically complete T-spline reaches surface admission");
    let mut carriers = Carriers::default();
    carriers
        .surface_geo
        .insert(0, SurfaceGeometry::Unknown { record: None });
    carriers.procedural_surface_defs.insert(0, decoded);
    emit_carrier_surface(
        &mut AsmBrep::default(),
        record,
        0,
        &mut carriers,
        &Reachable::default(),
        IdFormat("f3d"),
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
            record(revision, [1.0, 0.0, 0.0, 1.0], inline("program"), 0.25),
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
