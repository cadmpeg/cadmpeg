// SPDX-License-Identifier: Apache-2.0
//! Native-domain synthetic tests and fixtures.

use super::*;

pub(super) trait TestEncode {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::ExportReport, cadmpeg_core::CodecError>;
}

impl TestEncode for F3dCodec {
    fn encode(
        &self,
        ir: &cadmpeg_ir::CadIr,
        output: &mut dyn Write,
    ) -> Result<cadmpeg_ir::ExportReport, cadmpeg_core::CodecError> {
        self.plan(cadmpeg_ir::codec::EncodeInput { ir, fidelity: None })?
            .write_to(output)
    }
}

pub(super) fn with_scan<T>(bytes: &[u8], f: impl FnOnce(&container::ContainerScan<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    f(&scan)
}

pub(super) fn write_synthetic_manifests<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    options: zip::write::SimpleFileOptions,
) {
    zip.start_file("Manifest.dat", options).unwrap();
    zip.write_all(&crate::manifest::generated_top_level().unwrap())
        .unwrap();
    zip.start_file(
        format!(
            "{}/Manifest.dat",
            crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
        ),
        options,
    )
    .unwrap();
    zip.write_all(&crate::manifest::generated_design_asset().unwrap())
        .unwrap();
}

pub(super) fn assert_f3d_native_parity(ir: &cadmpeg_ir::document::CadIr) {
    let native = ir.native.namespace("f3d").expect("F3D native namespace");
    assert_eq!(native.version, crate::native::F3D_NATIVE_VERSION);
}

pub(super) fn f3d_native(ir: &cadmpeg_ir::document::CadIr) -> crate::native::F3dNative {
    crate::native::F3dNative::load(ir.native.namespace("f3d").expect("F3D native namespace"))
        .unwrap()
}

pub(super) struct F3dNativeMut<'a> {
    ir: &'a mut cadmpeg_ir::document::CadIr,
    native: crate::native::F3dNative,
}

impl std::ops::Deref for F3dNativeMut<'_> {
    type Target = crate::native::F3dNative;

    fn deref(&self) -> &Self::Target {
        &self.native
    }
}

impl std::ops::DerefMut for F3dNativeMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.native
    }
}

impl Drop for F3dNativeMut<'_> {
    fn drop(&mut self) {
        self.native
            .store(self.ir.native.namespace_mut("f3d"))
            .unwrap();
    }
}

pub(super) fn f3d_native_mut(ir: &mut cadmpeg_ir::document::CadIr) -> F3dNativeMut<'_> {
    let native = ir
        .native
        .namespace("f3d")
        .map(crate::native::F3dNative::load)
        .transpose()
        .unwrap()
        .unwrap_or_default();
    F3dNativeMut { ir, native }
}

#[test]
fn native_arenas_have_pinned_shape_and_typed_round_trip() {
    let catalogue_names = crate::native::F3D_FAMILIES
        .iter()
        .map(|row| row.arena)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(crate::native::F3D_FAMILIES.len(), 70);
    assert_eq!(
        catalogue_names,
        crate::native::F3D_ARENA_NAMES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = decoded.ir().native.namespace("f3d").unwrap();
    let typed = crate::native::F3dNative::load(original).unwrap();
    let mut round_trip = cadmpeg_ir::NativeNamespace::default();
    typed.store(&mut round_trip).unwrap();
    assert_eq!(typed, crate::native::F3dNative::load(&round_trip).unwrap());
    for name in crate::native::F3D_ARENA_NAMES {
        assert_eq!(
            round_trip.arenas.get(*name),
            original.arenas.get(*name),
            "native arena {name} did not survive a typed round trip"
        );
    }
    assert_eq!(round_trip.version, crate::native::F3D_NATIVE_VERSION);
    assert_eq!(
        round_trip
            .arenas
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::F3D_ARENA_NAMES
    );
    for records in round_trip.arenas.values() {
        for record in records {
            let json = serde_json::to_value(record).unwrap();
            assert_eq!(json["id"], record.id());
            assert!(json.as_object().unwrap().len() > 1);
        }
    }
}

#[test]
fn diff_reports_design_material_assignment_changes() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut edited = decoded.ir().clone();
    let assignment = &mut edited
        .native
        .namespace_mut("f3d")
        .arenas
        .get_mut("design_material_assignments")
        .unwrap()[0];
    let mut assignment_fields = assignment.fields();
    assignment_fields.insert("entity_suffix".into(), serde_json::json!(123_456));
    *assignment = cadmpeg_ir::NativeRecord::new(assignment.id().to_string(), assignment_fields);
    let report = cadmpeg_ir::diff(decoded.ir(), &edited);
    let arena = report
        .per_arena
        .iter()
        .find(|arena| arena.kind == "native.f3d.design_material_assignments")
        .unwrap();
    assert_eq!(arena.modified.len(), 1);
}

