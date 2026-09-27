// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SCALAR_IMAGE: &[u8] = &[0x46, 0, 0, 0, 0, 0, 0, 0];
const LINE: &[u8] = b"ent_list(line)\0\xe0\x00entity(line)\0\xf1\xe3\xf7\x11\
    \xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
    \x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
    \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b\
    \xe0\x00entity(text)\0";
const LINE3D: &[u8] = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
    \x0f\x0f\x0f\xe4\x0f\x0f\xe4";
const ARC_Z: &[u8] = b"ent_list(arc_z)\0\xe2\x2d\xe3\x2d\x0f\xe2\x01\
    \xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe0\x00ent_list(line3d)\0";
const POSITIONAL_CONIC: &[u8] = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
    \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
    \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\x0f\x11\x43\xf0\x00\xe4\
    \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x43\xf0\x00\x0f\x0f\
    \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0";
const NAMED_CONIC: &[u8] = b"ent_list(conic)\0\
    \xe0\x01id\0\x2a\xe0\x01type\0\x1e\
    \xe0\x00gen_info\0\xe2\xf7\x13\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\
    \xe0\x01flip\0\x01\
    \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
    \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
    \xe0\x02t0\0\x0f\xe0\x02t1\0\x11\
    \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
    \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
    \xf2\xf7\x0e\xe3";

fn run<T>(
    data: &[u8],
    items: u64,
    retained: u64,
    parse: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(data, &arena, &policy)
        .expect("root reference input is admitted");
    parse(&ctx)
}

fn assert_item(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == operation));
}

fn assert_retained(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == operation));
}

fn cache_case(parse: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>) -> CodecError {
    run(SCALAR_IMAGE, 0, u64::MAX, parse).expect_err("scalar image requires a cache item")
}

#[test]
fn named_conic_scalar_cache_refuses_before_hashset_growth() {
    assert_item(
        cache_case(|ctx| super::super::named_conics(ctx, SCALAR_IMAGE).map(|_| ())),
        "creo scalar cache unique images",
    );
}

#[test]
fn positional_conic_scalar_cache_refuses_before_hashset_growth() {
    assert_item(
        cache_case(|ctx| super::super::positional_conics(ctx, SCALAR_IMAGE).map(|_| ())),
        "creo scalar cache unique images",
    );
}

#[test]
fn line_scalar_cache_refuses_before_hashset_growth() {
    assert_item(
        cache_case(|ctx| super::super::lines(ctx, SCALAR_IMAGE).map(|_| ())),
        "creo scalar cache unique images",
    );
}

#[test]
fn line3d_scalar_cache_refuses_before_hashset_growth() {
    assert_item(
        cache_case(|ctx| super::super::line3d_lines(ctx, SCALAR_IMAGE).map(|_| ())),
        "creo scalar cache unique images",
    );
}

#[test]
fn arc_z_scalar_cache_refuses_before_hashset_growth() {
    assert_item(
        cache_case(|ctx| super::super::arc_z_circles(ctx, SCALAR_IMAGE).map(|_| ())),
        "creo scalar cache unique images",
    );
}

#[test]
fn named_conic_refuses_before_vec_growth() {
    assert_eq!(
        run(NAMED_CONIC, 1, u64::MAX, |ctx| super::super::named_conics(
            ctx,
            NAMED_CONIC
        ))
        .expect("one conic admitted")
        .len(),
        1
    );
    assert_item(
        run(NAMED_CONIC, 0, u64::MAX, |ctx| {
            super::super::named_conics(ctx, NAMED_CONIC)
        })
        .expect_err("one conic needs a Vec item"),
        "creo named reference conics",
    );
}

#[test]
fn named_conic_body_refuses_before_retained_copy() {
    assert_retained(
        run(NAMED_CONIC, 1, 0, |ctx| {
            super::super::named_conics(ctx, NAMED_CONIC)
        })
        .expect_err("conic body needs retained bytes"),
        "creo named reference conic body",
    );
}

#[test]
fn positional_conic_header_refuses_before_vec_growth() {
    assert_eq!(
        run(POSITIONAL_CONIC, 2, u64::MAX, |ctx| {
            super::super::positional_conics(ctx, POSITIONAL_CONIC)
        })
        .expect("one conic admitted")
        .len(),
        1
    );
    assert_item(
        run(POSITIONAL_CONIC, 0, u64::MAX, |ctx| {
            super::super::positional_conics(ctx, POSITIONAL_CONIC)
        })
        .expect_err("header needs a Vec item"),
        "creo positional conic headers",
    );
}

