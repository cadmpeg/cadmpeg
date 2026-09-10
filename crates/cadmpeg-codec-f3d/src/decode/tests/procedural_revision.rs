// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::TargetRequest;
use std::io::{Cursor, Write};

use cadmpeg_asm::asm_header;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::*;
use crate::F3dCodec;

#[test]
fn generated_revision_exact_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    assert_revision_surface_round_trip(smbh, "exact");
}

#[test]
fn generated_revision_exact_surface_carries_two_unextended_intervals() {
    use cadmpeg_ir::geometry::{ExactSpline, ProceduralSurfaceDefinition};

    // Two distinct non-[0,1] unextended parameter intervals: U then V.
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        for value in [0.0, std::f64::consts::FRAC_PI_2, 0.5, 2.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        push_tagged_i64(surface, 0x15, 0);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision exact decode");
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Exact(definition_payload) = procedural.definition() else {
        panic!("expected exact definition");
    };
    let spline = definition_payload.spline();

    let ExactSpline::Revision { intervals, .. } = spline else {
        panic!("expected revision exact-spline layout")
    };
    assert_eq!(
        *intervals,
        [
            [Some(0.0), Some(std::f64::consts::FRAC_PI_2)],
            [Some(0.5), Some(2.0)],
        ]
    );
    assert_revision_surface_round_trip(smbh, "exact");
}

#[test]
fn generated_revision_loft_surface_carries_one_nonempty_wrap_interval() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SplineSurfaceParameters};

    // First wrap interval non-empty [0,1]; second reversed [1,0] = empty.
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        t_long(surface, 1);
        t_dbl(surface, 0.0);
        t_long(surface, 1);
        t_long(surface, 1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_ident(surface, "null_surface");
        t_ident(surface, "nullbs");
        surface.push(0x0b);
        t_long(surface, -1);
        t_long(surface, 213);
        t_long(surface, 1);
        t_long(surface, 1);
        for value in [0.0, 1.0, 0.25, 0.75, 0.5, 1.5] {
            t_dbl(surface, value);
        }
        surface.push(0x0b);
        t_ident(surface, "null_curve");
        t_long(surface, 0);
        t_long(surface, -1);
        t_long(surface, 0);
        for value in [0.0, 1.0, 1.0, 0.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 0);
        t_long(surface, 0);
        push_revision_surface_tail(surface);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision loft decode");
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    let ProceduralSurfaceDefinition::Loft(definition_payload) = procedural.definition() else {
        panic!("expected loft definition");
    };
    let parameters = definition_payload.parameters();

    assert_eq!(
        parameters,
        &SplineSurfaceParameters::RevisionRanges {
            intervals: [[Some(0.0), Some(1.0)], [Some(1.0), Some(0.0)]],
        }
    );
    assert_revision_surface_round_trip(smbh, "loft");
}

#[test]
fn generated_revision_sum_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sum_spl_sur", |surface| {
        for (lower, upper) in [(0.0, 1.0), (-2.0, 2.0)] {
            surface.extend_from_slice(&generated_curve_block());
            surface.push(0x0a);
            t_dbl(surface, lower);
            surface.push(0x0a);
            t_dbl(surface, upper);
        }
        t_pos(surface, [1.0, 2.0, 3.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sum");
}

#[test]
fn generated_revision_rot_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("rot_spl_sur", |surface| {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "revolution");
}

#[test]
fn generated_revision_t_spline_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("t_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        for value in [0.0, 1.0, 0.0, 1.0] {
            surface.push(0x0a);
            t_dbl(surface, value);
        }
        push_tagged_i64(surface, 0x15, 0);
        surface.push(0x0f);
        t_ident(surface, "t_spl_subtrans_object");
        t_u16_string(
            surface,
            "degree 3\nunits mm\nv 1 0 0 0\nv 2 1 0 0\ne 1 1 2\n",
        );
        surface.push(0x0b);
        t_u16_string(surface, "100verts 1 2\n");
        surface.push(0x10);
        t_long(surface, 2);
    });
    assert_revision_surface_round_trip(smbh, "t_spline");
}

#[test]
fn generated_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh, "revision_g2_blend");
}

#[test]
fn generated_parameterized_revision_g2_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("g2_blend_spl_sur", |surface| {
        t_dbl(surface, 1.0);
        t_dbl(surface, 1.0);
        append_generated_variable_blend_side(surface, "left", 1.0);
        append_generated_variable_blend_side(surface, "right", 4.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.5);
        surface.push(0x0a);
        t_dbl(surface, 2.5);
        t_dbl(surface, 0.125);
        t_dbl(surface, 0.125);
        push_tagged_i64(surface, 0x15, -1);
        surface.extend_from_slice(&[0x0b; 4]);
        t_long(surface, 1);
        t_dbl(surface, 0.001);
        t_dbl(surface, 0.0001);
        t_long(surface, 1);
        push_parameterized_revision_surface_tail(surface);
        for value in [0, 0, 0] {
            t_long(surface, value);
        }
    });
    assert_revision_surface_round_trip(smbh.clone(), "revision_g2_blend");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision g2 blend decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RevisionG2Blend { construction } =
        procedural.definition()
    else {
        panic!("expected a revision g2 blend construction")
    };
    assert_parameterized_tail(&construction.cache);
}

