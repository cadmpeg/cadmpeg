// SPDX-License-Identifier: Apache-2.0

use std::{io::Cursor, path::Path};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::topology::{EdgeCarrier, LoopBoundary};
use cadmpeg_ir::{
    features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrusionDirectionSource, FeatureDefinition,
        FeatureOperation, FeatureTreeNodeRole, LinearTermination, PlanarProfileRef, ProfileRef,
    },
    sketches::{SketchConstraintDefinitionInput, SketchGeometryDefinition},
    Codec, DecodeOptions,
};
use cadmpeg_measure::test_support::round_trip::measured;

use crate::{
    brep::{Tables, TextShapeKind, TextSurface, TextTShapeGeometry},
    test_support::test_archive::assert_valid_document,
    FcstdCodec,
};

const POINT_TOLERANCE: f64 = 1.0e-12;
const VOLUME_TOLERANCE: f64 = 0.01;
const AREA_TOLERANCE: f64 = 0.01;
const BOX_TOLERANCE: f64 = 1.0e-6;

fn source_sketch_property<'a>(
    properties: &'a [crate::native::PropertyRecord],
    name: &str,
    label: &str,
) -> &'a crate::native::PropertyRecord {
    let owner = crate::native::native_id("object", "Sketch");
    properties
        .iter()
        .find(|property| property.owner == owner && property.name == name)
        .unwrap_or_else(|| panic!("{label}: missing source Sketch.{name}"))
}

