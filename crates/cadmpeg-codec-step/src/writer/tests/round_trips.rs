// SPDX-License-Identifier: Apache-2.0
//! STEP writer round-trip and emission tests.

#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
#![allow(unused_imports)]

use std::fmt::Write as _;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeMode, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::eval::{
    model_curve_point_by_id, model_surface_partials_by_id, model_surface_point_by_id, pcurve_uv,
};
use cadmpeg_ir::examples::unit_cube;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, SurfaceId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::{LengthUnit, Units};
use cadmpeg_ir::CadIr;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::test_support::{decode_inline, export};
use crate::{
    write_step, StepCodec, StepError, StepSchema, StepUnsupportedPolicy, StepWriteOptions,
};

fn align_sheet_edge_to_pcurve(ir: &mut CadIr, geometry: &PcurveGeometry) {
    let pcurve_id = ir.model.pcurves[0].id.clone();
    let edge_id = ir
        .model
        .coedges
        .iter()
        .find(|coedge| {
            coedge
                .pcurves
                .iter()
                .any(|pcurve| pcurve.pcurve == pcurve_id)
        })
        .expect("sheet pcurve coedge")
        .edge
        .clone();
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("sheet pcurve edge");
    let vertex_ids = [edge.start.clone(), edge.end.clone()];
    let point_ids = vertex_ids.map(|vertex_id| {
        ir.model
            .vertices
            .iter()
            .find(|vertex| vertex.id == vertex_id)
            .expect("sheet edge vertex")
            .point
            .clone()
    });
    let parameter_range = match geometry {
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => *parameter_range,
        _ => [0.0, 1.0],
    };
    let positions = parameter_range.map(|parameter| {
        let uv = pcurve_uv(geometry, parameter).expect("test pcurve endpoint");
        Point3::new(uv.u, uv.v, 0.0)
    });
    for (point_id, position) in point_ids.into_iter().zip(positions) {
        ir.model
            .points
            .iter_mut()
            .find(|point| point.id == point_id)
            .expect("sheet edge point")
            .position = position;
    }
}

/// Emit a single surface carrier in isolation and return the DATA lines joined.
fn emit_surface_only(g: &SurfaceGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::surface(&mut e, g).expect("surface geometry is writable");
    e.into_lines().join("\n")
}