pub(super) fn update_f3d_native<R>(
    ir: &mut cadmpeg_ir::document::CadIr,
    update: impl FnOnce(&mut crate::native::F3dNative) -> R,
) -> R {
    let mut native = f3d_native_mut(ir);
    update(&mut native)
}

#[test]
fn decode_transfers_generated_tolerant_coedge_parameters_and_topology() {
    let mut smbh = synthetic_geometry_smbh();
    let mut parameter_tail = Vec::new();
    t_dbl(&mut parameter_tail, 0.25);
    t_dbl(&mut parameter_tail, 0.75);
    t_ref(&mut parameter_tail, -1);
    t_long(&mut parameter_tail, 0);
    t_long(&mut parameter_tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &parameter_tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");
    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("generated tolerant coedges must decode");

    assert_eq!(decoded.ir().model.coedges.len(), 3);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert_eq!(decoded.ir().model.shells[0].faces.len(), 1);
    assert_eq!(
        f3d_native(decoded.ir())
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.parameter_range)
            .collect::<Vec<_>>(),
        vec![[0.25, 0.75]; 3]
    );
    assert!(f3d_native(decoded.ir())
        .tolerant_coedge_parameters
        .iter()
        .all(|parameters| matches!(
            parameters.extension,
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None }
        )));

    decoded.ir_mut().model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    update_f3d_native(decoded.ir_mut(), |native| {
        native.tolerant_coedge_parameters[0].parameter_range = [-1.5, 2.25];
    });
    let mut edited = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut edited)
        .expect("tolerant coedge sense edit");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("edited tolerant coedge round trip");
    assert_eq!(
        round_trip.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_coedge_parameters[0].parameter_range,
        [-1.5, 2.25]
    );
}

#[test]
fn decode_selects_tolerant_coedge_extension_from_save_format() {
    for (release, fixed_tail, expected) in [
        (
            23000u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, -1);
                t_long(&mut bytes, 1);
                bytes.extend_from_slice(&[0x0a, 0x0f]);
                t_long(&mut bytes, 22800);
                bytes.extend_from_slice(&[0x10, 0x0a]);
                t_dbl(&mut bytes, -2.0);
                bytes.push(0x0a);
                t_dbl(&mut bytes, 3.0);
                t_long(&mut bytes, 0);
                bytes
            },
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some([-2.0, 3.0]),
            },
        ),
        (
            21900u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, 17);
                bytes
            },
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Reference { target: Some(17) },
        ),
        (
            21400u32,
            Vec::new(),
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None,
        ),
    ] {
        let mut smbh = synthetic_geometry_smbh();
        smbh[15..19].copy_from_slice(&release.to_le_bytes());
        let mut tail = Vec::new();
        t_dbl(&mut tail, -0.5);
        t_dbl(&mut tail, 1.5);
        tail.extend_from_slice(&fixed_tail);
        append_generated_record_tail(&mut smbh, "coedge", &tail);
        replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("release-selected tolerant coedges must decode");
        assert_eq!(
            f3d_native(decoded.ir())
                .tolerant_coedge_parameters
                .iter()
                .map(|parameters| parameters.extension.clone())
                .collect::<Vec<_>>(),
            vec![expected; 3]
        );
    }
}

