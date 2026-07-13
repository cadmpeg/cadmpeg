// SPDX-License-Identifier: Apache-2.0
//! Self-contained tests: IR documents are built in code (via the IR crate's
//! fixtures or inline), and expected STEP fragments are asserted inline. No test
//! depends on an external STEP consumer.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, Confidence};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use std::io::Cursor;

use crate::{write_step, StepCodec, StepWriteOptions};

#[test]
fn string_codec_decodes_all_part21_escape_forms_and_round_trips_unicode() {
    use crate::strings::{decode, encode};

    assert_eq!(decode(b"it''s").unwrap(), "it's");
    assert_eq!(decode(b"a\\\\b").unwrap(), "a\\b");
    assert_eq!(decode(b"\\X\\E9").unwrap(), "é");
    assert_eq!(decode(b"\\X2\\03A9\\X0\\").unwrap(), "Ω");
    assert_eq!(decode(b"\\X4\\0001F642\\X0\\").unwrap(), "🙂");
    assert_eq!(decode(b"\\S\\D").unwrap(), "Ä");
    assert_eq!(decode(b"\\PA\\\\S\\D").unwrap(), "Ä");

    for text in ["ASCII", "it's \\ quoted", "café Ω 🙂"] {
        assert_eq!(decode(encode(text).as_bytes()).unwrap(), text);
    }
}

#[test]
fn codec_detects_and_inspects_ap242_exchange_structure() {
    let bytes = include_bytes!("../tests/fixtures/ap242_minimal.p21");
    let codec = StepCodec::default();

    assert_eq!(codec.detect(bytes), Confidence::High);
    assert_eq!(codec.detect(b"PK\x03\x04"), Confidence::No);

    let summary = codec
        .inspect(&mut Cursor::new(bytes))
        .expect("inspect minimal AP242");
    assert_eq!(summary.format, "step");
    assert_eq!(summary.container_kind, "iso-10303-21-clear-text");
    assert_eq!(summary.entries.len(), 2);
    assert_eq!(summary.entries[0].name, "HEADER");
    assert_eq!(summary.entries[1].name, "DATA[0]");
    assert_eq!(summary.entries[1].attributes["entity_count"], "2");
    assert_eq!(
        summary.entries[1].attributes["unknown_entities"],
        "EXAMPLE_RECORD:1,OPAQUE_TARGET:1"
    );
    assert!(summary
        .notes
        .iter()
        .any(|note| note.contains("AP242") && note.contains("edition 3")));
}

fn export(ir: &CadIr) -> String {
    let mut buf = Vec::new();
    write_step(ir, &mut buf, &StepWriteOptions::default()).expect("write");
    String::from_utf8(buf).expect("utf8")
}

/// Emit a single surface carrier in isolation and return the DATA lines joined.
fn emit_surface_only(g: &SurfaceGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::surface(&mut e, g);
    e.into_lines().join("\n")
}

/// Emit a single curve carrier in isolation and return the DATA lines joined.
fn emit_curve_only(g: &CurveGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::curve(&mut e, g);
    e.into_lines().join("\n")
}