#[test]
fn generated_revision_vertex_blend_round_trips() {
    let smbh = synthetic_revision_surface_smbh("VBL_SURF", |surface| {
        t_long(surface, 2);

        t_ident(surface, "circle");
        surface.push(0x0a);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.1);
        surface.push(0x0a);
        t_dbl(surface, 0.9);
        push_tagged_i64(surface, 0x15, 3);
        t_vec(surface, [0.0, 0.0, 0.5]);
        t_vec(surface, [0.5, 0.0, 0.0]);
        t_dbl(surface, 0.1);
        t_dbl(surface, 0.9);
        surface.push(0x0b);

        t_ident(surface, "pcurve");
        surface.push(0x0b);
        t_vec(surface, [0.0, 0.0, 0.0]);
        surface.push(0x0a);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        t_ident(surface, "plane");
        t_pos(surface, [0.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        surface.push(0x0b);
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_pcurve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.002);

        t_long(surface, 9);
        t_dbl(surface, 0.003);
    });
    assert_revision_surface_round_trip(smbh, "vertex_blend");
}

#[test]
fn generated_revision_offset_with_inline_untyped_support_decodes() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.push(0x0b);
        surface.push(0x0f);
        t_ident(surface, "mystery_spl_sur");
        t_long(surface, 23100);
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x10);
        surface.extend_from_slice(&[0x0b; 4]);
        t_dbl(surface, 0.3);
        surface.extend_from_slice(&[0x0b; 4]);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "offset");
}

#[test]
fn generated_single_radius_variable_blend_decodes_explicit_circular_cross_section() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                "srf_srf_v_bl_spl_sur",
                false,
                Some(0),
                [None, None],
            ))),
            &DecodeOptions::default(),
        )
        .expect("single-radius selector-zero decode");
    let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected variable blend")
    };
    let construction = definition_payload.construction();

    assert!(matches!(
        &construction.cross_section,
        Some(cadmpeg_ir::geometry::VariableBlendCrossSection::Circular)
    ));
    let expected = construction.clone();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("selector-zero source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("selector-zero round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::VariableBlend(definition_payload) if matches!((definition_payload.construction(),), (construction,) if construction == &expected)));
}

#[test]
fn generated_variable_blend_round_trips_parameterized_cross_sections() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, VariableBlendCrossSection};

    for (selector, expected_cross_section) in [
        (
            1,
            VariableBlendCrossSection::Thumbweights {
                parameters: [2.0, 2.0],
            },
        ),
        (
            7,
            VariableBlendCrossSection::G2Round {
                parameters: [2.0, 2.0],
            },
        ),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                    "srf_srf_v_bl_spl_sur",
                    false,
                    Some(selector),
                    [None, None],
                ))),
                &DecodeOptions::default(),
            )
            .expect("parameterized cross-section decode");
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected variable blend")
        };
        let construction = definition_payload.construction();

        assert_eq!(
            construction.cross_section.as_ref(),
            Some(&expected_cross_section)
        );

        let expected = construction.clone();
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("parameterized cross-section source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("parameterized cross-section round trip");
        assert!(matches!(
            &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::VariableBlend(definition_payload) if matches!((definition_payload.construction(),), (construction,) if construction == &expected)));
    }
}

#[test]
fn generated_variable_blend_round_trips_unclassified_bare_cross_sections() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, VariableBlendBareCrossSection, VariableBlendCrossSection,
    };

    for (selector, expected) in [
        (2, VariableBlendBareCrossSection::Selector2),
        (4, VariableBlendBareCrossSection::Selector4),
        (5, VariableBlendBareCrossSection::Selector5),
        (6, VariableBlendBareCrossSection::Selector6),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_variable_blend_smbh_with_selector(
                    "srf_srf_v_bl_spl_sur",
                    false,
                    Some(selector),
                    [None, None],
                ))),
                &DecodeOptions::default(),
            )
            .expect("bare cross-section decode");
        let ProceduralSurfaceDefinition::VariableBlend(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected variable blend")
        };
        let construction = definition_payload.construction();

        assert_eq!(
            construction.cross_section,
            Some(VariableBlendCrossSection::UnclassifiedBare { selector: expected })
        );

        let expected_construction = construction.clone();
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("bare cross-section source-less encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("bare cross-section round trip");
        assert!(matches!(
            &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::VariableBlend(definition_payload) if matches!((definition_payload.construction(),), (construction,) if construction == &expected_construction)));
    }
}

#[test]
fn generated_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_parameterized_revision_compound_loft_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_parameterized_revision_surface_tail(surface);
        push_revision_cl_scale(surface, true);
        t_long(surface, 2);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 0.0);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0b);
        surface.push(0x0b);
    });
    assert_revision_surface_round_trip(smbh.clone(), "revision_compound_loft");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision compound loft decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } =
        procedural.definition()
    else {
        panic!("expected a revision compound loft construction")
    };
    assert_parameterized_tail(&construction.cache);
}

