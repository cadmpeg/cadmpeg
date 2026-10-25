// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
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

use cadmpeg_core::convert::f64_from_index;

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut, update_f3d_native, TestEncode};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

/// Public planning must author the document even when retained layout declines.
pub(super) fn synthesize_document(
    ir: &cadmpeg_ir::CadIr,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
) -> (
    cadmpeg_ir::DecodeResult,
    cadmpeg_ir::report::export::ExportReport,
) {
    let plan = F3dCodec
        .plan(EncodeInput::new(ir, fidelity), TargetRequest::Inherit)
        .expect("a valid document has a synthesized F3D path");
    assert!(matches!(
        plan.report().write_path(),
        cadmpeg_ir::report::export::WritePath::Synthesized { .. }
    ));
    let mut bytes = Vec::new();
    let report = plan
        .write_to(&mut bytes)
        .expect("a synthesized F3D archive is writable");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the synthesized archive is a valid F3D file");
    (decoded, report)
}

pub(super) fn assert_native_omission(report: &cadmpeg_ir::report::export::ExportReport) {
    assert!(
        report.losses.iter().any(|loss| {
            loss.code.taxonomy() == cadmpeg_ir::report::loss::LossTaxonomy::RecordNotTyped
                || loss.code.taxonomy()
                    == cadmpeg_ir::report::loss::LossTaxonomy::PassthroughRecordOmitted
                || loss.code.taxonomy()
                    == cadmpeg_ir::report::loss::LossTaxonomy::ParametricRecordOmitted
        }),
        "unsupported native content must be reported: {:?}",
        report.losses
    );
}

const EPS_CONE_ANGLE: f64 = 1.0e-12;

fn finite_tolerance_pair(values: [f64; 2]) -> [cadmpeg_ir::scalar::FiniteReal; 2] {
    values.map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap())
}

#[test]
fn generated_f3d_synthesizes_only_document_content_and_applies_semantic_edits() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap(),
    );

    let (output, report) = synthesize_document(decoded.ir(), Some(decoded.source_fidelity()));
    assert_eq!(
        output.ir().model.bodies.len(),
        decoded.ir().model.bodies.len()
    );
    assert_eq!(
        output.ir().model.faces.len(),
        decoded.ir().model.faces.len()
    );
    assert_native_omission(&report);

    let mut point_edited = decoded.ir().clone();
    let moved = point_edited.model.points[0].position().get();
    point_edited.model.points[0].set_position(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            moved.x + 12.5,
            moved.y,
            moved.z,
        ))
        .expect("a finite position is a point"),
    );
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) =
        &mut point_edited.model.surfaces[0].geometry
    else {
        panic!("generated carrier must be a plane")
    };
    let origin = plane_surface.origin();
    let mut origin = *origin;

    origin.z += 25.0;
    let normal = cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0);
    let u_axis = cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0);
    *plane_surface =
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, normal, u_axis).unwrap();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(
        &point_edited,
        decoded.source_fidelity(),
        &mut regenerated,
    )
    .unwrap();
    assert_ne!(regenerated, source);
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.points[0].position().get(),
        point_edited.model.points[0].position().get()
    );
    assert_eq!(
        round_trip.ir().model.surfaces[0].geometry,
        point_edited.model.surfaces[0].geometry
    );

    let (mut modified, _, fidelity) = decoded.into_parts();
    modified.model.bodies[0].name = Some("edited".into());
    let (output, _) = synthesize_document(&modified, Some(&fidelity));
    assert_eq!(
        output.ir().model.bodies[0].name,
        modified.model.bodies[0].name
    );
}

