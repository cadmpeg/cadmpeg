// SPDX-License-Identifier: Apache-2.0

use super::{keep_faces_and_carriers, walk_reachable_topology, TopologyContext};
use crate::brep::{AsmBrep, Carriers, DecodePurpose, Reachable};
use crate::kernel_header::RefWidth;
use crate::nurbs;
use crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::RevisionCacheForm;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::collections::HashSet;

fn ref_record(index: usize, name: &str, refs: &[i64]) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: refs.iter().copied().map(Token::Ref).collect(),
        offset: 0,
        len: 0,
    }
}

#[test]
fn chains_list_only_children_that_are_emitted() {
    let records = [
        ref_record(0, "body", &[-1, -1, -1, 1]),
        ref_record(1, "lump", &[-1, -1, -1, 3, 2, 0]),
        ref_record(2, "shell", &[-1, -1, -1, 4, -1, -1, -1, 1]),
        ref_record(3, "lump", &[-1, -1, -1, -1, -1, 9]),
        ref_record(4, "face", &[-1, -1, -1, -1]),
    ];
    let by_index = records
        .iter()
        .map(|record| {
            (
                i64::try_from(record.index).expect("test value fits"),
                record,
            )
        })
        .collect();
    let format = crate::asm_format!("f3d");
    {
        let regions = super::region_chain(&records[0], &by_index, format);
        assert_eq!(
            regions.len(),
            1,
            "a lump owned by another body ends the chain"
        );
        let shells = super::shell_chain(
            &records[1],
            &by_index,
            &std::collections::HashSet::from([2]),
            format,
        );
        assert_eq!(shells.len(), 1, "a face record is not a shell");
        let missing = ref_record(5, "body", &[-1, -1, -1, 7]);
        assert!(super::region_chain(&missing, &by_index, format).is_empty());
    };
}

fn ident(bytes: &mut Vec<u8>, name: &str) {
    bytes.extend_from_slice(&[0x0d, u8::try_from(name.len()).unwrap()]);
    bytes.extend_from_slice(name.as_bytes());
}

fn integer(bytes: &mut Vec<u8>, tag: u8, value: i64, width: RefWidth) {
    bytes.push(tag);
    match width {
        RefWidth::Four => bytes.extend_from_slice(&i32::try_from(value).unwrap().to_le_bytes()),
        RefWidth::Eight => bytes.extend_from_slice(&value.to_le_bytes()),
    }
}

