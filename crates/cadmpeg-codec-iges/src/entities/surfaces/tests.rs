// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use cadmpeg_ir::geometry::SolvedCurveGeometry;

use std::io::Cursor;

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeFailure;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;

use crate::loss::IgesLossCode;

use crate::test_support::test_owned::owned_test_file;
use crate::test_support::test_owned::owned_test_file_with_global_and_line_fonts;
use crate::test_support::test_owned::OwnedTestEntity;
use crate::test_support::test_procedural_surfaces::interval_certified_linear_bezier_ruled_surface_file;
use crate::test_support::test_surface_fixtures::circular_ruled_surface_file;
use crate::test_support::test_surface_fixtures::composite_ruled_surface_file;
use crate::test_support::test_surface_fixtures::composite_tabulated_cylinder_file;
use crate::test_support::test_surface_fixtures::ellipse_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::hyperbola_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::hyperbola_surface_of_revolution_file_with_global;
use crate::test_support::test_surface_fixtures::line_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::line_surface_of_revolution_file_with_global;
use crate::test_support::test_surface_fixtures::nurbs_surface_file;
use crate::test_support::test_surface_fixtures::offset_nurbs_surface_file;
use crate::test_support::test_surface_fixtures::offset_plane_file;
use crate::test_support::test_surface_fixtures::placed_hyperbola_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::placed_overflow_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::placed_surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::plane_file;
use crate::test_support::test_surface_fixtures::rational_ruled_surface_file;
use crate::test_support::test_surface_fixtures::ruled_surface_file;
use crate::test_support::test_surface_fixtures::ruled_surface_file_with_developable_flag;
use crate::test_support::test_surface_fixtures::surface_of_revolution_file;
use crate::test_support::test_surface_fixtures::tabulated_cylinder_file;
use crate::test_support::test_surface_fixtures::tabulated_hyperbola_file;
use crate::test_support::test_surface_fixtures::tabulated_hyperbola_file_with_global;
use crate::test_support::test_surface_fixtures::trimmed_surface_of_revolution_file;

use crate::test_support::test_tabulated_surfaces::placed_tabulated_hyperbola_file;
use crate::test_support::test_tabulated_surfaces::placed_tabulated_hyperbola_file_with_global;
use crate::test_support::test_tabulated_surfaces::placed_tabulated_line_file;
use crate::test_support::test_tabulated_surfaces::placed_tabulated_line_file_with_global;
use crate::test_support::test_tabulated_surfaces::placed_tabulated_nurbs_overflow_file;
use crate::IgesCodec;

use crate::global::GlobalTable;

#[test]
fn surface_grid_error_fields_refuse_retained_limit_before_copy() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap();
    let cases = [
        (vec![Vec::new()], Vec::<Vec<f64>>::new(), 8_u64),
        (vec![Vec::new()], vec![vec![1.0]], 12_u64),
        (vec![vec![point]], vec![vec![0.0]], 12_u64),
    ];
    for (rows, weights, cap) in cases {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result =
            super::pair_admitted_surface_poles(Some(&ctx), rows, Some(weights), "outer", "inner");
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges surface grid error field")
        );
    }
}

const EPS_RATIONAL_RULED: f64 = 1.0e-10;
const EPS_LINEAR_BEZIER_RULED: f64 = 1.0e-5;

use super::{
    angular_basis, offset_indicator_parameters,
    tabulated_directrix_type_allowed,
};

fn assert_surface_collection_refusal(bytes: &[u8], operation: &str) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        match crate::IgesCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        ) {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == operation {
                    return;
                }
                cap = limit.used.checked_add(limit.additional).unwrap();
            }
            Ok(_) => {
                panic!("expected surface collection refusal at {operation}, but decode succeeded")
            }
            Err(error) => panic!("expected surface collection refusal at {operation}: {error:?}"),
        }
    }
    panic!("surface collection refusal was not reached: {operation}");
}

#[test]
fn surface_identity_copies_refuse_at_retained_byte_limit() {
    for bytes in [placed_tabulated_line_file()] {
        IgesCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let mut cap = 0_u64;
        let mut refused = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            match IgesCodec.decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    policy,
                    ..DecodeOptions::default()
                },
            ) {
                Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    if limit.operation == "iges surface identity copy" {
                        refused = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => {
                    panic!("expected surface identity refusal at cap {cap}, but decode succeeded")
                }
                Err(error) => panic!("expected surface identity refusal at cap {cap}: {error:?}"),
            }
        }
        assert!(refused, "surface identity refusal was not reached");
    }
}

#[test]
fn exact_placed_surface_carriers_refuse_nested_curve_box() {
    for bytes in [
        placed_tabulated_hyperbola_file(),
        placed_hyperbola_surface_of_revolution_file(),
    ] {
        IgesCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        assert_surface_collection_refusal(&bytes, "iges exact placed curve box");
    }
}