#[test]
fn generated_source_less_planar_triangle_writes_native_f3d() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.bodies[0].visible = Some(false);
    source_less.model.vertices[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.025).expect("positive finite tolerance"));
    source_less.model.edges[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.035).expect("positive finite tolerance"));
    let tangent_edge = source_less.model.edges[0].id.clone();
    let visible_body = source_less.model.bodies[0].id.clone();
    let tolerant_vertex = source_less.model.vertices[0].id.clone();
    let tolerant_edge = source_less.model.edges[0].id.clone();
    let owner_coedge = source_less.model.coedges[0].id.clone();
    let tolerant_coedge = source_less.model.coedges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut source_less);
        let metadata = native
            .edge_continuities
            .iter_mut()
            .find(|metadata| metadata.edge == tangent_edge)
            .expect("generated edge continuity");
        metadata.continuity = "tangent".into();
        metadata.sense = cadmpeg_ir::topology::Sense::Reversed;
        native.face_sidedness[0].containment =
            Some(cadmpeg_asm::brep::records::FaceContainment::In);
        native.edge_ownerships[0].owner_coedge = Some(owner_coedge);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
            evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Evaluated {
                trailing: Some(0),
            },
        }];
        native.tolerant_edge_tails = vec![cadmpeg_asm::brep::records::TolerantEdgeTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            edge: tolerant_edge,
            record_index: 0,
            entity_revision: 22800,
            trailing_field: Some(1),
        }];
        native.tolerant_coedge_parameters =
            vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                coedge: tolerant_coedge,
                record_index: 0,
                parameter_range: cadmpeg_ir::units::FiniteVector::new([0.25, 0.75])
                    .expect("finite interval"),
                extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::None {},
            }];
        native.body_visibilities = vec![crate::records::bodies::BodyVisibility::try_from(
            crate::records::bodies::BodyVisibilityWire {
                id: "f3d:design:body-visibility#42".into(),
                body: visible_body,
                stream: "FusionAssetName[Active]/Design1/BulkStream.dat".into(),
                byte_offset: 0,
                asm_body_key_offset: 0,
                asm_body_key: 42,
                entity_suffix: 42,
                visible: false,
            },
        )
        .unwrap()];
    }
    let mut encoded = Vec::new();
    crate::native::reset_load_count();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less F3D encode");
    assert!(
        (1..=3).contains(&crate::native::load_count()),
        "native reconciliation and fresh generation must make bounded namespace reads"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut properties = Vec::new();
    archive
        .by_name("Properties.dat")
        .expect("generated Properties.dat")
        .read_to_end(&mut properties)
        .expect("generated properties bytes");
    assert_eq!(properties, 0u32.to_le_bytes());
    let mut smbh = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
        .expect("generated BREP stream")
        .read_to_end(&mut smbh)
        .expect("generated BREP bytes");
    let record_start = smbh
        .windows(b"\x0d\x09asmheader".len())
        .position(|window| window == b"\x0d\x09asmheader")
        .expect("generated ASM record table");
    let records = cadmpeg_asm::test_support::sab::frame(
        &smbh,
        record_start,
        smbh.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("generated ASM records must frame");
    let point_records = records
        .iter()
        .filter(|record| record.head() == "point")
        .collect::<Vec<_>>();
    assert_eq!(point_records.len(), 3);
    assert!(point_records
        .iter()
        .all(|record| record.len == 60 && record.tokens.len() == 4));
    assert_eq!(
        records
            .iter()
            .filter(|record| record.head() == "tcoedge")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.head() == "tedge")
            .count(),
        1
    );
    drop(archive);
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less F3D round trip");

    {
        let mut invalid = source_less.clone();
        {
            let visibility = &mut f3d_native_mut(&mut invalid).body_visibilities[0];
            let mut wire = crate::records::bodies::BodyVisibilityContent::from(visibility.clone());
            wire.id = "f3d:design:body-visibility#43".into();
            wire.asm_body_key = 43;
            *visibility = crate::records::bodies::BodyVisibility::try_from(wire).unwrap();
        }
        let (updated, _) = synthesize_document(&invalid, None);
        assert_eq!(
            updated.ir().model.bodies[0].visible,
            invalid.model.bodies[0].visible
        );
        let native = f3d_native(updated.ir());
        assert_eq!(
            native.body_visibilities[0].asm_body_key(),
            native.body_native_keys[0]
                .asm_body_key
                .expect("a native body key is generated")
        );
    }

    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        f3d_native(round_trip.ir()).body_native_keys[0].asm_body_key,
        Some(42)
    );
    assert_eq!(round_trip.ir().model.bodies[0].visible, Some(false));
    assert_eq!(f3d_native(round_trip.ir()).body_visibilities.len(), 1);
    assert!(!f3d_native(round_trip.ir()).body_visibilities[0].visible);
    assert_eq!(
        f3d_native(round_trip.ir()).body_visibilities[0].id(),
        "f3d:FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh:body-visibility#42"
    );
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert_eq!(round_trip.ir().model.coedges.len(), 3);
    assert_eq!(round_trip.ir().model.edges.len(), 3);
    assert_eq!(round_trip.ir().model.vertices.len(), 3);
    assert_eq!(
        round_trip.ir().model.vertices[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.025)
    );
    assert_eq!(
        round_trip.ir().model.edges[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.035)
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_edge_tails[0].entity_revision,
        22800
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_edge_tails[0].trailing_field,
        Some(1)
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_vertex_tails[0]
            .leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [-1.0, -1.0]
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_coedge_parameters[0]
            .parameter_range
            .get(),
        [0.25, 0.75]
    );
    let ownerships = f3d_native(round_trip.ir()).vertex_ownerships;
    assert_eq!(ownerships.len(), 3);
    assert_eq!(
        ownerships
            .iter()
            .map(|metadata| metadata.endpoint_index.code())
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );
    let continuities = f3d_native(round_trip.ir()).edge_continuities;
    assert_eq!(continuities.len(), 3);
    assert_eq!(continuities[0].continuity, "tangent");
    assert_eq!(continuities[0].sense, cadmpeg_ir::topology::Sense::Reversed);
    assert_eq!(
        f3d_native(round_trip.ir()).edge_ownerships[0].owner_coedge,
        Some(round_trip.ir().model.coedges[0].id.clone())
    );
    assert!(continuities[1..]
        .iter()
        .all(|metadata| metadata.continuity == "unknown"));
    assert_eq!(
        f3d_native(round_trip.ir()).face_sidedness[0].containment,
        Some(cadmpeg_asm::brep::records::FaceContainment::In)
    );
    assert_eq!(round_trip.ir().model.points, source_less.model.points);
    assert_eq!(round_trip.ir().model.surfaces, source_less.model.surfaces);

    let (mut edited, _, fidelity) = round_trip.into_parts();
    edited.model.bodies[0].visible = Some(true);
    edited.model.vertices[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.05).expect("positive finite tolerance"));
    edited.model.edges[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.06).expect("positive finite tolerance"));
    {
        let mut native = f3d_native_mut(&mut edited);
        native.body_native_keys[0].asm_body_key = Some(84);
        native.face_sidedness[0].containment =
            Some(cadmpeg_asm::brep::records::FaceContainment::Out);
        native.tolerant_vertex_tails[0].leading_tolerances = finite_tolerance_pair([3.5, -4.5]);
    }
    let mut retained = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut retained)
        .expect("retained double-sided containment edit");
    let retained = F3dCodec
        .decode(&mut Cursor::new(retained), &DecodeOptions::default())
        .expect("retained double-sided containment round trip");
    assert_eq!(
        f3d_native(retained.ir()).face_sidedness[0].containment,
        Some(cadmpeg_asm::brep::records::FaceContainment::Out)
    );
    assert_eq!(
        retained.ir().model.vertices[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.05)
    );
    assert_eq!(
        retained.ir().model.edges[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.06)
    );
    assert_eq!(
        f3d_native(retained.ir()).tolerant_edge_tails[0].entity_revision,
        22800
    );
    assert_eq!(
        f3d_native(retained.ir()).tolerant_edge_tails[0].trailing_field,
        Some(1)
    );
    assert_eq!(retained.ir().model.bodies[0].visible, Some(true));
    assert_eq!(
        f3d_native(retained.ir()).body_native_keys[0].asm_body_key,
        Some(84)
    );
    assert_eq!(
        f3d_native(retained.ir()).body_visibilities[0].asm_body_key(),
        84
    );
    assert!(f3d_native(retained.ir()).body_visibilities[0].visible);
    assert_eq!(
        f3d_native(retained.ir()).tolerant_vertex_tails[0]
            .leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [3.5, -4.5]
    );
}

