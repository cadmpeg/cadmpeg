// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{accepts_non_manifold_write_loss, accepts_procedural_reduction_loss};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::writer::same_float;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

#[test]
fn encode_regenerates_a_degraded_type_102_as_an_exact_composite_carrier() {
    for version in [IgesVersion::V4_0, IgesVersion::V5_0] {
        let decoded = IgesCodec
            .decode(
                &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
                &DecodeOptions::default(),
            )
            .unwrap();
        let plan = IgesEncoder::new(IgesWriteOptions { version }).plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        });
        let plan = plan
            .unwrap_or_else(|error| panic!("{version:?} Type 102 semantic plan refused: {error}"));
        let mut written = Vec::new();
        let report = plan.write_to(&mut written).unwrap();
        let round_trip = IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap();
        assert!(!report.losses.iter().any(|loss| {
            loss.code.taxonomy() == cadmpeg_ir::LossTaxonomy::GeometryNotTransferred
        }));
        assert!(
            round_trip
                .ir()
                .native
                .namespace("iges")
                .and_then(|namespace| namespace.arenas.get("entities"))
                .is_some_and(|entities| {
                    entities.iter().any(|record| {
                        record.field("entity_type").and_then(|value| value.as_i64()) == Some(102)
                    })
                }),
            "{version:?} output has no Type 102 entity"
        );
        assert!(round_trip.ir().model.curves.iter().any(|curve| {
            curve
                .source_object
                .as_ref()
                .and_then(|source| source.name.as_deref())
                == Some("COMPOSIT")
        }));
        let validation =
            cadmpeg_ir::validate_neutral(round_trip.ir(), round_trip.report().losses.clone());
        assert!(
            validation.is_ok(),
            "{version:?}: {:#?}",
            validation.findings
        );
    }
}

#[test]
fn encode_reverses_a_composite_constituent_as_a_directed_type_102_child() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_with_join_gap(0.001_001)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let second_start = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            edge.curve
                .as_ref()
                .is_some_and(|curve| curve.as_str() == "iges:model:curve#D3")
        })
        .expect("second Type 102 child edge")
        .start
        .clone();
    {
        let mut ir = decoded.ir_mut();
        let composite = ir
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id.as_str() == "iges:model:curve#D5")
            .expect("Type 102 composite curve");
        let CurveGeometry::Composite { segments, .. } = &mut composite.geometry else {
            panic!("expected retained Type 102 composite geometry");
        };
        segments[1].same_sense = false;
        ir.model
            .edges
            .iter_mut()
            .find(|edge| {
                edge.curve
                    .as_ref()
                    .is_some_and(|curve| curve.as_str() == "iges:model:curve#D5")
            })
            .expect("Type 102 composite edge")
            .end = second_start;
    }
    let plan = IgesEncoder::new(IgesWriteOptions {
        version: IgesVersion::V5_0,
    })
    .plan(EncodeInput {
        ir: decoded.ir(),
        fidelity: None,
    })
    .expect("reversed Type 102 child is writable");
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip.ir().model.curves.iter().any(|curve| {
        matches!(
            curve.geometry,
            CurveGeometry::Line { origin, direction }
                if same_float(origin.x, 2.0) && same_float(direction.x, -1.0)
        )
    }));
    let validation =
        cadmpeg_ir::validate_neutral(round_trip.ir(), round_trip.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_a_bounded_sheet_with_resolution_tolerances() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_with_resolution_gap_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let (global, _) = crate::global::parse(&crate::card::scan(&written).unwrap()).unwrap();
    let context = global.length_context().unwrap();
    assert_eq!(context.minimum_resolution_mm(), 0.01);

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_replays_an_unchanged_iges_source_image() {
    let bytes = point_file();
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::VerbatimReplay);
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(written, bytes);
}

#[test]
fn encode_emits_and_decodes_the_requested_legacy_iges_targets() {
    for (version, name) in [(IgesVersion::V5_1, "5.1"), (IgesVersion::V5_2, "5.2")] {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(Point {
            id: PointId(format!("point#{name}")),
            source_object: None,
            position: Point3::new(4.0, 5.0, 6.0),
        });
        let encoder = IgesEncoder::new(IgesWriteOptions { version });
        let plan = encoder
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .unwrap();
        let mut written = Vec::new();
        let report = plan.write_to(&mut written).unwrap();
        assert!(report.losses.is_empty(), "{name}: {:#?}", report.losses);

        let decoded = IgesCodec
            .decode(
                &mut Cursor::new(written.as_slice()),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            decoded.ir().source.as_ref().unwrap().attributes["iges_version"],
            name
        );
        assert_eq!(decoded.ir().model.points.len(), 1);
        assert!(
            decoded.report().losses.is_empty(),
            "{name}: {:#?}",
            decoded.report().losses
        );
    }
}