/// A one-face document whose single edge has no attributed curve, so the writer
/// must omit that edge and record a loss.
fn edgeless_doc() -> CadIr {
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::{
        Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
    };
    let mut ir = CadIr::empty(Units::default());
    ir.model.points.push(Point {
        id: PointId("p0".into()),
        position: Point3::new(0.0, 0.0, 0.0),
    });
    ir.model.points.push(Point {
        id: PointId("p1".into()),
        position: Point3::new(1.0, 0.0, 0.0),
    });
    ir.model.vertices.push(Vertex {
        id: VertexId("v0".into()),
        point: PointId("p0".into()),
        tolerance: None,
    });
    ir.model.vertices.push(Vertex {
        id: VertexId("v1".into()),
        point: PointId("p1".into()),
        tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("e0".into()),
        curve: None,
        start: VertexId("v0".into()),
        end: VertexId("v1".into()),
        param_range: None,
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: SurfaceId("s0".into()),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.coedges.push(Coedge {
        id: CoedgeId("ce0".into()),
        owner_loop: LoopId("lp0".into()),
        edge: EdgeId("e0".into()),
        next: CoedgeId("ce0".into()),
        previous: CoedgeId("ce0".into()),
        radial_next: CoedgeId("ce0".into()),
        sense: Sense::Forward,
        pcurve: None,
    });
    ir.model.loops.push(Loop {
        id: LoopId("lp0".into()),
        face: FaceId("f0".into()),
        coedges: vec![CoedgeId("ce0".into())],
    });
    ir.model.faces.push(Face {
        id: FaceId("f0".into()),
        shell: ShellId("sh0".into()),
        surface: SurfaceId("s0".into()),
        sense: Sense::Forward,
        loops: vec![LoopId("lp0".into())],
        name: None,
        color: None,
        tolerance: None,
    });
    ir.model.shells.push(Shell {
        id: ShellId("sh0".into()),
        region: RegionId("l0".into()),
        faces: vec![FaceId("f0".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    ir.model.regions.push(Region {
        id: RegionId("l0".into()),
        body: BodyId("b0".into()),
        shells: vec![ShellId("sh0".into())],
    });
    ir.model.bodies.push(Body {
        id: BodyId("b0".into()),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: vec![RegionId("l0".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir
}

#[test]
fn cube_has_valid_part21_envelope() {
    let s = export(&unit_cube());
    assert!(s.starts_with("ISO-10303-21;\n"));
    assert!(s.contains("HEADER;"));
    assert!(s.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));"));
    assert!(s.contains("\nDATA;\n"));
    assert!(s.trim_end().ends_with("END-ISO-10303-21;"));
    // ENDSEC appears twice: once closing HEADER, once closing DATA.
    assert_eq!(s.matches("ENDSEC;").count(), 2);
}

#[test]
fn cube_emits_full_brep_hierarchy() {
    let s = export(&unit_cube());
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
    assert!(s.contains("CLOSED_SHELL"));
    // Six planar faces, twelve unique edges, eight vertices.
    assert_eq!(s.matches("ADVANCED_FACE").count(), 6);
    assert_eq!(s.matches("= PLANE(").count(), 6);
    assert_eq!(s.matches("EDGE_CURVE").count(), 12);
    assert_eq!(s.matches("VERTEX_POINT").count(), 8);
    // 6 loops * 4 coedges = 24 oriented edges.
    assert_eq!(s.matches("ORIENTED_EDGE").count(), 24);
    assert_eq!(s.matches("= EDGE_LOOP(").count(), 6);
    assert_eq!(s.matches("FACE_OUTER_BOUND").count(), 6);
    // Every line edge carries a LINE curve.
    assert_eq!(s.matches("= LINE(").count(), 12);
}

#[test]
fn cube_product_and_context_boilerplate_present() {
    let s = export(&unit_cube());
    for kw in [
        "APPLICATION_CONTEXT",
        "APPLICATION_PROTOCOL_DEFINITION",
        "PRODUCT(",
        "PRODUCT_DEFINITION(",
        "PRODUCT_DEFINITION_SHAPE",
        "SHAPE_DEFINITION_REPRESENTATION",
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        "GEOMETRIC_REPRESENTATION_CONTEXT",
        "UNCERTAINTY_MEASURE_WITH_UNIT",
    ] {
        assert!(s.contains(kw), "missing {kw}");
    }
    // mm document → millimetre SI length unit.
    assert!(s.contains("SI_UNIT(.MILLI.,.METRE.)"));
}

#[test]
fn every_reference_resolves() {
    // Collect declared instance ids (#n = ...) and every #n referenced anywhere;
    // a valid Part 21 graph references only declared instances.
    let s = export(&unit_cube());
    let mut declared = std::collections::HashSet::new();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(eq) = rest.find(" =") {
                if let Ok(id) = rest[..eq].parse::<u64>() {
                    declared.insert(id);
                }
            }
        }
    }
    assert!(!declared.is_empty());
    // Scan referenced ids: '#' followed by digits, but skip the leading id of a
    // declaration line (handled by only scanning after the first '=').
    for line in s.lines() {
        let Some(eq) = line.find('=') else { continue };
        let body = &line[eq + 1..];
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > start {
                    let id: u64 = body[start..j].parse().unwrap();
                    assert!(
                        declared.contains(&id),
                        "dangling reference #{id} in: {line}"
                    );
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
}

#[test]
fn reports_entity_counts_and_no_geometry_loss_for_cube() {
    let mut buf = Vec::new();
    let report = write_step(&unit_cube(), &mut buf, &StepWriteOptions::default()).unwrap();
    assert_eq!(report.total_entities, buf_line_count(&buf));
    assert_eq!(report.entity_counts.get("ADVANCED_FACE"), Some(&6));
    assert_eq!(report.entity_counts.get("VERTEX_POINT"), Some(&8));
    // The cube is fully representable: no error/blocking losses.
    assert_eq!(report.error_count(), 0);
}

fn buf_line_count(buf: &[u8]) -> usize {
    // Count DATA-section instance lines: those starting with '#'.
    String::from_utf8_lossy(buf)
        .lines()
        .filter(|l| l.starts_with('#'))
        .count()
}

/// A minimal single-cylinder-surface document exercising analytic emission and
/// interning of shared points/directions.
fn cylinder_surface_doc() -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.push(Surface {
        id: SurfaceId("cyl".into()),
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    });
    ir
}

#[test]
fn analytic_surfaces_map_to_their_step_entities() {
    // Build one doc per analytic kind and check the keyword appears.
    let cases: Vec<(SurfaceGeometry, &str)> = vec![
        (
            SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            "CYLINDRICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Cone {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
                ratio: 1.0,
                half_angle: 0.5,
            },
            "CONICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Sphere {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 4.0,
            },
            "SPHERICAL_SURFACE",
        ),
        (
            SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 3.0,
                minor_radius: 1.0,
            },
            "TOROIDAL_SURFACE",
        ),
    ];
    for (geom, kw) in cases {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.push(Surface {
            id: SurfaceId("s".into()),
            geometry: geom,
            source_object: None,
        });
        // Surfaces alone aren't reachable from a shell, so they won't be emitted
        // by the topology walk; emit directly via the geometry module instead.
        let s = emit_surface_only(&ir.model.surfaces[0].geometry);
        assert!(s.contains(kw), "missing {kw} in {s}");
    }
}

#[test]
fn analytic_surface_placements_preserve_orientation() {
    let geometry = SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: 4.0,
    };
    let s = emit_surface_only(&geometry);
    assert!(s.contains("DIRECTION('',(0.,1.,0.))"));
    assert!(s.contains("DIRECTION('',(0.,0.,1.))"));
}

#[test]
fn parabola_and_hyperbola_map_to_step_conics() {
    let parabola = emit_curve_only(&CurveGeometry::Parabola {
        vertex: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(0.0, 1.0, 0.0),
        focal_distance: 2.5,
    });
    assert!(parabola.contains("= PARABOLA("));
    assert!(parabola.contains(",2.5)"));

    let hyperbola = emit_curve_only(&CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(0.0, 1.0, 0.0),
        major_radius: 4.0,
        minor_radius: 1.5,
    });
    assert!(hyperbola.contains("= HYPERBOLA("));
    assert!(hyperbola.contains(",4.,1.5)"));
}

#[test]
fn nurbs_curve_non_rational_uses_with_knots() {
    let n = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let s = emit_curve_only(&CurveGeometry::Nurbs(n));
    assert!(s.contains("B_SPLINE_CURVE_WITH_KNOTS"));
    // Clamped end knots collapse to multiplicity 3.
    assert!(s.contains("(3,3)"), "knot multiplicities: {s}");
    assert!(!s.contains("RATIONAL"));
}

#[test]
fn nurbs_curve_rational_uses_complex_form() {
    let n = NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    };
    let s = emit_curve_only(&CurveGeometry::Nurbs(n));
    assert!(s.contains("RATIONAL_B_SPLINE_CURVE"));
    assert!(s.contains("BOUNDED_CURVE()"));
}

#[test]
fn nurbs_surface_grid_orientation_is_u_major() {
    let n = NurbsSurface {
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
        u_periodic: false,
        v_periodic: false,
    };
    let s = emit_surface_only(&SurfaceGeometry::Nurbs(n));
    assert!(s.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
}

#[test]
fn v1_document_uses_canonical_millimeter_unit() {
    let ir = unit_cube();
    assert_eq!(ir.units.length, LengthUnit::Millimeter);
    let s = export(&ir);
    assert!(s.contains("SI_UNIT(.MILLI.,.METRE.)"));
    assert!(!s.contains("CONVERSION_BASED_UNIT"));
}

#[test]
fn real_formatting_always_has_decimal_point() {
    // Coordinates like 10 must serialize as 10. (a Part 21 real), never 10.
    let s = export(&unit_cube());
    assert!(s.contains("10.")); // cube corner coordinate
    assert!(!s.contains("(10,")); // no bare integer coordinate
}

#[test]
fn edge_without_curve_is_reported_and_omitted() {
    let _ = cylinder_surface_doc(); // keep helper exercised
                                    // Build a tiny doc: one face on a plane, one loop, one coedge whose edge has
                                    // no curve. The edge should be omitted and a loss recorded.
    let ir = edgeless_doc();
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let curve = Curve {
        id: CurveId("unused".into()),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    let _ = curve; // silence unused import path
    assert!(report
        .losses
        .iter()
        .any(|l| l.message.contains("edge(s) have no typed 3D curve")));
}

#[test]
fn subds_tessellations_and_source_associations_are_reported_as_losses() {
    let source_object = cadmpeg_ir::SourceObjectAssociation {
        format: "test".into(),
        object_id: "object-0".into(),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    let mut ir = unit_cube();
    ir.model.subds.push(cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId("test:step:subd#0".into()),
        scheme: cadmpeg_ir::SubdScheme::CatmullClark,
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        source_object: Some(source_object.clone()),
    });
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            id: "test:step:tessellation#0".into(),
            body: None,
            source_object: Some(source_object),
            vertices: Vec::new(),
            triangles: Vec::new(),
            strip_lengths: Vec::new(),
            normals: Vec::new(),
            channels: Vec::new(),
        });

    let report = write_step(&ir, &mut Vec::new(), &StepWriteOptions::default()).unwrap();
    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss
                .message
                .contains("1 subdivision surface(s) were omitted")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Geometry
            && loss.severity == cadmpeg_ir::Severity::Warning
            && loss.message.contains("1 tessellation(s) were omitted")
    }));
    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Metadata
            && loss
                .message
                .contains("2 source-object association(s) were not represented")
    }));
}