#[test]
fn tolerant_edge_and_vertex_tails_round_trip_all_trailing_forms() {
    // The tedge tail carries the serializer revision stamp, then a trailing
    // LONG present only in newer streams and taking the values 0, 1, and 2.
    // The tvertex trailing LONG shares the gate and the value set. Each form
    // must survive a write/decode cycle byte-for-byte.
    for (edge_trailing, vertex_trailing) in [
        (Some(0), Some(0)),
        (Some(1), Some(1)),
        (Some(2), Some(2)),
        (None, None),
    ] {
        let source = f3d_with_smbh(&synthetic_geometry_smbh());
        let decoded = F3dCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("generated planar triangle decode");
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less.model.vertices[0].tolerance =
            Some(cadmpeg_ir::scalar::PositiveReal::new(0.025).expect("positive finite tolerance"));
        source_less.model.edges[0].tolerance =
            Some(cadmpeg_ir::scalar::PositiveReal::new(0.035).expect("positive finite tolerance"));
        let tolerant_vertex = source_less.model.vertices[0].id.clone();
        let tolerant_edge = source_less.model.edges[0].id.clone();
        {
            let mut native = f3d_native_mut(&mut source_less);
            native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                vertex: tolerant_vertex,
                record_index: 0,
                leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
                evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Evaluated {
                    trailing: vertex_trailing,
                },
            }];
            native.tolerant_edge_tails = vec![cadmpeg_asm::brep::records::TolerantEdgeTail {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                edge: tolerant_edge,
                record_index: 0,
                entity_revision: 22800,
                trailing_field: edge_trailing,
            }];
        }
        let mut encoded = Vec::new();
        F3dCodec
            .encode(&source_less, &mut encoded)
            .expect("tolerant tail encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("tolerant tail round trip");
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_edge_tails[0].entity_revision,
            22800
        );
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_edge_tails[0].trailing_field,
            edge_trailing
        );
        assert_eq!(
            f3d_native(round_trip.ir()).tolerant_vertex_tails[0]
                .evaluated_slot
                .trailing(),
            vertex_trailing
        );
    }
}

