// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry across an `FCStd` patch write, compared without
//! entity identities.

use std::io::Cursor;
use std::path::Path;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::test_support::round_trip::{measured, measures_agree, measures_decide};

use crate::FcstdCodec;

/// Corpus documents whose measures the patch round trip compares: all but
/// `application_payloads`, whose bodies compare nothing,
/// `core_design_product` and `product_assembly`, whose occurrences place
/// their bodies, and `core_operations`, whose body measures no comparable
/// quantity.
const MEASURED_FIXTURE_FLOOR: usize = 8;

/// The document label that the patch write stores.
const EDITED_LABEL: &str = "measured round trip";
const VOLUME_TOLERANCE: f64 = 1.0e-6;
const AREA_TOLERANCE: f64 = 1.0e-6;
const BOX_TOLERANCE: f64 = 1.0e-6;

#[test]
fn authored_box_and_cylinder_have_their_scripted_measures() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    for (name, volume, area, min, max) in [
        (
            "uniform_appearance",
            10.0 * 20.0 * 30.0,
            2.0 * (10.0 * 20.0 + 20.0 * 30.0 + 10.0 * 30.0),
            [0.0, 0.0, 0.0],
            [10.0, 20.0, 30.0],
        ),
        (
            "external_component",
            std::f64::consts::PI * 5.0 * 5.0 * 4.0,
            2.0 * std::f64::consts::PI * 5.0 * (5.0 + 4.0),
            [-5.0, -5.0, 0.0],
            [5.0, 5.0, 4.0],
        ),
    ] {
        let bytes = std::fs::read(fixtures.join(format!("{name}.FCStd"))).unwrap();
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let report = measured(name, decoded.ir()).unwrap();
        assert_eq!(report.bodies().len(), 1, "{name}");
        let body = &report.bodies()[0];
        let actual_volume = body
            .volume()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        let actual_area = body
            .area()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (actual_volume - volume).abs() < VOLUME_TOLERANCE,
            "{name}: volume {actual_volume}"
        );
        assert!(
            (actual_area - area).abs() < AREA_TOLERANCE,
            "{name}: area {actual_area}"
        );
        let bbox = body.bbox().measured().unwrap().value();
        for (actual, expected) in bbox
            .min
            .coordinates()
            .into_iter()
            .chain(bbox.max.coordinates())
            .zip(min.into_iter().chain(max))
        {
            assert!(
                (actual.get() - expected).abs() < BOX_TOLERANCE,
                "{name}: bounding box"
            );
        }
    }
}

/// Every corpus document that measures a comparable body keeps its
/// measured geometry when an edit of its document label forces the patch
/// writer to repack the archive.
///
/// The patch rewrites only `Document.xml` and repacks the retained BREP
/// entry bytes unchanged, so this measures the repack and the decode of
/// those bytes, not a write of BREP geometry.
#[test]
fn patched_corpus_documents_keep_measured_geometry() {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/freecad_fcstd/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("FreeCAD corpus fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "FCStd")
        })
        .collect();
    paths.sort();
    let mut measured = Vec::new();
    for path in paths {
        let label = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture name")
            .to_owned();
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|_| panic!("{label}: the fixture does not decode"));
        if !measures_decide(&label, decoded.ir()).expect("the fixture is measured") {
            continue;
        }
        let mut edited = decoded.ir().clone();
        FcstdCodec
            .set_property_value_attribute(
                &mut edited,
                crate::FcstdPropertyOwner::Document,
                "Label",
                0,
                "value",
                EDITED_LABEL,
            )
            .unwrap_or_else(|_| panic!("{label}: the document label is not editable"));
        let mut encoded = Vec::new();
        let plan = FcstdCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
            .unwrap_or_else(|_| panic!("{label}: the edit is not planned"));
        assert!(
            matches!(plan.report().write_path(), WritePath::Patched { .. }),
            "{label}: the label edit does not take the patch path"
        );
        plan.write_to(&mut encoded)
            .unwrap_or_else(|_| panic!("{label}: the patch write fails"));
        let round_trip = FcstdCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|_| panic!("{label}: the patched archive does not decode"));
        assert_eq!(
            round_trip.ir().model.bodies.len(),
            decoded.ir().model.bodies.len(),
            "{label}: the body count changes"
        );
        measures_agree(&label, decoded.ir(), round_trip.ir())
            .expect("the round trip keeps its measured geometry");
        measured.push(label);
    }
    assert!(
        measured.len() >= MEASURED_FIXTURE_FLOOR,
        "fewer corpus documents than the floor measure a comparable body through the patch writer"
    );
    for expected in ["binary_exact_shape", "geometry_topology"] {
        assert!(
            measured.iter().any(|label| label == expected),
            "an exact-shape corpus document is not measured"
        );
    }
}
#[test]
fn authored_geometry_topology_measures_match_dimensions() {
    use cadmpeg_ir::{Codec, DecodeOptions};
    use cadmpeg_measure::test_support::round_trip::measured;
    use std::io::Cursor;

    const VOLUME_TOLERANCE: f64 = 1.0e-5;
    const AREA_TOLERANCE: f64 = 1.0e-5;
    const LENGTH_TOLERANCE: f64 = 1.0e-6;
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd");
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let report = measured("geometry_topology", decoded.ir()).unwrap();
    let row = |name: &str| {
        let feature = decoded
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap();
        let [id] = feature.evaluation.outputs().as_slice() else {
            panic!("{name}: expected one result body")
        };
        report
            .bodies()
            .iter()
            .find(|body| body.body() == id.as_str())
            .unwrap()
    };
    let pi = std::f64::consts::PI;
    for (name, volume, area) in [
        (
            "Solid",
            20.0 * 15.0 * 10.0,
            2.0 * (20.0 * 15.0 + 20.0 * 10.0 + 15.0 * 10.0),
        ),
        (
            "Void",
            20.0_f64.powi(3) - 10.0_f64.powi(3),
            6.0 * (20.0_f64.powi(2) + 10.0_f64.powi(2)),
        ),
        (
            "Seam",
            pi * 6.0_f64.powi(2) * 18.0,
            2.0 * pi * 6.0 * (6.0 + 18.0),
        ),
        (
            "Degenerate",
            pi * 8.0_f64.powi(2) * 15.0 / 3.0,
            pi * 8.0 * (8.0 + 17.0),
        ),
        (
            "Compound",
            4.0 * pi * 4.0_f64.powi(3) / 3.0 + 2.0 * pi.powi(2) * 6.0 * 2.0_f64.powi(2),
            4.0 * pi * 4.0_f64.powi(2) + 4.0 * pi.powi(2) * 6.0 * 2.0,
        ),
        (
            "Revolved",
            0.75 * pi * (7.0_f64.powi(2) - 4.0_f64.powi(2)) * 8.0,
            2.0 * 0.75 * pi * (7.0_f64.powi(2) - 4.0_f64.powi(2))
                + 0.75 * 2.0 * pi * (7.0 + 4.0) * 8.0
                + 2.0 * (7.0 - 4.0) * 8.0,
        ),
        (
            "Swept",
            pi * 2.0_f64.powi(2) * 20.0,
            2.0 * pi * 2.0 * (2.0 + 20.0),
        ),
    ] {
        let body = row(name);
        let actual_volume = body
            .volume()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        let actual_area = body
            .area()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (actual_volume - volume).abs() < VOLUME_TOLERANCE,
            "{name}: volume {actual_volume}"
        );
        assert!(
            (actual_area - area).abs() < AREA_TOLERANCE,
            "{name}: area {actual_area}"
        );
    }
    for (name, expected) in [
        ("Sheet", 15.0 * 10.0),
        ("MultiShell", 2.0 * 6.0 * 5.0 * 5.0),
    ] {
        let area = row(name)
            .area()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (area - expected).abs() < AREA_TOLERANCE,
            "{name}: area {area}"
        );
    }
    let length = row("Wire")
        .length()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    assert!((length - 2.0 * 74.0_f64.sqrt()).abs() < LENGTH_TOLERANCE);
}