#[test]
fn type122_projection_refuses_tabulated_carrier_rows_knots_and_slots() {
    let bytes = placed_tabulated_line_file();
    for operation in [
        "iges tabulated pole rows",
        "iges tabulated pole row controls",
        "iges tabulated u knots",
        "iges tabulated v knots",
        "iges tabulated placed directrix slots",
        "iges tabulated neutral surface slots",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
}

#[test]
fn type122_projection_refuses_rational_tabulated_weight_rows() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "RATIONAL".into(),
            status: "00000000",
            parameters: "126,2,2,1,0,0,0,0,0,0,1,1,1,1,0.5,1,0,0,0,1,1,0,2,0,0,0,1,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 122,
            form: 0,
            label: "TABULATE".into(),
            status: "00000000",
            parameters: "122,1,0,0,2;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
            cache: Some(SolvedSurfaceGeometry::Nurbs(nurbs)),
            ..
        } if nurbs.weights().is_some()
    )));
    for operation in [
        "iges tabulated weight rows",
        "iges tabulated weight row controls",
        "iges tabulated weighted rows",
        "iges tabulated weighted row controls",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
}

#[test]
fn surface_projectors_refuse_neutral_and_placed_curve_slots() {
    for (bytes, operation) in [
        (plane_file(), "iges plane neutral surface slots"),
        (ruled_surface_file(), "iges ruled neutral surface slots"),
        (
            placed_tabulated_hyperbola_file(),
            "iges tabulated exact placed directrix slots",
        ),
        (
            tabulated_hyperbola_file(),
            "iges tabulated exact neutral surface slots",
        ),
        (
            hyperbola_surface_of_revolution_file(),
            "iges revolution exact neutral surface slots",
        ),
        (
            placed_hyperbola_surface_of_revolution_file(),
            "iges revolution exact placed generatrix slots",
        ),
        (
            placed_surface_of_revolution_file(),
            "iges revolution placed generatrix slots",
        ),
        (
            offset_plane_file(1.0, 2.0),
            "iges offset neutral surface slots",
        ),
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
}

#[test]
fn surface_projectors_refuse_procedural_attachment_slots() {
    for bytes in [
        ruled_surface_file(),
        tabulated_hyperbola_file(),
        placed_tabulated_line_file(),
        hyperbola_surface_of_revolution_file(),
        ellipse_surface_of_revolution_file(),
        nurbs_surface_file(),
        offset_plane_file(1.0, 2.0),
    ] {
        assert_surface_collection_refusal(&bytes, "iges procedural surface slots");
    }
}

#[test]
fn ruled_developability_loss_refuses_unadmitted_slot() {
    let bytes = ruled_surface_file();
    assert_surface_collection_refusal(&bytes, "iges entity loss slots");
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind() }));
}

#[test]
fn tabulated_nonfinite_placement_loss_refuses_unadmitted_slot() {
    let bytes = placed_tabulated_nurbs_overflow_file();
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == IgesLossCode::NurbsTransformNonFinite.kind() }),
        "{:#?}",
        result.report().losses
    );
    assert_surface_collection_refusal(&bytes, "iges entity loss slots");
}

#[test]
fn revolution_nonfinite_generatrix_loss_refuses_unadmitted_slot() {
    let bytes = placed_overflow_surface_of_revolution_file();
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(
        result
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == IgesLossCode::NurbsTransformNonFinite.kind() }),
        "{:#?}",
        result.report().losses
    );
    assert_surface_collection_refusal(&bytes, "iges entity loss slots");
}

#[test]
fn reversed_ruled_knots_loss_refuses_unadmitted_slot() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 110,
            form: 0,
            label: "RAIL1".into(),
            status: "00010000",
            parameters: "110,0,0,0,1,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 126,
            form: 0,
            label: "RAIL2".into(),
            status: "00010000",
            parameters:
                "126,1,1,1,0,1,0,8D307,8D307,1D308,1D308,1,1,0,1,0,1,1,0,8D307,1D308,0,0,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 118,
            form: 1,
            label: "RULED".into(),
            status: "00000000",
            parameters: "118,1,3,1,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(
        result.report().losses.iter().any(|loss| {
            loss.code == IgesLossCode::NurbsTransformNonFinite.kind()
                && loss.message == "IGES reversed second rail knots are non-finite"
        }),
        "{:#?}",
        result.report().losses
    );
    assert_surface_collection_refusal(&bytes, "iges entity loss slots");
}

#[test]
fn type128_projection_refuses_source_lanes_nested_rows_and_surface_slot() {
    let polynomial = nurbs_surface_file();
    for operation in [
        "iges NURBS surface source u knots",
        "iges NURBS surface source v knots",
        "iges NURBS surface admitted u knots",
        "iges NURBS surface admitted v knots",
        "iges NURBS surface source weights",
        "iges NURBS surface positive weights",
        "iges NURBS surface source poles",
        "iges NURBS surface source ranges",
        "iges NURBS surface placed controls",
        "iges NURBS surface pole rows",
        "iges NURBS surface pole row controls",
        "iges NURBS surface neutral slots",
    ] {
        assert_surface_collection_refusal(&polynomial, operation);
    }
    let rational = owned_test_file_with_global_and_line_fonts(&[OwnedTestEntity {
        entity_type: 128,
        form: 0,
        label: "SURFACE".into(),
        status: "00000000",
        parameters: "128,1,1,1,1,0,0,0,0,0,0,0,1,1,0,0,1,1,1,0.99,1,1,0,0,0,1,0,0,1,0,1,1,0,1,0,1,0,1;".into(),
    }], b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H900101.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;", &[(1, 1)]);
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(&rational), &DecodeOptions::default())
        .unwrap();
    assert_eq!(service.ir().model.surfaces.len(), 1);
    for operation in [
        "iges NURBS surface neutral weights",
        "iges NURBS surface weight rows",
        "iges NURBS surface weight row controls",
        "iges NURBS surface weighted rows",
        "iges NURBS surface weighted row controls",
    ] {
        assert_surface_collection_refusal(&rational, operation);
    }
}