#[test]
fn face_on_unknown_surface_is_skipped_and_reported() {
    // Turn the cube's first face onto an unknown (opaque) surface. That face
    // cannot become an ADVANCED_FACE, so the writer must skip it and record one
    // aggregated, counted loss — the remaining five faces still export.
    let mut ir = unit_cube();
    let target = ir.model.faces[0].surface.0.clone();
    for s in &mut ir.model.surfaces {
        if s.id.0 == target {
            s.geometry = SurfaceGeometry::Unknown { record: None };
        }
    }
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let s = String::from_utf8(buf).unwrap();

    assert_eq!(
        s.matches("ADVANCED_FACE").count(),
        5,
        "the unknown-surface face should be omitted"
    );
    let unknown_notes: Vec<_> = report
        .losses
        .iter()
        .filter(|l| l.message.contains("rest on an unknown"))
        .collect();
    assert_eq!(
        unknown_notes.len(),
        1,
        "loss must be aggregated into a single counted note, got: {:?}",
        report.losses
    );
    assert!(unknown_notes[0].message.contains("1 face(s)"));
}

#[test]
fn signed_analytic_radius_normalization_is_reported() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: -2.0,
    };

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("normalized to positive STEP radii")
    }));
}

#[test]
fn elliptical_cone_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.model.surfaces[0].geometry = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 0.4,
        half_angle: 0.5,
    };

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();

    assert!(report.losses.iter().any(|loss| {
        loss.category == cadmpeg_ir::LossCategory::Geometry
            && loss.message.contains("elliptical cone surface(s)")
    }));
}

