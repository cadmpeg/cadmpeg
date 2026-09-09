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
        let wire = serde_value::to_value(&out.face_sidedness[0]).unwrap();
        let serde_value::Value::Map(fields) = &wire else {
            panic!("record object")
        };
        assert_eq!(
            fields.get(&serde_value::Value::String("normalized_sense".into())),
            Some(&serde_value::to_value(normalized).unwrap())
        );
        let restored: FaceSidedness = serde::Deserialize::deserialize(wire).unwrap();
        assert_eq!(restored, out.face_sidedness[0]);
        assert_eq!(out.face_sidedness[0].carrier_flipped, native != normalized);
        out.faces[0].sense = match normalized {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        };
        assert_eq!(out.face_sidedness[0].carrier_flipped, native != normalized);
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
        )
        .unwrap();
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
            emit_vertices(&mut out, &records, &by_index, &reach, IdFormat("f3d"))
                .expect("valid tolerant vertex fixture");
            assert_eq!(out.vertices.len(), 1);
            assert_eq!(
                out.vertices[0]
                    .tolerance
                    .map(cadmpeg_ir::units::PositiveScalar::get),
                tolerance
            );
            assert_eq!(out.tolerant_vertex_tails.len(), 1);
            assert_eq!(
                out.tolerant_vertex_tails[0].leading_tolerances,
                [0.03, 0.07]
            );
            assert_eq!(out.tolerant_vertex_tails[0].evaluated_unset, unset);
        }
    }
}

#[test]
fn reversed_intcurve_context_uses_the_parsed_cache_domain() {
    use crate::nurbs::proc_curve::nurbs_curve_parameter_domain;
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SpringLayout};

    let record = |index, name: &str, tokens: Vec<Token>| Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    };
    for reversed in [false, true] {
        let mut curve_tokens = vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            if reversed { Token::True } else { Token::False },
            Token::SubtypeOpen,
            Token::Ident("spring_int_cur".into()),
            Token::Long(23_100),
            Token::Enum(0),
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Enum(0),
            Token::Long(2),
            Token::Double(2.0),
            Token::Long(1),
            Token::Double(5.0),
            Token::Long(1),
        ];
        curve_tokens.extend([0.0, 0.0, 0.0, 1.0, 0.0, 0.0].map(Token::Double));
        curve_tokens.extend([
            Token::Double(0.0004),
            Token::Ident("null_surface".into()),
            Token::Ident("null_surface".into()),
            Token::Ident("nullbs".into()),
            Token::Ident("nullbs".into()),
            Token::False,
            Token::False,
            Token::Long(0),
            Token::Long(0),
            Token::Long(0),
            Token::Long(7),
            Token::Enum(4),
            Token::SubtypeClose,
        ]);
        let refs = |values: &[i64]| values.iter().copied().map(Token::Ref).collect();
        let records = [
            record(0, "face", refs(&[-1, -1, -1, -1, 1])),
            record(1, "loop", refs(&[-1, -1, -1, -1, 2])),
            record(2, "coedge", refs(&[-1, -1, -1, 2, -1, -1, 3])),
            record(3, "edge", refs(&[-1, -1, -1, -1, -1, -1, -1, -1, 4])),
            record(4, "intcurve", curve_tokens),
        ];
        let by_index = records
            .iter()
            .map(|record| (record.index as i64, record))
            .collect();
        let table = crate::nurbs::toks::SubtypeTable::from_records(&records);
        let parsed =
            crate::nurbs::proc_curve::procedural_curve_resolving_refs(&records[4].tokens, &table)
                .unwrap();
        assert_eq!(
            nurbs_curve_parameter_domain(&parsed.curve),
            Some([2.0, 5.0])
        );
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        let mut reach = Reachable {
            faces: HashSet::from([0]),
            ..Reachable::default()
        };
        super::super::topology::walk_reachable_topology(
            &mut out,
            &by_index,
            &table,
            &mut carriers,
            &mut reach,
            super::super::DecodePurpose::Model,
            IdFormat("f3d"),
        );
        let CurveGeometry::Nurbs(normalized) = &carriers.curve_geo[&4] else {
            panic!("solved curve")
        };
        assert_eq!(
            nurbs_curve_parameter_domain(normalized),
            Some(if reversed { [-5.0, -2.0] } else { [2.0, 5.0] })
        );
        emit_carrier_curve(
            &mut out,
            4,
            &mut carriers,
            &HashSet::new(),
            &HashSet::new(),
            IdFormat("f3d"),
        )
        .unwrap();
        let ProceduralCurveDefinition::Spring {
            layout: SpringLayout::CacheFirst { context, .. },
            ..
        } = out.procedural_curves[0].1.definition()
        else {
            panic!("cache-first spring")
        };
        assert_eq!(context.parameter_range(), [2.0, 5.0]);
    }
}