#[test]
fn interval_certified_ruled_rails_refuse_coordinate_arrays() {
    let bytes = interval_certified_linear_bezier_ruled_surface_file();
    for operation in [
        "iges ruled linear x values",
        "iges ruled linear y values",
        "iges ruled linear z values",
        "iges ruled linear x uncertainties",
        "iges ruled linear y uncertainties",
        "iges ruled linear z uncertainties",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!service.ir().model.surfaces.is_empty());
}

#[test]
fn homogeneous_ruled_surface_refuses_control_and_knot_lanes() {
    let bytes = rational_ruled_surface_file();
    for operation in [
        "iges ruled homogeneous controls",
        "iges ruled homogeneous knots",
        "iges ruled surface controls",
        "iges ruled surface weights",
        "iges ruled span pole rows",
        "iges ruled span pole row controls",
        "iges ruled span weight rows",
        "iges ruled span weight row controls",
        "iges ruled span v knots",
        "iges ruled span weighted rows",
        "iges ruled span weighted row controls",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!service.ir().model.surfaces.is_empty());
}

#[test]
fn same_basis_ruled_surface_refuses_nested_poles_and_knots() {
    let bytes = ruled_surface_file();
    for operation in [
        "iges ruled same-basis pole rows",
        "iges ruled same-basis pole row controls",
        "iges ruled same-basis u knots",
        "iges ruled same-basis v knots",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!service.ir().model.surfaces.is_empty());
}

#[test]
fn ruled_shared_weight_admission_refuses_before_copy() {
    let bytes = ruled_surface_file();
    assert_surface_collection_refusal(&bytes, "iges ruled shared weights");
    let service = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(service.ir().model.surfaces.len(), 1);
}

#[test]
fn ruled_homogeneous_carriers_refuse_copied_poles_weights_and_controls() {
    let bytes = rational_ruled_surface_file();
    for operation in [
        "iges_surface_closure_weights",
        "iges_surface_closure_points",
        "iges_surface_closure_controls",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
    let service = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(service.ir().model.surfaces.len(), 1);
}

#[test]
fn aligned_ruled_spans_refuse_nested_split_and_partition_storage() {
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let second = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let spans = super::aligned_homogeneous_spans(None, &first, &second)
        .unwrap()
        .unwrap();
    assert_eq!(spans.len(), 2);
    for operation in [
        "Bezier knot copy",
        "Bezier internal knots",
        "Bezier spans",
        "Bezier span controls",
        "iges span normalized boundaries",
        "iges span combined boundaries",
        "iges span partition controls",
        "iges span split levels",
        "iges span split first controls",
        "iges span split level controls",
        "iges span split left controls",
        "iges span split right controls",
        "iges span partition slots",
        "iges span aligned pairs",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match super::aligned_homogeneous_spans(Some(&ctx), &first, &second) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => {
                    panic!("expected aligned-span refusal at {operation}, but alignment succeeded")
                }
                Err(error) => panic!("expected aligned-span refusal at {operation}: {error:?}"),
            }
        }
        assert!(found, "aligned-span refusal was not reached: {operation}");
    }
}

#[test]
fn unclamped_ruled_span_extraction_refuses_knot_insertion_storage() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![-1.0, 0.0, 1.0, 2.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let expected = super::homogeneous_bezier_spans(None, &curve)
        .unwrap()
        .unwrap();
    for operation in ["Bezier knot insertion", "Bezier inserted knot"] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..128 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match super::homogeneous_bezier_spans(Some(&ctx), &curve) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => panic!("span extraction succeeded before {operation}"),
                Err(error) => panic!("unexpected span refusal at {operation}: {error}"),
            }
        }
        assert!(
            found,
            "span insertion boundary was not reached: {operation}"
        );
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .unwrap();
    let actual = super::homogeneous_bezier_spans(Some(&ctx), &curve)
        .unwrap()
        .unwrap();
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.domain, expected.domain);
        assert_eq!(actual.controls, expected.controls);
    }
}