/// A tolerant coedge whose payload carries identifier tokens outside the
/// embedded scope: the freestanding embedded-curve type name before the sense
/// flag and trailing `null_curve` placeholders after the extension fields.
/// Identifiers are not fields, so the extension decodes exactly as it does
/// without them and the serialized token count stays defined over the value
/// tokens.
#[test]
fn tolerant_coedge_extension_ignores_payload_identifiers() {
    let mut smbh = synthetic_geometry_smbh();
    smbh[15..19].copy_from_slice(&23000u32.to_le_bytes());
    let mut tail = Vec::new();
    t_dbl(&mut tail, -0.5);
    t_dbl(&mut tail, 1.5);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    t_ident(&mut tail, "intcurve");
    tail.extend_from_slice(&[0x0a, 0x0f]);
    t_ident(&mut tail, "par_int_cur");
    t_long(&mut tail, 22800);
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    t_ident(&mut tail, "null_curve");
    t_ident(&mut tail, "null_curve");
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("ident-bearing tolerant coedges must decode");
    assert_eq!(
        f3d_native(decoded.ir())
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.extension.clone())
            .collect::<Vec<_>>(),
        vec![
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some([-2.0, 3.0]),
            };
            3
        ]
    );
}

#[test]
fn decode_transfers_embedded_tolerant_coedge_use_curves() {
    let mut smbh = synthetic_geometry_smbh();
    let mut tail = Vec::new();
    t_dbl(&mut tail, 0.0);
    t_dbl(&mut tail, 1.0);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    tail.extend_from_slice(&[0x0a, 0x0f]);
    tail.extend_from_slice(&generated_curve_block());
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("embedded tolerant-coedge curves must decode");
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        3
    );
    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge.use_curve_parameter_range == Some([-2.0, 3.0])
            && coedge.use_curve.as_ref().is_some_and(|id| {
                decoded.ir().model.curves.iter().any(|curve| {
                    curve.id == *id
                        && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref nurbs) if nurbs.degree == 2)
                })
            })
    }));
    let first_use_curve = decoded.ir().model.coedges[0]
        .use_curve
        .as_ref()
        .and_then(|id| {
            decoded
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *id)
        })
        .expect("first embedded use curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(first_use_curve) = &first_use_curve.geometry
    else {
        panic!("embedded use curve must be NURBS")
    };
    assert_eq!(
        first_use_curve.control_points[0],
        cadmpeg_ir::math::Point3::new(20.0, 0.0, 0.0)
    );
    assert_eq!(
        first_use_curve.control_points[2],
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(first_use_curve.knots, [-1.0, -1.0, -1.0, -0.0, -0.0, -0.0]);

    let mut edited = decoded.ir().clone();
    let use_curve = edited.model.coedges[0]
        .use_curve
        .clone()
        .expect("first coedge use curve");
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == use_curve)
        .expect("embedded use-curve carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        panic!("embedded use curve must be NURBS")
    };
    nurbs.control_points[0].x += 1.0;
    let expected = nurbs.clone();
    let mut preserved = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut preserved)
        .expect("embedded use-curve edit");
    let preserved = F3dCodec
        .decode(&mut Cursor::new(preserved), &DecodeOptions::default())
        .expect("embedded use-curve edit round trip");
    assert!(preserved.ir().model.curves.iter().any(|curve| {
        curve.id == use_curve
            && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref curve) if *curve == expected)
    }));

    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let generated_curve_id = cadmpeg_ir::ids::CurveId("generated:tolerant-use-curve#0".into());
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: generated_curve_id.clone(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Nurbs(expected.clone()),
        source_object: None,
    });
    let tolerant_coedge = source_less.model.coedges[0].id.clone();
    source_less.model.coedges[0].use_curve = Some(generated_curve_id);
    source_less.model.coedges[0].use_curve_parameter_range = Some([-2.0, 3.0]);
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
            id: "generated:tolerant-coedge-parameters#0".into(),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: [0.0, 1.0],
            extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: false,
                payload_token_count: 0,
                parameter_range: Some([-2.0, 3.0]),
            },
        }];
    let mut generated = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut generated))
        .expect("source-less embedded use curves");
    let generated = F3dCodec
        .decode(&mut Cursor::new(generated), &DecodeOptions::default())
        .expect("source-less embedded use-curve round trip");
    assert_eq!(
        generated
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        1
    );
    assert!(generated.ir().model.curves.iter().any(|curve| {
        matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(ref curve) if *curve == expected)
    }));
}