#[test]
fn encode_emits_the_versioned_point_targets_for_4_0_and_5_0() {
    for (version, name) in [(IgesVersion::V4_0, "4.0"), (IgesVersion::V5_0, "5.0")] {
        let mut ir = CadIr::empty(Units::default());
        ir.model.points.push(Point {
            id: PointId(format!("point#{name}")),
            source_object: None,
            position: Point3::new(4.0, 5.0, 6.0),
        });
        let plan = IgesEncoder::new(IgesWriteOptions { version })
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .unwrap();
        let mut written = Vec::new();
        let report = plan.write_to(&mut written).unwrap();
        assert!(report.losses.is_empty(), "{name}: {:#?}", report.losses);

        let decoded = IgesCodec
            .decode(
                &mut Cursor::new(written.as_slice()),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            decoded.ir().source.as_ref().unwrap().attributes["iges_version"],
            name
        );
        assert_eq!(decoded.ir().model.points.len(), 1);
        assert!(
            decoded.report().losses.is_empty(),
            "{name}: {:#?}",
            decoded.report()
        );
    }
}

#[test]
fn encode_emits_the_legacy_plane_target_for_4_0_and_5_0() {
    for version in [IgesVersion::V4_0, IgesVersion::V5_0] {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("surface#{version:?}")),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(4.0, 5.0, 6.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        });
        let plan = IgesEncoder::new(IgesWriteOptions { version })
            .plan(EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .unwrap_or_else(|error| panic!("{version:?}: {error}"));
        let mut written = Vec::new();
        let report = plan
            .write_to(&mut written)
            .unwrap_or_else(|error| panic!("{version:?}: {error}"));
        assert!(
            report.losses.is_empty(),
            "{version:?}: {:#?}",
            report.losses
        );

        let decoded = IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{version:?}: {error}"));
        assert_eq!(decoded.ir().model.surfaces.len(), 1, "{version:?}");
        let SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } = &decoded.ir().model.surfaces[0].geometry
        else {
            panic!("{version:?}: expected a decoded plane");
        };
        assert!(same_float(origin.x, 4.0), "{version:?}");
        assert!(same_float(origin.y, 5.0), "{version:?}");
        assert!(same_float(origin.z, 6.0), "{version:?}");
        assert_eq!(*normal, Vector3::new(1.0, 0.0, 0.0), "{version:?}");
        assert_eq!(*u_axis, Vector3::new(0.0, 1.0, 0.0), "{version:?}");
        assert!(
            decoded.report().losses.is_empty(),
            "{version:?}: {:#?}",
            decoded.report().losses
        );
        assert!(
            cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok(),
            "{version:?}"
        );

        let entities = &decoded.ir().native.namespace("iges").unwrap().arenas["entities"];
        assert!(entities.iter().any(|record| {
            record.field("entity_type").and_then(|value| value.as_i64()) == Some(108)
        }));
        assert!(!entities.iter().any(|record| {
            record.field("entity_type").and_then(|value| value.as_i64()) == Some(190)
        }));
    }
}

#[test]
fn encode_rejects_open_shells_before_iges_5_3() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_non_manifold_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    for version in [IgesVersion::V5_1, IgesVersion::V5_2] {
        let error = IgesEncoder::new(IgesWriteOptions { version })
            .plan(EncodeInput {
                ir: decoded.ir(),
                fidelity: None,
            })
            .err()
            .expect("legacy target must reject an open shell");
        assert!(
            error
                .to_string()
                .contains("does not define emitted entity Type 514 Form 2"),
            "{version:?}: {error}"
        );
    }
}

#[test]
fn encode_does_not_replay_a_source_with_the_wrong_version() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let encoder = IgesEncoder::new(IgesWriteOptions {
        version: IgesVersion::V5_2,
    });
    let plan = encoder
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: Some(decoded.source_fidelity()),
        })
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::Synthesized);

    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().source.as_ref().unwrap().attributes["iges_version"],
        "5.2"
    );
    assert_eq!(round_trip.ir().model.points.len(), 1);
}

#[test]
fn encode_regenerates_an_edited_point_from_neutral_ir() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("point#1".into()),
        source_object: None,
        position: Point3::new(4.0, 5.0, 6.0),
    });
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(report.losses.is_empty());
    let (global, _) = crate::global::parse(&crate::card::scan(&written).unwrap()).unwrap();
    assert!(global.maximum_coordinate_mm().unwrap() >= 6.0);
    assert_ne!(global.maximum_coordinate_mm().unwrap(), 1000.0);

    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(written.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        decoded.ir().model.points[0].position,
        Point3::new(4.0, 5.0, 6.0)
    );
}