#[test]
fn same_basis_ruled_surface_refuses_nested_weight_rows() {
    let rail = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
    let weight = cadmpeg_ir::scalar::NonZeroReal::try_from(0.5).unwrap();
    let weights = [weight, weight];
    super::same_basis_ruled_surface(&rail, &rail, &weights, None).unwrap();
    for operation in [
        "iges ruled same-basis weight rows",
        "iges ruled same-basis weight row controls",
        "iges ruled same-basis weighted rows",
        "iges ruled same-basis weighted row controls",
    ] {
        let mut cap = 0_u64;
        let mut found = false;
        for _ in 0..4096 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            match super::same_basis_ruled_surface(&rail, &rail, &weights, Some(&ctx)) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    if limit.operation == operation {
                        found = true;
                        break;
                    }
                    cap = limit.used.checked_add(limit.additional).unwrap();
                }
                Ok(_) => {
                    panic!("expected same-basis refusal at {operation}, but construction succeeded")
                }
                Err(error) => panic!("expected same-basis refusal at {operation}: {error:?}"),
            }
        }
        assert!(found, "same-basis refusal was not reached: {operation}");
    }
}

#[test]
fn revolution_angular_basis_refuses_knot_and_control_lanes() {
    let bytes = surface_of_revolution_file();
    for operation in [
        "iges revolution angular knots",
        "iges revolution angular controls",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!service.ir().model.surfaces.is_empty());
}

#[test]
fn revolution_nurbs_carrier_refuses_nested_pole_and_weight_rows() {
    let bytes = ellipse_surface_of_revolution_file();
    for operation in [
        "iges revolution surface controls",
        "iges revolution surface weights",
        "iges revolution surface u knots",
        "iges revolution pole rows",
        "iges revolution pole row controls",
        "iges revolution weight rows",
        "iges revolution weight row controls",
        "iges revolution weighted rows",
        "iges revolution weighted row controls",
        "iges revolution neutral surface slots",
    ] {
        assert_surface_collection_refusal(&bytes, operation);
    }
    let service = crate::IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(!service.ir().model.surfaces.is_empty());
}
fn type128_surface_with_closure(
    global: &[u8],
    closed_u: i64,
    closed_v: i64,
    poles: &str,
) -> Vec<u8> {
    let parameters =
        format!("128,1,1,1,1,{closed_u},{closed_v},1,0,0,0,0,1,1,0,0,1,1,1,1,1,1,{poles},0,1,0,1;");
    owned_test_file_with_global_and_line_fonts(
        &[OwnedTestEntity {
            entity_type: 128,
            form: 0,
            label: "SURFACE".into(),
            status: "00000000",
            parameters,
        }],
        global,
        &[(1, 1)],
    )
}

#[test]
fn tabulated_directrix_types_follow_the_declared_dialect() {
    assert!(tabulated_directrix_type_allowed(102, 0, GlobalTable::V4_0));
    assert!(tabulated_directrix_type_allowed(112, 0, GlobalTable::V4_0));
    assert!(!tabulated_directrix_type_allowed(112, 1, GlobalTable::V4_0));
    assert!(!tabulated_directrix_type_allowed(112, 3, GlobalTable::V5_0));
    assert!(!tabulated_directrix_type_allowed(130, 0, GlobalTable::V4_0));
    assert!(tabulated_directrix_type_allowed(130, 0, GlobalTable::V5_0));
    assert!(tabulated_directrix_type_allowed(
        142,
        0,
        GlobalTable::V5Later
    ));
}

#[test]
fn type_140_indicator_parameters_use_bounded_midpoint_or_unbounded_origin() {
    fn bounds(raw: [Option<f64>; 4]) -> Option<cadmpeg_ir::geometry::RecordBounds> {
        cadmpeg_ir::geometry::RecordBounds::try_new(raw).ok()
    }
    assert_eq!(
        offset_indicator_parameters(bounds([Some(-2.0), Some(6.0), Some(4.0), Some(8.0)])),
        [2.0, 6.0]
    );
    assert_eq!(
        offset_indicator_parameters(bounds([Some(-2.0), Some(6.0), None, Some(8.0)])),
        [0.0, 0.0]
    );
    assert_eq!(offset_indicator_parameters(None), [0.0, 0.0]);
}

#[test]
fn decode_type_140_uses_the_bounded_support_midpoint_normal() {
    for indicator in [
        "-0.4082482904638631D0,-0.4082482904638631D0,0.8164965809277261D0",
        "0,0,1",
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(offset_nurbs_surface_file(indicator)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id.as_str() == "iges:model:surface#D1"));
        assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
        assert_eq!(result.report().losses.len(), 1);
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::EntityNotProjected.kind()
        );
    }
}

#[test]
fn decode_refuses_a_nurbs_surface_over_its_pole_limit() {
    let error = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 128,
                form: 0,
                label: "SURFACE".into(),
                status: "00000000",
                parameters: "128,1000,1000,1,1,0,0,0,0,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Codec("iges_surface_poles")
                && limit.limit == 1_000_000
                && limit.used == 1_000_000
                && limit.additional == 2_001
    ));
}

#[test]
fn angular_basis_canonicalizes_a_full_sweep_with_decimal_roundoff() {
    let basis = angular_basis(
        0.0,
        std::f64::consts::TAU + std::f64::consts::TAU * 5.0e-13,
        None,
    )
    .unwrap()
    .expect("a near-full finite sweep has an exact rational basis");

    assert_eq!(basis.controls.len(), 9);
    assert_eq!(basis.knots.last(), Some(&std::f64::consts::TAU));
}