#[test]
fn decode_frames_history_less_stream_whose_final_record_ends_at_eof() {
    // A history-less `.smb` stream has no `delta_state` boundary and its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator.
    let mut smbh = synthetic_geometry_smbh();
    let marker = smbh
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .expect("generated history boundary");
    smbh.truncate(marker);
    for name in ["End", "of", "ASM"] {
        t_subident(&mut smbh, name);
    }
    t_ident(&mut smbh, "data"); // no trailing 0x11
    assert!(cadmpeg_asm::asm_header::solved_record_limit(&smbh).is_none());

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("history-less stream must decode");
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
}

#[test]
fn stamped_law_intcurve_round_trips_byte_exactly() {
    use cadmpeg_ir::geometry::{CurveGeometry, LawExpression, ProceduralCurveDefinition};

    // Formula names exceed 255 bytes to exercise the u16 (`0x08`) length prefix
    // the serializer selects for long law text.
    let primary_name = format!("TRANS({},TRANS1)", "VEC(X,X2,X3)*COS(X)+".repeat(20));
    let raw_name = "VEC(X,X2,X3)*COS(X)+".repeat(20);
    assert!(primary_name.len() > 255 && raw_name.len() > 255);
    let subtype = stamped_law_curve_subtype(&primary_name, &raw_name);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_stamped_law_curve_smbh(&subtype),
            )),
            &DecodeOptions::default(),
        )
        .expect("stamped law intcurve decode");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition, ProceduralCurveDefinition::Law { .. }))
        .expect("stamped law construction");
    let ProceduralCurveDefinition::Law {
        version,
        primary,
        additional,
        ..
    } = &procedural.definition
    else {
        unreachable!()
    };
    let version = version.as_ref().expect("version stamp");
    assert_eq!(version.stamp, 20900);
    assert_eq!(version.post_enum, 0);
    assert_eq!(version.parameter_range, [None, None]);
    assert_eq!(primary.name, primary_name);
    assert!(matches!(
        primary.variables[0],
        LawExpression::TransformVec { .. }
    ));
    assert_eq!(additional.len(), 4);
    assert_eq!(additional[0].name, "null_law");
    assert_eq!(additional[1].name, "null_law");
    assert_eq!(additional[2].name, raw_name);
    assert_eq!(additional[3].name, "TRANS(VEC(X,X2,X3),TRANS1)");
    assert!(matches!(
        additional[3].variables[0],
        LawExpression::TransformVec { .. }
    ));

    // Byte-exact re-emission of the subtype span. The solved cache uses
    // integer-valued control points so the cm->mm scaling round-trip is exact.
    let solved = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs.clone()),
            _ => None,
        })
        .expect("solved cache");
    let mut regenerated = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_curve(
        &mut regenerated,
        decoded.ir(),
        &procedural.curve,
        &solved,
    )
    .expect("regenerate stamped law curve");
    let inner = regenerated.iter().position(|&b| b == 0x0f).unwrap();
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(&regenerated, inner, 8).unwrap();
    assert_eq!(span, subtype.as_slice());
}

#[test]
fn legacy_law_intcurve_round_trips_byte_exactly() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition};

    let smbh = synthetic_geometry_with_law_curve_smbh();
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("legacy law intcurve decode");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition, ProceduralCurveDefinition::Law { .. }))
        .expect("legacy law construction");
    let ProceduralCurveDefinition::Law { version, .. } = &procedural.definition else {
        unreachable!()
    };
    assert!(version.is_none());

    let original = {
        let marker = smbh
            .windows(b"law_int_cur".len())
            .position(|window| window == b"law_int_cur")
            .unwrap()
            - 3;
        cadmpeg_asm::nurbs::subtypes::subtype_span(&smbh, marker, 8)
            .unwrap()
            .to_vec()
    };
    let solved = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => Some(nurbs.clone()),
            _ => None,
        })
        .expect("solved cache");
    let mut regenerated = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_curve(
        &mut regenerated,
        decoded.ir(),
        &procedural.curve,
        &solved,
    )
    .expect("regenerate legacy law curve");
    let inner = regenerated.iter().position(|&b| b == 0x0f).unwrap();
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(&regenerated, inner, 8).unwrap();
    assert_eq!(span, original.as_slice());
}

