// SPDX-License-Identifier: Apache-2.0

use super::{keep_faces_and_carriers, walk_reachable_topology, TopologyContext};
use crate::brep::{AsmBrep, Carriers, DecodePurpose, Reachable};
use crate::kernel_header::RefWidth;
use crate::nurbs;
use crate::nurbs::proc_surface::DecodedProceduralSurfaceDefinition;
use crate::sab::{Record, Token};
use cadmpeg_ir::geometry::RevisionCacheForm;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use std::collections::{HashMap, HashSet};

fn ref_record(index: usize, name: &str, refs: &[i64]) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: refs.iter().copied().map(Token::Ref).collect(),
        offset: 0,
        len: 0,
    }
}

fn with_collection_limit(
    operation: &str,
    mut f: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>) -> cadmpeg_core::CodecError,
) -> cadmpeg_core::CodecError {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            Err::<(), _>(f(&ctx))
        },
    )
}

fn with_work_limit(
    operation: &str,
    mut f: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>) -> cadmpeg_core::CodecError,
) -> cadmpeg_core::CodecError {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            Err::<(), _>(f(&ctx))
        },
    )
}

fn assert_work_refusal(error: &cadmpeg_core::CodecError, operation: &str) {
    use cadmpeg_core::decode::ResourceDimension;
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected work refusal: {error:?}")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, operation);
}

fn indexed_records(records: &[Record]) -> HashMap<i64, &Record> {
    records
        .iter()
        .map(|record| (i64::try_from(record.index).unwrap(), record))
        .collect()
}

fn reachable_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
    faces: HashSet<i64>,
) -> Result<(), cadmpeg_core::CodecError> {
    let by_index = indexed_records(records);
    let token_table = crate::nurbs::toks::SubtypeTable::from_records(ctx, records)?;
    let mut out = AsmBrep::default();
    let mut carriers = Carriers::default();
    let mut reach = Reachable {
        faces,
        ..Reachable::default()
    };
    super::walk_reachable_topology(
        TopologyContext {
            ctx,
            by_index: &by_index,
            token_table: &token_table,
            purpose: DecodePurpose::History,
            format: crate::asm_format!("f3d"),
        },
        &mut out,
        records,
        &mut carriers,
        &mut reach,
        &mut ctx.reserve_scoped(0, "ASM test scratch").unwrap(),
    )
}

fn wire_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[Record],
) -> Result<(), cadmpeg_core::CodecError> {
    let by_index = indexed_records(records);
    let token_table = crate::nurbs::toks::SubtypeTable::from_records(ctx, records)?;
    let mut out = AsmBrep::default();
    let mut carriers = Carriers::default();
    let mut reach = Reachable::default();
    super::collect_wire_topology(
        TopologyContext {
            ctx,
            by_index: &by_index,
            token_table: &token_table,
            purpose: DecodePurpose::Model,
            format: crate::asm_format!("f3d"),
        },
        &mut out,
        records,
        None,
        &mut carriers,
        &mut reach,
        &mut ctx.reserve_scoped(0, "ASM test scratch").unwrap(),
    )
    .map(|_| ())
}

fn assert_collection_refusal(error: &cadmpeg_core::CodecError, operation: &str) {
    use cadmpeg_core::decode::ResourceDimension;
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected resource refusal: {error:?}")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, operation);
}