#[test]
fn encode_regenerates_a_finite_line_from_neutral_ir() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, fidelity) = decoded.into_parts();
    ir.model.points[0].position.x += 1.0;
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: &ir,
            fidelity: Some(&fidelity),
        })
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::Synthesized);
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::PassthroughRecordOmitted.kind() }));
    let round_trip = IgesCodec
        .decode(
            &mut Cursor::new(written.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(round_trip.ir().model.curves.len(), 1);
    assert_eq!(round_trip.ir().model.edges.len(), 1);
    assert!(matches!(
        round_trip.ir().model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(round_trip.report().losses.is_empty());
}

#[test]
fn encode_refuses_unsupported_curve_geometry_instead_of_dropping_it() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.model.curves[0].geometry = CurveGeometry::Unknown { record: None };
    let Err(error) = IgesEncoder::default().plan(EncodeInput {
        ir: &ir,
        fidelity: None,
    }) else {
        panic!("unsupported curve geometry was accepted")
    };
    assert!(
        error
            .to_string()
            .contains("does not encode this curve geometry"),
        "{error}"
    );
}

#[test]
fn encode_refuses_an_empty_source_less_model() {
    let ir = CadIr::empty(Units::default());
    let Err(error) = IgesEncoder::default().plan(EncodeInput {
        ir: &ir,
        fidelity: None,
    }) else {
        panic!("empty semantic output was accepted")
    };
    assert!(
        error.to_string().contains("refuses an empty model"),
        "{error}"
    );
}

#[test]
fn encode_refuses_a_native_curve_without_neutral_geometry() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(line_file(0)), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.model.curves.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();
    ir.model.points.clear();
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();

    let Err(error) = IgesEncoder::default().plan(EncodeInput {
        ir: &ir,
        fidelity: None,
    }) else {
        panic!("native curve was silently omitted from semantic output")
    };
    assert!(
        error
            .to_string()
            .contains("native curve entity D1 without neutral geometry"),
        "{error}"
    );
}

#[test]
fn encode_refuses_a_native_point_without_neutral_geometry() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(point_file()), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.model.points.clear();
    ir.model.vertices.clear();
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();

    let Err(error) = IgesEncoder::default().plan(EncodeInput {
        ir: &ir,
        fidelity: None,
    }) else {
        panic!("native point was silently omitted from semantic output")
    };
    assert!(
        error
            .to_string()
            .contains("native point entity D1 without neutral geometry"),
        "{error}"
    );
}

#[test]
fn encode_refuses_a_native_surface_without_neutral_geometry() {
    let decoded = IgesCodec
        .decode(&mut Cursor::new(plane_file()), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    ir.model.surfaces.clear();

    let Err(error) = IgesEncoder::default().plan(EncodeInput {
        ir: &ir,
        fidelity: None,
    }) else {
        panic!("native surface was silently omitted from semantic output")
    };
    assert!(
        error
            .to_string()
            .contains("native surface entity D1 without neutral geometry"),
        "{error}"
    );
}

#[test]
fn encode_regenerates_supported_analytic_and_spline_curves() {
    let fixtures = [
        ("circle", circular_arc_file()),
        (
            "ellipse",
            conic_arc_file(0, b"104,0.25,0,1,0,0,-1,0,2,0,0,1;"),
        ),
        (
            "hyperbola",
            conic_arc_file(
                2,
                b"104,0.25,0,-0.1111111111111111,0,0,-1,0,2,0,3.086161269630487,3.525603580931404;",
            ),
        ),
        (
            "parabola",
            conic_arc_file(3, b"104,1,0,0,0,-4,0,0,2,1,-2,1;"),
        ),
        ("nurbs", nurbs_curve_file()),
        (
            "polyline",
            copious_data_file(11, b"106,1,2,0,0,0,1,0;", "00000000"),
        ),
    ];
    for (name, bytes) in fixtures {
        let decoded = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let plan = IgesEncoder::default()
            .plan(EncodeInput {
                ir: decoded.ir(),
                fidelity: None,
            })
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut written = Vec::new();
        plan.write_to(&mut written)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let round_trip = IgesCodec
            .decode(
                &mut Cursor::new(written.as_slice()),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            round_trip.report().losses.is_empty(),
            "{name}: {:?}",
            round_trip.report().losses
        );
        let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
        assert!(validation.is_ok(), "{name}: {:#?}", validation.findings);
    }
}

#[test]
fn encode_regenerates_planar_and_nurbs_surfaces() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("surface#plane".into()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(4.0, 5.0, 6.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("surface#nurbs".into()),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count: 2,
                v_count: 2,
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                weights: None,
                normal_reversed: false,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        },
    ]);

    let plan = IgesEncoder::new(IgesWriteOptions::default())
        .plan(EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(report.losses.is_empty(), "{:#?}", report.losses);

    let decoded = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.surfaces.len(), 2);
    let plane = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == SurfaceId("iges:model:surface#D9".into()))
        .unwrap();
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = &plane.geometry
    else {
        panic!("expected a decoded plane");
    };
    assert!((origin.x - 4.0).abs() < 1.0e-10);
    assert!((origin.y - 5.0).abs() < 1.0e-10);
    assert!((origin.z - 6.0).abs() < 1.0e-10);
    assert_eq!(*normal, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*u_axis, Vector3::new(1.0, 0.0, 0.0));
    assert!(
        decoded.report().losses.is_empty(),
        "{:#?}",
        decoded.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    let entities = &decoded.ir().native.namespace("iges").unwrap().arenas["entities"];
    assert!(entities.iter().any(|record| {
        record.field("entity_type").and_then(|value| value.as_i64()) == Some(190)
    }));
    assert!(entities.iter().any(|record| {
        record.field("entity_type").and_then(|value| value.as_i64()) == Some(128)
    }));
}