#[test]
fn decode_solves_a_parameter_matched_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact NURBS ruled cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.25, 0.75)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(0.25, 0.75, 0.0))
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_projects_an_interval_certified_linear_bezier_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(interval_certified_linear_bezier_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .and_then(|surface| match surface.geometry.solved() {
            Some(SolvedSurfaceGeometry::Nurbs(surface)) => Some(surface),
            _ => None,
        })
        .expect("linear Bezier ruled surface");
    let midpoint = cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5)
        .expect("linear Bezier ruled midpoint");
    assert!(
        midpoint.distance(Point3::new(1.5, 0.5, 0.0)) <= EPS_LINEAR_BEZIER_RULED,
        "{midpoint:?}"
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());
}

#[test]
fn decode_reconciles_rational_ruled_rail_denominators_exactly() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(rational_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational ruled cache");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (4, 1));
    assert!(surface.weights().is_some());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    let curve_point = |sequence: u32, parameter: f64| {
        let curve = result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id.as_str() == format!("iges:model:curve#D{sequence}"))
            .expect("rail curve");
        cadmpeg_ir::eval::curve_point(&curve.geometry, parameter).expect("rail point")
    };
    for (u, v) in [(0.2, 0.25), (0.5, 0.5), (0.8, 0.75)] {
        let first = curve_point(1, u);
        let second = curve_point(3, u);
        let expected = Point3::new(
            (1.0 - v) * first.x + v * second.x,
            (1.0 - v) * first.y + v * second.y,
            (1.0 - v) * first.z + v * second.z,
        );
        let actual = cadmpeg_ir::eval::nurbs_surface_point(surface, u, v).expect("surface point");
        assert!(
            actual.distance(expected) <= EPS_RATIONAL_RULED,
            "{actual:?} vs {expected:?}"
        );
    }
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());
}

#[test]
fn homogeneous_ruled_carrier_aligns_relative_parameter_partitions_and_refuses_weight_limit() {
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("valid first rail");
    let second = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, 1.0, 0.0),
        ],
        Some(vec![1.0, 0.5, 1.0]),
        false,
    )
    .expect("valid second rail");
    let surface = super::ruled_surface_carrier(&first, &second, None)
        .expect("ruled lanes pair")
        .expect("relative-parameter rational ruled carrier");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::ruled_surface_carrier(&first, &second, Some(&ctx))
        .expect_err("two unit weights exceed one admitted collection item");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems
    ));
    assert_eq!((surface.u_degree(), surface.v_degree()), (3, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (4, 2));
    for (u, v) in [(0.2, 0.25), (0.6, 0.75), (0.9, 0.5)] {
        let first_point =
            cadmpeg_ir::eval::nurbs_curve_point_at(&first, u).expect("first rail point");
        let second_point =
            cadmpeg_ir::eval::nurbs_curve_point_at(&second, 2.0 * u).expect("second rail point");
        let expected = Point3::new(
            (1.0 - v) * first_point.x + v * second_point.x,
            (1.0 - v) * first_point.y + v * second_point.y,
            (1.0 - v) * first_point.z + v * second_point.z,
        );
        let actual =
            cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).expect("ruled surface point");
        assert!(actual.distance(expected) <= EPS_RATIONAL_RULED);
    }
}

#[test]
fn homogeneous_ruled_carrier_splits_mismatched_knot_partitions() {
    let first = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("valid first rail");
    let second = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        Some(vec![1.0, 0.5]),
        false,
    )
    .expect("valid second rail");
    let surface = super::ruled_surface_carrier(&first, &second, None)
        .expect("ruled lanes pair")
        .expect("partition-aligned rational ruled carrier");
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert_eq!((surface.u_count(), surface.v_count()), (5, 2));
    assert_eq!(
        surface.u_knots().as_slice(),
        [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0]
    );
    for (u, v) in [(0.25, 0.4), (0.75, 0.6)] {
        let first_point =
            cadmpeg_ir::eval::nurbs_curve_point_at(&first, u).expect("first rail point");
        let second_point =
            cadmpeg_ir::eval::nurbs_curve_point_at(&second, u).expect("second rail point");
        let expected = Point3::new(
            (1.0 - v) * first_point.x + v * second_point.x,
            (1.0 - v) * first_point.y + v * second_point.y,
            (1.0 - v) * first_point.z + v * second_point.z,
        );
        let actual =
            cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v).expect("ruled surface point");
        assert!(actual.distance(expected) <= EPS_RATIONAL_RULED);
    }
}

#[test]
fn decode_projects_rational_circular_arc_length_ruled_surface() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(circular_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact circular ruled cache");
    };
    assert_eq!((surface.u_degree(), surface.v_degree()), (2, 1));
    assert!(surface.weights().is_some());
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("entity type 118")
    }));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());
}