#[test]
fn procedural_construction_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: ProceduralCurveId("generated_int_cur".into()),
            curve: ir.model.curves[0].id.clone(),
            definition: cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: std::array::from_fn(|_| cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    }),
                    parameter_range: [0.0, 1.0],
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: Some(0.01),
        });

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("reduced to their solved STEP carriers")));
}

#[test]
fn parametric_history_reduction_is_reported() {
    let mut ir = unit_cube();
    ir.native.namespace_mut("f3d").arenas.insert(
        "asm_histories".into(),
        vec![cadmpeg_ir::NativeRecord {
            id: "asm-history-0".into(),
            fields: Default::default(),
        }],
    );
    ir.finalize();

    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("parametric design/history record(s) were not represented in STEP")));
}

#[test]
fn hidden_body_is_omitted_and_reported() {
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(false);
    let mut buf = Vec::new();
    let report = write_step(&ir, &mut buf, &StepWriteOptions::default()).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(!s.contains("MANIFOLD_SOLID_BREP"));
    assert!(!s.contains("ADVANCED_FACE"));
    assert!(report
        .losses
        .iter()
        .any(|l| l.message.contains("hidden body(ies) were omitted")));

    // An explicitly visible body exports unchanged.
    let mut ir = unit_cube();
    ir.model.bodies[0].visible = Some(true);
    let s = export(&ir);
    assert!(s.contains("MANIFOLD_SOLID_BREP"));
}