#[test]
fn an_unset_tolerant_vertex_sentinel_round_trips_without_a_neutral_tolerance() {
    // The `-1` unset evaluated slot is a marker rather than a length: the
    // neutral vertex carries no tolerance, the native tail keeps the unset
    // fact, and generation writes the sentinel back into a tvertex record.
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let tolerant_vertex = source_less.model.vertices[0].id.clone();
    assert_eq!(source_less.model.vertices[0].tolerance, None);
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
            evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Unset {
                trailing: Some(0),
            },
        }];
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("unset tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("unset tolerant vertex round trip");
    let vertex = round_trip
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| {
            f3d_native(round_trip.ir())
                .tolerant_vertex_tails
                .iter()
                .any(|tail| tail.vertex == vertex.id)
        })
        .expect("tolerant vertex survives");
    assert_eq!(vertex.tolerance, None);
    let tail = &f3d_native(round_trip.ir()).tolerant_vertex_tails[0];
    assert_eq!(
        tail.evaluated_slot,
        cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Unset { trailing: Some(0) }
    );
    assert_eq!(
        tail.leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [-1.0, -1.0]
    );
}
#[test]
fn an_absent_tolerant_vertex_slot_round_trips_without_a_neutral_tolerance() {
    // A tvertex record that ends before the evaluated slot has no neutral
    // tolerance; the native tail keeps the absent fact and generation writes
    // only the leading slots back.
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let tolerant_vertex = source_less.model.vertices[0].id.clone();
    assert_eq!(source_less.model.vertices[0].tolerance, None);
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.tolerant_vertex_tails = vec![cadmpeg_asm::brep::records::TolerantVertexTail {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            vertex: tolerant_vertex,
            record_index: 0,
            leading_tolerances: finite_tolerance_pair([-1.0, -1.0]),
            evaluated_slot: cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Absent {},
        }];
    }
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("absent tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("absent tolerant vertex round trip");
    let vertex = round_trip
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| {
            f3d_native(round_trip.ir())
                .tolerant_vertex_tails
                .iter()
                .any(|tail| tail.vertex == vertex.id)
        })
        .expect("tolerant vertex survives");
    assert_eq!(vertex.tolerance, None);
    let tail = &f3d_native(round_trip.ir()).tolerant_vertex_tails[0];
    assert_eq!(
        tail.evaluated_slot,
        cadmpeg_asm::brep::records::EvaluatedToleranceSlot::Absent {}
    );
    assert_eq!(tail.evaluated_slot.trailing(), None);
    assert_eq!(
        tail.leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [-1.0, -1.0]
    );
}

#[test]
fn generated_source_less_f3d_reports_omitted_subds() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.subds.push(cadmpeg_ir::SubdSurface {
        native_ref: None,
        id: cadmpeg_ir::ids::SubdId::mint("test:f3d:subd#0").expect("identity grammar"),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        source_object: None,
        cage: cadmpeg_ir::subd::SubdCage::default(),
    });

    let (output, report) = synthesize_document(&source_less, None);
    assert!(output.ir().model.subds.is_empty());
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterSubdOmitted.kind()));
}

#[test]
fn generated_source_less_f3d_regenerates_unbacked_design_parameters() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less
        .model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            driving: true,
            id: cadmpeg_ir::features::ParameterId::mint("test:f3d:parameter#0")
                .expect("identity grammar"),
            owner: None,
            ordinal: 0,
            name: "Width".into(),
            expression: "60 mm".into(),
            display: None,
            value: Some(cadmpeg_ir::features::ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(60.0).unwrap(),
            )),

            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });

    let (output, _) = synthesize_document(&source_less, None);
    let parameter = output
        .ir()
        .model
        .parameters
        .iter()
        .find(|record| record.name == "Width")
        .expect("authored parameter is regenerated");
    assert_eq!(parameter.expression, "60 mm".into());
    assert_eq!(parameter.value, source_less.model.parameters[0].value);
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
}

#[test]
fn generated_source_less_f3d_writes_document_design_parameters() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let native_id = format!("f3d:{stream}:design-parameter#0");
    f3d_native_mut(&mut source_less).design_parameters.push(
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: native_id.clone(),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                    .unwrap(),
                record_index: 700,
                source_ordinal: 0,
                source: crate::records::parameters::DesignParameterSource::User {
                    family_discriminator: Some(crate::records::identity::Located {
                        value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                        offset: Some(22),
                    }),
                },
                expression: "Width / 2".into(),
                expression_offset: 36,
                source_kind_offset: 70,

                unit: Some(crate::records::identity::RecordedValue {
                    value: "mm".into(),
                    offset: 110,
                }),
                name: "HalfWidth".into(),
                name_offset: 120,
                evaluated_value: 3.0,
                evaluated_value_offset: 150,
            },
        )
        .unwrap(),
    );
    f3d_native_mut(&mut source_less).design_parameters.push(
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: format!("f3d:{stream}:design-parameter#1"),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                    .unwrap(),
                record_index: 701,
                source_ordinal: 1,
                source: crate::records::parameters::DesignParameterSource::User {
                    family_discriminator: Some(crate::records::identity::Located {
                        value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                        offset: Some(22),
                    }),
                },
                expression: "60 mm".into(),
                expression_offset: 36,
                source_kind_offset: 70,

                unit: Some(crate::records::identity::RecordedValue {
                    value: "mm".into(),
                    offset: 110,
                }),
                name: "Width".into(),
                name_offset: 120,
                evaluated_value: 6.0,
                evaluated_value_offset: 150,
            },
        )
        .unwrap(),
    );
    let crate::design::feature_project::ProjectedParameterDesign { parameters, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &f3d_native(&source_less).design_parameters,
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    source_less.model.parameters = parameters;

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less document parameter encode");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less document parameter round trip");
    let normalize = |parameters: &[cadmpeg_ir::features::DesignParameter]| {
        let identities = parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.id.clone(),
                    cadmpeg_ir::features::ParameterId::mint(format!(
                        "test:f3d:parameter#{}",
                        parameter.ordinal
                    ))
                    .unwrap(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut normalized = parameters.to_vec();
        for parameter in &mut normalized {
            parameter.id = identities[&parameter.id].clone();
            parameter.native_ref = None;
            for token in &mut parameter.expression.tokens {
                if let cadmpeg_ir::features::ExpressionToken::Parameter(id) = token {
                    *id = identities[id].clone();
                }
            }
        }
        normalized.sort_by_key(|parameter| parameter.ordinal);
        normalized
    };
    assert_eq!(
        normalize(&decoded.ir().model.parameters),
        normalize(&source_less.model.parameters)
    );
    let width = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .unwrap();
    let half = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "HalfWidth")
        .unwrap();
    assert_eq!(
        half.expression
            .dependencies()
            .cloned()
            .collect::<Vec<_>>()
            .as_slice(),
        [width.id.clone()]
    );
    let native = f3d_native(decoded.ir());
    assert_eq!(native.design_parameters.len(), 2);
    assert_eq!(
        native
            .design_parameters
            .iter()
            .find(|parameter| parameter.name() == "HalfWidth")
            .unwrap()
            .evaluated_value()
            .get(),
        3.0
    );
}