#[test]
fn generated_revision_compound_loft_trailing_curve_round_trips() {
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.extend_from_slice(&generated_curve_block());
    });
    assert_revision_surface_round_trip(smbh, "revision_compound_loft");
}

#[test]
fn generated_revision_compound_loft_rejects_present_parameters_without_a_curve() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    // The trailing curve is present exactly when both parameter values are
    // present, so a payload carrying two present values and closing straight
    // away is not a legal record. The decoder reads the curve on the parameter
    // pair alone; it does not look ahead for the subtype-close byte.
    let smbh = synthetic_revision_surface_smbh("cl_loft_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_revision_cl_scale(surface, false);
        t_long(surface, 1);
        push_revision_cl_scale(surface, false);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        surface.push(0x0b);
        surface.push(0x0b);
        t_long(surface, 0);
        t_vec(surface, [0.0, 0.0, 1.0]);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("decode retains the record as a native unknown");
    assert!(!decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            ProceduralSurfaceDefinition::RevisionCompoundLoft { .. }
        )));

    let legal = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_revision_surface_smbh(
                "cl_loft_spl_sur",
                |surface| {
                    push_revision_surface_tail(surface);
                    push_revision_cl_scale(surface, false);
                    t_long(surface, 1);
                    push_revision_cl_scale(surface, false);
                    t_dbl(surface, 1.0);
                    surface.push(0x0b);
                    surface.push(0x0b);
                    t_long(surface, 0);
                    surface.push(0x0b);
                    surface.push(0x0b);
                    t_long(surface, 0);
                    t_vec(surface, [0.0, 0.0, 1.0]);
                    surface.push(0x0a);
                    t_dbl(surface, 1.0);
                    surface.push(0x0a);
                    t_dbl(surface, 0.0);
                    surface.extend_from_slice(&generated_curve_block());
                },
            ))),
            &DecodeOptions::default(),
        )
        .expect("legal revision compound loft decode")
        .into_parts()
        .0;
    let mut wire = serde_json::to_value(&legal.model.procedural_surfaces[0]).unwrap();
    assert_eq!(wire["definition"]["construction"]["tail"]["kind"], "curve");
    wire["definition"]["construction"]["tail"]
        .as_object_mut()
        .unwrap()
        .remove("curve");
    let error =
        serde_json::from_value::<cadmpeg_ir::geometry::ProceduralSurface>(wire).unwrap_err();
    assert!(error.to_string().contains("curve"), "{error}");
}

#[test]
fn decode_carries_the_document_modeling_length_unit_into_source_metadata() {
    // The `Custom` system's `modelingLengthName` is the document's display
    // length unit. It reaches `SourceMeta`, not `CadIr::units`: no stored
    // quantity depends on it, and model-space coordinates stay centimetres
    // under every value.
    let design = crate::design::decode::units::tests::stream([
        "centimeter",
        "millimeter",
        "meter",
        "inch",
        "foot",
        "inch",
    ]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_geometry_smbh()).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&design).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("decode with a unit-systems design stream");
    assert_eq!(
        result
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("modeling_length_unit"))
            .map(String::as_str),
        Some("inch")
    );
}