#[test]
fn ring_coedges_refuses_collection_limit() {
    let records = [
        ref_record(0, "loop", &[-1, -1, -1, -1, 1]),
        ref_record(1, "coedge", &[-1, -1, -1, 1]),
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
    let error = with_collection_limit("ASM ring coedges", |ctx| {
        super::ring_coedges(
            ctx,
            &records[0],
            &by_index,
            &HashSet::from([1]),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_collection_refusal(&error, "ASM ring coedges");
}

#[test]
fn loop_chain_refuses_collection_limit() {
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, 1]),
        ref_record(1, "loop", &[-1, -1, -1, -1]),
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
    let error = with_collection_limit("ASM face loops", |ctx| {
        super::loop_chain(
            ctx,
            &records[0],
            &by_index,
            &HashSet::from([1]),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_collection_refusal(&error, "ASM face loops");
}

#[test]
fn face_chain_refuses_collection_limit() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, 1]),
        ref_record(1, "face", &[-1, -1, -1, -1]),
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
    let error = with_collection_limit("ASM shell faces", |ctx| {
        super::shell_faces(
            ctx,
            &records[0],
            &by_index,
            &HashSet::from([1]),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_collection_refusal(&error, "ASM shell faces");
}

#[test]
fn shell_wire_roots_refuses_collection_limit() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, -1, 1]),
        ref_record(1, "wire", &[]),
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
    let error = with_collection_limit("ASM shell wire roots", |ctx| {
        super::shell_wire_roots(ctx, &records[0], &by_index).unwrap_err()
    });
    assert_collection_refusal(&error, "ASM shell wire roots");
}

#[test]
fn shell_chain_refuses_collection_limit() {
    let records = [
        ref_record(0, "region", &[-1, -1, -1, -1, 1]),
        ref_record(1, "shell", &[-1, -1, -1, -1]),
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
    let error = with_collection_limit("ASM region shells", |ctx| {
        super::shell_chain(ctx, &records[0], &by_index, crate::asm_format!("f3d")).unwrap_err()
    });
    assert_collection_refusal(&error, "ASM region shells");
}

#[test]
fn region_chain_refuses_collection_limit() {
    let records = [
        ref_record(0, "body", &[-1, -1, -1, 1]),
        ref_record(1, "region", &[-1, -1, -1, -1]),
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
    let error = with_collection_limit("ASM body regions", |ctx| {
        super::region_chain(ctx, &records[0], &by_index, crate::asm_format!("f3d")).unwrap_err()
    });
    assert_collection_refusal(&error, "ASM body regions");
}

#[test]
fn reachable_face_loop_walk_refuses_work() {
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, 1]),
        ref_record(1, "loop", &[-1, -1, -1, -1, -1]),
    ];
    let error = with_work_limit("ASM reachable face loop walk", |ctx| {
        reachable_topology(ctx, &records, HashSet::from([0])).unwrap_err()
    });
    assert_work_refusal(&error, "ASM reachable face loop walk");
}

#[test]
fn reachable_coedge_ring_walk_refuses_work() {
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, 1]),
        ref_record(1, "loop", &[-1, -1, -1, -1, 2]),
        ref_record(2, "coedge", &[-1, -1, -1, -1]),
    ];
    let error = with_work_limit("ASM reachable coedge ring walk", |ctx| {
        reachable_topology(ctx, &records, HashSet::from([0])).unwrap_err()
    });
    assert_work_refusal(&error, "ASM reachable coedge ring walk");
}

#[test]
fn shell_wire_chain_walk_refuses_work() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, -1, 1]),
        ref_record(1, "wire", &[-1; 8]),
    ];
    let error = with_work_limit("ASM shell wire chain walk", |ctx| {
        wire_topology(ctx, &records).unwrap_err()
    });
    assert_work_refusal(&error, "ASM shell wire chain walk");
}

#[test]
fn wire_coedge_ring_walk_refuses_work() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, -1, 1]),
        ref_record(1, "wire", &[-1, -1, -1, -1, 2, -1, -1, -1]),
        ref_record(2, "coedge", &[-1; 7]),
    ];
    let error = with_work_limit("ASM wire coedge ring walk", |ctx| {
        wire_topology(ctx, &records).unwrap_err()
    });
    assert_work_refusal(&error, "ASM wire coedge ring walk");
}

#[test]
fn ring_coedges_walk_refuses_work() {
    let records = [
        ref_record(0, "loop", &[-1, -1, -1, -1, 1]),
        ref_record(1, "coedge", &[-1; 4]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM ring coedges walk", |ctx| {
        super::ring_coedges(
            ctx,
            &records[0],
            &by_index,
            &HashSet::from([1]),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_work_refusal(&error, "ASM ring coedges walk");
}

#[test]
fn loop_chain_walk_refuses_work() {
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, 1]),
        ref_record(1, "loop", &[-1; 4]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM face loop chain walk", |ctx| {
        super::loop_chain(
            ctx,
            &records[0],
            &by_index,
            &HashSet::from([1]),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_work_refusal(&error, "ASM face loop chain walk");
}

#[test]
fn face_chain_walk_refuses_work() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, -1, 1]),
        ref_record(1, "face", &[-1; 4]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM shell face chain walk", |ctx| {
        super::face_chain(
            ctx,
            &records[0],
            &by_index,
            &HashSet::from([1]),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_work_refusal(&error, "ASM shell face chain walk");
}

#[test]
fn subshell_ancestor_walk_refuses_work() {
    let records = [
        ref_record(0, "subshell", &[-1, -1, -1, 1]),
        ref_record(1, "shell", &[-1; 4]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM subshell ancestor walk", |ctx| {
        super::subshell_ancestor_shells(ctx, &records, &by_index).unwrap_err()
    });
    assert_work_refusal(&error, "ASM subshell ancestor walk");
}

#[test]
fn shell_faces_walk_refuses_work() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, 1, -1]),
        ref_record(1, "subshell", &[-1; 8]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM shell faces walk", |ctx| {
        super::shell_faces(
            ctx,
            &records[0],
            &by_index,
            &HashSet::new(),
            crate::asm_format!("f3d"),
        )
        .unwrap_err()
    });
    assert_work_refusal(&error, "ASM shell faces walk");
}