#[test]
fn generated_source_less_writes_document_tolerance_contract() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.tolerances.linear =
        cadmpeg_ir::scalar::PositiveLength::new(2.5e-7).expect("positive finite tolerance");
    source_less.tolerances.angular =
        cadmpeg_ir::scalar::PositiveAngle::new(4.0e-11).expect("positive finite tolerance");

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less tolerance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less tolerance round trip");
    assert_eq!(round_trip.ir().tolerances, source_less.tolerances);
}

#[test]
fn generated_source_less_preserves_supported_topology_tolerances_and_reports_other_loss() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();

    source_less.model.faces[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.02).expect("positive finite tolerance"));
    let (output, report) = synthesize_document(&source_less, None);
    assert_eq!(output.ir().model.faces.len(), source_less.model.faces.len());
    assert!(output.ir().model.faces[0].tolerance.is_none());
    assert!(report.losses.iter().any(|loss| loss.code
        == crate::loss::F3dLossCode::WriterAuxiliaryOmitted.kind()
        && loss.message.contains("tolerance")));

    source_less.model.faces[0].tolerance = None;
    source_less.model.edges[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.03).expect("positive finite tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("supported tolerant edge encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("supported tolerant edge round trip");
    assert_eq!(
        round_trip.ir().model.edges[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.03)
    );

    source_less.model.edges[0].tolerance = None;
    source_less.model.vertices[0].tolerance =
        Some(cadmpeg_ir::scalar::PositiveReal::new(0.04).expect("positive finite tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("supported tolerant vertex encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("supported tolerant vertex round trip");
    assert_eq!(
        round_trip.ir().model.vertices[0]
            .tolerance
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(0.04)
    );
}

#[test]
fn generated_source_less_reports_auxiliary_geometry_and_source_identity_loss() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::Tessellation;
    use cadmpeg_ir::SourceObjectAssociation;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let association = SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::CodecFormat::Step,
        object_id: cadmpeg_core::text::NonBlankString::new("object-1")
            .expect("nonempty source identity"),
        instance_path: Vec::new(),
    };

    let original_attribute_count = source_less.model.attributes.len();
    let original_view_count = source_less.model.view_presentations.len();
    let view_order = source_less
        .model
        .view_presentations
        .iter()
        .map(|view| view.order)
        .max()
        .map_or(0, |order| order.checked_add(1).unwrap());
    let presentation = |target: AttributeTarget| {
        [SourceAttribute {
            native_ref: None,
            id: "generated:test:attribute#association-name"
                .try_into()
                .unwrap(),
            target,
            name: "name".into(),
            values: vec![AttributeValue::String("exact carrier".into())],
        }]
    };
    source_less
        .model
        .attributes
        .extend(presentation(AttributeTarget::Surface(
            source_less.model.surfaces[0].id.clone(),
        )));
    source_less
        .model
        .view_presentations
        .push(cadmpeg_ir::presentation::ViewPresentation {
            id: "generated:test:view#association".try_into().unwrap(),
            object: Some(source_less.model.surfaces[0].id.to_string()),
            order: view_order,
            expanded: None,
            visible: Some(true),
            display_mode: None,
            selection_style: None,
            line_width: None,
            point_size: None,
            properties: Default::default(),
            native_ref: None,
        });
    source_less.model.surfaces[0].source_object = Some(association.clone());
    let (output, report) = synthesize_document(&source_less, None);
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
    assert!(output
        .ir()
        .model
        .surfaces
        .iter()
        .all(|record| record.source_object.is_none()));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterSourceAssociationOmitted.kind()));

    source_less.model.surfaces[0].source_object = None;
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        native_ref: None,
        parameter_range: None,
        id: "generated:test:associated-curve#0"
            .try_into()
            .expect("valid identity"),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: Some(association),
    });
    source_less
        .model
        .attributes
        .truncate(original_attribute_count);
    source_less
        .model
        .attributes
        .extend(presentation(AttributeTarget::Curve(
            source_less.model.curves.last().unwrap().id.clone(),
        )));
    source_less
        .model
        .view_presentations
        .last_mut()
        .unwrap()
        .object = Some(source_less.model.curves.last().unwrap().id.to_string());
    let (output, report) = synthesize_document(&source_less, None);
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
    assert!(output
        .ir()
        .model
        .curves
        .iter()
        .all(|record| record.source_object.is_none()));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterSourceAssociationOmitted.kind()));

    source_less.model.curves.pop();
    source_less
        .model
        .view_presentations
        .truncate(original_view_count);
    source_less
        .model
        .attributes
        .truncate(original_attribute_count);
    source_less.model.tessellations.push(
        Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("generated:test:tessellation#0")
                .expect("valid identity"),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );
    let (output, report) = synthesize_document(&source_less, None);
    assert_eq!(
        output.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
    assert!(output.ir().model.tessellations.is_empty());
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterTessellationOmitted.kind()));
}