#[test]
fn evaluated_and_absent_vertex_slots_have_the_same_native_tail_wire() {
    let decode = |slot: Option<f64>| {
        let mut tokens = vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Long(0),
            Token::Ref(1),
            Token::Double(0.03),
            Token::Double(0.07),
        ];
        tokens.extend(slot.map(Token::Double));
        let records = [Record {
            index: 0,
            name: "tvertex".into(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        }];
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
        emit_vertices(&mut out, &records, &by_index, &reach, IdFormat("f3d")).unwrap();
        (
            serde_value::to_value(&out.tolerant_vertex_tails[0]).unwrap(),
            out.vertices[0].tolerance,
        )
    };
    let (evaluated_wire, tolerance) = decode(Some(0.125));
    let (absent_wire, absent_tolerance) = decode(None);
    assert_eq!(
        tolerance.map(cadmpeg_ir::units::PositiveScalar::get),
        Some(1.25)
    );
    assert_eq!(absent_tolerance, None);
    assert_eq!(evaluated_wire, absent_wire);
}

#[test]
fn invalid_cache_first_context_keeps_the_decoded_curve() {
    let record = |index, name: &str, tokens: Vec<Token>| Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    };
    let mut curve_tokens = vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::False,
        Token::SubtypeOpen,
        Token::Ident("spring_int_cur".into()),
        Token::Long(23_100),
        Token::Enum(0),
        Token::Ident("nubs".into()),
        Token::Long(1),
        Token::Enum(0),
        Token::Long(2),
        Token::Double(2.0),
        Token::Long(1),
        Token::Double(5.0),
        Token::Long(1),
    ];
    curve_tokens.extend([0.0, 0.0, 0.0, 1.0, 0.0, 0.0].map(Token::Double));
    curve_tokens.extend([
        Token::Double(0.0004),
        Token::Ident("null_surface".into()),
        Token::Ident("null_surface".into()),
        Token::Ident("nullbs".into()),
        Token::Ident("nullbs".into()),
        Token::True,
        Token::Double(5.0),
        Token::True,
        Token::Double(2.0),
        Token::Long(0),
        Token::Long(0),
        Token::Long(0),
        Token::Long(7),
        Token::Enum(4),
        Token::SubtypeClose,
    ]);
    let refs = |values: &[i64]| values.iter().copied().map(Token::Ref).collect();
    let records = [
        record(0, "face", refs(&[-1, -1, -1, -1, 1, -1, -1, 5])),
        record(1, "loop", refs(&[-1, -1, -1, -1, 2])),
        record(2, "coedge", refs(&[-1, -1, -1, 2, -1, -1, 3])),
        record(3, "edge", refs(&[-1, -1, -1, -1, -1, -1, -1, -1, 4])),
        record(4, "intcurve", curve_tokens),
        record(5, "unknown-surface", vec![]),
    ];

    let result = super::super::decode_with_header(
        &records,
        &[],
        None,
        "context",
        IdFormat("f3d"),
        super::super::DecodePurpose::Model,
    );
    let out = result.expect("invalid construction must retain its cache");
    assert!(out
        .curves
        .iter()
        .any(|curve| curve.id.as_str() == "f3d:brep:entity#4"));
    assert!(out.procedural_curves.is_empty());
    assert_eq!(
        out.stats
            .procedural_curve_kinds
            .get("support context parameter_range must be finite and ordered"),
        Some(&1)
    );
}