#[test]
fn recipe_sketch_bspline_knots_and_points() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (variant, periodic) in [("a", true), ("b", true), ("c", false)] {
            let label = format!("recipe_{version}_N04{variant}");
            let bytes = std::fs::read(root.join(format!("{label}.FCStd"))).unwrap();
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            assert_valid_document(decoded.ir());
            let entities = &decoded.ir().model.sketch_entities;
            assert_eq!(entities.len(), 1, "{label}");
            let [sketch] = decoded.ir().model.sketches.as_slice() else {
                panic!("{label}: expected one sketch")
            };
            assert_eq!(entities[0].sketch, sketch.id, "{label}");
            let features = &decoded.ir().model.features;
            let pad = features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Pad"))
                .unwrap();
            let body_feature = features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Body"))
                .unwrap();
            let sketch_feature = features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Sketch"))
                .unwrap();
            let FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::SolidBodies,
                children,
            }) = body_feature.evaluation.definition()
            else {
                panic!("{label}: expected Body membership node")
            };
            assert_eq!(
                &**children,
                &[sketch_feature.id.clone(), pad.id.clone()],
                "{label}: Body.Group source order"
            );
            assert_eq!(
                children.active_child().as_ref(),
                Some(&pad.id),
                "{label}: Body.Tip"
            );

            let (origin, normal, u_axis) = sketch.resolved_placement().unwrap_or_else(|| {
                panic!("{label}: expected the attached sketch plane to resolve")
            });
            assert_eq!([origin.x, origin.y, origin.z], [0.0, 0.0, 0.0], "{label}");
            assert_eq!([normal.x, normal.y, normal.z], [0.0, 0.0, 1.0], "{label}");
            assert_eq!([u_axis.x, u_axis.y, u_axis.z], [1.0, 0.0, 0.0], "{label}");

            let source_properties = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap();
            let support_name = if version == "0.21.2" {
                "Support"
            } else {
                "AttachmentSupport"
            };
            let support = source_sketch_property(&source_properties, support_name, &label);
            assert_eq!(support.type_name, "App::PropertyLinkSubList", "{label}");
            let [Some(support_link)] = support.links() else {
                panic!("{label}: expected one sketch support link")
            };
            assert_eq!(
                support_link.object(),
                Some(crate::native::native_id("object", "XY_Plane").as_str()),
                "{label}"
            );
            assert_eq!(support_link.subelements(), [""], "{label}");

            let map_mode = source_sketch_property(&source_properties, "MapMode", &label);
            let [map_mode_value] = map_mode.values() else {
                panic!("{label}: expected one sketch MapMode value")
            };
            assert_eq!(map_mode_value.tag, "Integer", "{label}");
            assert_eq!(
                map_mode_value.attributes.get("value").map(String::as_str),
                Some("5"),
                "{label}: FlatFace map mode"
            );

            let offset = source_sketch_property(&source_properties, "AttachmentOffset", &label);
            assert_eq!(offset.type_name, "App::PropertyPlacement", "{label}");
            let [offset_value] = offset.values() else {
                panic!("{label}: expected one attachment offset placement")
            };
            assert_eq!(offset_value.tag, "PropertyPlacement", "{label}");
            for (field, expected) in [
                ("Px", 0.0),
                ("Py", 0.0),
                ("Pz", 0.0),
                ("Q0", 0.0),
                ("Q1", 0.0),
                ("Q2", 0.0),
                ("Q3", 1.0),
            ] {
                let actual = offset_value.attributes[field].parse::<f64>().unwrap();
                assert_eq!(actual, expected, "{label}: AttachmentOffset.{field}");
            }
            let [constraint] = decoded.ir().model.sketch_constraints.as_slice() else {
                panic!("{label}: expected one fixed constraint")
            };
            assert!(
                matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Fixed { entity } if entity == entities[0].id()),
                "{label}"
            );
            let expected_length = if variant == "b" { 6.0 } else { 5.0 };
            let FeatureDefinition::PostProcess {
                operation, refine, ..
            } = pad.evaluation.definition()
            else {
                panic!("{label}: expected the authored Pad refine flag")
            };
            assert_eq!(*refine, version != "0.21.2", "{label}: Pad.Refine");
            let FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(profile)),
                direction,
                extent: ExtrudeExtent::OneSided { side },
                op: BooleanOp::Join,
                length_along_profile_normal: Some(true),
                allow_multi_profile_faces: Some(true),
                ..
            } = operation
            else {
                panic!("{label}: expected extrusion")
            };
            assert_eq!(profile, &sketch.id, "{label}");
            let ExtrudeDirection::Explicit {
                vector,
                source: Some(ExtrusionDirectionSource::ProfileNormal {}),
            } = direction
            else {
                panic!("{label}: expected the attached sketch normal as Pad direction")
            };
            assert_eq!(
                [vector.get().x, vector.get().y, vector.get().z],
                [0.0, 0.0, 1.0],
                "{label}: Pad.Reversed and profile direction"
            );
            let LinearTermination::Blind { length } = side.termination else {
                panic!("{label}: expected blind pad")
            };
            assert!(
                (length.get() - expected_length).abs() < POINT_TOLERANCE,
                "{label}"
            );
            assert_eq!(side.draft, None, "{label}: zero Pad.TaperAngle");
            assert_eq!(pad.evaluation.outputs().len(), 1, "{label}");
            let SketchGeometryDefinition::Nurbs { curve } = entities[0].geometry.definition()
            else {
                panic!("{label}: expected neutral B-spline");
            };
            assert_eq!(curve.degree(), 2);
            assert_eq!(curve.periodic(), periodic);
            let mut poles = vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
            poles.push(poles[0]);
            if periodic {
                poles.push(poles[1]);
            }
            let points: Vec<_> = curve
                .control_points()
                .iter()
                .map(|point| point.coordinates().map(cadmpeg_ir::scalar::FiniteReal::get))
                .collect();
            assert_eq!(points, poles);
            assert_eq!(curve.pole_rows().weights(), None);
            let expected_knots: &[f64] = if periodic {
                &[-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
            } else {
                &[0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0]
            };
            assert_eq!(curve.knots().as_slice(), expected_knots);
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
            let raw_poles = curve.pole_rows().raw_points();
            let point = cadmpeg_ir::eval::nurbs_pcurve_uv(
                &ctx,
                curve.degree(),
                expected_knots,
                &raw_poles,
                None,
                0.0,
            )
            .unwrap()
            .coordinates()
            .map(cadmpeg_ir::scalar::FiniteReal::get);
            let expected = if periodic {
                [0.0, -10.0]
            } else {
                [-10.0, -10.0]
            };
            for (actual, expected) in point.into_iter().zip(expected) {
                assert!((actual - expected).abs() < POINT_TOLERANCE, "{label}");
            }
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn recipe_sketch_bspline_body_measures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    for version in ["0.21.2", "1.0.2", "1.1.4"] {
        for (variant, periodic, volume, height) in [
            ("a", true, 5000.0 / 3.0, 5.0),
            ("b", true, 2000.0, 6.0),
            ("c", false, 4750.0 / 3.0, 5.0),
        ] {
            let label = format!("recipe_{version}_N04{variant}");
            let bytes = std::fs::read(root.join(format!("{label}.FCStd"))).unwrap();
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let model = &decoded.ir().model;
            let report = measured(&label, decoded.ir()).unwrap();
            let pad = model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some("Pad"))
                .unwrap();
            assert_recipe_pad_topology(decoded.ir(), pad, periodic, height, &label);
            let [result] = pad.evaluation.outputs().as_slice() else {
                panic!("{label}: expected one pad body")
            };
            let pad_body = report
                .bodies()
                .iter()
                .find(|body| body.body() == result.as_str())
                .unwrap();
            assert!(
                pad_body
                    .volume()
                    .as_ref()
                    .and_then(|value| value.measured())
                    .is_some(),
                "{label}: pad volume"
            );
            let profile_area = if periodic { 1000.0 / 3.0 } else { 950.0 / 3.0 };
            let root_two = 2.0_f64.sqrt();
            let perimeter = if periodic {
                40.0 + 20.0 * root_two * 1.0_f64.asinh()
            } else {
                46.0 + 5.0 * root_two * 1.0_f64.asinh()
                    + 16.0 * 5.0_f64.sqrt() * (0.5_f64.asinh() + 2.0_f64.asinh()) / 5.0
            };
            let area = pad_body.area().as_ref().unwrap().measured().unwrap();
            let expected_area = 2.0 * profile_area + perimeter * height;
            assert!(
                (area.value().value.get() - expected_area).abs() < AREA_TOLERANCE,
                "{label}: area {} expected {expected_area}",
                area.value().value.get()
            );
            assert!(
                (area.value().value.get() - expected_area).abs()
                    <= area.numeric_error().get() + area.model_uncertainty().get(),
                "{label}: area {} expected {expected_area}; error {} uncertainty {}",
                area.value().value.get(),
                area.numeric_error().get(),
                area.model_uncertainty().get()
            );
            let bodies: Vec<_> = report
                .bodies()
                .iter()
                .filter(|body| {
                    body.volume()
                        .as_ref()
                        .and_then(|value| value.measured())
                        .is_some()
                })
                .collect();
            assert!(!bodies.is_empty(), "{label}: measured body");
            for body in bodies {
                let reading = body.volume().as_ref().unwrap().measured().unwrap();
                let actual = reading.value().value.get();
                assert!(
                    (actual - volume).abs() < VOLUME_TOLERANCE,
                    "{label}: volume {actual}"
                );
                assert!(
                    (actual - volume).abs()
                        <= reading.numeric_error().get() + reading.model_uncertainty().get(),
                    "{label}: volume {actual} expected {volume}; error {} uncertainty {}",
                    reading.numeric_error().get(),
                    reading.model_uncertainty().get()
                );
                if !periodic {
                    assert!((actual - 1583.33).abs() < VOLUME_TOLERANCE);
                }
                let bbox = body.bbox().measured().unwrap().value();
                for (actual, expected) in bbox
                    .min
                    .coordinates()
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
                    .into_iter()
                    .chain(
                        bbox.max
                            .coordinates()
                            .map(cadmpeg_ir::scalar::FiniteReal::get),
                    )
                    .zip([-10.0, -10.0, 0.0, 10.0, 10.0, height])
                {
                    assert!(
                        (actual - expected).abs() < BOX_TOLERANCE,
                        "{label}: bbox {actual}"
                    );
                }
            }
        }
    }
}