#[test]
fn generated_source_less_rejects_body_kind_that_conflicts_with_incidence() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    assert_eq!(
        source_less.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    source_less.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Solid;

    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("open face cannot be emitted as a solid body");
    assert!(matches!(error, cadmpeg_core::CodecError::InvalidInput(_)));
}

#[test]
fn generated_source_less_planar_polygon_plans_dynamic_record_indices() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, PointId, VertexId};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();

    let point_id = PointId::mint("generated:test:point#3").expect("identity grammar");
    source_less
        .model
        .points
        .push(cadmpeg_ir::topology::Point::new(
            point_id.clone(),
            cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0))
                .expect("a finite position is a point"),
            None,
        ));
    let vertex_id = VertexId::mint("generated:test:vertex#3").expect("identity grammar");
    source_less
        .model
        .vertices
        .push(cadmpeg_ir::topology::Vertex {
            native_ref: None,
            id: vertex_id.clone(),
            point: point_id,
            tolerance: None,
        });
    let first_vertex = source_less.model.edges[0].start.clone();
    source_less.model.edges[2].end = vertex_id.clone();
    let edge_id = EdgeId::mint("generated:test:edge#3").expect("identity grammar");
    source_less.model.edges.push(cadmpeg_ir::topology::Edge {
        native_ref: None,
        id: edge_id.clone(),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(None, Some([0.0, 1.0])).unwrap(),
        start: vertex_id,
        end: first_vertex,
        tolerance: None,
    });
    let coedge_id = CoedgeId::mint("generated:test:coedge#3").expect("identity grammar");
    let loop_id = source_less.model.loops[0].id.clone();
    source_less
        .model
        .coedges
        .push(cadmpeg_ir::topology::Coedge {
            native_ref: None,
            id: coedge_id.clone(),
            owner_loop: loop_id,
            edge: edge_id,
            radial_next: coedge_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    let mut coedges = source_less.model.loops[0].coedges().to_vec();
    coedges.push(coedge_id);
    let vertex_uses = source_less.model.loops[0].anchored_vertex_uses().to_vec();
    source_less.model.loops[0]
        .replace_ring(coedges, vertex_uses)
        .expect("source-less fixture loop remains a valid ring");

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less polygon encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less polygon round trip");

    assert_eq!(round_trip.ir().model.coedges.len(), 4);
    assert_eq!(round_trip.ir().model.edges.len(), 4);
    assert_eq!(round_trip.ir().model.vertices.len(), 4);
    assert_eq!(round_trip.ir().model.points.len(), 4);
    assert_eq!(
        round_trip
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>(),
        source_less
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>()
    );
}

#[test]
fn generated_source_less_planar_face_writes_straight_edge_carriers() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();

    for index in 0..source_less.model.edges.len() {
        let edge = &source_less.model.edges[index];
        let start = source_less
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| {
                source_less
                    .model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
            })
            .unwrap()
            .position()
            .get();
        let end = source_less
            .model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| {
                source_less
                    .model
                    .points
                    .iter()
                    .find(|point| point.id == vertex.point)
            })
            .unwrap()
            .position()
            .get();
        let delta =
            cadmpeg_ir::math::Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let length = delta.norm();
        let direction =
            cadmpeg_ir::math::Vector3::new(delta.x / length, delta.y / length, delta.z / length);
        let id = CurveId::mint(format!("generated:test:curve#{index}")).expect("identity grammar");
        source_less.model.curves.push(Curve {
            native_ref: None,
            parameter_range: None,
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(start, direction).unwrap(),
            )),
            source_object: None,
        });
        source_less.model.edges[index].set_curve(Some(id)).unwrap();
        source_less.model.edges[index].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
            source_less.model.edges[index].curve().cloned(),
            Some([0.0, length]),
        )
        .unwrap();
    }

    let expected = source_less
        .model
        .curves
        .iter()
        .map(|curve| curve.geometry.clone())
        .collect::<Vec<_>>();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less line-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less line-carrier round trip");
    assert_eq!(round_trip.ir().model.curves.len(), expected.len());
    for (actual, expected) in round_trip.ir().model.curves.iter().zip(expected) {
        let (
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)),
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve_2)),
        ) = (&actual.geometry, expected)
        else {
            panic!("expected line carriers")
        };
        let actual_origin = line_curve.origin().get();
        let actual_direction = *line_curve.direction().as_raw();
        let expected_origin = line_curve_2.origin().get();
        let expected_direction = *line_curve_2.direction().as_raw();
        assert_eq!(actual_origin, expected_origin);
        assert!((actual_direction.x - expected_direction.x).abs() < 1e-14);
        assert!((actual_direction.y - expected_direction.y).abs() < 1e-14);
        assert!((actual_direction.z - expected_direction.z).abs() < 1e-14);
    }
    assert!(round_trip
        .ir()
        .model
        .edges
        .iter()
        .all(|edge| edge.curve().is_some()));
}