#[test]
fn generated_cache_first_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "spring_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        curve.push(0x15);
                        curve.extend_from_slice(&4i64.to_le_bytes());
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first spring decode");
    let ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        cache_first,
        direction,
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("expected spring construction")
    };
    let form = cache_first.as_ref().expect("cache-first spring form");
    assert_eq!(form.revision, 23100);
    assert_eq!(form.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(form.extension, 7);
    assert_eq!(*direction, 4);
    assert!(!discontinuity_flag);
    assert_eq!(*surface_parameter_ranges, [None, None]);
    assert_eq!(*first_pcurve_parameter_range, None);
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first spring round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_cache_first_parametric_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamily};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "par_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        curve.push(0x0a);
                        curve.push(0x0b);
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first parametric decode");
    let ProceduralCurveDefinition::SurfaceCurve {
        family,
        context,
        tail,
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("expected surface-curve construction")
    };
    assert_eq!(*family, SurfaceCurveFamily::Parametric);
    let tail = tail.as_ref().expect("cache-first parametric tail");
    assert_eq!(tail.revision, 23100);
    assert_eq!(tail.extension, 7);
    assert!(tail.flag);
    assert_eq!(tail.second_flag, Some(false));
    assert_eq!(tail.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(context.parameter_range, [-1.0, 2.0]);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first parametric encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first parametric round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition,
        source_less.model.procedural_curves[0].definition
    );
}

#[test]
fn generated_cache_first_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "off_surf_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        for value in [-1.0, 2.0, -3.0, 4.0] {
                            curve.push(0x0a);
                            t_dbl(curve, value);
                        }
                        curve.extend_from_slice(&generated_curve_block());
                        curve.push(0x0b);
                        curve.push(0x0b);
                        curve.push(0x0a);
                        t_dbl(curve, -0.5);
                        curve.push(0x0a);
                        t_dbl(curve, 1.5);
                        t_dbl(curve, -0.25);
                        t_dbl(curve, 0.75);
                        t_dbl(curve, 1.25);
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first surface-offset decode");
    let ProceduralCurveDefinition::SurfaceOffset {
        cache_first,
        base_u_range,
        base_v_range,
        base_endpoints,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = &result.ir().model.procedural_curves[0].definition
    else {
        panic!("expected surface-offset construction")
    };
    let form = cache_first
        .as_ref()
        .expect("cache-first surface-offset form");
    assert_eq!(form.revision, 23100);
    assert_eq!(form.extension, 7);
    assert_eq!(*base_u_range, [-1.0, 2.0]);
    assert_eq!(*base_v_range, [-3.0, 4.0]);
    assert_eq!(*base_endpoints, [None, None]);
    assert_eq!(*base_range, [-0.5, 1.5]);
    assert_eq!(*distance, -2.5);
    assert_eq!(*shift, 0.75);
    assert_eq!(*scale, 1.25);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first surface-offset round trip");
    let mut expected = source_less.model.procedural_curves[0].definition.clone();
    let mut actual = round_trip.ir().model.procedural_curves[0]
        .definition
        .clone();
    let (
        ProceduralCurveDefinition::SurfaceOffset {
            base: expected_base,
            ..
        },
        ProceduralCurveDefinition::SurfaceOffset {
            base: actual_base, ..
        },
    ) = (&mut expected, &mut actual)
    else {
        panic!("expected surface-offset round trip")
    };
    let round_trip_base = actual_base.clone();
    *actual_base = expected_base.clone();
    assert_eq!(actual, expected);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == round_trip_base));
}