#[test]
fn decode_retains_both_ruled_surface_developability_values_in_native_parameters() {
    for developable_flag in [0, 1] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(ruled_surface_file_with_developable_flag(developable_flag)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let entity = result.ir().native.namespace("iges").unwrap().arenas()["entities"]
            .iter()
            .find(|entity| entity.id() == "iges:entity:directory#5")
            .unwrap();
        assert_eq!(
            entity.fields()["parameters"][4]["value"]["value"],
            developable_flag
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    }
}

#[test]
fn decode_projects_composite_ruled_and_tabulated_carriers() {
    let ruled = IgesCodec
        .decode(
            &mut Cursor::new(composite_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(ruled.ir().model.procedural_surfaces.len(), 1);
    assert!(ruled
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::RuledDevelopabilityNotTransferred.kind()));
    assert!(cadmpeg_ir::validate_neutral(ruled.ir(), Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());

    let tabulated = IgesCodec
        .decode(
            &mut Cursor::new(composite_tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(tabulated.ir().model.procedural_surfaces.len(), 1);
    assert!(
        tabulated.report().losses.is_empty(),
        "{:#?}",
        tabulated.report().losses
    );
    assert!(cadmpeg_ir::validate_neutral(tabulated.ir(), Vec::new())
        .expect("resource allocation did not fail")
        .is_ok());
}

#[test]
fn decode_solves_a_surface_of_revolution_as_rational_quadratic_spans() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.v_degree(), 2);
    assert_eq!(surface.pole_weights().unwrap().len(), 6);
    let point =
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, std::f64::consts::FRAC_PI_4).unwrap();
    let expected = 0.5_f64.sqrt();
    assert!((point.x - expected).abs() < 1.0e-12);
    assert!((point.y - expected).abs() < 1.0e-12);
    assert!((point.z - 1.0).abs() < 1.0e-12);
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_surface_of_revolution_from_an_ellipse_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ellipse_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational ellipse revolution cache");
    };
    let point = cadmpeg_ir::eval::nurbs_surface_point(
        surface,
        std::f64::consts::FRAC_PI_4,
        std::f64::consts::FRAC_PI_4,
    )
    .expect("ellipse revolution evaluates");
    assert!((point.x - 0.5).abs() < 1.0e-12);
    assert!((point.y - 1.5).abs() < 1.0e-12);
    assert!(point.z.abs() < 1.0e-12);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_surface_of_revolution_from_a_line_with_roundoff_endpoints() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .expect("line revolution fixture decodes");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .expect("line revolution surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } = &surface.geometry
    else {
        panic!("expected an exact construction-backed revolution");
    };
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)
        .expect("line revolution construction");
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload_0) =
        procedural.definition()
    else {
        panic!("expected an exact revolution definition");
    };
    let directrix = definition_payload_0.directrix();
    let Some(parameter_interval) = &definition_payload_0
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        panic!("expected an exact revolution definition");
    };
    assert_eq!(directrix.as_str(), "iges:model:curve#D3");
    assert!(matches!(
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("line generatrix")
            .geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
    ));
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    assert_eq!(
        procedural
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        Some([Some(0.0), Some(6.606_051_667_958_6), None, None])
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_uses_recovered_global_resolution_for_line_revolution_admission() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,64,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,2e-06.,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let result = IgesCodec
        .decode(
            &mut Cursor::new(line_surface_of_revolution_file_with_global(global)),
            &DecodeOptions::default(),
        )
        .expect("line revolution with recoverable Global syntax decodes");

    assert_eq!(result.ir().tolerances.linear.get(), 2e-6);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::GlobalNumericSyntaxRecovered.kind())
            .count(),
        1
    );
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
        "{:#?}",
        result.report().losses
    );
    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
}