#[test]
fn authored_uniform_appearance_binds_every_box_edge_and_vertex() {
    use std::collections::BTreeSet;

    use cadmpeg_ir::appearance::AppearanceTarget;
    use cadmpeg_ir::topology::Color;

    const BOX_OBJECT: &str = "Box";
    let bytes =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/uniform_appearance.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;

    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.faces.len(), 6);
    assert_eq!(model.edges.len(), 12);
    assert_eq!(model.vertices.len(), 8);

    let body_color = Color::from_rgba8(51, 102, 153, 255);
    let edge_color = Color::from_rgba8(179, 51, 26, 255);
    let vertex_color = Color::from_rgba8(26, 204, 77, 255);

    let body_target = AppearanceTarget::Body(model.bodies[0].id.clone());
    let body_bindings = model
        .appearance_bindings
        .iter()
        .filter(|binding| matches!(binding.target, AppearanceTarget::Body(_)))
        .collect::<Vec<_>>();
    assert_eq!(body_bindings.len(), 1);
    let body_binding = body_bindings[0];
    assert_eq!(body_binding.target, body_target);
    let body_appearance = model
        .appearances
        .iter()
        .find(|appearance| appearance.id == body_binding.appearance)
        .unwrap();
    assert_eq!(body_appearance.base_color, Some(body_color));
    assert_eq!(model.bodies[0].color, Some(body_color));

    let edge_targets = model
        .edges
        .iter()
        .map(|edge| AppearanceTarget::Edge(edge.id.clone()))
        .collect::<BTreeSet<_>>();
    let vertex_targets = model
        .vertices
        .iter()
        .map(|vertex| AppearanceTarget::Vertex(vertex.id.clone()))
        .collect::<BTreeSet<_>>();

    let mut bound_edge_targets = BTreeSet::new();
    let mut bound_vertex_targets = BTreeSet::new();
    for binding in &model.appearance_bindings {
        let appearance = model
            .appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
            .expect("every topology appearance binding resolves");
        match &binding.target {
            AppearanceTarget::Edge(id) => {
                assert!(bound_edge_targets.insert(AppearanceTarget::Edge(id.clone())));
                assert_eq!(appearance.base_color, Some(edge_color));
                assert_eq!(
                    appearance
                        .properties
                        .get("line_width")
                        .map(|width| width.get()),
                    Some(3.0)
                );
            }
            AppearanceTarget::Vertex(id) => {
                assert!(bound_vertex_targets.insert(AppearanceTarget::Vertex(id.clone())));
                assert_eq!(appearance.base_color, Some(vertex_color));
                assert_eq!(
                    appearance
                        .properties
                        .get("point_size")
                        .map(|size| size.get()),
                    Some(5.0)
                );
            }
            AppearanceTarget::Body(_) => {}
            _ => {}
        }
    }
    assert_eq!(bound_edge_targets, edge_targets);
    assert_eq!(bound_vertex_targets, vertex_targets);

    let [view] = model.view_presentations.as_slice() else {
        panic!("one authored box view presentation");
    };
    assert_eq!(
        view.object.as_deref(),
        Some(crate::native::native_id("object", BOX_OBJECT).as_str())
    );
    assert_eq!(
        view.line_width
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        Some(3.0)
    );
    assert_eq!(
        view.point_size
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        Some(5.0)
    );
}