#[test]
fn encode_reduces_exact_procedural_carriers_to_solved_geometry() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(composite_ruled_surface_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.procedural_surfaces.len(), 1);
    assert_eq!(decoded.ir().model.procedural_curves.len(), 2);

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(
        report.losses.iter().any(|loss| {
            loss.code == IgesLossCode::ProceduralReduced.kind()
                && loss.message.contains("1 procedural surface definition(s)")
                && loss.message.contains("2 procedural curve definition(s)")
        }),
        "{:#?}",
        report.losses
    );
    assert!(
        report
            .losses
            .iter()
            .all(|loss| accepts_procedural_reduction_loss(loss.code.taxonomy())),
        "{:#?}",
        report.losses
    );

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.surfaces.len(), 1);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_refuses_pointer_defined_analytic_surfaces_without_brep_topology() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("surface#cylinder".into()),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("surface#cone".into()),
            geometry: SurfaceGeometry::Cone {
                origin: Point3::new(-1.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 1.0,
                ratio: 1.0,
                half_angle: std::f64::consts::FRAC_PI_6,
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("surface#sphere".into()),
            geometry: SurfaceGeometry::Sphere {
                center: Point3::new(0.0, 4.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("surface#torus".into()),
            geometry: SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 5.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 4.0,
                minor_radius: 1.0,
            },
            source_object: None,
        },
    ]);

    let error = IgesEncoder::new(IgesWriteOptions::default())
        .plan(EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .err()
        .expect("standalone pointer-defined analytic surface must be refused");
    assert!(
        error.to_string().contains(
            "requires B-rep topology for Type 192 through 198 output; no bounded Type 128 domain is available"
        ),
        "{error}"
    );
}

#[test]
fn encode_refuses_a_free_analytic_surface_beside_brep_topology() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.surfaces.push(Surface {
        id: SurfaceId("surface#free-sphere".into()),
        geometry: SurfaceGeometry::Sphere {
            center: Point3::new(10.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        },
        source_object: None,
    });

    let error = IgesEncoder::new(IgesWriteOptions::default())
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .err()
        .expect("free analytic surface must not inherit B-rep eligibility");
    assert!(
        error
            .to_string()
            .contains("analytic surface surface#free-sphere requires B-rep topology"),
        "{error}"
    );
}

#[test]
fn encode_refuses_a_cylindrical_face_with_only_a_repeated_seam() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_cylinder_seam_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    let error = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .err()
        .expect("a cylindrical face without axial bounds must be refused");
    assert!(
        error
            .to_string()
            .contains("boundary loop that repeats one seam edge without axial bounds"),
        "{error}"
    );
}