/// Emit a single curve carrier in isolation and return the DATA lines joined.
fn emit_curve_only(g: &CurveGeometry) -> String {
    let mut e = crate::writer::Emitter::new();
    crate::geometry::curve(&mut e, g).expect("curve geometry is writable");
    e.into_lines().join("\n")
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
pub(crate) fn cylinder_surface_doc() -> CadIr {
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
pub(crate) fn writer_round_trips_rational_nurbs_pcurves() {
    let bytes = include_bytes!("../../../tests/fixtures/ap214_sheet.p21");
    let mut ir = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sheet")
        .into_parts()
        .0;
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![
            cadmpeg_ir::math::Point2::new(0.0, 0.0),
            cadmpeg_ir::math::Point2::new(10.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };
    let geometry = ir.model.pcurves[0].geometry.clone();
    align_sheet_edge_to_pcurve(&mut ir, &geometry);

    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write NURBS pcurve");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode NURBS pcurve");
    assert!(matches!(
        &decoded.ir().model.pcurves[0].geometry,
        cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
            degree: 1,
            control_points,
            weights: Some(weights),
            periodic: false,
            ..
        } if control_points.len() == 2 && weights == &[1.0, 2.0]
    ));
}

#[test]
fn writer_round_trips_every_exact_step_pcurve_family() {
    use cadmpeg_ir::geometry::PcurveGeometry;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::transform::Transform2;

    let bytes = include_bytes!("../../../tests/fixtures/ap214_sheet.p21");
    let template = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode sheet")
        .into_parts()
        .0;
    let x_axis = Point2::new(0.6, 0.8);
    let y_axis = Point2::new(-0.8, 0.6);
    let cases = [
        PcurveGeometry::Circle {
            center: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            radius: 4.0,
        },
        PcurveGeometry::Ellipse {
            center: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        PcurveGeometry::Parabola {
            vertex: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            focal_distance: 1.5,
        },
        PcurveGeometry::Hyperbola {
            center: Point2::new(2.0, 3.0),
            x_axis,
            y_axis,
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        PcurveGeometry::Trimmed {
            parameter_range: [0.25, 1.75],
            same_sense: true,
            basis: Box::new(PcurveGeometry::Circle {
                center: Point2::new(2.0, 3.0),
                x_axis,
                y_axis,
                radius: 4.0,
            }),
        },
        PcurveGeometry::Offset {
            distance: -0.5,
            basis: Box::new(PcurveGeometry::Line {
                origin: Point2::new(2.0, 3.0),
                direction: Point2::new(4.0, 0.0),
            }),
        },
        PcurveGeometry::Transformed {
            basis: Box::new(PcurveGeometry::Line {
                origin: Point2::new(1.0, 2.0),
                direction: Point2::new(3.0, 4.0),
            }),
            transform: Transform2 {
                rows: [[0.0, -2.0, 10.0], [2.0, 0.0, 20.0], [0.0, 0.0, 1.0]],
            },
        },
    ];

    for geometry in cases {
        let mut ir = template.clone();
        ir.model.pcurves[0].geometry = geometry.clone();
        align_sheet_edge_to_pcurve(&mut ir, &geometry);
        let mut output = Vec::new();
        write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write exact pcurve");
        let output_text = String::from_utf8(output).expect("STEP output is UTF-8");
        if matches!(&geometry, PcurveGeometry::Transformed { .. }) {
            assert!(output_text.contains("CURVE_REPLICA"));
            assert!(output_text.contains("CARTESIAN_TRANSFORMATION_OPERATOR_2D"));
        }
        let decoded = StepCodec::default()
            .decode(
                &mut Cursor::new(output_text.into_bytes()),
                &DecodeOptions::default(),
            )
            .expect("decode exact pcurve");
        assert_eq!(decoded.ir().model.pcurves[0].geometry, geometry);
        assert_eq!(decoded.ir().model.bodies.len(), 1);
        assert!(decoded
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("has no decoded surface or 2D curve")));
    }
}

#[test]
pub(crate) fn writer_round_trips_rigid_body_placements() {
    let mut ir = unit_cube();
    ir.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0, 2.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    let options = StepWriteOptions {
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&ir, &mut output, &options).expect("write placed body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode placed body");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].transform,
        ir.model.bodies[0].transform
    );
}

#[test]
pub(crate) fn writer_round_trips_product_body_ownership() {
    let mut ir = unit_cube();
    let product = cadmpeg_ir::ids::ProductDefinitionId("product-0".into());
    ir.model
        .product_definitions
        .push(cadmpeg_ir::products::ProductDefinition {
            id: product.clone(),
            kind: cadmpeg_ir::products::ProductDefinitionKind::Part,
            source_name: Some("Cube part".into()),
            label: Some("Cube part".into()),
            description: None,
            part_number: Some("PART-001".into()),
            bom_properties: std::collections::BTreeMap::default(),
            bodies: vec![ir.model.bodies[0].id.clone()],
            native_ref: None,
        });
    ir.model.occurrences.push(cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId("root-0".into()),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: product,
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Root,
        ordinal: 0,
        transform: cadmpeg_ir::transform::Transform::identity(),
        prototype_transform: cadmpeg_ir::transform::Transform::identity(),
        scale: [1.0; 3],
        name: Some("Cube root".into()),
        linked_subelements: Vec::new(),
        visible: None,
        element_component: None,
        claim_child: None,
        copy_on_change: None,
        copy_on_change_source: None,
        copy_on_change_group: None,
        copy_on_change_touched: None,
        link_transform: None,
        native_ref: None,
    });
    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        unsupported: StepUnsupportedPolicy::Reject,
        ..StepWriteOptions::default()
    };
    let mut output = Vec::new();
    write_step(&ir, &mut output, &options).expect("write product-owned body");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode product-owned body");
    assert_eq!(decoded.ir().model.product_definitions.len(), 1);
    assert_eq!(
        decoded.ir().model.product_definitions[0]
            .part_number
            .as_deref(),
        Some("PART-001")
    );
    assert_eq!(decoded.ir().model.product_definitions[0].bodies.len(), 1);
    assert_eq!(decoded.ir().model.occurrences.len(), 1);
}