pub(super) fn assert_revision_surface_round_trip(smbh: Vec<u8>, expected_kind: &str) {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision surface decode");
    let procedural = result
        .ir()
        .model
        .procedural_surfaces
        .first()
        .expect("revision surface construction");
    let expected = scrubbed_definition(&procedural.definition);
    let kind = serde_json::to_value(&procedural.definition).expect("kind")["kind"]
        .as_str()
        .expect("kind string")
        .to_string();
    assert_eq!(kind, expected_kind);
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less revision surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less revision surface round trip");
    let actual = scrubbed_definition(
        &round_trip
            .ir()
            .model
            .procedural_surfaces
            .first()
            .expect("round-trip construction")
            .definition,
    );
    assert_eq!(actual, expected);
}

#[test]
fn generated_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh.clone(), "offset");

    // The revision-gated layout shares byte positions with the pre-revision
    // U/V sense enums but no grammar, so its four-boolean carrier run travels
    // in the revision form and leaves the enum slots empty.
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision offset decode");
    let ProceduralSurfaceDefinition::Offset {
        u_sense,
        v_sense,
        extension_flags,
        revision_form,
        ..
    } = &result.ir().model.procedural_surfaces[0].definition
    else {
        panic!("expected offset surface construction")
    };
    assert_eq!((*u_sense, *v_sense), (None, None));
    assert!(extension_flags.is_empty());
    assert_eq!(
        revision_form.as_ref().expect("revision form").flags,
        [false, true, false, false]
    );
}

#[test]
fn generated_parameterized_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_parameterized_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh.clone(), "offset");

    let subtype = synthetic_revision_surface_subtype_span(&smbh);
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision offset decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    // Cache form 2 stores no fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset { revision_form, .. } =
        &procedural.definition
    else {
        panic!("expected offset surface construction")
    };
    let form = revision_form.as_ref().expect("revision form");
    assert_eq!(form.tail_enum, 2);
    let parameterization = form
        .tail_parameterization
        .as_ref()
        .expect("tail parameterization");
    assert_eq!(parameterization.u_interval, [Some(0.25), None]);
    assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
    assert_eq!(
        (parameterization.u_closure, parameterization.v_closure),
        (1, 0)
    );
    assert_eq!(
        (
            parameterization.u_singularity,
            parameterization.v_singularity
        ),
        (2, 3)
    );
    assert_eq!(regenerated_procedural_surface_span(result.ir()), subtype);
}

#[test]
fn generated_revision_orthogonal_taper_round_trips() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        surface.push(0x0a);
    });
    assert_revision_surface_round_trip(smbh, "taper");
}

#[test]
fn generated_revision_orthogonal_taper_decodes_sense_true() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        // Trailing orthogonal-sense logical set true.
        surface.push(0x0a);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("ortho revision decode");
    let definition = &result
        .ir()
        .model
        .procedural_surfaces
        .first()
        .expect("ortho construction")
        .definition;
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Taper { taper, .. } = definition else {
        panic!("expected taper definition, got {definition:?}");
    };
    assert_eq!(
        *taper,
        cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense: true }
    );
}

#[test]
fn generated_revision_sweep_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sweep_sur", |surface| {
        surface.push(0x0b);
        t_long(surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        t_pos(surface, [1.0, 2.0, 3.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 1.0, 0.0]);
        t_long(surface, 1);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 0.5);
        t_dbl(surface, 0.0);
        surface.push(0x0b);
        t_str(surface, "MTRAIL(EDGE1)");
        t_long(surface, 1);
        t_str(surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_dbl(surface, 0.0);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sweep");
}

#[test]
fn generated_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(surface, 1, Some(-1), push_revision_surface_tail);
    });
    assert_revision_surface_round_trip(smbh, "loft");
}

#[test]
fn generated_parameterized_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(
            surface,
            1,
            Some(-1),
            push_parameterized_revision_surface_tail,
        );
    });
    assert_revision_surface_round_trip(smbh.clone(), "loft");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision loft decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    // Cache form 2 stores no fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance, None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Loft { revision_form, .. } =
        &procedural.definition
    else {
        panic!("expected a loft construction")
    };
    let form = revision_form.as_ref().expect("revision form");
    assert_parameterized_tail(form.tail_enum, form.tail_parameterization.as_ref());
}