fn double(bytes: &mut Vec<u8>, value: f64) {
    bytes.push(0x06);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn triple(bytes: &mut Vec<u8>, tag: u8, values: [f64; 3]) {
    bytes.push(tag);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn revision_sum_solved_cache_remains_a_nurbs_face_carrier() {
    let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &asm_decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    for width in [RefWidth::Four, RefWidth::Eight] {
        for tolerance in [0.0, 0.125] {
            let mut bytes = Vec::new();
            ident(&mut bytes, "spline");
            bytes.extend_from_slice(&[0x0b, 0x0f]);
            ident(&mut bytes, "sum_spl_sur");
            integer(&mut bytes, 0x04, 23_100, width);
            for direction in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                ident(&mut bytes, "straight");
                triple(&mut bytes, 0x13, [0.0; 3]);
                triple(&mut bytes, 0x14, direction);
                bytes.extend_from_slice(&[0x0b, 0x0b]);
            }
            triple(&mut bytes, 0x13, [0.0; 3]);
            integer(&mut bytes, 0x15, 0, width);
            ident(&mut bytes, "nubs");
            for _ in 0..2 {
                integer(&mut bytes, 0x04, 1, width);
            }
            for _ in 0..4 {
                integer(&mut bytes, 0x15, 0, width);
            }
            for _ in 0..2 {
                integer(&mut bytes, 0x04, 2, width);
            }
            for _ in 0..2 {
                for knot in [0.0, 1.0] {
                    double(&mut bytes, knot);
                    integer(&mut bytes, 0x04, 1, width);
                }
            }
            for pole in [
                [0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
            ] {
                for coordinate in pole {
                    double(&mut bytes, coordinate);
                }
            }
            double(&mut bytes, tolerance);
            for _ in 0..6 {
                integer(&mut bytes, 0x04, 0, width);
            }
            bytes.extend_from_slice(&[0x0b, 0x10, 0x0b, 0x0b, 0x0b, 0x0b, 0x11]);
            ident(&mut bytes, "face");
            for reference in [-1, -1, -1, -1, -1, 2, -1, 0] {
                integer(&mut bytes, 0x0c, reference, width);
            }
            bytes.push(0x11);
            for (name, references) in [
                ("shell", &[-1, -1, -1, -1, -1, 1, -1, 3][..]),
                ("lump", &[-1, -1, -1, -1, 2, 4][..]),
                ("body", &[-1, -1, -1, 3][..]),
            ] {
                ident(&mut bytes, name);
                for &reference in references {
                    integer(&mut bytes, 0x0c, reference, width);
                }
                bytes.push(0x11);
            }

            let records = crate::test_support::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            let by_index = records
                .iter()
                .map(|record| {
                    (
                        i64::try_from(record.index).expect("test value fits"),
                        record,
                    )
                })
                .collect();
            let table = nurbs::toks::SubtypeTable::from_records(&records);
            let decoded = nurbs::proc_surface::procedural_surface_resolving_refs(
                &asm_decode_ctx,
                &records[0].tokens,
                &table,
            )
            .transpose()
            .expect("resource allocation did not fail")
            .unwrap();
            let DecodedProceduralSurfaceDefinition::Sum {
                revision_form: Some(form),
                ..
            } = decoded.definition()
            else {
                panic!("expected a revision Sum");
            };
            assert!(matches!(form.cache, RevisionCacheForm::SolvedCache { .. }));
            assert_eq!(
                form.cache
                    .fit_tolerance()
                    .map(cadmpeg_ir::geometry::FitTolerance::get),
                Some(tolerance * 10.0)
            );
            assert_eq!(decoded.cache_fit_tolerance(), Some(tolerance * 10.0));
            assert_eq!(decoded.legacy_cache_fit_tolerance(), None);

            let mut out = AsmBrep::default();
            let mut carriers = Carriers::default();
            let mut reach = Reachable::default();
            let format = crate::asm_format!("f3d");
            keep_faces_and_carriers(
                TopologyContext {
                    ctx: &asm_decode_ctx,
                    by_index: &by_index,
                    token_table: &table,
                    purpose: DecodePurpose::Model,
                    format,
                },
                &mut out,
                &records,
                &mut carriers,
                &mut reach,
            )
            .expect("generated identities are valid");
            assert_eq!(out.stats.nurbs_surfaces, 1);
            crate::brep::emit::emit_carrier_records(
                &mut out,
                &records,
                &mut carriers,
                &reach,
                crate::brep::emit::CurveSenseRefs {
                    reversed_curve_refs: &HashSet::new(),
                    forward_curve_refs: &HashSet::new(),
                },
                format,
            )
            .expect("valid carrier fixture");
            assert_eq!(out.surfaces.len(), 1);
            assert!(matches!(
                out.surfaces[0].geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
            ));
        }
    }
}

#[test]
fn history_pcurve_use_has_no_invented_parameter_interval() {
    let asm_decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let (asm_decode_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &asm_decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    let record = |index, name: &str, fields: &[i64]| Record {
        index,
        name: name.into(),
        tokens: fields.iter().copied().map(Token::Ref).collect(),
        offset: 0,
        len: 0,
    };
    let records = [
        record(0, "face", &[-1, -1, -1, -1, 1]),
        record(1, "loop", &[-1, -1, -1, -1, 2]),
        record(2, "coedge", &[-1, -1, -1, 2, 2, -1, 3, -1, 1, 4]),
        record(3, "edge", &[-1; 9]),
        record(4, "pcurve", &[]),
    ];
    let by_index = records
        .iter()
        .map(|r| (i64::try_from(r.index).expect("test value fits"), r))
        .collect();
    let table = nurbs::toks::SubtypeTable::from_records(&records);
    let mut carriers = Carriers::default();
    let mut reach = Reachable {
        faces: HashSet::from([0]),
        ..Reachable::default()
    };
    let mut out = AsmBrep::default();
    walk_reachable_topology(
        TopologyContext {
            ctx: &asm_decode_ctx,
            by_index: &by_index,
            token_table: &table,
            purpose: DecodePurpose::History,
            format: crate::asm_format!("f3d"),
        },
        &mut out,
        &mut carriers,
        &mut reach,
    )
    .expect("history topology is within resource limits");
    super::super::emit::emit_coedges(
        &asm_decode_ctx,
        &mut out,
        &records,
        crate::brep::emit::CoedgeDecodeInputs {
            token_table: &table,
            save_format_major: None,
        },
        &carriers,
        &reach,
        crate::asm_format!("f3d"),
    )
    .unwrap();
    assert_eq!(out.coedges.len(), 1);
    assert_eq!(out.coedges[0].pcurves.len(), 1);
    assert_eq!(out.coedges[0].pcurves[0].parameter_range, None);
}