#[test]
fn generated_source_less_planar_face_writes_circle_edge_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let curve_id = CurveId::mint("generated:test:circle#0").expect("identity grammar");
    let expected = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            cadmpeg_ir::math::Point3::new(4.0, -2.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            6.5,
        )
        .unwrap(),
    ));
    source_less.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: curve_id.clone(),
        geometry: expected.clone(),
        source_object: None,
    });
    source_less.model.edges[0]
        .set_curve(Some(curve_id))
        .unwrap();
    source_less.model.edges[0].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
        source_less.model.edges[0].curve().cloned(),
        Some([0.25, 1.75]),
    )
    .unwrap();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less circle-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less circle-carrier round trip");
    let mut round_trip = EditableDecodeResult::from(round_trip);
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected);
    assert_eq!(
        round_trip.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.25, 1.75])
    );
    assert!(round_trip.ir().model.edges[0].curve().is_some());
    assert!(
        !cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
            .expect("validate the neutral model")
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::check::Check::Annotations)
    );
    round_trip.ir_mut().model.curves[0].geometry =
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ));
    let (output, report) = synthesize_document(round_trip.ir(), Some(round_trip.source_fidelity()));
    assert!(
        output.ir().model.curves.iter().all(|curve| matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
        )),
        "an authored line must not be replaced by the retained ellipse"
    );
    assert!(
        !output.ir().model.edges.is_empty()
            || report
                .losses
                .iter()
                .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Error)
    );
}

#[test]
fn generated_source_less_planar_face_writes_ellipse_edge_carrier() {
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::CurveId;

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let curve_id = CurveId::mint("generated:test:ellipse#0").expect("identity grammar");
    let expected = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            cadmpeg_ir::math::Point3::new(-3.0, 5.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            8.0,
            2.0,
        )
        .unwrap(),
    ));
    source_less.model.curves.push(Curve {
        native_ref: None,
        parameter_range: None,
        id: curve_id.clone(),
        geometry: expected.clone(),
        source_object: None,
    });
    source_less.model.edges[0]
        .set_curve(Some(curve_id))
        .unwrap();
    source_less.model.edges[0].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
        source_less.model.edges[0].curve().cloned(),
        Some([0.5, 2.0]),
    )
    .unwrap();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less ellipse-carrier encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less ellipse-carrier round trip");
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected);
    assert_eq!(
        round_trip.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([0.5, 2.0])
    );
    assert!(
        !cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
            .expect("validate the neutral model")
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::check::Check::Annotations)
    );
}