fn assert_recipe_pad_topology(
    ir: &cadmpeg_ir::CadIr,
    pad: &cadmpeg_ir::features::Feature,
    periodic: bool,
    height: f64,
    label: &str,
) {
    let native = ir.native.namespace("fcstd").unwrap();
    let properties = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let pad_shape_property = properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("object", "Pad") && property.name == "Shape"
        })
        .unwrap_or_else(|| panic!("{label}: Pad.Shape property"));
    let shape_payloads = native
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let source_shape = shape_payloads
        .iter()
        .find(|payload| payload.property == pad_shape_property.id)
        .unwrap_or_else(|| panic!("{label}: Pad.Shape BREP"));
    let tables = Tables::from_payload(source_shape).unwrap();

    for (kind, expected_count) in [
        (TextShapeKind::Vertex, 1),
        (TextShapeKind::Edge, 2),
        (TextShapeKind::Wire, 2),
        (TextShapeKind::Face, 2),
        (TextShapeKind::Shell, 1),
        (TextShapeKind::Solid, 1),
    ] {
        assert_eq!(
            tables
                .tshapes
                .iter()
                .filter(|shape| shape.kind() == kind)
                .count(),
            expected_count,
            "{label}: Pad.Shape source BREP {kind:?} census"
        );
    }
    let [root] = tables.roots else {
        panic!("{label}: one saved Pad.Shape root")
    };
    let solid = tables.tshapes.resolve(root.shape).unwrap();
    assert_eq!(solid.kind(), TextShapeKind::Solid, "{label}: source root");
    let [shell_use] = solid.children.as_slice() else {
        panic!("{label}: one source shell")
    };
    let source_shell = tables.tshapes.resolve(shell_use.shape).unwrap();
    assert_eq!(
        source_shell.kind(),
        TextShapeKind::Shell,
        "{label}: source shell kind"
    );
    let source_face_uses = source_shell
        .children
        .iter()
        .filter(|shape_use| {
            tables
                .tshapes
                .resolve(shape_use.shape)
                .is_ok_and(|shape| shape.kind() == TextShapeKind::Face)
        })
        .collect::<Vec<_>>();
    assert_eq!(source_face_uses.len(), 3, "{label}: saved shell face uses");
    let mut source_face_surfaces = [0, 0];
    for shape_use in &source_face_uses {
        let face = tables.tshapes.resolve(shape_use.shape).unwrap();
        let TextTShapeGeometry::Face {
            surface: Some(surface),
            ..
        } = &face.geometry
        else {
            panic!("{label}: source face has a surface")
        };
        let surface = surface.resolve(tables.surfaces).unwrap();
        let index = match surface {
            TextSurface::Extrusion { .. } => 0,
            TextSurface::Plane { .. } => 1,
            other => panic!("{label}: unexpected saved Pad face surface {other:?}"),
        };
        source_face_surfaces[index] += 1;
    }
    assert_eq!(
        source_face_surfaces,
        [1, 2],
        "{label}: source face surfaces"
    );

    let [body_id] = pad.evaluation.outputs().as_slice() else {
        panic!("{label}: one Pad output body")
    };
    let body = ir
        .model
        .bodies
        .iter()
        .find(|body| &body.id == body_id)
        .unwrap_or_else(|| panic!("{label}: Pad output body resolves"));
    assert_eq!(body.kind, cadmpeg_ir::topology::BodyKind::Solid, "{label}");
    let [region_id] = body.regions.as_slice() else {
        panic!("{label}: one output region")
    };
    let region = ir
        .model
        .regions
        .iter()
        .find(|region| &region.id == region_id)
        .unwrap_or_else(|| panic!("{label}: Pad region resolves"));
    let [shell_id] = region.shells.as_slice() else {
        panic!("{label}: one output shell")
    };
    let shell = ir
        .model
        .shells
        .iter()
        .find(|shell| &shell.id == shell_id)
        .unwrap_or_else(|| panic!("{label}: Pad shell resolves"));
    let face_ids = shell.faces();
    assert_eq!(face_ids.len(), 3, "{label}: output shell face uses");
    let mut output_face_surfaces = [0, 0];
    let mut loop_coedge_counts = Vec::new();
    let mut edge_ids = std::collections::BTreeSet::new();
    let mut vertex_ids = std::collections::BTreeSet::new();
    let mut edge_curve_kinds = [0, 0];
    for face_id in face_ids {
        let face = ir
            .model
            .faces
            .iter()
            .find(|face| &face.id == face_id)
            .unwrap_or_else(|| panic!("{label}: face resolves"));
        assert_eq!(face.shell, shell.id, "{label}: face shell owner");
        match &ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .unwrap_or_else(|| panic!("{label}: face surface resolves"))
            .geometry
        {
            SurfaceGeometry::Procedural { cache, .. } => {
                assert!(
                    matches!(cache, Some(SolvedSurfaceGeometry::Unknown { .. })),
                    "{label}: source extrusion cache"
                );
                output_face_surfaces[0] += 1;
            }
            SurfaceGeometry::Solved(geometry) if is_plane_surface(geometry) => {
                output_face_surfaces[1] += 1;
            }
            other @ SurfaceGeometry::Solved(_) => {
                panic!("{label}: unexpected decoded Pad face surface {other:?}")
            }
        }
        assert_eq!(face.loops.len(), 1, "{label}: one loop per source face");
        for loop_id in &face.loops {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| &loop_.id == loop_id)
                .unwrap_or_else(|| panic!("{label}: face loop resolves"));
            let LoopBoundary::Ring(ring) = &loop_.boundary else {
                panic!("{label}: source face loop is a coedge ring")
            };
            loop_coedge_counts.push(ring.coedges().len());
            for coedge_id in ring.coedges() {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| &coedge.id == coedge_id)
                    .unwrap_or_else(|| panic!("{label}: face coedge resolves"));
                assert_eq!(coedge.owner_loop, loop_.id, "{label}: coedge loop owner");
                edge_ids.insert(coedge.edge.as_str());
            }
        }
    }
    assert_eq!(
        output_face_surfaces,
        [1, 2],
        "{label}: decoded face surfaces"
    );
    loop_coedge_counts.sort_unstable();
    assert_eq!(loop_coedge_counts, [1, 1, 4], "{label}: face boundaries");
    assert_eq!(edge_ids.len(), 3, "{label}: unique output edges");
    for edge_id in edge_ids {
        let edge = ir
            .model
            .edges
            .iter()
            .find(|edge| edge.id.as_str() == edge_id)
            .unwrap_or_else(|| panic!("{label}: edge resolves"));
        vertex_ids.insert(edge.start.as_str());
        vertex_ids.insert(edge.end.as_str());
        let Some(curve_id) = (match &edge.carrier {
            EdgeCarrier::Curve(curve_id) | EdgeCarrier::Bounded(curve_id, _) => Some(curve_id),
            EdgeCarrier::Free | EdgeCarrier::Endpoints(_) => None,
        }) else {
            panic!("{label}: closed solid edge has a curve")
        };
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| &curve.id == curve_id)
            .unwrap_or_else(|| panic!("{label}: edge curve resolves"));
        let Some(geometry) = curve.geometry.solved() else {
            panic!("{label}: edge curve is solved")
        };
        match curve_family(geometry) {
            Some(false) => edge_curve_kinds[0] += 1,
            Some(true) => edge_curve_kinds[1] += 1,
            None => panic!("{label}: spline pad edge curve {geometry:?}"),
        }
    }
    assert_eq!(edge_curve_kinds, [1, 2], "{label}: edge curve families");
    assert_eq!(vertex_ids.len(), 2, "{label}: output edge vertices");
    let mut endpoint_positions = vertex_ids
        .iter()
        .map(|vertex_id| {
            let vertex = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id.as_str() == *vertex_id)
                .unwrap_or_else(|| panic!("{label}: output vertex resolves"));
            let point = ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .unwrap_or_else(|| panic!("{label}: output vertex point resolves"));
            point
                .position()
                .coordinates()
                .map(cadmpeg_ir::scalar::FiniteReal::get)
        })
        .collect::<Vec<_>>();
    endpoint_positions.sort_by(|left, right| left[2].total_cmp(&right[2]));
    let start_xy = if periodic {
        [0.0, -10.0]
    } else {
        [-10.0, -10.0]
    };
    for (actual, expected) in endpoint_positions.iter().zip([start_xy, start_xy]) {
        assert!((actual[0] - expected[0]).abs() < POINT_TOLERANCE, "{label}");
        assert!((actual[1] - expected[1]).abs() < POINT_TOLERANCE, "{label}");
    }
    assert!(
        (endpoint_positions[0][2]).abs() < POINT_TOLERANCE,
        "{label}: lower profile endpoint"
    );
    assert!(
        (endpoint_positions[1][2] - height).abs() < POINT_TOLERANCE,
        "{label}: upper profile endpoint"
    );
}