#[test]
fn decode_solves_a_surface_of_revolution_from_an_exact_hyperbola_carrier() {
    const EPS_REVOLUTION_POINT: f64 = 1.0e-12;

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", hyperbola_surface_of_revolution_file()),
        (
            "4.0",
            hyperbola_surface_of_revolution_file_with_global(global_v4),
        ),
        (
            "5.0",
            hyperbola_surface_of_revolution_file_with_global(global_v5),
        ),
    ];
    for (version, bytes) in fixtures {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            version
        );

        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
            .expect("hyperbola revolution surface");
        let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } =
            &surface.geometry
        else {
            panic!("expected a construction-backed revolution surface");
        };
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .expect("hyperbola revolution construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact revolution definition");
        };
        let directrix = definition_payload_0.directrix();
        let Some(parameter_interval) = &definition_payload_0
            .parameter_interval()
            .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
        else {
            panic!("expected an exact revolution definition");
        };
        let angular_interval = definition_payload_0.angular_interval().endpoints();
        assert_eq!(directrix.as_str(), "iges:model:curve#D3");
        assert_eq!(angular_interval, [0.0, std::f64::consts::FRAC_PI_2]);
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("hyperbola directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_))
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let source_point = cadmpeg_ir::eval::curve_point(directrix_geometry, parameter)
            .expect("hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
        let quarter_turn = cadmpeg_ir::eval::model_surface_point_by_id(
            &index,
            &surface.id,
            parameter,
            std::f64::consts::FRAC_PI_2,
        )
        .expect("hyperbola revolution evaluates");
        let expected = Point3::new(-source_point.y, source_point.x, source_point.z);
        assert!(quarter_turn.distance(expected) < EPS_REVOLUTION_POINT);
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_projects_a_trimmed_revolution_at_an_intermediate_native_angle() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(
        result
            .ir()
            .model
            .faces
            .iter()
            .any(|face| face.id.as_str() == "iges:model:face#D13"),
        "losses={:#?}",
        result.report().losses
    );
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
        .expect("trimmed revolution support");
    assert!(matches!(
        surface.geometry.solved_cache(),
        Some(SolvedSurfaceGeometry::Nurbs(_))
    ));
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| {
            result.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
        })
        .expect("trimmed revolution construction");
    assert_eq!(
        procedural
            .record_bounds()
            .map(cadmpeg_ir::geometry::RecordBounds::get),
        Some([Some(0.0), Some(2.0), None, None])
    );
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload_0) =
        procedural.definition()
    else {
        panic!("expected bounded trimmed revolution");
    };
    let Some(parameter_interval) = &definition_payload_0
        .parameter_interval()
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        panic!("expected bounded trimmed revolution");
    };
    assert_eq!(*parameter_interval, [0.0, 1.0]);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_places_a_surface_of_revolution_and_its_procedural_carriers_once() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(placed_surface_of_revolution_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact rational revolution cache");
    };
    assert_eq!(surface.poles().first().copied().unwrap().x, 11.0);
    let procedural = &result.ir().model.procedural_surfaces[0];
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(definition_payload) =
        procedural.definition()
    else {
        panic!("expected a revolution definition");
    };
    let directrix = definition_payload.directrix();
    let axis_origin = definition_payload.axis_origin();
    assert_eq!(axis_origin.x, 10.0);
    assert_eq!(directrix.as_str(), "iges:model:curve#D7-placed-generatrix");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_solves_a_tabulated_cylinder_as_an_exact_extrusion() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(tabulated_cylinder_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    let Some(SolvedSurfaceGeometry::Nurbs(surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected an exact NURBS extrusion cache");
    };
    assert_eq!(
        cadmpeg_ir::eval::nurbs_surface_point(surface, 0.5, 0.5)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(0.5, 0.0, 1.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn decode_solves_a_tabulated_surface_from_a_type_142_model_carrier() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "MODEL".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 108,
                    form: 0,
                    label: "PLANE".into(),
                    status: "00010000",
                    parameters: "108,0,0,1,0,0,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "PCURVE".into(),
                    status: "00010500",
                    parameters: "106,1,5,0,0,0,1,0,1,1,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 142,
                    form: 0,
                    label: "CURVSRF".into(),
                    status: "00010000",
                    parameters: "142,0,3,5,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 122,
                    form: 0,
                    label: "TABULATE".into(),
                    status: "00000000",
                    parameters: "122,7,0,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| {
            result
                .ir()
                .model
                .procedural_surface_owner(&surface.id)
                .map(SurfaceId::as_str)
                == Some("iges:model:surface#D9")
        })
        .expect("Type 122 neutral carrier");
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload) =
        procedural.definition()
    else {
        panic!("expected an extrusion definition");
    };
    let directrix = definition_payload.directrix();
    assert_eq!(directrix.as_str(), "iges:model:curve#D1");
    assert!(
        result.report().losses.is_empty(),
        "{:?}",
        result.report().losses
    );
}