#[test]
fn encode_regenerates_a_single_face_trimmed_sheet() {
    let surface_id = SurfaceId("surface#sheet".into());
    let body_id = BodyId("body#sheet".into());
    let region_id = RegionId("region#sheet".into());
    let shell_id = ShellId("shell#sheet".into());
    let face_id = FaceId("face#sheet".into());
    let loop_id = LoopId("loop#sheet".into());
    let positions = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let point_ids = (0..4)
        .map(|index| PointId(format!("point#sheet:{index}")))
        .collect::<Vec<_>>();
    let vertex_ids = (0..4)
        .map(|index| VertexId(format!("vertex#sheet:{index}")))
        .collect::<Vec<_>>();
    let edge_ids = (0..4)
        .map(|index| EdgeId(format!("edge#sheet:{index}")))
        .collect::<Vec<_>>();
    let curve_ids = (0..4)
        .map(|index| CurveId(format!("curve#sheet:{index}")))
        .collect::<Vec<_>>();
    let coedge_ids = (0..4)
        .map(|index| CoedgeId(format!("coedge#sheet:{index}")))
        .collect::<Vec<_>>();
    let pcurve_ids = (0..4)
        .map(|index| PcurveId(format!("pcurve#sheet:{index}")))
        .collect::<Vec<_>>();
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    for (index, position) in positions.into_iter().enumerate() {
        ir.model.points.push(Point {
            id: point_ids[index].clone(),
            source_object: None,
            position,
        });
        ir.model.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: None,
        });
    }
    for index in 0..4 {
        let end = (index + 1) % 4;
        ir.model.curves.push(Curve {
            id: curve_ids[index].clone(),
            geometry: CurveGeometry::Line {
                origin: positions[index],
                direction: positions[index].vector_from(positions[end]),
            },
            source_object: None,
        });
        ir.model.edges.push(Edge {
            id: edge_ids[index].clone(),
            curve: Some(curve_ids[index].clone()),
            start: vertex_ids[index].clone(),
            end: vertex_ids[end].clone(),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        });
        let start = positions[index];
        let end_position = positions[end];
        let pcurve_end = if index == 0 {
            Point2::new(
                start.x.midpoint(end_position.x),
                start.y.midpoint(end_position.y),
            )
        } else {
            Point2::new(end_position.x, end_position.y)
        };
        ir.model.pcurves.push(Pcurve {
            id: pcurve_ids[index].clone(),
            geometry: PcurveGeometry::Nurbs {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![Point2::new(start.x, start.y), pcurve_end],
                weights: None,
                periodic: false,
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some([0.0, 1.0]),
            fit_tolerance: None,
        });
        let mut pcurve_uses = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: pcurve_ids[index].clone(),
            isoparametric: Some(false),
            parameter_range: None,
        }];
        if index == 0 {
            let midpoint = pcurve_end;
            let split_pcurve_id = PcurveId("pcurve#sheet:split".into());
            ir.model.pcurves.push(Pcurve {
                id: split_pcurve_id.clone(),
                geometry: PcurveGeometry::Nurbs {
                    degree: 1,
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    control_points: vec![midpoint, Point2::new(end_position.x, end_position.y)],
                    weights: None,
                    periodic: false,
                },
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: Some([0.0, 1.0]),
                fit_tolerance: None,
            });
            pcurve_uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve: split_pcurve_id,
                isoparametric: Some(false),
                parameter_range: None,
            });
        }
        ir.model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_ids[index].clone(),
            next: coedge_ids[end].clone(),
            previous: coedge_ids[(index + 3) % 4].clone(),
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: pcurve_uses,
            use_curve: None,
            use_curve_parameter_range: None,
        });
    }
    ir.model.loops.push(Loop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary_role: LoopBoundaryRole::Outer,
        coedges: coedge_ids.clone(),
        vertex_uses: Vec::new(),
    });
    ir.model.faces.push(Face {
        id: face_id.clone(),
        shell: shell_id.clone(),
        surface: surface_id,
        sense: Sense::Forward,
        loops: vec![loop_id],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: vec![face_id],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: region_id,
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::Sheet,
        regions: vec![RegionId("region#sheet".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });

    let reversed_order = [
        coedge_ids[3].clone(),
        coedge_ids[2].clone(),
        coedge_ids[1].clone(),
        coedge_ids[0].clone(),
    ];
    ir.model.loops[0].coedges = reversed_order.to_vec();
    for (index, coedge_id) in reversed_order.iter().enumerate() {
        let coedge = ir
            .model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == *coedge_id)
            .unwrap();
        coedge.sense = Sense::Reversed;
        coedge.next = reversed_order[(index + 1) % reversed_order.len()].clone();
        coedge.previous =
            reversed_order[(index + reversed_order.len() - 1) % reversed_order.len()].clone();
    }

    let plan = IgesEncoder::new(IgesWriteOptions::default())
        .plan(EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    assert_eq!(report.census.counts.get("102_composite_curve"), Some(&2));
    assert_eq!(
        report.census.counts.get("142_curve_on_parametric_surface"),
        Some(&1)
    );
    assert_eq!(report.census.counts.get("141_boundary"), None);
    assert_eq!(report.census.counts.get("144_trimmed_surface"), Some(&1));

    let decoded = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .any(|body| body.kind == BodyKind::Sheet));
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.loops.len(), 1);
    assert_eq!(decoded.ir().model.coedges.len(), 1);
    assert_eq!(decoded.ir().model.pcurves.len(), 1);
    assert!(
        decoded.report().losses.is_empty(),
        "{:#?}",
        decoded.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_a_decoded_trimmed_sheet_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_decoded_trimmed_sheet_inner_loop_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(trimmed_plane_with_inner_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 2);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_decoded_model_curve_bounded_sheet_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(report.census.counts.get("143_bounded_surface"), Some(&1));
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_decoded_parametric_bounded_sheet_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(parametrically_bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(report.census.counts.get("143_bounded_surface"), Some(&1));
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_declares_topology_preferences_and_hierarchy_consistently() {
    let regenerate = |source: Vec<u8>| {
        let decoded = IgesCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("source fixture decodes");
        let plan = IgesEncoder::default()
            .plan(EncodeInput {
                ir: decoded.ir(),
                fidelity: None,
            })
            .expect("fixture is semantically writable");
        let mut written = Vec::new();
        plan.write_to(&mut written).expect("write succeeds");
        IgesCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .expect("generated IGES decodes")
    };
    let parameter = |ir: &CadIr, entity_type: i64, index: usize| {
        ir.native.namespace("iges").unwrap().arenas["entities"]
            .iter()
            .find(|entity| entity.field("entity_type") == Some(entity_type.into()))
            .and_then(|entity| entity.field("parameters"))
            .and_then(|parameters| parameters.as_array().cloned())
            .and_then(|parameters| parameters.get(index).cloned())
            .and_then(|parameter| parameter["value"]["value"].as_i64())
            .expect("generated entity parameter exists")
    };

    let bounded = regenerate(parametrically_bounded_plane_file());
    assert_eq!(parameter(bounded.ir(), 141, 2), 1);

    let trimmed = regenerate(trimmed_plane_file());
    assert_eq!(parameter(trimmed.ir(), 142, 1), 0);
    assert_eq!(parameter(trimmed.ir(), 142, 5), 2);

    let brep = regenerate(explicit_tetrahedron_solid_file());
    let edge_list = brep.ir().native.namespace("iges").unwrap().arenas["entities"]
        .iter()
        .find(|entity| entity.field("entity_type") == Some(504.into()))
        .expect("generated B-rep has an edge list");
    assert_eq!(edge_list.field("subordinate_status"), Some(1.into()));
    assert_eq!(edge_list.field("hierarchy_status"), Some(1.into()));
}

#[test]
fn encode_rejects_a_bounded_sheet_with_disagreeing_pcurve_endpoints() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(parametrically_bounded_plane_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    {
        let mut ir = decoded.ir_mut();
        let pcurve = ir.model.pcurves.first_mut().unwrap();
        let PcurveGeometry::Nurbs { control_points, .. } = &mut pcurve.geometry else {
            panic!("decoded bounded-sheet pcurve is not a NURBS carrier");
        };
        control_points[0].u += 0.25;
    }

    let Err(error) = IgesEncoder::default().plan(EncodeInput {
        ir: decoded.ir(),
        fidelity: None,
    }) else {
        panic!("disagreeing pcurve endpoints were accepted")
    };
    assert!(
        error
            .to_string()
            .contains("pcurve chain endpoints disagree with its directed support edge"),
        "{error}"
    );
}

#[test]
fn encode_regenerates_decoded_multi_pcurve_bounded_sheet_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(multi_pcurve_boundary_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let coedge = decoded
        .ir()
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.id.0 == "iges:model:coedge#D11:0:0")
        .unwrap_or_else(|| panic!("losses={:#?}", decoded.report().losses));
    assert_eq!(coedge.pcurves.len(), 2);

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(report.census.counts.get("141_boundary"), Some(&1));
    assert_eq!(report.census.counts.get("143_bounded_surface"), Some(&1));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    assert_eq!(round_trip.ir().model.pcurves.len(), 2);
    let round_coedge = round_trip.ir().model.coedges.first().unwrap();
    assert_eq!(round_coedge.pcurves.len(), 2);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_a_reversed_multi_pcurve_bounded_sheet() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(multi_pcurve_boundary_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    {
        let mut ir = decoded.ir_mut();
        let coedge = ir.model.coedges.first_mut().unwrap();
        coedge.sense = Sense::Reversed;
        coedge.pcurves.reverse();
        let pcurve_ids = coedge
            .pcurves
            .iter()
            .map(|pcurve_use| pcurve_use.pcurve.clone())
            .collect::<Vec<_>>();
        for pcurve_id in pcurve_ids {
            let pcurve = ir
                .model
                .pcurves
                .iter_mut()
                .find(|pcurve| pcurve.id == pcurve_id)
                .unwrap();
            let PcurveGeometry::Nurbs { control_points, .. } = &mut pcurve.geometry else {
                panic!("decoded bounded-sheet pcurve is not a NURBS carrier");
            };
            control_points.reverse();
        }
    }

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(report.census.counts.get("141_boundary"), Some(&1));
    assert_eq!(report.census.counts.get("143_bounded_surface"), Some(&1));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.coedges.len(), 1);
    assert_eq!(round_trip.ir().model.coedges[0].sense, Sense::Reversed);
    assert_eq!(round_trip.ir().model.coedges[0].pcurves.len(), 2);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_decoded_manifold_brep_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::PassthroughRecordOmitted.kind()));
    assert_eq!(
        report.census.counts.get("186_manifold_solid_brep"),
        Some(&1)
    );
    assert_eq!(report.census.counts.get("190_pointer_plane"), Some(&4));
    assert_eq!(report.census.counts.get("123_direction"), Some(&8));
    assert_eq!(report.census.counts.get("unknown_entity"), None);
    assert_eq!(report.census.counts.get("502_vertex_list"), Some(&1));
    assert_eq!(report.census.counts.get("504_edge_list"), Some(&1));
    assert_eq!(report.census.counts.get("508_loop"), Some(&4));
    assert_eq!(report.census.counts.get("510_face"), Some(&4));
    assert_eq!(report.census.counts.get("514_shell"), Some(&1));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let body = round_trip
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Solid)
        .unwrap();
    assert_eq!(body.kind, BodyKind::Solid);
    assert_eq!(round_trip.ir().model.faces.len(), 4);
    let topology_edge_ids = round_trip
        .ir()
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(topology_edge_ids.len(), 6);
    assert_eq!(round_trip.ir().model.coedges.len(), 12);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_orients_a_source_less_brep_pcurve_for_a_reversed_edge_use() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let coedge_index = decoded
        .ir()
        .model
        .coedges
        .iter()
        .position(|coedge| coedge.sense == Sense::Reversed)
        .unwrap();
    let coedge_id = decoded.ir().model.coedges[coedge_index].id.clone();
    let edge_id = decoded.ir().model.coedges[coedge_index].edge.clone();
    let edge = decoded
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .unwrap();
    let start = decoded
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
        .and_then(|vertex| {
            decoded
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
        })
        .unwrap()
        .position;
    let end = decoded
        .ir()
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.end)
        .and_then(|vertex| {
            decoded
                .ir()
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
        })
        .unwrap()
        .position;
    let face = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| {
            face.loops.iter().any(|loop_id| {
                decoded
                    .ir()
                    .model
                    .loops
                    .iter()
                    .find(|loop_| loop_.id == *loop_id)
                    .is_some_and(|loop_| loop_.coedges.contains(&coedge_id))
            })
        })
        .unwrap();
    let surface = decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap();
    let start_uv = cadmpeg_ir::eval::analytic_surface_parameters(&surface.geometry, start).unwrap();
    let end_uv = cadmpeg_ir::eval::analytic_surface_parameters(&surface.geometry, end).unwrap();
    let pcurve_id = PcurveId("pcurve#brep:source-less".into());
    decoded.ir_mut().model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: PcurveGeometry::Nurbs {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![start_uv, end_uv],
            weights: None,
            periodic: false,
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    decoded.ir_mut().model.coedges[coedge_index]
        .pcurves
        .push(cadmpeg_ir::topology::PcurveUse {
            pcurve: pcurve_id,
            isoparametric: Some(false),
            parameter_range: None,
        });

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert_eq!(report.census.counts.get("508_loop"), Some(&4));

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert!(round_trip
        .ir()
        .model
        .coedges
        .iter()
        .any(|coedge| coedge.pcurves.len() == 1));
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_regenerates_decoded_vertex_only_pole_loop_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.loops.len(), 1);
    let loop_ = &round_trip.ir().model.loops[0];
    assert!(loop_.coedges.is_empty());
    assert_eq!(loop_.vertex_uses.len(), 1);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_preserves_an_unclassified_brep_loop_without_an_outer_marker() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.loops[0].boundary_role = LoopBoundaryRole::Unspecified;

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        round_trip.ir().model.loops[0].boundary_role,
        LoopBoundaryRole::Unspecified
    );
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
}