#[test]
fn revision_loft_member_omits_the_asm_integer_in_an_early_save_format_stream() {
    // Save format 22600: the constraint subdata follows the first flag with no
    // ASM integer between them.
    let smbh = with_save_format(
        synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
            push_revision_loft_body(surface, 1, None, push_revision_surface_tail);
        }),
        22600,
    );
    let subtype = synthetic_revision_surface_subtype_span(&smbh);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("early-era revision loft decode");
    let member = decoded_revision_loft_member(decoded.ir());
    assert_eq!(member.type_code, 1);
    assert_eq!(member.data.first_flag, Some(false));
    assert_eq!(member.data.asm_extension, None);
    assert_eq!(member.data.secondary_pcurve, None);
    assert_eq!(regenerated_procedural_surface_span(decoded.ir()), subtype);
}

#[test]
fn revision_loft_type_zero_member_stores_two_pcurve_slots() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(surface, 0, Some(-1), push_revision_surface_tail);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("type-zero revision loft decode");
    let member = decoded_revision_loft_member(decoded.ir());
    assert_eq!(member.type_code, 0);
    assert_eq!(member.data.surface, None);
    assert!(member.data.pcurve.is_some());
    assert_eq!(member.data.secondary_pcurve, None);
    assert_eq!(member.data.first_flag, None);
    assert_eq!(member.data.asm_extension, Some(-1));
    assert_revision_surface_round_trip(smbh, "loft");
}

/// Wrap an ASM stream byte blob into a `.f3d` ZIP as `Body1.smbh`.
pub(super) fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn malformed_tspline_cage_degrades_to_a_loss_note() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_geometry_smbh()).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/TSplines.BlobParts/Cage1.tsm",
        stored,
    )
    .unwrap();
    // An edge-root index far outside the half-edge range makes the cage
    // internally inconsistent while the entry itself stays well-formed.
    zip.write_all(b"tsm 1.0\ner 999\n").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("an inconsistent cage must not fail the document decode");
    assert!(result.ir().model.subds.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("T-spline control cage not decoded")));
}

#[test]
fn malformed_paramesh_reports_its_entry_and_parser_failure() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    let entry = "FusionAssetName[Active]/ParaMeshGeometry.BlobParts/broken.paramesh";
    zip.start_file(entry, stored).unwrap();
    zip.write_all(b"not a paramesh container").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("independent malformed mesh entry must not abort document decode");
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == LossCode::shared(LossTaxonomy::DecodeDiagnostic)
            && loss.severity == Severity::Error
            && loss.message.contains(entry)
            && loss.message.contains("paramesh container has no magic")
    }));
}

pub(super) fn f3d_with_deflated_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
        deflated,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(super) fn set_zip_entry_uncompressed_size(archive: &mut [u8], target: &[u8], size: u32) {
    let central = archive
        .windows(4)
        .enumerate()
        .find_map(|(offset, signature)| {
            if signature != b"PK\x01\x02" || offset + 46 > archive.len() {
                return None;
            }
            let name_length = u16::from_le_bytes(
                archive[offset + 28..offset + 30]
                    .try_into()
                    .expect("central name-length field"),
            ) as usize;
            (archive.get(offset + 46..offset + 46 + name_length) == Some(target)).then_some(offset)
        })
        .expect("generated ZIP central-directory entry");
    archive[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
}

#[test]
fn oversized_zip_entry_declaration_is_rejected_before_allocation() {
    let mut archive = f3d_with_deflated_smbh(&synthetic_geometry_smbh());
    let target = b"FusionAssetName[Active]/Breps.BlobParts/Body1.smbh";
    set_zip_entry_uncompressed_size(&mut archive, target, u32::MAX);

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("oversized inflated entry must be rejected");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)),
        "{error:?}"
    );
}

#[test]
fn write_path_protein_bounds_remain_local_constants() {
    // Decode nested Protein ZIPs charge through ArchiveSnapshot / begin_expand.
    // The write-path rewriter has no DecodeContext and keeps these local caps.
    assert_eq!(crate::container::MAX_ARCHIVE_BYTES, 256 * 1024 * 1024);
    assert_eq!(
        crate::container::MAX_INFLATED_ENTRY_BYTES,
        128 * 1024 * 1024
    );
}