#[test]
fn record_level_surface_bounds_round_trip() {
    let smbh = synthetic_revision_surface_smbh("exact_spl_sur", |surface| {
        push_revision_surface_tail(surface);
        push_optional_value_quartet(surface);
        push_tagged_i64(surface, 0x15, 0);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("exact revision decode");
    let (mut source_less, _, _) = decoded.into_parts();
    assert_eq!(source_less.model.procedural_surfaces[0].record_bounds, None);
    source_less.model.procedural_surfaces[0].record_bounds =
        Some([Some(0.1), None, Some(0.2), None]);
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("record-bounds encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("record-bounds round trip");
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].record_bounds,
        Some([Some(0.1), None, Some(0.2), None])
    );
}

#[test]
fn generated_vertex_blends_decode_all_boundary_variants() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, SurfaceGeometry, VertexBlendBoundaryGeometry,
    };

    for name in ["VBL_SURF", "vertexblendsur"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_vertex_blend_smbh(name))),
                &DecodeOptions::default(),
            )
            .expect("vertex-blend decode");
        let ProceduralSurfaceDefinition::VertexBlend(definition_payload) =
            &result.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected vertex blend")
        };
        let construction = definition_payload.construction();

        let owner = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                result
                    .ir()
                    .model
                    .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
                    == Some(&surface.id)
            })
            .expect("vertex-blend owner");
        assert!(
            matches!(
                owner.geometry,
                SurfaceGeometry::Procedural { ref construction, .. }
                    if *construction == result.ir().model.procedural_surfaces[0].id
            ),
            "unexpected vertex-blend carrier: {:?}",
            owner.geometry
        );
        assert_eq!(construction.boundaries.len(), 4);
        assert_eq!(construction.grid_size, 17);
        assert_eq!(construction.fit_tolerance.get(), 0.03);
        let VertexBlendBoundaryGeometry::Circle {
            twists,
            parameters,
            sense,
            ..
        } = &construction.boundaries[0].geometry
        else {
            panic!("expected circle boundary")
        };
        assert_eq!(twists.form(), 1);
        assert_eq!(
            twists.entries(),
            &[cadmpeg_ir::math::Point3::new(20.0, 30.0, 40.0)]
        );
        assert_eq!(*parameters, [0.1, 0.9]);
        assert!(!*sense);
        assert!(matches!(
            construction.boundaries[1].geometry,
            VertexBlendBoundaryGeometry::Degenerate { .. }
        ));
        assert!(matches!(
            construction.boundaries[2].geometry,
            VertexBlendBoundaryGeometry::Pcurve {
                pcurve: Some(_),
                ..
            }
        ));
        assert!(matches!(
            construction.boundaries[3].geometry,
            VertexBlendBoundaryGeometry::Plane { .. }
        ));
        let bounded_curves =
            [0usize, 3].map(|ordinal| match &construction.boundaries[ordinal].geometry {
                VertexBlendBoundaryGeometry::Circle {
                    curve, parameters, ..
                }
                | VertexBlendBoundaryGeometry::Plane {
                    curve, parameters, ..
                } => (curve.clone(), *parameters),
                _ => unreachable!(),
            });

        let expected = construction.clone();
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        for (ordinal, (curve, _)) in bounded_curves.iter().enumerate() {
            source_less
                .model
                .curves
                .iter_mut()
                .find(|candidate| candidate.id == *curve)
                .expect("vertex-blend boundary curve")
                .geometry = cadmpeg_ir::geometry::CurveGeometry::Line(
                cadmpeg_ir::geometry::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(ordinal as f64, 2.0, -3.0),
                    cadmpeg_ir::math::Vector3::new(2.0, -1.0, 4.0)
                        .unit()
                        .unwrap(),
                )
                .unwrap(),
            );
        }
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less vertex-blend encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less vertex-blend round trip");
        let ProceduralSurfaceDefinition::VertexBlend(definition_payload) =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip vertex blend")
        };
        let actual = definition_payload.construction();

        assert_eq!(actual, &expected);
        for (curve, range) in bounded_curves {
            assert!(matches!(
                round_trip
                    .ir()
                    .model
                    .curves
                    .iter()
                    .find(|candidate| candidate.id == curve)
                    .map(|curve| &curve.geometry),
                Some(cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve))
                    if curve.degree() == 1
                        && curve.knots() == [range[0], range[0], range[1], range[1]]
            ));
        }
    }
}

#[test]
fn decode_retains_generated_translational_extrusion_and_fit_contract() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.02));
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) = procedural.definition() else {
        panic!("expected extrusion")
    };
    let direction = definition_payload.direction();
    let directrix = definition_payload.directrix();
    let parameter_interval = definition_payload.parameter_interval();
    let native_position = definition_payload.native_position();
    let None = definition_payload.revision_form() else {
        panic!("expected extrusion")
    };
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(parameter_interval, Some([0.25, 0.75]));
    assert_eq!(
        native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
    let directrix = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .expect("extrusion directrix carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(directrix) = &directrix.geometry else {
        panic!("expected NURBS directrix")
    };
    assert_eq!(directrix.control_points().len(), 3);
}

#[test]
fn decode_retains_versioned_nested_translational_extrusion() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_versioned_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("versioned extrusion decode");
    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.02));
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) = procedural.definition() else {
        panic!("expected versioned extrusion")
    };
    let direction = definition_payload.direction();
    let parameter_interval = definition_payload.parameter_interval();
    let native_position = definition_payload.native_position();
    assert_eq!(parameter_interval, Some([0.25, 0.75]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 20.0));
    assert_eq!(
        native_position,
        Some(cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0))
    );
}

#[test]
fn revision_cylinder_rejects_tokens_after_its_terminal_surface_tail() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_versioned_cyl_spl_sur_with_trailing_token_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("opaque revision-cylinder decode");

    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .any(|surface| matches!(
            surface.definition(),
            ProceduralSurfaceDefinition::Unknown { .. }
        )));
}