fn is_plane_surface(surface: &SolvedSurfaceGeometry) -> bool {
    match surface {
        SolvedSurfaceGeometry::Plane(_) => true,
        SolvedSurfaceGeometry::Transformed(placed) => is_plane_surface(placed.basis()),
        _ => false,
    }
}

fn curve_family(curve: &SolvedCurveGeometry) -> Option<bool> {
    match curve {
        SolvedCurveGeometry::Line(_) => Some(false),
        SolvedCurveGeometry::Nurbs(_) => Some(true),
        SolvedCurveGeometry::Transformed(placed) => curve_family(placed.basis()),
        _ => None,
    }
}

fn sketch_document(knots: &str, poles: &str, periodic: &str) -> String {
    format!(
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
<Geometry type="Part::GeomBSplineCurve"><BSplineCurve PolesCount="4" KnotsCount="5" Degree="2" IsPeriodic="{periodic}">
{poles}{knots}</BSplineCurve></Geometry></GeometryList></Property>
</Properties></Object></ObjectData></Document>"#
    )
}

const PERIODIC_POLES: &str = r#"<Pole X="-10" Y="-10" Z="0" Weight="1"/>
<Pole X="10" Y="-10" Z="0" Weight="2"/>
<Pole X="10" Y="10" Z="0" Weight="1"/>
<Pole X="-10" Y="10" Z="0" Weight="1"/>"#;
const PERIODIC_KNOTS: &str = r#"<Knot Value="0" Mult="1"/><Knot Value="1" Mult="1"/>
<Knot Value="2" Mult="1"/><Knot Value="3" Mult="1"/><Knot Value="4" Mult="1"/>"#;