#[test]
fn procedural_curve_admission_failures_keep_the_carrier() {
    use super::super::ProceduralCurveSource;
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    for (source, cause) in [
        (
            ProceduralCurveSource::Cached {
                construction: Box::new(ProceduralCurveConstruction::Exact),
                cache_fit_tolerance: Some(-1.0),
                parsed_domain: Some([0.0, 1.0]),
            },
            "invalid procedural curve cache tolerance",
        ),
        (
            ProceduralCurveSource::Cacheless(Box::new(ProceduralCurveDefinition::Subset {
                source: CurveId::mint("f3d:brep:entity#source").unwrap(),
                parameter_range: [2.0, 1.0],
                sense: true,
            })),
            "subset-curve range is not finite and ordered",
        ),
    ] {
        let mut out = AsmBrep::default();
        let mut carriers = Carriers::default();
        carriers
            .curve_geo
            .insert(4, CurveGeometry::Unknown { record: None });
        carriers.procedural_curve_defs.insert(4, source);
        emit_carrier_curve(
            &mut out,
            4,
            &mut carriers,
            &HashSet::new(),
            &HashSet::new(),
            IdFormat("f3d"),
        )
        .unwrap();
        assert_eq!(out.curves.len(), 1);
        assert_eq!(out.curves[0].id.as_str(), "f3d:brep:entity#4");
        assert!(out.procedural_curves.is_empty());
        assert_eq!(out.stats.procedural_curve_kinds.get(cause), Some(&1));
    }
}

#[test]
fn failed_procedural_curves_discard_only_their_candidate_children() {
    use super::super::ProceduralCurveSource;
    use crate::nurbs::proc_curve::{EmbeddedIntersection, SupportSlot};
    use cadmpeg_ir::math::Point3;

    for (parameter_range, distance, tolerance, cause) in [
        (
            [2.0, 1.0],
            1.0,
            None,
            "support context parameter_range must be finite and ordered",
        ),
        (
            [0.0, 1.0],
            f64::NAN,
            None,
            "surface-offset fields are not finite and ordered",
        ),
        (
            [0.0, 1.0],
            1.0,
            Some(-1.0),
            "invalid procedural curve cache tolerance",
        ),
    ] {
        let mut out = AsmBrep::default();
        out.surfaces.push(Surface {
            id: SurfaceId::mint("f3d:brep:entity#existing-surface").unwrap(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        out.curves.push(Curve {
            id: CurveId::mint("f3d:brep:entity#existing-curve").unwrap(),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        let mut carriers = Carriers::default();
        carriers
            .curve_geo
            .insert(4, CurveGeometry::Unknown { record: None });
        carriers.procedural_curve_defs.insert(
            4,
            ProceduralCurveSource::Cached {
                construction: Box::new(ProceduralCurveConstruction::SurfaceOffset(
                    EmbeddedSurfaceOffset {
                        layout: EmbeddedSurfaceOffsetLayout::ContextFirst {
                            context: Box::new(EmbeddedIntersection {
                                surfaces: std::array::from_fn(|_| {
                                    SupportSlot::Surface(SurfaceGeometry::Unknown { record: None })
                                }),
                                pcurves: [None, None],
                                parameter_range,
                                discontinuities: std::array::from_fn(|_| Vec::new()),
                            }),
                            discontinuity_flag: false,
                        },
                        base_u_range: [0.0, 1.0],
                        base_v_range: [0.0, 1.0],
                        base_range: [0.0, 1.0],
                        base: NurbsCurve::new(
                            1,
                            vec![0.0, 0.0, 1.0, 1.0],
                            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                            None,
                            false,
                        )
                        .unwrap(),
                        distance,
                        shift: 0.0,
                        scale: 1.0,
                    },
                )),
                cache_fit_tolerance: tolerance,
                parsed_domain: Some([0.0, 1.0]),
            },
        );
        emit_carrier_curve(
            &mut out,
            4,
            &mut carriers,
            &HashSet::from([4]),
            &HashSet::from([4]),
            IdFormat("f3d"),
        )
        .unwrap();
        assert_eq!(
            out.surfaces
                .iter()
                .map(|surface| surface.id.as_str())
                .collect::<Vec<_>>(),
            ["f3d:brep:entity#existing-surface"]
        );
        assert_eq!(
            out.curves
                .iter()
                .map(|curve| curve.id.as_str())
                .collect::<Vec<_>>(),
            [
                "f3d:brep:entity#existing-curve",
                "f3d:brep:entity#4:reversed",
                "f3d:brep:entity#4"
            ]
        );
        assert!(out.procedural_curves.is_empty());
        assert_eq!(out.stats.procedural_curve_kinds.get(cause), Some(&1));
    }
}