#[test]
fn generated_f3d_rewrites_translational_extrusion_header() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_surfaces[0]
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Extrusion(definition_payload) = definition else {
                panic!("expected extrusion")
            };
            let mut parameter_interval_value = definition_payload.parameter_interval();
            let parameter_interval = &mut parameter_interval_value;
            let mut direction_value = *definition_payload.direction();
            let direction = &mut direction_value;
            let mut native_position_value = definition_payload.native_position();
            let native_position = &mut native_position_value;
            {
                *parameter_interval = Some([-0.5, 1.25]);
                *direction = cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0);
                *native_position = Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0));
            };
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                    definition_payload.directrix().clone(),
                    parameter_interval_value,
                    direction_value,
                    native_position_value,
                    definition_payload.revision_form().clone(),
                )
                .unwrap();
        })
        .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("extrusion-direction regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip extrusion")
    };
    let parameter_interval = definition_payload.parameter_interval();
    let direction = definition_payload.direction();
    let native_position = definition_payload.native_position();
    assert_eq!(parameter_interval, Some([-0.5, 1.25]));
    assert_eq!(*direction, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 30.0));
    assert_eq!(
        native_position,
        Some(cadmpeg_ir::math::Point3::new(-20.0, 70.0, 15.0))
    );
}

#[test]
fn generated_f3d_rewrites_procedural_surface_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-surface decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(Some(0.075))
        .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("procedural-surface fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-surface decode");
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].cache_fit_tolerance(),
        Some(0.075)
    );
}

#[test]
fn generated_f3d_rewrites_nurbs_surface_control_grid() {
    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated NURBS surface decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.geometry.solved_cache(),
                Some(cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(_))
            )
        })
        .expect("generated NURBS surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
        cache: Some(cache), ..
    } = &mut surface.geometry
    else {
        panic!("procedural carrier with a solved cache")
    };
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(mut nurbs) = cache.as_geometry().clone()
    else {
        unreachable!()
    };
    nurbs
        .edit_control_points(|points| {
            points[2].x = 17.5;
            points[2].z = -3.25;
        })
        .unwrap();
    nurbs
        .edit_u_knots(|knots| knots.copy_from_slice(&[-1.0, -1.0, 2.0, 2.0]))
        .unwrap();
    nurbs
        .edit_v_knots(|knots| knots.copy_from_slice(&[-0.5, -0.5, 1.5, 1.5]))
        .unwrap();
    nurbs.set_u_periodic(true);
    *cache = cadmpeg_ir::geometry::SolvedSurfaceGeometry::new(
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs.clone()),
    )
    .unwrap();
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("NURBS surface regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated NURBS surface decode");
    let surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip NURBS surface");
    assert_eq!(
        *surface.geometry.solved_cache().expect("solved NURBS cache"),
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_rational_nurbs_surface_weights() {
    let source = f3d_with_smbh(&synthetic_rational_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational surface decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| {
            matches!(
                surface.geometry.solved_cache(),
                Some(cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs))
                    if nurbs.weights().is_some()
            )
        })
        .expect("generated rational surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Procedural {
        cache: Some(cache), ..
    } = &mut surface.geometry
    else {
        panic!("procedural carrier with a solved cache")
    };
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(mut nurbs) = cache.as_geometry().clone()
    else {
        unreachable!()
    };
    nurbs.edit_weights(|weights| weights[1] = 0.65).unwrap();
    *cache = cadmpeg_ir::geometry::SolvedSurfaceGeometry::new(
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs.clone()),
    )
    .unwrap();
    let expected = nurbs.clone();
    let surface_id = surface.id.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("rational-weight regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational surface decode");
    let surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == surface_id)
        .expect("round-trip rational surface");
    assert_eq!(
        *surface.geometry.solved_cache().expect("solved NURBS cache"),
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(expected)
    );
}

#[test]
fn generated_f3d_rewrites_extrusion_directrix_control_points() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_cyl_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated extrusion decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let ProceduralSurfaceDefinition::Extrusion(definition_payload) =
        edited.model.procedural_surfaces[0].definition()
    else {
        panic!("expected extrusion")
    };
    let directrix = definition_payload.directrix();
    let directrix_id = directrix.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == directrix_id)
        .expect("extrusion directrix");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS directrix")
    };
    let mut control_points = nurbs.control_points().to_vec();
    control_points[1].y = 12.5;
    control_points[1].z = -2.0;
    *nurbs = cadmpeg_ir::geometry::NurbsCurve::new(
        1,
        vec![-2.0, -2.0, 3.0, 3.0, 3.0],
        control_points,
        nurbs.weights().map(<[f64]>::to_vec),
        true,
    )
    .unwrap();
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("extrusion-directrix regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated extrusion decode");
    let curve = round_trip
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == directrix_id)
        .expect("round-trip directrix");
    assert_eq!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(expected)
    );
}

#[test]
fn decode_resolves_generated_ref_translational_extrusion() {
    let f3d = f3d_with_smbh(&synthetic_ref_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir().model.procedural_surfaces[0].cache_fit_tolerance(),
        Some(0.02)
    );
}

#[test]
fn decode_resolves_revision_extrusion_implicit_directrix_reference() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let f3d = f3d_with_smbh(&synthetic_revision_ref_directrix_cyl_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.procedural_surfaces.len(), 1);
    assert!(matches!(
        result.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::Extrusion(_)
    ));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == F3dLossCode::SurfaceShapeNotDecoded.kind() }));
}