#[test]
fn positional_conic_refuses_before_vec_growth() {
    assert_item(
        run(POSITIONAL_CONIC, 1, u64::MAX, |ctx| {
            super::super::positional_conics(ctx, POSITIONAL_CONIC)
        })
        .expect_err("result needs a Vec item"),
        "creo positional reference conics",
    );
}

#[test]
fn positional_conic_body_refuses_before_retained_copy() {
    assert_retained(
        run(POSITIONAL_CONIC, 2, 0, |ctx| {
            super::super::positional_conics(ctx, POSITIONAL_CONIC)
        })
        .expect_err("body needs retained bytes"),
        "creo positional reference conic body",
    );
}

#[test]
fn reference_line_start_refuses_before_vec_growth() {
    assert_eq!(
        run(LINE, 2, u64::MAX, |ctx| super::super::lines(ctx, LINE))
            .expect("one line admitted")
            .len(),
        1
    );
    assert_item(
        run(LINE, 0, u64::MAX, |ctx| super::super::lines(ctx, LINE))
            .expect_err("start needs a Vec item"),
        "creo reference line starts",
    );
}

#[test]
fn reference_line_result_refuses_before_vec_growth() {
    assert_item(
        run(LINE, 1, u64::MAX, |ctx| super::super::lines(ctx, LINE))
            .expect_err("line needs a Vec item"),
        "creo reference lines",
    );
}

#[test]
fn line3d_header_refuses_before_vec_growth() {
    assert_eq!(
        run(LINE3D, 2, u64::MAX, |ctx| super::super::line3d_lines(
            ctx, LINE3D
        ))
        .expect("one line admitted")
        .len(),
        1
    );
    assert_item(
        run(LINE3D, 0, u64::MAX, |ctx| {
            super::super::line3d_lines(ctx, LINE3D)
        })
        .expect_err("header needs a Vec item"),
        "creo line3d headers",
    );
}

#[test]
fn line3d_result_refuses_before_vec_growth() {
    assert_item(
        run(LINE3D, 1, u64::MAX, |ctx| {
            super::super::line3d_lines(ctx, LINE3D)
        })
        .expect_err("line needs a Vec item"),
        "creo line3d reference lines",
    );
}

#[test]
fn arc_z_header_refuses_before_vec_growth() {
    assert_eq!(
        run(ARC_Z, 2, u64::MAX, |ctx| super::super::arc_z_circles(
            ctx, ARC_Z
        ))
        .expect("one circle admitted")
        .len(),
        1
    );
    assert_item(
        run(ARC_Z, 0, u64::MAX, |ctx| {
            super::super::arc_z_circles(ctx, ARC_Z)
        })
        .expect_err("header needs a Vec item"),
        "creo arc-z headers",
    );
}

#[test]
fn arc_z_result_refuses_before_vec_growth() {
    assert_item(
        run(ARC_Z, 1, u64::MAX, |ctx| {
            super::super::arc_z_circles(ctx, ARC_Z)
        })
        .expect_err("circle needs a Vec item"),
        "creo arc-z circles",
    );
}

#[test]
fn reference_ellipse_refuses_before_vec_growth() {
    let conic = super::super::ReferenceConic {
        entity_id: 7,
        type_id: super::super::ConicType::Ellipse,
        flip: 1,
        start: cadmpeg_ir::features::FinitePoint3::new([-3.0, 2.0, 4.0].into())
            .expect("finite start"),
        end: cadmpeg_ir::features::FinitePoint3::new([2.0, 4.0, 4.0].into()).expect("finite end"),
        parameter_start: None,
        parameter_end: None,
        coefficient_1: cadmpeg_ir::scalar::FiniteReal::new(-5.0).expect("finite coefficient"),
        coefficient_2: cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite coefficient"),
        local_system: cadmpeg_ir::units::FiniteVector::new([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0,
        ]),
        body: Vec::new(),
        offset: 10,
    };
    let conics = [conic];
    assert_eq!(
        run(&[], 1, u64::MAX, |ctx| super::super::ellipse_carriers(
            ctx, &conics
        ))
        .expect("ellipse admitted")
        .len(),
        1
    );
    assert_item(
        run(&[], 0, u64::MAX, |ctx| {
            super::super::ellipse_carriers(ctx, &conics)
        })
        .expect_err("ellipse needs a Vec item"),
        "creo reference ellipses",
    );
}