#[test]
fn shell_wire_roots_walk_refuses_work() {
    let records = [
        ref_record(0, "shell", &[-1, -1, -1, -1, 1, -1, -1]),
        ref_record(1, "subshell", &[-1; 8]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM shell wire roots walk", |ctx| {
        super::shell_wire_roots(ctx, &records[0], &by_index).unwrap_err()
    });
    assert_work_refusal(&error, "ASM shell wire roots walk");
}

#[test]
fn subshell_face_chain_walk_refuses_work() {
    let records = [ref_record(0, "face", &[-1; 4])];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM subshell face chain walk", |ctx| {
        super::face_chain_from(
            ctx,
            Some(0),
            &by_index,
            &HashSet::from([0]),
            crate::asm_format!("f3d"),
            &mut Vec::new(),
        )
        .unwrap_err()
    });
    assert_work_refusal(&error, "ASM subshell face chain walk");
}

#[test]
fn region_shell_chain_walk_refuses_work() {
    let records = [
        ref_record(0, "region", &[-1, -1, -1, -1, 1]),
        ref_record(1, "shell", &[-1; 4]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM region shell chain walk", |ctx| {
        super::shell_chain(ctx, &records[0], &by_index, crate::asm_format!("f3d")).unwrap_err()
    });
    assert_work_refusal(&error, "ASM region shell chain walk");
}

#[test]
fn body_region_chain_walk_refuses_work() {
    let records = [
        ref_record(0, "body", &[-1, -1, -1, 1]),
        ref_record(1, "region", &[-1; 4]),
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM body region chain walk", |ctx| {
        super::region_chain(ctx, &records[0], &by_index, crate::asm_format!("f3d")).unwrap_err()
    });
    assert_work_refusal(&error, "ASM body region chain walk");
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
            for reference in [-1, -1, -1, -1, -1, -1, -1, 0] {
                integer(&mut bytes, 0x0c, reference, width);
            }
            bytes.push(0x11);

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
            let table = nurbs::toks::SubtypeTable::from_records(&asm_decode_ctx, &records).unwrap();
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
                &mut asm_decode_ctx
                    .reserve_scoped(0, "ASM test scratch")
                    .unwrap(),
            )
            .expect("generated identities are valid");
            assert_eq!(out.stats.nurbs_surfaces, 1);
            crate::brep::emit::emit_carrier_records(
                &asm_decode_ctx,
                &mut out,
                &records,
                (
                    &mut carriers,
                    &mut asm_decode_ctx
                        .reserve_scoped(0, "ASM test scratch")
                        .unwrap(),
                    DecodePurpose::Model,
                ),
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
    let table = nurbs::toks::SubtypeTable::from_records(&asm_decode_ctx, &records).unwrap();
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
        &records,
        &mut carriers,
        &mut reach,
        &mut asm_decode_ctx
            .reserve_scoped(0, "ASM test scratch")
            .unwrap(),
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

#[test]
fn model_pcurve_parameter_range_refuses_collection_limit() {
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, 1]),
        ref_record(1, "loop", &[-1, -1, -1, -1, 2]),
        ref_record(2, "coedge", &[-1, -1, -1, 2, -1, -1, 3, -1, -1, 4]),
        ref_record(3, "edge", &[-1; 9]),
        Record {
            index: 4,
            name: "pcurve".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Long(0),
                Token::True,
                Token::SubtypeOpen,
                Token::Ident("exp_par_cur".into()),
                Token::Ident("nubs".into()),
                Token::Long(1),
                Token::Enum(0),
                Token::Long(2),
                Token::Double(0.0),
                Token::Long(1),
                Token::Double(1.0),
                Token::Long(1),
                Token::Double(0.0),
                Token::Double(0.0),
                Token::Double(1.0),
                Token::Double(0.0),
                Token::SubtypeClose,
            ]
            .into(),
            offset: 0,
            len: 0,
        },
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
    let error = with_collection_limit("ASM topology pcurve_parameter_ranges", |ctx| {
        let table = nurbs::toks::SubtypeTable::from_records(ctx, &records)
            .expect("subtype table fits limit");
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable {
            faces: HashSet::from([0]),
            ..Reachable::default()
        };
        walk_reachable_topology(
            TopologyContext {
                ctx,
                by_index: &by_index,
                token_table: &table,
                purpose: DecodePurpose::Model,
                format: crate::asm_format!("f3d"),
            },
            &mut out,
            &records,
            &mut carriers,
            &mut reach,
            &mut ctx.reserve_scoped(0, "ASM test scratch").unwrap(),
        )
        .expect_err("parameter-range map exceeds collection limit")
    });
    assert_collection_refusal(&error, "ASM topology pcurve_parameter_ranges");
}