#[test]
fn decode_retains_generated_rolling_ball_definition() {
    use cadmpeg_ir::geometry::{BlendCrossSection, BlendRadiusLaw, ProceduralSurfaceDefinition};

    let f3d = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    assert_eq!(procedural.cache_fit_tolerance(), Some(0.01));
    let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() else {
        panic!("expected rolling-ball blend")
    };
    let supports = definition_payload.supports();
    let spine = definition_payload.spine();
    let radius = definition_payload.radius();
    let cross_section = definition_payload.cross_section();

    assert!(supports.iter().all(Option::is_some));
    assert!(supports.iter().flatten().all(|support| result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id == support.surface)));
    let spine = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == spine.as_ref())
        .expect("blend spine carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(spine) = &spine.geometry else {
        panic!("expected NURBS blend spine")
    };
    assert_eq!(spine.control_points().len(), 3);
    assert_eq!(cross_section, &BlendCrossSection::Circular);
    assert_eq!(
        radius,
        &BlendRadiusLaw::Constant {
            signed_radius: -3.0
        }
    );
}

#[test]
fn generated_solved_plane_plane_blend_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{
        BlendRadiusLaw, CurveGeometry, NurbsCurve, ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated rolling-ball decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let (support_ids, spine_id) = source_less.model.procedural_surfaces[0]
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
                panic!("expected rolling-ball definition")
            };
            let mut edited_supports = definition_payload.supports().clone();
            let mut edited_spine = definition_payload.spine().clone();
            let mut edited_radius = definition_payload.radius().clone();
            let (supports, Some(spine), radius) =
                (&mut edited_supports, &mut edited_spine, &mut edited_radius)
            else {
                panic!("expected rolling-ball definition")
            };

            let support_ids = [
                supports[0].as_ref().expect("first support").surface.clone(),
                supports[1]
                    .as_ref()
                    .expect("second support")
                    .surface
                    .clone(),
            ];
            let spine_id = spine.clone();
            *radius = BlendRadiusLaw::Constant {
                signed_radius: -2.0,
            };
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    edited_supports,
                    edited_spine,
                    edited_radius,
                    definition_payload.cross_section().clone(),
                    definition_payload.native().clone(),
                )
                .unwrap();
            (support_ids, spine_id)
        })
        .unwrap();
    let support_geometry = [
        SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        ),
        SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
    ];
    for (id, geometry) in support_ids.into_iter().zip(support_geometry) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == id)
            .expect("rolling-ball support")
            .geometry = geometry;
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("rolling-ball spine")
        .geometry = CurveGeometry::Nurbs(
        NurbsCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(2.0, 2.0, -4.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(2.0, 2.0, 7.0),
            ],
            None,
            false,
        )
        .unwrap(),
    );

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    let carrier_id = round_trip
        .ir()
        .model
        .procedural_surface_owner(&round_trip.ir().model.procedural_surfaces[0].id)
        .expect("rolling-ball carrier");
    assert!(matches!(round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| &surface.id == carrier_id)
        .expect("rolling-ball carrier")
        .geometry
        .solved_cache()
        .expect("solved rolling-ball cache"), SurfaceGeometry::Cylinder(cylinder_surface)
            if {
                let origin = cylinder_surface.origin();
    let axis = cylinder_surface.axis();
    let radius = cylinder_surface.radius();
                *origin == Point3::new(2.0, 2.0, -4.0)
                    && *axis == Vector3::new(0.0, 0.0, 1.0)
                    && radius == 2.0
            }));
}

#[test]
fn generated_rolling_ball_surface_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rbblnsur", "pipe_spl_sur", "pipesur"] {
        let bytes =
            with_legacy_subtype(synthetic_rb_blend_spl_sur_smbh(), "rb_blend_spl_sur", name);
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball alias decode");
        assert!(matches!(
            result.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Blend(..)
        ));
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less.set_native_unknowns("f3d", &[]).unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("canonical rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical rolling-ball round trip");
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Blend(..)
        ));
    }
}

#[test]
fn generated_f3d_rewrites_rolling_ball_radius_law() {
    use cadmpeg_ir::geometry::{BlendRadiusLaw, ProceduralSurfaceDefinition};

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_surfaces[0]
        .edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
                panic!("expected rolling-ball blend")
            };
            let mut edited_radius = definition_payload.radius().clone();
            let radius = &mut edited_radius;

            *radius = BlendRadiusLaw::Linear {
                start: -2.0,
                end: -4.0,
            };
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    definition_payload.supports().clone(),
                    definition_payload.spine().clone(),
                    edited_radius,
                    definition_payload.cross_section().clone(),
                    definition_payload.native().clone(),
                )
                .unwrap();
        })
        .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("rolling-ball radius regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip rolling-ball blend")
    };
    let radius = definition_payload.radius();

    assert_eq!(
        radius,
        &BlendRadiusLaw::Linear {
            start: -2.0,
            end: -4.0,
        }
    );
}