#[test]
fn sketch_bspline_rational_periodic_weights_and_point() {
    let xml = sketch_document(PERIODIC_KNOTS, PERIODIC_POLES, "1");
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_valid_document(result.ir());
    let SketchGeometryDefinition::Nurbs { curve } =
        result.ir().model.sketch_entities[0].geometry.definition()
    else {
        panic!("neutral rational curve");
    };
    let weights = curve.pole_rows().weights().unwrap();
    assert_eq!(weights, [1.0, 2.0, 1.0, 1.0, 1.0, 2.0]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let point = cadmpeg_ir::eval::nurbs_pcurve_uv(
        &ctx,
        2,
        curve.knots().as_slice(),
        &curve.pole_rows().raw_points(),
        Some(&weights),
        0.0,
    )
    .unwrap()
    .coordinates()
    .map(cadmpeg_ir::scalar::FiniteReal::get);
    assert!((point[0] - 10.0 / 3.0).abs() < POINT_TOLERANCE);
    assert!((point[1] + 10.0).abs() < POINT_TOLERANCE);
    ctx.finish_session().unwrap();
}

#[test]
fn sketch_bspline_malformed_counts_fail() {
    let xml = sketch_document(PERIODIC_KNOTS, PERIODIC_POLES, "1");
    for invalid in [
        xml.replace("PolesCount=\"4\"", "PolesCount=\"5\""),
        xml.replace("PolesCount=\"4\"", "PolesCount=\"invalid\""),
        xml.replace("Value=\"2\" Mult=\"1\"", "Value=\"2\" Mult=\"0\""),
        xml.replace("KnotsCount=\"5\"", "KnotsCount=\"6\""),
        xml.replace("Value=\"2\" Mult=\"1\"", "Value=\"2\" Mult=\"2\""),
        xml.replace("Value=\"4\" Mult=\"1\"", "Value=\"4\" Mult=\"2\""),
        sketch_document(PERIODIC_KNOTS, PERIODIC_POLES, "0"),
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&invalid)),
                &DecodeOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn sketch_bspline_unprojected_form_has_native_loss() {
    for xml in [
        sketch_document(
            PERIODIC_KNOTS,
            &PERIODIC_POLES.replace("Z=\"0\"", "Z=\"1e-300\""),
            "1",
        ),
        sketch_document(
            PERIODIC_KNOTS,
            &PERIODIC_POLES.replace("Z=\"0\"", "Z=\"1\""),
            "1",
        ),
        sketch_document(
            &PERIODIC_KNOTS
                .replace("Value=\"0\"", "Value=\"-1e308\"")
                .replace("Value=\"4\"", "Value=\"1e308\""),
            PERIODIC_POLES,
            "1",
        ),
    ] {
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_valid_document(result.ir());
        assert!(matches!(
            result.ir().model.sketch_entities[0].geometry.definition(),
            SketchGeometryDefinition::Native { .. }
        ));
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "sketch.nurbs-native"));
    }
}

#[test]
fn sketch_bspline_keeps_near_unit_weights() {
    let xml = sketch_document(
        PERIODIC_KNOTS,
        &PERIODIC_POLES.replace("Weight=\"2\"", "Weight=\"1.0000000000000002\""),
        "1",
    );
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let SketchGeometryDefinition::Nurbs { curve } =
        result.ir().model.sketch_entities[0].geometry.definition()
    else {
        panic!("neutral rational curve");
    };
    assert_eq!(
        curve.pole_rows().weights(),
        Some(vec![
            1.0,
            1.0 + f64::EPSILON,
            1.0,
            1.0,
            1.0,
            1.0 + f64::EPSILON
        ])
    );
    assert_valid_document(result.ir());
}