#[test]
fn generated_source_less_face_writes_cylinder_surface_carrier() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let expected = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            cadmpeg_ir::math::Point3::new(2.0, -4.0, 6.0),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            7.5,
        )
        .unwrap(),
    ));
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cylinder encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cylinder round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_closed_cylinder_band_keeps_compact_periodic_topology() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
        VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{
        Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };

    let mut source_less = CadIr::empty();
    let body = BodyId::mint("synthetic:cylinder-band:body#0").expect("identity grammar");
    let region = RegionId::mint("synthetic:cylinder-band:region#0").expect("identity grammar");
    let shell = ShellId::mint("synthetic:cylinder-band:shell#0").expect("identity grammar");
    let face = FaceId::mint("synthetic:cylinder-band:face#0").expect("identity grammar");
    let surface = SurfaceId::mint("synthetic:cylinder-band:surface#0").expect("identity grammar");
    let loops = [
        LoopId::mint("synthetic:cylinder-band:loop#bottom").expect("identity grammar"),
        LoopId::mint("synthetic:cylinder-band:loop#top").expect("identity grammar"),
    ];
    let coedges = [
        CoedgeId::mint("synthetic:cylinder-band:coedge#bottom").expect("identity grammar"),
        CoedgeId::mint("synthetic:cylinder-band:coedge#top").expect("identity grammar"),
    ];
    let edges = [
        EdgeId::mint("synthetic:cylinder-band:edge#bottom").expect("identity grammar"),
        EdgeId::mint("synthetic:cylinder-band:edge#top").expect("identity grammar"),
    ];
    let curves = [
        CurveId::mint("synthetic:cylinder-band:curve#bottom").expect("identity grammar"),
        CurveId::mint("synthetic:cylinder-band:curve#top").expect("identity grammar"),
    ];
    let vertices = [
        VertexId::mint("synthetic:cylinder-band:vertex#bottom").expect("identity grammar"),
        VertexId::mint("synthetic:cylinder-band:vertex#top").expect("identity grammar"),
    ];
    let points = [
        PointId::mint("synthetic:cylinder-band:point#bottom").expect("identity grammar"),
        PointId::mint("synthetic:cylinder-band:point#top").expect("identity grammar"),
    ];

    source_less.model.bodies.push(Body {
        native_ref: None,
        id: body.clone(),
        kind: BodyKind::Sheet,
        regions: vec![region.clone()],
        transform: None,
        name: Some("closed cylinder band".into()),
        color: None,
        visible: None,
    });
    source_less.model.regions.push(Region {
        native_ref: None,
        id: region.clone(),
        body,
        shells: vec![shell.clone()],
    });
    source_less
        .model
        .shells
        .push(Shell::with_face(shell.clone(), region, face.clone()));
    source_less.model.faces.push(Face {
        native_ref: None,
        id: face.clone(),
        shell,
        surface: surface.clone(),
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops.to_vec()),
        name: None,
        color: None,
        tolerance: None,
    });
    source_less.model.surfaces.push(Surface {
        native_ref: None,
        id: surface,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    for index in 0..2 {
        let z = f64_from_index(index).expect("fixture index is exact in f64") * 10.0;
        source_less.model.loops.push(Loop {
            native_ref: None,
            id: loops[index].clone(),
            face: face.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
                cadmpeg_ir::topology::LoopRing::new(vec![coedges[index].clone()], Vec::new())
                    .expect("fixture ring admission"),
            ),
        });
        source_less.model.coedges.push(Coedge {
            native_ref: None,
            id: coedges[index].clone(),
            owner_loop: loops[index].clone(),
            edge: edges[index].clone(),
            radial_next: coedges[index].clone(),
            sense: if index == 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            pcurves: Vec::new(),
            use_curve: None,
        });
        source_less.model.edges.push(Edge {
            native_ref: None,
            id: edges[index].clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(
                Some(curves[index].clone()),
                Some([-std::f64::consts::PI, std::f64::consts::PI]),
            )
            .unwrap(),
            start: vertices[index].clone(),
            end: vertices[index].clone(),
            tolerance: None,
        });
        source_less.model.curves.push(Curve {
            native_ref: None,
            parameter_range: None,
            id: curves[index].clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    Point3::new(0.0, 0.0, z),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    5.0,
                )
                .unwrap(),
            )),
            source_object: None,
        });
        source_less.model.vertices.push(Vertex {
            native_ref: None,
            id: vertices[index].clone(),
            point: points[index].clone(),
            tolerance: None,
        });
        source_less.model.points.push(Point::new(
            points[index].clone(),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(-5.0, 0.0, z))
                .expect("a finite position is a point"),
            None,
        ));
    }
    source_less.finalize();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less closed cylinder band encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less closed cylinder band round trip");

    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert_eq!(round_trip.ir().model.coedges.len(), 2);
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    assert!(
        round_trip.ir().model.edges.iter().all(|edge| {
            edge.start == edge.end
                && edge.param_range().is_some_and(|range| {
                    (range[0] + std::f64::consts::PI).abs() < 1.0e-12
                        && (range[1] - std::f64::consts::PI).abs() < 1.0e-12
                })
        }),
        "{:?}",
        round_trip.ir().model.edges
    );
    assert!(round_trip.ir().model.loops.iter().all(|loop_| {
        loop_.coedges().len() == 1
            && round_trip
                .ir()
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == loop_.coedges()[0])
                .is_some_and(|coedge| coedge.radial_next == coedge.id)
    }));
}

#[test]
fn generated_source_less_face_writes_signed_sphere_surface_carrier() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let expected = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            cadmpeg_ir::math::Point3::new(-2.0, 4.0, 8.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            -3.5,
        )
        .unwrap(),
    ));
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less sphere encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sphere round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_source_less_face_writes_cone_surface_carrier() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let expected = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            9.0,
            1.0,
            0.5,
        )
        .unwrap(),
    ));
    source_less.model.surfaces[0].geometry = expected.clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cone encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cone round trip");
    assert_eq!(round_trip.ir().model.surfaces[0].geometry, expected);
}

#[test]
fn generated_f3d_rewrites_cone_ratio_and_half_angle() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated planar triangle decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            cadmpeg_ir::math::Point3::new(1.0, 3.0, -5.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            9.0,
            0.6,
            0.5,
        )
        .unwrap(),
    ));

    let mut initial = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut initial))
        .expect("source-less cone encode");
    let retained_decode = F3dCodec
        .decode(&mut Cursor::new(initial), &DecodeOptions::default())
        .expect("generated cone decode");
    let (mut retained, _, fidelity) = retained_decode.into_parts();
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) =
        &mut retained.model.surfaces[0].geometry
    else {
        panic!("expected cone")
    };
    let origin = cone_surface.origin();
    let axis = cone_surface.frame().axis().as_raw();
    let ref_direction = cone_surface.frame().reference().as_raw();
    let radius = cone_surface.radius().get();

    let ratio = 0.4;
    let half_angle = 0.35;
    *cone_surface = cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
        *origin,
        *axis,
        *ref_direction,
        radius,
        ratio,
        half_angle,
    )
    .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&retained, &fidelity, &mut regenerated)
        .expect("cone ratio regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated cone decode");
    assert!(
        matches!(round_trip.ir().model.surfaces[0].geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
        if {
            let half_angle = cone_surface.half_angle().get();
            (cone_surface.ratio().get() == 0.4) && ((half_angle - 0.35).abs() < EPS_CONE_ANGLE)
        })
    );
}

mod closed_edges;
mod configurations;
mod family_changes;