#[test]
pub(crate) fn writer_round_trips_edge_based_wire_bodies() {
    let mut ir = unit_cube();
    let edge = ir.model.edges[0].clone();
    let curve = edge.curve.clone().expect("cube edge curve");
    ir.model.edges.retain(|candidate| candidate.id == edge.id);
    ir.model.curves.retain(|candidate| candidate.id == curve);
    ir.model
        .vertices
        .retain(|vertex| vertex.id == edge.start || vertex.id == edge.end);
    let point_ids = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect::<Vec<_>>();
    ir.model
        .points
        .retain(|point| point_ids.contains(&point.id));
    ir.model.coedges.clear();
    ir.model.loops.clear();
    ir.model.faces.clear();
    ir.model.surfaces.clear();
    ir.model.shells.truncate(1);
    ir.model.shells[0].faces.clear();
    ir.model.shells[0].wire_edges = vec![edge.id];
    ir.model.shells[0].free_vertices.clear();
    ir.model.regions.truncate(1);
    ir.model.regions[0].shells = vec![ir.model.shells[0].id.clone()];
    ir.model.bodies.truncate(1);
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Wire;
    ir.model.bodies[0].color = Some(cadmpeg_ir::topology::Color {
        r: 0.2,
        g: 0.4,
        b: 0.8,
        a: 1.0,
    });
    ir.model.bodies[0].regions = vec![ir.model.regions[0].id.clone()];

    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write wire body");
    let text = String::from_utf8(output.clone()).expect("wire STEP is UTF-8");
    assert!(text.contains("CURVE_STYLE"));
    assert_eq!(text.matches("STYLED_ITEM").count(), 1);
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode wire body");
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(decoded.ir().model.edges.len(), 1);
    assert_eq!(decoded.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(
        decoded.ir().model.bodies[0].color,
        Some(cadmpeg_ir::topology::Color {
            r: 0.2,
            g: 0.4,
            b: 0.8,
            a: 1.0,
        })
    );
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn writer_round_trips_standalone_points_and_curves() {
    let mut ir = unit_cube();
    ir.model.curves.truncate(1);
    ir.model.surfaces.clear();
    ir.model.bodies.clear();
    ir.model.regions.clear();
    ir.model.shells.clear();
    ir.model.faces.clear();
    ir.model.loops.clear();
    ir.model.coedges.clear();
    ir.model.edges.clear();
    ir.model.vertices.clear();

    let mut output = Vec::new();
    write_step(&ir, &mut output, &StepWriteOptions::default()).expect("write standalone geometry");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode standalone geometry");
    assert_eq!(decoded.ir().model.curves.len(), 1);
    assert_eq!(decoded.ir().model.points.len(), ir.model.points.len());
    assert!(decoded.ir().model.bodies.is_empty());
}

#[test]
pub(crate) fn ap242_writer_round_trips_indexed_tessellation_and_exact_body_link() {
    let mut ir = unit_cube();
    ir.model
        .tessellations
        .push(cadmpeg_ir::tessellation::Tessellation {
            faces: Vec::new(),
            chordal_deflection: None,
            id: "mesh-0".into(),
            body: Some(ir.model.bodies[0].id.clone()),
            source_object: None,
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2], [2, 1, 0]],
            feature_edges: Vec::new(),
            strip_lengths: Vec::new(),
            normals: vec![Vector3::new(0.0, 0.0, 1.0); 3],
            corner_normals: Vec::new(),
            triangle_groups: Vec::new(),
            texture_assignments: Vec::new(),
            channels: Vec::new(),
        });
    let options = StepWriteOptions {
        schema: StepSchema::Ap242Edition3,
        ..StepWriteOptions::default()
    };
    let mut bytes = Vec::new();
    let report = write_step(&ir, &mut bytes, &options).expect("write AP242 tessellation");
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("tessellation")));
    let text = String::from_utf8(bytes.clone()).expect("STEP text");
    assert_eq!(text.matches("TRIANGULATED_FACE(").count(), 1);

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode AP242 tessellation");
    assert_eq!(decoded.ir().model.tessellations.len(), 1);
    let mesh = &decoded.ir().model.tessellations[0];
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.triangles, [[0, 1, 2], [2, 1, 0]]);
    assert_eq!(mesh.normals.len(), 3);
    assert!(mesh.body.is_some());
}