#[test]
fn generated_f3d_rewrites_rolling_ball_spine_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        edited.model.procedural_surfaces[0].definition()
    else {
        panic!("expected rolling-ball spine")
    };
    let (Some(spine),) = (definition_payload.spine(),) else {
        panic!("expected rolling-ball spine")
    };

    let spine_id = spine.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("blend spine curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("expected NURBS blend spine")
    };
    let mut control_points = nurbs.control_points().to_vec();
    control_points[1].x = 8.0;
    control_points[1].y = -6.0;
    *nurbs = cadmpeg_ir::geometry::NurbsCurve::new(
        1,
        vec![-1.0, -1.0, 2.0, 2.0, 2.0],
        control_points,
        nurbs.weights().map(<[f64]>::to_vec),
        nurbs.periodic(),
    )
    .unwrap();
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("blend-spine regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn generated_f3d_rewrites_rolling_ball_support_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        edited.model.procedural_surfaces[0].definition()
    else {
        panic!("expected rolling-ball blend")
    };
    let supports = definition_payload.supports();

    let support_id = supports[0]
        .as_ref()
        .expect("first blend support")
        .surface
        .clone();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == support_id)
        .expect("blend support surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &mut surface.geometry else {
        panic!("expected NURBS blend support")
    };
    nurbs
        .edit_control_points(|points| {
            points[1].x = 6.0;
            points[1].z = 4.0;
        })
        .unwrap();
    nurbs
        .edit_u_knots(|knots| knots.copy_from_slice(&[-1.0, -1.0, 2.0, 2.0]))
        .unwrap();
    let expected = surface.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("blend-support regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface == &expected));
}

#[test]
fn decode_reports_generated_partial_rolling_ball_supports() {
    let f3d = f3d_with_smbh(&synthetic_partial_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("only one of two native supports resolved")));
}

#[test]
fn subtype_reference_resolves_surface_cache() {
    let mut target = Vec::new();
    target.extend_from_slice(b"\x0f\x0d\x07surface");
    // A payload byte equal to SUBTYPE_CLOSE must not terminate the span.
    target.push(0x06);
    target.extend_from_slice(&[0x10, 0, 0, 0, 0, 0, 0, 0]);
    target.extend_from_slice(&generated_surface_block());
    target.push(0x10);

    let mut source = Vec::new();
    source.extend_from_slice(b"\x0f\x0d\x03ref\x04");
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);

    let mut active = target;
    active.extend_from_slice(&source);
    let decoded = cadmpeg_asm::nurbs::core::surface_cache_resolving_refs(
        &cadmpeg_asm::nurbs::toks::lex_test_span(
            &source,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        ),
        &cadmpeg_asm::nurbs::toks::test_table(&active, cadmpeg_asm::kernel_header::RefWidth::Eight),
    )
    .expect("subtype-table reference resolves to its surface cache");
    assert_eq!((decoded.u_count(), decoded.v_count()), (2, 2));
}

#[test]
fn a_form_two_par_int_cur_decodes_as_its_support_isoline() {
    use cadmpeg_asm::nurbs::proc_curve::decode_par_int_cur_isoline;
    use cadmpeg_ir::math::Point3;

    // The support is the unit bilinear patch scaled to millimetres, so the
    // isoline at u = 1 is the patch's far edge.
    let scope = generated_form_two_par_int_cur([1.0, 0.0], [1.0, 1.0]);
    let curve =
        decode_par_int_cur_isoline(&scope, cadmpeg_asm::kernel_header::RefWidth::Eight, None)
            .expect("form-2 isoline");
    assert_eq!(curve.degree(), 1);
    assert_eq!(curve.knots(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        curve.control_points(),
        [Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)]
    );

    // A pcurve that crosses the support holds neither parameter fixed, so no
    // NURBS curve reproduces it and the form is refused.
    let diagonal = generated_form_two_par_int_cur([0.0, 0.0], [1.0, 1.0]);
    assert!(decode_par_int_cur_isoline(
        &diagonal,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        None
    )
    .is_none());

    // A pcurve running only part of the support's domain would need a trim.
    let partial = generated_form_two_par_int_cur([1.0, 0.0], [1.0, 0.5]);
    assert!(decode_par_int_cur_isoline(
        &partial,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
        None
    )
    .is_none());
}

#[test]
fn a_nested_construction_cache_is_not_the_enclosing_scope_cache() {
    use cadmpeg_asm::nurbs::core::{decode_curve_cache, decode_owned_curve_cache_at};

    // A `par_int_cur` whose cache slot is `nullbs` and whose support is an
    // intcurve construction carrying a curve block of its own.
    let mut scope = vec![0x0f];
    t_ident(&mut scope, "par_int_cur");
    scope.push(0x0f);
    t_ident(&mut scope, "exact_int_cur");
    scope.extend_from_slice(&generated_curve_block());
    scope.push(0x10);
    t_ident(&mut scope, "nullbs");
    scope.push(0x10);

    assert!(decode_curve_cache(&scope).is_some());
    assert!(
        decode_owned_curve_cache_at(&scope, cadmpeg_asm::kernel_header::RefWidth::Eight).is_none()
    );
}