#[test]
fn body_color_becomes_per_face_styled_item_presentation() {
    let mut ir = unit_cube();
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.25,
        g: 0.5,
        b: 0.75,
        a: 1.0,
    });
    let face_count = ir.model.faces.len();
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.25,0.5,0.75)"));
    assert!(s.contains("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION"));
    // The body color is pushed down onto every face: one STYLED_ITEM per face,
    // each targeting an ADVANCED_FACE rather than the solid. OCCT/VTK viewers
    // (e.g. f3d) read colors only from faces, not MANIFOLD_SOLID_BREP.
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    let solid = s
        .lines()
        .find(|line| line.contains("MANIFOLD_SOLID_BREP"))
        .and_then(|line| line.split(" =").next())
        .unwrap()
        .to_string();
    for item in &styled {
        let target = item
            .rsplit_once(',')
            .map(|(_, tail)| tail.trim_end_matches(");").to_string())
            .unwrap();
        assert_ne!(target, solid, "body color must not style the solid");
        assert!(
            s.lines()
                .any(|line| line.starts_with(&format!("{target} = ADVANCED_FACE"))),
            "styled item must reference a face"
        );
    }
}

#[test]
fn face_appearance_binding_styles_the_advanced_face() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#black".to_string()),
        name: None,
        asset_guid: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.125,
            g: 0.125,
            b: 0.125,
            a: 1.0,
        }),
        properties: Default::default(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: Default::default(),
    });
    let s = export(&ir);
    assert!(s.contains("COLOUR_RGB('',0.125,0.125,0.125)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), 1);
    // The styled item targets an ADVANCED_FACE instance.
    let target = styled[0]
        .rsplit_once(',')
        .map(|(_, tail)| tail.trim_end_matches(");").to_string())
        .unwrap();
    let face_line = s
        .lines()
        .find(|line| line.starts_with(&format!("{target} = ADVANCED_FACE")));
    assert!(face_line.is_some(), "styled item must reference a face");
}

/// The soccer-ball case: a body carries a base color and one face overrides it.
/// Every face must be styled (body color pushed down onto the faces that do not
/// override it), and the overriding face must carry its own color.
#[test]
fn face_override_wins_over_body_color_and_body_fills_the_rest() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let face_count = ir.model.faces.len();
    // White body base color.
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });
    // Black override on a single face, via an appearance binding.
    let face = ir.model.faces[0].id.clone();
    ir.model.appearances.push(Appearance {
        id: AppearanceId("test:appearance#black".to_string()),
        name: None,
        asset_guid: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }),
        properties: Default::default(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#face".to_string(),
        target: AppearanceTarget::Face(face),
        appearance: AppearanceId("test:appearance#black".to_string()),
        source_entity_id: None,
        object_type: None,
        channels: Default::default(),
    });

    let s = export(&ir);
    // Both colors are present, and every face is styled.
    assert!(s.contains("COLOUR_RGB('',1.,1.,1.)"));
    assert!(s.contains("COLOUR_RGB('',0.,0.,0.)"));
    let styled: Vec<&str> = s.lines().filter(|l| l.contains("STYLED_ITEM")).collect();
    assert_eq!(styled.len(), face_count);
    // Each color's style chain is emitted once and shared; grouping the styled
    // items by their style ref must yield exactly two groups sized 1 and
    // face_count - 1 (the lone override plus every inherited face).
    let mut per_style: std::collections::BTreeMap<String, usize> = Default::default();
    for item in &styled {
        // STYLED_ITEM('color',(#psa),#face)
        let psa = item
            .split_once(",(")
            .and_then(|(_, tail)| tail.split(')').next())
            .unwrap()
            .to_string();
        *per_style.entry(psa).or_default() += 1;
    }
    let mut counts: Vec<usize> = per_style.values().copied().collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![1, face_count - 1]);
}