#[test]
fn decode_solves_a_tabulated_surface_from_an_exact_hyperbola_directrix() {
    const EPS_TABULATED_POINT: f64 = 1.0e-12;

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", tabulated_hyperbola_file()),
        ("4.0", tabulated_hyperbola_file_with_global(global_v4)),
        ("5.0", tabulated_hyperbola_file_with_global(global_v5)),
    ];
    for (version, bytes) in fixtures {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            version
        );
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D3")
            .expect("hyperbola tabulated surface");
        let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } =
            &surface.geometry
        else {
            panic!("expected a construction-backed tabulated surface");
        };
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .expect("hyperbola tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact extrusion definition");
        };
        let directrix = definition_payload_0.directrix();
        let Some(parameter_interval) = &definition_payload_0.parameter_interval() else {
            panic!("expected an exact extrusion definition");
        };
        let direction = definition_payload_0.direction();
        let Some(native_position) = &definition_payload_0.native_position() else {
            panic!("expected an exact extrusion definition");
        };
        assert_eq!(directrix.as_str(), "iges:model:curve#D1");
        assert_eq!(
            *native_position,
            Point3::new(3.086_161_269_630_487, 3.525_603_580_931_404, 2.0)
        );
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("hyperbola directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_))
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let directrix_point = cadmpeg_ir::eval::curve_point(directrix_geometry, parameter)
            .expect("hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
        let surface_point =
            cadmpeg_ir::eval::model_surface_point_by_id(&index, &surface.id, parameter, 1.0)
                .expect("hyperbola tabulated surface evaluates");
        assert!(
            surface_point.distance(directrix_point.translated(direction.get(), 1.0))
                < EPS_TABULATED_POINT
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_places_a_tabulated_surface_and_its_exact_directrix() {
    const EPS_PLACED_TABULATED_POINT: f64 = 1.0e-12;

    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", placed_tabulated_hyperbola_file()),
        (
            "4.0",
            placed_tabulated_hyperbola_file_with_global(global_v4),
        ),
        (
            "5.0",
            placed_tabulated_hyperbola_file_with_global(global_v5),
        ),
    ];
    for (version, bytes) in fixtures {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            version
        );
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
            .expect("placed tabulated surface");
        let cadmpeg_ir::geometry::SurfaceGeometry::Procedural { construction, .. } =
            &surface.geometry
        else {
            panic!("expected a construction-backed placed tabulated surface");
        };
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .expect("placed tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact placed extrusion definition");
        };
        let directrix = definition_payload_0.directrix();
        let Some(parameter_interval) = &definition_payload_0.parameter_interval() else {
            panic!("expected an exact placed extrusion definition");
        };
        let direction = definition_payload_0.direction();
        let Some(native_position) = &definition_payload_0.native_position() else {
            panic!("expected an exact placed extrusion definition");
        };
        assert_eq!(directrix.as_str(), "iges:model:curve#D5-placed-directrix");
        assert!(
            native_position.distance(Point3::new(
                13.086_161_269_630_487,
                23.525_603_580_931_404,
                32.0,
            )) < EPS_PLACED_TABULATED_POINT
        );
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("placed hyperbola directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_))
        ));
        let parameter = parameter_interval[0].midpoint(parameter_interval[1]);
        let directrix_point = cadmpeg_ir::eval::curve_point(directrix_geometry, parameter)
            .expect("placed hyperbola directrix evaluates");
        let index = cadmpeg_ir::index::ModelIndex::new(result.ir());
        let surface_point =
            cadmpeg_ir::eval::model_surface_point_by_id(&index, &surface.id, parameter, 1.0)
                .expect("placed hyperbola tabulated surface evaluates");
        assert!(
            surface_point.distance(directrix_point.translated(direction.get(), 1.0))
                < EPS_PLACED_TABULATED_POINT
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_places_a_nurbs_tabulated_surface_and_its_exact_directrix() {
    let global_v4 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;";
    let global_v5 = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;";
    let fixtures = [
        ("5.3", placed_tabulated_line_file()),
        ("4.0", placed_tabulated_line_file_with_global(global_v4)),
        ("5.0", placed_tabulated_line_file_with_global(global_v5)),
    ];
    for (version, bytes) in fixtures {
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.report().dialects().unwrap().primary().declared()["effective_version"],
            version
        );
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "iges:model:surface#D5")
            .expect("placed NURBS tabulated surface");
        assert!(matches!(
            surface.geometry.solved_cache(),
            Some(SolvedSurfaceGeometry::Nurbs(_))
        ));
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| {
                result.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
            })
            .expect("placed NURBS tabulated construction");
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(definition_payload_0) =
            procedural.definition()
        else {
            panic!("expected an exact placed NURBS extrusion definition");
        };
        let directrix = definition_payload_0.directrix();
        let direction = definition_payload_0.direction();
        let Some(native_position) = &definition_payload_0.native_position() else {
            panic!("expected an exact placed NURBS extrusion definition");
        };
        assert_eq!(directrix.as_str(), "iges:model:curve#D5-placed-directrix");
        assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0));
        assert_eq!(*native_position, Point3::new(10.0, 20.0, 32.0));
        let directrix_geometry = &result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *directrix)
            .expect("placed NURBS directrix")
            .geometry;
        assert!(matches!(
            directrix_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
        ));
        assert_eq!(
            cadmpeg_ir::eval::curve_point(directrix_geometry, 0.5)
                .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(Point3::new(10.5, 20.0, 30.0))
        );
        assert_eq!(
            cadmpeg_ir::eval::surface_point(&surface.geometry, 0.5, 0.5)
                .map(cadmpeg_ir::features::FinitePoint3::get),
            Ok(Point3::new(10.5, 20.0, 31.0))
        );
        assert!(
            result
                .report()
                .losses
                .iter()
                .all(|loss| loss.code != IgesLossCode::EntityNotProjected.kind()),
            "{:#?}",
            result.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
            .expect("resource allocation did not fail");
        assert!(validation.is_ok(), "{:#?}", validation.findings);
    }
}

#[test]
fn decode_projects_an_unbounded_plane_from_implicit_coefficients() {
    let result = IgesCodec
        .decode(&mut Cursor::new(plane_file()), &DecodeOptions::default())
        .unwrap();

    let Some(SolvedSurfaceGeometry::Plane(plane_surface)) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected a plane carrier");
    };
    let origin = plane_surface.origin();
    let normal = plane_surface.frame().axis().as_raw();
    let u_axis = plane_surface.frame().reference().as_raw();
    assert_eq!(*origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 2.0));
    assert_eq!(*normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(
        cadmpeg_ir::eval::surface_point(&result.ir().model.surfaces[0].geometry, 1.0, 3.0)
            .map(cadmpeg_ir::features::FinitePoint3::get),
        Ok(cadmpeg_ir::math::Point3::new(1.0, 3.0, 2.0))
    );
    assert!(result.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

mod projection;