#[test]
fn oversized_nested_protein_entry_is_rejected_before_allocation() {
    let target = b"AssetData/InstanceProperties.bin";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file(std::str::from_utf8(target).unwrap(), stored)
        .unwrap();
    zip.write_all(b"properties").unwrap();
    let mut protein = zip.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX);

    let error =
        crate::materials::patch_protein_appearances(&protein, &std::collections::BTreeMap::new())
            .expect_err("oversized nested Protein entry must be rejected");
    assert!(error.to_string().contains("inflated bytes"));
}

#[test]
fn nested_protein_decode_charges_through_session_expand_ceilings() {
    use cadmpeg_core::decode::ResourceDimension;

    let target = b"AssetData/InstanceProperties.bin";
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    nested
        .start_file(std::str::from_utf8(target).unwrap(), deflated)
        .unwrap();
    nested.write_all(b"properties").unwrap();
    let mut protein = nested.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX);

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut outer, stored);
    outer
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
    outer.write_all(&synthetic_geometry_smbh()).unwrap();
    outer
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    outer.write_all(&protein).unwrap();
    let archive = outer.finish().unwrap().into_inner();

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("nested Protein inflate must refuse session expand ceilings");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::DecompressedBytes
        ),
        "{error:?}"
    );
}

#[test]
fn nested_protein_decode_honors_operator_per_expand_ceiling() {
    use cadmpeg_core::decode::ResourceDimension;

    let target = b"AssetData/InstanceProperties.bin";
    let payload = vec![b'x'; 64 * 1024];
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    nested
        .start_file(std::str::from_utf8(target).unwrap(), deflated)
        .unwrap();
    nested.write_all(&payload).unwrap();
    let protein = nested.finish().unwrap().into_inner();

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut outer, stored);
    outer
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
    outer.write_all(&synthetic_geometry_smbh()).unwrap();
    outer
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    outer.write_all(&protein).unwrap();
    let archive = outer.finish().unwrap().into_inner();

    let mut options = DecodeOptions::default();
    options.policy.limits.max_decompressed_bytes_per_expand = 1024;
    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &options)
        .expect_err("operator per-expand ceiling must bind nested Protein inflate");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::DecompressedBytes
        ),
        "{error:?}"
    );
}

pub(super) fn f3d_with_configuration(smbh: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file(name, stored).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn form_dispatcher_binds_the_legacy_single_cage_gate() {
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut bulk = Vec::new();
    let mut cage_list = vec![0; 100];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"355");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    cage_list[47..49].copy_from_slice(&[0xfc, 0]);
    bulk.extend_from_slice(&cage_list);

    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"262");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    bulk.extend_from_slice(&paired);

    let mut object = vec![0; 15];
    object[..4].copy_from_slice(&3u32.to_le_bytes());
    object[4..7].copy_from_slice(b"325");
    object[7..11].copy_from_slice(&971u32.to_le_bytes());
    bulk.extend_from_slice(&object);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
            properties: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("f3d:model:subd#1".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    }];

    crate::tests::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("legacy Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}

#[test]
fn form_dispatcher_binds_a_unique_long_cage_list() {
    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut cage_list = vec![0; 99];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"415");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[21] = 1;
    cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
    cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
    cage_list[36] = 1;
    cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"258");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    let mut bulk = cage_list;
    bulk.extend_from_slice(&paired);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        "Form",
        201,
    );
    scope.reference_members = vec![205];
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "Form".into(),
            parameters: Default::default(),
            properties: Default::default(),
        },
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("f3d:model:subd#1".into()),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: None,
    }];

    crate::tests::with_scan(&archive, |scan| {
        crate::design::feature_project::bind_form_cages(
            scan,
            std::slice::from_ref(&scope),
            &mut features,
            &cages,
        )
    })
    .expect("long Form cage binding");
    assert_eq!(
        features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::Form {
            cages: vec![cages[0].id.clone()],
        }
    );
}