#[test]
pub(crate) fn analytic_conics_round_trip_through_step() {
    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(0.0, 1.0, 0.0),
        focal_distance: 2.5,
    };
    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        major_direction: Vector3::new(0.0, 1.0, 0.0),
        major_radius: 4.0,
        minor_radius: 1.5,
    };
    let mut source = CadIr::empty(Units::default());
    source.model.curves.extend([
        Curve {
            id: CurveId("parabola".into()),
            geometry: parabola.clone(),
            source_object: None,
        },
        Curve {
            id: CurveId("hyperbola".into()),
            geometry: hyperbola.clone(),
            source_object: None,
        },
    ]);

    let mut output = Vec::new();
    write_step(&source, &mut output, &StepWriteOptions::default()).expect("write conics");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode conics");
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == parabola));
    assert!(decoded
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == hyperbola));
}

#[test]
pub(crate) fn standalone_geometry_uses_general_shape_representation() {
    let mut ir = CadIr::empty(Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("line".into()),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    let output = export(&ir);
    assert!(output.contains("SHAPE_REPRESENTATION('',"));
    assert!(!output.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
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
    assert_eq!(report.census.total(), buf_line_count(&buf));
    assert_eq!(report.census.counts.get("ADVANCED_FACE"), Some(&6));
    assert_eq!(report.census.counts.get("VERTEX_POINT"), Some(&8));
    // The cube is fully representable: no error/blocking losses.
    assert_eq!(report.error_count(), 0);
}

#[test]
fn writer_round_trips_binding_scoped_appearance_visibility() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let appearance = AppearanceId("test:appearance#hidden".into());
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: Some("hidden face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.8,
            g: 0.2,
            b: 0.1,
            a: 1.0,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#hidden-face".into(),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: Some(false),
        channels: std::collections::BTreeMap::new(),
    });

    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, &StepWriteOptions::default())
        .expect("write hidden appearance binding");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("INVISIBILITY"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode hidden appearance binding");
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .any(|binding| binding.visible == Some(false)));
}