#[test]
fn encode_declares_the_largest_topology_tolerance_as_minimum_resolution() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_vertex_loop_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    decoded.ir_mut().model.vertices[0].tolerance = Some(0.25);

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    let (global, _) = crate::global::parse(&crate::card::scan(&written).unwrap()).unwrap();
    let context = global.length_context().unwrap();
    assert_eq!(context.minimum_resolution_mm(), 0.25);

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
}

#[test]
fn encode_regenerates_decoded_non_manifold_sheet_without_source_bytes() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_non_manifold_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).unwrap();
    assert!(
        report
            .losses
            .iter()
            .all(|loss| accepts_non_manifold_write_loss(loss.code.taxonomy())),
        "{:#?}",
        report.losses
    );

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let body = round_trip
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Sheet)
        .unwrap();
    let region = round_trip
        .ir()
        .model
        .regions
        .iter()
        .find(|region| region.id == body.regions[0])
        .unwrap();
    let shell = round_trip
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id == region.shells[0])
        .unwrap();
    assert_eq!(shell.faces.len(), 3);
    let shared_edge = round_trip
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| {
            round_trip
                .ir()
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
                .count()
                == 3
        })
        .unwrap();
    let shared_uses = round_trip
        .ir()
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == shared_edge.id)
        .collect::<Vec<_>>();
    assert_eq!(shared_uses.len(), 3);
    let shared_use_ids = shared_uses
        .iter()
        .map(|coedge| coedge.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut current = shared_uses[0];
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..3 {
        assert!(visited.insert(current.id.clone()));
        assert!(shared_use_ids.contains(&current.radial_next));
        current = round_trip
            .ir()
            .model
            .coedges
            .iter()
            .find(|coedge| coedge.id == current.radial_next)
            .unwrap();
    }
    assert_eq!(current.id, shared_uses[0].id);
    assert!(
        round_trip.report().losses.is_empty(),
        "{:#?}",
        round_trip.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn encode_places_a_brep_outer_loop_first_when_face_storage_is_reordered() {
    let mut decoded = IgesCodec
        .decode(
            &mut Cursor::new(explicit_non_manifold_open_shell_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = decoded
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.kind == BodyKind::Sheet)
        .unwrap();
    let region_id = body.regions[0].clone();
    let region = decoded
        .ir()
        .model
        .regions
        .iter()
        .find(|region| region.id == region_id)
        .unwrap();
    let shell_id = region.shells[0].clone();
    let shell = decoded
        .ir()
        .model
        .shells
        .iter()
        .find(|shell| shell.id == shell_id)
        .unwrap();
    let target_face_id = shell.faces[0].clone();
    let moved_face_id = shell.faces[2].clone();
    let outer_loop_id = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id == target_face_id)
        .unwrap()
        .loops[0]
        .clone();
    let moved_loop_id = decoded
        .ir()
        .model
        .faces
        .iter()
        .find(|face| face.id == moved_face_id)
        .unwrap()
        .loops[0]
        .clone();

    decoded
        .ir_mut()
        .model
        .faces
        .iter_mut()
        .find(|face| face.id == target_face_id)
        .unwrap()
        .loops = vec![moved_loop_id.clone(), outer_loop_id.clone()];
    decoded
        .ir_mut()
        .model
        .faces
        .retain(|face| face.id != moved_face_id);
    decoded
        .ir_mut()
        .model
        .shells
        .iter_mut()
        .find(|shell| shell.id == shell_id)
        .unwrap()
        .faces
        .retain(|face_id| *face_id != moved_face_id);
    {
        let mut ir = decoded.ir_mut();
        let moved_loop = ir
            .model
            .loops
            .iter_mut()
            .find(|loop_| loop_.id == moved_loop_id)
            .unwrap();
        moved_loop.face = target_face_id;
        moved_loop.boundary_role = LoopBoundaryRole::Inner;
    }

    let emitted_loop_ids = decoded
        .ir()
        .model
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().cloned())
        .collect::<Vec<_>>();
    let outer_loop_index = emitted_loop_ids
        .iter()
        .position(|loop_id| *loop_id == outer_loop_id)
        .unwrap();
    let moved_loop_index = emitted_loop_ids
        .iter()
        .position(|loop_id| *loop_id == moved_loop_id)
        .unwrap();

    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: decoded.ir(),
            fidelity: None,
        })
        .unwrap();
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();

    let round_trip = IgesCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    let entities = &round_trip.ir().native.namespace("iges").unwrap().arenas["entities"];
    let loop_sequences = entities
        .iter()
        .filter(|entity| entity.field("entity_type") == Some(510.into()))
        .count();
    assert_eq!(loop_sequences, 2);
    let loop_sequences = entities
        .iter()
        .filter(|entity| entity.field("entity_type") == Some(508.into()))
        .map(|entity| {
            entity
                .field("directory_sequence")
                .unwrap()
                .as_i64()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let face_parameters = entities
        .iter()
        .filter(|entity| entity.field("entity_type") == Some(510.into()))
        .find_map(|entity| {
            let parameters = entity.field("parameters")?;
            let values = parameters.as_array()?;
            (values.get(2)?["value"]["value"].as_i64() == Some(2)).then_some(parameters)
        })
        .unwrap();
    let face = face_parameters.as_array().unwrap();
    assert_eq!(face[3]["value"]["value"].as_i64(), Some(1));
    assert_eq!(
        face[4]["value"]["value"].as_i64(),
        Some(loop_sequences[outer_loop_index])
    );
    assert_eq!(
        face[5]["value"]["value"].as_i64(),
        Some(loop_sequences[moved_loop_index])
    );
    assert!(round_trip.report().losses.is_empty());
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

mod curves;
mod region_and_surface;
mod replay;