#[test]
fn a_nested_construction_does_not_claim_its_enclosing_record() {
    use cadmpeg_asm::nurbs::proc_surface::{
        procedural_surface_resolving_refs, DecodedProceduralSurfaceDefinition,
    };

    let bytes = synthetic_cyl_spl_sur_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &records[9];
    let owned = bytes[record.offset..record.offset + record.len].to_vec();
    let decoded = procedural_surface_resolving_refs(
        &record.tokens,
        &cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(std::slice::from_ref(record)),
    )
    .expect("the record owns its extrusion");
    assert!(matches!(
        decoded.definition(),
        DecodedProceduralSurfaceDefinition::Extrusion { .. }
    ));

    // The same extrusion nested inside a variable-blend scope is that blend's
    // support surface, not the record's own surface.
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let at = owned
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let mut nested = owned.clone();
    nested.splice(at..at, *b"\x0f\x0d\x14srf_srf_v_bl_spl_sur");
    let terminator = nested.len() - 1;
    nested.insert(terminator, 0x10);
    let nested_records = cadmpeg_asm::sab::frame(
        &nested,
        0,
        nested.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    assert!(procedural_surface_resolving_refs(
        &nested_records[0].tokens,
        &cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(&nested_records),
    )
    .is_none());
}

#[test]
fn subtype_table_walks_wide_strings_at_the_stream_ref_width() {
    for ref_width in [
        cadmpeg_asm::kernel_header::RefWidth::Four,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    ] {
        // The last four payload bytes spell a definition opening. Only a walker
        // that consumes the length prefix at `ref_width` steps past them.
        let payload = [b'0', b'1', b'2', b'3', 0x0f, 0x0d, 0x01, b'x'];

        let mut active = Vec::new();
        t_ident(&mut active, "tspl");
        active.push(0x09);
        active.extend_from_slice(&payload.len().to_le_bytes()[..ref_width.bytes()]);
        active.extend_from_slice(&payload);
        let definition = active.len();
        active.extend_from_slice(b"\x0f\x0d\x08real_def\x10");
        active.push(0x11);

        let tables = cadmpeg_asm::nurbs::subtypes::SubtypeTables::from_stream(&active);
        assert_eq!(tables.for_width(ref_width), [definition]);
    }
}

#[test]
fn rgb_attribute_chain_decodes_body_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "body");
    t_ref(&mut bytes, 1); // attrib-chain head
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1); // next attrib
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (0.1, 0.2, 0.3, 1.0)
    );
}

#[test]
fn truecolor_attribute_chain_decodes_by_color_as_opaque_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "truecolor");
    t_subident(&mut bytes, "adesk");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    bytes.push(0x17);
    bytes.extend_from_slice(&(0xc240_80c0i64).to_le_bytes());
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_attribute_chain_decodes_rgb() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    push_u8_string(&mut bytes, "4227264"); // 0x4080c0
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
    );
}

#[test]
fn bt_text_color_rejects_non_decimal_and_overwide_values() {
    use std::collections::HashMap;

    for value in ["", "+4227264", "0x4080c0", "16777216"] {
        let mut bytes = Vec::new();
        t_ident(&mut bytes, "face");
        t_ref(&mut bytes, 1);
        t_end(&mut bytes);
        t_subident(&mut bytes, "entatt_color");
        t_subident(&mut bytes, "bt");
        t_ident(&mut bytes, "attrib");
        t_ref(&mut bytes, -1);
        push_u8_string(&mut bytes, value);
        t_end(&mut bytes);

        let records = cadmpeg_asm::sab::frame(
            &bytes,
            0,
            bytes.len(),
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .unwrap();
        let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
        assert!(
            cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).is_none()
        );
    }
}

#[test]
fn invalid_color_attribute_does_not_hide_later_chain_color() {
    use std::collections::HashMap;

    let mut bytes = Vec::new();
    t_ident(&mut bytes, "face");
    t_ref(&mut bytes, 1);
    t_end(&mut bytes);
    t_subident(&mut bytes, "entatt_color");
    t_subident(&mut bytes, "bt");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, 2);
    push_u8_string(&mut bytes, "not-a-color");
    t_end(&mut bytes);
    t_subident(&mut bytes, "rgb_color");
    t_subident(&mut bytes, "st");
    t_ident(&mut bytes, "attrib");
    t_ref(&mut bytes, -1);
    t_dbl(&mut bytes, 0.1);
    t_dbl(&mut bytes, 0.2);
    t_dbl(&mut bytes, 0.3);
    t_end(&mut bytes);

    let records = cadmpeg_asm::sab::frame(
        &bytes,
        0,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let by_index: HashMap<i64, _> = records.iter().map(|r| (r.index as i64, r)).collect();
    let color =
        cadmpeg_asm::brep::attributes::attribute_chain_color(&records[0], &by_index).unwrap();
    assert_eq!(
        (color.r(), color.g(), color.b(), color.a()),
        (0.1, 0.2, 0.3, 1.0)
    );
}