#[test]
fn writer_round_trips_surface_appearance_transparency() {
    const EPS_ALPHA: f32 = 0.000_001;

    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;

    let mut ir = unit_cube();
    let appearance = AppearanceId("test:appearance#transparent".into());
    let second_appearance = AppearanceId("test:appearance#more-transparent".into());
    ir.model.appearances.push(Appearance {
        id: appearance.clone(),
        name: Some("transparent face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.8,
            g: 0.2,
            b: 0.1,
            a: 0.35,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearances.push(Appearance {
        id: second_appearance.clone(),
        name: Some("more transparent face".into()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(cadmpeg_ir::topology::Color {
            r: 0.8,
            g: 0.2,
            b: 0.1,
            a: 0.65,
        }),
        properties: std::collections::BTreeMap::new(),
        textures: Vec::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#transparent-face".into(),
        target: AppearanceTarget::Face(ir.model.faces[0].id.clone()),
        appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });
    ir.model.appearance_bindings.push(AppearanceBinding {
        id: "test:appearance-binding#more-transparent-face".into(),
        target: AppearanceTarget::Face(ir.model.faces[1].id.clone()),
        appearance: second_appearance,
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    });

    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, &StepWriteOptions::default())
        .expect("write transparent surface appearance");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("SURFACE_STYLE_TRANSPARENT"));
    assert!(text.contains("SURFACE_STYLE_RENDERING_WITH_PROPERTIES"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode transparent surface appearance");
    let alphas = decoded
        .ir()
        .model
        .appearances
        .iter()
        .filter_map(|appearance| appearance.base_color.map(|color| color.a))
        .collect::<Vec<_>>();
    assert_eq!(alphas.len(), 2);
    assert!(alphas.iter().any(|alpha| (*alpha - 0.35).abs() < EPS_ALPHA));
    assert!(alphas.iter().any(|alpha| (*alpha - 0.65).abs() < EPS_ALPHA));
}

#[test]
fn writer_round_trips_presentation_layer_visibility() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#hidden".into()),
        name: "hidden layer".into(),
        description: Some("layer visibility".into()),
        visible: Some(false),
        items: vec![PresentationItem::Body { body }],
    });

    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, &StepWriteOptions::default())
        .expect("write hidden presentation layer");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('hidden layer','layer visibility',"));
    assert!(text.contains("INVISIBILITY"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode hidden presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name == "hidden layer")
        .expect("decoded hidden presentation layer");
    assert_eq!(layer.visible, Some(false));
    assert_ne!(decoded.ir().model.bodies[0].visible, Some(false));
}

#[test]
fn writer_round_trips_empty_presentation_layer_label() {
    use cadmpeg_ir::ids::LayerId;
    use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};

    let mut ir = unit_cube();
    let body = ir.model.bodies[0].id.clone();
    ir.model.presentation_layers.push(PresentationLayer {
        id: LayerId("test:layer#unnamed".into()),
        name: String::new(),
        description: Some("unnamed layer".into()),
        visible: Some(false),
        items: vec![PresentationItem::Body { body }],
    });

    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, &StepWriteOptions::default())
        .expect("write empty-label presentation layer");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let text = String::from_utf8(output).expect("STEP output is UTF-8");
    assert!(text.contains("PRESENTATION_LAYER_ASSIGNMENT('','unnamed layer',"));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(text), &DecodeOptions::default())
        .expect("decode empty-label presentation layer");
    let layer = decoded
        .ir()
        .model
        .presentation_layers
        .iter()
        .find(|layer| layer.name.is_empty())
        .expect("decoded empty-label presentation layer");
    assert_eq!(layer.description.as_deref(), Some("unnamed layer"));
    assert_eq!(layer.visible, Some(false));
    assert!(matches!(
        layer.items.as_slice(),
        [PresentationItem::Body { .. }]
    ));
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
pub(crate) fn nurbs_surface_grid_orientation_is_u_major() {
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
fn writer_emits_both_carriers_for_mixed_general_bodies() {
    let mut ir = unit_cube();
    let edge = ir.model.edges[0].id.clone();
    ir.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::General;
    ir.model.shells[0].wire_edges = vec![edge];

    let mut output = Vec::new();
    let report = write_step(&ir, &mut output, &StepWriteOptions::default())
        .expect("write mixed general body");
    assert!(!report.losses.iter().any(|loss| {
        loss.code == StepLossCode::WireRegionNoConnectedEdgeSet.kind()
            && loss.message.contains("wire region")
    }));
    let text = String::from_utf8(output).expect("mixed general STEP is UTF-8");
    assert!(text.contains("SHELL_BASED_SURFACE_MODEL"));
    assert!(text.contains("EDGE_BASED_WIREFRAME_MODEL"));
}

#[test]
fn writer_orders_edge_loop_coedges_by_oriented_endpoints() {
    let mut source = unit_cube();
    source
        .model
        .loops
        .iter_mut()
        .find(|loop_| loop_.coedges.len() >= 3)
        .expect("unit cube has an edge loop")
        .coedges
        .swap(0, 1);

    let mut bytes = Vec::new();
    let report = write_step(&source, &mut bytes, &StepWriteOptions::default())
        .expect("writer should recover a continuous loop order");
    assert!(!report.losses.iter().any(|loss| {
        loss.code == StepLossCode::LoopNoContinuousOrdering.kind()
            && loss.severity == cadmpeg_ir::Severity::Error
            && loss.message.contains("continuous vertex-to-vertex")
    }));

    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode reordered edge loops");
    assert_eq!(decoded.ir().model.faces.len(), source.model.faces.len());
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), decoded.report().losses.clone());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn writer_declares_each_supported_target_schema_exactly() {
    for schema in [
        StepSchema::Ap203Edition1,
        StepSchema::Ap203Edition2,
        StepSchema::Ap214,
        StepSchema::Ap242Edition1,
        StepSchema::Ap242Edition2,
        StepSchema::Ap242Edition3,
    ] {
        let options = StepWriteOptions {
            schema,
            unsupported: StepUnsupportedPolicy::Reject,
            ..StepWriteOptions::default()
        };
        let mut bytes = Vec::new();
        write_step(&unit_cube(), &mut bytes, &options).expect("write target schema");
        let text = std::str::from_utf8(&bytes).expect("ASCII STEP output");
        assert!(text.contains(&format!("FILE_SCHEMA(('{}'));", schema.file_schema())));
        StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode target-schema output");
    }
}