#[test]
fn model_pcurve_subtype_lookup_propagates_work_refusal() {
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, 1]),
        ref_record(1, "loop", &[-1, -1, -1, -1, 2]),
        ref_record(2, "coedge", &[-1, -1, -1, 2, -1, -1, 3, -1, -1, 4]),
        ref_record(3, "edge", &[-1; 9]),
        Record {
            index: 4,
            name: "pcurve".into(),
            tokens: vec![
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Ref(-1),
                Token::Long(0),
                Token::True,
                Token::SubtypeOpen,
                Token::Ident("exp_par_cur".into()),
                Token::SubtypeClose,
            ]
            .into(),
            offset: 0,
            len: 0,
        },
    ];
    let by_index = indexed_records(&records);
    let error = with_work_limit("ASM payload subtype token scan", |ctx| {
        let table = nurbs::toks::SubtypeTable::from_records(ctx, &records)
            .expect("subtype table fits limit");
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable {
            faces: HashSet::from([0]),
            ..Reachable::default()
        };
        match walk_reachable_topology(
            TopologyContext {
                ctx,
                by_index: &by_index,
                token_table: &table,
                purpose: DecodePurpose::Model,
                format: crate::asm_format!("f3d"),
            },
            &mut out,
            &records,
            &mut carriers,
            &mut reach,
            &mut ctx.reserve_scoped(0, "ASM test scratch").unwrap(),
        ) {
            Err(error) => error,
            Ok(()) => panic!("payload subtype lookup did not refuse"),
        }
    });
    assert_work_refusal(&error, "ASM payload subtype token scan");
}

#[test]
fn history_construction_kind_does_not_consume_retained_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let records = [
        ref_record(0, "face", &[-1, -1, -1, -1, -1, -1, -1, 1]),
        Record {
            index: 1,
            name: "spline".into(),
            offset: 0,
            len: 0,
            tokens: vec![
                Token::SubtypeOpen,
                Token::Ident("mystery".into()),
                Token::SubtypeClose,
            ]
            .into(),
        },
    ];
    let by_index = indexed_records(&records);
    let token_table = nurbs::toks::SubtypeTable::from_records(
        &cadmpeg_test_support::service_decode_context(),
        &records,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scratch = ctx.reserve_scoped(0, "ASM test decode scratch").unwrap();
    let mut carriers = Carriers::default();
    let mut reach = Reachable::default();
    keep_faces_and_carriers(
        TopologyContext {
            ctx: &ctx,
            by_index: &by_index,
            token_table: &token_table,
            purpose: DecodePurpose::History,
            format: crate::asm_format!("f3d"),
        },
        &mut AsmBrep::default(),
        &records,
        &mut carriers,
        &mut reach,
        &mut scratch,
    )
    .unwrap();
    assert!(reach.faces.contains(&0));
    assert!(reach.surfaces.contains(&1));
    assert!(carriers.procedural_surface_defs.is_empty());
    assert!(matches!(
        carriers.surface_geo.get(&1),
        Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
            record: None
        }))
    ));
}
